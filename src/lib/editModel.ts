// Non-destructive edit model. Tracks are lists of clips that point into source files.
// Original files are never touched: every edit just produces a new immutable Project value,
// and History keeps those values so undo/redo is cheap and exact.

export interface Source { path: string; name: string; duration: number; sampleRate: number; channels: number; peaks: number[]; }
/** seconds inside the source file; `split` = a deliberate clip boundary before this clip (Split/Join) */
export interface Clip { src: string; start: number; end: number; split?: boolean; }
/** Text label on a label track (t1 > t0 for a range label). */
export interface Label { id: number; t0: number; t1: number; text: string; }
export type SpecScale = "log" | "linear";
export type SpecColors = "classic" | "inferno" | "grey";
import type { FxSlot } from "./fx";
export interface EditTrack { id: number; name: string; kind: string; clips: Clip[]; fx?: FxSlot[]; video?: string; muted?: boolean; solo?: boolean;
  /** kind "labels": a label track (no audio) */ labels?: Label[];
  view?: "wave" | "spectrogram"; specScale?: SpecScale; specColors?: SpecColors; }
export interface Marker { id: number; time: number; label: string; }
export interface Project { tracks: EditTrack[]; markers?: Marker[]; }
/** a <= b, seconds on the timeline. `tracks` = every selected track (multi-track selection), incl. trackId. */
export interface Selection { trackId: number; a: number; b: number; tracks?: number[]; }

const EPS = 1e-9;
export const clipLen = (c: Clip) => c.end - c.start;
export const trackLen = (t: EditTrack) => Math.max(t.clips.reduce((s, c) => s + clipLen(c), 0), ...(t.labels ?? []).map((l) => l.t1));
export const isLabelTrack = (t: EditTrack) => t.kind === "labels";
export const norm = (a: number, b: number): [number, number] => (a <= b ? [a, b] : [b, a]);

