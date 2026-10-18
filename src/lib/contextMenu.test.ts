import { describe, it, expect } from "vitest";
import { audioMenu, labelMenu, emptyMenu, nextEnabled, placeMenu, placeSubmenu, inTriangle, aimingAt, findItem, isBrowserShortcut, isEntry, type MenuItem } from "./contextMenu";
import { FX_NAMES } from "./fx";

const ctx = { hasRange: false, hasClipboard: false, canJoin: false, canSplit: true, muted: false, solo: true, spectrogram: false, tracks: 1, trackName: "Mic" };
const dis = (items: MenuItem[], id: string) => findItem(items, id)!.disabled ?? false;

describe("context menus", () => {
  it("disables what doesn't apply", () => {
    const m = audioMenu(ctx);
    expect(["cut", "copy", "delete", "trim", "silence", "paste", "join"].map((id) => dis(m, id))).toEqual([true, true, true, true, true, true, true]);
    expect(["split", "selectclip", "addlabel", "marker", "fx:Gain", "fadein", "normalize", "studio", "rename", "deletetrack"].every((id) => !dis(m, id))).toBe(true);
    const r = audioMenu({ ...ctx, hasRange: true, hasClipboard: true, canJoin: true, tracks: 3 });
    expect(["cut", "copy", "delete", "trim", "silence", "paste", "join"].every((id) => !dis(r, id))).toBe(true);
    expect(findItem(r, "cut")!.label).toBe("Cut (3 tracks)");
    expect(findItem(m, "solo")!.checked).toBe(true);
  });
  it("lists every effect once in the Effects submenu, plus fades / normalize / Studio Sound", () => {
    const fx = findItem(audioMenu(ctx), "effects")!.submenu!.filter(isEntry).map((e) => e.id);
    const all = Object.keys(FX_NAMES).filter((t) => t !== "Fade").map((t) => `fx:${t}`);
    expect(fx.filter((id) => id.startsWith("fx:")).sort()).toEqual(all.sort());
    expect(fx).toEqual(expect.arrayContaining(["fadein", "fadeout", "normalize", "studio"]));
  });
  it("shows shortcut hints that match the app's keys", () => {
    const m = audioMenu(ctx);
    expect(["cut", "copy", "paste", "split", "join", "addlabel"].map((id) => findItem(m, id)!.shortcut!.replace("⌘", "Ctrl+"))).toEqual(["Ctrl+X", "Ctrl+C", "Ctrl+V", "Ctrl+K", "Ctrl+J", "Ctrl+B"]);
  });
  it("label and empty-area menus", () => {
    expect(labelMenu({ onLabel: true, hasRange: false, trackName: "L" }).filter(isEntry).map((e) => e.id).slice(0, 3)).toEqual(["editlabel", "selectlabel", "deletelabel"]);
    expect(findItem(labelMenu({ onLabel: false, hasRange: false, trackName: "L" }), "deletelabel")).toBeUndefined();
    expect(dis(emptyMenu({ hasClipboard: false }), "paste")).toBe(true);
    expect(emptyMenu({ hasClipboard: true }).filter(isEntry).map((e) => e.id)).toEqual(["paste", "import", "importaup3", "labeltrack"]);
  });
  it("keyboard navigation skips separators and disabled items and wraps", () => {
    const m = audioMenu(ctx); // cut/copy/paste/delete disabled, then separator, split...
    expect(nextEnabled(m, -1, 1)).toBe(5);
    expect(nextEnabled(m, 5, -1)).toBe(m.length - 1);
    expect(nextEnabled([{ separator: true }, { id: "a", label: "a", disabled: true }], 0, 1)).toBe(-1);
  });
  it("menus stay on screen", () => {
    expect(placeMenu(10, 10, 200, 300, 1000, 800)).toEqual({ x: 10, y: 10, maxH: 792, side: "right" });
    // near the bottom-right corner: flips up and left at the pointer
    expect(placeMenu(900, 700, 200, 300, 1000, 800)).toEqual({ x: 700, y: 400, maxH: 792, side: "left" });
  });
  it("keeps the main menu inside small windows", () => {
    // 900x600 window, right-click near the bottom: opens upwards from the pointer
    const p = placeMenu(225, 560, 240, 420, 900, 600);
    expect(p.y).toBe(140); expect(p.y + 420).toBeLessThanOrEqual(596);
    // not enough room above or below the pointer: pinned to the bottom edge, fully visible
    const q = placeMenu(100, 300, 240, 420, 900, 600);
    expect(q.y).toBe(176); expect(q.y + 420).toBe(596);
    // taller than the window: top margin + max height (scrolls)
    const r = placeMenu(100, 300, 240, 700, 900, 600);
    expect(r).toMatchObject({ y: 4, maxH: 592 });
    // wider than the room on both sides: clamped inside
    expect(placeMenu(50, 10, 240, 100, 260, 600).x).toBe(16);
  });
  it("places submenus beside the parent, flipping and shifting up, never over the parent items", () => {
    const parent = { left: 200, top: 150, right: 440, bottom: 596 }, item = { left: 205, top: 540, right: 435, bottom: 568 };
    const s = placeSubmenu(parent, item, 230, 430, 900, 600);
    expect(s.side).toBe("right"); expect(s.x).toBe(437); // 3px into the parent's 5px padding: no hover gap
    expect(s.y + 430).toBe(596); // shifted up so the bottom fits
    expect(s.x).toBeGreaterThan(parent.right - 5);
    // no room on the right: flips left of the parent
    const p2 = { left: 620, top: 150, right: 860, bottom: 596 };
    const l = placeSubmenu(p2, { ...item, left: 625, right: 855 }, 230, 430, 900, 600);
    expect(l.side).toBe("left"); expect(l.x + 230).toBe(623); expect(l.x + 230).toBeLessThan(p2.left + 5);
    // aligned with the item when it fits
    expect(placeSubmenu({ left: 10, top: 10, right: 250, bottom: 300 }, { left: 15, top: 100, right: 245, bottom: 128 }, 230, 200, 900, 600).y).toBe(95);
    // taller than the window: scrolls
    expect(placeSubmenu(parent, item, 230, 900, 900, 600)).toMatchObject({ y: 4, maxH: 592 });
  });
  it("hover intent: moving diagonally towards the submenu keeps it open", () => {
    expect(inTriangle({ x: 1, y: 1 }, { x: 0, y: 0 }, { x: 4, y: 0 }, { x: 0, y: 4 })).toBe(true);
    expect(inTriangle({ x: 3, y: 3 }, { x: 0, y: 0 }, { x: 4, y: 0 }, { x: 0, y: 4 })).toBe(false);
    const sub = { left: 437, top: 166, right: 667, bottom: 596 };
    const from = { x: 380, y: 555 }; // over "Effects"
    expect(aimingAt(from, { x: 400, y: 530 }, sub, "right")).toBe(true);  // up-right, over "Silence selection"
    expect(aimingAt(from, { x: 410, y: 575 }, sub, "right")).toBe(true);  // down-right, over "Track"
    expect(aimingAt(from, { x: 380, y: 520 }, sub, "right")).toBe(false); // straight up: switching items
    expect(aimingAt(from, { x: 300, y: 560 }, sub, "right")).toBe(false); // moving away
    expect(aimingAt(from, { x: 500, y: 300 }, sub, "right")).toBe(true);  // inside the submenu
    const subL = { left: 390, top: 166, right: 620, bottom: 596 };
    expect(aimingAt({ x: 700, y: 555 }, { x: 680, y: 530 }, subL, "left")).toBe(true);
  });
  it("swallows browser shortcuts but not app or text-editing keys", () => {
    const k = (key: string, o: Partial<KeyboardEvent> = {}) => isBrowserShortcut({ key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...o }, false);
    expect([k("F5"), k("r", { ctrlKey: true }), k("p", { ctrlKey: true }), k("f", { ctrlKey: true }), k("=", { ctrlKey: true }), k("0", { ctrlKey: true }), k("i", { ctrlKey: true, shiftKey: true }), k("ArrowLeft", { altKey: true }), k("F12")]).toEqual(Array(9).fill(true));
    expect([k("c", { ctrlKey: true }), k("v", { ctrlKey: true }), k("x", { ctrlKey: true }), k("z", { ctrlKey: true }), k("a", { ctrlKey: true }), k("k", { ctrlKey: true }), k("b", { ctrlKey: true }), k("r"), k("m"), k(" ")]).toEqual(Array(10).fill(false));
    expect(isBrowserShortcut({ key: "F12", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false }, true)).toBe(false);
  });
});
