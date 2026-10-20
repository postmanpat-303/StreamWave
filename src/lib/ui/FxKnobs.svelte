<script lang="ts">
  /** All parameters of one effect as a grid of knobs (plus the EQ low-cut switch). Same PARAMS table the sliders used. */
  import { PARAMS, defaultFx, getPath, type Effect } from "../fx";
  import Knob from "./Knob.svelte";
  import LedToggle from "./LedToggle.svelte";
  let { fx, size = 50, scale = "ends", cols = 3, oninput, onchange, ontoggle }: {
    fx: Effect; size?: number; scale?: "ends" | "ring" | "none"; cols?: number;
    oninput?: (path: string, v: number) => void; onchange: (path: string, v: number, label: string) => void; ontoggle?: (path: string, v: boolean) => void } = $props();
  const defs = $derived(defaultFx(fx.type) as Effect);
  // EQ bands get the secondary colours, like the coloured bands of a spectrum display
  const tint = (path: string) => (/^low_shelf|hpf/.test(path) ? "var(--teal)" : /peaks\.0/.test(path) ? "var(--blue)" : /peaks\.1/.test(path) ? "var(--purple)"
    : /peaks\.2/.test(path) ? "var(--pink)" : /high_shelf/.test(path) ? "var(--warn)" : "var(--accent)");
</script>
{#if fx.type === "Eq"}
  <div class="sw"><LedToggle label="Low cut" title="Low cut (high-pass) filter on/off" checked={fx.hpf_on} onchange={(v) => ontoggle?.("hpf_on", v)} /></div>
{/if}
<div class="grid" style="--cols:{cols}">
  {#each PARAMS[fx.type] as [path, label, min, max, step, unit] (path)}
    <Knob value={getPath(fx, path)} {min} {max} {step} {unit} {label} {size} {scale} def={getPath(defs, path)}
      accent={fx.type === "Eq" ? tint(path) : "var(--accent)"} disabled={fx.type === "Eq" && path === "hpf_hz" && !fx.hpf_on}
      oninput={(v) => oninput?.(path, v)} onchange={(v) => onchange(path, v, label)} />
  {/each}
</div>
<style>
  .grid { display: grid; grid-template-columns: repeat(var(--cols), minmax(0, 1fr)); gap: 10px 4px; justify-items: center; padding: 6px 0 2px; }
  .sw { display: flex; justify-content: flex-start; margin: 4px 0 2px; }
</style>
