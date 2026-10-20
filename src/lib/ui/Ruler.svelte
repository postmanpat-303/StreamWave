<script lang="ts">
  // Timeline ruler: m:ss labels on the SAME scale as the lanes (inner width = zoom × 100 %, span = timeline length),
  // scrolled together with them (its .scroll is synced by the parent), with playhead, selection and marker indicators.
  // Click or drag sets the edit cursor; ←/→ nudge it when focused.
  import { pickStep, rulerTicks, fmtRuler } from "./ruler";
  let { duration, zoom, playhead = null, cursor = null, selection = null, markers = [], onscroll, onseek }: {
    duration: number; zoom: number; playhead?: number | null; cursor?: number | null; selection?: { a: number; b: number } | null;
    markers?: { time: number; label: string }[]; onscroll?: (e: Event) => void; onseek: (t: number, final: boolean) => void } = $props();
  let el: HTMLDivElement, inner: HTMLDivElement;
  let w = $state(0);
  const pps = $derived(duration > 0 ? (w * zoom) / duration : 0);
  const ticks = $derived(rulerTicks(duration, pps));
  const step = $derived(pickStep(pps || 1));
  const pct = (t: number) => `${(Math.max(0, Math.min(duration, t)) / duration) * 100}%`;
  const long = $derived(duration >= 3600);
  let dragging = false;
  function tAt(x: number) { const r = inner.getBoundingClientRect(); return r.width ? Math.max(0, Math.min(duration, ((x - r.left) / r.width) * duration)) : 0; }
  function down(e: PointerEvent) { if (e.button !== 0 || !duration) return; dragging = true; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); onseek(tAt(e.clientX), false); }
  function move(e: PointerEvent) { if (dragging) onseek(tAt(e.clientX), false); }
  function up(e: PointerEvent) { if (!dragging) return; dragging = false; onseek(tAt(e.clientX), true); }
  function key(e: KeyboardEvent) {
    const c = cursor ?? 0, d = step[0] / step[1] * (e.shiftKey ? 5 : 1);
    let t: number | null = null;
    if (e.key === "ArrowLeft") t = c - d; else if (e.key === "ArrowRight") t = c + d; else if (e.key === "Home") t = 0; else if (e.key === "End") t = duration;
    if (t === null) return; e.preventDefault(); e.stopPropagation(); onseek(Math.max(0, Math.min(duration, t)), true);
  }
  // join the lanes' current scroll position when (re)mounted or rescaled
  $effect(() => { void zoom; const other = el?.closest(".tracks")?.querySelector<HTMLElement>(".lane .scroll"); if (other && el) queueMicrotask(() => (el.scrollLeft = other.scrollLeft)); });
</script>
<div class="rwrap">
  <div class="scroll" bind:this={el} bind:clientWidth={w} {onscroll}>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="inner" bind:this={inner} style="width:{zoom * 100}%" role="slider" tabindex="0" aria-label="Timeline position"
      aria-valuemin={0} aria-valuemax={+duration.toFixed(3)} aria-valuenow={+(cursor ?? 0).toFixed(3)} aria-valuetext={fmtRuler(cursor ?? 0, 0.01, long)}
      onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={() => (dragging = false)} onkeydown={key}>
      {#if selection && selection.b > selection.a}<div class="sel" style="left:{pct(selection.a)};width:{((selection.b - selection.a) / duration) * 100}%"></div>{/if}
      {#each ticks as k (k.t)}
        <div class="tk" class:major={k.major} style="left:{pct(k.t)}">{#if k.label}<span>{k.label}</span>{/if}</div>
      {/each}
      {#each markers as m}<div class="mk" style="left:{pct(m.time)}" title={m.label}></div>{/each}
      {#if cursor !== null && playhead === null}<div class="cur" style="left:{pct(cursor)}"></div>{/if}
      {#if playhead !== null}<div class="ph" style="left:{pct(playhead)}"></div>{/if}
    </div>
  </div>
</div>
<style>
  .rwrap { position: sticky; top: 0; z-index: 4; padding: 0 12px; border: 1px solid transparent; flex: none; }
  .scroll { overflow-x: auto; overflow-y: hidden; scrollbar-width: none; border-radius: 6px; background: var(--inset-bg); box-shadow: var(--inset-shadow), 0 4px 10px #0007; }
  .scroll::-webkit-scrollbar { display: none; }
  .inner { position: relative; height: 26px; cursor: text; outline: none; min-width: 100%; }
  .inner:focus-visible { box-shadow: 0 0 0 2px var(--blue) inset; }
  .tk { position: absolute; bottom: 0; width: 1px; height: 5px; background: #ffffff33; pointer-events: none; }
  .tk.major { height: 11px; background: #ffffff80; }
  .tk span { position: absolute; left: 4px; bottom: 9px; font-size: 10px; font-weight: 700; letter-spacing: .03em; color: var(--muted); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .sel { position: absolute; top: 0; bottom: 0; background: rgb(var(--blue-rgb, 77 155 255) / .25); border-left: 1px solid var(--blue); border-right: 1px solid var(--blue); pointer-events: none; }
  .mk { position: absolute; top: 0; width: 0; height: 0; margin-left: -4px; border: 4px solid transparent; border-top: 6px solid var(--blue-hi); pointer-events: auto; }
  .cur { position: absolute; top: 0; bottom: 0; width: 1px; background: var(--text); pointer-events: none; }
  .ph { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--accent); box-shadow: 0 0 6px var(--accent); pointer-events: none; }
  .ph::before { content: ""; position: absolute; top: 0; left: -4px; border: 5px solid transparent; border-top: 7px solid var(--accent); }
</style>
