// Turns the result of the backend's read-only Audacity (.aup3) import into StreamWave tracks and markers.
import type { EditTrack, Marker } from "./editModel";
import { slot } from "./fx";

export interface Aup3Clip { path: string; start: number; end: number; }
export interface Aup3Track { name: string; clips: Aup3Clip[]; gain_db: number | null; muted: boolean; solo: boolean; channels: number; clip_count: number; }
export interface Aup3Label { t0: number; t1: number; title: string; track: string; }
export interface Aup3Result { name: string; media_dir: string; tracks: Aup3Track[]; labels: Aup3Label[]; notes: string[]; from_autosave: boolean; }

export const isAup3 = (p: string) => /\.aup3$/i.test(p);
export const isProjectFile = (p: string) => /\.twt$/i.test(p);
export const AUDIO_EXT = ["wav", "flac", "mp3", "ogg", "m4a", "aac", "aif", "aiff", "opus", "mkv", "mp4", "mov", "webm"];
export const isMedia = (p: string) => AUDIO_EXT.includes(p.split(".").pop()!.toLowerCase());

/** Every distinct media file the import produced (each must be loaded as a source). */
export const importedFiles = (r: Aup3Result) => [...new Set(r.tracks.flatMap((t) => t.clips.map((c) => c.path)))];

/**
 * Build StreamWave tracks/markers. `resolve` maps an imported file path to the path the source was
 * registered under (normally the same). Gain becomes a Gain effect; mute/solo are carried as flags.
 */
export function buildFromAup3(r: Aup3Result, resolve: (p: string) => string, newId: () => number): { tracks: EditTrack[]; markers: Marker[] } {
  const tracks = r.tracks.filter((t) => t.clips.length).map((t): EditTrack => ({
    id: newId(), name: t.name || r.name, kind: "audacity",
    clips: t.clips.map((c) => ({ src: resolve(c.path), start: c.start, end: c.end })),
    ...(t.gain_db != null && Math.abs(t.gain_db) > 0.01 ? { fx: [slot({ type: "Gain", db: Math.round(t.gain_db * 10) / 10 })] } : {}),
    ...(t.muted ? { muted: true } : {}), ...(t.solo ? { solo: true } : {}),
  }));
  const markers = [...r.labels].sort((a, b) => a.t0 - b.t0).map((l): Marker => ({
    id: newId(), time: l.t0,
    label: (l.title || "Label") + (l.t1 - l.t0 > 0.0005 ? ` (to ${l.t1.toFixed(2)}s)` : ""),
  }));
  return { tracks, markers };
}

/** One friendly paragraph about what came across and what didn't. */
export function summary(r: Aup3Result): string {
  const n = r.tracks.filter((t) => t.clips.length).length;
  const clips = r.tracks.reduce((s, t) => s + t.clip_count, 0);
  const parts = [`Imported ${n} track${n === 1 ? "" : "s"} (${clips} clip${clips === 1 ? "" : "s"})` + (r.labels.length ? ` and ${r.labels.length} label${r.labels.length === 1 ? "" : "s"} as markers` : "") + "."];
  return parts.concat(r.notes).join("\n");
}
