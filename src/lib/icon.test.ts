import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Windows Explorer/Edge often can't draw PNG-compressed frames at small sizes and fall back to the
// generic installer icon. Keep 16-128 px as BMP (only 256 px may be PNG). `npx tauri icon` writes
// all-PNG .ico files, so don't let it overwrite src-tauri/icons/icon.ico.
describe("Windows icon (src-tauri/icons/icon.ico)", () => {
  it("has 16, 24, 32, 48, 64, 128 px as BMP and 256 px", () => {
    const d = readFileSync(fileURLToPath(new URL("../../src-tauri/icons/icon.ico", import.meta.url)));
    expect(d.readUInt16LE(2)).toBe(1); // icon
    const n = d.readUInt16LE(4);
    const frames = new Map<number, string>();
    for (let i = 0; i < n; i++) {
      const e = 6 + 16 * i;
      const size = d[e] || 256;
      const off = d.readUInt32LE(e + 12);
      const png = d.subarray(off, off + 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
      frames.set(size, png ? "png" : "bmp");
    }
    for (const s of [16, 24, 32, 48, 64, 128]) expect(frames.get(s), `${s}px`).toBe("bmp");
    expect(frames.has(256)).toBe(true);
  });
});
