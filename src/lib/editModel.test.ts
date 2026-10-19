import { describe, it, expect } from "vitest";
import { addMarker, removeMarker, nextMarker, setFx, moveFx, cut, copy, del, paste, removeRegions, applyClipProps, History, trackLen, extract, trackPeaks, type Project, type Source } from "./editModel";

const P = (): Project => ({ tracks: [{ id: 1, name: "t", kind: "mic", clips: [{ src: "a.wav", start: 0, end: 10 }] }] });
const T = (p: Project) => p.tracks[0];

describe("edit model", () => {
  it("cut removes the range and returns it", () => {
    const { project, clipboard } = cut(P(), { trackId: 1, a: 2, b: 5 });
    expect(trackLen(T(project))).toBeCloseTo(7);
    expect(T(project).clips).toEqual([{ src: "a.wav", start: 0, end: 2 }, { src: "a.wav", start: 5, end: 10 }]);
    expect(clipboard).toEqual([{ src: "a.wav", start: 2, end: 5 }]);
  });
  it("reversed selection behaves the same", () => {
    expect(copy(P(), { trackId: 1, a: 5, b: 2 })).toEqual([{ src: "a.wav", start: 2, end: 5 }]);
  });
  it("cut then paste at the same spot restores the original (clips merge back)", () => {
    const { project, clipboard } = cut(P(), { trackId: 1, a: 2, b: 5 });
    const back = paste(project, { trackId: 1, a: 2, b: 2 }, clipboard);
    expect(T(back).clips).toEqual(T(P()).clips);
  });
  it("removeRegions trims many silences in one edit and merges the rest", () => {
    const p = removeRegions(P(), 1, [[2, 3], [5, 6]]);
    expect(trackLen(T(p))).toBeCloseTo(8);
    expect(T(p).clips).toEqual([{ src: "a.wav", start: 0, end: 2 }, { src: "a.wav", start: 3, end: 5 }, { src: "a.wav", start: 6, end: 10 }]);
    const p0 = P();
    const q = removeRegions(p0, 1, []);
    expect(q).toBe(p0);
  });
  it("applyClipProps sets gain/fades on the selection only", () => {
    const p = applyClipProps(P(), { trackId: 1, a: 2, b: 5 }, { gain_db: -6, fade_in_ms: 100, fade_out_ms: 200 });
    expect(T(p).clips).toEqual([{ src: "a.wav", start: 0, end: 2 }, { src: "a.wav", start: 2, end: 5, gain_db: -6, fade_in_ms: 100, fade_out_ms: 200 }, { src: "a.wav", start: 5, end: 10 }]);
    expect(T(P()).clips[0].gain_db).toBeUndefined(); // originals untouched
  });
  it("clips with different gain/fades are not merged, equal ones are", () => {
    const p = applyClipProps(P(), { trackId: 1, a: 0, b: 5 }, { gain_db: -6 });
    const q = applyClipProps(p, { trackId: 1, a: 5, b: 10 }, { gain_db: -6 });
    expect(T(q).clips).toHaveLength(1); // same props -> merged back
    const r = applyClipProps(p, { trackId: 1, a: 5, b: 10 }, { gain_db: -12 });
    expect(T(r).clips).toHaveLength(2); // different gain -> stays split
  });
  it("paste replaces the selection", () => {
    const p = paste(P(), { trackId: 1, a: 1, b: 4 }, [{ src: "b.wav", start: 0, end: 1 }]);
    expect(trackLen(T(p))).toBeCloseTo(8);
    expect(T(p).clips[1]).toEqual({ src: "b.wav", start: 0, end: 1 });
  });
  it("paste at end appends; delete of empty selection is a no-op", () => {
    const p = paste(P(), { trackId: 1, a: 10, b: 10 }, [{ src: "b.wav", start: 0, end: 2 }]);
    expect(trackLen(T(p))).toBeCloseTo(12);
    const q = P(); expect(del(q, { trackId: 1, a: 3, b: 3 })).toEqual(q);
  });
  it("extract across clip boundaries", () => {
    const p = paste(P(), { trackId: 1, a: 5, b: 5 }, [{ src: "b.wav", start: 0, end: 2 }]);
    expect(extract(T(p), 4, 8)).toEqual([{ src: "a.wav", start: 4, end: 5 }, { src: "b.wav", start: 0, end: 2 }, { src: "a.wav", start: 5, end: 6 }]);
  });
  it("never mutates the input project", () => {
    const p = P(); const snap = JSON.stringify(p);
    cut(p, { trackId: 1, a: 1, b: 2 }); paste(p, { trackId: 1, a: 1, b: 1 }, [{ src: "x", start: 0, end: 1 }]);
    expect(JSON.stringify(p)).toBe(snap);
  });
  it("undo/redo", () => {
    const h = new History(P());
    h.apply("Delete", del(h.present, { trackId: 1, a: 0, b: 5 }));
    h.apply("Delete", del(h.present, { trackId: 1, a: 0, b: 1 }));
    expect(trackLen(T(h.present))).toBeCloseTo(4);
    expect(h.undo()).toBe(true); expect(trackLen(T(h.present))).toBeCloseTo(5);
    expect(h.undo()).toBe(true); expect(trackLen(T(h.present))).toBeCloseTo(10);
    expect(h.undo()).toBe(false);
    expect(h.redo()).toBe(true); expect(trackLen(T(h.present))).toBeCloseTo(5);
    h.apply("Cut", del(h.present, { trackId: 1, a: 0, b: 1 }));
    expect(h.canRedo).toBe(false);
  });
  it("trackPeaks maps through clips", () => {
    const s: Source = { path: "a.wav", name: "a", duration: 10, sampleRate: 48000, channels: 1, peaks: Array.from({ length: 10 }, (_, i) => [-i / 10, i / 10]).flat() };
    const { project } = cut(P(), { trackId: 1, a: 0, b: 5 }); // keep 5..10
    const pk = trackPeaks(T(project), new Map([["a.wav", s]]), 5);
    expect(pk[1]).toBeCloseTo(0.5); expect(pk[9]).toBeCloseTo(0.9);
  });
});

