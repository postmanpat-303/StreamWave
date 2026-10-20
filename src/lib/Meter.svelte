<script lang="ts">
  // Input meter: LED dot rows (L/R, zone-coloured, peak hold) + a compact vertical gradient meter. Values are linear amplitude.
  import LedMeter from "./ui/LedMeter.svelte";
  import LevelMeter from "./ui/LevelMeter.svelte";
  import { ampPos, dbPos } from "./ui/meter";
  const N = 32; // LED dots per channel; the scale below is placed with the same dB→position mapping
  let { left = 0, right = 0, status = "ok" }: { left?: number; right?: number; status?: "ok" | "warn" | "bad" | "dead" } = $props();
  let holdL = $state(0), holdR = $state(0);
  $effect(() => { holdL = Math.max(left, holdL * 0.97); holdR = Math.max(right, holdR * 0.97); });
  const statusText = { ok: "", warn: "Level warning", bad: "Clipping or no signal", dead: "No input" } as const;
</script>
<div class="meter {status}" title={statusText[status]}>
  <div class="rows" style="--n:{N}">
    <LedMeter label="L" value={ampPos(left)} hold={ampPos(holdL)} n={N} />
    <LedMeter label="R" value={ampPos(right)} hold={ampPos(holdR)} n={N} />
    <div class="scale" aria-hidden="true">{#each [-60, -40, -24, -12, -6, 0] as m}<span style="left:{dbPos(m) * 100}%">{m}{m === 0 ? " dB" : ""}</span>{/each}</div>
  </div>
  <LevelMeter {left} {right} {holdL} {holdR} height={64} label="IN" marks={[0, -12, -24, -60]} />
</div>
<style>
  .meter { width: 100%; display: flex; align-items: center; gap: 12px; padding: 6px 10px; border-radius: 8px; background: var(--inset-bg); box-shadow: var(--inset-shadow); }
  .rows { flex: 0 1 calc(26px + var(--n) * 14px - 3px); min-width: 0; display: flex; flex-direction: column; gap: 5px; margin-right: auto; }
  .meter.warn { box-shadow: var(--inset-shadow), 0 0 0 1px var(--warn); }
  .meter.bad { box-shadow: var(--inset-shadow), 0 0 0 1px var(--rec); animation: flash .6s 2; }
  .meter.dead { box-shadow: var(--inset-shadow), 0 0 0 1px #666; } .meter.dead .rows { filter: grayscale(1); }
  @keyframes flash { 50% { box-shadow: var(--inset-shadow), 0 0 0 3px var(--rec); } }
  .scale { position: relative; height: 12px; margin-left: 26px; margin-right: 4px; font-size: 9.5px; font-weight: 700; color: var(--muted); letter-spacing: .03em; }
  .scale span { position: absolute; transform: translateX(-50%); white-space: nowrap; } .scale span:first-child { transform: none; } .scale span:last-child { transform: translateX(-80%); }
</style>
