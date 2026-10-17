<script lang="ts">
  import { onMount } from "svelte";
  import { api, type DeviceInfo, type FfmpegInfo, type Level, type Settings } from "./api";
  import Meter from "./Meter.svelte";
  let { settings, devices, level, onsetinput, ondone }: { settings: Settings; devices: DeviceInfo[]; level: Level; onsetinput: (id: string) => void | Promise<void>; ondone: () => void | Promise<void> } = $props();
  let step = $state(0);
  let ffmpeg = $state<FfmpegInfo | null>(null);
  let obsMsg = $state<string | null>(null), obsBusy = $state(false);
  const inputs = $derived(devices.filter((d) => d.is_input));
  onMount(async () => { try { ffmpeg = await api.ffmpegStatus(); } catch (e) { ffmpeg = { found: false, path: null, version: null, message: String(e) }; } });
  async function tryObs() {
    obsBusy = true; obsMsg = null;
    try { const s = await api.obsConnect(settings.obs_host || "localhost", settings.obs_port || 4455, settings.obs_password || ""); obsMsg = s.connected ? `Connected${s.obs_version ? ` to OBS ${s.obs_version}` : ""}` : "Not connected"; }
    catch (e) { obsMsg = String(e); }
    obsBusy = false;
  }
</script>
<div class="veil" role="dialog" aria-modal="true" aria-label="Welcome to StreamWave">
  <div class="card">
    <h2>Welcome to StreamWave</h2>
    <div class="steps">{#each ["Microphone", "OBS (optional)", "FFmpeg"] as s, i}<span class:on={i === step}>{i + 1}. {s}</span>{/each}</div>
    {#if step === 0}
      <p>Pick your mic and say something, the meter should move. The mic is always opened in shared mode, so OBS and Discord keep working.</p>
      <select value={settings.input_device ?? ""} onchange={(e) => onsetinput((e.target as HTMLSelectElement).value)}>
        <option value="" disabled>Choose an input...</option>
        {#each inputs as d}<option value={d.id}>{d.friendly_name}{d.is_default ? " (default)" : ""}</option>{/each}
      </select>
      <Meter left={level.left} right={level.right} />
    {:else if step === 1}
      <p>Connect to OBS so recordings, markers and scenes stay in sync. In OBS: Tools &gt; WebSocket Server Settings &gt; Enable, then copy the password.</p>
      <label for="ob-host">Host</label><input id="ob-host" bind:value={settings.obs_host} placeholder="localhost" />
      <label for="ob-port">Port</label><input id="ob-port" type="number" bind:value={settings.obs_port} />
      <label for="ob-pw">Password</label><input id="ob-pw" type="password" bind:value={settings.obs_password} />
      <button onclick={tryObs} disabled={obsBusy}>{obsBusy ? "Connecting..." : "Test connection"}</button>
      {#if obsMsg}<p class="note">{obsMsg}</p>{/if}
    {:else}
      {#if !ffmpeg}<p>Looking for FFmpeg...</p>
      {:else if ffmpeg.found}<p>✅ FFmpeg found{ffmpeg.version ? ` (${ffmpeg.version})` : ""}. MP3/AAC/Opus export and video import are available.</p>
      {:else}<p>⚠️ FFmpeg was not found. WAV recording and editing work without it. For MP3/AAC/Opus export and OBS video files, put <code>ffmpeg.exe</code> and <code>ffprobe.exe</code> next to StreamWave or on your PATH (e.g. <code>winget install Gyan.FFmpeg</code>), then restart.</p>{/if}
    {/if}
    <div class="row">
      <button class="ghost" onclick={() => ondone()}>Skip setup</button>
      <span class="grow"></span>
      {#if step > 0}<button class="ghost" onclick={() => step--}>Back</button>{/if}
      {#if step < 2}<button onclick={() => step++}>Next</button>{:else}<button onclick={() => ondone()}>Start</button>{/if}
    </div>
  </div>
</div>
<style>
  .veil { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: grid; place-items: center; z-index: 100; }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: 14px; padding: 22px; width: min(520px, 92vw); display: flex; flex-direction: column; gap: 10px; }
  .steps { display: flex; gap: 12px; font-size: 12px; color: var(--muted); } .steps .on { color: var(--accent); font-weight: 600; }
  .row { display: flex; gap: 8px; margin-top: 8px; } .grow { flex: 1; }
  .ghost { background: transparent; } .note { font-size: 12px; color: var(--muted); }
</style>
