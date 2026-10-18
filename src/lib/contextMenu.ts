// Right-click menus: item model, keyboard navigation, placement, the menu contents for each
// timeline area, and which browser shortcuts an app window should swallow.
import { FX_NAMES, type EffectType } from "./fx";

export type MenuItem =
  | { separator: true }
  | { id: string; label: string; shortcut?: string; disabled?: boolean; checked?: boolean; danger?: boolean; submenu?: MenuItem[] };
export type Entry = Exclude<MenuItem, { separator: true }>;
export const isEntry = (m: MenuItem): m is Entry => !("separator" in m);
const sep: MenuItem = { separator: true };

/** Next focusable index from i in direction dir (wraps; skips separators and disabled items). -1 if none. */
export function nextEnabled(items: MenuItem[], i: number, dir: 1 | -1): number {
  const n = items.length;
  for (let k = 1; k <= n; k++) {
    const j = (((i + dir * k) % n) + n) % n; const m = items[j];
    if (isEntry(m) && !m.disabled) return j;
  }
  return -1;
}
export interface Pt { x: number; y: number }
export interface Rect { left: number; top: number; right: number; bottom: number }
export interface Placement { x: number; y: number; maxH: number; side: "right" | "left" }
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Place a w x h menu opened at the pointer (x, y) inside a vw x vh viewport (margin m).
 *  Opens right/down; flips left/up at the pointer when there is no room; if it is taller
 *  than the viewport it gets maxH (and scrolls). */
export function placeMenu(x: number, y: number, w: number, h: number, vw: number, vh: number, m = 4): Placement {
  const maxH = Math.max(40, vh - 2 * m), hh = Math.min(h, maxH);
  let px = x, side: Placement["side"] = "right";
  if (x + w + m > vw) { side = "left"; px = x - w >= m ? x - w : vw - m - w; }
  let py = y;
  if (y + hh + m > vh) py = y - hh >= m ? y - hh : vh - m - hh;
  return { x: clamp(px, m, Math.max(m, vw - m - w)), y: clamp(py, m, Math.max(m, vh - m - hh)), maxH, side };
}

/** Place a w x h submenu next to its parent menu (rect `parent`) and the item that opened it (rect `item`).
 *  Prefers the right side, flips to the left when it doesn't fit, aligns its first item with the parent item
 *  and shifts up so the bottom fits; scrolls (maxH) when taller than the viewport. It overlaps the parent by
 *  `overlap` px (inside the parent's padding, never over its items) so there is no hover gap. */
export function placeSubmenu(parent: Rect, item: Rect, w: number, h: number, vw: number, vh: number, m = 4, overlap = 3, pad = 5): Placement {
  const maxH = Math.max(40, vh - 2 * m), hh = Math.min(h, maxH);
  const roomR = vw - m - (parent.right - overlap), roomL = parent.left + overlap - m;
  const side: Placement["side"] = w <= roomR ? "right" : w <= roomL ? "left" : roomR >= roomL ? "right" : "left";
  const x = side === "right" ? parent.right - overlap : parent.left + overlap - w;
  const y = clamp(item.top - pad, m, Math.max(m, vh - m - hh));
  return { x: clamp(x, m, Math.max(m, vw - m - w)), y, maxH, side };
}

/** Is p inside triangle abc (edges included)? */
export function inTriangle(p: Pt, a: Pt, b: Pt, c: Pt): boolean {
  const s = (p1: Pt, p2: Pt, p3: Pt) => (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y);
  const d1 = s(p, a, b), d2 = s(p, b, c), d3 = s(p, c, a);
  return !((d1 < 0 || d2 < 0 || d3 < 0) && (d1 > 0 || d2 > 0 || d3 > 0));
}
/** Hover intent ("safe triangle"): is the pointer p, which was last over the submenu's parent item at `from`,
 *  heading into the open submenu `sub` (on `side`)? Then crossing other items shouldn't switch menus. */
export function aimingAt(from: Pt, p: Pt, sub: Rect, side: "right" | "left", tol = 6): boolean {
  if (p.x >= sub.left && p.x <= sub.right && p.y >= sub.top && p.y <= sub.bottom) return true;
  const nx = side === "right" ? sub.left : sub.right;
  return inTriangle(p, from, { x: nx, y: sub.top - tol }, { x: nx, y: sub.bottom + tol });
}

/** Find an item (also inside submenus) by id. */
export function findItem(items: MenuItem[], id: string): Entry | undefined {
  for (const m of items) { if (!isEntry(m)) continue; if (m.id === id) return m; const s = m.submenu && findItem(m.submenu, id); if (s) return s; }
}

const MOD = typeof navigator !== "undefined" && /Mac/.test(navigator.platform) ? "⌘" : "Ctrl+";
export const FX_MENU_ORDER: EffectType[] = ["Gain", "Eq", "Compressor", "Gate", "NoiseReduction", "DeEsser", "Denoise", "Limiter", "Loudness", "Normalize", "Fade"];

