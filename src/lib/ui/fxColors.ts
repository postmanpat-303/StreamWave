import type { EffectType } from "../fx";
/** One colour per effect type, so modules are recognisable at a glance (rack, Layering Effects, Effects Layer). New effects: add a line. */
export const FX_COLORS: Record<EffectType, string> = {
  Gain: "#f5a03a", Eq: "#4d9bff", Compressor: "#ff5c8a", Gate: "#9b74ff", NoiseReduction: "#2ec4b6", Normalize: "#ffd04a",
  Loudness: "#ffb658", Fade: "#8cbcff", Limiter: "#ff7a59", DeEsser: "#c38bff", Denoise: "#3fe0c5",
};
export const fxColor = (t: EffectType) => FX_COLORS[t] ?? "#f5a03a";
