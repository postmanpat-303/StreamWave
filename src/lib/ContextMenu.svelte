<script lang="ts">
  import { onMount, tick } from "svelte";
  import Self from "./ContextMenu.svelte";
  import { isEntry, nextEnabled, placeMenu, type MenuItem } from "./contextMenu";
  let { items, x, y, onpick, onclose, sub = false, altX = 0, label = "Context menu" }:
    { items: MenuItem[]; x: number; y: number; altX?: number; onpick: (id: string) => void; onclose: (all: boolean) => void; sub?: boolean; label?: string } = $props();
  let el: HTMLDivElement;
  let pos = $state({ x: 0, y: 0 });
  let active = $state(-1);
  let openSub = $state<number | null>(null);
  let subPos = $state({ x: 0, y: 0, alt: 0 });
  const btns: HTMLButtonElement[] = [];

  onMount(() => {
    pos = { x, y };
    tick().then(() => {
    const r = el.getBoundingClientRect();
    const p = placeMenu(x, y, r.width, r.height, window.innerWidth, window.innerHeight);
    // a submenu that doesn't fit on the right opens on the left of its parent
    pos = sub && x + r.width > window.innerWidth - 4 ? { x: Math.max(4, altX - r.width + 2), y: p.y } : p;
    focus(nextEnabled(items, -1, 1));
    });
    if (!sub) {
      const away = (e: Event) => { if (!(e.target as Element | null)?.closest?.(".ctxmenu")) onclose(true); };
      const blur = () => onclose(true);
      setTimeout(() => { window.addEventListener("pointerdown", away, true); window.addEventListener("blur", blur); window.addEventListener("resize", blur); document.querySelector(".tracks")?.addEventListener("scroll", blur, true); });
      return () => { window.removeEventListener("pointerdown", away, true); window.removeEventListener("blur", blur); window.removeEventListener("resize", blur); document.querySelector(".tracks")?.removeEventListener("scroll", blur, true); };
    }
  });
  function focus(i: number) { active = i; if (i >= 0) btns[i]?.focus(); }
  function openSubmenu(i: number, viaKey = false) {
    const b = btns[i]; if (!b) return; const r = b.getBoundingClientRect();
    subPos = { x: r.right - 2, y: r.top - 5, alt: r.left }; openSub = i;
    if (!viaKey) active = i;
  }
  function activate(i: number) {
    const m = items[i]; if (!isEntry(m) || m.disabled) return;
    if (m.submenu) { openSubmenu(i, true); return; }
    onpick(m.id); onclose(true);
  }
  function key(e: KeyboardEvent) {
    e.stopPropagation();
    if (openSub !== null) return; // the submenu handles keys
    if (e.key === "ArrowDown") { e.preventDefault(); focus(nextEnabled(items, active, 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); focus(nextEnabled(items, active, -1)); }
    else if (e.key === "Home") { e.preventDefault(); focus(nextEnabled(items, -1, 1)); }
    else if (e.key === "End") { e.preventDefault(); focus(nextEnabled(items, 0, -1)); }
    else if (e.key === "ArrowRight") { e.preventDefault(); const m = items[active]; if (m && isEntry(m) && m.submenu) openSubmenu(active, true); }
    else if (e.key === "Escape" || (e.key === "ArrowLeft" && sub)) { e.preventDefault(); onclose(!sub); }
    else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); activate(active); }
    else if (e.key === "Tab") { e.preventDefault(); focus(nextEnabled(items, active, e.shiftKey ? -1 : 1)); }
    else if (e.key.length === 1) { // type-ahead: first letter
      const ch = e.key.toLowerCase(), n = items.length;
      for (let k = 1; k <= n; k++) { const j = (active + k) % n, m = items[j]; if (isEntry(m) && !m.disabled && m.label.toLowerCase().startsWith(ch)) { focus(j); break; } }
    }
  }
</script>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="ctxmenu" class:sub bind:this={el} role="menu" tabindex="-1" aria-label={label} style="left:{pos.x}px;top:{pos.y}px" onkeydown={key} oncontextmenu={(e) => e.preventDefault()}>
  {#each items as m, i}
    {#if isEntry(m)}
      <button bind:this={btns[i]} class="it" class:act={active === i} class:danger={m.danger} role={m.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
        aria-checked={m.checked} aria-disabled={m.disabled} aria-haspopup={m.submenu ? "menu" : undefined} aria-expanded={m.submenu ? openSub === i : undefined}
        disabled={m.disabled} tabindex="-1"
        onpointerenter={() => { if (m.disabled) return; active = i; btns[i]?.focus(); if (m.submenu) openSubmenu(i); else openSub = null; }}
        onclick={() => activate(i)}>
        <span class="ck">{m.checked ? "✓" : ""}</span><span class="lb">{m.label}</span>
        {#if m.submenu}<span class="sc">▸</span>{:else if m.shortcut}<span class="sc">{m.shortcut}</span>{/if}
      </button>
    {:else}<div class="sep" role="separator"></div>{/if}
  {/each}
</div>
{#if openSub !== null}
  {@const s = items[openSub]}
  {#if isEntry(s) && s.submenu}
    <Self items={s.submenu} x={subPos.x} y={subPos.y} altX={subPos.alt} sub label={s.label} {onpick}
      onclose={(all) => { const i = openSub; openSub = null; if (all) onclose(true); else if (i !== null) focus(i); }} />
  {/if}
{/if}
<style>
  .ctxmenu { position: fixed; z-index: 100; min-width: 230px; max-width: 340px; padding: 5px; background: #1b2230f5; border: 1px solid #36425a; border-radius: 10px;
    box-shadow: 0 12px 32px #000a, 0 0 0 1px #0004; backdrop-filter: blur(6px); outline: none; font-size: 13px; }
  .it { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px 6px 4px; background: none; border: 0; border-radius: 6px; text-align: left; color: var(--text); cursor: default; }
  .it.act:not(:disabled), .it:focus-visible { background: #19d3b026; outline: none; }
  .it.act:not(:disabled) .lb { color: #fff; }
  .it:disabled { color: #ffffff44; }
  .it.danger:not(:disabled) .lb { color: #ff8095; }
  .ck { width: 16px; text-align: center; color: var(--accent); flex: none; }
  .lb { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sc { color: var(--muted); font-size: 11px; margin-left: 16px; flex: none; }
  .it:disabled .sc { color: #ffffff30; }
  .sep { height: 1px; margin: 4px 6px; background: #ffffff14; }
</style>
