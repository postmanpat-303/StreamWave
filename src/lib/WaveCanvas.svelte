<script lang="ts">
  import { renderSpec, freqTicks, type Spec } from "./spectrogram";
  import type { SpecColors, SpecScale } from "./editModel";
  type Phase = "down" | "move";
  /** An audio clip drawn on this lane (gaps are not listed). `fx` = active Layering Effects. */
  type LaneClip = { a: number; b: number; index: number; name: string; fx: number };
  let { peaks, duration, selection = null, progress = 0, color = "#2ec4b6", active = false, markers = [], bounds = [], spec = null, specScale = "log", specColors = "classic",
        clips = [], movingIndex = null, onselect, onpickclip, oncontext, ongrab, onlayers }:
    { peaks: number[]; duration: number; selection?: { a: number; b: number } | null; progress?: number; color?: string; active?: boolean; markers?: { id: number; time: number; label: string }[];
      bounds?: { t: number; split: boolean }[]; spec?: Spec | null; specScale?: SpecScale; specColors?: SpecColors;
      clips?: LaneClip[]; movingIndex?: number | null;
      onselect: (a: number, b: number, e: PointerEvent, phase: Phase) => void; onpickclip?: (t: number) => void; oncontext?: (t: number, e: MouseEvent) => void;
      /** pointer down on a clip's title bar: start moving it */ ongrab?: (index: number, e: PointerEvent, grabAt: number) => void;
      /** the clip's Layering Effects button */ onlayers?: (index: number) => void } = $props();
  let canvas: HTMLCanvasElement;
  let w = $state(800), h = $state(90);
  let anchor = 0, dragging = false;
  const specImg = $derived.by(() => {
    if (!spec || h < 2) return null;
    const H = Math.round(h * (window.devicePixelRatio || 1));
    return new ImageData(renderSpec(spec, H, specScale, specColors), spec.cols, H);
  });
  const ticks = $derived(spec ? freqTicks(h, specScale, spec) : []);

  $effect(() => {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = w * dpr; canvas.height = h * dpr;
    const ctx = canvas.getContext("2d")!;
    ctx.scale(dpr, dpr); ctx.clearRect(0, 0, w, h);
    // clip blocks: audio clips get a faint body, silence (gaps) stays empty
    if (duration > 0) for (const c of clips) { ctx.fillStyle = "#ffffff08"; ctx.fillRect((c.a / duration) * w, 0, ((c.b - c.a) / duration) * w, h); }
    if (specImg) {
      const off = document.createElement("canvas"); off.width = specImg.width; off.height = specImg.height;
      off.getContext("2d")!.putImageData(specImg, 0, 0);
      ctx.imageSmoothingEnabled = true; ctx.drawImage(off, 0, 0, w, h);
      if (progress > 0) { ctx.fillStyle = "#ffffff22"; ctx.fillRect(0, 0, w * progress, h); }
    } else {
      const n = peaks.length / 2, mid = h / 2;
      ctx.fillStyle = "#ffffff10"; ctx.fillRect(0, mid, w, 1);
      for (let x = 0; x < w; x++) {
        const a = Math.floor((x / w) * n), b = Math.max(a + 1, Math.floor(((x + 1) / w) * n));
        let mn = 0, mx = 0;
        for (let i = a; i < b && i < n; i++) { mn = Math.min(mn, peaks[i * 2]); mx = Math.max(mx, peaks[i * 2 + 1]); }
        ctx.fillStyle = x / w < progress ? "#ffffff" : color;
        ctx.fillRect(x, mid - mx * mid, 1, Math.max(1, (mx - mn) * mid));
      }
    }
    // clip boundaries: deliberate splits bright, other joins between sources faint
    for (const bd of bounds) {
      if (duration <= 0) break;
      const x = Math.round((bd.t / duration) * w) + 0.5;
      ctx.fillStyle = bd.split ? "#ffffffcc" : "#ffffff30"; ctx.fillRect(x - 0.5, 0, 1, h);
      if (bd.split) { ctx.beginPath(); ctx.moveTo(x - 4, 0); ctx.lineTo(x + 4, 0); ctx.lineTo(x, 5); ctx.fill(); }
    }
  });

  const timeAt = (e: { clientX: number }) => {
    const r = canvas.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * duration;
  };
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    const t = timeAt(e);
    if (e.shiftKey && selection) {
      // extend: keep the end farthest from the click as the anchor
      anchor = Math.abs(t - selection.a) > Math.abs(t - selection.b) ? selection.a : selection.b;
    } else anchor = t;
    dragging = true; canvas.setPointerCapture(e.pointerId);
    onselect(Math.min(anchor, t), Math.max(anchor, t), e, "down");
  }
  function move(e: PointerEvent) { if (dragging) { const t = timeAt(e); onselect(Math.min(anchor, t), Math.max(anchor, t), e, "move"); } }
  function up(e: PointerEvent) { dragging = false; canvas.releasePointerCapture?.(e.pointerId); }
  const pct = (t: number) => (duration > 0 ? (t / duration) * 100 : 0);
