<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { FX_NAMES, setPath, type Effect } from "./fx";
  import FxKnobs from "./ui/FxKnobs.svelte";
  import { fxColor } from "./ui/fxColors";
  let { fx: initial, scope, busy = false, onapply, onaddchain, onclose }:
    { fx: Effect; scope: string; busy?: boolean; onapply: (fx: Effect) => void; onaddchain: (fx: Effect) => void; onclose: () => void } = $props();
  let fx = $state<Effect>(untrack(() => structuredClone($state.snapshot(initial) as Effect)));
  let box: HTMLDivElement;
  const needsProfile = $derived(fx.type === "NoiseReduction" && !fx.profile.length);
  onMount(() => { (box.querySelector("[role=slider],input,button") as HTMLElement | null)?.focus(); });
  function key(e: KeyboardEvent) { e.stopPropagation(); if (e.key === "Escape") { e.preventDefault(); onclose(); } }
</script>
<div class="back" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dlg panel" bind:this={box} role="dialog" aria-modal="true" aria-label="{FX_NAMES[fx.type]} on the selection" tabindex="-1" onkeydown={key}>
    <h3 style="color:{fxColor(fx.type)}">{FX_NAMES[fx.type]}</h3>
    <div class="small scope">Applies to: {scope}</div>
    {#if needsProfile}<p class="warn small">Noise reduction needs a noise profile. Add Noise reduction to the track's effects, select a bit of background noise and click <b>Learn</b>, then try again.</p>{/if}
    <FxKnobs {fx} size={60} scale="ring" cols={fx.type === "Eq" ? 4 : 3}
      oninput={(path, v) => (fx = setPath($state.snapshot(fx) as Effect, path, v))}
      onchange={(path, v) => (fx = setPath($state.snapshot(fx) as Effect, path, v))}
      ontoggle={(path, v) => (fx = setPath($state.snapshot(fx) as Effect, path, v))} />
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
  .dlg { width: min(560px, 94vw); max-height: 85vh; overflow: auto; outline: none; background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 16px 18px; box-shadow: 0 16px 48px #000a; }
  .small { font-size: 12px; } h3 { font-size: 14px; letter-spacing: .1em; text-transform: uppercase; font-weight: 800; }
  h3 { margin: 0 0 4px; } .scope { color: var(--muted); margin-bottom: 10px; }
  .row { display: flex; gap: 8px; margin: 14px 0 8px; flex-wrap: wrap; }
  .warn { color: var(--warn); } .muted { color: var(--muted); }
</style>
