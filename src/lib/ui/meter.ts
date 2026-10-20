/** Shared meter maths: linear amplitude → dBFS → 0..1 position on a -60..0 dB scale. */
export const FLOOR_DB = -60;
export const toDb = (v: number) => (v <= 0.00001 ? FLOOR_DB : Math.max(FLOOR_DB, 20 * Math.log10(v)));
export const dbPos = (db: number) => Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB));
export const ampPos = (v: number) => dbPos(toDb(v));
/** Colour zone of a meter position: green/teal up to -12 dB, yellow to -3 dB, red above. */
export const zone = (pos: number) => (pos >= dbPos(-3) ? "hot" : pos >= dbPos(-12) ? "mid" : "lo");
/** Dots lit for a 0..1 position (any signal above the floor lights at least one). */
export const litDots = (pos: number, n: number) => (pos <= 0 ? 0 : Math.max(1, Math.round(pos * n)));
