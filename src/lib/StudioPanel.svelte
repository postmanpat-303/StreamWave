<script lang="ts">
  import type { Analysis } from "./api";
  type S = "Subtle" | "Medium" | "Strong";
  let { enabled, ab, busy, analysis, onrun, onab }: { enabled: boolean; ab: boolean; busy: boolean; analysis: Analysis | null;
    onrun: (s: S) => void; onab: () => void } = $props();
  let strength = $state<S>("Medium");
</script>
<div class="panel">
  <button class="big" disabled={!enabled || busy} onclick={() => onrun(strength)}>
    {busy ? "Listening to your track..." : "✨ Studio Sound"}
  </button>
  <div class="seg">
    {#each ["Subtle", "Medium", "Strong"] as s}<button class:sel={strength === s} onclick={() => { strength = s as S; if (analysis) onrun(strength); }}>{s}</button>{/each}
  </div>
  <button class="ab" class:b={ab} onclick={onab} disabled={!analysis}>{ab ? "Hearing: before" : "Hearing: after"} · tap to compare</button>
  {#if analysis}
    <div class="stats">
      <span>Noise {analysis.snr_db.toFixed(0)} dB below voice</span>
      {#if analysis.lufs !== null}<span>{analysis.lufs.toFixed(1)} → -16 LUFS</span>{/if}
    </div>
    {#if analysis.notes.length}<ul>{#each analysis.notes as n}<li>{n}</li>{/each}</ul>{:else}<div class="muted">Already sounds clean. Applied gentle polish.</div>{/if}
    <div class="muted">The chain below is fully editable, and Ctrl+Z undoes it.</div>
  {:else}
    <div class="muted">One click: learns the background noise, removes hiss/hum, tames pops and harsh "s" sounds, balances the tone, evens out levels and sets streaming-ready loudness.</div>
  {/if}
</div>
<style>
  .panel { background: linear-gradient(160deg, rgb(var(--accent-rgb) / 0.08), var(--panel)); border: 1px solid rgb(var(--accent-rgb) / 0.33); border-radius: 12px; padding: 14px; }
  .big { width: 100%; padding: 14px; font-size: 17px; font-weight: 700; border-radius: 12px; background: linear-gradient(90deg, var(--accent), var(--blue)); color: #021; border: none; }
  .big:disabled { opacity: .5; }
  .seg { display: flex; gap: 4px; margin: 8px 0; } .seg button { flex: 1; font-size: 12px; padding: 5px; } .seg .sel { background: var(--accent); color: #002; border-color: var(--accent); }
  .ab { width: 100%; font-size: 12px; } .ab.b { border-color: var(--warn); color: var(--warn); }
  .stats { display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); margin-top: 8px; }
  ul { margin: 6px 0; padding-left: 18px; font-size: 12px; } .muted { color: var(--muted); font-size: 11px; margin-top: 6px; }
</style>
