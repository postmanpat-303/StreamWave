<script lang="ts">
  // Effects Layer workspace: replaces the right settings column. Left: the effect picker (the active track's chain and the
  // layers of the clip under the cursor), right: colour-coded modules with knobs for the picked effect.
  // It owns no audio state: every edit goes through the same callbacks as the Effects rack / Layering Effects panel
  // (track → changeFx, clip → setClipFx + live dry swap), so nothing is processed twice and switching views changes nothing.
  import { FX_NAMES, LAYER_TYPES, PRESETS, applyPreset, defaultFx, moveLayer, setPath, slot, type EffectType, type FxSlot } from "./fx";
  import { moveFx } from "./editModel";
  import FxKnobs from "./ui/FxKnobs.svelte";
  import LedToggle from "./ui/LedToggle.svelte";
  import Dropdown from "./ui/Dropdown.svelte";
  import { fxColor } from "./ui/fxColors";
  type Level = "track" | "clip";
  let { trackName, chain, clip = null, hasSelection, ontrack, ontracklive, onclip, onlearn }: {
    trackName: string | null; chain: FxSlot[]; clip?: { name: string; at: number; fx: FxSlot[] } | null; hasSelection: boolean;
    ontrack: (fx: FxSlot[], label: string) => void; ontracklive: (fx: FxSlot[]) => void;
    onclip: (fx: FxSlot[], label: string) => void; onlearn: (slotId: number) => void } = $props();

  let pick = $state<{ level: Level; id: number } | null>(null);
  let pinned = $state<boolean | null>(null); // null = automatic (by width)
  let w = $state(0);
  const collapsed = $derived(pinned === null ? w < 560 : !pinned);
  let overlay = $state(false); // narrow + collapsed: list opened over the module area (never over the timeline)
  let addTrack = $state<EffectType>("Eq"), addClip = $state<EffectType>("Eq");

  const groups = $derived<{ l: Level; xs: FxSlot[]; title: string }[]>([
    { l: "track", xs: chain, title: trackName ? `Track · ${trackName}` : "No track" },
    ...(clip ? [{ l: "clip" as Level, xs: clip.fx, title: `Clip · ${clip.name}` }] : []),
  ]);
  const list = (l: Level) => (l === "track" ? chain : clip?.fx ?? []);
  const cur = $derived.by(() => {
    if (pick) { const s = list(pick.level).find((x) => x.id === pick!.id); if (s) return { level: pick.level, s }; }
    const s = chain[0] ?? clip?.fx[0]; return s ? { level: (chain[0] ? "track" : "clip") as Level, s } : null;
  });
  const commit = (l: Level, fx: FxSlot[], label: string) => (l === "track" ? ontrack(fx, label) : onclip(fx, label));
  function replace(l: Level, s: FxSlot, label: string) { commit(l, list(l).map((x) => (x.id === s.id ? s : x)), label); }
  function add(l: Level) {
    const t = l === "track" ? addTrack : addClip, s = slot(defaultFx(t));
    commit(l, [...list(l), s], l === "track" ? `Add effect: ${FX_NAMES[t]}` : `Add layer: ${FX_NAMES[t]}`); pick = { level: l, id: s.id };
  }
  function remove(l: Level, s: FxSlot) { commit(l, list(l).filter((x) => x.id !== s.id), `Remove ${l === "track" ? "effect" : "layer"}: ${FX_NAMES[s.fx.type]}`); if (pick?.id === s.id) pick = null; }
  function move(l: Level, i: number, to: number) {
    const xs = list(l); if (to < 0 || to >= xs.length) return;
    if (l === "track") ontrack(moveFx(xs, i, to < i ? -1 : 1), "Reorder effects"); else { const n = moveLayer(xs, i, to); if (n !== xs) onclip(n, "Reorder layers"); }
  }
  // live knob drags on the track chain: push the draft into the running playback (no history entry until release)
  let raf = 0, pending: FxSlot[] | null = null;
  function live(l: Level, s: FxSlot, path: string, v: number) {
    if (l !== "track") return; // clip layers re-render the clip; applied once on release
    pending = chain.map((x) => (x.id === s.id ? { ...s, fx: setPath(s.fx, path, v) } : x));
    if (!raf) raf = requestAnimationFrame(() => { raf = 0; if (pending) ontracklive(pending); pending = null; });
  }
  function choose(l: Level, s: FxSlot) { pick = { level: l, id: s.id }; overlay = false; }
</script>

