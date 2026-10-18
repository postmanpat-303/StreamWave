<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { FX_NAMES, PARAMS, getPath, setPath, type Effect } from "./fx";
  let { fx: initial, scope, busy = false, onapply, onaddchain, onclose }:
    { fx: Effect; scope: string; busy?: boolean; onapply: (fx: Effect) => void; onaddchain: (fx: Effect) => void; onclose: () => void } = $props();
  let fx = $state<Effect>(untrack(() => structuredClone($state.snapshot(initial) as Effect)));
  let box: HTMLDivElement;
  const needsProfile = $derived(fx.type === "NoiseReduction" && !fx.profile.length);
  const fmt = (v: number, unit: string) => `${Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(unit === "" ? 2 : 1)} ${unit}`;
  onMount(() => { (box.querySelector("input,button") as HTMLElement | null)?.focus(); });
  function key(e: KeyboardEvent) { e.stopPropagation(); if (e.key === "Escape") { e.preventDefault(); onclose(); } }
</script>
<div class="back" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dlg panel" bind:this={box} role="dialog" aria-modal="true" aria-label="{FX_NAMES[fx.type]} on the selection" tabindex="-1" onkeydown={key}>
    <h3>{FX_NAMES[fx.type]}</h3>
    <div class="small scope">Applies to: {scope}</div>
    {#if fx.type === "Eq"}
      <label class="chk"><input type="checkbox" checked={fx.hpf_on} onchange={(e) => (fx = setPath($state.snapshot(fx) as Effect, "hpf_on", e.currentTarget.checked))} /> Low cut (high-pass) on</label>
    {/if}
    {#if needsProfile}<p class="warn small">Noise reduction needs a noise profile. Add Noise reduction to the track's effects, select a bit of background noise and click <b>Learn</b>, then try again.</p>{/if}
    {#each PARAMS[fx.type] as [path, label, min, max, step, unit]}
      <div class="prm">
        <div class="pl"><span>{label}</span><span class="val">{fmt(getPath(fx, path), unit)}</span></div>
        <input type="range" {min} {max} {step} value={getPath(fx, path)} aria-label={label} oninput={(e) => (fx = setPath($state.snapshot(fx) as Effect, path, +e.currentTarget.value))} />
      </div>
    {/each}
    <div class="row">
      <button class="primary" disabled={busy || needsProfile} onclick={() => onapply($state.snapshot(fx) as Effect)}>{busy ? "Applying…" : "Apply"}</button>
      <button disabled={busy} title="Keep it live and editable in the track's effect chain (whole track)" onclick={() => onaddchain($state.snapshot(fx) as Effect)}>Add to track effects instead</button>
      <button disabled={busy} onclick={onclose}>Cancel</button>
    </div>
    <div class="small muted">Apply renders a new audio file for the selection; your original recording isn't changed and Ctrl+Z undoes it.</div>
  </div>
</div>
<style>
  .back { position: fixed; inset: 0; background: #0007; display: grid; place-items: center; z-index: 90; }
  .dlg { width: 420px; max-height: 85vh; overflow: auto; outline: none; background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 16px 18px; box-shadow: 0 16px 48px #000a; }
  .small { font-size: 12px; } h3 { font-size: 13px; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
  h3 { margin: 0 0 4px; } .scope { color: var(--muted); margin-bottom: 10px; }
  .prm { margin: 8px 0; } .pl { display: flex; justify-content: space-between; font-size: 12px; } .val { color: var(--accent); }
  .prm input { width: 100%; padding: 0; }
  .chk { display: flex; gap: 6px; align-items: center; font-size: 13px; } .chk input { width: auto; }
  .row { display: flex; gap: 8px; margin: 14px 0 8px; flex-wrap: wrap; }
  .primary { background: var(--accent); color: #062; border-color: var(--accent); font-weight: 600; }
  .warn { color: var(--warn); } .muted { color: var(--muted); }
</style>
