<script lang="ts">
  // Layering Effects: the ordered effect stack of ONE clip. Top layer runs first, then the next, ...
  // then the clip's gain/fades, then the track's effects, then the mix. Same effect engine as track effects.
  import { FX_NAMES, PARAMS, LAYER_TYPES, defaultFx, getPath, setPath, slot, moveLayer, type FxSlot, type EffectType } from "./fx";
  import type { Clip } from "./editModel";
  import { fmtTime } from "./api";
  let { clip, clipName, trackName, at, onchange, onclose }: {
    clip: Clip; clipName: string; trackName: string; at: number;
    onchange: (fx: FxSlot[], label: string) => void; onclose: () => void } = $props();
  const layers = $derived(clip.fx ?? []);
  let selId = $state<number | null>(null);
  let addType = $state<EffectType>("Eq");
  let drafts = $state<Record<string, number>>({});
  const sel = $derived(layers.find((s) => s.id === selId) ?? null);
  const fmt = (v: number, unit: string) => `${Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(1)} ${unit}`;
  const rows: HTMLElement[] = [];

  function set(i: number, s: FxSlot, label: string) { const c = [...layers]; c[i] = s; onchange(c, label); }
  function move(i: number, to: number, focus = true) {
    const next = moveLayer(layers, i, to); if (next === layers) return;
    onchange(next, "Reorder layers");
    if (focus) queueMicrotask(() => rows[Math.max(0, Math.min(layers.length - 1, to))]?.focus());
  }
  function add() { const s = slot(defaultFx(addType)); onchange([...layers, s], `Add layer: ${FX_NAMES[addType]}`); selId = s.id; }
  function remove(i: number) { const s = layers[i]; onchange(layers.filter((x) => x.id !== s.id), `Remove layer: ${FX_NAMES[s.fx.type]}`); if (selId === s.id) selId = null; }
  function onRowKey(e: KeyboardEvent, i: number) {
    const s = layers[i]; let handled = true;
    if ((e.altKey || e.ctrlKey || e.metaKey) && e.key === "ArrowUp") move(i, i - 1);
    else if ((e.altKey || e.ctrlKey || e.metaKey) && e.key === "ArrowDown") move(i, i + 1);
    else if (e.key === "ArrowUp") rows[Math.max(0, i - 1)]?.focus();
    else if (e.key === "ArrowDown") rows[Math.min(layers.length - 1, i + 1)]?.focus();
    else if (e.key === "Enter") selId = selId === s.id ? null : s.id;
    else if (e.key === " ") set(i, { ...s, bypass: !s.bypass }, s.bypass ? "Enable layer" : "Bypass layer");
    else if (e.key === "Delete" || e.key === "Backspace") remove(i);
    else handled = false;
    if (handled) { e.preventDefault(); e.stopPropagation(); }
  }
  // pointer drag on the grip: live insertion marker, one undo step on drop
  let drag = $state<{ from: number; to: number } | null>(null);
  function gripDown(e: PointerEvent, i: number) {
    if (e.button !== 0) return; e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); drag = { from: i, to: i };
  }
  function gripMove(e: PointerEvent) {
    if (!drag) return;
    let to = layers.length - 1;
    for (let k = 0; k < rows.length; k++) { const r = rows[k]?.getBoundingClientRect(); if (r && e.clientY < r.top + r.height / 2) { to = k > drag.from ? k - 1 : k; break; } }
    drag = { ...drag, to: Math.max(0, Math.min(layers.length - 1, to)) };
  }
  function gripUp() { if (drag) { const d = drag; drag = null; move(d.from, d.to, false); } }
</script>

