import { describe, it, expect } from "vitest";
import { arrangeImport, moveClip, setClipFx, placed, audioClips, trackLen, cut, paste, copy, splitClips, History, snapClip, canPlace, gap,
  type Project, type ImportFile, type Clip } from "./editModel";
import { serialize, deserialize } from "./project";
import { slot } from "./fx";

const F = (name: string, duration: number, sampleRate = 48000): ImportFile => ({ path: `/m/${name}`, name, duration, sampleRate });
let id = 100; const nid = () => id++;
const base = (): Project => ({ tracks: [{ id: 1, name: "Mic", kind: "mic", clips: [{ src: "/m/mic.wav", start: 0, end: 10 }] }], markers: [{ id: 5, time: 3, label: "Intro" }] });
const pos = (p: Project, tid: number) => audioClips(p.tracks.find((t) => t.id === tid)!).map((q) => [q.clip.src, q.a, q.b]);
const T = (p: Project, tid: number) => p.tracks.find((t) => t.id === tid)!;

describe("multi-file import arrangement", () => {
  // mixed formats, sample rates and durations
  const files = [F("intro.mp3", 3.5, 44100), F("voice.wav", 61.25, 48000), F("outro.flac", 2 + 1 / 96000, 96000)];
  it("sequential: one new track, back to back in selection order, no gaps or overlaps", () => {
    const { project, trackIds } = arrangeImport(base(), files, "sequential", 0, nid);
    expect(trackIds).toHaveLength(1);
    const t = T(project, trackIds[0]);
    expect(t.name).toBe("intro.mp3 + 2 more");
    const q = audioClips(t);
    expect(q.map((x) => x.clip.src)).toEqual(files.map((f) => f.path));
    for (let i = 1; i < q.length; i++) expect(q[i].a).toBe(q[i - 1].b); // exactly contiguous
    expect(q.map((x) => x.clip.end - x.clip.start)).toEqual(files.map((f) => f.duration)); // full, exact file durations
    expect(trackLen(t)).toBeCloseTo(3.5 + 61.25 + 2 + 1 / 96000, 12);
    expect(t.clips.every((c) => !c.gap)).toBe(true);
  });
  it("sequential from the playhead starts with silence, snapped to a whole sample", () => {
    const { project, trackIds } = arrangeImport(base(), files, "sequential", 12.3456789, nid);
    const q = audioClips(T(project, trackIds[0]));
    expect(q[0].a * 44100).toBeCloseTo(Math.round(12.3456789 * 44100), 6);
    expect(q[1].a).toBeCloseTo(q[0].a + 3.5, 12);
  });
  it("one track per file, all aligned at the same start, named from the files", () => {
    const { project, trackIds } = arrangeImport(base(), files, "tracks", 4, nid);
    expect(trackIds).toHaveLength(3);
    expect(trackIds.map((i) => T(project, i).name)).toEqual(["intro.mp3", "voice.wav", "outro.flac"]);
    for (const [k, i] of trackIds.entries()) { const q = audioClips(T(project, i)); expect(q).toHaveLength(1); expect(q[0].a).toBe(4); expect(q[0].clip.end - q[0].clip.start).toBe(files[k].duration); }
  });
  it("keeps existing content and markers; references files without copying them", () => {
    const b = base();
    for (const mode of ["sequential", "tracks"] as const) {
      const { project } = arrangeImport(b, files, mode, 0, nid);
      expect(project.tracks[0]).toBe(b.tracks[0]);
      expect(project.markers).toBe(b.markers);
      const srcs = project.tracks.slice(1).flatMap((t) => t.clips.filter((c) => !c.gap).map((c) => c.src));
      expect(new Set(srcs)).toEqual(new Set(files.map((f) => f.path)));
    }
  });
  it("is one undo step; empty selection or cancel leaves the project unchanged", () => {
    const b = base(); const h = new History(b);
    h.apply("Import 3 files", arrangeImport(b, files, "tracks", 0, nid).project);
    expect(h.present.tracks).toHaveLength(4); h.undo(); expect(h.present).toBe(b);
    const empty = arrangeImport(b, [], "sequential", 0, nid);
    expect(empty.project).toBe(b); expect(empty.trackIds).toEqual([]);
    expect(arrangeImport(b, [F("broken.wav", 0)], "tracks", 0, nid).project).toBe(b);
  });
  it("is saved in the project file", () => {
    const { project } = arrangeImport(base(), files, "sequential", 2, nid);
    const f = deserialize(serialize(project, new Map(), 200));
    expect(f.tracks.map((t) => t.clips)).toEqual(project.tracks.map((t) => t.clips));
    expect(f.version).toBe(2); // uses a leading gap
    expect(f.sources.map((s) => s.path)).not.toContain(""); // gaps are not files
  });
});

