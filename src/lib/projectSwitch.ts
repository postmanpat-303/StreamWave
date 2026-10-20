// Replacing the project (New / Open / Close / Recover / double-clicked .twt): the one place that decides the order,
// so the old project's audio can never keep playing under the new one.
export interface SwitchDeps {
  /** A recording is running. */
  recording: () => boolean;
  /** Ask the user (native dialog). */
  confirm: (msg: string) => Promise<boolean>;
  /** Stop the recording and wait until the take has landed in the CURRENT project (record-finished/error). */
  stopRecording: () => Promise<void>;
  /** Unsaved changes (checked after the take landed, so the prompt covers it). */
  dirty: () => boolean;
  /** Stop playback with the de-click fade, reset transport/playhead, free the old project's audio. */
  releaseAudio: () => Promise<void>;
}

/**
 * Run the guards before `replace()` loads the new project:
 *   1. a running recording: ask, then stop it and let the take land in the old project;
 *   2. unsaved changes: ask "Discard unsaved changes?";
 *   3. `pick` (e.g. the Open dialog) may still cancel; nothing has been stopped yet if it does;
 *   4. stop playback / free audio, then replace.
 * Returns false if anything was cancelled (then playback and the project are untouched).
 */
export async function switchProject<T>(d: SwitchDeps, pick: () => Promise<T | null>, replace: (picked: T) => Promise<void>): Promise<boolean> {
  if (d.recording()) {
    if (!(await d.confirm("A recording is running. Stop recording first? The take is saved to your recordings folder and added to this project before it closes."))) return false;
    await d.stopRecording();
  }
  if (d.dirty() && !(await d.confirm("Discard unsaved changes?"))) return false;
  const picked = await pick();
  if (picked === null) return false;
  try { await d.releaseAudio(); } catch { /* engine already stopped: carry on, the new project must still load */ }
  await replace(picked);
  return true;
}
