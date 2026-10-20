<script lang="ts">
  // Input meter: LED dot rows (L/R, zone-coloured, peak hold) + a compact vertical gradient meter. Values are linear amplitude.
  import LedMeter from "./ui/LedMeter.svelte";
  import LevelMeter from "./ui/LevelMeter.svelte";
  import { ampPos } from "./ui/meter";
  let { left = 0, right = 0, status = "ok" }: { left?: number; right?: number; status?: "ok" | "warn" | "bad" | "dead" } = $props();
  let holdL = $state(0), holdR = $state(0);
  $effect(() => { holdL = Math.max(left, holdL * 0.97); holdR = Math.max(right, holdR * 0.97); });
  const statusText = { ok: "", warn: "Level warning", bad: "Clipping or no signal", dead: "No input" } as const;
</script>
<div class="meter {status}" title={statusText[status]}>
  <div class="rows">
    <LedMeter label="L" value={ampPos(left)} hold={ampPos(holdL)} n={24} />
    <LedMeter label="R" value={ampPos(right)} hold={ampPos(holdR)} n={24} />
    <div class="scale" aria-hidden="true"><span>-60</span><span>-24</span><span>-12</span><span>-6</span><span>0 dB</span></div>
  </div>
  <LevelMeter {left} {right} {holdL} {holdR} height={62} label="IN" />
</div>
<style>
  .meter { width: 100%; display: flex; align-items: center; gap: 12px; padding: 6px 10px; border-radius: 8px; background: var(--inset-bg); box-shadow: var(--inset-shadow); }
  .rows { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 5px; }
  .meter.warn { box-shadow: var(--inset-shadow), 0 0 0 1px var(--warn); }
  .meter.bad { box-shadow: var(--inset-shadow), 0 0 0 1px var(--rec); animation: flash .6s 2; }
  .meter.dead { box-shadow: var(--inset-shadow), 0 0 0 1px #666; } .meter.dead .rows { filter: grayscale(1); }
  @keyframes flash { 50% { box-shadow: var(--inset-shadow), 0 0 0 3px var(--rec); } }
  .scale { display: flex; justify-content: space-between; font-size: 9.5px; font-weight: 700; color: var(--muted); padding-left: 26px; letter-spacing: .03em; }
</style>
