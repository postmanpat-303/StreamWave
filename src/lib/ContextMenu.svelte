<script lang="ts">
  import { onMount, tick } from "svelte";
  import Self from "./ContextMenu.svelte";
  import { aimingAt, isEntry, nextEnabled, placeMenu, placeSubmenu, type MenuItem, type Pt, type Rect } from "./contextMenu";
  let { items, x = 0, y = 0, onpick, onclose, sub = false, parentRect, itemRect, focusFirst = true, depth = 0, onenter, label = "Context menu" }:
    { items: MenuItem[]; x?: number; y?: number; onpick: (id: string) => void; onclose: (all: boolean) => void; sub?: boolean;
      parentRect?: Rect; itemRect?: Rect; focusFirst?: boolean; depth?: number; onenter?: () => void; label?: string } = $props();
  let el: HTMLDivElement;
  let pos = $state({ x: 0, y: 0, maxH: 0 });
  let placed = $state(false);
  let active = $state(-1);
  let openSub = $state<number | null>(null);
  let subArgs = $state<{ parent: Rect; item: Rect; focus: boolean } | null>(null);
  let subKey = $state(0);
  const btns: HTMLButtonElement[] = [];
  // hover intent: where the pointer last was over the item that owns the open submenu, and a pending switch
  let anchor: Pt | null = null, pointer: Pt = { x: 0, y: 0 };
  let pending: { i: number; timer: ReturnType<typeof setTimeout> } | null = null;
  const INTENT_MS = 400;

  onMount(() => {
    tick().then(() => {
      const w = el.offsetWidth, h = el.scrollHeight, vw = window.innerWidth, vh = window.innerHeight;
      const p = sub && parentRect && itemRect ? placeSubmenu(parentRect, itemRect, w, h, vw, vh) : placeMenu(x, y, w, h, vw, vh);
      pos = { x: p.x, y: p.y, maxH: p.maxH }; placed = true;
      if (focusFirst) focus(nextEnabled(items, -1, 1)); else el.focus({ preventScroll: true });
    });
    if (!sub) {
      const away = (e: Event) => { if (!(e.target as Element | null)?.closest?.(".ctxmenu")) onclose(true); };
      const blur = () => onclose(true);
      // close when the user scrolls the page under the menu (wheel outside it); programmatic or focus scrolls don't close it
      setTimeout(() => { window.addEventListener("pointerdown", away, true); window.addEventListener("wheel", away, { capture: true, passive: true }); window.addEventListener("blur", blur); window.addEventListener("resize", blur); });
      return () => { cancelPending(); window.removeEventListener("pointerdown", away, true); window.removeEventListener("wheel", away, true); window.removeEventListener("blur", blur); window.removeEventListener("resize", blur); };
    }
    return cancelPending;
  });
  function focus(i: number) { active = i; if (i >= 0) { btns[i]?.focus({ preventScroll: true }); btns[i]?.scrollIntoView({ block: "nearest" }); } }
  function cancelPending() { if (pending) { clearTimeout(pending.timer); pending = null; } }
  function subEl(): HTMLElement | null { return document.querySelector(`.ctxmenu[data-depth="${depth + 1}"]`); }
  function openSubmenu(i: number, viaKey: boolean) {
    cancelPending();
    const b = btns[i]; if (!b) return;
    if (openSub === i && !viaKey) return; // already open: keep it
    const r = b.getBoundingClientRect(), pr = el.getBoundingClientRect();
    subArgs = { parent: { left: pr.left, top: pr.top, right: pr.right, bottom: pr.bottom }, item: { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, focus: viaKey };
    subKey++; openSub = i; active = i;
    anchor = { ...pointer };
  }
  /** What hovering item i means right now: open its submenu, or close the open one. */
  function applyHover(i: number) {
    cancelPending();
    const m = items[i];
    if (isEntry(m) && m.submenu && !m.disabled) openSubmenu(i, false);
    else { openSub = null; subArgs = null; }
  }
  function hover(i: number) {
    const m = items[i]; if (!isEntry(m)) return;
    active = i; btns[i]?.focus({ preventScroll: true });
    if (openSub === i) { cancelPending(); anchor = { ...pointer }; return; }
    if (openSub !== null && anchor) {
      const s = subEl()?.getBoundingClientRect();
      if (s && aimingAt(anchor, pointer, s, s.left >= el.getBoundingClientRect().left ? "right" : "left")) {
        // heading for the submenu across other items: wait a moment instead of switching
        if (pending?.i !== i) { cancelPending(); pending = { i, timer: setTimeout(() => { pending = null; if (active === i) applyHover(i); }, INTENT_MS) }; }
        return;
      }
    }
    applyHover(i);
  }
  function move(e: PointerEvent) {
    const prev = pointer; pointer = { x: e.clientX, y: e.clientY };
    if (openSub !== null && active === openSub) anchor = prev; // still on the submenu's own item
    if (pending && anchor) { // re-check: stopped aiming at the submenu? switch now
      const s = subEl()?.getBoundingClientRect();
      if (!s || !aimingAt(anchor, pointer, s, s.left >= el.getBoundingClientRect().left ? "right" : "left")) { const i = pending.i; cancelPending(); applyHover(i); }
    }
  }
  function subEntered() { cancelPending(); if (openSub !== null) active = openSub; onenter?.(); }
  function activate(i: number, viaKey = false) {
    const m = items[i]; if (!isEntry(m) || m.disabled) return;
    if (m.submenu) { openSubmenu(i, viaKey); return; } // a click opens it (and leaves it open)
    onpick(m.id); onclose(true);
  }
  function key(e: KeyboardEvent) {
    e.stopPropagation();
    const closeSub = () => { cancelPending(); openSub = null; subArgs = null; };
    if (e.key === "ArrowDown") { e.preventDefault(); closeSub(); focus(nextEnabled(items, active, 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); closeSub(); focus(nextEnabled(items, active, -1)); }
    else if (e.key === "Home") { e.preventDefault(); closeSub(); focus(nextEnabled(items, -1, 1)); }
    else if (e.key === "End") { e.preventDefault(); closeSub(); focus(nextEnabled(items, 0, -1)); }
    else if (e.key === "ArrowRight") { e.preventDefault(); const m = items[active]; if (m && isEntry(m) && m.submenu && !m.disabled) openSubmenu(active, true); }
    else if (e.key === "Escape" || (e.key === "ArrowLeft" && sub)) { e.preventDefault(); onclose(!sub); }
    else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); activate(active, true); }
    else if (e.key === "Tab") { e.preventDefault(); closeSub(); focus(nextEnabled(items, active, e.shiftKey ? -1 : 1)); }
    else if (e.key.length === 1) { // type-ahead: first letter
      const ch = e.key.toLowerCase(), n = items.length;
      for (let k = 1; k <= n; k++) { const j = (active + k) % n, m = items[j]; if (isEntry(m) && !m.disabled && m.label.toLowerCase().startsWith(ch)) { closeSub(); focus(j); break; } }
    }
  }
