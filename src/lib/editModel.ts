// Non-destructive edit model. Tracks are lists of clips that point into source files.
// Original files are never touched: every edit just produces a new immutable Project value,
// and History keeps those values so undo/redo is cheap and exact.

export interface Source { path: string; name: string; duration: number; sampleRate: number; channels: number; peaks: number[]; }
/** seconds inside the source file; `split` = a deliberate clip boundary before this clip (Split/Join).
 * `gap` = silence (src ""), used to position clips freely on the timeline (0.10.0).
 * `fx` = the clip's own ordered effect stack ("Layering Effects"), owned by this clip instance. */
export interface Clip { src: string; start: number; end: number; split?: boolean; gain_db?: number; fade_in_ms?: number; fade_out_ms?: number; gap?: boolean; fx?: FxSlot[];
  /** shape of this clip's own fades (default: the S-curve every older version used) */ fade_curve?: FadeCurve;
  /** crossfade with the previous clip, centred on the cut (format v3) */ xfade_ms?: number; xfade_curve?: XfadeCurve;
  /** signature of the clip it crossfades from; an edit that changes that neighbour drops the crossfade */ xfade_from?: string; }
export type FadeCurve = "s_curve" | "linear" | "equal_power";
export type XfadeCurve = "linear" | "equal_power";
export type ClipProps = { gain_db?: number; fade_in_ms?: number; fade_out_ms?: number; fade_curve?: FadeCurve };
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
/** Overlap tolerance for clip placement (well below one sample at 192 kHz). */
const POS_EPS = 1e-7;
export const clipLen = (c: Clip) => c.end - c.start;
export const GAP_SRC = "";
export const isGap = (c: Clip) => !!c.gap;
/** Silence of `len` seconds. */
export const gap = (len: number): Clip => ({ src: GAP_SRC, start: 0, end: len, gap: true });
/** Round a timeline position to a whole sample at `rate` (no-op without a rate). */
export const snapToSamples = (t: number, rate: number) => (rate > 0 ? Math.round(t * rate) / rate : t);
/** Independent copy of a clip (its effect stack is copied too, so editing one never changes the other). */
export const cloneClip = (c: Clip): Clip => ({ ...c, ...(c.fx ? { fx: c.fx.map((s) => ({ ...s, fx: structuredClone(s.fx) })) } : {}) });
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
      out.push({ ...c, end: cut }, stripXfade({ ...c, start: cut, split: undefined })); // a crossfade belongs to the clip's head only
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
  return { ...t, clips: merge([...before, ...clips.map(cloneClip), ...after]) };
}

const tidy = (c: Clip, first: boolean): Clip => {
  if (c.gap) return gap(clipLen(c));
  const { split, ...rest } = c; if (rest.fx && !rest.fx.length) delete rest.fx;
  return split && !first ? { ...rest, split } : rest;
};
const sameFx = (a: Clip, b: Clip) => JSON.stringify(a.fx ?? []) === JSON.stringify(b.fx ?? []);
/** Re-join adjacent clips that are contiguous in the same source with identical gain/fades/effects
 * (keeps the model tidy after undoable splits; clips with different props or deliberate split boundaries stay separate).
 * Neighbouring gaps become one gap and trailing gaps are dropped, so a track ends where its last audio ends. */
