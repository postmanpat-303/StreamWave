<script lang="ts">
  /** Row of LED dots. `value` and `hold` are 0..1 positions. mode "level" colours dots by zone (teal / yellow / red); "single" uses one colour. */
  import { litDots, zone } from "./meter";
  let { value = 0, hold = 0, n = 16, label = "", mode = "level", color = "var(--accent)" }:
    { value?: number; hold?: number; n?: number; label?: string; mode?: "level" | "single"; color?: string } = $props();
  const lit = $derived(litDots(value, n));
  const holdAt = $derived(hold > 0 ? Math.min(n, Math.max(1, Math.round(hold * n))) - 1 : -1);
</script>
<div class="led" style="--c:{color}" role="meter" aria-label={label || "Level"} aria-valuemin={0} aria-valuemax={1} aria-valuenow={+value.toFixed(3)}>
  {#if label}<span class="lbl">{label}</span>{/if}
  <span class="dots">
    {#each Array(n) as _, i}<i class={mode === "level" ? zone((i + 1) / n) : "one"} class:on={i < lit || i === holdAt}></i>{/each}
  </span>
</div>
<style>
  .led { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .lbl { font-size: 10.5px; font-weight: 800; letter-spacing: .06em; text-transform: uppercase; color: var(--text); width: 18px; flex: none; }
  .dots { display: flex; gap: 3px; flex: 1; min-width: 0; }
  i { flex: 1; max-width: 11px; aspect-ratio: 1; min-width: 4px; border-radius: 50%; background: radial-gradient(circle at 40% 35%, #3a3d43, #1b1c1f); box-shadow: 0 1px 1px #000a inset, 0 1px 0 #ffffff0d; }
  i.on.lo { background: radial-gradient(circle at 40% 35%, #8ff5ea, var(--teal)); box-shadow: 0 0 5px rgb(var(--teal-rgb) / .8); }
  i.on.mid { background: radial-gradient(circle at 40% 35%, #fff0a8, var(--warn)); box-shadow: 0 0 5px rgb(var(--warn-rgb) / .8); }
  i.on.hot { background: radial-gradient(circle at 40% 35%, #ffb0b0, var(--rec)); box-shadow: 0 0 6px rgb(var(--rec-rgb) / .9); }
  i.on.one { background: radial-gradient(circle at 40% 35%, #ffd9a6, var(--c)); box-shadow: 0 0 5px var(--c); }
</style>
