import { describe, expect, it } from "vitest";
import { fmtRuler, pickStep, rulerTicks } from "./ruler";

describe("ruler", () => {
  it("picks coarser steps when zoomed out and finer when zoomed in", () => {
    expect(pickStep(1)[0]).toBe(120);    // 1 px/s
    expect(pickStep(10)[0]).toBe(10);
    expect(pickStep(100)[0]).toBe(1);
    expect(pickStep(1000)[0]).toBe(0.1);
    expect(pickStep(1e-6)[0]).toBe(3600); // never runs out
  });
  it("formats m:ss, decimals for sub-second steps, h:mm:ss for long timelines", () => {
    expect(fmtRuler(0, 1)).toBe("0:00");
    expect(fmtRuler(75, 5)).toBe("1:15");
    expect(fmtRuler(1.5, 0.5)).toBe("0:01.5");
    expect(fmtRuler(61.25, 0.05)).toBe("1:01.25");
    expect(fmtRuler(3725, 60)).toBe("1:02:05");
    expect(fmtRuler(65, 5, true)).toBe("0:01:05");
  });
  it("labels every major tick, spaces labels at least minLabelPx apart, and stays within the duration", () => {
    const pps = 50, ticks = rulerTicks(30, pps);
    const majors = ticks.filter((t) => t.major);
    expect(majors[0]).toEqual({ t: 0, major: true, label: "0:00" });
    expect(majors.every((t) => t.label)).toBe(true);
    expect(ticks.filter((t) => !t.major).every((t) => t.label === undefined)).toBe(true);
    for (let i = 1; i < majors.length; i++) expect((majors[i].t - majors[i - 1].t) * pps).toBeGreaterThanOrEqual(72);
    expect(ticks[ticks.length - 1].t).toBeLessThanOrEqual(30);
    expect(majors.map((t) => t.label)).toEqual(["0:00", "0:02", "0:04", "0:06", "0:08", "0:10", "0:12", "0:14", "0:16", "0:18", "0:20", "0:22", "0:24", "0:26", "0:28", "0:30"]);
  });
  it("has no floating-point drift on minor ticks", () => {
    const ticks = rulerTicks(3, 1000); // 0.1 s labels, 0.02 s minors
    expect(ticks.find((t) => Math.abs(t.t - 2.3) < 1e-9)?.label).toBe("0:02.3");
    expect(ticks.every((t) => Number.isFinite(t.t) && String(t.t).length < 10)).toBe(true);
  });
  it("returns nothing for empty or invalid input", () => {
    expect(rulerTicks(0, 100)).toEqual([]);
    expect(rulerTicks(10, 0)).toEqual([]);
    expect(rulerTicks(NaN, 10)).toEqual([]);
  });
});
