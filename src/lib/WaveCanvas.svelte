<script lang="ts">
  let { peaks, duration, selection = null, progress = 0, color = "#19d3b0", active = false, markers = [], onselect }:
    { peaks: number[]; duration: number; selection?: { a: number; b: number } | null; progress?: number; color?: string; active?: boolean; markers?: { id: number; time: number; label: string }[];
      onselect: (a: number, b: number) => void } = $props();
  let canvas: HTMLCanvasElement;
  let w = $state(800), h = $state(90);
  let anchor = 0, dragging = false;

  $effect(() => {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = w * dpr; canvas.height = h * dpr;
    const ctx = canvas.getContext("2d")!;
    ctx.scale(dpr, dpr); ctx.clearRect(0, 0, w, h);
    const n = peaks.length / 2, mid = h / 2;
    ctx.fillStyle = "#ffffff10"; ctx.fillRect(0, mid, w, 1);
    for (let x = 0; x < w; x++) {
      const a = Math.floor((x / w) * n), b = Math.max(a + 1, Math.floor(((x + 1) / w) * n));
      let mn = 0, mx = 0;
      for (let i = a; i < b && i < n; i++) { mn = Math.min(mn, peaks[i * 2]); mx = Math.max(mx, peaks[i * 2 + 1]); }
      ctx.fillStyle = x / w < progress ? "#ffffff" : color;
      ctx.fillRect(x, mid - mx * mid, 1, Math.max(1, (mx - mn) * mid));
    }
  });

  const timeAt = (e: PointerEvent) => {
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
    onselect(Math.min(anchor, t), Math.max(anchor, t));
  }
  function move(e: PointerEvent) { if (dragging) { const t = timeAt(e); onselect(Math.min(anchor, t), Math.max(anchor, t)); } }
  function up(e: PointerEvent) { dragging = false; canvas.releasePointerCapture?.(e.pointerId); }
  const pct = (t: number) => (duration > 0 ? (t / duration) * 100 : 0);
</script>
<div class="wrap" class:active bind:clientWidth={w} bind:clientHeight={h}>
  <canvas bind:this={canvas} style="width:{w}px;height:{h}px" onpointerdown={down} onpointermove={move} onpointerup={up}></canvas>
  {#each markers.filter((m) => m.time <= duration) as m (m.id)}
    <div class="mk" style="left:{pct(m.time)}%" title={m.label}><span>{m.label}</span></div>
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
  .wrap { position: relative; width: 100%; height: 90px; border-radius: 6px; }
  .wrap.active { box-shadow: inset 0 0 0 1px #19d3b055; }
  canvas { display: block; cursor: text; touch-action: none; }
  .sel { position: absolute; top: 0; bottom: 0; background: #4da3ff40; border-left: 1px solid #7dbbff; border-right: 1px solid #7dbbff; pointer-events: none; }
  .mk { position: absolute; top: 0; bottom: 0; width: 0; border-left: 1px dashed #ffb020; pointer-events: none; }
  .mk span { position: absolute; top: 0; left: 2px; font-size: 9px; color: #ffb020; white-space: nowrap; background: #0e1117cc; padding: 0 3px; border-radius: 3px; }
  .cursor { position: absolute; top: 0; bottom: 0; width: 1px; background: #fff; pointer-events: none; }
</style>
