<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, fmtTime, type ObsStatus, type Settings } from "./api";
  let { settings, onRecordingReady, onstatus, onstate }: { settings: Settings; onRecordingReady: (path: string) => void; onstatus?: (s: ObsStatus) => void; onstate?: (state: string) => void } = $props();
  let host = $state("localhost"), port = $state(4455), password = $state("");
  let status = $state<ObsStatus>({ connected: false, recording: false, paused: false, duration_ms: 0, obs_version: null, message: null });
  let busy = $state(false), error = $state<string | null>(null), lastState = $state("");
  /** Live connection state from the backend's keep-alive loop. */
  let conn = $state<{ state: string; attempt: number; retry_in_secs: number; message: string | null }>({ state: "disconnected", attempt: 0, retry_in_secs: 0, message: null });
  $effect(() => { host = settings.obs_host || "localhost"; port = settings.obs_port || 4455; password = settings.obs_password || ""; });

  async function refresh() { try { status = await api.obsStatus(); onstatus?.(status); } catch {} }
  async function connect() {
    busy = true; error = null;
    try { status = await api.obsConnect(host, Number(port), password); onstatus?.(status); } catch (e) { error = String(e); }
    busy = false;
  }
  async function disconnect() { await api.obsDisconnect(); await refresh(); }
  async function toggle() {
    error = null;
    try { if (status.recording) await api.obsStop(); else await api.obsStart(); } catch (e) { error = String(e); }
    setTimeout(refresh, 300);
  }
  onMount(() => {
    const t = setInterval(() => status.connected && refresh(), 1000);
    const uns = [
      listen<{ active: boolean; state: string; path: string | null }>("obs-record-state", (e) => { lastState = e.payload.state; onstate?.(e.payload.state); refresh(); }),
      listen<string>("obs-recording-ready", (e) => { if (settings.obs_auto_import) onRecordingReady(e.payload); }),
      listen<string>("obs-disconnected", (e) => { error = `OBS disconnected: ${e.payload}`; refresh(); }),
      listen<typeof conn>("obs-conn", (e) => { conn = e.payload; if (e.payload.state === "connected") { error = null; refresh(); } else if (e.payload.state !== "reconnecting") refresh(); }),
    ];
    return () => { clearInterval(t); uns.forEach((u) => u.then((f) => f())); };
  });
</script>
<div class="panel">
  <h3>OBS Studio <span class="dot" class:on={status.connected} class:retry={conn.state === "reconnecting"}></span></h3>
  {#if conn.state === "reconnecting"}
    <div class="warn">OBS connection lost{conn.message ? ` (${conn.message})` : ""}. Reconnecting, attempt {conn.attempt}, next try in {conn.retry_in_secs}s...</div>
    <button class="wide ghost" onclick={disconnect}>Stop trying</button>
  {:else if !status.connected}
    <label for="obs-host">Host</label><input id="obs-host" bind:value={host} />
    <label for="obs-port">Port</label><input id="obs-port" type="number" bind:value={port} />
    <label for="obs-pw">Password</label><input id="obs-pw" type="password" bind:value={password} placeholder="from OBS > Tools > WebSocket Server Settings" />
    <button class="wide" onclick={connect} disabled={busy}>{busy ? "Connecting..." : "Connect to OBS"}</button>
  {:else}
    <div class="info">Connected {status.obs_version ? `· OBS ${status.obs_version}` : ""}</div>
    <button class="wide rec" class:active={status.recording} onclick={toggle}>
      {status.recording ? `■ Stop OBS recording  ${fmtTime(status.duration_ms / 1000)}` : "● Start OBS recording"}
    </button>
    <label class="chk"><input type="checkbox" bind:checked={settings.follow_obs} onchange={() => api.saveSettings(settings)} /> Follow OBS: record / pause / stop together</label>
    <label class="chk"><input type="checkbox" bind:checked={settings.obs_auto_import} onchange={() => api.saveSettings(settings)} /> Auto-import when OBS stops (each audio track separately)</label>
    {#if lastState}<div class="info">Last event: {lastState}</div>{/if}
    <button class="wide ghost" onclick={disconnect}>Disconnect</button>
  {/if}
  {#if error}<div class="err">{error}</div>{/if}
</div>
<style>
  .panel { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 14px; }
  .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: #555; margin-left: 6px; }
  .dot.on { background: var(--accent); box-shadow: 0 0 8px var(--accent); }
  .wide { width: 100%; margin-top: 10px; }
  .rec.active { background: var(--rec); border-color: var(--rec); }
  .ghost { background: transparent; }
  .info { font-size: 12px; color: var(--muted); margin-top: 6px; }
  .dot.retry { background: #e0a030; }
  .warn { font-size: 12px; color: #e0a030; margin-top: 6px; }
  .err { font-size: 12px; color: var(--rec); margin-top: 8px; }
  .chk { display: flex; gap: 8px; align-items: center; } .chk input { width: auto; }
</style>
