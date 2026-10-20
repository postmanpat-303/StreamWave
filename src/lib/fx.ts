// Effect chain definitions shared with the Rust `fx` module (serde tag = "type").
export interface EqBand { freq: number; gain_db: number; q: number; }
export type Effect =
  | { type: "Gain"; db: number }
  | { type: "Eq"; hpf_hz: number; hpf_on: boolean; low_shelf: EqBand; peaks: [EqBand, EqBand, EqBand]; high_shelf: EqBand }
  | { type: "Compressor"; threshold_db: number; ratio: number; attack_ms: number; release_ms: number; makeup_db: number }
  | { type: "Gate"; threshold_db: number; floor_db: number; attack_ms: number; hold_ms: number; release_ms: number }
  | { type: "NoiseReduction"; reduction_db: number; sensitivity_db: number; profile: number[] }
  | { type: "Normalize"; peak_db: number }
  | { type: "Loudness"; target_lufs: number; ceiling_db?: number | null }
  | { type: "Fade"; in_ms: number; out_ms: number }
  | { type: "Limiter"; ceiling_db: number; release_ms: number }
  | { type: "DeEsser"; freq_hz: number; threshold_db: number; max_reduction_db: number }
  | { type: "Denoise"; mix: number };
export type EffectType = Effect["type"];
export interface FxSlot { id: number; bypass: boolean; fx: Effect; }

export const FX_NAMES: Record<EffectType, string> = {
  Gain: "Gain", Eq: "Equalizer", Compressor: "Compressor", Gate: "Noise gate", NoiseReduction: "Noise reduction",
  Normalize: "Normalize (peak)", Loudness: "Loudness (LUFS)", Fade: "Fade in / out", Limiter: "Limiter",
  DeEsser: "De-esser", Denoise: "AI denoise (RNNoise)",
};

const band = (freq: number, gain_db = 0, q = 1): EqBand => ({ freq, gain_db, q });
export function defaultFx(t: EffectType): Effect {
  switch (t) {
    case "Gain": return { type: t, db: 0 };
    case "Eq": return { type: t, hpf_hz: 80, hpf_on: true, low_shelf: band(120), peaks: [band(300), band(1500), band(4000)], high_shelf: band(9000) };
    case "Compressor": return { type: t, threshold_db: -20, ratio: 3, attack_ms: 10, release_ms: 120, makeup_db: 3 };
    case "Gate": return { type: t, threshold_db: -50, floor_db: -30, attack_ms: 2, hold_ms: 80, release_ms: 150 };
    case "NoiseReduction": return { type: t, reduction_db: 18, sensitivity_db: 6, profile: [] };
    case "Normalize": return { type: t, peak_db: -1 };
    case "Loudness": return { type: t, target_lufs: -16 };
    case "Fade": return { type: t, in_ms: 500, out_ms: 1000 };
    case "Limiter": return { type: t, ceiling_db: -1, release_ms: 80 };
    case "DeEsser": return { type: t, freq_hz: 5500, threshold_db: -30, max_reduction_db: 8 };
    case "Denoise": return { type: t, mix: 0.8 };
  }
}

/** Slider definitions: [path, label, min, max, step, unit] */
export type Param = [string, string, number, number, number, string];
export const PARAMS: Record<EffectType, Param[]> = {
  Gain: [["db", "Gain", -24, 24, 0.5, "dB"]],
  Eq: [["hpf_hz", "Low cut", 20, 400, 5, "Hz"],
    ["low_shelf.freq", "Low shelf freq", 40, 500, 5, "Hz"], ["low_shelf.gain_db", "Low shelf", -12, 12, 0.5, "dB"],
    ...[0, 1, 2].flatMap((i): Param[] => [[`peaks.${i}.freq`, `Band ${i + 1} freq`, 60, 12000, 10, "Hz"], [`peaks.${i}.gain_db`, `Band ${i + 1} gain`, -12, 12, 0.5, "dB"], [`peaks.${i}.q`, `Band ${i + 1} width (Q)`, 0.3, 6, 0.1, ""]]),
    ["high_shelf.freq", "High shelf freq", 2000, 16000, 100, "Hz"], ["high_shelf.gain_db", "High shelf", -12, 12, 0.5, "dB"]],
  Compressor: [["threshold_db", "Threshold", -60, 0, 1, "dB"], ["ratio", "Ratio", 1, 20, 0.5, ":1"], ["attack_ms", "Attack", 0.1, 100, 0.5, "ms"],
    ["release_ms", "Release", 10, 1000, 10, "ms"], ["makeup_db", "Makeup", 0, 24, 0.5, "dB"]],
  Gate: [["threshold_db", "Threshold", -80, -10, 1, "dB"], ["floor_db", "Reduce by", -80, 0, 1, "dB"], ["attack_ms", "Attack", 0.1, 50, 0.5, "ms"],
    ["hold_ms", "Hold", 0, 500, 10, "ms"], ["release_ms", "Release", 10, 1000, 10, "ms"]],
  NoiseReduction: [["reduction_db", "Reduction", 0, 40, 1, "dB"], ["sensitivity_db", "Sensitivity", 0, 15, 0.5, "dB"]],
  Normalize: [["peak_db", "Peak level", -12, 0, 0.1, "dBFS"]],
  Loudness: [["target_lufs", "Target", -30, -8, 0.5, "LUFS"]],
  Fade: [["in_ms", "Fade in", 0, 10000, 50, "ms"], ["out_ms", "Fade out", 0, 10000, 50, "ms"]],
  Limiter: [["ceiling_db", "Ceiling", -12, 0, 0.1, "dBFS"], ["release_ms", "Release", 5, 500, 5, "ms"]],
  DeEsser: [["freq_hz", "Above", 3000, 10000, 100, "Hz"], ["threshold_db", "Threshold", -60, 0, 1, "dB"], ["max_reduction_db", "Max reduction", 0, 24, 0.5, "dB"]],
  Denoise: [["mix", "Amount", 0, 1, 0.05, ""]],
};