export interface AudioCtx {
  hasRange: boolean; hasClipboard: boolean; canJoin: boolean; canSplit: boolean; playing?: boolean;
  muted: boolean; solo: boolean; spectrogram: boolean; tracks: number; trackName: string;
}
/** Menu for the waveform / clip area of an audio track. Ids are run by the app. */
export function audioMenu(c: AudioCtx): MenuItem[] {
  const r = !c.hasRange, multi = c.tracks > 1 ? ` (${c.tracks} tracks)` : "";
  return [
    { id: "cut", label: "Cut" + multi, shortcut: MOD + "X", disabled: r },
    { id: "copy", label: "Copy" + multi, shortcut: MOD + "C", disabled: r },
    { id: "paste", label: "Paste", shortcut: MOD + "V", disabled: !c.hasClipboard },
    { id: "delete", label: "Delete", shortcut: "Del", disabled: r },
    sep,
    { id: "split", label: c.hasRange ? "Split at selection edges" : "Split at cursor", shortcut: MOD + "K", disabled: !c.canSplit },
    { id: "join", label: "Join clips", shortcut: MOD + "J", disabled: !c.canJoin },
    { id: "selectclip", label: "Select whole clip", shortcut: "Double-click" },
    sep,
    { id: "addlabel", label: c.hasRange ? "Add label for selection" : "Add label here", shortcut: MOD + "B" },
    { id: "marker", label: "Add marker here", shortcut: "M" },
    sep,
    { id: "trim", label: "Trim to selection", disabled: r },
    { id: "silence", label: "Silence selection", disabled: r },
    { id: "effects", label: "Effects", submenu: [
      ...FX_MENU_ORDER.filter((t) => t !== "Fade").map((t): MenuItem => ({ id: `fx:${t}`, label: FX_NAMES[t] + "…" })),
      sep,
      { id: "fadein", label: "Fade in" + (c.hasRange ? "" : " (first 1 s)") },
      { id: "fadeout", label: "Fade out" + (c.hasRange ? "" : " (last 1 s)") },
      { id: "normalize", label: "Normalize to −1 dB" },
      { id: "studio", label: "✨ Studio Sound (whole track)" },
    ] },
    { id: "track", label: "Track", submenu: [
      { id: "rename", label: `Rename “${c.trackName}”…` },
      { id: "mute", label: "Mute", checked: c.muted },
      { id: "solo", label: "Solo", checked: c.solo },
      { id: "toggleview", label: "Spectrogram view", checked: c.spectrogram, shortcut: MOD + "Shift+G" },
      sep,
      { id: "deletetrack", label: "Delete track", danger: true },
    ] },
  ];
}
/** Menu on a label track: on a label, or on the empty lane. */
export function labelMenu(c: { onLabel: boolean; hasRange: boolean; trackName: string }): MenuItem[] {
  return [
    ...(c.onLabel ? [
      { id: "editlabel", label: "Edit label text" },
      { id: "selectlabel", label: "Select label range" },
      { id: "deletelabel", label: "Delete label", danger: true },
      sep] as MenuItem[] : []),
    { id: "addlabel", label: c.hasRange ? "Add label for selection" : "Add label here", shortcut: MOD + "B" },
    { id: "labelsmarkers", label: "Copy labels to markers" },
    { id: "exportlabels", label: "Export labels…" },
    sep,
    { id: "rename", label: `Rename “${c.trackName}”…` },
    { id: "deletetrack", label: "Delete label track", danger: true },
  ];
}
/** Menu on the empty timeline area. */
export function emptyMenu(c: { hasClipboard: boolean }): MenuItem[] {
  return [
    { id: "paste", label: "Paste as new track", shortcut: MOD + "V", disabled: !c.hasClipboard },
    sep,
    { id: "import", label: "Add track: import audio…", shortcut: MOD + "I" },
    { id: "importaup3", label: "Import Audacity project…" },
    { id: "labeltrack", label: "New label track" },
  ];
}

/** Browser shortcuts that make no sense in an app window (reload, print, find, page zoom, history, devtools in release). */
export function isBrowserShortcut(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">, dev: boolean): boolean {
  const mod = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
  if (["f5", "f3", "f7", "browserback", "browserforward", "browserrefresh"].includes(k)) return true;
  if (k === "f12" && !dev) return true;
  if (e.altKey && (k === "arrowleft" || k === "arrowright")) return true;
  if (!mod) return false;
  if (["r", "p", "f", "g", "h", "u", "=", "+", "-", "_", "0", "f5"].includes(k) && !e.altKey) return true;
  if (e.shiftKey && ["i", "c"].includes(k) && !dev) return true; // devtools
  return false;
}
