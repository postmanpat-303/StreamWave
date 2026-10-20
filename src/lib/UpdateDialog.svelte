<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RELEASES_URL, REPO_URL } from "./about";
  import { LATEST_RELEASE_API, friendlyError, manualUpdateReason, parseLatestRelease, renderMarkdown, shouldOffer } from "./updates";
  import { api } from "./api";

  let { quiet = false, skipped = null, busy = null, dirty = false, onskip, onchecked, onclose }: {
    /** startup check: render nothing unless an update should be offered */
    quiet?: boolean;
    skipped?: string | null;
    /** reason installing is not possible right now (e.g. recording) */
    busy?: string | null;
    dirty?: boolean;
    onskip: (version: string) => void;
    onchecked: () => void;
    onclose: () => void;
  } = $props();

  type Phase = "checking" | "latest" | "available" | "downloading" | "installing" | "ready" | "error";
  let phase = $state<Phase>("checking");
  let current = $state("");
  /** the plugin's update (Windows/Linux); null on macOS */
  let update = $state<Update | null>(null);
  /** what we show: from the plugin, or on macOS from GitHub's latest release */
  let info = $state<{ version: string; body?: string | null; currentVersion: string } | null>(null);
  let message = $state("");
  let got = $state(0);
  let total = $state<number | null>(null);
  const isWindows = navigator.userAgent.includes("Windows");
  // No in-app updater on macOS yet (latest.json has no Mac entry), and .deb/.rpm/Arch installs on Linux are updated by hand
  // (latest.json only lists the AppImage): there we check GitHub's latest release and link to the download page.
  let manual = $state<"mac" | "package" | null>(null);

  async function macLatest(): Promise<{ version: string; body: string }> {
    const ctl = new AbortController();
    const t = setTimeout(() => ctl.abort(), 20000);
    try {
      const r = await fetch(LATEST_RELEASE_API, { headers: { Accept: "application/vnd.github+json" }, signal: ctl.signal });
      if (!r.ok) throw new Error(`latest release: HTTP ${r.status}${r.status === 404 ? " not found" : ""}`);
      const rel = parseLatestRelease(await r.json());
      if (!rel) throw new Error("latest release: not found");
      return rel;
    } finally { clearTimeout(t); }
  }
  const pct = $derived(total ? Math.min(100, Math.round((got / total) * 100)) : null);
  const mb = (n: number) => (n / 1048576).toFixed(1);

  onMount(() => {
    (async () => {
      try { current = await getVersion(); } catch {}
      let kind: string | null = null;
      try { kind = await api.installKind(); } catch {}
      manual = manualUpdateReason(navigator.userAgent, kind);
      try {
        if (manual) {
          const rel = await macLatest();
          onchecked();
          if (current && shouldOffer(rel.version, current, skipped, !quiet)) { info = { version: rel.version, body: rel.body, currentVersion: current }; phase = "available"; }
          else if (quiet) onclose();
          else phase = "latest";
          return;
        }
        const u = await check({ timeout: 20000 });
        onchecked();
        if (u && shouldOffer(u.version, current || u.currentVersion, skipped, !quiet)) { update = u; info = { version: u.version, body: u.body, currentVersion: u.currentVersion }; phase = "available"; }
        else if (quiet) onclose();
        else phase = "latest";
      } catch (e) {
        console.warn("update check failed", e);
        if (quiet) { onclose(); return; } // stay silent on startup
        message = friendlyError(e); phase = "error";
      }
    })();
    const k = (e: KeyboardEvent) => { if (e.key === "Escape" && phase !== "downloading" && phase !== "installing") { e.stopPropagation(); onclose(); } };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  });

  async function install() {
    if (!update || busy) return;
    phase = "downloading"; got = 0; total = null;
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? null;
        else if (ev.event === "Progress") got += ev.data.chunkLength;
        else if (ev.event === "Finished") phase = "installing"; // on Windows the installer now takes over and StreamWave closes
      });
      phase = "ready";
    } catch (e) {
      console.warn("update install failed", e);
      message = friendlyError(e); phase = "error";
    }
  }
  function skip() { if (info) onskip(info.version); onclose(); }
  function openDownloads() { openUrl(RELEASES_URL).catch(() => openUrl(REPO_URL).catch(() => {})); onclose(); }
  async function restart() { try { await relaunch(); } catch (e) { message = `Please restart StreamWave yourself to finish updating. (${e})`; phase = "error"; } }
</script>

