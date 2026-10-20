<script lang="ts">
  /** Dark dropdown with a small caption above the value (native <select> underneath, so keyboard and screen readers work as usual). */
  import type { Snippet } from "svelte";
  let { caption, value = $bindable(), id = undefined, title = undefined, disabled = false, onchange, children }:
    { caption: string; value?: any; id?: string; title?: string; disabled?: boolean; onchange?: (e: Event & { currentTarget: HTMLSelectElement }) => void; children: Snippet } = $props();
</script>
<label class="dd" class:disabled {title}>
  <span class="cap">{caption}</span>
  <select {id} {disabled} bind:value {onchange}>{@render children()}</select>
</label>
<style>
  .dd { position: relative; display: block; margin: 0; min-width: 0; }
  .cap { position: absolute; left: 11px; top: 5px; font-size: 9.5px; line-height: 1; color: var(--muted); pointer-events: none; letter-spacing: .02em; }
  .dd select { padding-top: 17px; padding-bottom: 5px; padding-left: 10px; font-size: 13px; }
  .disabled { opacity: .5; }
</style>
