<script lang="ts">
  // Compact overview of the selected (active) track: whole-track waveform, selection, playhead, the part the main
  // timeline currently shows, and exact numbers. Click = move cursor / seek, drag = select. Deliberately much smaller than the timeline.
  import { fmtRuler } from "./ruler";
  let { name, peaks, duration, selection = null, cursor = null, playhead = null, view = null, color = "#2ec4b6", onseek, onselect, onzoomsel, onfit, zoomed = false }: {
    name: string; peaks: number[]; duration: number; selection?: { a: number; b: number } | null; cursor?: number | null; playhead?: number | null;
    view?: { a: number; b: number } | null; color?: string; zoomed?: boolean;
    onseek: (t: number) => void; onselect: (a: number, b: number) => void; onzoomsel: () => void; onfit: () => void } = $props();
  let cv: HTMLCanvasElement; let w = $state(0), h = $state(0);
  const hasSel = $derived(!!selection && selection.b - selection.a > 0.0005);
  const f = (t: number) => fmtRuler(t, 0.01, duration >= 3600);
  $effect(() => {
    const p = peaks, cw = w, ch = h; if (!cv || !cw || !ch) return;
    const dpr = window.devicePixelRatio || 1; cv.width = Math.round(cw * dpr); cv.height = Math.round(ch * dpr);
    const g = cv.getContext("2d"); if (!g) return; g.setTransform(dpr, 0, 0, dpr, 0, 0); g.clearRect(0, 0, cw, ch);
    const n = p.length / 2, mid = ch / 2; if (!n) return;
    g.fillStyle = color; g.globalAlpha = 0.9;
    for (let x = 0; x < cw; x++) {
      const a = Math.floor((x / cw) * n), b = Math.max(a + 1, Math.floor(((x + 1) / cw) * n)); let mn = 0, mx = 0;
      for (let i = a; i < b && i < n; i++) { mn = Math.min(mn, p[i * 2]); mx = Math.max(mx, p[i * 2 + 1]); }
      const y0 = mid - mx * (mid - 1), y1 = mid - mn * (mid - 1); g.fillRect(x, y0, 1, Math.max(1, y1 - y0));
    }
  });
  const pct = (t: number) => `${duration > 0 ? (Math.max(0, Math.min(duration, t)) / duration) * 100 : 0}%`;
  let drag: { x0: number; t0: number; moved: boolean } | null = null;
  function tAt(e: PointerEvent) { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); return r.width ? Math.max(0, Math.min(duration, ((e.clientX - r.left) / r.width) * duration)) : 0; }
  function down(e: PointerEvent) { if (e.button !== 0 || !duration) return; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); drag = { x0: e.clientX, t0: tAt(e), moved: false }; }
  function move(e: PointerEvent) { if (!drag) return; if (Math.abs(e.clientX - drag.x0) > 3) drag.moved = true; if (drag.moved) { const t = tAt(e); onselect(Math.min(t, drag.t0), Math.max(t, drag.t0)); } }
  function up(e: PointerEvent) { if (!drag) return; if (!drag.moved) onseek(drag.t0); drag = null; }
</script>
<div class="pv" aria-label="Selected audio preview">
  <div class="meta">
    <span class="nm" title={name}>{name}</span>
    <span class="rd"><i>LEN</i>{f(duration)}</span>
    <span class="rd" class:dim={!hasSel}><i>START</i>{f(hasSel ? selection!.a : cursor ?? 0)}</span>
    <span class="rd" class:dim={!hasSel}><i>END</i>{hasSel ? f(selection!.b) : "–"}</span>
    <span class="rd" class:dim={!hasSel}><i>SEL</i>{hasSel ? f(selection!.b - selection!.a) : "–"}</span>
    <span class="rd ph" class:dim={playhead === null}><i>PLAY</i>{playhead === null ? "–" : f(playhead)}</span>
    <span class="btns">
      <button class="pill small" disabled={!hasSel} onclick={onzoomsel} title="Zoom the timeline to the selection">Zoom to selection</button>
      <button class="pill small" disabled={!zoomed} onclick={onfit} title="Show the whole timeline">Fit</button>
    </span>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="ov" bind:clientWidth={w} bind:clientHeight={h} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={() => (drag = null)}
    title="Click to move the cursor (seeks while playing) · drag to select">
    <canvas bind:this={cv}></canvas>
    {#if view && zoomed}<div class="vw" style="left:{pct(view.a)};width:calc({pct(view.b)} - {pct(view.a)})"></div>{/if}
    {#if hasSel}<div class="sel" style="left:{pct(selection!.a)};width:calc({pct(selection!.b)} - {pct(selection!.a)})"></div>{/if}
    {#if cursor !== null && !hasSel && playhead === null}<div class="cur" style="left:{pct(cursor)}"></div>{/if}
    {#if playhead !== null}<div class="phl" style="left:{pct(playhead)}"></div>{/if}
  </div>
</div>
<style>
  .pv { display: flex; flex-direction: column; gap: 4px; padding: 7px 12px 9px; flex: none; background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); }
  .meta { display: flex; align-items: center; gap: 12px; font-size: 11.5px; min-width: 0; flex-wrap: wrap; row-gap: 4px; }
  .nm { font-weight: 800; text-transform: uppercase; letter-spacing: .05em; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rd { font-variant-numeric: tabular-nums; color: var(--text); white-space: nowrap; } .rd i { font-style: normal; font-size: 9.5px; font-weight: 800; letter-spacing: .06em; color: var(--muted); margin-right: 5px; }
  .rd.ph { color: var(--accent); } .dim { opacity: .45; }
  .btns { margin-left: auto; display: flex; gap: 6px; } .btns button { padding: 2px 10px; font-size: 11px; }
  .ov { position: relative; height: 40px; border-radius: 6px; background: var(--inset-bg); box-shadow: var(--inset-shadow); overflow: hidden; cursor: text; touch-action: none; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; }
  .vw { position: absolute; top: 0; bottom: 0; border: 1px solid #ffffff55; background: #ffffff0d; border-radius: 3px; pointer-events: none; box-sizing: border-box; }
  .sel { position: absolute; top: 0; bottom: 0; background: rgb(var(--blue-rgb) / .28); border-left: 1px solid var(--blue); border-right: 1px solid var(--blue); pointer-events: none; box-sizing: border-box; }
  .cur { position: absolute; top: 0; bottom: 0; width: 1px; background: var(--text); pointer-events: none; }
  .phl { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--accent); box-shadow: 0 0 6px var(--accent); pointer-events: none; }
</style>
