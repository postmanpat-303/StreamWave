import { describe, it, expect } from "vitest";
import { toNorm, fromNorm, snap, dragTo, nudge, keyValue, litTicks, scaleLabels, readout, angleOf, autoLog, DRAG_PX } from "./knob";

const gain = { min: -24, max: 24, step: 0.5 };
const freq = { min: 20, max: 20000, step: 1, log: true };

describe("knob maths", () => {
  it("maps linear ranges both ways and snaps to the step grid", () => {
    expect(toNorm(0, gain)).toBe(0.5);
    expect(fromNorm(0.5, gain)).toBe(0);
    expect(fromNorm(0.51, gain)).toBe(0.5);
    expect(snap(0.1 + 0.2, { min: 0, max: 1, step: 0.1 })).toBe(0.3);
    expect(snap(99, gain)).toBe(24); expect(snap(-99, gain)).toBe(-24);
  });
  it("uses a log taper for frequencies (centre ≈ geometric mean)", () => {
    expect(autoLog(20, 20000, "Hz")).toBe(true); expect(autoLog(-24, 24, "dB")).toBe(false);
    expect(fromNorm(0.5, freq)).toBe(632);
    expect(toNorm(632.4555, freq)).toBeCloseTo(0.5, 4);
  });
  it("angles sweep 270° from lower-left to lower-right", () => {
    expect(angleOf(0)).toBe(-135); expect(angleOf(1)).toBe(135); expect(angleOf(0.5)).toBe(0);
  });
  it("drags up/right to increase; full travel = DRAG_PX; Shift is 10× finer", () => {
    expect(dragTo(-24, 0, -DRAG_PX, gain)).toBe(24);
    expect(dragTo(0, DRAG_PX / 4, 0, gain)).toBe(12);
    expect(dragTo(0, 0, DRAG_PX / 4, gain)).toBe(-12);
    expect(dragTo(0, 0, -DRAG_PX / 4, gain, true)).toBe(1); // 2.5 % of 48 dB = 1.2 → snapped to 1.0
  });
  it("wheel/arrow nudges always move at least one step and stop at the ends", () => {
    expect(nudge(0, 1, gain)).toBe(0.5);
    expect(nudge(24, 1, gain)).toBe(24);
    expect(nudge(0, -1, gain, true)).toBe(-5);
  });
  it("keyboard: arrows, PageUp/Down, Home/End, Delete resets, other keys ignored", () => {
    expect(keyValue("ArrowUp", 0, gain)).toBe(0.5);
    expect(keyValue("ArrowLeft", 0, gain)).toBe(-0.5);
    expect(keyValue("PageUp", 0, gain)).toBe(5);
    expect(keyValue("Home", 3, gain)).toBe(-24);
    expect(keyValue("End", 3, gain)).toBe(24);
    expect(keyValue("Delete", 3, gain, false, 0)).toBe(0);
    expect(keyValue("Delete", 3, gain)).toBeNull();
    expect(keyValue("a", 3, gain)).toBeNull();
  });
  it("lights tick dots up to the value", () => {
    expect(litTicks(-24, gain, 11)).toBe(0);
    expect(litTicks(0, gain, 11)).toBe(6);
    expect(litTicks(24, gain, 11)).toBe(11);
  });
  it("scale labels and readouts are compact", () => {
    expect(scaleLabels(gain).map((l) => l[1])).toEqual(["-24", "-12", "0", "12", "24"]);
    expect(scaleLabels(freq, 4).map((l) => l[1])).toEqual(["20", "200", "2k", "20k"]);
    expect(readout(-12, "dB")).toBe("-12.0 dB");
    expect(readout(5500, "Hz")).toBe("5.5 kHz");
    expect(readout(0.8, "")).toBe("0.80");
  });
});