export function getPath(o: any, path: string): any { return path.split(".").reduce((a, k) => a?.[k], o); }
export function setPath<T>(o: T, path: string, v: any): T {
  const c = structuredClone(o) as any; const ks = path.split(".");
  let t = c; for (const k of ks.slice(0, -1)) t = t[k]; t[ks[ks.length - 1]] = v; return c;
}

let nid = 1;
export const slot = (fx: Effect, bypass = false): FxSlot => ({ id: Date.now() * 100 + nid++ % 100, bypass, fx });
const withEq = (p: Partial<Extract<Effect, { type: "Eq" }>>): Effect => ({ ...(defaultFx("Eq") as any), ...p });

export const PRESETS: { name: string; desc: string; build: () => Effect[] }[] = [
  { name: "Podcast voice", desc: "Low cut, presence, gentle gate + compression, -16 LUFS", build: () => [
    withEq({ hpf_hz: 80, low_shelf: band(200, -1.5), peaks: [band(250, -2, 1.2), band(3000, 3, 1), band(6500, -1.5, 3)], high_shelf: band(10000, 2) }),
    { type: "Gate", threshold_db: -50, floor_db: -18, attack_ms: 2, hold_ms: 100, release_ms: 200 },
    { type: "Compressor", threshold_db: -22, ratio: 3, attack_ms: 8, release_ms: 150, makeup_db: 4 },
    { type: "Loudness", target_lufs: -16 }, { type: "Limiter", ceiling_db: -1, release_ms: 80 }] },
  { name: "Clean up noise", desc: "Learns noise from your selection, then removes hum/hiss", build: () => [
    withEq({ hpf_hz: 70 }), { type: "NoiseReduction", reduction_db: 18, sensitivity_db: 6, profile: [] },
    { type: "Gate", threshold_db: -55, floor_db: -12, attack_ms: 2, hold_ms: 120, release_ms: 250 }] },
  { name: "Streaming -14 LUFS", desc: "Spotify / YouTube loudness", build: () => [
    { type: "Compressor", threshold_db: -18, ratio: 2, attack_ms: 15, release_ms: 200, makeup_db: 2 },
    { type: "Loudness", target_lufs: -14 }, { type: "Limiter", ceiling_db: -1, release_ms: 60 }] },
  { name: "Broadcast -16 LUFS", desc: "Podcast / broadcast loudness", build: () => [
    { type: "Compressor", threshold_db: -20, ratio: 2.5, attack_ms: 10, release_ms: 200, makeup_db: 2 },
    { type: "Loudness", target_lufs: -16 }, { type: "Limiter", ceiling_db: -1.5, release_ms: 80 }] },
  { name: "Warm voice", desc: "Fuller lows, softer highs", build: () => [
    withEq({ hpf_hz: 60, low_shelf: band(150, 3), peaks: [band(400, -1, 1), band(3500, -2, 1.2), band(7000, -1, 2)], high_shelf: band(9000, -1.5) }),
    { type: "Compressor", threshold_db: -20, ratio: 2, attack_ms: 20, release_ms: 250, makeup_db: 2 }] },
];

/** Build a preset chain, carrying over any noise profile already learned on the track. */
export function applyPreset(current: FxSlot[], idx: number): FxSlot[] {
  const old = current.find((s) => s.fx.type === "NoiseReduction")?.fx as Extract<Effect, { type: "NoiseReduction" }> | undefined;
  return PRESETS[idx].build().map((fx) => slot(fx.type === "NoiseReduction" && old?.profile.length ? { ...fx, profile: old.profile } : fx));
}
export const activeChain = (fx: FxSlot[] | undefined) => (fx ?? []).map(({ bypass, fx }) => ({ bypass, fx }));

/** Reorder a layer stack: move item `from` to index `to` (Layering Effects drag / keyboard). Same array if nothing moves. */
export function moveLayer(fx: FxSlot[], from: number, to: number): FxSlot[] {
  if (from < 0 || from >= fx.length) return fx;
  to = Math.max(0, Math.min(fx.length - 1, to)); if (to === from) return fx;
  const c = [...fx]; const [m] = c.splice(from, 1); c.splice(to, 0, m); return c;
}
/** Effects offered as clip layers. Noise reduction needs a learned profile from a track selection, so it stays a track effect. */
export const LAYER_TYPES = (Object.keys(FX_NAMES) as EffectType[]).filter((t) => t !== "NoiseReduction");
