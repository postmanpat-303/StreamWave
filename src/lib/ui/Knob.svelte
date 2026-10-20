<script lang="ts">
  /** Hardware-style rotary knob. Drag up/right (or wheel, arrows) to turn; Shift = fine; double-click or Delete = reset.
   *  `oninput` fires while turning, `onchange` once per gesture (pointer release, or ~350 ms after the last wheel/key step). */
  import { onDestroy } from "svelte";
  import { angleOf, autoLog, dragTo, keyValue, litTicks, nudge, readout, scaleLabels, toNorm, type KnobRange } from "./knob";
  let { value, min, max, step = 0, label = "", unit = "", def = undefined, size = 64, log = undefined, ticks = 21,
    scale = "ends", accent = "var(--accent)", disabled = false, format = undefined, oninput, onchange }: {
    value: number; min: number; max: number; step?: number; label?: string; unit?: string; def?: number; size?: number; log?: boolean; ticks?: number;
    scale?: "ends" | "ring" | "none"; accent?: string; disabled?: boolean; format?: (v: number) => string;
    oninput?: (v: number) => void; onchange?: (v: number) => void } = $props();

  const r = $derived<KnobRange>({ min, max, step, log: log ?? autoLog(min, max, unit) });
  let live = $state<number | null>(null);          // value while the user is turning it
  const v = $derived(live ?? value);
  const t = $derived(toNorm(v, r));
  const lit = $derived(litTicks(v, r, ticks));
  const text = $derived(format ? format(v) : readout(v, unit));
  const labels = $derived(scale === "none" ? [] : scaleLabels(r, scale === "ring" ? 5 : 2));

  let drag: { x: number; y: number; v: number; id: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function set(nv: number) { if (nv !== v) { live = nv; oninput?.(nv); } }
  function commit() { clearTimeout(timer); timer = undefined; if (live !== null) { const nv = live; live = null; if (nv !== value) onchange?.(nv); } }
  function later() { clearTimeout(timer); timer = setTimeout(commit, 350); }
  onDestroy(() => { clearTimeout(timer); });

  function down(e: PointerEvent) {
    if (disabled || e.button !== 0) return;
    e.preventDefault(); (e.currentTarget as HTMLElement).focus(); (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    clearTimeout(timer); drag = { x: e.clientX, y: e.clientY, v, id: e.pointerId };
  }
  function move(e: PointerEvent) { if (drag && e.pointerId === drag.id) set(dragTo(drag.v, e.clientX - drag.x, e.clientY - drag.y, r, e.shiftKey)); }
  function up(e: PointerEvent) { if (drag && e.pointerId === drag.id) { drag = null; commit(); } }
  function wheel(e: WheelEvent) { if (disabled) return; e.preventDefault(); const d = -(Math.sign(e.deltaY || -e.deltaX)); if (d) { set(nudge(v, d, r, false, e.shiftKey)); later(); } }
  function key(e: KeyboardEvent) {
    if (disabled) return;
    const nv = keyValue(e.key, v, r, e.shiftKey, def);
    if (nv === null) return;
    e.preventDefault(); e.stopPropagation(); set(nv); later();
  }
  function reset() { if (!disabled && def !== undefined) { set(def); commit(); } }

  // tick dots on a 100×100 grid
  const dots = $derived(Array.from({ length: ticks }, (_, i) => { const a = (angleOf(i / (ticks - 1)) * Math.PI) / 180; return { x: 50 + 46 * Math.sin(a), y: 50 - 46 * Math.cos(a), on: i < lit }; }));
  const pos = (u: number, rad: number) => { const a = (angleOf(u) * Math.PI) / 180; return `left:${50 + rad * Math.sin(a)}%;top:${50 - rad * Math.cos(a)}%`; };
</script>

<div class="knob" class:disabled class:ring={scale === "ring"} style="--sz:{size}px;--acc:{accent}">
  {#if label}<div class="cap">{label}</div>{/if}
  <div class="dial" role="slider" tabindex={disabled ? -1 : 0} aria-label={label || "Knob"} aria-valuemin={min} aria-valuemax={max} aria-valuenow={v}
    aria-valuetext={text} aria-disabled={disabled} title="{label ? label + ': ' : ''}{text} — drag, scroll or use the arrow keys{def !== undefined ? '; double-click to reset' : ''}"
    onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onwheel={wheel} onkeydown={key} ondblclick={reset}>
    <svg class="ticks" viewBox="0 0 100 100" aria-hidden="true">
      {#each dots as d}<circle cx={d.x} cy={d.y} r="2.3" class:on={d.on} />{/each}
    </svg>
    <div class="rim"><div class="body"><div class="ind" style="transform:rotate({angleOf(t)}deg)"></div></div></div>
    {#if scale === "ring"}
      {#each labels as [u, s]}<span class="sl" class:hot={Math.abs(u - t) < 0.13} style={pos(u, 66)}>{s}</span>{/each}
    {/if}
  </div>
  {#if scale === "ends"}<div class="ends" aria-hidden="true"><span>{labels[0]?.[1]}</span><span>{labels[1]?.[1]}</span></div>{/if}
  <div class="val">{text}</div>
</div>

<style>
  .knob { display: flex; flex-direction: column; align-items: center; gap: 3px; min-width: calc(var(--sz) + 8px); user-select: none; }
  .knob.ring { padding: 4px 16px 0; }
  .knob.disabled { opacity: .45; }
  .cap { font-size: 10.5px; font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--text); text-align: center; line-height: 1.15; max-width: calc(var(--sz) + 36px); }
  .knob.ring .cap { margin-bottom: 12px; }
  .dial { position: relative; width: var(--sz); height: var(--sz); border-radius: 50%; cursor: ns-resize; touch-action: none; outline: none; }
  .dial:focus-visible { box-shadow: 0 0 0 2px var(--acc); }
  .disabled .dial { cursor: default; }
  .ticks { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  .ticks circle { fill: var(--tick-off); transition: fill 60ms; }
  .ticks circle.on { fill: var(--acc); filter: drop-shadow(0 0 2px var(--acc)); }
  /* knurled rim: fine radial grooves (repeating conic gradient) on a raised ring */
  .rim { position: absolute; inset: 14%; border-radius: 50%;
    background: repeating-conic-gradient(from 0deg, #3a3e45 0deg 3deg, #16181c 3deg 6deg);
    box-shadow: 0 4px 9px #000c, 0 1px 0 #ffffff14 inset, 0 0 0 1px #000; }
  .body { position: absolute; inset: 12%; border-radius: 50%;
    background: radial-gradient(circle at 50% 30%, #3c4048 0%, #26292f 55%, #1b1d22 100%);
    box-shadow: 0 2px 4px #000a, 0 1px 0 #ffffff1f inset, 0 -2px 4px #0008 inset; }
  .body::after { content: ""; position: absolute; inset: 22%; border-radius: 50%; background: radial-gradient(circle at 50% 35%, #2c2f35, #1f2126); box-shadow: 0 1px 1px #ffffff12 inset, 0 0 0 1px #0006; }
  .ind { position: absolute; inset: 0; z-index: 1; }
  .ind::before { content: ""; position: absolute; left: 50%; top: 5%; width: 3px; height: 20%; margin-left: -1.5px; border-radius: 2px; background: var(--acc); box-shadow: 0 0 4px var(--acc); }
  .sl { position: absolute; transform: translate(-50%, -50%); font-size: 9.5px; color: var(--muted); white-space: nowrap; font-variant-numeric: tabular-nums; pointer-events: none; }
  .sl.hot { color: var(--acc); }
  .ends { display: flex; justify-content: space-between; width: calc(var(--sz) + 10px); margin-top: -6px; font-size: 9px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .val { font-size: 10.5px; color: var(--acc); font-variant-numeric: tabular-nums; white-space: nowrap; }
</style>
