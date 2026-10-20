<script lang="ts">
  import { renderSpec, freqTicks, type Spec } from "./spectrogram";
  import { curveGain, type FadeCurve, type SpecColors, type SpecScale, type XfadeCurve } from "./editModel";
  type Phase = "down" | "move";
  /** An audio clip drawn on this lane (gaps are not listed). `fx` = active Layering Effects. */
  type LaneClip = { a: number; b: number; index: number; name: string; fx: number;
    /** own fades (s) and their shape */ fi?: number; fo?: number; fcurve?: FadeCurve;
    /** crossfade with the previous clip: possible here?, heard length (ms), curve */ canX?: boolean; xf?: number; xcurve?: XfadeCurve; /** crossfade at its end (ms; replaces its fade-out) */ xo?: number };
  let { peaks, duration, selection = null, progress = 0, color = "#2ec4b6", active = false, markers = [], bounds = [], spec = null, specScale = "log", specColors = "classic",
        clips = [], movingIndex = null, onselect, onpickclip, oncontext, ongrab, onlayers, onfade, onxfade, onxfademenu }:
    { peaks: number[]; duration: number; selection?: { a: number; b: number } | null; progress?: number; color?: string; active?: boolean; markers?: { id: number; time: number; label: string }[];
      bounds?: { t: number; split: boolean }[]; spec?: Spec | null; specScale?: SpecScale; specColors?: SpecColors;
      clips?: LaneClip[]; movingIndex?: number | null;
      onselect: (a: number, b: number, e: PointerEvent, phase: Phase) => void; onpickclip?: (t: number) => void; oncontext?: (t: number, e: MouseEvent) => void;
      /** pointer down on a clip's title bar: start moving it */ ongrab?: (index: number, e: PointerEvent, grabAt: number) => void;
      /** the clip's Layering Effects button */ onlayers?: (index: number) => void;
      /** fade handle released */ onfade?: (index: number, edge: "in" | "out", ms: number) => void;
      /** crossfade handle dragged + released */ onxfade?: (index: number, ms: number) => void;
      /** crossfade handle clicked (no drag): open its settings */ onxfademenu?: (index: number, e: MouseEvent) => void } = $props();
  // ---- fade / crossfade handles (draft value while dragging; one commit on release) ----
  type HDrag = { index: number; kind: "in" | "out" | "x"; x0: number; moved: boolean; val: number };
  let hdrag = $state<HDrag | null>(null);
  const draft = (index: number, kind: HDrag["kind"]) => (hdrag && hdrag.index === index && hdrag.kind === kind && hdrag.moved ? hdrag.val : null);
  function hDown(e: PointerEvent, c: LaneClip, kind: HDrag["kind"]) {
    if (e.button !== 0) return; e.preventDefault(); e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    hdrag = { index: c.index, kind, x0: e.clientX, moved: false, val: kind === "in" ? c.fi ?? 0 : kind === "out" ? c.fo ?? 0 : (c.xf ?? 0) / 1000 };
  }
  function hMove(e: PointerEvent, c: LaneClip) {
    if (!hdrag) return; if (Math.abs(e.clientX - hdrag.x0) > 3) hdrag.moved = true; if (!hdrag.moved) return;
    const t = timeAt(e), len = c.b - c.a;
    if (hdrag.kind === "in") hdrag.val = Math.max(0, Math.min(len - (c.fo ?? 0), t - c.a));
    else if (hdrag.kind === "out") hdrag.val = Math.max(0, Math.min(len - (c.fi ?? 0), c.b - t));
    else hdrag.val = Math.max(0, 2 * Math.abs(t - c.a)); // the crossfade's edge follows the pointer
  }
  function hUp(e: PointerEvent, c: LaneClip) {
    const d = hdrag; hdrag = null; if (!d) return; e.stopPropagation();
    if (!d.moved) { if (d.kind === "x") onxfademenu?.(c.index, e); return; }
    if (d.kind === "x") onxfade?.(c.index, Math.round(d.val * 1000)); else onfade?.(c.index, d.kind, Math.round(d.val * 1000));
  }
  /** SVG path (viewBox 0..100) of a fade: shaded attenuation above the gain curve + the curve itself. */
  function fadePath(curve: FadeCurve | XfadeCurve | undefined, out: boolean) {
    const pts: string[] = [];
    for (let k = 0; k <= 32; k++) { const x = k / 32, g = curveGain(curve, out ? 1 - x : x); pts.push(`${(x * 100).toFixed(2)},${(100 - g * 100).toFixed(2)}`); }
    return { area: `M0,0 L${pts.join(" L")} L100,0 Z`, line: `M${pts.join(" L")}` };
  }
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
    {@const fiv = (c.xf ?? 0) > 0 ? 0 : draft(c.index, "in") ?? c.fi ?? 0}
    {@const fov = (c.xo ?? 0) > 0 ? 0 : draft(c.index, "out") ?? c.fo ?? 0}
    {#if fiv > 0}{@const fp = fadePath(c.fcurve, false)}<svg class="fade" style="left:{pct(c.a)}%;width:{pct(fiv)}%" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path class="area" d={fp.area} /><path class="line" d={fp.line} /></svg>{/if}
    {#if fov > 0}{@const fp = fadePath(c.fcurve, true)}<svg class="fade" style="left:{pct(c.b - fov)}%;width:{pct(fov)}%" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path class="area" d={fp.area} /><path class="line" d={fp.line} /></svg>{/if}
    {#if onfade && !((c.xf ?? 0) > 0)}
      <div class="fh in" class:set={fiv > 0} style="left:{pct(c.a + fiv)}%" role="slider" tabindex="-1" aria-label="Fade-in of {c.name}" aria-valuenow={Math.round(fiv * 1000)} aria-valuemin={0}
        title="Fade-in {Math.round(fiv * 1000)} ms: drag" onpointerdown={(e) => hDown(e, c, "in")} onpointermove={(e) => hMove(e, c)} onpointerup={(e) => hUp(e, c)} onpointercancel={() => (hdrag = null)}></div>
    {/if}
    {#if onfade && !((c.xo ?? 0) > 0)}
      <div class="fh out" class:set={fov > 0} style="left:{pct(c.b - fov)}%" role="slider" tabindex="-1" aria-label="Fade-out of {c.name}" aria-valuenow={Math.round(fov * 1000)} aria-valuemin={0}
        title="Fade-out {Math.round(fov * 1000)} ms: drag" onpointerdown={(e) => hDown(e, c, "out")} onpointermove={(e) => hMove(e, c)} onpointerup={(e) => hUp(e, c)} onpointercancel={() => (hdrag = null)}></div>
    {/if}
    {#if c.canX && onxfade}
      {@const xv = draft(c.index, "x") ?? (c.xf ?? 0) / 1000}
      {#if xv > 0}{@const po = fadePath(c.xcurve ?? "equal_power", true)}{@const pi = fadePath(c.xcurve ?? "equal_power", false)}
        <svg class="xf" style="left:{pct(c.a - xv / 2)}%;width:{pct(xv)}%" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path class="o" d={po.line} /><path class="i" d={pi.line} /></svg>
      {/if}
      <button class="xh" class:set={xv > 0} style="left:{pct(c.a)}%" title={xv > 0 ? `Crossfade ${Math.round(xv * 1000)} ms (${c.xcurve === "linear" ? "linear" : "equal power"}): drag to change, click for options` : "Crossfade: drag outwards, or click for options"}
        aria-label="Crossfade into {c.name}" onpointerdown={(e) => hDown(e, c, "x")} onpointermove={(e) => hMove(e, c)} onpointerup={(e) => hUp(e, c)} onpointercancel={() => (hdrag = null)}>
        <svg viewBox="0 0 16 10" width="16" height="10" aria-hidden="true"><path d="M1 9 L15 1 M1 1 L15 9" /></svg>{#if hdrag?.kind === "x" && hdrag.index === c.index && hdrag.moved}<span>{Math.round(xv * 1000)} ms</span>{/if}
      </button>
    {/if}
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
  .fade { position: absolute; top: 16px; bottom: 0; pointer-events: none; z-index: 1; overflow: visible; }
  .fade .area { fill: #000000a0; } .fade .line, .xf path { fill: none; stroke-width: 1.5; vector-effect: non-scaling-stroke; } .fade .line { stroke: var(--accent); }
  .fh { position: absolute; top: 16px; width: 9px; height: 9px; box-sizing: border-box; background: var(--panel3); border: 1px solid var(--accent); border-radius: 2px; cursor: ew-resize; z-index: 3; opacity: .45; touch-action: none; }
  .fh.in { border-top-left-radius: 0; } .fh.out { margin-left: -9px; border-top-right-radius: 0; }
  .fh.set { background: var(--accent); opacity: .9; } .wrap:hover .fh { opacity: 1; }
  .xf { position: absolute; top: 16px; bottom: 0; pointer-events: none; z-index: 1; background: rgb(var(--accent-rgb) / .10); border-left: 1px dashed rgb(var(--accent-rgb) / .5); border-right: 1px dashed rgb(var(--accent-rgb) / .5); box-sizing: border-box; }
  .xf .o { stroke: var(--blue-hi); } .xf .i { stroke: var(--accent); }
  /* kept above the bottom 14px: WebKitGTK overlay scrollbars sit there when the timeline is zoomed */
  .xh { position: absolute; bottom: 16px; transform: translateX(-50%); z-index: 4; display: flex; align-items: center; gap: 4px; padding: 1px 4px; border-radius: 4px; font-size: 9.5px; font-weight: 700;
    background: rgb(var(--bg-rgb) / .85); border: 1px solid #ffffff40; color: var(--text); cursor: ew-resize; box-shadow: 0 1px 3px #000a; touch-action: none; opacity: .6; }
  .xh svg path { stroke: currentColor; stroke-width: 1.6; fill: none; } .wrap:hover .xh, .xh.set { opacity: 1; }
  .xh.set { border-color: var(--accent); color: var(--accent); }
  .tick { position: absolute; left: 2px; transform: translateY(-50%); font-size: 9px; color: #fff; background: #0008; padding: 0 3px; border-radius: 3px; pointer-events: none; }
</style>
