<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import logoUrl from "../assets/logo-256.png";
  import { AUTHOR, COFFEE_URL, PAYPAL_URL, REPO_URL, type AllowedLink } from "./about";
  let { onclose }: { onclose: () => void } = $props();
  let version = $state("");
  let err = $state<string | null>(null);
  onMount(() => {
    getVersion().then((v) => (version = v)).catch(() => {});
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onclose(); } };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  });
  async function go(url: AllowedLink) {
    err = null;
    try { await openUrl(url); } catch (e) { err = `Couldn't open the browser: ${e}`; }
  }
</script>
<div class="veil" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="about-title">
    <img class="logo" src={logoUrl} alt="StreamWave logo: an engraved humpback whale singing as it breaches a wave" width="128" height="128" />
    <h2 id="about-title">StreamWave</h2>
    <div class="ver">Version {version || "…"}</div>
    <div class="by">Created by <b>{AUTHOR}</b></div>
    <button class="coffee" onclick={() => go(COFFEE_URL)}>☕ Buy me a coffee</button>
    <button class="paypal" onclick={() => go(PAYPAL_URL)}>Donate via PayPal</button>
    <button class="link" onclick={() => go(REPO_URL)}>GitHub: postmanpat-303/StreamWave ↗</button>
    {#if err}<div class="err">{err}</div>{/if}
    <div class="legal">
      <p>StreamWave is free software under the <b>MIT licence</b>. © 2026 {AUTHOR}.</p>
      <p>The Windows version includes <b>FFmpeg</b> (ffmpeg.exe, ffprobe.exe), licensed under the <b>GNU LGPL</b> and shipped as separate, unmodified programs. Its licence text is in the <code>licenses</code> folder next to StreamWave. FFmpeg is a trademark of Fabrice Bellard.</p>
    </div>
    <button class="ghost" onclick={onclose}>Close</button>
  </div>
</div>
<style>
  .veil { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: grid; place-items: center; z-index: 120; }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: 16px; padding: 26px; width: min(440px, 92vw); display: flex; flex-direction: column; align-items: center; gap: 8px; text-align: center; }
  .logo { border-radius: 28px; box-shadow: 0 12px 40px rgba(255,122,26,.35); margin-bottom: 6px; }
  h2 { margin: 0; font-size: 26px; }
  .ver { color: var(--muted); font-size: 13px; }
  .by { margin: 4px 0 10px; }
  .coffee { width: 100%; padding: 12px 18px; font-size: 16px; font-weight: 700; border: 0; border-radius: 999px; background: linear-gradient(90deg, #ffb347, #ff7a1a); color: #1a0f05; cursor: pointer; box-shadow: 0 6px 24px rgba(255,122,26,.4); }
  .coffee:hover { filter: brightness(1.08); }
  .paypal { padding: 6px 14px; font-size: 12.5px; border: 1px solid var(--line); border-radius: 999px; background: transparent; color: var(--muted); cursor: pointer; }
  .paypal:hover { color: var(--accent); border-color: var(--accent); }
  .link { background: transparent; border: 0; color: var(--accent); cursor: pointer; }
  .legal { font-size: 11.5px; color: var(--muted); line-height: 1.5; margin-top: 8px; text-align: left; }
  .legal p { margin: 6px 0; }
  .err { color: var(--rec); font-size: 12px; }
  .ghost { background: transparent; margin-top: 6px; }
</style>