<section class="ews" bind:clientWidth={w} aria-label="Effects Layer workspace">
  <nav class="list hw" class:collapsed class:overlay={collapsed && overlay} aria-label="Effects">
    <div class="lh">
      {#if !collapsed || overlay}<span class="t">Effects</span>{/if}
      <button class="ic" title={collapsed && !overlay ? "Expand the effects list" : "Collapse the effects list"} aria-expanded={!collapsed || overlay}
        onclick={() => { if (collapsed && w < 560) overlay = !overlay; else pinned = collapsed; }}>{collapsed && !overlay ? "»" : "«"}</button>
    </div>
    {#each groups as { l, xs, title } (l)}
        <div class="grp">
          {#if !collapsed || overlay}<div class="gt" title={title}>{title}</div>{:else}<div class="gsep" title={title}>{l === "track" ? "T" : "C"}</div>{/if}
          {#each xs as s, i (s.id)}
            <button class="item" class:sel={cur?.s.id === s.id} class:off={s.bypass} style="--mod:{fxColor(s.fx.type)}" onclick={() => choose(l, s)}
              title="{i + 1}. {FX_NAMES[s.fx.type]}{s.bypass ? ' (bypassed)' : ''}" aria-current={cur?.s.id === s.id}>
              <span class="dot"></span>{#if !collapsed || overlay}<span class="nm">{FX_NAMES[s.fx.type]}</span>{/if}
            </button>
          {/each}
          {#if (!collapsed || overlay) && !xs.length}<div class="none">{l === "track" ? "No track effects" : "No clip layers"}</div>{/if}
        </div>
    {/each}
    {#if !clip && (!collapsed || overlay)}<div class="none hint">Put the cursor on a clip to edit its layers here.</div>{/if}
  </nav>

  <div class="work">
    {#if trackName}
      <div class="flow" aria-label="Signal flow">
        {#if clip}
          <span class="lv">Clip</span>
          {#each clip.fx as s (s.id)}<button class="blk" class:sel={cur?.s.id === s.id} class:off={s.bypass} style="--mod:{fxColor(s.fx.type)}" onclick={() => choose("clip", s)}>{FX_NAMES[s.fx.type]}</button>{/each}
          <span class="arr">→ gain/fades →</span>
        {/if}
        <span class="lv">Track</span>
        {#each chain as s (s.id)}<button class="blk" class:sel={cur?.s.id === s.id} class:off={s.bypass} style="--mod:{fxColor(s.fx.type)}" onclick={() => choose("track", s)}>{FX_NAMES[s.fx.type]}</button>{/each}
        <span class="arr">→ mix</span>
      </div>

      {#if cur}
        {@const l = cur.level}
        {@const s = cur.s}
        {@const xs = list(l)}
        {@const i = xs.findIndex((x) => x.id === s.id)}
        <div class="mod hw" style="--mod:{fxColor(s.fx.type)}" class:off={s.bypass}>
          <div class="mh">
            <LedToggle checked={!s.bypass} title={s.bypass ? "Turn on" : "Bypass"} onchange={(on) => replace(l, { ...s, bypass: !on }, on ? "Enable effect" : "Bypass effect")} />
            <h3>{FX_NAMES[s.fx.type]}</h3>
            <span class="lvl">{l === "track" ? "Track effect" : "Clip layer"} {i + 1}/{xs.length}</span>
            <button class="ic" disabled={i <= 0} onclick={() => move(l, i, i - 1)} title="Run earlier" aria-label="Move earlier">↑</button>
            <button class="ic" disabled={i >= xs.length - 1} onclick={() => move(l, i, i + 1)} title="Run later" aria-label="Move later">↓</button>
            <button class="ic" onclick={() => remove(l, s)} title="Remove" aria-label="Remove {FX_NAMES[s.fx.type]}">✕</button>
          </div>
          {#if s.fx.type === "NoiseReduction"}
            <div class="nr"><button class="pill small" disabled={!hasSelection} onclick={() => onlearn(s.id)}>{s.fx.profile.length ? "Re-learn" : "Learn"} noise from selection</button>
              <span class="muted">{s.fx.profile.length ? "Noise profile learned ✓" : "Select background noise first."}</span></div>
          {/if}
          <FxKnobs fx={s.fx} size={66} scale="ring" cols={s.fx.type === "Eq" ? 4 : 3}
            oninput={(path, v) => live(l, s, path, v)}
            onchange={(path, v, label) => replace(l, { ...s, fx: setPath(s.fx, path, v) }, `${FX_NAMES[s.fx.type]}: ${label}`)}
            ontoggle={(path, v) => replace(l, { ...s, fx: setPath(s.fx, path, v) }, "Edit effect")} />
        </div>
      {:else}
        <div class="empty">No effects yet. Add a track effect or a clip layer below.</div>
      {/if}

      <div class="adds">
        <div class="addrow"><Dropdown caption="Add track effect" bind:value={addTrack}>{#each Object.entries(FX_NAMES) as [t, n]}<option value={t}>{n}</option>{/each}</Dropdown><button onclick={() => add("track")}>＋ Track</button></div>
        {#if clip}<div class="addrow"><Dropdown caption="Add clip layer" bind:value={addClip}>{#each LAYER_TYPES as t}<option value={t}>{FX_NAMES[t]}</option>{/each}</Dropdown><button onclick={() => add("clip")}>＋ Clip</button></div>{/if}
      </div>
      <div class="presets">
        <span class="cap">Track presets</span>
        {#each PRESETS as p, k}<button class="pill small" title={p.desc} onclick={() => { const n = applyPreset(chain, k); ontrack(n, `Preset: ${p.name}`); pick = null; }}>{p.name}</button>{/each}
      </div>
      <div class="muted foot">Order: clip layers (top first) → clip gain/fades → track effects → mix. There is no master bus; edits are live and never change your files.</div>
    {:else}
      <div class="empty">Add or select an audio track to edit its effects.</div>
    {/if}
  </div>
</section>

<style>
  .ews { position: relative; display: flex; gap: 10px; min-height: 0; height: 100%; }
  .list { flex: none; width: 188px; padding: 10px 8px; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; transition: width .15s; }
  .list.collapsed { width: 46px; padding: 10px 5px; }
  .list.overlay { position: absolute; left: 0; top: 0; bottom: 0; width: 210px; z-index: 5; box-shadow: 6px 0 24px #000c; }
  .lh { display: flex; align-items: center; justify-content: space-between; gap: 4px; } .lh .t { font-size: 11px; font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
  .collapsed:not(.overlay) .lh { justify-content: center; }
  .gt { font-size: 9.5px; font-weight: 800; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); margin: 0 2px 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gsep { font-size: 9.5px; font-weight: 800; color: var(--muted); text-align: center; margin-bottom: 4px; }
  .grp { display: flex; flex-direction: column; gap: 4px; }
  .item { display: flex; align-items: center; gap: 8px; padding: 6px 8px; font-size: 11.5px; font-weight: 700; text-transform: uppercase; letter-spacing: .04em; text-align: left; border-radius: 7px; }
  .collapsed:not(.overlay) .item { justify-content: center; padding: 7px 0; }
  .item.sel { border-color: var(--mod); box-shadow: 0 0 0 1px var(--mod), 0 0 10px #0000, inset 3px 0 0 var(--mod); color: var(--mod); }
  .item.off { opacity: .5; } .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { width: 9px; height: 9px; border-radius: 50%; flex: none; background: var(--mod); box-shadow: 0 0 6px var(--mod); }
  .none { font-size: 11px; color: var(--muted); padding: 2px 4px; } .hint { margin-top: auto; }
  .work { flex: 1; min-width: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; }
  .flow { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; font-size: 11px; padding: 8px 10px; border-radius: 8px; background: var(--inset-bg); box-shadow: var(--inset-shadow); }
  .lv { font-size: 9.5px; font-weight: 800; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
  .arr { color: var(--muted); font-size: 10.5px; }
  .blk { padding: 3px 8px; font-size: 10.5px; font-weight: 800; text-transform: uppercase; letter-spacing: .04em; border-radius: 5px; border: 1px solid var(--mod); color: var(--mod); background: #0006; box-shadow: none; }
  .blk.sel { background: var(--mod); color: #111; } .blk.off { opacity: .45; border-style: dashed; }
  .mod { padding: 12px 14px 14px; border-top: 3px solid var(--mod); }
  .mod.off :global(.grid) { opacity: .5; }
  .mh { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
  .mh h3 { margin: 0; color: var(--mod); font-size: 15px; letter-spacing: .1em; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lvl { font-size: 10px; font-weight: 700; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; white-space: nowrap; }
  .ic { padding: 2px 7px; font-size: 12px; }
  .nr { display: flex; align-items: center; gap: 10px; margin-bottom: 6px; }
  .adds { display: flex; flex-direction: column; gap: 6px; } .addrow { display: flex; gap: 6px; } .addrow > :global(.dd) { flex: 1; }
  .presets { display: flex; flex-wrap: wrap; gap: 5px; align-items: center; } .cap { font-size: 9.5px; font-weight: 800; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); margin-right: 4px; }
  .presets button { font-size: 11px; padding: 3px 10px; }
  .empty { color: var(--muted); font-size: 12px; text-align: center; padding: 24px; border: 1px dashed var(--line); border-radius: 10px; }
  .muted { color: var(--muted); font-size: 11px; } .foot { margin-top: auto; padding-top: 4px; }
</style>
