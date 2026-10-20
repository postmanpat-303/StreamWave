/** Pure maths for the rotary Knob component (kept out of the .svelte file so it is unit-tested). */
export type KnobRange = { min: number; max: number; step: number; log?: boolean };

/** Sweep of the knob: 270° from -135° (min, lower left) to +135° (max, lower right), 0° = straight up. */
export const SWEEP = 270;
export const START = -135;

/** Use a logarithmic taper for wide positive ranges such as frequencies (20 Hz – 20 kHz). */
export const autoLog = (min: number, max: number, unit = "") => min > 0 && max / min >= 20 && /hz/i.test(unit);

const clamp = (x: number, a: number, b: number) => Math.min(b, Math.max(a, x));

/** value → position 0..1 */
export function toNorm(v: number, r: KnobRange): number {
  if (r.max === r.min) return 0;
  const x = clamp(v, r.min, r.max);
  return r.log && r.min > 0 ? Math.log(x / r.min) / Math.log(r.max / r.min) : (x - r.min) / (r.max - r.min);
}

/** Round to the step grid (counted from min) and clamp; avoids float noise such as 0.30000000000000004. */
export function snap(v: number, r: KnobRange): number {
  const x = clamp(v, r.min, r.max);
  if (!(r.step > 0)) return x;
  const n = Math.round((x - r.min) / r.step);
  const dec = Math.max(0, -Math.floor(Math.log10(r.step) + 1e-9)) + 2;
  return clamp(+(r.min + n * r.step).toFixed(Math.min(10, dec)), r.min, r.max);
}

/** position 0..1 → snapped value */
export function fromNorm(t: number, r: KnobRange): number {
  const u = clamp(t, 0, 1);
  const v = r.log && r.min > 0 ? r.min * Math.pow(r.max / r.min, u) : r.min + u * (r.max - r.min);
  return snap(v, r);
}

export const angleOf = (t: number) => START + SWEEP * clamp(t, 0, 1);

/** Pixels of drag for the full sweep (Shift = 10× finer). */
export const DRAG_PX = 200;

/** New value after dragging dx (right = up) and dy (screen pixels, down = positive) from a start value. */
export function dragTo(start: number, dx: number, dy: number, r: KnobRange, fine = false): number {
  const d = (dx - dy) / (DRAG_PX * (fine ? 10 : 1));
  return fromNorm(toNorm(start, r) + d, r);
}

/** One wheel notch / arrow key = 1 % of the sweep (at least one step); PageUp/Down = 10 %. Shift = finer. */
export function nudge(v: number, dir: number, r: KnobRange, big = false, fine = false): number {
  const t = toNorm(v, r);
  const pct = big ? 0.1 : fine ? 0.002 : 0.01;
  let nv = fromNorm(t + dir * pct, r);
  if (nv === snap(v, r) && r.step > 0) nv = snap(v + dir * r.step, r); // always move at least one step
  return nv;
}

/** Keyboard handling: returns the new value, or null if the key isn't a knob key. */
export function keyValue(key: string, v: number, r: KnobRange, shift = false, def?: number): number | null {
  switch (key) {
    case "ArrowUp": case "ArrowRight": return nudge(v, 1, r, false, shift);
    case "ArrowDown": case "ArrowLeft": return nudge(v, -1, r, false, shift);
    case "PageUp": return nudge(v, 1, r, true);
    case "PageDown": return nudge(v, -1, r, true);
    case "Home": return snap(r.min, r);
    case "End": return snap(r.max, r);
    case "Delete": case "Backspace": return def === undefined ? null : snap(def, r);
    default: return null;
  }
}

/** How many of n tick dots are lit for a value (the first dot is lit from just above min). */
export const litTicks = (v: number, r: KnobRange, n: number) => Math.round(toNorm(v, r) * (n - 1)) + (toNorm(v, r) > 0 ? 1 : 0);

/** Lit tick indices [from, to] (inclusive), or null for none. Bipolar ranges (min < 0 < max, linear) light from the 0 tick
 *  towards the value, like a pan/gain knob; others light from the start. */
export function litRange(v: number, r: KnobRange, n: number): [number, number] | null {
  if (r.min < 0 && r.max > 0 && !r.log) {
    const c = Math.round(toNorm(0, r) * (n - 1)), i = Math.round(toNorm(v, r) * (n - 1));
    return [Math.min(c, i), Math.max(c, i)];
  }
  const k = litTicks(v, r, n); return k > 0 ? [0, k - 1] : null;
}

/** Compact number for scale labels: 20, 400, 1.5k, 12k, 0.3, -12. */
export function short(v: number): string {
  const a = Math.abs(v);
  if (a >= 1000) return `${+(v / 1000).toFixed(a >= 10000 ? 0 : 1)}k`;
  if (a >= 100 || Number.isInteger(v)) return `${Math.round(v)}`;
  return `${+v.toFixed(a < 1 ? 2 : 1)}`;
}

/** Value labels for the scale ring: n evenly spaced positions (log-aware), each [position 0..1, text]. */
export function scaleLabels(r: KnobRange, n = 5): [number, string][] {
  return Array.from({ length: n }, (_, i) => { const t = i / (n - 1); return [t, short(fromNorm(t, { ...r, step: 0 }))] as [number, string]; });
}

/** Readout text: "-12.0 dB", "1.5 kHz", "3.0 :1". */
export function readout(v: number, unit = ""): string {
  if (/^hz$/i.test(unit) && Math.abs(v) >= 1000) return `${+(v / 1000).toFixed(2)} kHz`;
  const s = Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(unit ? 1 : 2);
  return unit ? `${s} ${unit}` : s;
}
