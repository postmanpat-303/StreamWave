<script lang="ts">
  import { FX_NAMES, PRESETS, defaultFx, setPath, slot, applyPreset, type FxSlot, type EffectType } from "./fx";
  import { moveFx, type EditTrack } from "./editModel";
  import FxKnobs from "./ui/FxKnobs.svelte";
  import LedToggle from "./ui/LedToggle.svelte";
  import Dropdown from "./ui/Dropdown.svelte";
  import { fxColor } from "./ui/fxColors";
  let { track, ab, lufs, hasSelection, onchange, onlearn, onab, onmeasure }: {
    track: EditTrack | undefined; ab: boolean; lufs: number | null; hasSelection: boolean;
    onchange: (fx: FxSlot[], label: string) => void; onlearn: (slotId: number) => void; onab: () => void; onmeasure: () => void } = $props();
  let open = $state<Record<number, boolean>>({});
    let addType = $state<EffectType>("Gain");
  const chain = $derived(track?.fx ?? []);

  function update(i: number, s: FxSlot, label: string) { const c = [...chain]; c[i] = s; onchange(c, label); }
</script>

<div class="panel">
  <div class="hdr">
    <h3>Effects {track ? `· ${track.name}` : ""}</h3>
    <button class="ab" class:b={ab} onclick={onab} title="Compare before/after (A/B)">{ab ? "B: original" : "A: with effects"}</button>
  </div>
  {#if !track}
    <div class="muted">Select a track to add effects.</div>
  {:else}
    <div class="presets">
      {#each PRESETS as p, i}<button class="pill" title={p.desc} onclick={() => onchange(applyPreset(chain, i), `Preset: ${p.name}`)}>{p.name}</button>{/each}
    </div>
    <div class="lufs"><button class="small" onclick={onmeasure}>Measure loudness</button>{#if lufs !== null}<span>{lufs.toFixed(1)} LUFS</span>{/if}</div>

    {#each chain as s, i (s.id)}
      <div class="fx" class:off={s.bypass} style="--mod:{fxColor(s.fx.type)}">
        <div class="fxh">
          <LedToggle checked={!s.bypass} title={s.bypass ? "Turn on" : "Bypass"} onchange={(on) => update(i, { ...s, bypass: !on }, on ? "Enable effect" : "Bypass effect")} />
          <button class="name" onclick={() => (open[s.id] = !open[s.id])}>{FX_NAMES[s.fx.type]} <span class="muted">{open[s.id] ? "▾" : "▸"}</span></button>
          <button class="ic" disabled={i === 0} onclick={() => onchange(moveFx(chain, i, -1), "Move effect")} title="Move up">↑</button>
          <button class="ic" disabled={i === chain.length - 1} onclick={() => onchange(moveFx(chain, i, 1), "Move effect")} title="Move down">↓</button>
          <button class="ic" onclick={() => onchange(chain.filter((x) => x.id !== s.id), "Remove effect")} title="Remove">✕</button>
        </div>
        {#if open[s.id]}
          {#if s.fx.type === "NoiseReduction"}
            <button class="learn" disabled={!hasSelection} onclick={() => onlearn(s.id)}>{s.fx.profile.length ? "Re-learn" : "Learn"} noise from selection</button>
            <div class="muted">{s.fx.profile.length ? "Noise profile learned ✓" : "Select a bit of silence/background noise, then click Learn."}</div>
          {/if}
          <FxKnobs fx={s.fx} size={46}
            onchange={(path, v, label) => update(i, { ...s, fx: setPath(s.fx, path, v) }, `${FX_NAMES[s.fx.type]}: ${label}`)}
            ontoggle={(path, v) => update(i, { ...s, fx: setPath(s.fx, path, v) }, "Edit effect")} />
        {/if}
      </div>
    {/each}
    <div class="add">
      <Dropdown caption="Add effect" bind:value={addType}>{#each Object.entries(FX_NAMES) as [t, n]}<option value={t}>{n}</option>{/each}</Dropdown>
      <button onclick={() => { const s = slot(defaultFx(addType)); open[s.id] = true; onchange([...chain, s], "Add effect"); }}>＋ Add</button>
    </div>
    <div class="muted">Effects are live and never change your files. Ctrl+Z undoes any change.</div>
  {/if}
</div>

<style>
  .panel { background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 14px; }
  .hdr { display: flex; align-items: center; justify-content: space-between; gap: 8px; } .hdr h3 { margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ab { font-size: 11px; padding: 4px 8px; background: rgb(var(--accent-rgb) / 0.13); border-color: var(--accent); } .ab.b { background: rgb(var(--warn-rgb) / 0.13); border-color: var(--warn); }
  .presets { display: flex; flex-wrap: wrap; gap: 6px; margin: 10px 0; } .presets button { font-size: 12px; padding: 5px 9px; border-radius: 14px; }
  .lufs { display: flex; gap: 10px; align-items: center; font-size: 12px; margin-bottom: 8px; } .small { font-size: 11px; padding: 3px 8px; }
  .fx { border: 1px solid var(--line-dark); border-left: 3px solid var(--mod); border-radius: 8px; padding: 6px 8px; margin-bottom: 7px;
    background: linear-gradient(180deg, #2b2e33, #222428); box-shadow: 0 1px 0 #ffffff0d inset, 0 2px 6px #0007; } .fx.off { opacity: .55; }
  .fxh { display: flex; align-items: center; gap: 4px; }
  .name { flex: 1; text-align: left; background: none; border: none; box-shadow: none; padding: 4px; font-weight: 800; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; color: var(--mod); }
  .ic { padding: 2px 7px; font-size: 12px; }
  .add { display: flex; gap: 6px; margin: 8px 0; align-items: stretch; } .add > :global(.dd) { flex: 1; }
  .learn { width: 100%; margin: 6px 0 2px; font-size: 12px; }
  .muted { color: var(--muted); font-size: 11px; }
</style>