</script>
<div class="wrap" class:active class:spec={!!spec} bind:clientWidth={w} bind:clientHeight={h}>
  <canvas bind:this={canvas} style="width:{w}px;height:{h}px" onpointerdown={down} onpointermove={move} onpointerup={up}
    ondblclick={(e) => onpickclip?.(timeAt(e))}
    oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); oncontext?.(timeAt(e), e); }}></canvas>
  {#each ticks as t (t.label)}<div class="tick" style="top:{t.y}px">{t.label}</div>{/each}
  {#each markers.filter((m) => m.time <= duration) as m (m.id)}
    <div class="mk" style="left:{pct(m.time)}%" title={m.label}><span>{m.label}</span></div>
  {/each}
  {#each clips as c (c.index)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="clipbar" class:moving={movingIndex === c.index} style="left:{pct(c.a)}%;width:{pct(c.b - c.a)}%" title="{c.name}: drag to move (Alt = no snapping)"
      onpointerdown={(e) => { if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return; e.preventDefault(); e.stopPropagation(); ongrab?.(c.index, e, timeAt(e)); }}
      oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); oncontext?.(timeAt(e), e); }}>
      <span class="cn">{c.name}</span>
      {#if onlayers}<button class="lfx" class:has={c.fx > 0} title="Layering Effects for this clip" aria-label="Layering Effects for clip {c.name}" onclick={(e) => { e.stopPropagation(); onlayers(c.index); }}>◧{c.fx ? ` ${c.fx}` : ""}</button>{/if}
    </div>
    {#if movingIndex === c.index}<div class="clipdim" style="left:{pct(c.a)}%;width:{pct(c.b - c.a)}%"></div>{/if}
  {/each}
  {#if selection}
    {#if selection.b - selection.a > 0.0005}
      <div class="sel" style="left:{pct(selection.a)}%;width:{pct(selection.b - selection.a)}%"></div>
    {:else}
      <div class="cursor" style="left:{pct(selection.a)}%"></div>
    {/if}
  {/if}
</div>
<style>
  .wrap { position: relative; width: 100%; height: 90px; border-radius: 6px; overflow: hidden; }
  .wrap.spec { height: 140px; }
  .wrap.active { box-shadow: inset 0 0 0 1px rgb(var(--accent-rgb) / 0.33); }
  canvas { display: block; cursor: text; touch-action: none; }
  .sel { position: absolute; top: 0; bottom: 0; background: rgb(var(--blue-rgb) / 0.25); border-left: 1px solid var(--blue-hi); border-right: 1px solid var(--blue-hi); pointer-events: none; }
  .mk { position: absolute; top: 0; bottom: 0; width: 0; border-left: 1px dashed var(--warn); pointer-events: none; }
  .mk span { position: absolute; top: 0; left: 2px; font-size: 9px; color: var(--warn); white-space: nowrap; background: rgb(var(--bg-rgb) / 0.80); padding: 0 3px; border-radius: 3px; }
  .cursor { position: absolute; top: 0; bottom: 0; width: 1px; background: #fff; pointer-events: none; }
  .clipbar { position: absolute; top: 0; height: 16px; display: flex; align-items: center; gap: 4px; padding: 0 3px 0 5px; box-sizing: border-box; min-width: 4px;
    background: rgb(var(--accent-rgb) / 0.16); border: 1px solid rgb(var(--accent-rgb) / 0.33); border-radius: 4px 4px 0 0; cursor: grab; overflow: hidden; touch-action: none; z-index: 2; }
  .clipbar:hover { background: rgb(var(--accent-rgb) / 0.27); } .clipbar.moving { opacity: .35; cursor: grabbing; }
  .cn { flex: 1; min-width: 0; font-size: 9.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; pointer-events: none; }
  .lfx { flex: none; font-size: 10px; line-height: 12px; padding: 0 4px; border-radius: 3px; background: rgb(var(--bg-rgb) / 0.80); border: 1px solid #ffffff33; color: var(--text); cursor: pointer; }
  .lfx.has { color: var(--bg); background: var(--accent); border-color: var(--accent); font-weight: 700; }
  .clipdim { position: absolute; top: 0; bottom: 0; background: rgb(var(--bg-rgb) / 0.67); pointer-events: none; z-index: 1; }
  .tick { position: absolute; left: 2px; transform: translateY(-50%); font-size: 9px; color: #fff; background: #0008; padding: 0 3px; border-radius: 3px; pointer-events: none; }
</style>