describe("moving clips (non-destructive)", () => {
  const two = (): Project => ({ tracks: [
    { id: 1, name: "A", kind: "mic", clips: [{ src: "/a.wav", start: 0, end: 4 }, { src: "/b.wav", start: 1, end: 3, split: true, gain_db: -3, fade_in_ms: 20, fx: [slot({ type: "Gain", db: 2 })] }, { src: "/c.wav", start: 0, end: 5, split: true }] },
    { id: 2, name: "B", kind: "import", clips: [{ src: "/d.wav", start: 0, end: 2 }] },
    { id: 3, name: "Labels", kind: "labels", clips: [], labels: [{ id: 9, t0: 1, t1: 1, text: "x" }] },
  ], markers: [{ id: 4, time: 5, label: "M" }] });
  it("moves along the same track, leaving silence; nothing else shifts", () => {
    const p = two();
    const r = moveClip(p, 1, 0, 1, 20, 48000); expect(r.ok).toBe(true); if (!r.ok) return;
    expect(pos(r.project, 1)).toEqual([["/b.wav", 4, 6], ["/c.wav", 6, 11], ["/a.wav", 20, 24]]);
    expect(trackLen(T(r.project, 1))).toBe(24); // the timeline extends past the old end
    expect(r.project.markers).toBe(p.markers);
  });
  it("moves onto another track, past its end; source, trim, gain, fades and effects travel with it", () => {
    const p = two(); const c = T(p, 1).clips[1];
    const r = moveClip(p, 1, 1, 2, 7.5, 48000); if (!r.ok) throw new Error(r.reason);
    const moved = audioClips(T(r.project, 2)).find((q) => q.clip.src === "/b.wav")!;
    expect([moved.a, moved.b]).toEqual([7.5, 9.5]);
    expect({ ...moved.clip, split: undefined }).toEqual({ ...c, split: undefined });
    expect(pos(r.project, 1)).toEqual([["/a.wav", 0, 4], ["/c.wav", 6, 11]]); // c stays where it was
  });
  it("refuses to drop onto existing audio, and onto label tracks", () => {
    const p = two();
    expect(moveClip(p, 2, 0, 1, 3, 48000)).toEqual({ ok: false, reason: "overlap" });
    expect(moveClip(p, 2, 0, 1, 10.999, 48000)).toEqual({ ok: false, reason: "overlap" });
    expect(moveClip(p, 1, 0, 3, 0, 48000)).toEqual({ ok: false, reason: "invalid" });
    expect(canPlace(T(p, 1), 11, 1)).toBe(true); // touching the end is fine
  });
  it("can drop into silence between clips, exactly filling it", () => {
    let p = two();
    let r = moveClip(p, 1, 1, 2, 2, 48000); if (!r.ok) throw 0; p = r.project; // leaves a 2 s gap at 4..6 on track 1
    r = moveClip(p, 2, 0, 1, 4, 48000); if (!r.ok) throw 0;
    expect(pos(r.project, 1)).toEqual([["/a.wav", 0, 4], ["/d.wav", 4, 6], ["/c.wav", 6, 11]]);
  });
  it("positions are whole samples and do not drift over many moves", () => {
    let p = two(); const rate = 44100;
    let at = 0;
    for (let i = 0; i < 200; i++) {
      at = 11 + ((i * 7919) % 100000) / 9973.3; // awkward fractional targets
      const t = T(p, 1); const idx = t.clips.findIndex((c) => c.src === "/a.wav");
      const r = moveClip(p, 1, idx, 1, at, rate); if (!r.ok) throw new Error(`move ${i}`); p = r.project;
      const q = audioClips(T(p, 1)).find((x) => x.clip.src === "/a.wav")!;
      expect(Math.abs(q.a * rate - Math.round(at * rate))).toBeLessThan(1e-6);
    }
    expect(pos(p, 1).slice(0, 2)).toEqual([["/b.wav", 4, 6], ["/c.wav", 6, 11]]);
  });
  it("undo/redo restore exactly, and moves survive a save/reload", () => {
    const p = two(); const h = new History(p);
    const r = moveClip(p, 1, 1, 2, 30, 48000); if (!r.ok) throw 0;
    h.apply("Move clip", r.project); h.undo(); expect(h.present).toBe(p); h.redo(); expect(h.present).toBe(r.project);
    const f = deserialize(serialize(r.project, new Map(), 50));
    expect(f.tracks).toEqual(r.project.tracks.map((t) => ({ ...t, fx: t.fx ?? [] })));
  });
  it("snaps either clip edge to the nearest point within tolerance", () => {
    expect(snapClip(9.95, 2, [10, 20], 0.1)).toEqual({ t: 10, to: 10 });
    expect(snapClip(17.97, 2, [10, 20], 0.1)).toEqual({ t: 18, to: 20 });
    expect(snapClip(15, 2, [10, 20], 0.1)).toEqual({ t: 15, to: null });
  });
  it("older projects (no gaps, no clip effects) still open and are still written as version 1", () => {
    const old = '{"format":"streamwave-project","version":1,"tracks":[{"id":1,"name":"t","kind":"mic","clips":[{"src":"/a.wav","start":0,"end":3}]}]}';
    const f = deserialize(old); expect(f.tracks[0].clips).toEqual([{ src: "/a.wav", start: 0, end: 3 }]);
    expect(JSON.parse(serialize({ tracks: f.tracks }, new Map(), 2)).version).toBe(1);
    expect(() => deserialize('{"format":"streamwave-project","version":2,"tracks":[{"id":1,"clips":[{"src":"","start":0,"end":3}]}]}')).toThrow(/Damaged/); // empty src must be a gap
  });
});

