import { describe, it, expect } from "vitest";
import { canCrossfade, cut, joinClips, placed, setCrossfade, splitClips, trackLen, usesCrossfades, xfadeLen, type Project } from "./editModel";
import { deserialize, serialize } from "./project";

const dur: Record<string, number> = { "a.wav": 10, "b.wav": 8 };
const srcDur = (s: string) => dur[s];
// a: 2..6 of a 10 s file (handles both sides), b: 1..5 of an 8 s file
const handles: Project = { tracks: [{ id: 1, name: "T", kind: "mic", clips: [{ src: "a.wav", start: 2, end: 6 }, { src: "b.wav", start: 1, end: 5 }] }] };
// two whole files back to back (no spare audio at the cut)
const whole: Project = { tracks: [{ id: 1, name: "T", kind: "import", clips: [{ src: "a.wav", start: 0, end: 10 }, { src: "b.wav", start: 0, end: 8, split: true }] }] };

describe("crossfades", () => {
  it("uses the sources' spare audio so nothing moves", () => {
    const r = setCrossfade(handles, 1, 1, 400, "equal_power", srcDur)!;
    expect(r.trimmed).toBe(0); expect(r.ms).toBe(400);
    const t = r.project.tracks[0];
    expect(trackLen(t)).toBe(8); expect(placed(t).map((q) => [q.a, q.b])).toEqual([[0, 4], [4, 8]]);
    expect(t.clips[1]).toMatchObject({ xfade_ms: 400, xfade_curve: "equal_power" });
    expect(xfadeLen(t, 1, srcDur)).toBe(400);
  });
  it("overlaps whole files by trimming the missing handle, later audio moves left", () => {
    const r = setCrossfade(whole, 1, 1, 1000, "linear", srcDur)!;
    expect(r.trimmed).toBeCloseTo(1, 9); // 0.5 s off the end of a, 0.5 s off the start of b
    const t = r.project.tracks[0];
    expect(t.clips[0].end).toBeCloseTo(9.5, 9); expect(t.clips[1].start).toBeCloseTo(0.5, 9);
    expect(trackLen(t)).toBeCloseTo(17, 9);
    expect(xfadeLen(t, 1, srcDur)).toBeCloseTo(1000, 6); // now fully backed by real audio
  });
  it("limits the length to what both clips allow and refuses gaps / the first clip", () => {
    const short: Project = { tracks: [{ id: 1, name: "T", kind: "mic", clips: [{ src: "a.wav", start: 2, end: 2.3 }, { src: "b.wav", start: 1, end: 5 }] }] };
    const r = setCrossfade(short, 1, 1, 2000, "linear", srcDur)!;
    expect(r.ms).toBeLessThanOrEqual(300); expect(r.ms).toBeGreaterThan(250);
    const g: Project = { tracks: [{ id: 1, name: "T", kind: "mic", clips: [{ src: "", start: 0, end: 1, gap: true }, { src: "b.wav", start: 1, end: 5 }] }] };
    expect(canCrossfade(g.tracks[0], 1)).toBe(false); expect(setCrossfade(g, 1, 1, 100, "linear", srcDur)).toBeNull();
    expect(setCrossfade(handles, 1, 0, 100, "linear", srcDur)).toBeNull();
  });
  it("removing it restores the plain cut", () => {
    const on = setCrossfade(handles, 1, 1, 300, "linear", srcDur)!.project;
    const off = setCrossfade(on, 1, 1, 0, "linear", srcDur)!.project;
    expect(off.tracks[0].clips).toEqual(handles.tracks[0].clips);
  });
  it("survives split + join of either clip and stays on the clip's head", () => {
    const on = setCrossfade(handles, 1, 1, 300, "linear", srcDur)!.project;
    const s = splitClips(on, { trackId: 1, a: 6, b: 6 }); // split inside clip b
    const cs = s.tracks[0].clips;
    expect(cs).toHaveLength(3); expect(cs[1].xfade_ms).toBe(300); expect(cs[2].xfade_ms).toBeUndefined();
    const sa = splitClips(on, { trackId: 1, a: 1, b: 1 }); // split inside clip a (the outgoing one): end unchanged, crossfade kept
    expect(sa.tracks[0].clips[2].xfade_ms).toBe(300);
    expect(joinClips(s, { trackId: 1, a: 0, b: 8 }).tracks[0].clips).toEqual(on.tracks[0].clips);
  });
  it("is dropped when an edit changes the neighbour it fades from", () => {
    const on = setCrossfade(handles, 1, 1, 300, "linear", srcDur)!.project;
    const trimmed = cut(on, { trackId: 1, a: 3.5, b: 4 }).project; // shortens a's tail
    expect(trimmed.tracks[0].clips.some((c) => c.xfade_ms)).toBe(false);
    const removed = cut(on, { trackId: 1, a: 0, b: 4 }).project; // removes a entirely
    expect(removed.tracks[0].clips[0].xfade_ms).toBeUndefined();
  });
  it("saves as format v3 only when used, round-trips, and v1/v2 files still load", () => {
    const on = setCrossfade(handles, 1, 1, 300, "equal_power", srcDur)!.project;
    expect(usesCrossfades(on)).toBe(true); expect(usesCrossfades(handles)).toBe(false);
    const f = deserialize(serialize(on, new Map(), 5));
    expect(f.version).toBe(3); expect(f.tracks[0].clips).toEqual(on.tracks[0].clips);
    expect(deserialize(serialize(handles, new Map(), 5)).version).toBe(1);
    const v2 = '{"format":"streamwave-project","version":2,"nextId":3,"tracks":[{"id":1,"name":"T","kind":"mic","clips":[{"src":"","start":0,"end":1,"gap":true},{"src":"a.wav","start":0,"end":2}]}],"markers":[],"sources":[]}';
    expect(deserialize(v2).tracks[0].clips).toHaveLength(2);
    expect(() => deserialize('{"format":"streamwave-project","version":4,"tracks":[]}')).toThrow(/newer/);
  });
});

import { curveGain, setClipFade } from "./editModel";
describe("fades", () => {
  it("curve shapes match the renderer (S-curve default, linear, equal power)", () => {
    expect(curveGain(undefined, 0.25)).toBeCloseTo(0.5 * (1 - Math.cos(Math.PI * 0.25)), 12);
    expect(curveGain("linear", 0.25)).toBe(0.25);
    expect(curveGain("equal_power", 0.25)).toBeCloseTo(Math.sin(Math.PI / 8), 12);
    for (const c of ["linear", "equal_power", "s_curve"] as const) { expect(curveGain(c, 0)).toBe(0); expect(curveGain(c, 1)).toBeCloseTo(1, 12); }
  });
  it("fade handles set one clip's fades without letting them overlap", () => {
    let p = setClipFade(handles, 1, 0, "in", 500);
    expect(p.tracks[0].clips[0].fade_in_ms).toBe(500);
    p = setClipFade(p, 1, 0, "out", 99999); // clip is 4 s long, 0.5 s already fades in
    expect(p.tracks[0].clips[0].fade_out_ms).toBe(3500);
    p = setClipFade(p, 1, 0, "in", 0); expect(p.tracks[0].clips[0].fade_in_ms).toBeUndefined();
    expect(p.tracks[0].clips[1]).toEqual(handles.tracks[0].clips[1]);
  });
});
