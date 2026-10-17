// Non-destructive edit model. Tracks are lists of clips that point into source files.
// Original files are never touched: every edit just produces a new immutable Project value,
// and History keeps those values so undo/redo is cheap and exact.

export interface Source { path: string; name: string; duration: number; sampleRate: number; channels: number; peaks: number[]; }
export interface Clip { src: string; start: number; end: number; } // seconds inside the source file
import type { FxSlot } from "./fx";
export interface EditTrack { id: number; name: string; kind: string; clips: Clip[]; fx?: FxSlot[]; video?: string; }
export interface Marker { id: number; time: number; label: string; }
export interface Project { tracks: EditTrack[]; markers?: Marker[]; }
export interface Selection { trackId: number; a: number; b: number; } // a <= b, seconds on the track timeline

const EPS = 1e-9;
export const clipLen = (c: Clip) => c.end - c.start;
export const trackLen = (t: EditTrack) => t.clips.reduce((s, c) => s + clipLen(c), 0);
export const norm = (a: number, b: number): [number, number] => (a <= b ? [a, b] : [b, a]);

/** Split the clip list so that there is a clip boundary at time t. */
function splitAt(clips: Clip[], t: number): Clip[] {
  const out: Clip[] = []; let pos = 0;
  for (const c of clips) {
    const len = clipLen(c);
    if (t > pos + EPS && t < pos + len - EPS) {
      const cut = c.start + (t - pos);
      out.push({ ...c, end: cut }, { ...c, start: cut });
    } else out.push(c);
    pos += len;
  }
  return out;
}

/** Clips fully inside [a,b] (after splitting) and the remaining clips before/after. */
function partition(clips: Clip[], a: number, b: number) {
  const s = splitAt(splitAt(clips, a), b);
  const before: Clip[] = [], inside: Clip[] = [], after: Clip[] = []; let pos = 0;
  for (const c of s) {
    const mid = pos + clipLen(c) / 2;
    (mid < a ? before : mid > b ? after : inside).push(c);
    pos += clipLen(c);
  }
  return { before, inside, after };
}

export function extract(t: EditTrack, a: number, b: number): Clip[] {
  [a, b] = norm(a, b);
  return partition(t.clips, a, b).inside.map((c) => ({ ...c }));
}

export function removeRange(t: EditTrack, a: number, b: number): EditTrack {
  [a, b] = norm(a, b);
  if (b - a < EPS) return t;
  const { before, after } = partition(t.clips, a, b);
  return { ...t, clips: merge([...before, ...after]) };
}

export function insertAt(t: EditTrack, at: number, clips: Clip[]): EditTrack {
  at = Math.max(0, Math.min(at, trackLen(t)));
  const { before, after } = partition(t.clips, at, at);
  return { ...t, clips: merge([...before, ...clips.map((c) => ({ ...c })), ...after]) };
}

/** Re-join adjacent clips that are contiguous in the same source (keeps the model tidy after undoable splits). */
function merge(clips: Clip[]): Clip[] {
  const out: Clip[] = [];
  for (const c of clips) {
    if (clipLen(c) < EPS) continue;
    const p = out[out.length - 1];
    if (p && p.src === c.src && Math.abs(p.end - c.start) < EPS) out[out.length - 1] = { ...p, end: c.end };
    else out.push({ ...c });
  }
  return out;
}

const mapTrack = (p: Project, id: number, f: (t: EditTrack) => EditTrack): Project => ({ ...p, tracks: p.tracks.map((t) => (t.id === id ? f(t) : t)) });

// ---- commands -------------------------------------------------------------
export function cut(p: Project, sel: Selection): { project: Project; clipboard: Clip[] } {
  const t = p.tracks.find((x) => x.id === sel.trackId); if (!t) return { project: p, clipboard: [] };
  return { project: mapTrack(p, sel.trackId, (t) => removeRange(t, sel.a, sel.b)), clipboard: extract(t, sel.a, sel.b) };
}
export function copy(p: Project, sel: Selection): Clip[] {
  const t = p.tracks.find((x) => x.id === sel.trackId); return t ? extract(t, sel.a, sel.b) : [];
}
export function del(p: Project, sel: Selection): Project { return mapTrack(p, sel.trackId, (t) => removeRange(t, sel.a, sel.b)); }
/** Paste replaces the selection (if any) and inserts at its start. */
export function paste(p: Project, sel: Selection, clips: Clip[]): Project {
  return mapTrack(p, sel.trackId, (t) => insertAt(removeRange(t, sel.a, sel.b), Math.min(sel.a, sel.b), clips));
}