</script>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="ctxmenu" class:sub bind:this={el} role="menu" tabindex="-1" aria-label={label} data-depth={depth}
  style="left:{placed ? pos.x : 0}px;top:{placed ? pos.y : 0}px;{placed ? `max-height:${pos.maxH}px;` : 'visibility:hidden;'}"
  onkeydown={key} onpointermove={move} onpointerenter={() => onenter?.()} oncontextmenu={(e) => e.preventDefault()}
  onscroll={() => { if (openSub !== null) { cancelPending(); openSub = null; subArgs = null; } }}>
  {#each items as m, i}
    {#if isEntry(m)}
      <button bind:this={btns[i]} class="it" class:act={active === i} class:dis={m.disabled} class:open={openSub === i} class:danger={m.danger}
        role={m.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
        aria-checked={m.checked} aria-disabled={m.disabled ? "true" : undefined} aria-haspopup={m.submenu ? "menu" : undefined} aria-expanded={m.submenu ? openSub === i : undefined}
        tabindex="-1"
        onpointerenter={(e) => { pointer = { x: e.clientX, y: e.clientY }; hover(i); }}
        onclick={() => activate(i)}>
        <span class="ck">{m.checked ? "✓" : ""}</span><span class="lb">{m.label}</span>
        {#if m.submenu}<span class="sc">▸</span>{:else if m.shortcut}<span class="sc">{m.shortcut}</span>{/if}
      </button>
    {:else}<div class="sep" role="separator"></div>{/if}
  {/each}
</div>
{#if openSub !== null && subArgs}
  {@const s = items[openSub]}
  {#if isEntry(s) && s.submenu}
    {#key subKey}
      <Self items={s.submenu} sub parentRect={subArgs.parent} itemRect={subArgs.item} focusFirst={subArgs.focus} depth={depth + 1} label={s.label} {onpick}
        onenter={subEntered}
        onclose={(all) => { const i = openSub; openSub = null; subArgs = null; if (all) onclose(true); else if (i !== null) focus(i); }} />
    {/key}
  {/if}
{/if}
<style>
  .ctxmenu { position: fixed; z-index: 100; min-width: 230px; max-width: 340px; padding: 5px; background: #1b2230f5; border: 1px solid #36425a; border-radius: 10px;
    box-shadow: 0 12px 32px #000a, 0 0 0 1px #0004; backdrop-filter: blur(6px); outline: none; font-size: 13px;
    overflow-y: auto; overscroll-behavior: contain; box-sizing: border-box; }
  .ctxmenu.sub { z-index: 101; }
  .it { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px 6px 4px; background: none; border: 0; border-radius: 6px; text-align: left; color: var(--text); cursor: default; }
  .it.act:not(.dis), .it.open:not(.dis), .it:focus-visible:not(.dis) { background: rgb(var(--accent-rgb) / 0.15); outline: none; }
  .it.act:not(.dis) .lb, .it.open .lb { color: #fff; }
  .it.dis { color: #ffffff44; } .it:focus { outline: none; }
  .it.danger:not(.dis) .lb { color: #ff8095; }
  .ck { width: 16px; text-align: center; color: var(--accent); flex: none; }
  .lb { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sc { color: var(--muted); font-size: 11px; margin-left: 16px; flex: none; }
  .it.dis .sc { color: #ffffff30; }
  .sep { height: 1px; margin: 4px 6px; background: #ffffff14; }
</style>