<div class="sheet panel" role="dialog" aria-label="Layering Effects for clip {clipName}">
  <div class="hdr">
    <h3>◧ Layering Effects</h3>
    <button class="x" onclick={onclose} aria-label="Close" title="Close (Esc)">✕</button>
  </div>
  <div class="who">Clip <b>{clipName}</b> at {fmtTime(at)} on <b>{trackName}</b></div>
  <div class="own">Belongs to this clip only. Copying a clip copies its layers; split pieces each keep their own copy.</div>
  <div class="flow" aria-hidden="true">Clip audio → <b>layers ↓</b> → clip gain/fades → track effects → mix</div>

  {#if !layers.length}<div class="empty">No layers yet. Add one below; the top layer runs first.</div>{/if}
  <ol class="layers" aria-label="Effect layers, top runs first">
    {#each layers as s, i (s.id)}
      <li bind:this={rows[i]} class="layer" class:off={s.bypass} class:sel={selId === s.id} class:dragging={drag?.from === i}
        class:dropabove={drag && drag.to === i && drag.from > i} class:dropbelow={drag && drag.to === i && drag.from < i}
        tabindex="0" aria-label="Layer {i + 1}: {FX_NAMES[s.fx.type]}{s.bypass ? ' (bypassed)' : ''}" aria-selected={selId === s.id} role="option"
        onkeydown={(e) => onRowKey(e, i)}>
        <span class="grip" title="Drag to reorder (or Alt+↑/↓)" role="presentation" onpointerdown={(e) => gripDown(e, i)} onpointermove={gripMove} onpointerup={gripUp} onpointercancel={() => (drag = null)}>⋮⋮</span>
        <span class="num">{i + 1}</span>
        <button class="pw" class:on={!s.bypass} title={s.bypass ? "Turn on" : "Bypass"} aria-pressed={!s.bypass} onclick={() => set(i, { ...s, bypass: !s.bypass }, s.bypass ? "Enable layer" : "Bypass layer")}>⏻</button>
        <button class="name" onclick={() => (selId = selId === s.id ? null : s.id)}>{FX_NAMES[s.fx.type]}</button>
        <button class="ic" disabled={i === 0} onclick={() => move(i, i - 1)} title="Move up" aria-label="Move {FX_NAMES[s.fx.type]} up">↑</button>
        <button class="ic" disabled={i === layers.length - 1} onclick={() => move(i, i + 1)} title="Move down" aria-label="Move {FX_NAMES[s.fx.type]} down">↓</button>
        <button class="ic" onclick={() => remove(i)} title="Remove layer" aria-label="Remove {FX_NAMES[s.fx.type]}">✕</button>
      </li>
    {/each}
  </ol>
  <div class="add">
    <select bind:value={addType} aria-label="Effect to add">{#each LAYER_TYPES as t}<option value={t}>{FX_NAMES[t]}</option>{/each}</select>
    <button onclick={add}>＋ Add layer</button>
  </div>

  {#if sel}
    {@const i = layers.indexOf(sel)}
    <div class="edit">
      <h4>{i + 1}. {FX_NAMES[sel.fx.type]}{sel.bypass ? " (bypassed)" : ""}</h4>
      {#if sel.fx.type === "Eq"}
        <label class="chk"><input type="checkbox" checked={sel.fx.hpf_on} onchange={(e) => set(i, { ...sel, fx: setPath(sel.fx, "hpf_on", e.currentTarget.checked) }, "Edit layer")} /> Low cut (high-pass) on</label>
      {/if}
      {#each PARAMS[sel.fx.type] as [path, label, min, max, step, unit]}
        {@const key = `${sel.id}:${path}`}
        {@const v = drafts[key] ?? getPath(sel.fx, path)}
        <div class="prm">
          <div class="pl"><span>{label}</span><span class="val">{fmt(v, unit)}</span></div>
          <input type="range" {min} {max} {step} value={getPath(sel.fx, path)} aria-label={label}
            oninput={(e) => (drafts[key] = +e.currentTarget.value)}
            onchange={(e) => { const nv = +e.currentTarget.value; delete drafts[key]; set(i, { ...sel, fx: setPath(sel.fx, path, nv) }, `${FX_NAMES[sel.fx.type]}: ${label}`); }} />
        </div>
      {/each}
    </div>
  {:else if layers.length}
    <div class="muted">Select a layer to edit its settings.</div>
  {/if}
  <div class="muted keys">Keys: ↑/↓ pick a layer · Alt+↑/↓ move it · Enter edit · Space on/off · Del remove · Ctrl+Z undo</div>
</div>

<style>
  .sheet { position: fixed; top: 70px; right: 16px; bottom: 16px; width: 340px; overflow-y: auto; z-index: 65; background: var(--hw-bg); border: 1px solid rgb(var(--accent-rgb) / 0.40); border-radius: 12px; padding: 14px; box-shadow: 0 12px 40px #000a; }
  .hdr { display: flex; align-items: center; justify-content: space-between; } h3 { margin: 0; } .x { background: none; border: none; color: var(--muted); font-size: 15px; }
  .who { font-size: 12px; margin: 8px 0 4px; overflow-wrap: anywhere; }
  .own { font-size: 11px; color: #ffd27a; background: rgb(var(--warn-rgb) / 0.07); border: 1px solid rgb(var(--warn-rgb) / 0.27); border-radius: 8px; padding: 6px 8px; }
  .flow { font-size: 11px; color: var(--muted); margin: 8px 0; } .flow b { color: var(--accent); }
  .empty { font-size: 12px; color: var(--muted); padding: 10px; border: 1px dashed var(--line); border-radius: 8px; text-align: center; }
  .layers { list-style: none; margin: 6px 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .layer { display: flex; align-items: center; gap: 4px; border: 1px solid var(--line); border-radius: 9px; padding: 4px 6px; background: var(--panel2); outline: none; position: relative; }
  .layer:focus-visible { box-shadow: 0 0 0 2px var(--blue); } .layer.sel { border-color: var(--accent); } .layer.off { opacity: .55; } .layer.dragging { opacity: .4; }
  .layer.dropabove::before, .layer.dropbelow::after { content: ""; position: absolute; left: 4px; right: 4px; height: 3px; background: var(--blue); border-radius: 2px; }
  .layer.dropabove::before { top: -5px; } .layer.dropbelow::after { bottom: -5px; }
  .grip { cursor: grab; color: var(--muted); padding: 0 3px; user-select: none; touch-action: none; letter-spacing: -2px; }
  .num { font-size: 10px; color: var(--muted); width: 14px; text-align: center; font-variant-numeric: tabular-nums; }
  .name { flex: 1; text-align: left; background: none; border: none; padding: 4px; font-weight: 600; }
  .ic, .pw { padding: 2px 7px; font-size: 12px; background: none; } .pw.on { color: var(--accent); border-color: var(--accent); }
  .add { display: flex; gap: 6px; margin: 8px 0; } .add select { flex: 1; }
  .edit { border-top: 1px solid var(--line); margin-top: 8px; padding-top: 8px; } h4 { margin: 0 0 6px; font-size: 13px; }
  .prm { margin: 6px 2px; } .pl { display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); } .val { color: var(--text); font-variant-numeric: tabular-nums; }
  input[type="range"] { padding: 0; accent-color: var(--accent); width: 100%; }
  .chk { display: flex; gap: 6px; align-items: center; font-size: 12px; } .chk input { width: auto; }
  .muted { color: var(--muted); font-size: 11px; } .keys { margin-top: 10px; }
</style>