describe("clip effect stacks (Layering Effects)", () => {
  const P = (): Project => ({ tracks: [{ id: 1, name: "A", kind: "mic", clips: [{ src: "/a.wav", start: 0, end: 10 }] }] });
  const stack = [slot({ type: "Eq", hpf_hz: 100, hpf_on: true, low_shelf: { freq: 120, gain_db: 0, q: 1 }, peaks: [{ freq: 300, gain_db: 0, q: 1 }, { freq: 1500, gain_db: 0, q: 1 }, { freq: 4000, gain_db: 3, q: 1 }], high_shelf: { freq: 9000, gain_db: 0, q: 1 } }),
    slot({ type: "Compressor", threshold_db: -20, ratio: 3, attack_ms: 10, release_ms: 120, makeup_db: 3 })];
  it("belongs to one clip; order is kept", () => {
    const p = setClipFx(P(), 1, 0, stack);
    expect(T(p, 1).clips[0].fx!.map((s) => s.fx.type)).toEqual(["Eq", "Compressor"]);
    expect(setClipFx(p, 1, 0, [stack[1], stack[0]]).tracks[0].clips[0].fx!.map((s) => s.fx.type)).toEqual(["Compressor", "Eq"]);
    expect(T(setClipFx(p, 1, 0, []), 1).clips[0].fx).toBeUndefined();
  });
  it("copy/paste copies the stack (independent), split pieces each keep a copy", () => {
    const p = setClipFx(P(), 1, 0, stack);
    const cb = copy(p, { trackId: 1, a: 2, b: 4 });
    const q = paste(p, { trackId: 1, a: 10, b: 10 }, cb);
    const [orig, pasted] = [T(q, 1).clips[0], T(q, 1).clips[T(q, 1).clips.length - 1]];
    expect(pasted.fx).toEqual(orig.fx);
    expect(pasted.fx).not.toBe(orig.fx); expect(pasted.fx![0].fx).not.toBe(orig.fx![0].fx);
    const s = splitClips(p, { trackId: 1, a: 5, b: 5 });
    expect(T(s, 1).clips.map((c) => c.fx!.length)).toEqual([2, 2]);
    // editing one half does not touch the other
    const e = setClipFx(s, 1, 1, [{ ...stack[0], bypass: true }]);
    expect(T(e, 1).clips[0].fx).toEqual(stack); expect(T(e, 1).clips[1].fx![0].bypass).toBe(true);
  });
  it("pieces with different stacks are never merged back together", () => {
    const p = setClipFx(splitClips(P(), { trackId: 1, a: 5, b: 5 }), 1, 1, [stack[0]]);
    const r = cut(p, { trackId: 1, a: 2, b: 3 }).project; // re-merges internal pieces only where props match
    expect(T(r, 1).clips.map((c) => [c.start, c.end, (c.fx ?? []).length])).toEqual([[0, 2, 0], [3, 5, 0], [5, 10, 1]]);
  });
  it("persists in .twt (version 2)", () => {
    const p = setClipFx(P(), 1, 0, stack);
    const f = deserialize(serialize(p, new Map(), 9));
    expect(f.version).toBe(2); expect(f.tracks[0].clips[0].fx).toEqual(stack);
  });
  it("gap helpers", () => {
    const t = { id: 1, name: "x", kind: "mic", clips: [gap(2), { src: "/a.wav", start: 0, end: 1 } as Clip] };
    expect(placed(t).map((q) => [q.a, q.b])).toEqual([[0, 2], [2, 3]]);
    expect(audioClips(t)).toHaveLength(1);
  });
});

