import { describe, it, expect } from "vitest";
import { serialize, deserialize } from "./project";
import { cut, setFx, History, type Project, type Source } from "./editModel";
import { applyPreset } from "./fx";

describe("project file", () => {
  const src: Source = { path: "C:\\rec\\a.wav", name: "a.wav", duration: 10, sampleRate: 48000, channels: 2, peaks: [0, 1] };
  const base: Project = { tracks: [{ id: 3, name: "Mic", kind: "mic", clips: [{ src: src.path, start: 0, end: 10 }] }] };
  it("round-trips tracks, clips, effects and noise profiles", () => {
    let p = cut(base, { trackId: 3, a: 2, b: 4 }).project;
    const chain = applyPreset([], 1).map((s) => (s.fx.type === "NoiseReduction" ? { ...s, fx: { ...s.fx, profile: [0.1, 0.2, 0.3] } } : s));
    p = setFx(p, 3, chain);
    p = { ...p, markers: [{ id: 11, time: 1.5, label: "Scene: Intro" }], tracks: p.tracks.map((t) => ({ ...t, video: "C:\\obs\\x.mkv" })) };
    const json = serialize(p, new Map([[src.path, src]]), 7);
    const f = deserialize(json);
    expect(f.tracks).toEqual(p.tracks);
    expect(f.markers).toEqual(p.markers);
    expect(f.sources).toEqual([{ path: src.path, name: "a.wav", duration: 10, sampleRate: 48000, channels: 2 }]);
    expect(f.nextId).toBe(12); // bumped past marker ids
    expect((f.tracks[0].fx!.find((s) => s.fx.type === "NoiseReduction")!.fx as any).profile).toEqual([0.1, 0.2, 0.3]);
    expect(JSON.parse(json).sources[0].peaks).toBeUndefined(); // waveform peaks are recomputed, keeps files small
  });
  it("saving does not affect undo history", () => {
    const h = new History(base); h.apply("Cut", cut(base, { trackId: 3, a: 0, b: 1 }).project);
    serialize(h.present, new Map(), 1);
    expect(h.undo()).toBe(true); expect(h.present).toBe(base);
  });
  it("rejects garbage and newer versions, fixes nextId", () => {
    expect(() => deserialize('{"format":"other"}')).toThrow();
    expect(deserialize('{"format":"truewave-project","version":1,"tracks":[]}').tracks).toEqual([]); // pre-rename files
    expect(() => deserialize('{"format":"streamwave-project","version":99,"tracks":[]}')).toThrow(/newer/);
    expect(() => deserialize('{"format":"truewave-project","version":1,"tracks":[{"id":1,"clips":[{"src":"a","start":5,"end":1}]}]}')).toThrow();
    const f = deserialize('{"format":"truewave-project","version":1,"tracks":[{"id":9,"name":"x","kind":"mic","clips":[]}]}');
    expect(f.nextId).toBe(10); expect(f.tracks[0].fx).toEqual([]);
  });
});