/** Split the clip list so that there is a clip boundary at time t. */
function splitAt(clips: Clip[], t: number): Clip[] {
  const out: Clip[] = []; let pos = 0;
  for (const c of clips) {
    const len = clipLen(c);
    if (t > pos + EPS && t < pos + len - EPS) {
      const cut = c.start + (t - pos);
      out.push({ ...c, end: cut }, { ...c, start: cut, split: undefined });
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

const tidy = (c: Clip, first: boolean): Clip => { const { split, ...rest } = c; return split && !first ? { ...rest, split } : rest; };
/** Re-join adjacent clips that are contiguous in the same source (keeps the model tidy after undoable splits). */
function merge(clips: Clip[]): Clip[] {
  const out: Clip[] = [];
  for (const c of clips) {
    if (clipLen(c) < EPS) continue;
    const p = out[out.length - 1];
    if (p && !c.split && p.src === c.src && Math.abs(p.end - c.start) < EPS) out[out.length - 1] = { ...p, end: c.end };
    else out.push(tidy(c, out.length === 0));
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

// ---- multi-track selection ---------------------------------------------------
/** Ids of every selected track, in project order. */
export function selectedIds(p: Project, sel: Selection | null): number[] {
  if (!sel) return [];
  const set = new Set([sel.trackId, ...(sel.tracks ?? [])]);
  return p.tracks.filter((t) => set.has(t.id)).map((t) => t.id);
}
const mapSel = (p: Project, sel: Selection, f: (t: EditTrack) => EditTrack): Project => {
  const ids = new Set(selectedIds(p, sel));
  return { ...p, tracks: p.tracks.map((t) => (ids.has(t.id) ? f(t) : t)) };
};
/** Labels: remove those inside [a,b], cut those overlapping, shift later ones left (keeps them in sync with the audio). */
export function removeLabelRange(ls: Label[], a: number, b: number): Label[] {
  [a, b] = norm(a, b); const d = b - a; if (d < EPS) return ls;
  const at = (t: number) => (t <= a ? t : t >= b ? t - d : a);
  return ls.filter((l) => (l.t1 - l.t0 < EPS ? !(l.t0 > a + EPS && l.t0 < b - EPS) : !(l.t0 >= a - EPS && l.t1 <= b + EPS)))
    .map((l) => ({ ...l, t0: at(l.t0), t1: at(l.t1) }));
}
/** Make room for d seconds inserted at `at`: later labels move right, labels spanning `at` grow. */
export function shiftLabels(ls: Label[], at: number, d: number): Label[] {
  return ls.map((l) => {
    const after = l.t0 >= at - EPS;
    return { ...l, t0: after ? l.t0 + d : l.t0, t1: after || l.t1 > at + EPS ? l.t1 + d : l.t1 };
  });
}
const removeAny = (t: EditTrack, a: number, b: number): EditTrack =>
  isLabelTrack(t) ? { ...t, labels: removeLabelRange(t.labels ?? [], a, b) } : removeRange(t, a, b);

/** Cut on every selected track. The clipboard has one entry per selected audio track. */
export function cutMany(p: Project, sel: Selection): { project: Project; clipboard: Clip[][] } {
  return { project: mapSel(p, sel, (t) => removeAny(t, sel.a, sel.b)), clipboard: copyMany(p, sel) };
}
export function copyMany(p: Project, sel: Selection): Clip[][] {
  const ids = new Set(selectedIds(p, sel));
  return p.tracks.filter((t) => ids.has(t.id) && !isLabelTrack(t)).map((t) => extract(t, sel.a, sel.b));
}
export function delMany(p: Project, sel: Selection): Project { return mapSel(p, sel, (t) => removeAny(t, sel.a, sel.b)); }
/** Paste clipboard entry i into the i-th selected audio track (one entry goes into every selected track). Label tracks make room. */
export function pasteMany(p: Project, sel: Selection, clip: Clip[][]): Project {
  if (!clip.length) return p;
  const at = Math.min(sel.a, sel.b); let i = 0;
  const len = Math.max(...clip.map((cs) => cs.reduce((s, c) => s + clipLen(c), 0)));
  return mapSel(p, sel, (t) => {
    if (isLabelTrack(t)) return { ...t, labels: shiftLabels(removeLabelRange(t.labels ?? [], sel.a, sel.b), at, len) };
    const cs = clip.length === 1 ? clip[0] : clip[i++]; if (!cs) return t;
    return insertAt(removeRange(t, sel.a, sel.b), at, cs);
  });
}

/** Replace [a,b] on the given tracks with new clips (e.g. a rendered effect), kept apart as their own clip. */
export function replaceRanges(p: Project, a: number, b: number, byTrack: Map<number, Clip[]>): Project {
  [a, b] = norm(a, b);
  return { ...p, tracks: p.tracks.map((t) => {
    const cs = byTrack.get(t.id); if (!cs || isLabelTrack(t)) return t;
    const len = cs.reduce((s, c) => s + clipLen(c), 0);
    const ins = cs.map((c, i) => (i === 0 ? { ...c, split: true } : { ...c }));
    let r = insertAt(removeRange(t, a, b), a, ins);
    r = splitTrackAt(r, a + len); return r;
  }) };
}
/** Trim: keep only [a,b] on every selected track (labels too, moved to start at 0). */
export function trimMany(p: Project, sel: Selection): Project {
  const [a, b] = norm(sel.a, sel.b); if (b - a < EPS) return p;
  return mapSel(p, sel, (t) => isLabelTrack(t)
    ? { ...t, labels: (t.labels ?? []).filter((l) => l.t1 >= a - EPS && l.t0 <= b + EPS).map((l) => ({ ...l, t0: Math.max(l.t0, a) - a, t1: Math.min(l.t1, b) - a })) }
    : { ...t, clips: merge(extract(t, a, b)) });
}
/** The clip under time t on a track, as a timeline range. */
export function clipAt(t: EditTrack, at: number): { a: number; b: number } | null {
  let pos = 0;
  for (const c of t.clips) { const l = clipLen(c); if (at >= pos - EPS && at <= pos + l + EPS) return { a: pos, b: pos + l }; pos += l; }
  return null;
}

// ---- split / join clips ---------------------------------------------------------
/** Put a deliberate clip boundary at time t (no-op at the very start or end). */
export function splitTrackAt(t: EditTrack, at: number): EditTrack {
  if (isLabelTrack(t) || at <= EPS || at >= trackLen(t) - EPS) return t;
  const s = splitAt(t.clips, at); let pos = 0;
  return { ...t, clips: merge(s.map((c) => { const r = Math.abs(pos - at) < 1e-6 ? { ...c, split: true } : c; pos += clipLen(c); return r; })) };
}
/** Split every selected track at the cursor, or at both edges of a range. */
export function splitClips(p: Project, sel: Selection): Project {
  const ts = sel.b - sel.a > EPS ? [sel.a, sel.b] : [sel.a];
  return mapSel(p, sel, (t) => ts.reduce(splitTrackAt, t));
}
/** Join: remove the clip boundaries strictly inside [a,b]; pieces of the same source merge back into one clip. */
export function joinTrack(t: EditTrack, a: number, b: number): EditTrack {
  if (isLabelTrack(t)) return t;
  let pos = 0;
  return { ...t, clips: merge(t.clips.map((c) => { const r = pos > a + EPS && pos < b - EPS && c.split ? { ...c, split: undefined } : c; pos += clipLen(c); return r; })) };
}
export function joinClips(p: Project, sel: Selection): Project { return mapSel(p, sel, (t) => joinTrack(t, sel.a, sel.b)); }
/** Clip start positions on the track timeline (excluding 0), with whether each is a deliberate split. */
export function clipBounds(t: EditTrack): { t: number; split: boolean }[] {
  const out: { t: number; split: boolean }[] = []; let pos = 0;
  t.clips.forEach((c, i) => { if (i) out.push({ t: pos, split: !!c.split }); pos += clipLen(c); });
  return out;
}

// ---- label tracks -------------------------------------------------------------
export function setLabels(p: Project, trackId: number, f: (ls: Label[]) => Label[]): Project {
  return mapTrack(p, trackId, (t) => ({ ...t, labels: f(t.labels ?? []).slice().sort((x, y) => x.t0 - y.t0 || x.id - y.id) }));
}
const fmt6 = (x: number) => x.toFixed(6);
/** Audacity-compatible label text: "start<TAB>end<TAB>text" per line. */
export function labelsToText(ls: Label[]): string {
  return ls.map((l) => `${fmt6(l.t0)}\t${fmt6(l.t1)}\t${l.text.replace(/[\r\n\t]+/g, " ")}`).join("\n") + (ls.length ? "\n" : "");
}
export function textToLabels(s: string, newId: () => number): Label[] {
  return s.split(/\r?\n/).map((line) => line.split("\t")).filter((f) => f.length >= 2 && !isNaN(+f[0]))
    .map((f) => { const t0 = +f[0], t1 = isNaN(+f[1]) ? t0 : +f[1]; return { id: newId(), t0, t1: Math.max(t0, t1), text: f.slice(2).join(" ") }; });
}
/** Copy labels to project markers (for chapters in exports / Send back to video). Skips ones already there. */
export function labelsToMarkers(p: Project, ls: Label[], newId: () => number): Project {
  const have = new Set((p.markers ?? []).map((m) => `${m.time.toFixed(3)}|${m.label}`));
  const add = ls.filter((l) => !have.has(`${l.t0.toFixed(3)}|${l.text || "Label"}`)).map((l) => ({ id: newId(), time: l.t0, label: l.text || "Label" }));
  return { ...p, markers: [...(p.markers ?? []), ...add].sort((a, b) => a.time - b.time) };
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
