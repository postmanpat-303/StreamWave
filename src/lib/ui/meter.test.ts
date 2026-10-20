import { describe, it, expect } from "vitest";
import { toDb, dbPos, ampPos, zone, litDots } from "./meter";
describe("meter maths", () => {
  it("maps amplitude to a -60..0 dB position", () => {
    expect(toDb(1)).toBe(0); expect(toDb(0)).toBe(-60); expect(toDb(0.5)).toBeCloseTo(-6.02, 2);
    expect(dbPos(-60)).toBe(0); expect(dbPos(0)).toBe(1); expect(ampPos(0.1)).toBeCloseTo(2 / 3, 5);
  });
  it("zones: teal below -12 dB, yellow to -3 dB, red above", () => {
    expect(zone(dbPos(-20))).toBe("lo"); expect(zone(dbPos(-6))).toBe("mid"); expect(zone(dbPos(-1))).toBe("hot");
  });
  it("lights at least one dot for any signal", () => {
    expect(litDots(0, 16)).toBe(0); expect(litDots(0.01, 16)).toBe(1); expect(litDots(1, 16)).toBe(16);
  });
});
