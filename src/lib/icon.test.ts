import { describe, expect, it } from "vitest";
import icoDataUrl from "../../src-tauri/icons/icon.ico?inline";

// Windows Explorer/Edge often can't draw PNG-compressed frames at small sizes and fall back to the
// generic installer icon. Keep 16-128 px as BMP (only 256 px may be PNG). `npx tauri icon` writes
// all-PNG .ico files, so don't let it overwrite src-tauri/icons/icon.ico.
describe("Windows icon (src-tauri/icons/icon.ico)", () => {
  it("has 16, 24, 32, 48, 64, 128 px as BMP and 256 px", () => {
    const bin = atob(icoDataUrl.slice(icoDataUrl.indexOf(",") + 1));
    const b = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    const v = new DataView(b.buffer);
    expect(v.getUint16(2, true)).toBe(1); // icon
    const n = v.getUint16(4, true);
    const PNG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    const frames = new Map<number, string>();
    for (let i = 0; i < n; i++) {
      const e = 6 + 16 * i;
      const size = b[e] || 256;
      const off = v.getUint32(e + 12, true);
      frames.set(size, PNG.every((x, j) => b[off + j] === x) ? "png" : "bmp");
    }
    for (const s of [16, 24, 32, 48, 64, 128]) expect(frames.get(s), `${s}px`).toBe("bmp");
    expect(frames.has(256)).toBe(true);
  });
});