import { applyPreset, PRESETS, setPath, getPath, defaultFx, slot } from "./fx";
describe("effect chain", () => {
  it("setFx is undoable and does not touch clips", () => {
    const h = new History(P());
    h.apply("Preset", setFx(h.present, 1, applyPreset([], 0)));
    expect(T(h.present).fx!.length).toBe(PRESETS[0].build().length);
    expect(T(h.present).clips).toEqual(T(P()).clips);
    h.undo(); expect(T(h.present).fx).toBeUndefined();
    h.redo(); expect(T(h.present).fx![0].fx.type).toBe("Eq");
  });
  it("reorder and bypass", () => {
    const fx = [slot(defaultFx("Gain")), slot(defaultFx("Limiter"))];
    const m = moveFx(fx, 0, 1); expect(m.map((s) => s.fx.type)).toEqual(["Limiter", "Gain"]);
    expect(moveFx(fx, 0, -1)).toBe(fx);
  });
  it("presets keep a learned noise profile", () => {
    const withProfile = [slot({ type: "NoiseReduction", reduction_db: 10, sensitivity_db: 6, profile: [1, 2, 3] })];
    const out = applyPreset(withProfile, 1);
    expect((out.find((s) => s.fx.type === "NoiseReduction")!.fx as any).profile).toEqual([1, 2, 3]);
  });
  it("param paths set nested values immutably", () => {
    const eq = defaultFx("Eq"); const e2 = setPath(eq, "peaks.1.gain_db", 4);
    expect(getPath(e2, "peaks.1.gain_db")).toBe(4); expect(getPath(eq, "peaks.1.gain_db")).toBe(0);
  });
});

describe("markers", () => {
  it("add/remove/navigate, undoable, survive edits", () => {
    const h = new History(P());
    h.apply("Marker", addMarker(h.present, { id: 1, time: 5, label: "Scene: Game" }));
    h.apply("Marker", addMarker(h.present, { id: 2, time: 2, label: "M1" }));
    expect(h.present.markers!.map((m) => m.time)).toEqual([2, 5]);
    expect(nextMarker(h.present, 0, 1)!.id).toBe(2);
    expect(nextMarker(h.present, 2, 1)!.id).toBe(1);
    expect(nextMarker(h.present, 5, -1)!.id).toBe(2);
    expect(nextMarker(h.present, 5, 1)).toBeUndefined();
    h.apply("Delete", del(h.present, { trackId: 1, a: 0, b: 1 }));
    expect(h.present.markers!.length).toBe(2);
    h.apply("Remove", removeMarker(h.present, 1)); expect(h.present.markers!.length).toBe(1);
    h.undo(); h.undo(); h.undo(); expect(h.present.markers!.length).toBe(1);
  });
});

