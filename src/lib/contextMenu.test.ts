import { describe, it, expect } from "vitest";
import { audioMenu, labelMenu, emptyMenu, nextEnabled, placeMenu, findItem, isBrowserShortcut, isEntry, type MenuItem } from "./contextMenu";
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
    expect(placeMenu(10, 10, 200, 300, 1000, 800)).toEqual({ x: 10, y: 10 });
    expect(placeMenu(900, 700, 200, 300, 1000, 800)).toEqual({ x: 700, y: 496 });
  });
  it("swallows browser shortcuts but not app or text-editing keys", () => {
    const k = (key: string, o: Partial<KeyboardEvent> = {}) => isBrowserShortcut({ key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...o }, false);
    expect([k("F5"), k("r", { ctrlKey: true }), k("p", { ctrlKey: true }), k("f", { ctrlKey: true }), k("=", { ctrlKey: true }), k("0", { ctrlKey: true }), k("i", { ctrlKey: true, shiftKey: true }), k("ArrowLeft", { altKey: true }), k("F12")]).toEqual(Array(9).fill(true));
    expect([k("c", { ctrlKey: true }), k("v", { ctrlKey: true }), k("x", { ctrlKey: true }), k("z", { ctrlKey: true }), k("a", { ctrlKey: true }), k("k", { ctrlKey: true }), k("b", { ctrlKey: true }), k("r"), k("m"), k(" ")]).toEqual(Array(10).fill(false));
    expect(isBrowserShortcut({ key: "F12", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false }, true)).toBe(false);
  });
});
