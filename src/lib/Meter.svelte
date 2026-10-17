<script lang="ts">
  let { left = 0, right = 0, status = "ok" }: { left?: number; right?: number; status?: "ok" | "warn" | "bad" | "dead" } = $props();
  let holdL = $state(0), holdR = $state(0);
  const db = (v: number) => (v <= 0.00001 ? -60 : Math.max(-60, 20 * Math.log10(v)));
  const pct = (v: number) => ((db(v) + 60) / 60) * 100;
  $effect(() => { holdL = Math.max(left, holdL * 0.97); holdR = Math.max(right, holdR * 0.97); });
</script>
<div class="meter {status}">
  {#each [[left, holdL, "L"], [right, holdR, "R"]] as [v, h, n]}
    <div class="row"><span>{n}</span>
      <div class="bar"><div class="fill" style="width:{pct(v as number)}%"></div><div class="hold" style="left:{pct(h as number)}%"></div></div>
    </div>
  {/each}
  <div class="scale"><span>-60</span><span>-24</span><span>-12</span><span>-6</span><span>0 dB</span></div>
</div>
<style>
  .meter { width: 100%; }
  .meter.warn .bar { box-shadow: 0 0 0 1px var(--warn); } .meter.warn .fill { filter: hue-rotate(-120deg) saturate(1.3); }
  .meter.bad .bar { box-shadow: 0 0 0 1px var(--rec); animation: flash .6s 2; }
  .meter.dead .bar { box-shadow: 0 0 0 1px #666; } .meter.dead .fill { filter: grayscale(1); }
  @keyframes flash { 50% { box-shadow: 0 0 0 3px var(--rec); } }
  .row { display: flex; align-items: center; gap: 8px; margin: 4px 0; font-size: 11px; color: var(--muted); }
  .bar { position: relative; flex: 1; height: 12px; background: #0a0d12; border-radius: 6px; overflow: hidden; }
  .fill { height: 100%; background: linear-gradient(90deg, #19d3b0 0%, #19d3b0 70%, #ffb020 85%, #ff3b5c 100%); background-size: 100vw 100%; transition: width 40ms linear; }
  .hold { position: absolute; top: 0; width: 2px; height: 100%; background: #fff; }
  .scale { display: flex; justify-content: space-between; font-size: 10px; color: var(--muted); padding-left: 16px; }
</style>
