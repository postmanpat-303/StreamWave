<script lang="ts">
  // Shown when 2+ audio files are imported at once: how to lay them out. Nothing changes until "Import".
  import type { ImportMode } from "./editModel";
  import { fmtTime } from "./api";
  let { files, playhead, onimport, oncancel }: {
    files: string[]; playhead: number;
    onimport: (order: string[], mode: ImportMode, start: number) => void; oncancel: () => void } = $props();
  // svelte-ignore state_referenced_locally
  let order = $state([...files]); // the dialog is created fresh for each import
  let mode = $state<ImportMode>("sequential");
  // svelte-ignore state_referenced_locally
  let from = $state<"zero" | "playhead">(playhead > 0 ? "playhead" : "zero");
  const name = (p: string) => p.split(/[\\/]/).pop()!;
  function mv(i: number, d: -1 | 1) { const j = i + d; if (j < 0 || j >= order.length) return; const o = [...order]; [o[i], o[j]] = [o[j], o[i]]; order = o; }
  function key(e: KeyboardEvent) { if (e.key === "Escape") { e.stopPropagation(); oncancel(); } }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="modal-back" role="presentation" onkeydown={key} onclick={(e) => { if (e.target === e.currentTarget) oncancel(); }}>
  <div class="modal panel" role="dialog" aria-modal="true" aria-labelledby="iad-h">
    <h3 id="iad-h">Import {order.length} files</h3>
    <div class="modes" role="radiogroup" aria-label="Arrangement">
      <label class="opt" class:sel={mode === "sequential"}>
        <input type="radio" name="iad-mode" value="sequential" bind:group={mode} />
        <span class="pic seq" aria-hidden="true"><i></i><i></i><i></i></span>
        <span><b>One after another on one track</b><br /><span class="small">In the order below, each file starting exactly where the previous one ends. No gaps, no overlaps.</span></span>
      </label>
      <label class="opt" class:sel={mode === "tracks"}>
        <input type="radio" name="iad-mode" value="tracks" bind:group={mode} />
        <span class="pic stack" aria-hidden="true"><i></i><i></i><i></i></span>
        <span><b>One track per file</b><br /><span class="small">All starting at the same time, each track named after its file.</span></span>
      </label>
    </div>
    <div class="from">
      <span class="small">Start at</span>
      <label class="chk"><input type="radio" name="iad-from" value="zero" bind:group={from} /> 0:00</label>
      <label class="chk"><input type="radio" name="iad-from" value="playhead" bind:group={from} disabled={playhead <= 0} /> the cursor ({fmtTime(playhead)})</label>
    </div>
    <ol class="files" aria-label="Files in order">
      {#each order as f, i (i)}
        <li><span class="n">{i + 1}</span><span class="fn" title={f}>{name(f)}</span>
          {#if mode === "sequential"}
            <button class="ic" disabled={i === 0} onclick={() => mv(i, -1)} aria-label="Move {name(f)} earlier">↑</button>
            <button class="ic" disabled={i === order.length - 1} onclick={() => mv(i, 1)} aria-label="Move {name(f)} later">↓</button>
          {/if}
        </li>
      {/each}
    </ol>
    <div class="small">The files are only referenced, never copied or changed. One Ctrl+Z undoes the whole import.</div>
    <div class="row2">
      <button onclick={oncancel}>Cancel</button>
      <button class="primary" onclick={() => onimport(order, mode, from === "playhead" ? playhead : 0)}>Import</button>
    </div>
  </div>
</div>

<style>
  .modal-back { position: fixed; inset: 0; background: #0008; display: grid; place-items: center; z-index: 70; }
  .modal { width: 520px; max-width: 92vw; max-height: 86vh; overflow: auto; background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 16px; }
  h3 { margin: 0 0 10px; }
  .modes { display: flex; flex-direction: column; gap: 8px; }
  .opt { display: flex; gap: 12px; align-items: center; border: 1px solid var(--line); border-radius: 10px; padding: 10px; cursor: pointer; margin: 0; }
  .opt.sel { border-color: var(--accent); background: rgb(var(--accent-rgb) / 0.07); }
  .opt input { width: auto; }
  .pic { flex: none; width: 74px; height: 38px; display: flex; gap: 2px; padding: 3px; border-radius: 6px; background: #0003; }
  .pic i { display: block; border-radius: 3px; background: rgb(var(--accent-rgb) / 0.67); }
  .pic.seq { align-items: center; } .pic.seq i { height: 14px; flex: 1; } .pic.seq i:nth-child(2) { flex: 1.6; background: rgb(var(--purple-rgb) / 0.67); } .pic.seq i:nth-child(3) { flex: .7; background: rgb(var(--warn-rgb) / 0.67); }
  .pic.stack { flex-direction: column; } .pic.stack i { height: 9px; } .pic.stack i:nth-child(1) { width: 60%; } .pic.stack i:nth-child(2) { width: 100%; background: rgb(var(--purple-rgb) / 0.67); } .pic.stack i:nth-child(3) { width: 35%; background: rgb(var(--warn-rgb) / 0.67); }
  .from { display: flex; gap: 14px; align-items: center; margin: 12px 0 6px; } .chk { display: flex; gap: 6px; align-items: center; margin: 0; font-size: 13px; } .chk input { width: auto; }
  .files { list-style: none; margin: 8px 0; padding: 0; max-height: 220px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; }
  .files li { display: flex; align-items: center; gap: 8px; padding: 5px 8px; border-bottom: 1px solid var(--line); font-size: 13px; } .files li:last-child { border-bottom: none; }
  .n { color: var(--muted); width: 18px; text-align: right; font-variant-numeric: tabular-nums; } .fn { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ic { padding: 1px 7px; font-size: 12px; background: none; }
  .small { color: var(--muted); font-size: 11px; }
  .row2 { display: flex; gap: 8px; margin-top: 12px; } .row2 button { flex: 1; }
  .primary { background: var(--accent); color: #002; border-color: var(--accent); font-weight: 600; }
</style>