import { cutMany, copyMany, delMany, pasteMany, splitClips, joinClips, clipBounds, selectedIds, removeLabelRange, shiftLabels, labelsToText, textToLabels, labelsToMarkers, setLabels } from "./editModel";
const M = (): Project => ({ tracks: [
  { id: 1, name: "a", kind: "mic", clips: [{ src: "a.wav", start: 0, end: 10 }] },
  { id: 2, name: "b", kind: "mic", clips: [{ src: "b.wav", start: 0, end: 10 }] },
  { id: 3, name: "Labels", kind: "labels", clips: [], labels: [{ id: 9, t0: 1, t1: 1, text: "one" }, { id: 10, t0: 3, t1: 4, text: "mid" }, { id: 11, t0: 6, t1: 8, text: "late" }] },
  { id: 4, name: "c", kind: "mic", clips: [{ src: "c.wav", start: 0, end: 10 }] },
] });
const L = (p: Project, id: number) => p.tracks.find((t) => t.id === id)!;

describe("multi-track selection", () => {
  const sel = { trackId: 1, a: 2, b: 5, tracks: [2, 1, 3] };
  it("lists selected tracks in project order", () => { expect(selectedIds(M(), sel)).toEqual([1, 2, 3]); expect(selectedIds(M(), null)).toEqual([]); });
  it("cut applies to every selected track and keeps labels in sync", () => {
    const { project, clipboard } = cutMany(M(), sel);
    expect(clipboard).toEqual([[{ src: "a.wav", start: 2, end: 5 }], [{ src: "b.wav", start: 2, end: 5 }]]);
    expect(trackLen(L(project, 1))).toBeCloseTo(7); expect(trackLen(L(project, 2))).toBeCloseTo(7);
    expect(L(project, 4)).toEqual(L(M(), 4)); // unselected track untouched
    expect(L(project, 3).labels!.map((l) => [l.text, l.t0, l.t1])).toEqual([["one", 1, 1], ["late", 3, 5]]);
  });
  it("paste puts each clipboard track back into its own track", () => {
    const { project, clipboard } = cutMany(M(), sel);
    const back = pasteMany(project, { ...sel, b: 2 }, clipboard);
    expect(L(back, 1).clips).toEqual(L(M(), 1).clips); expect(L(back, 2).clips).toEqual(L(M(), 2).clips);
    expect(L(back, 3).labels!.map((l) => [l.t0, l.t1])).toEqual([[1, 1], [6, 8]]);
  });
  it("one clipboard track pastes into every selected track; delete and copy work on all", () => {
    const p = pasteMany(M(), { trackId: 1, a: 0, b: 0, tracks: [1, 4] }, [[{ src: "x.wav", start: 0, end: 1 }]]);
    expect([trackLen(L(p, 1)), trackLen(L(p, 2)), trackLen(L(p, 4))]).toEqual([11, 10, 11]);
    expect(copyMany(M(), { trackId: 2, a: 0, b: 1, tracks: [4] })).toHaveLength(2);
    expect(trackLen(L(delMany(M(), { trackId: 4, a: 0, b: 4, tracks: [1] }), 1))).toBeCloseTo(6);
  });
});

describe("split and join", () => {
  it("split at the cursor makes two clips that don't merge back after edits", () => {
    const p = splitClips(M(), { trackId: 1, a: 4, b: 4 });
    expect(L(p, 1).clips).toEqual([{ src: "a.wav", start: 0, end: 4 }, { src: "a.wav", start: 4, end: 10, split: true }]);
    expect(clipBounds(L(p, 1))).toEqual([{ t: 4, split: true }]);
    const q = delMany(p, { trackId: 1, a: 6, b: 7 }); // an unrelated edit keeps the split
    expect(clipBounds(L(q, 1)).filter((b) => b.split).map((b) => b.t)).toEqual([4]);
  });
  it("split a range on several tracks, then join restores the original", () => {
    const sel = { trackId: 1, a: 2, b: 5, tracks: [2] };
    const p = splitClips(M(), sel);
    expect(L(p, 1).clips).toHaveLength(3); expect(L(p, 2).clips).toHaveLength(3); expect(L(p, 4).clips).toHaveLength(1);
    const j = joinClips(p, { ...sel, a: 0, b: 10 });
    expect(L(j, 1).clips).toEqual(L(M(), 1).clips); expect(L(j, 2).clips).toEqual(L(M(), 2).clips);
  });
  it("join only removes boundaries inside the selection; splitting at the edges is a no-op", () => {
    let p = splitClips(M(), { trackId: 1, a: 2, b: 5 });
    p = joinClips(p, { trackId: 1, a: 1, b: 3 });
    expect(clipBounds(L(p, 1))).toEqual([{ t: 5, split: true }]);
    expect(splitClips(M(), { trackId: 1, a: 0, b: 0 })).toEqual(M());
    expect(splitClips(M(), { trackId: 1, a: 10, b: 10 })).toEqual(M());
  });
  it("cut then paste across a split keeps the split", () => {
    const p = splitClips(M(), { trackId: 1, a: 4, b: 4 });
    const { project, clipboard } = cutMany(p, { trackId: 1, a: 3, b: 5 });
    const back = pasteMany(project, { trackId: 1, a: 3, b: 3 }, clipboard);
    expect(L(back, 1).clips).toEqual(L(p, 1).clips);
  });
});

