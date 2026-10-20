import { describe, it, expect } from "vitest";
import { followStep, inView, FollowState, HeadClock, ANCHOR, USER_HOLD_MS } from "./follow";

const v = (left: number, width = 1000, max = 9000) => ({ left, width, max });

describe("followStep", () => {
  it("does nothing when the whole timeline fits", () => {
    expect(followStep(500, { left: 0, width: 1000, max: 0 }, ANCHOR, 1 / 60).left).toBe(0);
  });
  it("lets the head walk to the anchor before scrolling", () => {
    expect(followStep(200, v(0), ANCHOR, 1 / 60).left).toBe(0);
  });
  it("scrolls continuously with the head past the anchor (smooth, never backwards)", () => {
    let left = 0, anchor = ANCHOR;
    const seen: number[] = [];
    let last = 0;
    for (let px = 300; px < 3000; px += 7) { ({ left, anchor } = followStep(px, v(left), anchor, 1 / 60)); seen.push(left); last = px; }
    expect(seen.every((x, i) => i === 0 || x >= seen[i - 1])).toBe(true);
    expect(Math.max(...seen.map((x, i) => (i ? x - seen[i - 1] : 0)))).toBeLessThanOrEqual(7.01); // moves with the head, no jumps
    expect(last - left).toBeCloseTo(1000 * ANCHOR, 0);
  });
  it("pages to the head when it is outside the view (seek, start elsewhere)", () => {
    expect(followStep(5000, v(0), ANCHOR, 1 / 60).left).toBeCloseTo(5000 - 1000 * ANCHOR);
    expect(followStep(100, v(4000), ANCHOR, 1 / 60).left).toBe(0); // clamped
    expect(followStep(9990, v(0), ANCHOR, 1 / 60).left).toBe(9000); // clamped at the end
  });
  it("after a resume with the head far right the anchor glides back instead of jumping", () => {
    let left = 1000, anchor = 0.9; const head0 = 1000 + 900;
    const r = followStep(head0, v(left), anchor, 1 / 60);
    expect(r.left - 1000).toBeLessThan(6.5); // no jump on the first frame, just the glide
    left = r.left; anchor = r.anchor;
    for (let i = 1; i < 600; i++) ({ left, anchor } = followStep(head0 + i * 4, v(left), anchor, 1 / 60));
    expect(anchor).toBe(ANCHOR);
  });
  it("inView", () => { expect(inView(500, v(0))).toBe(true); expect(inView(1500, v(0))).toBe(false); expect(inView(0.5, v(0))).toBe(false); });
});

describe("FollowState: pauses for manual scroll/drag, resumes on Play or when the head re-enters", () => {
  it("follows by default", () => { expect(new FollowState().active(0, 0.5)).toBe(true); });
  it("pauses while the user scrolls and resumes after if the head is still visible", () => {
    const f = new FollowState(); f.userScroll(1000);
    expect(f.active(1000 + 100, 0.5)).toBe(false);
    expect(f.active(1000 + USER_HOLD_MS + 1, 0.5)).toBe(true);
    expect(f.resumeAnchor).toBeCloseTo(0.5);
  });
  it("stays paused while the head is off-screen, resumes when it re-enters", () => {
    const f = new FollowState(); f.userScroll(0);
    expect(f.active(5000, 1.7)).toBe(false);
    expect(f.active(6000, -0.4)).toBe(false);
    expect(f.active(7000, 0.02)).toBe(true);
  });
  it("pauses for the whole of a drag", () => {
    const f = new FollowState(); f.pointerDown();
    expect(f.active(99999, 0.5)).toBe(false);
    f.pointerUp(100000);
    expect(f.active(100000 + 10, 0.5)).toBe(false);
    expect(f.active(100000 + USER_HOLD_MS + 1, 0.5)).toBe(true);
  });
  it("Play resumes immediately even if the head is off-screen", () => {
    const f = new FollowState(); f.userScroll(0); expect(f.active(10, 3)).toBe(false);
    f.play(); expect(f.active(11, 3)).toBe(true);
  });
});

describe("HeadClock: smooth head between 30 Hz engine reports", () => {
  it("extrapolates while running, capped", () => {
    const c = new HeadClock(); c.report(10, 0, true);
    expect(c.now(16)).toBeCloseTo(10.016, 3);
    expect(c.now(1000)).toBeCloseTo(10.12, 3);
  });
  it("never steps back by a hair, but follows a seek", () => {
    const c = new HeadClock(); c.report(10, 0, true); expect(c.now(30)).toBeCloseTo(10.03);
    c.report(10.02, 33, true); expect(c.now(34)).toBeCloseTo(10.03); // late report: hold, don't jitter back
    c.report(3, 66, true); expect(c.now(66)).toBe(3);
  });
  it("holds still while paused", () => {
    const c = new HeadClock(); c.report(5, 0, false); expect(c.now(500)).toBe(5);
  });
});