function merge(clips: Clip[]): Clip[] {
  const sameProps = (a: Clip, b: Clip) =>
    (a.gain_db ?? 0) === (b.gain_db ?? 0) && (a.fade_in_ms ?? 0) === (b.fade_in_ms ?? 0) && (a.fade_out_ms ?? 0) === (b.fade_out_ms ?? 0) &&
    (a.fade_curve ?? "s_curve") === (b.fade_curve ?? "s_curve") && sameFx(a, b);
  const out: Clip[] = [];
  for (const c of clips) {
    if (clipLen(c) < EPS) continue;
    const p = out[out.length - 1];
    if (p && p.gap && c.gap) out[out.length - 1] = gap(clipLen(p) + clipLen(c));
    else if (p && !c.split && !c.xfade_ms && !c.gap && !p.gap && p.src === c.src && Math.abs(p.end - c.start) < EPS && sameProps(p, c)) out[out.length - 1] = { ...p, end: c.end };
    else out.push(tidy(c, out.length === 0));
  }
  while (out.length && out[out.length - 1].gap) out.pop();
  // a crossfade stays only while its clip still follows the same neighbour (cut/paste/move/trim drop it)
  for (let i = 0; i < out.length; i++) {
    const c = out[i]; if (c.xfade_ms === undefined) continue;
    const p = out[i - 1]; if (!p || p.gap || c.gap || xfadeSig(p) !== c.xfade_from) out[i] = stripXfade(c);
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

/** Remove many regions in one edit (e.g. auto-trim silences). Regions are [start,end] track-time seconds. */
export function removeRegions(p: Project, trackId: number, regions: [number, number][]): Project {
    if (!regions.length) return p;
    return mapTrack(p, trackId, (t) => {
        let clips = t.clips;
        // remove back-to-front so earlier positions stay valid
        for (const [a, b] of [...regions].sort((x, y) => y[0] - x[0])) clips = removeRange({ ...t, clips }, a, b).clips;
        return { ...t, clips };
    });
}
/** Paste replaces the selection (if any) and inserts at its start. */
export function paste(p: Project, sel: Selection, clips: Clip[]): Project {
  return mapTrack(p, sel.trackId, (t) => insertAt(removeRange(t, sel.a, sel.b), Math.min(sel.a, sel.b), clips));
}

/** Set gain/fades on every clip overlapping the selection (single undo step). */
export function applyClipProps(p: Project, sel: Selection, props: ClipProps): Project {
  const t = p.tracks.find((x) => x.id === sel.trackId); if (!t) return p;
  return mapTrack(p, sel.trackId, (t) => {
    const { before, inside, after } = partition(t.clips, sel.a, sel.b);
    if (!inside.length) return t;
    return { ...t, clips: merge([...before, ...inside.map((c) => (c.gap ? c : { ...c, ...props })), ...after]) };
  });
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
/** The audio clip under time t on a track, as a timeline range (gaps are not clips). */
export function clipAt(t: EditTrack, at: number): { a: number; b: number } | null {
  const p = audioClipAt(t, at); return p ? { a: p.a, b: p.b } : null;
}

// ---- positioned clips, moving clips, multi-file import (0.10.0) ----------------
/** A clip with its timeline position. */
export interface Placed { clip: Clip; index: number; a: number; b: number; }
export function placed(t: EditTrack): Placed[] {
  let pos = 0;
  return t.clips.map((clip, index) => { const a = pos; pos += clipLen(clip); return { clip, index, a, b: pos }; });
}
/** Audio clips only (no gaps), with positions. */
export const audioClips = (t: EditTrack) => placed(t).filter((p) => !p.clip.gap);
export function audioClipAt(t: EditTrack, at: number): Placed | null {
  return audioClips(t).find((p) => at >= p.a - EPS && at <= p.b + EPS) ?? null;
}
/** Would a clip of `len` seconds at `at` overlap existing audio on the track? */
export function canPlace(t: EditTrack, at: number, len: number): boolean {
  return !isLabelTrack(t) && at >= 0 && !audioClips(t).some((q) => q.a < at + len - POS_EPS && q.b > at + POS_EPS);
}
/** Put clip c at `at` into silence (a gap) or after the end of the track. null if it would overlap audio. */
export function placeClip(t: EditTrack, at: number, c: Clip): EditTrack | null {
  const len = clipLen(c), L = trackLen(t);
  if (!canPlace(t, at, len)) return null;
  const ins: Clip = { ...c, split: true }; // keep it its own clip where it lands
  if (at >= L - POS_EPS) {
    const g = at - L;
    return { ...t, clips: merge([...t.clips, ...(g > EPS ? [gap(g)] : []), ins]) };
  }
  // the whole region [at, at+len] is silence inside the track: replace that silence with the clip
  const { before, after } = partition(t.clips, at, at + len);
  return { ...t, clips: merge([...before, ins, ...after]) };
}
export type MoveResult = { ok: true; project: Project; trackId: number; at: number } | { ok: false; reason: "overlap" | "invalid" };
/**
 * Move audio clip `index` of track `fromId` to time `at` on track `toId` (metadata only: source, trim, gain,
 * fades and the clip's effects travel with it; nothing else on either track shifts). `at` is rounded to a whole
 * sample at `rate` (the target track's render rate). Refuses to overlap existing audio.
 */
export function moveClip(p: Project, fromId: number, index: number, toId: number, at: number, rate: number): MoveResult {
  const from = p.tracks.find((t) => t.id === fromId), to = p.tracks.find((t) => t.id === toId);
  if (!from || !to || isLabelTrack(from) || isLabelTrack(to)) return { ok: false, reason: "invalid" };
  const c = from.clips[index]; if (!c || c.gap) return { ok: false, reason: "invalid" };
  at = snapToSamples(Math.max(0, at), rate);
  const lifted: EditTrack = { ...from, clips: merge(from.clips.map((x, i) => (i === index ? gap(clipLen(c)) : x))) };
  const target = placeClip(fromId === toId ? lifted : to, at, c);
  if (!target) return { ok: false, reason: "overlap" };
  return { ok: true, trackId: toId, at, project: { ...p, tracks: p.tracks.map((t) => (t.id === toId ? target : t.id === fromId ? lifted : t)) } };
}
/** Pick the closest snap point for a clip [start, start+len): either edge may snap. */
export function snapClip(start: number, len: number, points: number[], tol: number): { t: number; to: number | null } {
  let best: { t: number; to: number | null; d: number } = { t: start, to: null, d: tol };
  for (const q of points) {
    for (const [cand, d] of [[q, Math.abs(start - q)], [q - len, Math.abs(start + len - q)]] as const) {
      if (d <= best.d && cand >= 0) best = { t: cand, to: q, d };
    }
  }
  return { t: best.t, to: best.to };
}
export interface ImportFile { path: string; name: string; duration: number; sampleRate: number; }
export type ImportMode = "sequential" | "tracks";
/**
 * Add several imported files in one edit. "sequential": one new track, files back to back in the given order
 * from `start` (each clip starts exactly where the previous ends; no gaps, no overlaps). "tracks": one new track
 * per file, all starting at `start`, named after the file. Clips reference the files directly (no copies).
 */
export function arrangeImport(p: Project, files: ImportFile[], mode: ImportMode, start: number, newId: () => number, kind = "import"): { project: Project; trackIds: number[] } {
  files = files.filter((f) => f.duration > 0);
  if (!files.length) return { project: p, trackIds: [] };
  const lead = (rate: number): Clip[] => { const s = snapToSamples(Math.max(0, start), rate); return s > EPS ? [gap(s)] : []; };
  const clip = (f: ImportFile, split: boolean): Clip => ({ src: f.path, start: 0, end: f.duration, ...(split ? { split: true } : {}) });
  const tracks: EditTrack[] = mode === "sequential"
    ? [{ id: newId(), name: files.length > 1 ? `${files[0].name} + ${files.length - 1} more` : files[0].name, kind,
        clips: merge([...lead(files[0].sampleRate), ...files.map((f, i) => clip(f, i > 0))]) }]
    : files.map((f) => ({ id: newId(), name: f.name, kind, clips: merge([...lead(f.sampleRate), clip(f, false)]) }));
  return { project: { ...p, tracks: [...p.tracks, ...tracks] }, trackIds: tracks.map((t) => t.id) };
}
/** Replace the effect stack of one clip (parameters only; Layering Effects). */
export function setClipFx(p: Project, trackId: number, index: number, fx: FxSlot[]): Project {
  return mapTrack(p, trackId, (t) => {
    const c = t.clips[index]; if (!c || c.gap) return t;
    const clips = [...t.clips]; const n: Clip = { ...c, fx }; if (!fx.length) delete n.fx; clips[index] = n;
    return { ...t, clips };
  });
}
// ---- crossfades -------------------------------------------------------------------
export const xfadeSig = (c: Clip) => `${c.src}|${c.end.toFixed(6)}`;
export function stripXfade(c: Clip): Clip { if (c.xfade_ms === undefined && c.xfade_curve === undefined && c.xfade_from === undefined) return c; const { xfade_ms, xfade_curve, xfade_from, ...r } = c; return r; }
/** Crossfade length that will actually be heard at the head of clip i (same clamping as the renderer): limited by the
 *  source audio beyond the previous clip's end and before this clip's start, and by either clip's length. */
export function xfadeLen(t: EditTrack, i: number, srcDur: (src: string) => number | undefined): number {
  const c = t.clips[i], p = t.clips[i - 1];
  if (!c || !p || c.gap || p.gap || !c.xfade_ms) return 0;
  const tail = Math.max(0, (srcDur(p.src) ?? p.end) - p.end), head = Math.max(0, c.start);
  return Math.max(0, Math.min(c.xfade_ms, 2000 * tail, 2000 * head, 1000 * clipLen(c), 1000 * clipLen(p)));
}
/** Can clip i get a crossfade (both it and the clip before it are audio)? */
export const canCrossfade = (t: EditTrack, i: number) => i > 0 && !!t.clips[i] && !t.clips[i].gap && !t.clips[i - 1].gap;
/**
 * Set (ms > 0) or remove (ms = 0) the crossfade between clip `index` and the clip before it. The crossfade is centred
 * on the cut and plays the sources' spare audio ("handles") on both sides, so nothing moves. Where a source has no
 * spare audio there (e.g. two whole files back to back), the clips are trimmed by the missing amount so they truly
 * overlap; everything after the cut then moves left by `trimmed` seconds. The length is limited to what both clips allow.
 */
export function setCrossfade(p: Project, trackId: number, index: number, ms: number, curve: XfadeCurve, srcDur: (src: string) => number | undefined):
  { project: Project; ms: number; trimmed: number } | null {
  const t = p.tracks.find((x) => x.id === trackId); if (!t || isLabelTrack(t) || !canCrossfade(t, index)) return null;
  const prev = t.clips[index - 1], c = t.clips[index], clips = [...t.clips];
  if (!(ms > 0)) { clips[index] = stripXfade(c); return { project: mapTrack(p, trackId, (x) => ({ ...x, clips: merge(clips) })), ms: 0, trimmed: 0 }; }
  const tail = Math.max(0, (srcDur(prev.src) ?? prev.end) - prev.end), head = Math.max(0, c.start);
  const plan = (m: number) => { const h = m / 2000, nt = Math.max(0, h - tail), nh = Math.max(0, h - head);
    return { nt, nh, ok: m / 1000 <= clipLen(prev) - nt + 1e-9 && m / 1000 <= clipLen(c) - nh + 1e-9 }; };
  let m = Math.round(ms); while (m > 1 && !plan(m).ok) m = Math.floor(m * 0.97);
  const { nt, nh, ok } = plan(m); if (!ok || m < 1) return null;
  const np: Clip = { ...prev, end: prev.end - nt };
  clips[index - 1] = np;
  clips[index] = { ...c, start: c.start + nh, xfade_ms: m, xfade_curve: curve, xfade_from: xfadeSig(np) };
  // the clip after `index` keeps its own crossfade: its signature uses this clip's end, which did not change
  return { project: mapTrack(p, trackId, (x) => ({ ...x, clips: merge(clips) })), ms: m, trimmed: nt + nh };
}
/** Fade-in gain at x in [0,1] for a curve (fade-out = gain(1 - x)); identical to the renderer's curves. */
export function curveGain(curve: FadeCurve | XfadeCurve | undefined, x: number): number {
  x = Math.max(0, Math.min(1, x));
  return curve === "linear" ? x : curve === "equal_power" ? Math.sin((x * Math.PI) / 2) : 0.5 * (1 - Math.cos(Math.PI * x));
}
/** Set one clip's own fade-in or fade-out (ms) by index; fades never overlap each other inside the clip. */
export function setClipFade(p: Project, trackId: number, index: number, edge: "in" | "out", ms: number): Project {
  return mapTrack(p, trackId, (t) => {
    const c = t.clips[index]; if (!c || c.gap) return t;
    const other = edge === "in" ? c.fade_out_ms ?? 0 : c.fade_in_ms ?? 0;
    const v = Math.round(Math.max(0, Math.min(ms, clipLen(c) * 1000 - other)));
    const n: Clip = { ...c }; if (edge === "in") { if (v) n.fade_in_ms = v; else delete n.fade_in_ms; } else { if (v) n.fade_out_ms = v; else delete n.fade_out_ms; }
    const clips = [...t.clips]; clips[index] = n; return { ...t, clips };
  });
}
/** Does the project use crossfades or non-default fade shapes (format v3)? */
export const usesCrossfades = (p: Project) => p.tracks.some((t) => t.clips.some((c) => (c.xfade_ms ?? 0) > 0 || (c.fade_curve && c.fade_curve !== "s_curve")));

/** Does the project use 0.10.0-only clip data (gaps or clip effects)? */
export const usesClipFeatures = (p: Project) => p.tracks.some((t) => t.clips.some((c) => c.gap || (c.fx && c.fx.length)));

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