describe("labels", () => {
  const ls = [{ id: 1, t0: 1, t1: 1, text: "a" }, { id: 2, t0: 2, t1: 6, text: "span" }, { id: 3, t0: 3, t1: 4, text: "in" }, { id: 4, t0: 8, t1: 9, text: "after" }];
  it("remove a range: labels inside go, spanning ones shrink, later ones move", () => {
    expect(removeLabelRange(ls, 2.5, 4.5).map((l) => [l.text, l.t0, l.t1])).toEqual([["a", 1, 1], ["span", 2, 4], ["after", 6, 7]]);
  });
  it("insert: later labels move right, spanning ones grow", () => {
    expect(shiftLabels(ls, 3, 2).map((l) => [l.t0, l.t1])).toEqual([[1, 1], [2, 8], [5, 6], [10, 11]]);
  });
  it("Audacity text format round trip", () => {
    const txt = labelsToText([{ id: 1, t0: 0.25, t1: 0.25, text: "Intro" }, { id: 2, t0: 2.5, t1: 3, text: "Out\tro" }]);
    expect(txt).toBe("0.250000\t0.250000\tIntro\n2.500000\t3.000000\tOut ro\n");
    let n = 5; expect(textToLabels(txt + "junk\n", () => n++)).toEqual([{ id: 5, t0: 0.25, t1: 0.25, text: "Intro" }, { id: 6, t0: 2.5, t1: 3, text: "Out ro" }]);
  });
  it("labels to markers (no duplicates), sorted setLabels, label track length", () => {
    let n = 100; const p = labelsToMarkers(M(), L(M(), 3).labels!, () => n++);
    expect(p.markers!.map((m) => [m.time, m.label])).toEqual([[1, "one"], [3, "mid"], [6, "late"]]);
    expect(labelsToMarkers(p, L(M(), 3).labels!, () => n++).markers).toHaveLength(3);
    const q = setLabels(M(), 3, (x) => [...x, { id: 50, t0: 0.5, t1: 0.5, text: "first" }]);
    expect(L(q, 3).labels![0].text).toBe("first");
    expect(trackLen(L(M(), 3))).toBe(8);
  });
});

import { replaceRanges, trimMany, clipAt } from "./editModel";
describe("selection effects, trim, clip lookup", () => {
  it("replaces a range with rendered audio as its own clip", () => {
    const p = replaceRanges(M(), 2, 5, new Map([[1, [{ src: "fx.wav", start: 0, end: 3 }]]]));
    expect(L(p, 1).clips).toEqual([{ src: "a.wav", start: 0, end: 2 }, { src: "fx.wav", start: 0, end: 3, split: true }, { src: "a.wav", start: 5, end: 10, split: true }]);
    expect(trackLen(L(p, 1))).toBeCloseTo(10);
    expect(L(p, 2)).toEqual(L(M(), 2));
  });
  it("trim keeps only the selection on the selected tracks", () => {
    const p = trimMany(M(), { trackId: 1, a: 2, b: 7, tracks: [3] });
    expect(L(p, 1).clips).toEqual([{ src: "a.wav", start: 2, end: 7 }]);
    expect(L(p, 2)).toEqual(L(M(), 2));
    expect(L(p, 3).labels!.map((l) => [l.text, l.t0, l.t1])).toEqual([["mid", 1, 2], ["late", 4, 5]]);
    expect(trimMany(M(), { trackId: 1, a: 3, b: 3 })).toEqual(M());
  });
  it("finds the clip under the pointer", () => {
    const p = splitClips(M(), { trackId: 1, a: 4, b: 4 });
    expect(clipAt(L(p, 1), 6)).toEqual({ a: 4, b: 10 }); expect(clipAt(L(p, 1), 1)).toEqual({ a: 0, b: 4 }); expect(clipAt(L(p, 1), 11)).toBeNull();
  });
});