{#if !(quiet && (phase === "checking" || phase === "latest"))}
<div class="veil" role="presentation" onclick={(e) => { if (e.target === e.currentTarget && (phase === "latest" || phase === "error" || phase === "checking")) onclose(); }}>
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="upd-title">
    {#if phase === "checking"}
      <h2 id="upd-title">Checking for updates…</h2>
      <div class="spin" aria-hidden="true"></div>
      <div class="actions"><button class="ghost" onclick={onclose}>Cancel</button></div>
    {:else if phase === "latest"}
      <h2 id="upd-title">🎉 You're on the latest version</h2>
      <p>StreamWave {current} is up to date.</p>
      <div class="actions"><button class="primary" onclick={onclose}>OK</button></div>
    {:else if phase === "error"}
      <h2 id="upd-title">Update check didn't work this time</h2>
      <p>{message}</p>
      <p class="muted">StreamWave keeps working as normal. You can also get new versions from the GitHub releases page.</p>
      <div class="actions">
        <button class="ghost" onclick={() => openUrl(REPO_URL).catch(() => {})}>Open GitHub</button>
        <button class="primary" onclick={onclose}>OK</button>
      </div>
    {:else if info}
      <h2 id="upd-title">A new version of StreamWave is available</h2>
      <div class="vers"><span>Installed: <b>{current || info.currentVersion}</b></span><span>→</span><span>New: <b class="new">{info.version}</b></span></div>
      {#if info.body}
        <div class="notes">{@html renderMarkdown(info.body)}</div>
      {/if}
      {#if phase === "available" && manual}
        {#if manual === "mac"}
          <p class="muted">On Mac, updates are installed by hand for now: download the new <b>.dmg</b>, open it and drag StreamWave into Applications to replace the old one. Your settings and projects are kept.</p>
        {:else}
          <p class="muted">This copy was installed from a Linux package (.deb, .rpm or Arch), so update it the same way: download the new package for your distro and install it over this one. Your settings and projects are kept.</p>
        {/if}
        <div class="actions">
          <button class="ghost" onclick={skip}>Skip this version</button>
          <button class="ghost" onclick={onclose}>Not now</button>
          <button class="primary" onclick={openDownloads}>Open download page</button>
        </div>
      {:else if phase === "available"}
        {#if busy}<p class="warn">{busy}</p>
        {:else if dirty}<p class="warn">You have unsaved changes. Save your project first, because StreamWave restarts to install the update.</p>{/if}
        <div class="actions">
          <button class="ghost" onclick={skip}>Skip this version</button>
          <button class="ghost" onclick={onclose}>Not now</button>
          <button class="primary" onclick={install} disabled={!!busy}>Download &amp; install</button>
        </div>
      {:else if phase === "downloading"}
        <div class="bar"><div style:width={pct === null ? "35%" : `${pct}%`} class:indet={pct === null}></div></div>
        <p class="muted">Downloading… {pct === null ? `${mb(got)} MB` : `${pct}% (${mb(got)} of ${mb(total ?? 0)} MB)`}</p>
      {:else if phase === "installing"}
        <div class="bar"><div style:width="100%"></div></div>
        <p class="muted">{isWindows ? "Installing… StreamWave will close and the installer will reopen it when it's done." : "Installing…"}</p>
      {:else if phase === "ready"}
        <p>✅ StreamWave {info.version} is installed. Restart to start using it.</p>
        <div class="actions">
          <button class="ghost" onclick={onclose}>Later</button>
          <button class="primary" onclick={restart}>Restart now</button>
        </div>
      {/if}
    {/if}
  </div>
</div>
{/if}

<style>
  .veil { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: grid; place-items: center; z-index: 130; }
  .card { background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 24px; width: min(560px, 92vw); max-height: 86vh; display: flex; flex-direction: column; gap: 10px; }
  h2 { margin: 0; font-size: 20px; }
  p { margin: 0; line-height: 1.5; }
  .muted { color: var(--muted); font-size: 13px; }
  .warn { color: var(--warn, #ffb347); font-size: 13px; }
  .vers { display: flex; gap: 12px; align-items: center; font-size: 14px; }
  .new { color: var(--accent); }
  .notes { overflow: auto; max-height: 46vh; border: 1px solid var(--line); border-radius: 10px; padding: 10px 14px; font-size: 13px; line-height: 1.5; background: rgba(0,0,0,.15); }
  .notes :global(h3) { font-size: 15px; margin: 10px 0 4px; } .notes :global(h4) { font-size: 13.5px; margin: 8px 0 4px; }
  .notes :global(p) { margin: 4px 0; } .notes :global(ul), .notes :global(ol) { margin: 4px 0; padding-left: 20px; }
  .notes :global(code) { font-size: 12px; background: rgba(255,255,255,.08); padding: 0 4px; border-radius: 4px; }
  .notes :global(.mdlink) { color: var(--accent); }
  .notes :global(.quote) { border-left: 3px solid var(--accent); padding-left: 8px; }
  .notes :global(table) { border-collapse: collapse; margin: 6px 0; } .notes :global(td) { border: 1px solid var(--line); padding: 2px 8px; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 6px; flex-wrap: wrap; }
  .primary { background: linear-gradient(90deg, #ffb347, #ff7a1a); color: #1a0f05; font-weight: 700; border: 0; }
  .primary:disabled { opacity: .5; cursor: not-allowed; }
  .ghost { background: transparent; }
  .bar { height: 8px; border-radius: 99px; background: rgba(255,255,255,.08); overflow: hidden; }
  .bar div { height: 100%; background: linear-gradient(90deg, #ffb347, #ff7a1a); transition: width .2s; }
  .bar .indet { animation: slide 1.2s infinite ease-in-out; }
  @keyframes slide { from { margin-left: -35%; } to { margin-left: 100%; } }
  .spin { width: 28px; height: 28px; border: 3px solid rgba(255,255,255,.15); border-top-color: var(--accent); border-radius: 50%; animation: r 0.9s linear infinite; align-self: center; margin: 8px; }
  @keyframes r { to { transform: rotate(360deg); } }
</style>
