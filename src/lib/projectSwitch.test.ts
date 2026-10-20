import { describe, it, expect } from "vitest";
import { switchProject, type SwitchDeps } from "./projectSwitch";

function deps(o: { recording?: boolean; dirty?: boolean; answers?: boolean[]; failRelease?: boolean } = {}) {
  const log: string[] = [];
  let rec = !!o.recording, dirty = !!o.dirty; const answers = [...(o.answers ?? [])];
  const d: SwitchDeps = {
    recording: () => rec,
    confirm: async (m) => { log.push(`ask:${m.startsWith("Discard") ? "discard" : "recording"}`); return answers.shift() ?? false; },
    stopRecording: async () => { log.push("stopRecording"); rec = false; dirty = true; /* the take landed in the old project */ },
    dirty: () => dirty,
    releaseAudio: async () => { log.push("release"); if (o.failRelease) throw new Error("engine gone"); },
  };
  return { d, log };
}
const pick = (v: string | null, log: string[]) => async () => { log.push("pick"); return v; };
const replace = (log: string[]) => async (p: string) => { log.push(`replace:${p}`); };

describe("switching projects stops the old audio first", () => {
  it("clean project: stops playback, then replaces", async () => {
    const { d, log } = deps();
    expect(await switchProject(d, pick("b.twt", log), replace(log))).toBe(true);
    expect(log).toEqual(["pick", "release", "replace:b.twt"]);
  });
  it("dirty project: asks first; Cancel leaves playback and the project alone", async () => {
    const { d, log } = deps({ dirty: true, answers: [false] });
    expect(await switchProject(d, pick("b.twt", log), replace(log))).toBe(false);
    expect(log).toEqual(["ask:discard"]);
  });
  it("dirty project: Discard -> stop -> replace (never replace while the old audio plays)", async () => {
    const { d, log } = deps({ dirty: true, answers: [true] });
    await switchProject(d, pick("b.twt", log), replace(log));
    expect(log).toEqual(["ask:discard", "pick", "release", "replace:b.twt"]);
    expect(log.indexOf("release")).toBeLessThan(log.indexOf("replace:b.twt"));
  });
  it("cancelling the Open dialog stops nothing", async () => {
    const { d, log } = deps();
    expect(await switchProject(d, pick(null, log), replace(log))).toBe(false);
    expect(log).toEqual(["pick"]);
  });
  it("while recording: asks, stops the recording, then the unsaved prompt covers the new take", async () => {
    const { d, log } = deps({ recording: true, answers: [true, true] });
    expect(await switchProject(d, pick("new", log), replace(log))).toBe(true);
    expect(log).toEqual(["ask:recording", "stopRecording", "ask:discard", "pick", "release", "replace:new"]);
  });
  it("while recording: saying No keeps recording and the project", async () => {
    const { d, log } = deps({ recording: true, answers: [false] });
    expect(await switchProject(d, pick("new", log), replace(log))).toBe(false);
    expect(log).toEqual(["ask:recording"]);
  });
  it("keeping the take (No to discard) after stopping the recording keeps the project", async () => {
    const { d, log } = deps({ recording: true, answers: [true, false] });
    expect(await switchProject(d, pick("new", log), replace(log))).toBe(false);
    expect(log).toEqual(["ask:recording", "stopRecording", "ask:discard"]);
  });
  it("an engine error while stopping doesn't block loading the new project", async () => {
    const { d, log } = deps({ failRelease: true });
    expect(await switchProject(d, pick("b.twt", log), replace(log))).toBe(true);
    expect(log).toEqual(["pick", "release", "replace:b.twt"]);
  });
});
