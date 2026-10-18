<script lang="ts">
  import type { Label } from "./editModel";
  type Phase = "down" | "move";
  let { labels, duration, selection = null, active = false, focusId = null, onselect, onedit, ondelete, onpick }:
    { labels: Label[]; duration: number; selection?: { a: number; b: number } | null; active?: boolean; focusId?: number | null;
      onselect: (a: number, b: number, e: PointerEvent, phase: Phase) => void; onedit: (id: number, text: string) => void; ondelete: (id: number) => void; onpick: (l: Label) => void } = $props();
  let lane: HTMLDivElement;
  let anchor = 0, dragging = false;
  const pct = (t: number) => (duration > 0 ? (t / duration) * 100 : 0);
  const timeAt = (e: { clientX: number }) => { const r = lane.getBoundingClientRect(); return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * duration; };
  function down(e: PointerEvent) {
    if (e.button !== 0 || e.target !== lane) return;
    const t = timeAt(e);
    anchor = e.shiftKey && selection ? (Math.abs(t - selection.a) > Math.abs(t - selection.b) ? selection.a : selection.b) : t;
    dragging = true; lane.setPointerCapture(e.pointerId);
    onselect(Math.min(anchor, t), Math.max(anchor, t), e, "down");
  }
  function move(e: PointerEvent) { if (dragging) { const t = timeAt(e); onselect(Math.min(anchor, t), Math.max(anchor, t), e, "move"); } }
  function up(e: PointerEvent) { dragging = false; lane.releasePointerCapture?.(e.pointerId); }
  function focusIf(el: HTMLInputElement, id: number) { if (id === focusId) queueMicrotask(() => { el.focus(); el.select(); }); }
  function key(e: KeyboardEvent, l: Label) {
    if (e.key === "Enter" || e.key === "Escape") { (e.target as HTMLInputElement).blur(); e.preventDefault(); }
    if (e.key === "Backspace" && (e.target as HTMLInputElement).value === "" && l.text === "") { e.preventDefault(); ondelete(l.id); }
    e.stopPropagation();
  }
</script>
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="lane" class:active bind:this={lane} onpointerdown={down} onpointermove={move} onpointerup={up}>
  {#each labels as l, i (l.id)}
    {#if l.t1 - l.t0 > 0.0005}<div class="range" style="left:{pct(l.t0)}%;width:{pct(l.t1 - l.t0)}%"></div>{/if}
    <div class="lab" style="left:{pct(l.t0)}%;top:{4 + (i % 2) * 24}px">
      <button class="pin" title="Select this label's {l.t1 - l.t0 > 0.0005 ? 'range' : 'position'}" onclick={() => onpick(l)}>▾</button>
      <input value={l.text} placeholder="label" size={Math.max(4, Math.min(30, l.text.length + 1))} use:focusIf={l.id}
        onkeydown={(e) => key(e, l)} onchange={(e) => onedit(l.id, (e.target as HTMLInputElement).value)} />
      <button class="del" title="Delete label" onclick={() => ondelete(l.id)}>✕</button>
    </div>
  {/each}
  {#if selection}
    {#if selection.b - selection.a > 0.0005}<div class="sel" style="left:{pct(selection.a)}%;width:{pct(selection.b - selection.a)}%"></div>
    {:else}<div class="cursor" style="left:{pct(selection.a)}%"></div>{/if}
  {/if}
  {#if !labels.length}<div class="empty">Click to place the cursor (or drag a range), then press Ctrl+B to add a label</div>{/if}
</div>
<style>
  .lane { position: relative; width: 100%; height: 56px; border-radius: 6px; background: #ffffff06; cursor: text; touch-action: none; overflow: hidden; }
  .lane.active { box-shadow: inset 0 0 0 1px #19d3b055; }
  .range { position: absolute; top: 0; bottom: 0; background: #ffb02018; border-left: 1px solid #ffb020; border-right: 1px solid #ffb020; pointer-events: none; }
  .lab { position: absolute; display: flex; align-items: center; gap: 1px; border-left: 2px solid #ffb020; padding-left: 1px; z-index: 1; }
  .lab input { font-size: 11px; padding: 1px 4px; background: #0e1117ee; color: #ffd27a; border: 1px solid #ffb02055; border-radius: 4px; min-width: 0; }
  .lab input:focus { outline: 1px solid #ffb020; }
  .pin, .del { font-size: 10px; padding: 0 3px; background: none; border: none; color: #ffb020; cursor: pointer; }
  .del { opacity: 0; } .lab:hover .del, .lab:focus-within .del { opacity: 0.8; }
  .sel { position: absolute; top: 0; bottom: 0; background: #4da3ff30; border-left: 1px solid #7dbbff; border-right: 1px solid #7dbbff; pointer-events: none; }
  .cursor { position: absolute; top: 0; bottom: 0; width: 1px; background: #fff; pointer-events: none; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; font-size: 11px; color: #ffffff55; pointer-events: none; }
</style>
