<script lang="ts">
  /** Vertical gradient level meter (teal → yellow → red) for one or two channels, with peak hold and a dB scale. Values are linear amplitude. */
  import { ampPos, dbPos } from "./meter";
  let { left = 0, right = undefined, holdL = 0, holdR = 0, height = 110, label = "" }:
    { left?: number; right?: number; holdL?: number; holdR?: number; height?: number; label?: string } = $props();
  const chans = $derived(right === undefined ? [[left, holdL]] : [[left, holdL], [right, holdR]]);
  const marks = [0, -6, -12, -24, -40, -60];
</script>
<div class="lm" style="--h:{height}px">
  <div class="body">
    <div class="scale" aria-hidden="true">{#each marks as m}<span style="bottom:{dbPos(m) * 100}%">{m}</span>{/each}</div>
    {#each chans as [v, h], i}
      <div class="ch" role="meter" aria-label="{label || 'Level'} {chans.length > 1 ? (i ? 'right' : 'left') : ''}" aria-valuemin={0} aria-valuemax={1} aria-valuenow={+ampPos(v).toFixed(3)}>
        <div class="fill" style="clip-path: inset({(1 - ampPos(v)) * 100}% 0 0 0)"></div>
        {#if h > 0}<div class="hold" style="bottom:{ampPos(h) * 100}%"></div>{/if}
      </div>
    {/each}
  </div>
  {#if label}<div class="lbl">{label}</div>{/if}
</div>
<style>
  .lm { display: flex; flex-direction: column; align-items: center; gap: 5px; }
  .body { display: flex; gap: 4px; height: var(--h); align-items: stretch; }
  .scale { position: relative; width: 20px; font-size: 8.5px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .scale span { position: absolute; right: 2px; transform: translateY(50%); }
  .ch { position: relative; width: 9px; border-radius: 3px; overflow: hidden; background: repeating-linear-gradient(0deg, #0b0c0e 0 2px, #17181b 2px 4px); box-shadow: 0 1px 2px #000c inset, 0 1px 0 #ffffff0d; }
  .fill { position: absolute; inset: 0; transition: clip-path 40ms linear;
    background: linear-gradient(0deg, var(--teal) 0%, var(--teal) 72%, var(--warn) 82%, var(--warn) 92%, var(--rec) 100%);
    -webkit-mask: repeating-linear-gradient(0deg, #000 0 3px, #0006 3px 4px); mask: repeating-linear-gradient(0deg, #000 0 3px, #0006 3px 4px); }
  .hold { position: absolute; left: 0; right: 0; height: 2px; background: #fff; box-shadow: 0 0 3px #fff; }
  .lbl { font-size: 10px; font-weight: 800; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
</style>
