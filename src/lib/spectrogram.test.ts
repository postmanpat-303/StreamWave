import { describe, it, expect } from "vitest";
import { parseSpec, rowFreq, freqBin, lut, renderSpec, freqTicks, minFreq } from "./spectrogram";

function buf(cols: number, bins: number, rate: number, fft: number, f: (c: number, b: number) => number) {
  const b = new ArrayBuffer(16 + cols * bins), v = new DataView(b);
  [cols, bins, rate, fft].forEach((x, i) => v.setUint32(i * 4, x, true));
  const d = new Uint8Array(b, 16); for (let c = 0; c < cols; c++) for (let k = 0; k < bins; k++) d[c * bins + k] = f(c, k);
  return b;
}
describe("spectrogram", () => {
  const s = parseSpec(buf(4, 512, 8000, 1024, (_c, k) => (k === 128 ? 250 : 0)));
  it("parses the backend header", () => {
    expect([s.cols, s.bins, s.rate, s.fft, s.data.length]).toEqual([4, 512, 8000, 1024, 2048]);
    expect(() => parseSpec(buf(2, 2, 1, 4, () => 0).slice(0, 18))).toThrow();
  });
  it("maps rows to frequencies (log and linear), top = Nyquist", () => {
    expect(rowFreq(-0.5, 100, "linear", s)).toBeCloseTo(4000); expect(rowFreq(99.5, 100, "linear", s)).toBeCloseTo(0);
    expect(rowFreq(-0.5, 100, "log", s)).toBeCloseTo(4000); expect(rowFreq(99.5, 100, "log", s)).toBeCloseTo(minFreq(s));
    expect(rowFreq(49.5, 100, "log", s)).toBeCloseTo(Math.sqrt(minFreq(s) * 4000));
    expect(freqBin(1000, s)).toBe(128); expect(freqBin(99999, s)).toBe(511);
  });
  it("colours: dark for silence, bright for loud, three scales", () => {
    for (const c of ["classic", "inferno", "grey"] as const) { const L = lut(c); expect(L[0] + L[1] + L[2]).toBeLessThan(80); expect(L[765] + L[766] + L[767]).toBeGreaterThan(600); }
  });
  it("renders the 1 kHz line at the right height on both scales", () => {
    for (const scale of ["linear", "log"] as const) {
      const h = 200, px = renderSpec(s, h, scale, "grey");
      const lit = [...Array(h).keys()].filter((y) => px[(y * s.cols + 1) * 4] > 200);
      expect(lit.length).toBeGreaterThan(0);
      const y = lit[0], f = rowFreq(y, h, scale, s);
      expect(Math.abs(f - 1000)).toBeLessThan(scale === "linear" ? 25 : 40);
    }
    expect(freqTicks(200, "log", s).map((t) => t.label)).toContain("1k");
  });
});