import { moveLayer, LAYER_TYPES } from "./fx";
import { audioMenu, findItem } from "./contextMenu";
describe("Layering Effects panel helpers", () => {
  const L = ["Eq", "Compressor", "Limiter", "Gain"].map((t, i) => ({ id: i + 1, bypass: false, fx: { type: t } as any }));
  it("reorders by drag (from -> to) and keyboard (±1); list order = DSP order", () => {
    expect(moveLayer(L, 0, 2).map((s) => s.id)).toEqual([2, 3, 1, 4]);
    expect(moveLayer(L, 3, 0).map((s) => s.id)).toEqual([4, 1, 2, 3]);
    expect(moveLayer(L, 1, 0).map((s) => s.id)).toEqual([2, 1, 3, 4]);
    expect(moveLayer(L, 0, -1)).toBe(L); // already at the top: no change, no undo step
    expect(moveLayer(L, 3, 9)).toBe(L);
    expect(moveLayer(L, 1, 1)).toBe(L);
  });
  it("offers the track effects as layers (except noise reduction, which needs a learned profile)", () => {
    expect(LAYER_TYPES).toContain("Eq"); expect(LAYER_TYPES).toContain("Compressor"); expect(LAYER_TYPES).not.toContain("NoiseReduction");
  });
  it("clip right-click menu has Layering Effects, only on a clip", () => {
    const ctx = { hasRange: false, hasClipboard: false, canJoin: false, canSplit: true, muted: false, solo: false, spectrogram: false, tracks: 1, trackName: "Mic" };
    expect(findItem(audioMenu({ ...ctx, onClip: true }), "layerfx")!.disabled).toBe(false);
    expect(findItem(audioMenu({ ...ctx, onClip: false }), "layerfx")!.disabled).toBe(true);
  });
});
