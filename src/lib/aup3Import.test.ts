import { describe, it, expect } from "vitest";
import { buildFromAup3, importedFiles, summary, isAup3, isMedia, isProjectFile, type Aup3Result } from "./aup3Import";

const r: Aup3Result = {
  name: "show", media_dir: "/m", from_autosave: false,
  tracks: [
    { name: "Voice", clips: [{ path: "/m/01 a.wav", start: 0, end: 1 }, { path: "/m/01 s.wav", start: 0, end: 0.5 }, { path: "/m/01 b.wav", start: 0, end: 1.5 }], gain_db: -6.02, muted: false, solo: true, channels: 2, clip_count: 2 },
    { name: "Music", clips: [{ path: "/m/02 a.wav", start: 0, end: 2 }], gain_db: null, muted: true, solo: false, channels: 2, clip_count: 1 },
    { name: "Empty", clips: [], gain_db: null, muted: false, solo: false, channels: 1, clip_count: 0 },
  ],
  labels: [{ t0: 2.5, t1: 3, title: "Outro", track: "L" }, { t0: 0.25, t1: 0.25, title: "Intro", track: "L" }],
  notes: ["Audacity effects aren't imported: Reverb (on \"Voice\")."],
};

describe("aup3 import mapping", () => {
  it("maps tracks, clips, gain, mute/solo and labels", () => {
    let id = 1;
    const { tracks, markers } = buildFromAup3(r, (p) => p, () => id++);
    expect(tracks.map((t) => t.name)).toEqual(["Voice", "Music"]);
    expect(tracks[0].clips).toEqual([{ src: "/m/01 a.wav", start: 0, end: 1 }, { src: "/m/01 s.wav", start: 0, end: 0.5 }, { src: "/m/01 b.wav", start: 0, end: 1.5 }]);
    expect(tracks[0].fx?.[0].fx).toEqual({ type: "Gain", db: -6 });
    expect(tracks[0].solo).toBe(true); expect(tracks[0].muted).toBeUndefined();
    expect(tracks[1].fx).toBeUndefined(); expect(tracks[1].muted).toBe(true);
    expect(tracks.every((t) => t.kind === "audacity")).toBe(true);
    expect(markers.map((m) => [m.time, m.label])).toEqual([[0.25, "Intro"], [2.5, "Outro (to 3.00s)"]]);
    expect(new Set([...tracks.map((t) => t.id), ...markers.map((m) => m.id)]).size).toBe(4);
  });
  it("lists each file once and summarises", () => {
    expect(importedFiles(r)).toHaveLength(4);
    expect(summary(r)).toBe("Imported 2 tracks (3 clips) and 2 labels as markers.\nAudacity effects aren't imported: Reverb (on \"Voice\").");
  });
  it("routes dropped files", () => {
    expect([isAup3("a/B.AUP3"), isProjectFile("x.twt"), isMedia("c.Flac"), isMedia("d.aup")]).toEqual([true, true, true, false]);
  });
});
