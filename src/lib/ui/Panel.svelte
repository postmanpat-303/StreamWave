<script lang="ts">
  /** Brushed charcoal panel with corner screws and an optional uppercase title. */
  import type { Snippet } from "svelte";
  let { title = "", pad = 14, children, actions }: { title?: string; pad?: number; children: Snippet; actions?: Snippet } = $props();
</script>
<section class="hw pnl" style="padding:{pad}px {pad + 4}px">
  {#if title || actions}<div class="ph">{#if title}<h3>{title}</h3>{/if}{#if actions}<div class="pa">{@render actions()}</div>{/if}</div>{/if}
  {@render children()}
</section>
<style>
  .pnl { position: relative; }
  .ph { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-bottom: 8px; }
  .ph h3 { margin: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pa { display: flex; gap: 6px; flex: none; }
</style>
