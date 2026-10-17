<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open, save, ask } from "@tauri-apps/plugin-dialog";
  import { readText } from "@tauri-apps/plugin-clipboard-manager";
  import * as E from "./lib/editModel";
  import Meter from "./lib/Meter.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import logoUrl from "./assets/logo.png";
  import WaveCanvas from "./lib/WaveCanvas.svelte";
  import ObsPanel from "./lib/ObsPanel.svelte";
  import FxPanel from "./lib/FxPanel.svelte";
  import ExportPanel from "./lib/ExportPanel.svelte";
  import StudioPanel from "./lib/StudioPanel.svelte";
  import { serialize, deserialize } from "./lib/project";
  import { activeChain, slot, type FxSlot } from "./lib/fx";
  import { api, fmtTime, type DeviceInfo, type EngineStatus, type Level, type Settings, type LatencyProfile, type MicReport, type ExportOptions, type Analysis, type ObsStatus } from "./lib/api";

  let settings = $state<Settings>({ input_device: null, output_device: null, latency: "Balanced", auto_backoff: true,
    record_dir: null, obs_host: "localhost", obs_port: 4455, obs_password: "", obs_auto_import: true, preroll_secs: 30, preroll_prepend: false, recent_projects: [], follow_obs: false });
  let devices = $state<DeviceInfo[]>([]);
  let level = $state<Level>({ left: 0, right: 0, recording: false, paused: false, rec_secs: 0, xruns: 0, playing: false, play_secs: 0, play_total: 0 });
  let engine = $state<EngineStatus | null>(null);
  // ---- non-destructive edit model ----
  const hist = new E.History({ tracks: [] });
  let project = $state.raw<E.Project>(hist.present);
  let sources = $state.raw(new Map<string, E.Source>());
  let selection = $state<E.Selection | null>(null);
  let clipboard: E.Clip[] = [];
  let playingId = $state<number | null>(null);
  let canUndo = $state(false), canRedo = $state(false);
  let toasts = $state<{ id: number; text: string; kind: string }[]>([]);
  let showSettings = $state(false);
  let nextId = 1;

  const inputs = $derived(devices.filter((d) => d.is_input));
  const outputs = $derived(devices.filter((d) => !d.is_input));
  const recording = $derived(level.recording);

  function toast(text: string, kind = "info") {
    const id = nextId++; toasts.push({ id, text, kind });
    setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 5000);
  }
  function sync() { project = hist.present; canUndo = hist.canUndo; canRedo = hist.canRedo; markDirty();
    if (selection && !project.tracks.some((t) => t.id === selection!.trackId)) selection = null; }
  function commit(label: string, next: E.Project) { hist.apply(label, next); sync(); }
  const activeTrack = () => project.tracks.find((t) => t.id === selection?.trackId) ?? project.tracks[0];
  const hasRange = () => !!selection && selection.b - selection.a > 0.0005;

  async function addTrack(path: string, kind: string, name?: string, video?: string) {
    try {
      const w = await api.importAudio(path);
      const m = new Map(sources); m.set(w.path, { path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate, channels: w.channels, peaks: w.peaks });
      sources = m;
      const id = nextId++;
      commit("Add track", { ...hist.present, tracks: [...hist.present.tracks, { id, name: name ?? w.name, kind, video, clips: [{ src: w.path, start: 0, end: w.duration_secs }] }] });
      selection = { trackId: id, a: 0, b: 0 };
    } catch (e) { toast(String(e), "err"); }
  }
  function removeTrack(id: number) { commit("Remove track", { ...hist.present, tracks: hist.present.tracks.filter((t) => t.id !== id) }); }

  // ---- edit actions ----
  type Action = "undo" | "redo" | "cut" | "copy" | "paste" | "delete" | "selectall" | "playpause" | "stop" | "record" | "import" | "export" | "zoomin" | "zoomout" | "save" | "saveas" | "open" | "new" | "savelast" | "pause" | "marker" | "prevmarker" | "nextmarker" | "sendvideo";
  async function run(action: Action) {
    const sel = selection;
    switch (action) {
      case "undo": { const l = hist.undoLabel; if (hist.undo()) { sync(); toast(`Undo ${l}`); restartIfPlaying(); } break; }
      case "redo": { const l = hist.redoLabel; if (hist.redo()) { sync(); toast(`Redo ${l}`); restartIfPlaying(); } break; }
      case "zoomin": setZoom(zoom * 1.5); break;
      case "zoomout": setZoom(zoom / 1.5); break;
      case "cut": if (sel && hasRange()) { const r = E.cut(hist.present, sel); clipboard = r.clipboard; commit("Cut", r.project); selection = { ...sel, b: sel.a }; } break;
      case "copy": if (sel && hasRange()) { clipboard = E.copy(hist.present, sel); toast("Copied selection"); } break;
      case "delete": if (sel && hasRange()) { commit("Delete", E.del(hist.present, sel)); selection = { ...sel, b: sel.a }; } break;
      case "paste": {
        if (!clipboard.length) break;
        const len = clipboard.reduce((s, c) => s + E.clipLen(c), 0);
        if (!sel) { // no target: paste as a new track
          const id = nextId++;
          commit("Paste", { ...hist.present, tracks: [...hist.present.tracks, { id, name: "Pasted audio", kind: "import", clips: clipboard.map((c) => ({ ...c })) }] });
          selection = { trackId: id, a: len, b: len };
        } else { commit("Paste", E.paste(hist.present, sel, clipboard)); selection = { ...sel, a: sel.a + len, b: sel.a + len }; }
        break;
      }
      case "selectall": { const t = activeTrack(); if (t) selection = { trackId: t.id, a: 0, b: E.trackLen(t) }; break; }
      case "playpause": {
        // Space: pause / resume the running playback in place
        if (level.playing) { await api.pausePlayback(!level.play_paused); break; }
        const t = activeTrack(); if (!t || !t.clips.length) break;
        const ranged = sel && sel.trackId === t.id && hasRange();
        const clips = ranged ? E.extract(t, sel!.a, sel!.b) : t.clips;
        const start = !ranged && sel && sel.trackId === t.id ? sel.a : 0;
        playOffset = ranged ? sel!.a : 0; playingId = t.id;
        try { await api.playClips(clips, fxFor(t), start); } catch (e) { toast(String(e), "err"); }
        break;
      }
      case "stop": await api.stopPlayback(); playingId = null; break;
      case "record": await toggleRecord(); break;
      case "import": await importFile(); break;
      case "export": if (activeTrack()) showExport = true; else toast("Nothing to export", "warn"); break;
      case "save": await saveProject(false); break;
      case "saveas": await saveProject(true); break;
      case "open": await openProject(); break;
      case "new": await newProject(); break;
      case "savelast": await saveLast(); break;
      case "pause": await togglePause(); break;
      case "marker": addMarker(); break;
      case "prevmarker": gotoMarker(-1); break;
      case "nextmarker": gotoMarker(1); break;
      case "sendvideo": await sendToVideo(); break;
    }
  }
  let playOffset = $state(0);

  // ---- projects ----
  let projectPath = $state<string | null>(null);
  let savedPresent: E.Project = hist.present;   // what's on disk
  let autosavedPresent: E.Project = hist.present;
  let dirty = $state(false);
  let recovery = $state<string | null>(null);   // autosave JSON offered for recovery
  let recentProjects = $state<string[]>([]);
  const projName = $derived(projectPath ? projectPath.split(/[\\/]/).pop()!.replace(/\.twt$/, "") : "Untitled");
  const markDirty = () => (dirty = hist.present !== savedPresent);

  async function saveProject(as = false) {
    let path = projectPath;
    if (as || !path) {
      const p = await save({ defaultPath: `${projName}.twt`, filters: [{ name: "StreamWave project", extensions: ["twt"] }] });
      if (!p) return; path = p.endsWith(".twt") ? p : p + ".twt";
    }
    try {
      await api.projectSave(path, serialize(hist.present, sources, nextId));
      projectPath = path; savedPresent = hist.present; markDirty();
      await api.autosaveClear(); autosavedPresent = hist.present;
      recentProjects = await api.recentProjects(); toast(`Saved ${projName}`);
    } catch (e) { toast(`Save failed: ${e}`, "err"); }
  }
  async function loadProjectJson(json: string, path: string | null) {
    const f = deserialize(json);
    const m = new Map<string, E.Source>(); const missing: string[] = [];
    for (const r of f.sources) {
      try { const w = await api.importAudio(r.path); m.set(r.path, { path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate, channels: w.channels, peaks: w.peaks }); }
      catch { missing.push(r.name); }
    }
    sources = m; nextId = Math.max(nextId, f.nextId);
    hist.reset({ tracks: f.tracks, markers: f.markers }); selection = null; projectPath = path;
    savedPresent = path ? hist.present : ({ tracks: [] } as E.Project); autosavedPresent = hist.present;
    sync(); markDirty();
    if (missing.length) toast(`Missing source files: ${missing.join(", ")}`, "err");
  }
  async function openProject(path?: string) {
    if (dirty && !(await ask("Discard unsaved changes?", { title: "StreamWave", kind: "warning" }))) return;
    const p = path ?? (await open({ filters: [{ name: "StreamWave project", extensions: ["twt"] }] }));
    if (!p || Array.isArray(p)) return;
    try { await loadProjectJson(await api.projectOpen(p), p); toast(`Opened ${projName}`); }
    catch (e) { toast(`Could not open project: ${e}`, "err"); }
    recentProjects = await api.recentProjects();
  }
  async function newProject() {
    if (dirty && !(await ask("Discard unsaved changes?", { title: "StreamWave", kind: "warning" }))) return;
    hist.reset({ tracks: [] }); savedPresent = autosavedPresent = hist.present; projectPath = null; selection = null; sync(); markDirty();
    await api.autosaveClear();
  }
  async function recover(accept: boolean) {
    const json = recovery; recovery = null;
    if (accept && json) { try { await loadProjectJson(json, null); toast("Recovered your unsaved work"); } catch (e) { toast(String(e), "err"); } }
    else await api.autosaveClear();
  }

  // ---- Studio Sound ----
  let studioBusy = $state(false);
  let analysis = $state<Analysis | null>(null);
  async function studioSound(strength: "Subtle" | "Medium" | "Strong") {
    const t = activeTrack(); if (!t) return;
    studioBusy = true;
    try {
      const r = await api.studioSound(t.clips, strength);
      analysis = r.analysis; ab = false;
      changeFx(r.chain.map((c) => slot(c.fx, c.bypass)), `Studio Sound (${strength.toLowerCase()})`);
      toast(`Studio Sound applied (${strength.toLowerCase()})`);
    } catch (e) { toast(`Studio Sound failed: ${e}`, "err"); }
    studioBusy = false;
  }

  // ---- OBS sync, markers, send back to video ----
  let obs = $state<ObsStatus | null>(null);
  let markerN = 1;
  function markerTime() {
    if (level.recording) return level.rec_secs;
    if (level.playing) return playOffset + level.play_secs;
    return selection?.a ?? 0;
  }
  function addMarker(label?: string, time = markerTime()) {
    commit("Add marker", E.addMarker(hist.present, { id: nextId++, time, label: label ?? `Marker ${markerN++}` }));
  }
  function gotoMarker(dir: 1 | -1) {
    const t = activeTrack(); if (!t) return;
    const m = E.nextMarker(hist.present, selection?.trackId === t.id ? selection.a : 0, dir);
    if (m) { selection = { trackId: t.id, a: m.time, b: m.time }; toast(m.label); }
  }
  async function onObsState(state: string) {
    if (!settings.follow_obs) return;
    try {
      if (state === "started" && !level.recording) await api.startRecording();
      else if (state === "stopped" && level.recording) await api.stopRecording();
      else if (state === "paused" && level.recording && !level.paused) await api.pauseRecording(true);
      else if (state === "resumed" && level.paused) await api.pauseRecording(false);
    } catch (e) { toast(String(e), "err"); }
  }
  async function togglePause() {
    if (!level.recording) return;
    const p = !level.paused;
    await api.pauseRecording(p);
    if (settings.follow_obs && obs?.connected && obs.recording && obs.paused !== p) api.obsPause(p).catch(() => {});
  }
  async function importObs(path: string) {
    toast("OBS recording ready, importing its audio tracks...");
    try {
      const parts = await api.obsSplit(path);
      for (const p of parts) await addTrack(p.path, "obs", `OBS · ${p.label}`, path);
      toast(`Imported ${parts.length} track(s) from OBS`);
    } catch (e) {
      toast(`Couldn't split OBS tracks (${e}). Importing the mixed audio instead.`, "warn");
      await addTrack(path, "obs", undefined, path);
    }
  }
  async function sendToVideo() {
    const t = activeTrack(); const video = t?.video ?? project.tracks.find((x) => x.video)?.video;
    if (!video) { toast("No OBS video linked. Import an OBS recording first.", "warn"); return; }
    const ext = video.split(".").pop()!.toLowerCase();
    const out = await save({ defaultPath: video.replace(/\.[^.]+$/, ` (StreamWave).${ext}`), filters: [{ name: "Video", extensions: [ext, "mkv", "mp4"] }] });
    if (!out) return;
    const tracks = project.tracks.filter((x) => x.video === video).map((x) => ({ clips: x.clips, fx: activeChain(x.fx) }));
    toast("Mixing and putting the audio back into the video (picture is copied, not re-encoded)...");
    try { await api.sendToVideo(video, tracks, (project.markers ?? []).map((m) => ({ time: m.time, label: m.label })), out); toast(`Video ready: ${out.split(/[\\/]/).pop()}`); }
    catch (e) { toast(`Send to video failed: ${e}`, "err"); }
  }

  // ---- export panel, pre-roll, mic check ----
  let showExport = $state(false);
  let armed = $state(true);
  let micStatus = $state<"ok" | "warn" | "bad" | "dead">("ok");
  const lastWarn: Record<string, number> = {};
  const MIC_MSG: Record<string, [string, "warn" | "err"]> = {
    Clipping: ["🔴 Too loud: your mic is clipping. Turn the input gain down or move back a little.", "err"],
    TooQuiet: ["🔈 Very quiet: move closer to the mic or turn the input gain up.", "warn"],
    Silence: ["🎙 No sound from the mic. Is it muted, unplugged, or is the wrong device selected?", "err"],
    DcOffset: ["〰 DC offset: the signal is shifted off-centre (often a cheap USB mic or bad cable). The low cut in the EQ fixes it.", "warn"],
    OneChannelSilent: ["🎚 One input channel is silent. Mono mic on a stereo interface? Pick the matching input or pan it in your interface software.", "warn"],
    Hum: ["⚡ Mains hum (50/60 Hz) detected. Check cables/grounding, or use the low cut in the EQ.", "warn"],
  };
  function onMic(r: MicReport) {
    micStatus = r.issues.includes("Clipping") ? "bad" : r.issues.includes("Silence") ? "dead" : r.issues.length ? "warn" : "ok";
    if (!armed && !level.recording) return;
    const now = Date.now();
    for (const i of r.issues) {
      if (now - (lastWarn[i] ?? 0) < 20000) continue;
      lastWarn[i] = now; toast(MIC_MSG[i][0], MIC_MSG[i][1]);
    }
  }
  async function setPreroll() { await api.setPreroll(settings.preroll_secs, settings.preroll_prepend); }
  async function saveLast() {
    if (!settings.preroll_secs) { toast("The pre-record buffer is off. Set it in Settings.", "warn"); return; }
    await api.saveLast(settings.preroll_secs); toast(`Saving the last ${settings.preroll_secs}s...`);
  }
  async function doExport(path: string, o: ExportOptions) {
    const t = activeTrack(); if (!t) return;
    try { await api.exportTrack(t.clips, activeChain(t.fx), path, o); toast(`Exported ${path.split(/[\\/]/).pop()}`); }
    catch (e) { toast(`Export failed: ${e}`, "err"); }
  }
  let ab = $state(false); // true = hear the original (effects bypassed)
  let lufs = $state<number | null>(null);
  let zoom = $state(1);
  const setZoom = (z: number) => (zoom = Math.max(1, Math.min(12, z)));
  const fxFor = (t: E.EditTrack) => (ab ? [] : activeChain(t.fx));

  /** Effects / A-B: push the new chain into the running playback; applied within ~80 ms, no restart. */
  function liveFx() {
    if (!level.playing || playingId === null) return;
    const t = project.tracks.find((x) => x.id === playingId); if (!t) return;
    api.updateFx(fxFor(t)).catch((e) => toast(String(e), "err"));
  }
  /** Clip edits (undo/redo) change the audio itself, so playback restarts at the current spot. */
  let restartTimer: ReturnType<typeof setTimeout> | undefined;
  function restartIfPlaying() {
    if (!level.playing || playingId === null) return;
    clearTimeout(restartTimer);
    restartTimer = setTimeout(async () => {
      const t = project.tracks.find((x) => x.id === playingId); if (!t) return;
      const at = playOffset + level.play_secs;
      playOffset = 0;
      try { await api.playClips(t.clips, fxFor(t), at); } catch (e) { toast(String(e), "err"); }
    }, 150);
  }
  function changeFx(fx: FxSlot[], label: string) {
    const t = activeTrack(); if (!t) return;
    commit(label, E.setFx(hist.present, t.id, fx)); lufs = null; liveFx();
  }
  async function learnNoise(slotId: number) {
    const t = activeTrack(); if (!t || !selection || !hasRange()) return;
    try {
      const profile = await api.learnNoise(E.extract(t, selection.a, selection.b));
      changeFx((t.fx ?? []).map((s) => (s.id === slotId && s.fx.type === "NoiseReduction" ? { ...s, fx: { ...s.fx, profile } } : s)), "Learn noise profile");
      toast("Noise profile learned");
    } catch (e) { toast(String(e), "err"); }
  }
  async function measure() {
    const t = activeTrack(); if (!t) return;
    try { lufs = await api.measureLufs(t.clips, fxFor(t)); if (lufs === null) toast("Too short or silent to measure", "warn"); } catch (e) { toast(String(e), "err"); }
  }

  // Keyboard: shortcuts never swallow keys inside text fields (native copy/paste/undo work there).
  const recent: Record<string, number> = {};
  function isTextField(el: Element | null) {
    if (el instanceof HTMLInputElement) return !["checkbox", "radio", "button", "range", "submit", "color", "file"].includes(el.type);
    return el instanceof HTMLTextAreaElement || (el instanceof HTMLElement && el.isContentEditable);
  }
  function keyAction(e: KeyboardEvent): Action | null {
    const mod = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
    if (mod && !e.altKey) {
      if (k === "z") return e.shiftKey ? "redo" : "undo";
      if (k === "s") return e.shiftKey ? "saveas" : "save";
      if (k === "r" && e.shiftKey) return "savelast";
      if (k === "p" && !e.shiftKey) return "pause";
      if (e.shiftKey) return null;
      return ({ y: "redo", x: "cut", c: "copy", v: "paste", a: "selectall", i: "import", e: "export", o: "open", n: "new" } as Record<string, Action>)[k] ?? null;
    }
    if (e.altKey) return null;
    if (e.key === "Delete" || e.key === "Backspace") return "delete";
    if (e.code === "Space") return "playpause";
    if (e.key === "Escape" && level.playing) return "stop";
    if (e.code === "KeyR" && !e.repeat) return "record";
    if (e.code === "KeyM" && !e.repeat) return "marker";
    if (e.key === "[") return "prevmarker";
    if (e.key === "]") return "nextmarker";
    if (e.key === "+" || e.key === "=") return "zoomin";
    if (e.key === "-" || e.key === "_") return "zoomout";
    return null;
  }
  function onKey(e: KeyboardEvent) {
    const a = keyAction(e); if (!a) return;
    recent[a] = performance.now();
    if (isTextField(document.activeElement)) return; // let the browser do its thing
    e.preventDefault();
    run(a);
  }
  // Menu accelerators may fire instead of (or as well as) keydown depending on OS/webview.
  async function onMenu(a: Action) {
    if (performance.now() - (recent[a] ?? -1e9) < 400) return; // already handled by keydown
    recent[a] = performance.now();
    const el = document.activeElement;
    if (isTextField(el) && ["undo", "redo", "cut", "copy", "paste", "selectall"].includes(a)) {
      if (a === "paste") { try { const t = await readText(); if (t) document.execCommand("insertText", false, t); } catch {} }
      else if (a === "selectall") (el as HTMLInputElement).select?.();
      else document.execCommand(a);
      return;
    }
    run(a);
  }
  async function toggleRecord() {
    try {
      recording ? await api.stopRecording() : await api.startRecording();
      if (settings.follow_obs && obs?.connected) {
        if (!recording && !obs.recording) await api.obsStart();
        else if (recording && obs.recording) await api.obsStop();
      }
    } catch (e) { toast(String(e), "err"); }
  }
  async function importFile() {
    const sel = await open({ multiple: true, filters: [{ name: "Audio / Video", extensions: ["wav", "mp3", "flac", "ogg", "m4a", "aac", "mp4", "mkv", "aiff", "caf"] }] });
    for (const p of (Array.isArray(sel) ? sel : sel ? [sel] : [])) await addTrack(p, "import");
  }
  async function togglePlay(t: E.EditTrack) {
    if (playingId === t.id && level.playing) { await api.pausePlayback(!level.play_paused); return; }
    if (selection?.trackId !== t.id) selection = { trackId: t.id, a: 0, b: 0 };
    if (level.playing) { await api.stopPlayback(); playingId = null; }
    const sel = selection; const ranged = sel && sel.trackId === t.id && hasRange();
    const clips = ranged ? E.extract(t, sel!.a, sel!.b) : t.clips; if (!clips.length) return;
    playOffset = ranged ? sel!.a : 0; playingId = t.id;
    try { await api.playClips(clips, fxFor(t), ranged ? 0 : sel?.a ?? 0); } catch (e) { toast(String(e), "err"); }
  }
  function progressFor(t: E.EditTrack) {
    if (playingId !== t.id || !level.playing || !level.play_total) return 0;
    const len = E.trackLen(t); return len ? (playOffset + level.play_secs) / len : 0;
  }
  async function setInput(id: string) { settings.input_device = id; await api.selectInput(id); }
  async function setOutput(id: string) { settings.output_device = id; await api.selectOutput(id); }
  async function setLatency(p: LatencyProfile) { settings.latency = p; await api.setLatency(p, settings.auto_backoff); }

  onMount(() => {
    (async () => {
      settings = await api.getSettings(); devices = await api.listDevices(); recentProjects = settings.recent_projects;
      const a = await api.autosaveCheck();
      if (a) { try { if (deserialize(a).tracks.length) recovery = a; else api.autosaveClear(); } catch { api.autosaveClear(); } }
    })();
    const autosave = setInterval(async () => {
      if (hist.present === autosavedPresent || !hist.present.tracks.length) return;
      try { await api.autosaveWrite(serialize(hist.present, sources, nextId)); autosavedPresent = hist.present; } catch {}
    }, 30000);
    const beforeUnload = (e: BeforeUnloadEvent) => { if (dirty) { e.preventDefault(); } };
    window.addEventListener("beforeunload", beforeUnload);
    const uns = [
      listen<Level>("level", (e) => (level = e.payload)),
      listen<EngineStatus>("engine-status", (e) => { engine = e.payload; if (e.payload.note) toast(e.payload.note, "warn"); }),
      listen<string>("record-finished", (e) => { toast("Recording saved"); addTrack(e.payload, "mic"); }),
      listen<string>("record-part", (e) => toast(`Long take: saved part ${e.payload.split(/[\\/]/).pop()}`)),
      listen<string>("record-error", (e) => toast(`Recording error: ${e.payload}`, "err")),
      listen<string>("playback-error", (e) => toast(`Playback error: ${e.payload}`, "err")),
      listen<{ devices: DeviceInfo[]; added: string[]; removed: string[] }>("devices-changed", (e) => {
        devices = e.payload.devices;
        e.payload.added.forEach((a) => toast(`Plugged in: ${a.split("::")[1]}`));
        e.payload.removed.forEach((r) => toast(`Unplugged: ${r.split("::")[1]}`, "warn"));
      }),
      listen<{ device: string | null; saved: string | null }>("record-device-lost", (e) => {
        toast(`Your mic disconnected${e.payload.saved ? " – recording so far was saved" : ""}. Switched to default device.`, "err");
        if (e.payload.saved) addTrack(e.payload.saved, "mic");
      }),
      listen<string>("device-restored", (e) => toast(`${e.payload.split("::")[1]} is back – switched to it`)),
      listen<LatencyProfile>("latency-backoff", (e) => { settings.latency = e.payload; toast(`Audio glitches detected – switched to "${e.payload}" latency`, "warn"); }),
      listen<string[]>("recordings-recovered", (e) => { toast(`Recovered ${e.payload.length} interrupted recording(s)`, "warn"); e.payload.forEach((p) => addTrack(p, "recovered")); }),
    ];
    uns.push(listen<string>("menu", (e) => onMenu(e.payload as Action)));
    uns.push(listen<MicReport>("mic-check", (e) => onMic(e.payload)));
    uns.push(listen<string>("obs-scene", (e) => { if (level.recording) addMarker(`Scene: ${e.payload}`, level.rec_secs); }));
    window.addEventListener("keydown", onKey);
    return () => { uns.forEach((u) => u.then((f) => f())); window.removeEventListener("keydown", onKey); clearInterval(autosave); window.removeEventListener("beforeunload", beforeUnload); };
  });