/** Replace a track's effect chain (parameters only; audio is never baked). */
export function setFx(p: Project, trackId: number, fx: FxSlot[]): Project { return mapTrack(p, trackId, (t) => ({ ...t, fx })); }
export function moveFx(fx: FxSlot[], i: number, dir: -1 | 1): FxSlot[] {
  const j = i + dir; if (j < 0 || j >= fx.length) return fx;
  const c = [...fx]; [c[i], c[j]] = [c[j], c[i]]; return c;
}

// ---- markers (timeline positions, seconds) ----
export function addMarker(p: Project, m: Marker): Project {
  return { ...p, markers: [...(p.markers ?? []), m].sort((a, b) => a.time - b.time) };
}
export function removeMarker(p: Project, id: number): Project { return { ...p, markers: (p.markers ?? []).filter((m) => m.id !== id) }; }
export function nextMarker(p: Project, t: number, dir: 1 | -1): Marker | undefined {
  const ms = p.markers ?? [];
  return dir > 0 ? ms.find((m) => m.time > t + 1e-3) : [...ms].reverse().find((m) => m.time < t - 1e-3);
}

// ---- history --------------------------------------------------------------
export class History {
  private past: { label: string; p: Project }[] = [];
  private future: { label: string; p: Project }[] = [];
  constructor(public present: Project, private limit = 500) {}
  apply(label: string, next: Project) {
    if (next === this.present) return;
    this.past.push({ label, p: this.present }); if (this.past.length > this.limit) this.past.shift();
    this.present = next; this.future = [];
  }
  /** Start over with a fresh history (used when opening a project). */
  reset(p: Project) { this.past = []; this.future = []; this.present = p; }
  get canUndo() { return this.past.length > 0; }
  get canRedo() { return this.future.length > 0; }
  get undoLabel() { return this.past[this.past.length - 1]?.label; }
  get redoLabel() { return this.future[this.future.length - 1]?.label; }
  undo() { const e = this.past.pop(); if (!e) return false; this.future.push({ label: e.label, p: this.present }); this.present = e.p; return true; }
  redo() { const e = this.future.pop(); if (!e) return false; this.past.push({ label: e.label, p: this.present }); this.present = e.p; return true; }
}

// ---- display --------------------------------------------------------------
/** Build min/max peaks for a track by looking up each bucket in its source's peak summary. */
export function trackPeaks(t: EditTrack, sources: Map<string, Source>, buckets: number): number[] {
  const len = trackLen(t); const out: number[] = new Array(buckets * 2).fill(0);
  if (len <= 0) return out;
  let ci = 0, pos = 0;
  for (let b = 0; b < buckets; b++) {
    const t0 = (b / buckets) * len, t1 = ((b + 1) / buckets) * len;
    let mn = 0, mx = 0;
    // walk clips overlapping [t0,t1)
    while (ci < t.clips.length && pos + clipLen(t.clips[ci]) <= t0) { pos += clipLen(t.clips[ci]); ci++; }
    let j = ci, jp = pos;
    while (j < t.clips.length && jp < t1) {
      const c = t.clips[j], s = sources.get(c.src);
      if (s && s.duration > 0) {
        const n = s.peaks.length / 2;
        const sa = c.start + Math.max(0, t0 - jp), sb = c.start + Math.min(clipLen(c), t1 - jp);
        const ia = Math.floor((sa / s.duration) * n), ib = Math.max(ia + 1, Math.ceil((sb / s.duration) * n));
        for (let i = ia; i < ib && i < n; i++) { mn = Math.min(mn, s.peaks[i * 2]); mx = Math.max(mx, s.peaks[i * 2 + 1]); }
      }
      jp += clipLen(c); j++;
    }
    out[b * 2] = mn; out[b * 2 + 1] = mx;
  }
  return out;
}
