import { describe, it, expect } from "vitest";
import { addMarker, removeMarker, nextMarker, setFx, moveFx, cut, copy, del, paste, History, trackLen, extract, trackPeaks, type Project, type Source } from "./editModel";

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
