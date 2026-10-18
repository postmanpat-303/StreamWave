// Spectrogram view helpers: decode the backend's binary spectrogram, map rows to frequencies
// (log or linear) and colour it.
import type { SpecColors, SpecScale } from "./editModel";

export interface Spec { cols: number; bins: number; rate: number; fft: number; data: Uint8Array; }
export const FLOOR_DB = -100;

export function parseSpec(buf: ArrayBuffer): Spec {
  const h = new DataView(buf, 0, 16);
  const [cols, bins, rate, fft] = [0, 4, 8, 12].map((o) => h.getUint32(o, true));
  const data = new Uint8Array(buf, 16);
  if (data.length !== cols * bins) throw new Error("bad spectrogram data");
  return { cols, bins, rate, fft, data };
}

/** Lowest frequency shown on the log scale. */
export const minFreq = (s: Pick<Spec, "rate" | "fft">) => Math.max(20, s.rate / s.fft);
/** Frequency (Hz) at row y of h rows, y = 0 at the top. */
export function rowFreq(y: number, h: number, scale: SpecScale, s: Pick<Spec, "rate" | "fft">): number {
  const ny = s.rate / 2, f = 1 - (y + 0.5) / h;
  if (scale === "linear") return f * ny;
  const lo = minFreq(s);
  return lo * Math.pow(ny / lo, f);
}
export const freqBin = (hz: number, s: Pick<Spec, "rate" | "fft" | "bins">) => Math.max(0, Math.min(s.bins - 1, Math.round((hz / s.rate) * s.fft)));

type Stop = [number, number, number, number]; // pos 0..1, r, g, b
const STOPS: Record<SpecColors, Stop[]> = {
  // dark blue -> purple -> red -> yellow -> white (the familiar "classic" editor look)
  classic: [[0, 10, 12, 30], [0.25, 40, 30, 120], [0.5, 170, 40, 120], [0.72, 240, 90, 40], [0.88, 255, 210, 60], [1, 255, 255, 240]],
  inferno: [[0, 0, 0, 4], [0.25, 87, 16, 110], [0.5, 188, 55, 84], [0.75, 249, 142, 9], [1, 252, 255, 164]],
  grey: [[0, 8, 8, 10], [1, 255, 255, 255]],
};
/** 256-entry RGB lookup table for a colour scale. */
export function lut(c: SpecColors): Uint8Array {
  const st = STOPS[c] ?? STOPS.classic, out = new Uint8Array(256 * 3);
  for (let i = 0; i < 256; i++) {
    const x = i / 255; let k = 0; while (k < st.length - 2 && x > st[k + 1][0]) k++;
    const [p0, r0, g0, b0] = st[k], [p1, r1, g1, b1] = st[k + 1], t = Math.min(1, Math.max(0, (x - p0) / (p1 - p0)));
    out.set([r0 + (r1 - r0) * t, g0 + (g1 - g0) * t, b0 + (b1 - b0) * t].map(Math.round), i * 3);
  }
  return out;
}

/** RGBA pixels (cols x h) for the spectrogram. Rows covering several bins take the loudest one. */
export function renderSpec(s: Spec, h: number, scale: SpecScale, colors: SpecColors): Uint8ClampedArray {
  const px = new Uint8ClampedArray(s.cols * h * 4), L = lut(colors);
  const rows: [number, number][] = [];
  for (let y = 0; y < h; y++) {
    const a = freqBin(rowFreq(y + 0.5, h, scale, s), s), b = freqBin(rowFreq(y - 0.5, h, scale, s), s);
    rows.push([Math.min(a, b), Math.max(a, b)]);
  }
  for (let x = 0; x < s.cols; x++) {
    const col = x * s.bins;
    for (let y = 0; y < h; y++) {
      const [a, b] = rows[y]; let v = 0;
      for (let k = a; k <= b; k++) v = Math.max(v, s.data[col + k]);
      const o = (y * s.cols + x) * 4;
      px[o] = L[v * 3]; px[o + 1] = L[v * 3 + 1]; px[o + 2] = L[v * 3 + 2]; px[o + 3] = 255;
    }
  }
  return px;
}

/** Frequency axis ticks that fit in h px. */
export function freqTicks(h: number, scale: SpecScale, s: Pick<Spec, "rate" | "fft">): { y: number; label: string }[] {
  const ny = s.rate / 2;
  const cand = scale === "log" ? [50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000] : [1000, 2000, 5000, 10000, 15000, 20000].concat(ny < 8000 ? [500, 1500, 2500, 3000, 3500] : []);
  const lo = minFreq(s), y = (f: number) => (scale === "linear" ? (1 - f / ny) * h : (1 - Math.log(f / lo) / Math.log(ny / lo)) * h);
  const out: { y: number; label: string }[] = [];
  for (const f of cand.filter((f) => f < ny && f > lo).sort((a, b) => a - b)) {
    const yy = y(f); if (out.some((t) => Math.abs(t.y - yy) < 14) || yy < 8 || yy > h - 4) continue;
    out.push({ y: yy, label: f >= 1000 ? `${f / 1000}k` : `${f}` });
  }
  return out;
}
