<script lang="ts">
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { api, type ExportOptions, type FfmpegInfo } from "./api";
  let { trackName, onexport, onclose }: { trackName: string; onexport: (path: string, o: ExportOptions) => Promise<void>; onclose: () => void } = $props();

  type Fmt = { id: string; name: string; ext: string; lossy: boolean; ff: boolean; bitrate?: number[]; quality?: [string, number, number, number] };
  const FORMATS: Fmt[] = [
    { id: "wav", name: "WAV (uncompressed)", ext: "wav", lossy: false, ff: false },
    { id: "flac", name: "FLAC (lossless)", ext: "flac", lossy: false, ff: true, quality: ["Compression", 0, 12, 5] },
    { id: "wavpack", name: "WavPack (lossless)", ext: "wv", lossy: false, ff: true },
    { id: "mka", name: "Matroska audio (FLAC in MKA)", ext: "mka", lossy: false, ff: true },
    { id: "mp3", name: "MP3", ext: "mp3", lossy: true, ff: true, bitrate: [96, 128, 160, 192, 256, 320] },
    { id: "ogg", name: "Ogg Vorbis", ext: "ogg", lossy: true, ff: true, quality: ["Quality", 0, 10, 6] },
    { id: "opus", name: "Opus", ext: "opus", lossy: true, ff: true, bitrate: [32, 48, 64, 96, 128, 160, 256] },
    { id: "webm", name: "WebM (Opus)", ext: "webm", lossy: true, ff: true, bitrate: [64, 96, 128, 160, 256] },
    { id: "speex", name: "Speex (speech, mono)", ext: "spx", lossy: true, ff: true, quality: ["Quality", 0, 10, 8] },
  ];
  let ff = $state<FfmpegInfo | null>(null);
  let fmt = $state("wav");
  let rate = $state<number | null>(null), channels = $state<number | null>(null), bits = $state(24);
  let bitrate = $state(192), quality = $state(5);
  let tags = $state({ title: "", artist: "", album: "", date: String(new Date().getFullYear()), genre: "", comment: "Made with StreamWave" });
  let busy = $state(false);
  const f = $derived(FORMATS.find((x) => x.id === fmt)!);
  $effect(() => { if (f.quality) quality = f.quality[3]; if (f.bitrate) bitrate = f.bitrate.includes(192) ? 192 : 128; });
  onMount(async () => { ff = await api.ffmpegStatus(); tags.title = trackName.replace(/\.[^.]+$/, ""); });

  async function go() {
    const path = await save({ defaultPath: `${tags.title || "export"}.${f.ext}`, filters: [{ name: f.name, extensions: [f.ext] }] });
    if (!path) return;
    busy = true;
    try {
      await onexport(path, { format: fmt, sample_rate: rate, channels, bit_depth: fmt === "wav" ? bits : null,
        bitrate_kbps: f.bitrate ? bitrate : null, quality: f.quality ? quality : null, tags: { ...tags } });
    } finally { busy = false; }
  }
</script>

<div class="panel">
  <div class="hdr"><h3>Export · {trackName}</h3><button class="x" onclick={onclose}>✕</button></div>
  {#if ff && !ff.found}<div class="warn">{ff.message}</div>{/if}
  <label for="ef">Format</label>
  <select id="ef" bind:value={fmt}>
    {#each FORMATS as x}<option value={x.id} disabled={x.ff && ff !== null && !ff.found}>{x.name}</option>{/each}
  </select>
  <div class="row">
    <div><label for="er">Sample rate</label>
      <select id="er" bind:value={rate}><option value={null}>Same as source</option>{#each [8000, 16000, 22050, 32000, 44100, 48000, 96000] as r}<option value={r}>{r / 1000} kHz</option>{/each}</select></div>
    <div><label for="ec">Channels</label>
      <select id="ec" bind:value={channels}><option value={null}>Same as source</option><option value={1}>Mono</option><option value={2}>Stereo</option></select></div>
  </div>
  {#if fmt === "wav"}
    <label for="eb">Bit depth</label>
    <select id="eb" bind:value={bits}><option value={16}>16-bit (CD)</option><option value={24}>24-bit</option><option value={32}>32-bit float</option></select>
  {/if}
  {#if f.bitrate}
    <label for="ebr">Bitrate</label>
    <select id="ebr" bind:value={bitrate}>{#each f.bitrate as b}<option value={b}>{b} kbps</option>{/each}</select>
  {/if}
  {#if f.quality}
    <label for="eq">{f.quality[0]}: {quality}</label>
    <input id="eq" type="range" min={f.quality[1]} max={f.quality[2]} step="1" bind:value={quality} />
  {/if}
  {#if fmt === "opus" || fmt === "webm"}<div class="muted">Opus always uses 48 kHz (or 8/12/16/24 kHz).</div>{/if}
  <details><summary>Tags (title, artist...)</summary>
    {#each [["title", "Title"], ["artist", "Artist"], ["album", "Album"], ["date", "Year"], ["genre", "Genre"], ["comment", "Comment"]] as [k, n]}
      <label for={"t" + k}>{n}</label><input id={"t" + k} bind:value={(tags as any)[k]} />
    {/each}
    {#if fmt === "wav"}<div class="muted">WAV files don't carry these tags. Use FLAC for tagged lossless files.</div>{/if}
  </details>
  <button class="go" onclick={go} disabled={busy}>{busy ? "Exporting..." : "Export"}</button>
  <div class="muted">Includes the track's edits and effects. Your original files aren't changed.</div>
</div>
<style>
  .panel { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 14px; }
  .hdr { display: flex; justify-content: space-between; align-items: center; } .hdr h3 { margin: 0; } .x { background: none; border: none; }
  .row { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .go { width: 100%; margin-top: 12px; background: var(--accent); color: #002; border-color: var(--accent); font-weight: 600; }
  .warn { background: #ffb02018; border: 1px solid #ffb02066; border-radius: 8px; padding: 8px; font-size: 12px; margin: 8px 0; }
  .muted { color: var(--muted); font-size: 11px; margin-top: 6px; }
  details { margin-top: 10px; font-size: 13px; } summary { cursor: pointer; color: var(--muted); }
  input[type="range"] { padding: 0; accent-color: var(--accent); }
</style>
