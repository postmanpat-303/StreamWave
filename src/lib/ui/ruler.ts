/** Timeline ruler maths (pure, tested). Times are seconds on the shared timeline; pxPerSec is the lanes' scale. */
export type Tick = { t: number; major: boolean; label?: string };

/** Label steps (s) with how many minor divisions each gets. */
const STEPS: [number, number][] = [
  [0.01, 5], [0.02, 4], [0.05, 5], [0.1, 5], [0.2, 4], [0.5, 5], [1, 5], [2, 4], [5, 5], [10, 5], [15, 3], [30, 6],
  [60, 6], [120, 4], [300, 5], [600, 5], [900, 3], [1800, 6], [3600, 6],
];

/** Smallest label step whose labels are at least `minLabelPx` apart. */
export function pickStep(pxPerSec: number, minLabelPx = 72): [number, number] {
  for (const s of STEPS) if (s[0] * pxPerSec >= minLabelPx) return s;
  return STEPS[STEPS.length - 1];
}

/** m:ss (or h:mm:ss), with as many decimals as the step needs. */
export function fmtRuler(t: number, step: number, long = false): string {
  const dec = step < 0.1 ? 2 : step < 1 ? 1 : 0;
  const k = 10 ** dec; let v = Math.round(t * k) / k; if (Math.abs(v) < 1e-9) v = 0;
  const h = Math.floor(v / 3600), m = Math.floor((v % 3600) / 60), s = v - h * 3600 - m * 60;
  const ss = (dec ? s.toFixed(dec) : String(Math.round(s))).padStart(dec ? 3 + dec : 2, "0");
  return long || h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Ticks from 0 to `duration`: a labelled major tick every step, unlabelled minor ticks between. */
export function rulerTicks(duration: number, pxPerSec: number, minLabelPx = 72): Tick[] {
  if (!(duration > 0) || !(pxPerSec > 0)) return [];
  const [step, div] = pickStep(pxPerSec, minLabelPx);
  const minor = step / div, n = Math.floor(duration / minor + 1e-6), long = duration >= 3600, out: Tick[] = [];
  for (let i = 0; i <= n && i < 20000; i++) {
    const t = Math.round(i * minor * 1e6) / 1e6, major = i % div === 0;
    out.push(major ? { t, major, label: fmtRuler(t, step, long) } : { t, major });
  }
  return out;
}
