// "Follow playhead": keeps the play/record head in view by scrolling the timeline smoothly (one scrollLeft write per
// animation frame, no layout reads after writes). Pure logic here; App.svelte owns the rAF loop and the DOM.

/** Where the head sits in the view while following (fraction of the visible width from the left). */
export const ANCHOR = 1 / 3;
/** How fast the anchor glides back to ANCHOR after following resumes with the head further right (per second). */
const GLIDE_PER_S = 0.35;
/** A manual scroll / wheel counts as "the user is scrolling" for this long after the last event (ms). */
export const USER_HOLD_MS = 700;

export interface View { left: number; width: number; max: number } // px; max = scrollWidth - clientWidth

/** Is `headPx` inside the view (with a small margin so a head sitting on the edge counts as outside)? */
export function inView(headPx: number, v: View, margin = 2): boolean {
  return headPx >= v.left + margin && headPx <= v.left + v.width - margin;
}

/**
 * Next scroll position for one animation frame. `anchor` is the head's target fraction (glides to ANCHOR).
 * - head outside the view (start of playback elsewhere, a seek, resume after the head re-entered): page so the
 *   head sits at ANCHOR;
 * - head left of the anchor: don't move (the head walks into place);
 * - otherwise: scroll continuously with the head (smooth, never backwards).
 */
export function followStep(headPx: number, v: View, anchor: number, dt: number): { left: number; anchor: number } {
  const clamp = (x: number) => Math.max(0, Math.min(v.max, x));
  if (v.max <= 0 || v.width <= 0) return { left: 0, anchor };
  if (!inView(headPx, v, 0)) return { left: clamp(headPx - v.width * ANCHOR), anchor: ANCHOR };
  const a = anchor > ANCHOR ? Math.max(ANCHOR, anchor - GLIDE_PER_S * Math.min(dt, 0.1)) : ANCHOR;
  const want = headPx - v.width * a;
  return { left: want > v.left ? clamp(want) : v.left, anchor: a };
}

/**
 * When to follow. Following pauses while the user scrolls or drags; afterwards it resumes on its own if the head is
 * in view, else when the head re-enters the view, or on the next Play.
 */
export class FollowState {
  private pointer = false;
  private lastUser = -1e9;
  private suspended = false;
  /** Anchor to use after a resume (the head's position then, so nothing jumps). */
  resumeAnchor: number | null = null;
  /** Play pressed / playback (re)started: follow again from scratch. */
  play() { this.suspended = false; this.pointer = false; this.lastUser = -1e9; this.resumeAnchor = null; }
  /** Wheel / scrollbar / keyboard scroll by the user. */
  userScroll(now: number) { this.lastUser = now; this.suspended = true; }
  /** Pointer pressed / released inside the timeline (drag-select, clip drag, scrollbar drag). */
  pointerDown() { this.pointer = true; this.suspended = true; }
  pointerUp(now: number) { this.pointer = false; this.lastUser = now; }
  get paused() { return this.suspended; }
  /** Should the loop drive the scroll this frame? `headFrac` = head position in the view (0..1, or outside). */
  active(now: number, headFrac: number): boolean {
    if (!this.suspended) return true;
    if (this.pointer || now - this.lastUser < USER_HOLD_MS) return false;
    if (headFrac >= 0 && headFrac <= 1) { this.suspended = false; this.resumeAnchor = Math.max(ANCHOR, Math.min(0.95, headFrac)); return true; }
    return false; // head is off-screen: wait for it to come back (or for Play)
  }
}

/**
 * Smooth head position between the ~30 Hz engine updates: extrapolate from the last report, never further than
 * `maxAhead` seconds, and never backwards by a hair (only real jumps such as a seek go back).
 */
export class HeadClock {
  private base = 0; private at = 0; private shown = 0; private running = false;
  report(secs: number, now: number, running: boolean) {
    if (Math.abs(secs - this.shown) > 0.25 || !running) this.shown = secs; // seek / pause / stop: snap
    this.base = secs; this.at = now; this.running = running;
  }
  now(now: number, maxAhead = 0.12): number {
    const t = this.running ? this.base + Math.min(maxAhead, Math.max(0, (now - this.at) / 1000)) : this.base;
    this.shown = t >= this.shown || this.shown - t > 0.25 ? t : this.shown;
    return this.shown;
  }
}