</script>

{#if settings && settings.onboarded === false}
  <Onboarding {settings} {devices} {level} onsetinput={setInput} ondone={async () => { settings.onboarded = true; await api.saveSettings(settings); }} />
{/if}
<div class="app">
  <header>
    <div class="brand"><img class="logo" src={logoUrl} alt="" width="22" height="22" /> Stream<b>Wave</b> <span class="proj">· {projName}{dirty ? " •" : ""}</span></div>
    <select class="recent" value="" onchange={(e) => { const v = e.currentTarget.value; e.currentTarget.value = ""; e.currentTarget.blur(); if (v === "__open") openProject(); else if (v === "__new") newProject(); else if (v) openProject(v); }} title="Projects">
      <option value="">📁 Project</option><option value="__new">New project (Ctrl+N)</option><option value="__open">Open... (Ctrl+O)</option>
      {#each recentProjects as r}<option value={r}>{r.split(/[\\/]/).pop()}</option>{/each}
    </select>
    <button onclick={() => saveProject(false)} title="Save (Ctrl+S)">💾</button>
    <div class="devsel">
      <select value={engine?.input_id ?? settings.input_device ?? ""} onchange={(e) => { setInput(e.currentTarget.value); e.currentTarget.blur(); }} title="Microphone">
        {#each inputs as d}<option value={d.id}>🎙 {d.friendly_name}{d.is_default ? " (default)" : ""}</option>{/each}
      </select>
    </div>
    <button onclick={() => run("undo")} disabled={!canUndo} title="Undo (Ctrl+Z)">↶</button>
    <button onclick={() => run("redo")} disabled={!canRedo} title="Redo (Ctrl+Y)">↷</button>
    <button onclick={importFile}>＋ Import</button>
    <button onclick={() => run("export")} disabled={!project.tracks.length} title="Export (Ctrl+E)">⇩ Export</button>
    <button onclick={() => (showSettings = !showSettings)}>⚙ Settings</button>
  </header>

  <main>
    <section class="left">
      <div class="recpanel">
        <button class="bigrec" class:on={recording} onclick={toggleRecord} title="Record (R)">
          <span class="ico">{recording ? "■" : "●"}</span>
        </button>
        <div class="recinfo">
          <div class="time">{fmtTime(level.rec_secs)}{#if level.paused} <span class="paused">PAUSED</span>{/if}</div>
          <div class="hint">{recording ? "Recording… press R or click to stop" : "Press R or click to record"}</div>
          <Meter left={level.left} right={level.right} status={micStatus} />
          <div class="armrow">
            <label class="chk"><input type="checkbox" bind:checked={armed} /> Armed: mic check{settings.preroll_secs ? ` + ${settings.preroll_secs}s pre-record buffer` : ""}</label>
            <button class="small" onclick={saveLast} disabled={!settings.preroll_secs} title="Ctrl+Shift+R">⟲ Save last {settings.preroll_secs}s</button>
          </div>
          {#if engine}<div class="small">{engine.sample_rate} Hz · {engine.channels} ch · {engine.buffer_frames || "driver"} frames · {settings.latency}{level.xruns ? ` · ${level.xruns} dropouts` : ""}</div>{/if}
        </div>
      </div>

      <div class="tracks" onwheel={(e) => { if (e.ctrlKey) { e.preventDefault(); setZoom(zoom * (e.deltaY < 0 ? 1.25 : 0.8)); } }}>
        {#if project.tracks.length === 0}
          <div class="empty">No tracks yet. Hit the big red button, import a file, or record in OBS.</div>
        {/if}
        {#each project.tracks as t (t.id)}
          {@const len = E.trackLen(t)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="track" class:act={selection?.trackId === t.id}>
            <div class="thead">
              <button class="play" onclick={() => togglePlay(t)}>{playingId === t.id && level.playing && !level.play_paused ? "❚❚" : "▶"}</button>
              {#if playingId === t.id && level.playing}<button class="play" title="Stop" onclick={() => run("stop")}>■</button>{/if}
              <div class="tname" title={t.clips.map((c) => c.src).join("\n")}>{t.name}</div>
              <span class="tag {t.kind}">{t.kind}</span>
              <span class="small">{fmtTime(len)}{t.clips.length > 1 ? ` · ${t.clips.length} clips` : ""}{t.fx?.some((f) => !f.bypass) ? ` · ${t.fx.filter((f) => !f.bypass).length} fx` : ""}
                {#if selection?.trackId === t.id && selection.b - selection.a > 0.0005} · sel {fmtTime(selection.a)}–{fmtTime(selection.b)}{/if}</span>
              <button class="x" onclick={() => removeTrack(t.id)} title="Remove from project (file is kept)">✕</button>
            </div>
            <div class="scroll"><div style="width:{zoom * 100}%">
            <WaveCanvas peaks={E.trackPeaks(t, sources, Math.round(2000 * zoom))} duration={len} active={selection?.trackId === t.id}
              selection={selection?.trackId === t.id ? selection : null} progress={progressFor(t)} markers={project.markers ?? []}
              onselect={(a, b) => (selection = { trackId: t.id, a, b })}
              color={t.kind === "obs" ? "#8b7bff" : t.kind === "recovered" ? "#ffb020" : "#19d3b0"} />
            </div></div>
          </div>
        {/each}
        {#if project.tracks.length}<div class="small hint2">Drag to select · Shift-click to extend · Ctrl+X/C/V · Del · Ctrl+A · Ctrl+Z / Ctrl+Y · Space play/pause · M marker · [ ] jump · Ctrl+scroll or +/− zoom ({zoom.toFixed(1)}×) — edits never change your original files</div>{/if}
      </div>
    </section>

    <aside>
      {#if recovery}
        <div class="panel rec-banner"><b>Unsaved work found</b><div class="small">StreamWave closed unexpectedly. Do you want your last autosave back?</div>
          <div class="row2"><button onclick={() => recover(true)}>Recover</button><button onclick={() => recover(false)}>Discard</button></div></div>
      {/if}
      {#if showExport && activeTrack()}
        <ExportPanel trackName={activeTrack()!.name} onexport={doExport} onclose={() => (showExport = false)} />
      {/if}
      <StudioPanel enabled={!!activeTrack()} {ab} busy={studioBusy} {analysis} onrun={studioSound} onab={() => { ab = !ab; lufs = null; liveFx(); }} />
      <FxPanel track={activeTrack()} {ab} {lufs} hasSelection={hasRange() && selection?.trackId === activeTrack()?.id}
        onchange={changeFx} onlearn={learnNoise} onab={() => { ab = !ab; lufs = null; liveFx(); }} onmeasure={measure} />
      <ObsPanel {settings} onRecordingReady={importObs} onstatus={(s) => (obs = s)} onstate={onObsState} />
      {#if project.tracks.some((t) => t.video)}
        <button class="sendvid" onclick={sendToVideo}>🎬 Send back to video</button>
      {/if}
      {#if showSettings}
        <div class="panel">
          <h3>Audio settings</h3>
          <label for="in">Microphone / input</label>
          <select id="in" value={engine?.input_id ?? ""} onchange={(e) => { setInput(e.currentTarget.value); e.currentTarget.blur(); }}>
            {#each inputs as d}<option value={d.id}>{d.friendly_name} · {d.host}</option>{/each}
          </select>
          <label for="out">Speakers / output</label>
          <select id="out" value={settings.output_device ?? ""} onchange={(e) => { setOutput(e.currentTarget.value); e.currentTarget.blur(); }}>
            <option value="">System default</option>
            {#each outputs as d}<option value={d.id}>{d.friendly_name} · {d.host}</option>{/each}
          </select>
          <label for="lat">Latency</label>
          <div class="seg" id="lat">
            {#each [["LowLatency", "Low latency"], ["Balanced", "Balanced"], ["Safe", "Safe"]] as [v, n]}
              <button class:sel={settings.latency === v} onclick={() => setLatency(v as LatencyProfile)}>{n}</button>
            {/each}
          </div>
          <label for="pre">Pre-record buffer: {settings.preroll_secs}s {settings.preroll_secs ? "" : "(off)"}</label>
          <input id="pre" type="range" min="0" max="60" step="5" bind:value={settings.preroll_secs} onchange={setPreroll} />
          <label class="chk"><input type="checkbox" bind:checked={settings.preroll_prepend} onchange={setPreroll} /> Add the buffer to the start when I press Record</label>
          <div class="small">The mic is always shared, so OBS, Discord and other apps can use it at the same time.</div>
          <label class="chk"><input type="checkbox" bind:checked={settings.auto_backoff} onchange={() => setLatency(settings.latency)} /> Auto-switch to safer latency on dropouts</label>
        </div>
      {/if}
    </aside>
  </main>

  <div class="toasts">{#each toasts as t (t.id)}<div class="toast {t.kind}">{t.text}</div>{/each}</div>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 12px; padding: 10px 16px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .brand { font-size: 17px; margin-right: auto; } .brand b { color: var(--accent); } .brand { display: flex; align-items: center; gap: 8px; } .logo { border-radius: 6px; }
  .devsel { width: 320px; }
  main { flex: 1; display: grid; grid-template-columns: 1fr 320px; gap: 16px; padding: 16px; overflow: hidden; }
  .left { display: flex; flex-direction: column; gap: 16px; overflow: hidden; }
  aside { display: flex; flex-direction: column; gap: 16px; overflow-y: auto; }
  .panel { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 14px; }
  .recpanel { display: flex; gap: 24px; align-items: center; background: var(--panel); border: 1px solid var(--line); border-radius: 16px; padding: 20px; }
  .bigrec { width: 110px; height: 110px; border-radius: 50%; border: 4px solid #2e3746; background: radial-gradient(circle at 35% 30%, #ff6b84, var(--rec)); flex: none; transition: transform .1s, box-shadow .2s; }
  .bigrec:hover { transform: scale(1.04); border-color: #fff3; }
  .bigrec.on { animation: pulse 1.4s infinite; }
  .bigrec .ico { font-size: 42px; color: #fff; }
  @keyframes pulse { 0%,100% { box-shadow: 0 0 0 0 #ff3b5c88; } 50% { box-shadow: 0 0 0 18px #ff3b5c00; } }
  .recinfo { flex: 1; }
  .time { font-size: 38px; font-variant-numeric: tabular-nums; font-weight: 300; }
  .hint, .small { color: var(--muted); font-size: 12px; } .hint { margin-bottom: 8px; }
  .tracks { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; }
  .empty { color: var(--muted); text-align: center; padding: 60px; border: 2px dashed var(--line); border-radius: 12px; }
  .track.act { border-color: #19d3b066; }
  .scroll { overflow-x: auto; }
  .paused { font-size: 14px; color: var(--warn); letter-spacing: .1em; }
  .sendvid { width: 100%; padding: 10px; font-weight: 600; }
  .proj { color: var(--muted); font-size: 13px; } .recent { width: 130px; }
  .armrow { display: flex; justify-content: space-between; align-items: center; margin-top: 6px; font-size: 12px; } .armrow label { margin: 0; }
  .small { font-size: 11px; } button.small { padding: 3px 8px; }
  .rec-banner { border-color: var(--warn); } .row2 { display: flex; gap: 8px; margin-top: 8px; } .row2 button { flex: 1; }
  .hint2 { text-align: center; padding: 6px; }
  .track { background: var(--panel); border: 1px solid var(--line); border-radius: 12px; padding: 10px 12px; }
  .thead { display: flex; align-items: center; gap: 10px; margin-bottom: 6px; }
  .play { width: 34px; height: 34px; padding: 0; border-radius: 50%; }
  .tname { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 40%; }
  .tag { font-size: 10px; text-transform: uppercase; padding: 2px 7px; border-radius: 10px; background: #19d3b022; color: var(--accent); }
  .tag.obs { background: #8b7bff22; color: #a99cff; } .tag.recovered { background: #ffb02022; color: var(--warn); }
  .x { margin-left: auto; background: transparent; border: none; color: var(--muted); }
  .seg { display: flex; gap: 4px; } .seg button { flex: 1; padding: 6px 4px; font-size: 12px; } .seg .sel { background: var(--accent); color: #002; border-color: var(--accent); }
  .chk { display: flex; gap: 8px; align-items: center; } .chk input { width: auto; }
  .toasts { position: fixed; bottom: 16px; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; gap: 6px; }
  .toast { background: var(--panel2); border: 1px solid var(--line); border-left: 4px solid var(--accent); padding: 9px 14px; border-radius: 8px; font-size: 13px; }
  .toast.warn { border-left-color: var(--warn); } .toast.err { border-left-color: var(--rec); }
</style>
