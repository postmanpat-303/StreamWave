<script lang="ts">
  /** Glowing LED push button (on = orange glow). role="switch" with aria-checked. */
  let { checked = false, label = "", title = "", disabled = false, onchange }:
    { checked?: boolean; label?: string; title?: string; disabled?: boolean; onchange?: (v: boolean) => void } = $props();
</script>
<button class="lt" class:on={checked} role="switch" aria-checked={checked} aria-label={label || title} {title} {disabled} onclick={() => onchange?.(!checked)}>
  <span class="lens"></span>{#if label}<span class="tx">{label}</span>{/if}
</button>
<style>
  .lt { display: inline-flex; align-items: center; gap: 7px; padding: 4px 9px 4px 5px; border-radius: 999px; font-size: 11px; font-weight: 700; letter-spacing: .05em; text-transform: uppercase; }
  .lens { width: 14px; height: 14px; border-radius: 50%; flex: none; background: radial-gradient(circle at 40% 35%, #4b4436, #1d1a15); box-shadow: 0 0 0 2px #0b0c0e, 0 0 0 3px #4a4d54; transition: background .15s, box-shadow .15s; }
  .on .lens { background: radial-gradient(circle at 40% 35%, #fff1d6 0%, #ffbf6b 35%, var(--accent) 70%); box-shadow: 0 0 0 2px #0b0c0e, 0 0 0 3px #6b4a1f, 0 0 10px 2px rgb(var(--accent-rgb) / .75); }
  .on .tx { color: var(--accent); }
</style>
