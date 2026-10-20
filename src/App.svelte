<script lang="ts">
  import { onMount, tick } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open, save, ask } from "@tauri-apps/plugin-dialog";
  import { readText } from "@tauri-apps/plugin-clipboard-manager";
  import * as E from "./lib/editModel";
  import Meter from "./lib/Meter.svelte";
  import Dropdown from "./lib/ui/Dropdown.svelte";
  import Ruler from "./lib/ui/Ruler.svelte";
  import Preview from "./lib/ui/Preview.svelte";
  import Knob from "./lib/ui/Knob.svelte";
  import LedToggle from "./lib/ui/LedToggle.svelte";
  import { ANCHOR, FollowState, HeadClock, followStep } from "./lib/follow";
  import { switchProject } from "./lib/projectSwitch";
  import EffectsWorkspace from "./lib/EffectsWorkspace.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import About from "./lib/About.svelte";
  import UpdateDialog from "./lib/UpdateDialog.svelte";
  import { shouldAutoCheck } from "./lib/updates";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { buildFromAup3, importedFiles, summary as aup3Summary, isAup3, isMedia, isProjectFile, type Aup3Result } from "./lib/aup3Import";
  import logoUrl from "./assets/logo.png";
  import WaveCanvas from "./lib/WaveCanvas.svelte";
  import LabelLane from "./lib/LabelLane.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import EffectDialog from "./lib/EffectDialog.svelte";
  import { audioMenu, labelMenu, emptyMenu, isBrowserShortcut, type MenuItem } from "./lib/contextMenu";
  import { defaultFx, FX_NAMES, type Effect, type EffectType } from "./lib/fx";
  import { parseSpec, type Spec } from "./lib/spectrogram";
  import ObsPanel from "./lib/ObsPanel.svelte";
  import FxPanel from "./lib/FxPanel.svelte";
  import ExportPanel from "./lib/ExportPanel.svelte";
  import StudioPanel from "./lib/StudioPanel.svelte";
  import ImportArrangeDialog from "./lib/ImportArrangeDialog.svelte";
  import LayerFxPanel from "./lib/LayerFxPanel.svelte";
  import { serialize, deserialize } from "./lib/project";
  import { activeChain, slot, type FxSlot } from "./lib/fx";
  import { api, fmtTime, type DeviceInfo, type EngineStatus, type Level, type Settings, type LatencyProfile, type MicReport, type ExportOptions, type Analysis, type ObsStatus } from "./lib/api";

  let settings = $state<Settings>({ input_device: null, output_device: null, latency: "Balanced", auto_backoff: true,
    record_dir: null, obs_host: "localhost", obs_port: 4455, obs_password: "", obs_auto_import: true, preroll_secs: 30, preroll_prepend: false, recent_projects: [], follow_obs: false, follow_playhead: true });
  let devices = $state<DeviceInfo[]>([]);
  let level = $state<Level>({ left: 0, right: 0, recording: false, paused: false, rec_secs: 0, xruns: 0, playing: false, play_secs: 0, play_total: 0 });
  let engine = $state<EngineStatus | null>(null);
  // ---- non-destructive edit model ----
  const hist = new E.History({ tracks: [] });
  let project = $state.raw<E.Project>(hist.present);
  let sources = $state.raw(new Map<string, E.Source>());
  let selection = $state<E.Selection | null>(null);
  let clipboard: E.Clip[][] = [];
  let playingId = $state<number | null>(null);
  let canUndo = $state(false), canRedo = $state(false);
  let toasts = $state<{ id: number; text: string; kind: string }[]>([]);
  let showSettings = $state(false);
  let showAbout = $state(false);
  /** null = closed, "manual" = Help > Check for updates, "quiet" = startup check */
  let updateMode = $state<null | "manual" | "quiet">(null);
  async function patchSettings(p: Partial<Settings>) {
    Object.assign(settings, p);
    try { const s = await api.getSettings(); await api.saveSettings({ ...s, ...p }); } catch {}
  }
  let nextId = 1;

  const inputs = $derived(devices.filter((d) => d.is_input));
  const outputs = $derived(devices.filter((d) => !d.is_input));
  const virtualOutputs = $derived(outputs.filter((d) => /cable|vb-audio|voicemeeter|loopback|stereo mix|virtual audio|blackhole/i.test(d.friendly_name)));
  const recording = $derived(level.recording);

  function toast(text: string, kind = "info") {
    const id = nextId++; toasts.push({ id, text, kind });
    setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 5000);
  }
  function sync() { project = hist.present; canUndo = hist.canUndo; canRedo = hist.canRedo; markDirty();
    if (selection) {
      const ids = new Set(project.tracks.map((t) => t.id));
      const rest = (selection.tracks ?? []).filter((id) => ids.has(id));
      if (!ids.has(selection.trackId)) selection = rest.length ? { ...selection, trackId: rest[0], tracks: rest } : null;
      else if (rest.length !== (selection.tracks ?? []).length) selection = { ...selection, tracks: rest };
    } }
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
      api.preparePlayback().catch(() => {});
    } catch (e) { toast(String(e), "err"); }
  }
  // ---- Audacity (.aup3) import: read-only, audio extracted to a media folder ----
  let aupBusy = $state<{ name: string; frac: number } | null>(null);
  let importReport = $state<{ title: string; text: string; dir: string } | null>(null);
  async function importAudacity(path?: string) {
    if (aupBusy) { toast("An Audacity import is already running", "warn"); return; }
    const p = path ?? (await open({ filters: [{ name: "Audacity project", extensions: ["aup3"] }] }));
    if (!p || Array.isArray(p)) return;
    const base = p.split(/[\\/]/).pop()!.replace(/\.aup3$/i, "");
    // saved project: keep the audio next to it; otherwise in the recordings folder
    const mediaDir = projectPath ? projectPath.replace(/\.twt$/i, "") + " media" : null;
    aupBusy = { name: base, frac: 0 };
    const un = listen<number>("aup3-progress", (e) => { if (aupBusy) aupBusy = { ...aupBusy, frac: e.payload }; });
    try {
      const r = await api.importAup3(p, mediaDir);
      const m = new Map(sources); const real = new Map<string, string>();
      for (const f of importedFiles(r)) {
        const w = await api.importAudio(f);
        m.set(w.path, { path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate, channels: w.channels, peaks: w.peaks });
        real.set(f, w.path);
      }
      sources = m;
      const { tracks, markers } = buildFromAup3(r, (f) => real.get(f) ?? f, () => nextId++);
      if (!tracks.length && !markers.length) { toast(`"${base}" has no audio to import`, "warn"); return; }
      commit("Import Audacity project", { ...hist.present, tracks: [...hist.present.tracks, ...tracks], markers: [...(hist.present.markers ?? []), ...markers].sort((a, b) => a.time - b.time) });
      if (tracks.length) selection = { trackId: tracks[0].id, a: 0, b: 0 };
      importReport = { title: `Imported "${r.name || base}"`, text: aup3Summary(r), dir: r.media_dir };
    } catch (e) { toast(`Could not import Audacity project: ${e}`, "err"); }
    finally { aupBusy = null; un.then((f) => f()); }
  }
  async function openDropped(paths: string[]) {
    const twt = paths.find(isProjectFile);
    if (twt) { await openProject(twt); return; }
    let skipped = 0; const media: string[] = [];
    for (const p of paths) {
      if (isAup3(p)) await importAudacity(p);
      else if (isMedia(p)) media.push(p);
      else skipped++;
    }
    if (skipped) toast(`${skipped} file${skipped > 1 ? "s" : ""} skipped (not audio, video or an Audacity project)`, "warn");
    await importMedia(media);
  }
  // ---- multi-file import: 2+ files at once ask how to arrange them (nothing changes until confirmed) ----
  let importAsk = $state<{ files: string[]; playhead: number } | null>(null);
  async function importMedia(paths: string[]) {
    if (!paths.length) return;
    if (paths.length === 1) { await addTrack(paths[0], "import"); return; }
    importAsk = { files: paths, playhead: level.playing ? playOffset + level.play_secs : selection?.a ?? 0 };
  }
  async function importArranged(order: string[], mode: E.ImportMode, start: number) {
    importAsk = null;
    const m = new Map(sources); const files: E.ImportFile[] = []; const failed: string[] = [];
    toast(`Importing ${order.length} files…`);
    for (const p of order) {
      try {
        const w = await api.importAudio(p);
        m.set(w.path, { path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate, channels: w.channels, peaks: w.peaks });
        files.push({ path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate });
      } catch { failed.push(p.split(/[\\/]/).pop()!); }
    }
    if (failed.length) toast(`Couldn't import: ${failed.join(", ")}`, "err");
    const r = E.arrangeImport(hist.present, files, mode, start, () => nextId++);
    if (!r.trackIds.length) return;
    sources = m;
    commit(mode === "sequential" ? `Import ${files.length} files in sequence` : `Import ${files.length} files as tracks`, r.project);
    selection = { trackId: r.trackIds[0], a: start, b: start };
    toast(mode === "sequential" ? `Imported ${files.length} files one after another on one track` : `Imported ${files.length} files as ${files.length} tracks`);
  }
  let dropHover = $state(false);
  function toggleTrackFlag(id: number, k: "muted" | "solo") {
    commit(k === "muted" ? "Mute" : "Solo", { ...hist.present, tracks: hist.present.tracks.map((t) => (t.id === id ? { ...t, [k]: !t[k] } : t)) });
  }
  // ---- multi-track selection: drag across tracks, Ctrl-click adds/removes a track ----
  const isSel = (id: number) => !!selection && (selection.trackId === id || !!selection.tracks?.includes(id));
  const selIds = () => E.selectedIds(project, selection);
  let dragAnchor: number | null = null;
  function onSel(t: E.EditTrack, a: number, b: number, e: PointerEvent, phase: "down" | "move") {
    if (phase === "down") {
      if ((e.ctrlKey || e.metaKey) && selection) {
        const cur = selIds(); const has = cur.includes(t.id);
        const next = has ? cur.filter((x) => x !== t.id) : [...cur, t.id];
        if (next.length) { selection = { ...selection, trackId: has ? (selection.trackId === t.id ? next[0] : selection.trackId) : t.id, tracks: next }; dragAnchor = null; return; }
      }
      dragAnchor = t.id;
      const keep = e.shiftKey && selection ? selection.tracks : undefined;
      selection = { trackId: t.id, a, b, ...(keep ? { tracks: keep } : {}) };
      return;
    }
    if (dragAnchor === null) return;
    // which track row is the pointer over now?
    let over = dragAnchor;
    for (const el of document.querySelectorAll<HTMLElement>("[data-tid]")) {
      const r = el.getBoundingClientRect(); if (e.clientY >= r.top && e.clientY <= r.bottom) { over = +el.dataset.tid!; break; }
    }
    const order = project.tracks.map((x) => x.id), i = order.indexOf(dragAnchor), j = order.indexOf(over);
    const ids = i < 0 || j < 0 ? [dragAnchor] : order.slice(Math.min(i, j), Math.max(i, j) + 1);
    selection = { trackId: dragAnchor, a, b, ...(ids.length > 1 ? { tracks: ids } : {}) };
  }
  /** Double-click: select the clip under the pointer. */
  function pickClip(t: E.EditTrack, at: number) {
    const q = E.audioClipAt(t, at); if (q) selection = { trackId: t.id, a: q.a, b: q.b };
  }
  /** Project duration = the latest content end (clips, labels, markers). */
  const contentLen = $derived(Math.max(0.001, ...project.tracks.map((t) => E.trackLen(t)), ...(project.markers ?? []).map((m) => m.time)));
  // ---- moving clips: drag a clip's title bar along the timeline or onto another audio track ----
  type Drag = { fromId: number; index: number; len: number; grab: number; x0: number; y0: number; moved: boolean; room: number;
    toId: number; at: number; ok: boolean; snapTo: number | null; noSnap: boolean; x: number; y: number; name: string };
  let clipDrag = $state<Drag | null>(null);
  // while a clip is dragged the timeline gets room for it after the current end (fixed for the drag, so the scale doesn't jump)
  const timelineLen = $derived(clipDrag?.moved ? Math.max(contentLen, clipDrag.room) : contentLen);
  const clipName = (c: E.Clip) => sources.get(c.src)?.name ?? c.src.split(/[\\/]/).pop() ?? "clip";
  /** Render rate of a track (its first audio clip's source); null for an empty track. */
  function trackRate(t: E.EditTrack): number | null { const c = t.clips.find((x) => !x.gap); return c ? sources.get(c.src)?.sampleRate ?? null : null; }
  const srcDur = (src: string) => sources.get(src)?.duration;
  const laneClips = (t: E.EditTrack) => E.audioClips(t).map((q) => ({ a: q.a, b: q.b, index: q.index, name: clipName(q.clip), fx: (q.clip.fx ?? []).filter((f) => !f.bypass).length,
    fi: (q.clip.fade_in_ms ?? 0) / 1000, fo: (q.clip.fade_out_ms ?? 0) / 1000, fcurve: q.clip.fade_curve,
    canX: E.canCrossfade(t, q.index), xf: E.xfadeLen(t, q.index, srcDur), xcurve: q.clip.xfade_curve, xo: E.xfadeLen(t, q.index + 1, srcDur) }));
  // ---- fades & crossfades from the lane handles (one undo step each; audio updates in place while playing) ----
  function laneFade(t: E.EditTrack, index: number, edge: "in" | "out", ms: number) {
    commit(edge === "in" ? `Fade-in ${ms} ms` : `Fade-out ${ms} ms`, E.setClipFade(hist.present, t.id, index, edge, ms)); liveDry();
  }
  let xfMenu = $state<{ trackId: number; index: number; x: number; y: number } | null>(null);
  function laneXfade(trackId: number, index: number, ms: number, curve?: E.XfadeCurve) {
    const t = hist.present.tracks.find((x) => x.id === trackId); if (!t) return;
    const r = E.setCrossfade(hist.present, trackId, index, ms, curve ?? t.clips[index]?.xfade_curve ?? "equal_power", srcDur);
    if (!r) { toast("A crossfade needs two neighbouring clips (no silence between them)", "warn"); return; }
    commit(r.ms ? `Crossfade ${r.ms} ms` : "Remove crossfade", r.project);
    if (r.trimmed > 0.0005) { toast(`No spare audio at that cut: the clips now overlap by ${r.ms} ms (later audio moved ${(r.trimmed * 1000).toFixed(0)} ms earlier)`); restartIfPlaying(); }
    else liveDry();
  }
  const xfClip = $derived.by(() => {
    if (!xfMenu) return null; const t = project.tracks.find((x) => x.id === xfMenu!.trackId); const c = t?.clips[xfMenu.index];
    if (!t || !c || !E.canCrossfade(t, xfMenu.index)) return null;
    const p = t.clips[xfMenu.index - 1], tail = Math.max(0, (srcDur(p.src) ?? p.end) - p.end);
    return { t, c, ms: E.xfadeLen(t, xfMenu.index, srcDur), max: Math.round(Math.min(5000, 1000 * Math.min(E.clipLen(c), E.clipLen(p)))), handles: Math.round(2000 * Math.min(tail, c.start)) };
  });
  function scrollEl(id: number) { return document.querySelector<HTMLElement>(`[data-tid="${id}"] .scroll`); }
  /** px per second on the shared timeline (all lanes use the same scale). */
  function pps(id: number) { const el = scrollEl(id); return el ? (el.clientWidth * zoom) / timelineLen : 1; }
  function timeAtX(id: number, clientX: number) { const el = scrollEl(id); if (!el) return 0; const r = el.getBoundingClientRect(); return (clientX - r.left + el.scrollLeft) / pps(id); }
  function grabClip(t: E.EditTrack, index: number, e: PointerEvent, grabAt: number) {
    const q = E.placed(t)[index]; if (!q || q.clip.gap) return;
    clipDrag = { fromId: t.id, index, len: q.b - q.a, grab: grabAt - q.a, x0: e.clientX, y0: e.clientY, moved: false, room: contentLen + (q.b - q.a),
      toId: t.id, at: q.a, ok: true, snapTo: null, noSnap: false, x: e.clientX, y: e.clientY, name: clipName(q.clip) };
    window.addEventListener("pointermove", dragMove); window.addEventListener("pointerup", dragUp); window.addEventListener("keydown", dragKey, true);
  }
  function endDrag() { clipDrag = null; window.removeEventListener("pointermove", dragMove); window.removeEventListener("pointerup", dragUp); window.removeEventListener("keydown", dragKey, true); }
  function dragKey(e: KeyboardEvent) { if (e.key === "Escape" && clipDrag) { e.preventDefault(); e.stopPropagation(); endDrag(); toast("Move cancelled"); } }
  function dragTarget(d: Drag, e: PointerEvent): Drag {
    let toId = d.toId;
    for (const el of document.querySelectorAll<HTMLElement>("[data-tid]")) {
      const r = el.getBoundingClientRect();
      if (e.clientY >= r.top && e.clientY <= r.bottom) { const t = project.tracks.find((x) => x.id === +el.dataset.tid!); if (t && !E.isLabelTrack(t)) toId = t.id; break; }
    }
    const from = project.tracks.find((x) => x.id === d.fromId)!, to = project.tracks.find((x) => x.id === toId)!;
    const rate = trackRate(to) ?? sources.get(from.clips[d.index].src)?.sampleRate ?? 48000;
    let at = Math.max(0, timeAtX(toId, e.clientX) - d.grab), snapTo: number | null = null;
    if (!e.altKey) {
      const pts = [0, ...(project.markers ?? []).map((m) => m.time), ...(selection ? [selection.a] : []),
        ...project.tracks.filter((x) => !E.isLabelTrack(x)).flatMap((x) => E.audioClips(x).filter((q) => !(x.id === d.fromId && q.index === d.index)).flatMap((q) => [q.a, q.b]))];
      const s = E.snapClip(at, d.len, pts, 8 / pps(toId)); at = s.t; snapTo = s.to;
    }
    at = E.snapToSamples(at, rate);
    const ok = E.moveClip(project, d.fromId, d.index, toId, at, rate).ok;
    return { ...d, toId, at, ok, snapTo, noSnap: e.altKey, x: e.clientX, y: e.clientY };
  }
  function dragMove(e: PointerEvent) {
    const d = clipDrag; if (!d) return;
    if (!d.moved && Math.hypot(e.clientX - d.x0, e.clientY - d.y0) < 4) return;
    clipDrag = dragTarget({ ...d, moved: true }, e);
  }
  function dragUp(e: PointerEvent) {
    const d0 = clipDrag; if (!d0) return;
    if (!d0.moved) { endDrag(); const t = project.tracks.find((x) => x.id === d0.fromId); const q = t && E.placed(t)[d0.index]; if (t && q) selection = { trackId: t.id, a: q.a, b: q.b }; return; }
    const d = dragTarget(d0, e); endDrag();
    const from = project.tracks.find((x) => x.id === d.fromId)!, to = project.tracks.find((x) => x.id === d.toId)!;
    const q = E.placed(from)[d.index];
    if (d.fromId === d.toId && q && Math.abs(q.a - d.at) < 1e-9) return; // dropped where it was
    const rate = trackRate(to) ?? sources.get(from.clips[d.index].src)?.sampleRate ?? 48000;
    const r = E.moveClip(hist.present, d.fromId, d.index, d.toId, d.at, rate);
    if (!r.ok) { toast(r.reason === "overlap" ? `Can't drop there: it would overlap audio on "${to.name}". Drop into silence or after the end.` : "Clips can only go onto audio tracks", "warn"); return; }
    commit(d.fromId === d.toId ? "Move clip" : `Move clip to ${to.name}`, r.project);
    selection = { trackId: r.trackId, a: r.at, b: r.at + d.len };
    if (layerFor) layerFor = null; // clip indices changed
    restartIfPlaying();
  }
  const fmtPrecise = (t: number) => { const m = Math.floor(t / 60); return `${m}:${(t % 60).toFixed(3).padStart(6, "0")}`; };

  // ---- Layering Effects (per-clip effect stack) ----
  let layerFor = $state<{ trackId: number; index: number } | null>(null);
  const layerClip = $derived.by(() => {
    if (!layerFor) return null;
    const t = project.tracks.find((x) => x.id === layerFor!.trackId); const q = t && E.placed(t)[layerFor.index];
    return t && q && !q.clip.gap ? { t, q } : null;
  });
  function openLayers(t: E.EditTrack, index: number) { const q = E.placed(t)[index]; if (!q || q.clip.gap) return; layerFor = { trackId: t.id, index }; selection = { trackId: t.id, a: q.a, b: q.b }; }
  function changeLayers(fx: FxSlot[], label: string) {
    if (!layerFor) return;
    commit(label, E.setClipFx(hist.present, layerFor.trackId, layerFor.index, fx)); liveDry();
  }
  function syncScroll(e: Event) {
    const x = (e.currentTarget as HTMLElement).scrollLeft;
    const now = performance.now();
    // our own follow scroll: every lane, the ruler and the preview box were already updated in that frame
    if (now < autoUntil && Math.abs(x - autoLeft) < 2) return;
    // anyone else moving the timeline while it plays (scrollbar, keys, touchpad) is the user: follow steps aside
    if ((level.playing || level.recording) && now - zoomAt > 400 && Math.abs(x - autoLeft) >= 2) follow.userScroll(now);
    for (const el of document.querySelectorAll<HTMLElement>(".tracks .scroll")) if (el !== e.currentTarget && el.scrollLeft !== x) el.scrollLeft = x;
    updateViewWin();
  }
  // ---- Follow playhead: keep the play/record head in view (one rAF loop while playing or recording) ----
  const followOn = $derived(settings.follow_playhead ?? true);
  const follow = new FollowState();
  const headClock = new HeadClock(), recClock = new HeadClock();
  let smoothHead = $state<number | null>(null), smoothRec = $state<number | null>(null);
  let followHeld = $state(false);
  let followAnchor = ANCHOR, autoLeft = -1, autoUntil = 0, zoomAt = 0, raf = 0, lastFrame = 0;
  $effect(() => { void zoom; zoomAt = performance.now(); });
  // warm the play cache for the track Space would play (after loads / edits settle; never while playing)
  let prerenderTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void project; const id = selection?.trackId;
    clearTimeout(prerenderTimer);
    prerenderTimer = setTimeout(() => {
      if (level.playing || level.recording) return;
      const t = project.tracks.find((x) => x.id === id && x.clips.length) ?? project.tracks.find((x) => x.clips.length);
      if (t && !E.isLabelTrack(t)) api.prerenderClips(clipsFor(t.clips), fxFor(t)).catch(() => {});
    }, 500);
  });
  function setFollow(on: boolean) {
    settings.follow_playhead = on; follow.play(); followAnchor = ANCHOR;
    api.setFollowPlayhead(on).catch(() => {});
    toast(on ? "Follow playhead: on" : "Follow playhead: off");
  }
  function onLevel(l: Level) {
    const now = performance.now();
    if (l.playing && now - localStopAt < 150) l = { ...l, playing: false, play_paused: false }; // a report sent before our Stop
    const started = (l.playing && !level.playing) || (l.playing && level.play_paused && !l.play_paused);
    level = l;
    if (started) { follow.play(); followAnchor = ANCHOR; }
    headClock.report(l.playing && playingId !== null ? playOffset + l.play_secs : 0, now, l.playing && !l.play_paused);
    recClock.report(l.rec_secs, now, l.recording && !l.paused);
    if ((l.playing || l.recording) && !raf) { lastFrame = now; raf = requestAnimationFrame(followFrame); }
  }
  function followFrame(now: number) {
    raf = 0;
    const playing = level.playing && playingId !== null;
    if (!playing && !level.recording) { smoothHead = smoothRec = null; followHeld = false; return; }
    raf = requestAnimationFrame(followFrame);
    const dt = (now - lastFrame) / 1000; lastFrame = now;
    // the displayed playhead (ruler + preview) moves every frame, not in 30 Hz steps
    if (playing) smoothHead = headClock.now(now); else smoothRec = recClock.now(now);
    if (!followOn) { followHeld = false; return; }
    const head = playing ? smoothHead! : smoothRec!;
    // reads first (the ruler is always zoom × 100 % wide), then writes only: no layout thrash
    const ref = document.querySelector<HTMLElement>(".tracks .rwrap .scroll");
    if (!ref || !timelineLen) return;
    const width = ref.clientWidth, max = ref.scrollWidth - width, left = ref.scrollLeft;
    if (max <= 0) { followHeld = false; return; }
    const px = (width * zoom) / timelineLen, headPx = Math.min(head, timelineLen) * px;
    const on = follow.active(now, (headPx - left) / width);
    if (followHeld !== !on) followHeld = !on;
    if (!on) return;
    if (follow.resumeAnchor !== null) { followAnchor = follow.resumeAnchor; follow.resumeAnchor = null; }
    const r = followStep(headPx, { left, width, max }, followAnchor, dt);
    followAnchor = r.anchor;
    if (Math.abs(r.left - left) < 0.5) return;
    autoLeft = r.left; autoUntil = now + 250;
    for (const el of document.querySelectorAll<HTMLElement>(".tracks .scroll")) el.scrollLeft = r.left;
    viewWin = { a: r.left / px, b: (r.left + width) / px };
  }

  // ---- the part of the timeline the lanes currently show (for the preview's viewport box) + zoom to selection ----
  let viewWin = $state<{ a: number; b: number } | null>(null);
  function updateViewWin() {
    const el = document.querySelector<HTMLElement>(".tracks .lane .scroll"); if (!el || !timelineLen) { viewWin = null; return; }
    const px = (el.clientWidth * zoom) / timelineLen; if (!px) return;
    viewWin = { a: el.scrollLeft / px, b: (el.scrollLeft + el.clientWidth) / px };
  }
  $effect(() => { void zoom; void timelineLen; void project.tracks.length; requestAnimationFrame(updateViewWin); });
  async function zoomTo(a: number, b: number) {
    if (!(b > a) || !timelineLen) return;
    setZoom((timelineLen / (b - a)) * 0.9); await tick();
    const el = document.querySelector<HTMLElement>(".tracks .lane .scroll"); if (!el) return;
    const px = (el.clientWidth * zoom) / timelineLen, x = Math.max(0, a * px - el.clientWidth * 0.05);
    for (const s of document.querySelectorAll<HTMLElement>(".tracks .scroll")) s.scrollLeft = x;
    updateViewWin();
  }
  const previewTrack = $derived.by(() => { const t = activeTrack(); return t && !E.isLabelTrack(t) && t.clips.some((c) => !c.gap) ? t : null; });

  // ---- label tracks ----
  let focusLabel = $state<number | null>(null);
  function newLabelTrack(): number {
    const id = nextId++; const n = project.tracks.filter(E.isLabelTrack).length;
    commit("New label track", { ...hist.present, tracks: [...hist.present.tracks, { id, name: n ? `Labels ${n + 1}` : "Labels", kind: "labels", clips: [], labels: [] }] });
    return id;
  }
  function addLabel() {
    const sel = selection ?? { trackId: -1, a: 0, b: 0 };
    const at = level.playing ? playOffset + level.play_secs : sel.a, end = level.playing ? at : sel.b;
    const target = project.tracks.find((t) => isSel(t.id) && E.isLabelTrack(t)) ?? project.tracks.find(E.isLabelTrack);
    const tid = target?.id ?? newLabelTrack();
    const l: E.Label = { id: nextId++, t0: at, t1: Math.max(at, end), text: "" };
    commit("Add label", E.setLabels(hist.present, tid, (ls) => [...ls, l]));
    focusLabel = l.id;
  }
  const allLabels = () => project.tracks.filter(E.isLabelTrack).flatMap((t) => t.labels ?? []).sort((a, b) => a.t0 - b.t0);
  async function exportLabels() {
    const t = project.tracks.find((x) => isSel(x.id) && E.isLabelTrack(x)) ?? project.tracks.find(E.isLabelTrack);
    const ls = t?.labels ?? [];
    if (!ls.length) { toast("No labels to export. Press Ctrl+B to add one.", "warn"); return; }
    const p = await save({ defaultPath: `${projName} labels.txt`, filters: [{ name: "Label text (Audacity format)", extensions: ["txt"] }] });
    if (!p) return;
    const path = /\.txt$/i.test(p) ? p : p + ".txt";
    try { await api.saveTextFile(path, E.labelsToText(ls)); toast(`Exported ${ls.length} label${ls.length === 1 ? "" : "s"}`); } catch (e) { toast(`Export failed: ${e}`, "err"); }
  }
  async function importLabels() {
    const p = await open({ filters: [{ name: "Label text", extensions: ["txt"] }] });
    if (!p || Array.isArray(p)) return;
    try {
      const ls = E.textToLabels(await api.readTextFile(p), () => nextId++);
      if (!ls.length) { toast("No labels found in that file", "warn"); return; }
      const id = nextId++, name = p.split(/[\\/]/).pop()!.replace(/\.txt$/i, "");
      commit("Import labels", { ...hist.present, tracks: [...hist.present.tracks, { id, name, kind: "labels", clips: [], labels: ls }] });
      toast(`Imported ${ls.length} label${ls.length === 1 ? "" : "s"}`);
    } catch (e) { toast(`Could not import labels: ${e}`, "err"); }
  }
  function labelsToMarkers() {
    const ls = allLabels(); if (!ls.length) { toast("No labels yet. Press Ctrl+B to add one.", "warn"); return; }
    const before = (hist.present.markers ?? []).length;
    commit("Labels to markers", E.labelsToMarkers(hist.present, ls, () => nextId++));
    toast(`Added ${(hist.present.markers ?? []).length - before} marker(s) from labels`);
  }

  // ---- spectrogram view (per track) ----
  let zoom = $state(1);
  let specs = $state.raw(new Map<number, { key: string; spec: Spec | null }>());
  const specCols = $derived(Math.min(8000, Math.round(1200 * zoom)));
  function setView(id: number, patch: Partial<E.EditTrack>, label = "Track view") {
    commit(label, { ...hist.present, tracks: hist.present.tracks.map((t) => (t.id === id ? { ...t, ...patch } : t)) });
  }
  function toggleView() {
    const ids = selIds().length ? selIds() : project.tracks[0] ? [project.tracks[0].id] : [];
    const ts = project.tracks.filter((t) => ids.includes(t.id) && !E.isLabelTrack(t)); if (!ts.length) return;
    const to = ts[0].view === "spectrogram" ? "wave" : "spectrogram";
    commit(to === "spectrogram" ? "Spectrogram view" : "Waveform view", { ...hist.present, tracks: hist.present.tracks.map((t) => (ts.includes(t) ? { ...t, view: to } : t)) });
  }
  $effect(() => {
    for (const t of project.tracks) {
      if (t.view !== "spectrogram" || !t.clips.length) continue;
      const cols = Math.max(50, Math.round(specCols * E.trackLen(t) / timelineLen));
      const key = JSON.stringify([t.clips, cols]);
      if (specs.get(t.id)?.key === key) continue;
      specs = new Map(specs).set(t.id, { key, spec: specs.get(t.id)?.spec ?? null });
      api.spectrogram(t.clips, cols).then((buf) => {
        if (specs.get(t.id)?.key !== key) return;
        specs = new Map(specs).set(t.id, { key, spec: parseSpec(buf) });
      }).catch((e) => toast(`Spectrogram failed: ${e}`, "err"));
    }
  });

  // ---- right-click menus ----
  let ctxMenu = $state<{ items: MenuItem[]; x: number; y: number; run: (id: string) => void } | null>(null);
  let renaming = $state<number | null>(null);
  let fxDialog = $state<{ fx: Effect; scope: string } | null>(null);
  let renderBusy = $state(false);
  const mediaDir = () => (projectPath ? projectPath.replace(/\.twt$/i, "") + " media" : null);
  /** Right-click outside the current selection first moves the cursor there (like a click). */
  function selectForContext(t: E.EditTrack, at: number) {
    const s = selection, inRange = !!s && isSel(t.id) && (hasRange() ? at >= s.a - 1e-3 && at <= s.b + 1e-3 : Math.abs(at - s.a) < 1e-3);
    if (!inRange) selection = { trackId: t.id, a: at, b: at };
    else if (selection && selection.trackId !== t.id) selection = { ...selection, trackId: t.id };
  }
  function openAudioMenu(t: E.EditTrack, at: number, e: MouseEvent) {
    selectForContext(t, at);
    const sel = selection!, ids = selIds();
    const next = E.splitClips(hist.present, sel);
    const canSplit = next.tracks.some((x, i) => x !== hist.present.tracks[i]);
    const canJoin = hasRange() && project.tracks.some((x) => ids.includes(x.id) && E.clipBounds(x).some((b) => b.split && b.t > sel.a + 1e-9 && b.t < sel.b - 1e-9));
    const items = audioMenu({ hasRange: hasRange(), hasClipboard: clipboard.length > 0, canJoin, canSplit, muted: !!t.muted, solo: !!t.solo,
      spectrogram: t.view === "spectrogram", tracks: ids.length, trackName: t.name, onClip: !!E.audioClipAt(t, at) });
    ctxMenu = { items, x: e.clientX, y: e.clientY, run: (id) => audioAction(t, at, id) };
  }
  function openTrackHeaderMenu(t: E.EditTrack, e: MouseEvent) {
    if (!isSel(t.id)) selection = { trackId: t.id, a: selection?.a ?? 0, b: selection?.b ?? 0 };
    if (E.isLabelTrack(t)) { openLabelMenu(t, selection?.a ?? 0, e, null); return; }
    const items = audioMenu({ hasRange: false, hasClipboard: false, canJoin: false, canSplit: false, muted: !!t.muted, solo: !!t.solo, spectrogram: t.view === "spectrogram", tracks: 1, trackName: t.name });
    const tr = items.find((m) => "id" in m && m.id === "track") as { submenu: MenuItem[] };
    ctxMenu = { items: tr.submenu, x: e.clientX, y: e.clientY, run: (id) => audioAction(t, selection?.a ?? 0, id) };
  }
  function openLabelMenu(t: E.EditTrack, at: number, e: MouseEvent, l: E.Label | null) {
    if (l) selection = { trackId: t.id, a: l.t0, b: l.t1 }; else selectForContext(t, at);
    ctxMenu = { items: labelMenu({ onLabel: !!l, hasRange: hasRange(), trackName: t.name }), x: e.clientX, y: e.clientY, run: async (id) => {
      if (id === "editlabel" && l) { focusLabel = null; await tick(); focusLabel = l.id; }
      else if (id === "selectlabel" && l) selection = { trackId: t.id, a: l.t0, b: l.t1 };
      else if (id === "deletelabel" && l) commit("Delete label", E.setLabels(hist.present, t.id, (ls) => ls.filter((x) => x.id !== l.id)));
      else if (id === "rename") renaming = t.id;
      else if (id === "deletetrack") removeTrack(t.id);
      else run(id as Action);
    } };
  }
  function openEmptyMenu(e: MouseEvent) {
    if ((e.target as Element).closest(".track")) return;
    e.preventDefault();
    ctxMenu = { items: emptyMenu({ hasClipboard: clipboard.length > 0 }), x: e.clientX, y: e.clientY, run: (id) => { if (id === "paste") selection = null; run(id as Action); } };
  }
  async function audioAction(t: E.EditTrack, at: number, id: string) {
    const sel = selection;
    if (id.startsWith("fx:")) { openFxDialog(id.slice(3) as EffectType, t); return; }
    switch (id) {
      case "marker": addMarker(undefined, sel?.a ?? at); break;
      case "selectclip": pickClip(t, at); break;
      case "layerfx": { const q = E.audioClipAt(t, at); if (q) openLayers(t, q.index); break; }
      case "trim": if (sel && hasRange()) { commit("Trim to selection", E.trimMany(hist.present, sel)); selection = { ...sel, a: 0, b: sel.b - sel.a }; } break;
      case "silence": await applyRendered(null, "Silence", true); break;
      case "fadein": case "fadeout": {
        ctxFade = id === "fadein" ? "in" : "out"; const r = fxRange(t); const ms = Math.round((r.b - r.a) * 1000);
        if (!hasRange()) selection = { trackId: t.id, a: r.a, b: r.b };
        await applyRendered({ type: "Fade", in_ms: id === "fadein" ? ms : 0, out_ms: id === "fadeout" ? ms : 0 }, id === "fadein" ? "Fade in" : "Fade out");
        break;
      }
      case "normalize": await applyRendered({ type: "Normalize", peak_db: -1 }, "Normalize"); break;
      case "studio": selection = { trackId: t.id, a: sel?.a ?? 0, b: sel?.b ?? 0 }; await studioSound("Medium"); break;
      case "rename": renaming = t.id; break;
      case "mute": toggleTrackFlag(t.id, "muted"); break;
      case "solo": toggleTrackFlag(t.id, "solo"); break;
      case "toggleview": setView(t.id, { view: t.view === "spectrogram" ? "wave" : "spectrogram" }, t.view === "spectrogram" ? "Waveform view" : "Spectrogram view"); break;
      case "deletetrack": removeTrack(t.id); break;
      default: await run(id as Action);
    }
  }
  /** Range used by fades without a selection: the first / last second of the track. */
  function fxRange(t: E.EditTrack) {
    if (selection && hasRange()) return { a: selection.a, b: selection.b };
    const len = E.trackLen(t), d = Math.min(1, len);
    return ctxFade === "out" ? { a: len - d, b: len } : { a: 0, b: d };
  }
  let ctxFade: "in" | "out" = "in";
  function scopeText() {
    const ids = selIds().filter((id) => !E.isLabelTrack(project.tracks.find((t) => t.id === id)!));
    const n = ids.length, tr = n > 1 ? ` on ${n} tracks` : "";
    return hasRange() ? `the selection ${fmtTime(selection!.a)}–${fmtTime(selection!.b)}${tr}` : `the whole track${n > 1 ? "s" : ""}${tr}`;
  }
  function openFxDialog(type: EffectType, t: E.EditTrack) {
    let fx = defaultFx(type);
    if (fx.type === "NoiseReduction") { const s = t.fx?.find((x) => x.fx.type === "NoiseReduction"); if (s && s.fx.type === "NoiseReduction") fx = { ...fx, profile: s.fx.profile }; }
    fxDialog = { fx, scope: scopeText() };
  }
  /**
   * Render the selection (or the whole track when there is no range) of every selected audio track
   * through one effect, or as silence, into new files, and swap them in as one undoable edit.
   */
  async function applyRendered(fx: Effect | null, label: string, silence = false) {
    const sel = selection; if (!sel || renderBusy) return false;
    const ts = project.tracks.filter((t) => isSel(t.id) && !E.isLabelTrack(t) && t.clips.length);
    if (!ts.length) { toast("Select some audio first", "warn"); return false; }
    renderBusy = true; toast(`${label}…`);
    try {
      let p = hist.present; const m = new Map(sources);
      for (const t of ts) {
        const [a, b] = hasRange() ? [sel.a, sel.b] : [0, E.trackLen(t)];
        const clips = E.extract(t, a, Math.min(b, E.trackLen(t))); if (!clips.length) continue;
        const path = await api.renderSelection(clips, fx ? [{ bypass: false, fx }] : [], silence, `${t.name} - ${label}`, mediaDir());
        const w = await api.importAudio(path);
        m.set(w.path, { path: w.path, name: w.name, duration: w.duration_secs, sampleRate: w.sample_rate, channels: w.channels, peaks: w.peaks });
        p = E.replaceRanges(p, a, Math.min(b, E.trackLen(t)), new Map([[t.id, [{ src: w.path, start: 0, end: w.duration_secs }]]]));
      }
      sources = m; commit(ts.length > 1 ? `${label} (${ts.length} tracks)` : label, p); restartIfPlaying();
      toast(`${label} applied${hasRange() ? " to the selection" : ""}. Ctrl+Z undoes it.`);
      return true;
    } catch (e) { toast(`${label} failed: ${e}`, "err"); return false; }
    finally { renderBusy = false; }
  }
  function renameTrack(id: number, name: string) {
    renaming = null; name = name.trim();
    const t = project.tracks.find((x) => x.id === id); if (!t || !name || name === t.name) return;
    commit("Rename track", { ...hist.present, tracks: hist.present.tracks.map((x) => (x.id === id ? { ...x, name } : x)) });
  }

  function removeTrack(id: number) { commit("Remove track", { ...hist.present, tracks: hist.present.tracks.filter((t) => t.id !== id) }); }

  // ---- edit actions ----
  type Action = "undo" | "redo" | "cut" | "copy" | "paste" | "delete" | "selectall" | "playpause" | "stop" | "record" | "import" | "export" | "zoomin" | "zoomout" | "save" | "saveas" | "open" | "new" | "savelast" | "pause" | "marker" | "prevmarker" | "nextmarker" | "sendvideo" | "about" | "checkupdates" | "importaup3" | "split" | "join" | "addlabel" | "labeltrack" | "labelsmarkers" | "importlabels" | "exportlabels" | "toggleview" | "close" | "follow";
  let perfOn = false;
  api.perfEnabled().then((v) => (perfOn = v)).catch(() => {});
  async function run(action: Action) {
    if (perfOn && (action === "playpause" || action === "stop")) api.perfMark(`${action} pressed`);
    const sel = selection;
    switch (action) {
      case "undo": { const l = hist.undoLabel; if (hist.undo()) { sync(); toast(`Undo ${l}`); restartIfPlaying(); } break; }
      case "redo": { const l = hist.redoLabel; if (hist.redo()) { sync(); toast(`Redo ${l}`); restartIfPlaying(); } break; }
      case "zoomin": setZoom(zoom * 1.5); break;
      case "zoomout": setZoom(zoom / 1.5); break;
      case "cut": if (sel && hasRange()) { const r = E.cutMany(hist.present, sel); clipboard = r.clipboard; commit(selIds().length > 1 ? `Cut (${selIds().length} tracks)` : "Cut", r.project); selection = { ...sel, b: sel.a }; } break;
      case "copy": if (sel && hasRange()) { clipboard = E.copyMany(hist.present, sel); toast(clipboard.length > 1 ? `Copied ${clipboard.length} tracks` : "Copied selection"); } break;
      case "delete": if (sel && hasRange()) { commit(selIds().length > 1 ? `Delete (${selIds().length} tracks)` : "Delete", E.delMany(hist.present, sel)); selection = { ...sel, b: sel.a }; } break;
      case "paste": {
        if (!clipboard.length) break;
        const len = Math.max(...clipboard.map((cs) => cs.reduce((s, c) => s + E.clipLen(c), 0)));
        const audioSel = sel && project.tracks.some((t) => isSel(t.id) && !E.isLabelTrack(t));
        if (!sel || !audioSel) { // no audio target: paste as new track(s)
          const ts = clipboard.map((cs) => ({ id: nextId++, name: "Pasted audio", kind: "import", clips: cs.map(E.cloneClip) }));
          commit("Paste", { ...hist.present, tracks: [...hist.present.tracks, ...ts] });
          selection = { trackId: ts[0].id, a: len, b: len };
        } else { commit("Paste", E.pasteMany(hist.present, sel, clipboard)); selection = { ...sel, a: sel.a + len, b: sel.a + len }; }
        break;
      }
      case "selectall": if (project.tracks.length) selection = { trackId: activeTrack()!.id, a: 0, b: timelineLen, tracks: project.tracks.map((t) => t.id) }; break;
      case "split": {
        if (!sel) { toast("Click where you want to split first", "warn"); break; }
        const next = E.splitClips(hist.present, sel);
        if (next.tracks.every((t, i) => t === hist.present.tracks[i])) { toast("Nothing to split here (that's already the edge of the clip)", "warn"); break; }
        commit("Split clip", next); break;
      }
      case "join": {
        if (!sel || !hasRange()) { toast("Select across the clips you want to join, then press Ctrl+J", "warn"); break; }
        const next = E.joinClips(hist.present, sel);
        if (next.tracks.every((t, i) => E.clipBounds(t).filter((b) => b.split).length === E.clipBounds(hist.present.tracks[i]).filter((b) => b.split).length)) { toast("No split clips in the selection to join", "warn"); break; }
        commit("Join clips", next); break;
      }
      case "addlabel": addLabel(); break;
      case "labeltrack": { const id = newLabelTrack(); selection = { trackId: id, a: sel?.a ?? 0, b: sel?.b ?? 0 }; break; }
      case "labelsmarkers": labelsToMarkers(); break;
      case "importlabels": await importLabels(); break;
      case "exportlabels": await exportLabels(); break;
      case "toggleview": toggleView(); break;
      case "playpause": {
        // Space: pause / resume the running playback in place
        if (level.playing) { await api.pausePlayback(!level.play_paused); break; }
        const t = project.tracks.find((x) => x.id === sel?.trackId && x.clips.length) ?? project.tracks.find((x) => isSel(x.id) && x.clips.length) ?? project.tracks.find((x) => x.clips.length); if (!t) break;
        const ranged = sel && sel.trackId === t.id && hasRange();
        const clips = ranged ? E.extract(t, sel!.a, sel!.b) : t.clips;
        const start = !ranged && sel && sel.trackId === t.id ? sel.a : 0;
        playOffset = ranged ? sel!.a : 0; playingId = t.id; playRange = ranged ? { a: sel!.a, b: sel!.b } : null;
        try { await api.playClips(clipsFor(clips), fxFor(t), start); } catch (e) { toast(String(e), "err"); }
        break;
      }
      case "stop": stopTransportUi(); await api.stopPlayback(); break;
      case "close": await closeProject(); break;
      case "follow": setFollow(!followOn); break;
      case "about": showAbout = true; break;
      case "checkupdates": showAbout = false; updateMode = "manual"; break;
      case "record": await toggleRecord(); break;
      case "import": await importFile(); break;
      case "importaup3": await importAudacity(); break;
      case "export": if (activeTrack()?.clips.length) showExport = true; else toast(activeTrack() && E.isLabelTrack(activeTrack()!) ? "Pick an audio track to export (labels: File → Export Labels…)" : "Nothing to export", "warn"); break;
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
    if (f.tracks.length) api.preparePlayback().catch(() => {});
  }

  // ---- replacing the project: stop the old audio first (New / Open / Close / Recover / double-clicked .twt) ----
  let takeLanded: (() => void) | null = null;
  /** Stop the running recording and wait until its take has been added to the current project. */
  function stopRecordingAndWait(): Promise<void> {
    return new Promise((res) => {
      const t = setTimeout(() => { takeLanded = null; res(); }, 10000);
      takeLanded = () => { clearTimeout(t); takeLanded = null; res(); };
      toggleRecord().catch(() => takeLanded?.());
    });
  }
  let localStopAt = 0;
  /** Stop playback now (engine fades out in a few ms) and reset the transport/playhead in the UI right away. */
  function stopTransportUi() {
    clearTimeout(restartTimer);
    playingId = null; playOffset = 0; playRange = null; smoothHead = null; localStopAt = performance.now();
    level = { ...level, playing: false, play_paused: false, play_secs: 0, play_total: 0 };
    headClock.report(0, performance.now(), false);
  }
  async function releaseAudio() {
    stopTransportUi();
    specs = new Map();
    await api.releaseProjectAudio();
  }
  const switchDeps = {
    recording: () => level.recording,
    confirm: (m: string) => ask(m, { title: "StreamWave", kind: "warning" }),
    stopRecording: stopRecordingAndWait,
    dirty: () => dirty,
    releaseAudio,
  };
  async function openProject(path?: string) {
    const pick = async () => { const p = path ?? (await open({ filters: [{ name: "StreamWave project", extensions: ["twt"] }] })); return !p || Array.isArray(p) ? null : p; };
    await switchProject(switchDeps, pick, async (p) => {
      try { await loadProjectJson(await api.projectOpen(p), p); toast(`Opened ${projName}`); }
      catch (e) { toast(`Could not open project: ${e}`, "err"); }
    });
    recentProjects = await api.recentProjects();
  }
  async function emptyProject() {
    hist.reset({ tracks: [] }); savedPresent = autosavedPresent = hist.present; projectPath = null; selection = null; sources = new Map(); sync(); markDirty();
    await api.autosaveClear();
  }
  async function newProject() { await switchProject(switchDeps, async () => true, emptyProject); }
  async function closeProject() {
    const had = project.tracks.length > 0 || !!projectPath;
    if (await switchProject(switchDeps, async () => true, emptyProject) && had) toast("Project closed");
  }
  async function recover(accept: boolean) {
    const json = recovery; recovery = null;
    if (accept && json) {
      await switchProject(switchDeps, async () => json, async (j) => {
        try { await loadProjectJson(j, null); toast("Recovered your unsaved work"); } catch (e) { toast(String(e), "err"); }
      });
    }
    else await api.autosaveClear();
  }

  // ---- Studio Sound ----
  let studioBusy = $state(false);
  let analysis = $state<Analysis | null>(null);
  async function studioSound(strength: "Subtle" | "Medium" | "Strong") {
    const t = activeTrack(); if (!t) return;
    if (!t.clips.length) { toast("Pick an audio track first", "warn"); return; }
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
    const tracks = project.tracks.filter((x) => x.video === video && !x.muted).map((x) => ({ clips: x.clips, fx: activeChain(x.fx) }));
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
  let vtState = $state<"off" | "armed" | "recording">("off");
  async function setVoiceTrigger(on: boolean) {
    const cur = settings.voice_trigger;
    const cfg = on ? { threshold_db: cur?.threshold_db ?? -45, hold_ms: cur?.hold_ms ?? 300 } : null;
    settings.voice_trigger = cfg;
    vtState = cfg ? "armed" : "off";
    try { await api.setVoiceTrigger(cfg); } catch (e) { toast(String(e), "err"); }
  }
  async function saveLast() {
    if (!settings.preroll_secs) { toast("The pre-record buffer is off. Set it in Settings.", "warn"); return; }
    await api.saveLast(settings.preroll_secs); toast(`Saving the last ${settings.preroll_secs}s...`);
  }
  async function doExport(path: string, o: ExportOptions) {
    const t = activeTrack(); if (!t) return;
    try { await api.exportTrack(t.clips, activeChain(t.fx), path, o); toast(`Exported ${path.split(/[\\/]/).pop()}`); }
    catch (e) { toast(`Export failed: ${e}`, "err"); }
  }

  // ---- batch export every track to WAV with a loudness target ----
  let batchLufs = $state(-16);
  let batchBusy = $state(false);
  async function batchExport() {
    if (!project.tracks.length || batchBusy) return;
    const dir = await open({ directory: true, title: "Choose a folder for the exported tracks" });
    if (!dir || Array.isArray(dir)) return;
    batchBusy = true;
    let done = 0, failed = 0;
    try {
      for (const t of project.tracks) {
        const fx = [...(t.fx ?? [])];
        const i = fx.findIndex((s) => s.fx.type === "Loudness");
        if (i >= 0) fx[i] = { ...fx[i], fx: { ...(fx[i].fx as Extract<Effect, { type: "Loudness" }>), target_lufs: batchLufs } };
        else fx.push({ id: nextId++, bypass: false, fx: { type: "Loudness", target_lufs: batchLufs } });
        const path = `${dir}/${t.name.replace(/[\\/:*?"<>|]/g, "_")}.wav`;
        try {
          await api.exportTrack(t.clips, activeChain(fx), path, { format: "wav", sample_rate: null, channels: null, bit_depth: 24,
            bitrate_kbps: null, quality: null, tags: { title: t.name, artist: "", album: "", date: String(new Date().getFullYear()), genre: "", comment: "Made with StreamWave" } });
          done++;
        } catch { failed++; toast(`Failed: ${t.name}`, "err"); }
      }
      toast(`Batch export: ${done}/${project.tracks.length} track(s) → ${dir}${failed ? ` (${failed} failed)` : ""}`);
    } finally { batchBusy = false; }
  }
  let ab = $state(false); // true = hear the original (effects bypassed)
  let lufs = $state<number | null>(null);
  const setZoom = (z: number) => (zoom = Math.max(1, Math.min(12, z)));
  const fxFor = (t: E.EditTrack) => (ab ? [] : activeChain(t.fx));
  /** A/B "original" also bypasses the clips' own layers. */
  const clipsFor = (cs: E.Clip[]) => (ab ? cs.map((c) => (c.fx ? { ...c, fx: [] } : c)) : cs);
  const mixTracks = () => project.tracks.filter((t) => t.clips.length).map((t) => ({ clips: clipsFor(t.clips), fx: fxFor(t) }));
  /** Range of the track being played (null = whole track), so a live re-render plays the same audio. */
  let playRange: { a: number; b: number } | null = null;
  /** Clip effects changed while playing: re-render off the audio thread and swap in at the current spot (no restart). */
  function liveDry() {
    if (!level.playing || playingId === null) return;
    const done = (ok: boolean) => { if (!ok) restartIfPlaying(); };
    if (playingId === -1) { api.updatePlayMix(mixTracks()).then(done).catch((e) => toast(String(e), "err")); return; }
    const t = project.tracks.find((x) => x.id === playingId); if (!t) return;
    const cs = playRange ? E.extract(t, playRange.a, playRange.b) : t.clips; if (!cs.length) return;
    api.updatePlayClips(clipsFor(cs)).then(done).catch((e) => toast(String(e), "err"));
  }

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
      playOffset = 0; playRange = null;
      try { await api.playClips(clipsFor(t.clips), fxFor(t), at); } catch (e) { toast(String(e), "err"); }
    }, 150);
  }
  function changeFx(fx: FxSlot[], label: string) {
    const t = activeTrack(); if (!t) return;
    commit(label, E.setFx(hist.present, t.id, fx)); lufs = null; liveFx();
  }
  // ---- workspaces: Settings (the classic right column) | Effects Layer. Pure view state; playback and project are untouched. ----
  let workspace = $state<"settings" | "fx">("settings");
  /** Clip under the edit cursor on the active track (its layers show in the Effects Layer workspace). */
  const cursorClip = $derived.by(() => {
    const t = activeTrack(); if (!t || E.isLabelTrack(t) || selection?.trackId !== t.id) return null;
    const at = selection.a, qs = E.audioClips(t);
    const q = qs.find((x) => at >= x.a && at < x.b) ?? (qs.length && Math.abs(at - qs[qs.length - 1].b) < 1e-6 ? qs[qs.length - 1] : undefined);
    return q ? { t, q } : null;
  });
  function changeCursorClipFx(fx: FxSlot[], label: string) {
    const c = cursorClip; if (!c) return;
    commit(label, E.setClipFx(hist.present, c.t.id, c.q.index, fx)); liveDry();
  }
  /** Knob being dragged on a track effect: push the draft chain into the running playback only (history on release). */
  function liveTrackDraft(chain: FxSlot[]) {
    const t = activeTrack(); if (!t || !level.playing || playingId !== t.id) return;
    api.updateFx(ab ? [] : activeChain(chain)).catch((e) => toast(String(e), "err"));
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

  // ---- per-clip gain & fades ----
  let selGain = $state(0);
  let selFadeIn = $state(0);
  let selFadeOut = $state(0);
  function onSelect(t: E.EditTrack, a: number, b: number) {
    selection = { trackId: t.id, a, b };
    // pre-fill the panel from the clip under the selection start
    let pos = 0;
    for (const c of t.clips) {
      if (a <= pos + E.clipLen(c) || c === t.clips[t.clips.length - 1]) {
        selGain = c.gain_db ?? 0; selFadeIn = c.fade_in_ms ?? 0; selFadeOut = c.fade_out_ms ?? 0;
        break;
      }
      pos += E.clipLen(c);
    }
  }
  function applySelProps() {
    const t = activeTrack();
    if (!t || !selection || selection.trackId !== t.id || !hasRange()) { toast("Select a range on the active track first", "warn"); return }
    commit("Clip gain/fades", E.applyClipProps(hist.present, selection, { gain_db: selGain, fade_in_ms: selFadeIn, fade_out_ms: selFadeOut }));
    sync(); markDirty(); restartIfPlaying();
  }

  // ---- Whisper transcription ----
  let whisper = $state<{ found: boolean; path: string | null; model_found: boolean; message: string } | null>(null);
  let segments = $state<{ start: number; end: number; text: string }[]>([]);
  let segmentsTrack = $state<number | null>(null);
  let transcribing = $state(false);
  async function transcribe() {
    const t = activeTrack(); if (!t) return;
    transcribing = true; segments = [];
    try {
      const first = t.clips.find((c) => !c.gap); if (!first) return;
      segments = await api.transcribe(first.src);
      segmentsTrack = t.id;
      if (!segments.length) toast("No speech recognized in the first source file of this track", "warn");
    } catch (e) { toast(`Transcription failed: ${e}`, "err"); }
    transcribing = false;
  }
  /** Timeline position being played (timeline seconds), or null when nothing plays. */
  const playhead = $derived(level.playing && playingId !== null ? smoothHead ?? playOffset + level.play_secs : null);
  /** While recording (and not playing) the ruler shows where the take is; it lands at 0 like the markers. */
  const recHead = $derived(level.recording && playhead === null ? smoothRec : null);
  /** Ruler click/drag: move the edit cursor; when a whole track is playing, continue playback from there on release. */
  async function rulerSeek(at: number, final: boolean) {
    const t = activeTrack(); if (!t) return;
    selection = { trackId: t.id, a: at, b: at };
    if (!final || !level.playing || playingId === null || playingId < 0 || playRange) return;
    const pt = project.tracks.find((x) => x.id === playingId); if (!pt || !pt.clips.length) return;
    await api.stopPlayback(); playingId = pt.id; playOffset = 0; playRange = null;
    try { await api.playClips(clipsFor(pt.clips), fxFor(pt), Math.min(at, E.trackLen(pt))); } catch (e) { toast(String(e), "err"); }
  }
  async function seekSeg(s: { start: number }) {
    const t = activeTrack(); if (!t || !t.clips.length) return;
    if (level.playing) await api.stopPlayback();
    playingId = t.id; playOffset = 0; playRange = null;
    try { await api.playClips(clipsFor(t.clips), fxFor(t), s.start); } catch (e) { toast(String(e), "err"); }
  }
  async function pickWhisperModel() {
    const p = await open({ filters: [{ name: "Whisper model", extensions: ["bin"] }] });
    if (!p || Array.isArray(p)) return;
    settings.whisper_model = p; await api.saveSettings(settings); whisper = await api.whisperStatus();
  }

  // ---- silence auto-trim ----
  let pendingSilences = $state<{ trackId: number; regions: [number, number][] } | null>(null);
  async function findSilences() {
    const t = activeTrack(); if (!t) return;
    pendingSilences = null;
    try {
      const regions = (await api.detectSilence(t.clips, null, 250, 80)).map((r) => [r.start, r.end] as [number, number]);
      if (!regions.length) { toast("No silences found (nothing quieter than the floor + 8 dB for 250 ms)", "warn"); return }
      pendingSilences = { trackId: t.id, regions };
      toast(`Found ${regions.length} silence${regions.length > 1 ? "s" : ""} (${fmtTime(regions.reduce((s, r) => s + r[1] - r[0], 0))} total). Click "Trim silences" to cut them.`);
    } catch (e) { toast(`Silence scan failed: ${e}`, "err"); }
  }
  function trimSilences() {
    const t = activeTrack(); if (!t || !pendingSilences || pendingSilences.trackId !== t.id) return;
    const n = pendingSilences.regions.length;
    commit(`Trim ${n} silence${n > 1 ? "s" : ""}`, E.removeRegions(hist.present, t.id, pendingSilences.regions));
    selection = null; pendingSilences = null; sync(); markDirty(); restartIfPlaying();
    toast("Silences trimmed (undo with Ctrl+Z)");
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
      if (k === "g" && e.shiftKey) return "toggleview";
      if (e.shiftKey) return null;
      return ({ y: "redo", x: "cut", c: "copy", v: "paste", a: "selectall", i: "import", e: "export", o: "open", n: "new", w: "close", k: "split", j: "join", b: "addlabel" } as Record<string, Action>)[k] ?? null;
    }
    if (e.altKey) return null;
    if (e.key === "Delete" || e.key === "Backspace") return "delete";
    if (e.code === "Space") return "playpause";
    if (e.key === "Escape" && level.playing) return "stop";
    if (e.code === "KeyR" && !e.repeat) return "record";
    if (e.code === "KeyM" && !e.repeat) return "marker";
    if (e.code === "KeyF" && !e.repeat && !e.shiftKey) return "follow";
    if (e.key === "[") return "prevmarker";
    if (e.key === "]") return "nextmarker";
    if (e.key === "+" || e.key === "=") return "zoomin";
    if (e.key === "-" || e.key === "_") return "zoomout";
    return null;
  }
  function onKey(e: KeyboardEvent) {
    // reload / print / find / page zoom etc. make no sense in an app window (app hotkeys still run below)
    if (isBrowserShortcut(e, import.meta.env.DEV)) e.preventDefault();
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
    const sel = await open({ multiple: true, filters: [{ name: "Audio / Video", extensions: ["wav", "mp3", "flac", "ogg", "m4a", "aac", "mp4", "mkv", "aiff", "caf"] }, { name: "Audacity project", extensions: ["aup3"] }] });
    const picked = Array.isArray(sel) ? sel : sel ? [sel] : [];
    for (const p of picked.filter(isAup3)) await importAudacity(p);
    await importMedia(picked.filter((p) => !isAup3(p)));
  }
  async function togglePlay(t: E.EditTrack) {
    if (playingId === t.id && level.playing) { await api.pausePlayback(!level.play_paused); return; }
    if (selection?.trackId !== t.id) selection = { trackId: t.id, a: 0, b: 0 };
    if (level.playing) { await api.stopPlayback(); playingId = null; }
    const sel = selection; const ranged = sel && sel.trackId === t.id && hasRange();
    const clips = ranged ? E.extract(t, sel!.a, sel!.b) : t.clips; if (!clips.length) return;
    playOffset = ranged ? sel!.a : 0; playingId = t.id; playRange = ranged ? { a: sel!.a, b: sel!.b } : null;
    try { await api.playClips(clipsFor(clips), fxFor(t), ranged ? 0 : sel?.a ?? 0); } catch (e) { toast(String(e), "err"); }
  }
  /** Render all tracks with their effects and play the full mix. */
  async function playMix() {
    const tracks = mixTracks();
    if (!tracks.length) return;
    if (level.playing) { await api.stopPlayback(); }
    playingId = -1; playOffset = 0;
    try { await api.playMix(tracks); } catch (e) { toast(`Mix playback failed: ${e}`, "err"); playingId = null; }
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
      vtState = settings.voice_trigger ? "armed" : "off";
      whisper = await api.whisperStatus();
      const a = await api.autosaveCheck();
      if (a) { try { if (deserialize(a).tracks.length) recovery = a; else api.autosaveClear(); } catch { api.autosaveClear(); } }
      // quiet update check, at most once a day, a few seconds after startup
      if (settings.onboarded !== false && shouldAutoCheck(settings.update_auto_check ?? true, settings.update_last_check, Date.now()))
        setTimeout(() => { if (!updateMode) updateMode = "quiet"; }, 8000);
    })();
    const autosave = setInterval(async () => {
      if (hist.present === autosavedPresent || !hist.present.tracks.length) return;
      try { await api.autosaveWrite(serialize(hist.present, sources, nextId)); autosavedPresent = hist.present; } catch {}
    }, 30000);
    const beforeUnload = (e: BeforeUnloadEvent) => { if (dirty) { e.preventDefault(); } };
    window.addEventListener("beforeunload", beforeUnload);
    const uns = [
      listen<Level>("level", (e) => onLevel(e.payload)),
      listen<EngineStatus>("engine-status", (e) => { engine = e.payload; if (e.payload.note) toast(e.payload.note, "warn"); }),
      listen<string>("record-finished", async (e) => { toast("Recording saved"); await addTrack(e.payload, "mic"); takeLanded?.(); }),
      listen<string>("record-part", (e) => toast(`Long take: saved part ${e.payload.split(/[\\/]/).pop()}`)),
      listen<string>("record-error", (e) => { toast(`Recording error: ${e.payload}`, "err"); takeLanded?.(); }),
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
      listen<string>("voice-trigger", (e) => { vtState = e.payload as typeof vtState; toast(e.payload === "recording" ? "Voice trigger: recording started" : "Voice trigger: armed", "warn"); }),
    ];
    uns.push(listen<string>("menu", (e) => onMenu(e.payload as Action)));
    uns.push(listen<string>("open-project-path", (e) => openProject(e.payload)));
    uns.push(listen<MicReport>("mic-check", (e) => onMic(e.payload)));
    uns.push(listen<string>("obs-scene", (e) => { if (level.recording) addMarker(`Scene: ${e.payload}`, level.rec_secs); }));
    uns.push(getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") dropHover = true;
      else if (e.payload.type === "leave") dropHover = false;
      else if (e.payload.type === "drop") { dropHover = false; openDropped(e.payload.paths); }
    }).catch(() => () => {}));
    window.addEventListener("keydown", onKey);
    // no browser context menu ("Save image as…") except in text boxes, where copy/paste of text is useful
    const noCtx = (e: MouseEvent) => { if (!isTextField(e.target as Element)) e.preventDefault(); };
    const noZoom = (e: WheelEvent) => { if (e.ctrlKey) e.preventDefault(); };
    window.addEventListener("contextmenu", noCtx);
    window.addEventListener("wheel", noZoom, { passive: false });
    const followUp = () => follow.pointerUp(performance.now());
    window.addEventListener("pointerup", followUp); window.addEventListener("pointercancel", followUp);
    uns.push(Promise.resolve(() => { window.removeEventListener("contextmenu", noCtx); window.removeEventListener("wheel", noZoom); window.removeEventListener("pointerup", followUp); window.removeEventListener("pointercancel", followUp); cancelAnimationFrame(raf); }));
    return () => { uns.forEach((u) => u.then((f) => f())); window.removeEventListener("keydown", onKey); clearInterval(autosave); window.removeEventListener("beforeunload", beforeUnload); };
  });
</script>

{#if ctxMenu}<ContextMenu items={ctxMenu.items} x={ctxMenu.x} y={ctxMenu.y} onpick={(id) => ctxMenu?.run(id)} onclose={() => (ctxMenu = null)} />{/if}
{#if fxDialog}
  <EffectDialog fx={fxDialog.fx} scope={fxDialog.scope} busy={renderBusy}
    onapply={async (fx) => { if (await applyRendered(fx, FX_NAMES[fx.type])) fxDialog = null; }}
    onaddchain={(fx) => { const t = activeTrack(); if (t) { changeFx([...(t.fx ?? []), slot(fx)], `Add ${FX_NAMES[fx.type]}`); toast(`${FX_NAMES[fx.type]} added to ${t.name}'s effects`); } fxDialog = null; }}
    onclose={() => (fxDialog = null)} />
{/if}
{#if importAsk}<ImportArrangeDialog files={importAsk.files} playhead={importAsk.playhead} onimport={importArranged} oncancel={() => { importAsk = null; toast("Import cancelled"); }} />{/if}
  {#if xfMenu && xfClip}
    {@const xm = xfMenu}
    <!-- closes on pointerdown (not click): the click that follows the handle's pointerup must not close it again -->
    <div class="xfveil" role="presentation" onpointerdown={() => (xfMenu = null)} oncontextmenu={(e) => { e.preventDefault(); xfMenu = null; }}></div>
    <div class="xfpop hw" role="dialog" aria-label="Crossfade" tabindex="-1" style="left:{Math.min(xm.x, innerWidth - 300)}px;top:{Math.min(xm.y + 12, innerHeight - 260)}px"
      onkeydown={(e) => { if (e.key === "Escape") { e.stopPropagation(); xfMenu = null; } }}>
      <h3>Crossfade</h3>
      <div class="xfrow">
        <Knob label="Length" unit="ms" min={0} max={xfClip.max} step={10} value={xfClip.ms} def={Math.min(300, xfClip.max)} size={64} scale="ends"
          onchange={(v) => laneXfade(xm.trackId, xm.index, v)} />
        <div class="xfside">
          <Dropdown caption="Curve" value={xfClip.c.xfade_curve ?? "equal_power"} onchange={(e) => laneXfade(xm.trackId, xm.index, xfClip.ms || Math.min(300, xfClip.max), e.currentTarget.value as E.XfadeCurve)}>
            <option value="equal_power" title="Best between different material: keeps the loudness steady">Equal power</option><option value="linear" title="Best between takes of the same material or room tone">Linear</option>
          </Dropdown>
          <div class="small">{xfClip.handles >= (xfClip.ms || 1) ? "Uses spare audio around the cut: nothing moves." : `Spare audio for ${xfClip.handles} ms; longer crossfades overlap the clips and move later audio earlier.`}</div>
          <button class="pill small" disabled={!xfClip.ms} onclick={() => { laneXfade(xm.trackId, xm.index, 0); xfMenu = null; }}>Remove crossfade</button>
        </div>
      </div>
    </div>
  {/if}
{#if layerClip}
  <LayerFxPanel clip={layerClip.q.clip} clipName={clipName(layerClip.q.clip)} trackName={layerClip.t.name} at={layerClip.q.a} onchange={changeLayers} onclose={() => (layerFor = null)} />
{/if}
{#if clipDrag?.moved}
  {@const tn = project.tracks.find((x) => x.id === clipDrag!.toId)?.name ?? ""}
  <div class="dragread" class:bad={!clipDrag.ok} style="left:{clipDrag.x + 14}px;top:{clipDrag.y + 16}px">
    <b>{fmtPrecise(clipDrag.at)}</b> → {tn}{clipDrag.ok ? "" : " · overlaps audio"}{clipDrag.snapTo !== null ? " · snapped" : clipDrag.noSnap ? " · no snap (Alt)" : ""}
  </div>
{/if}
{#if dropHover}<div class="drophint">Drop audio, video, an Audacity project (.aup3) or a StreamWave project (.twt)</div>{/if}
{#if aupBusy}<div class="aupbusy panel">Importing Audacity project "{aupBusy.name}"… {Math.round(aupBusy.frac * 100)}%<div class="bar"><div style="width:{aupBusy.frac * 100}%"></div></div><div class="small">Your .aup3 file is only read, never changed.</div></div>{/if}
{#if importReport}
  <div class="modal-back" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) importReport = null; }}>
    <div class="modal panel" role="dialog" aria-modal="true" aria-label="Audacity import summary">
      <h3>{importReport.title}</h3>
      {#each importReport.text.split("\n") as line, i}<p class={i ? "small" : ""}>{line}</p>{/each}
      <p class="small">Audio was copied to: {importReport.dir}<br />Your .aup3 file was not changed.</p>
      <div class="row2"><button onclick={() => (importReport = null)}>OK</button></div>
    </div>
  </div>
{/if}
{#if showAbout}<About onclose={() => (showAbout = false)} onupdates={() => { showAbout = false; updateMode = "manual"; }} />{/if}
{#if updateMode}
  <UpdateDialog quiet={updateMode === "quiet"} skipped={settings.update_skip_version ?? null} {dirty}
    busy={level.recording ? "A recording is in progress. Stop recording before installing the update." : null}
    onchecked={() => patchSettings({ update_last_check: Date.now() })}
    onskip={(v) => patchSettings({ update_skip_version: v })}
    onclose={() => (updateMode = null)} />
{/if}
{#if settings && settings.onboarded === false}
  <Onboarding {settings} {devices} {level} onsetinput={setInput} ondone={async () => { settings.onboarded = true; await api.saveSettings(settings); }} />
{/if}
<div class="app">
  <header>
    <div class="brand"><img class="logo" src={logoUrl} alt="" width="22" height="22" /> Stream<b>Wave</b> <span class="proj">· {projName}{dirty ? " •" : ""}</span></div>
    <div class="wsw" role="tablist" aria-label="Workspace">
      <button role="tab" aria-selected={workspace === "settings"} onclick={() => (workspace = "settings")} title="Settings panel: selection, Studio Sound, transcript, effects rack, OBS">Settings</button>
      <button role="tab" aria-selected={workspace === "fx"} onclick={() => (workspace = "fx")} title="Effects Layer workspace: every effect of the track and the clip under the cursor, with knobs">Effects Layer</button>
    </div>
    <select class="recent" value="" onchange={(e) => { const v = e.currentTarget.value; e.currentTarget.value = ""; e.currentTarget.blur(); if (v === "__open") openProject(); else if (v === "__new") newProject(); else if (v) openProject(v); }} title="Projects">
      <option value="">📁 Project</option><option value="__new">New project (Ctrl+N)</option><option value="__open">Open... (Ctrl+O)</option>
      {#each recentProjects as r}<option value={r}>{r.split(/[\\/]/).pop()}</option>{/each}
    </select>
    <button onclick={() => saveProject(false)} title="Save (Ctrl+S)">💾</button>
    <div class="devsel">
      <Dropdown caption="Input device" value={engine?.input_id ?? settings.input_device ?? ""} onchange={(e) => { setInput(e.currentTarget.value); e.currentTarget.blur(); }} title="Microphone">
        {#each inputs as d}<option value={d.id}>🎙 {d.friendly_name}{d.is_default ? " (default)" : ""}</option>{/each}
      </Dropdown>
    </div>
    <button onclick={() => run("undo")} disabled={!canUndo} title="Undo (Ctrl+Z)">↶</button>
    <button onclick={() => run("redo")} disabled={!canRedo} title="Redo (Ctrl+Y)">↷</button>
    <button onclick={importFile}>＋ Import</button>
    <button onclick={() => run("export")} disabled={!project.tracks.length} title="Export (Ctrl+E)">⇩ Export</button>
    <button onclick={batchExport} disabled={!project.tracks.length || batchBusy} title="Export every track as WAV at the loudness target set in Settings">⇩⇩ Batch</button>
    <button onclick={() => { if (workspace !== "settings") { workspace = "settings"; showSettings = true; } else showSettings = !showSettings; }}>⚙ Settings</button>
  </header>

  <main class:fxws={workspace === "fx"}>
    <section class="left">
      <div class="recpanel">
        <button class="bigrec" class:on={recording} onclick={toggleRecord} title="Record (R)">
          <span class="ico">{recording ? "■" : "●"}</span>
        </button>
        <div class="recinfo">
          <div class="time">{fmtTime(level.rec_secs)}{#if level.paused} <span class="paused">PAUSED</span>{/if}</div>
          <div class="hint">{recording ? "Recording… press R or click to stop" : "Press R or click to record"}{vtState !== "off" ? ` · 🎙 voice trigger ${vtState === "recording" ? "REC" : "armed"}` : ""}</div>
          <Meter left={level.left} right={level.right} status={micStatus} />
          <div class="armrow">
            <label class="chk"><input type="checkbox" bind:checked={armed} /> Armed: mic check{settings.preroll_secs ? ` + ${settings.preroll_secs}s pre-record buffer` : ""}</label>
            <button class="small" onclick={saveLast} disabled={!settings.preroll_secs} title="Ctrl+Shift+R">⟲ Save last {settings.preroll_secs}s</button>
          </div>
          {#if engine}<div class="small">{engine.sample_rate} Hz · {engine.channels} ch · {engine.buffer_frames || "driver"} frames · {settings.latency}{level.xruns ? ` · ${level.xruns} dropouts` : ""}</div>{/if}
        </div>
      </div>

      {#if previewTrack}
        {@const pt = previewTrack}
        {@const psel = selection?.trackId === pt.id ? selection : null}
        <Preview name={pt.name} peaks={E.trackPeaks(pt, sources, 900)} duration={E.trackLen(pt)}
          selection={psel ? { a: psel.a, b: psel.b } : null} cursor={psel?.a ?? null}
          playhead={playingId === pt.id || playingId === -1 ? playhead : null} view={viewWin} zoomed={zoom > 1.001}
          color={pt.kind === "obs" ? "#9b74ff" : pt.kind === "recovered" ? "#ffd04a" : "#2ec4b6"}
          onseek={(at) => { selection = { trackId: pt.id, a: at, b: at }; rulerSeek(at, true); }}
          onselect={(a, b) => (selection = { trackId: pt.id, a, b })}
          onzoomsel={() => { if (psel) zoomTo(psel.a, psel.b); }} onfit={() => setZoom(1)} />
      {/if}

      <div class="tracks" role="region" aria-label="Tracks" oncontextmenu={openEmptyMenu}
        onpointerdowncapture={(e) => { if (e.button === 0 && !(e.target as Element).closest?.("button, .mixbar")) follow.pointerDown(); }}
        onwheel={(e) => { if (e.ctrlKey) { e.preventDefault(); setZoom(zoom * (e.deltaY < 0 ? 1.25 : 0.8)); } else if (e.deltaX || e.shiftKey) follow.userScroll(performance.now()); }}>
        {#if project.tracks.length}
          <div class="mixbar">
            <button class="small" onclick={playMix} title="Render every track with its effects and play the combined mix">▶ Mix all {project.tracks.length} track{project.tracks.length > 1 ? "s" : ""}</button>
            {#if playingId === -1 && level.playing}<button class="small" onclick={() => run("stop")}>■ Stop mix</button>{/if}
            <span class="follow" class:held={followOn && followHeld}>
              {#if followOn && followHeld}<span class="small fhold">Follow paused · Play or scroll back to the playhead</span>{/if}
              <LedToggle checked={followOn} label="Follow" title="Follow playhead (F): keep the playhead in view while playing or recording" onchange={setFollow} />
            </span>
          </div>
        {/if}
        {#if project.tracks.length}
          <Ruler duration={timelineLen} {zoom} playhead={playhead ?? recHead} cursor={selection?.a ?? null}
            selection={selection && selection.b - selection.a > 0.0005 ? selection : null} markers={project.markers ?? []}
            onscroll={syncScroll} onseek={rulerSeek} />
        {/if}
        {#if project.tracks.length === 0}
          <div class="empty">No tracks yet. Hit the big red button, import a file, or record in OBS.</div>
        {/if}
        {#each project.tracks as t (t.id)}
          {@const len = E.trackLen(t)}
          {@const sel = isSel(t.id) ? selection : null}
          {@const lab = E.isLabelTrack(t)}
          {@const span = lab ? Math.max(len, timelineLen) : Math.max(len, timelineLen * 0.02)}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="track" class:act={selection?.trackId === t.id} class:multi={isSel(t.id) && selIds().length > 1} class:droptarget={clipDrag?.moved && clipDrag.toId === t.id} class:dropbad={clipDrag?.moved && clipDrag.toId === t.id && !clipDrag.ok} data-tid={t.id}>
            <div class="thead" role="toolbar" tabindex="-1" aria-label="{t.name} track" oncontextmenu={(e) => { if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return; e.preventDefault(); e.stopPropagation(); openTrackHeaderMenu(t, e); }}>
              {#if !lab}<button class="play" onclick={() => togglePlay(t)}>{playingId === t.id && level.playing && !level.play_paused ? "❚❚" : "▶"}</button>{/if}
              {#if playingId === t.id && level.playing}<button class="play" title="Stop" onclick={() => run("stop")}>■</button>{/if}
              {#if renaming === t.id}
                <!-- svelte-ignore a11y_autofocus -->
                <input class="rename" value={t.name} autofocus aria-label="Track name" onfocus={(e) => e.currentTarget.select()}
                  onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") renameTrack(t.id, e.currentTarget.value); if (e.key === "Escape") renaming = null; }}
                  onblur={(e) => renameTrack(t.id, e.currentTarget.value)} />
              {:else}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div class="tname" title={t.clips.filter((c) => !c.gap).map((c) => c.src).join("\n")} ondblclick={() => (renaming = t.id)}>{t.name}</div>
              {/if}
              <span class="tag {t.kind}">{t.kind}</span>
              {#if t.muted}<button class="tag flag" title="Muted in Audacity (left out of Send Back to Video). Click to unmute." onclick={() => toggleTrackFlag(t.id, "muted")}>muted</button>{/if}
              {#if t.solo}<button class="tag flag" title="Solo in Audacity. Click to clear." onclick={() => toggleTrackFlag(t.id, "solo")}>solo</button>{/if}
              {#if lab}
                <span class="small">{(t.labels ?? []).length} label{(t.labels ?? []).length === 1 ? "" : "s"}</span>
                <button class="small vbtn" title="Add a label at the cursor or selection (Ctrl+B)" onclick={() => { if (!isSel(t.id)) selection = { trackId: t.id, a: selection?.a ?? 0, b: selection?.b ?? 0 }; addLabel(); }}>＋ Label</button>
                <button class="small vbtn" title="Save these labels as a text file (Audacity format)" onclick={() => { selection = { trackId: t.id, a: selection?.a ?? 0, b: selection?.b ?? 0 }; exportLabels(); }}>Export…</button>
                <button class="small vbtn" title="Copy all labels to project markers (chapters in exports and Send Back to Video)" onclick={labelsToMarkers}>→ Markers</button>
              {:else}
                <span class="small">{fmtTime(len)}{t.clips.length > 1 ? ` · ${t.clips.length} clips` : ""}{t.fx?.some((f) => !f.bypass) ? ` · ${t.fx.filter((f) => !f.bypass).length} fx` : ""}
                  {#if sel && sel.b - sel.a > 0.0005} · sel {fmtTime(sel.a)}–{fmtTime(sel.b)}{/if}</span>
                {#if sel}
                  <button class="small" onclick={findSilences} title="Scan the track for long silences/pauses">✂ Silences</button>
                {/if}
                {#if pendingSilences && pendingSilences.trackId === t.id}
                  <button class="small trimbtn" onclick={trimSilences}>Trim {pendingSilences.regions.length} silence{pendingSilences.regions.length > 1 ? "s" : ""}</button>
                {/if}
                <button class="small vbtn" class:on={t.view === "spectrogram"} title="Waveform / spectrogram (Ctrl+Shift+G)" onclick={() => setView(t.id, { view: t.view === "spectrogram" ? "wave" : "spectrogram" }, t.view === "spectrogram" ? "Waveform view" : "Spectrogram view")}>{t.view === "spectrogram" ? "▤ Spectrogram" : "〰 Waveform"}</button>
                {#if t.view === "spectrogram"}
                  <select class="small vsel" title="Frequency scale" value={t.specScale ?? "log"} onchange={(e) => setView(t.id, { specScale: (e.target as HTMLSelectElement).value as E.SpecScale }, "Spectrogram scale")}>
                    <option value="log">Log</option><option value="linear">Linear</option></select>
                  <select class="small vsel" title="Colours" value={t.specColors ?? "classic"} onchange={(e) => setView(t.id, { specColors: (e.target as HTMLSelectElement).value as E.SpecColors }, "Spectrogram colours")}>
                    <option value="classic">Classic</option><option value="inferno">Inferno</option><option value="grey">Greyscale</option></select>
                {/if}
              {/if}
              <button class="x" onclick={() => removeTrack(t.id)} title="Remove from project (file is kept)">✕</button>
            </div>
            <div class="lane">
            {#if clipDrag?.moved && clipDrag.toId === t.id}
              {@const px = pps(t.id)}
              {@const sl = scrollEl(t.id)?.scrollLeft ?? 0}
              <div class="ghost" class:bad={!clipDrag.ok} style="left:{clipDrag.at * px - sl}px;width:{Math.max(3, clipDrag.len * px)}px"><span>{clipDrag.name}</span></div>
              {#if clipDrag.snapTo !== null}<div class="snapline" style="left:{clipDrag.snapTo * px - sl}px"></div>{/if}
            {/if}
            <div class="scroll" onscroll={syncScroll}><div style="width:{zoom * 100 * span / timelineLen}%">
            {#if lab}
              <LabelLane labels={t.labels ?? []} duration={span} active={selection?.trackId === t.id} selection={sel} focusId={focusLabel}
                onselect={(a, b, e, ph) => onSel(t, a, b, e, ph)}
                onedit={(id, text) => commit("Edit label", E.setLabels(hist.present, t.id, (ls) => ls.map((l) => (l.id === id ? { ...l, text } : l))))}
                ondelete={(id) => commit("Delete label", E.setLabels(hist.present, t.id, (ls) => ls.filter((l) => l.id !== id)))}
                onpick={(l) => (selection = { trackId: t.id, a: l.t0, b: l.t1 })}
                oncontext={(at, e, l) => openLabelMenu(t, at, e, l)} />
            {:else}
            <WaveCanvas peaks={E.trackPeaks(t, sources, Math.round(2000 * zoom))} duration={len} active={selection?.trackId === t.id}
              selection={sel} progress={progressFor(t)} markers={project.markers ?? []} bounds={E.clipBounds(t)}
              spec={t.view === "spectrogram" ? specs.get(t.id)?.spec ?? null : null} specScale={t.specScale ?? "log"} specColors={t.specColors ?? "classic"}
              onselect={(a, b, e, ph) => onSel(t, a, b, e, ph)} onpickclip={(at) => pickClip(t, at)} oncontext={(at, e) => openAudioMenu(t, at, e)}
              clips={laneClips(t)} movingIndex={clipDrag?.moved && clipDrag.fromId === t.id ? clipDrag.index : null}
              ongrab={(i, e, at) => grabClip(t, i, e, at)} onlayers={(i) => openLayers(t, i)}
              onfade={(i, edge, ms) => laneFade(t, i, edge, ms)} onxfade={(i, ms) => laneXfade(t.id, i, ms)}
              onxfademenu={(i, e) => { xfMenu = { trackId: t.id, index: i, x: e.clientX, y: e.clientY }; tick().then(() => document.querySelector<HTMLElement>(".xfpop [role=slider]")?.focus()); }}
              color={t.kind === "obs" ? "#9b74ff" : t.kind === "recovered" ? "#ffd04a" : "#2ec4b6"} />
            {/if}
            </div></div></div>
          </div>
        {/each}
        {#if project.tracks.length}<div class="small hint2">Drag to select (drag up/down for more tracks, Ctrl-click adds a track) · drag a clip's title bar to move it (Alt = no snapping, Esc cancels) · ◧ = Layering Effects · Shift-click to extend · Ctrl+X/C/V · Del · Ctrl+A · Ctrl+K split · Ctrl+J join · double-click a clip · Ctrl+B label · Ctrl+Z / Ctrl+Y · Space play/pause · F follow playhead · M marker · [ ] jump · Ctrl+scroll or +/− zoom ({zoom.toFixed(1)}×) — edits never change your original files</div>{/if}
      </div>
    </section>

    {#if workspace === "fx"}
      <aside class="fxaside">
        <EffectsWorkspace trackName={activeTrack() && !E.isLabelTrack(activeTrack()!) ? activeTrack()!.name : null} chain={activeTrack()?.fx ?? []}
          clip={cursorClip ? { name: clipName(cursorClip.q.clip), at: cursorClip.q.a, fx: cursorClip.q.clip.fx ?? [] } : null}
          hasSelection={hasRange() && selection?.trackId === activeTrack()?.id}
          ontrack={changeFx} ontracklive={liveTrackDraft} onclip={changeCursorClipFx} onlearn={learnNoise} />
      </aside>
    {/if}
    <aside class:hidden={workspace === "fx"}>
      {#if selection && selection.trackId === activeTrack()?.id && hasRange()}
        <div class="panel">
          <h3>Selection {fmtTime(selection.a)}–{fmtTime(selection.b)}</h3>
          <label for="sg">Gain: {selGain} dB</label>
          <input id="sg" type="range" min="-24" max="12" step="0.5" bind:value={selGain} />
          <label for="sfi">Fade in (ms)</label>
          <input id="sfi" type="number" min="0" max="10000" step="10" bind:value={selFadeIn} />
          <label for="sfo">Fade out (ms)</label>
          <input id="sfo" type="number" min="0" max="10000" step="10" bind:value={selFadeOut} />
          <button onclick={applySelProps}>Apply to selection</button>
          <div class="small">Rides on the clip (non-destructive), baked in on export.</div>
        </div>
      {/if}
      {#if recovery}
        <div class="panel rec-banner"><b>Unsaved work found</b><div class="small">StreamWave closed unexpectedly. Do you want your last autosave back?</div>
          <div class="row2"><button onclick={() => recover(true)}>Recover</button><button onclick={() => recover(false)}>Discard</button></div></div>
      {/if}
      {#if showExport && activeTrack()}
        <ExportPanel trackName={activeTrack()!.name} onexport={doExport} onclose={() => (showExport = false)} />
      {/if}
      <StudioPanel enabled={!!activeTrack()} {ab} busy={studioBusy} {analysis} onrun={studioSound} onab={() => { ab = !ab; lufs = null; liveFx(); liveDry(); }} />
      {#if activeTrack()}
        <div class="panel">
          <h3>Transcript</h3>
          {#if whisper && !whisper.found}
            <div class="small">{whisper.message}</div>
          {:else if whisper && !whisper.model_found}
            <div class="small">{whisper.message}</div>
            <button class="small" onclick={pickWhisperModel}>Choose model…</button>
          {:else}
            <button class="small" onclick={transcribe} disabled={transcribing}>{transcribing ? "Transcribing…" : "Transcribe track"}</button>
          {/if}
          {#if segments.length && segmentsTrack === activeTrack()?.id}
            <div class="segs">
              {#each segments as s (s.start)}
                <button class="seg" onclick={() => seekSeg(s)} title="Play from here"><b>{fmtTime(s.start)}</b> {s.text}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}      <FxPanel track={activeTrack()} {ab} {lufs} hasSelection={hasRange() && selection?.trackId === activeTrack()?.id}
        onchange={changeFx} onlearn={learnNoise} onab={() => { ab = !ab; lufs = null; liveFx(); liveDry(); }} onmeasure={measure} />
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
          {#if virtualOutputs.length}
            <div class="virthint">🔀 Virtual audio device detected ({virtualOutputs.map((d) => d.friendly_name).join(" · ")}). Choose it here as the output, then select the same device as the microphone in OBS / Discord / Zoom to feed them StreamWave playback.</div>
          {/if}
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
          <label class="chk"><input type="checkbox" checked={settings.update_auto_check ?? true} onchange={(e) => patchSettings({ update_auto_check: e.currentTarget.checked })} /> Check for updates at startup (once a day)</label>
          <button class="aboutlink" onclick={() => (showAbout = true)}>ℹ About StreamWave · ☕ Support the developer</button>
          <label class="chk"><input type="checkbox" bind:checked={settings.auto_backoff} onchange={() => setLatency(settings.latency)} /> Auto-switch to safer latency on dropouts</label>
          <label class="chk"><input type="checkbox" checked={settings.safety_track ?? false} onchange={(e) => { settings.safety_track = e.currentTarget.checked; api.setSafety(e.currentTarget.checked); }} /> Safety track: also save a −12 dB copy of every recording</label>
          <label class="chk"><input type="checkbox" checked={!!settings.voice_trigger} onchange={(e) => setVoiceTrigger(e.currentTarget.checked)} /> Voice trigger: start/stop recording by input level</label>
          {#if settings.voice_trigger}
            <div class="vtrow">
              <label for="vtthr">Threshold {settings.voice_trigger.threshold_db} dB</label>
              <input id="vtthr" type="number" min="-60" max="0" step="1" bind:value={settings.voice_trigger.threshold_db} onchange={() => setVoiceTrigger(true)} />
              <label for="vthold">Hold {settings.voice_trigger.hold_ms} ms</label>
              <input id="vthold" type="number" min="50" max="5000" step="50" bind:value={settings.voice_trigger.hold_ms} onchange={() => setVoiceTrigger(true)} />
            </div>
            <div class="small">Starts when the input stays above the threshold, stops ~1.5 s after it drops back below.</div>
          {/if}
          <label for="blufs">Batch export loudness target: {batchLufs} LUFS</label>
          <select id="blufs" bind:value={batchLufs}>
            <option value={-14}>-14 LUFS · Spotify / YouTube / streaming</option>
            <option value={-16}>-16 LUFS · Podcast / Apple Podcasts</option>
            <option value={-23}>-23 LUFS · Broadcast (EBU R128)</option>
          </select>
          <label for="wmodel">Whisper model (ggml .bin, for transcription)</label>
          <div class="row2"><input id="wmodel" bind:value={settings.whisper_model} placeholder="auto-detect next to the app" onchange={() => api.saveSettings(settings)} /><button onclick={pickWhisperModel}>Browse…</button></div>
        </div>
      {/if}
    </aside>
  </main>

  <div class="toasts">{#each toasts as t (t.id)}<div class="toast {t.kind}">{t.text}</div>{/each}</div>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 12px; padding: 10px 16px; border-bottom: 1px solid var(--line-dark); background: linear-gradient(180deg, #2a2c31, #1d1f22); box-shadow: 0 1px 0 #ffffff0d inset, 0 3px 10px #0008; position: relative; z-index: 2; }
  .brand { font-size: 17px; margin-right: auto; } .brand b { color: var(--accent); } .aboutlink { margin-top: 10px; width: 100%; background: transparent; color: var(--accent); }
  .brand { display: flex; align-items: center; gap: 8px; } .logo { border-radius: 6px; }
  .devsel { flex: 0 1 300px; min-width: 150px; }
  header > button, .wsw button { white-space: nowrap; flex: none; }
  .brand { min-width: 0; white-space: nowrap; } .proj { overflow: hidden; text-overflow: ellipsis; max-width: 240px; min-width: 0; }
  main { flex: 1; display: grid; grid-template-columns: 1fr 320px; gap: 16px; padding: 16px; overflow: hidden; }
  .left { display: flex; flex-direction: column; gap: 16px; overflow: hidden; }
  aside { display: flex; flex-direction: column; gap: 16px; overflow-y: auto; } aside.hidden { display: none; }
  .xfveil { position: fixed; inset: 0; z-index: 90; }
  .xfpop { position: fixed; z-index: 91; width: 290px; padding: 12px 14px; box-shadow: var(--hw-shadow), 0 12px 32px #000c; outline: none; }
  .xfpop h3 { margin: 0 0 8px; font-size: 12px; } .xfrow { display: flex; gap: 12px; align-items: flex-start; } .xfside { flex: 1; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  main.fxws { grid-template-columns: minmax(0, 1fr) minmax(420px, 46%); }
  .fxaside { overflow: hidden; }
  .wsw { display: flex; padding: 3px; gap: 2px; border-radius: 999px; background: var(--inset-bg); box-shadow: var(--inset-shadow); }
  .wsw button { border-radius: 999px; padding: 5px 14px; font-size: 12px; font-weight: 800; letter-spacing: .06em; text-transform: uppercase; background: none; border-color: transparent; box-shadow: none; color: var(--muted); }
  .wsw button[aria-selected="true"] { background: linear-gradient(180deg, #ffb658, var(--accent)); color: var(--accent-ink); box-shadow: 0 0 10px rgb(var(--accent-rgb) / .35); }
  .panel { background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 14px; }
  .recpanel { display: flex; gap: 24px; align-items: center; background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 20px; }
  .bigrec { width: 110px; height: 110px; border-radius: 50%; border: 6px solid #15161a; background: radial-gradient(circle at 35% 30%, #ff7b8f, var(--rec) 55%, #a8202c); box-shadow: 0 0 0 3px #3a3d43, 0 0 0 4px #0009, 0 6px 14px #000a, 0 2px 0 #ffffff33 inset; flex: none; transition: transform .1s, box-shadow .2s; }
  .bigrec:hover { transform: scale(1.03); }
  .bigrec.on { animation: pulse 1.4s infinite; }
  .bigrec .ico { font-size: 42px; color: #fff; }
  @keyframes pulse { 0%,100% { box-shadow: 0 0 0 0 rgb(var(--rec-rgb) / 0.53); } 50% { box-shadow: 0 0 0 18px rgb(var(--rec-rgb) / 0.00); } }
  .recinfo { flex: 1; }
  .time { display: inline-block; font-size: 34px; font-variant-numeric: tabular-nums; font-weight: 600; letter-spacing: .04em; color: var(--accent); text-shadow: 0 0 10px rgb(var(--accent-rgb) / .45); background: var(--inset-bg); box-shadow: var(--inset-shadow); border-radius: 8px; padding: 2px 14px; margin-bottom: 6px; }
  .hint, .small { color: var(--muted); font-size: 12px; } .hint { margin-bottom: 8px; }
  .tracks { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; }
  .empty { color: var(--muted); text-align: center; padding: 60px; border: 2px dashed var(--line); border-radius: 12px; }
  .track.act { border-color: rgb(var(--accent-rgb) / 0.40); }
  .scroll { overflow-x: auto; }
  .lane { position: relative; overflow: hidden; }
  .ghost { position: absolute; top: 0; bottom: 0; z-index: 5; pointer-events: none; border: 2px dashed var(--accent); background: rgb(var(--accent-rgb) / 0.19); border-radius: 4px; box-sizing: border-box; }
  .ghost span { position: absolute; top: 1px; left: 4px; font-size: 10px; color: var(--text); white-space: nowrap; }
  .ghost.bad { border-color: var(--rec); background: rgb(var(--rec-rgb) / 0.19); }
  .snapline { position: absolute; top: -4px; bottom: -4px; width: 0; border-left: 2px solid var(--blue); z-index: 6; pointer-events: none; }
  .track.droptarget { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent), 0 0 14px rgb(var(--accent-rgb) / 0.20); }
  .track.dropbad { border-color: var(--rec); box-shadow: 0 0 0 1px var(--rec); }
  .dragread { position: fixed; z-index: 80; pointer-events: none; background: rgb(var(--bg-rgb) / 0.93); border: 1px solid var(--accent); color: var(--text); font-size: 12px; padding: 4px 8px; border-radius: 6px; white-space: nowrap; }
  .dragread.bad { border-color: var(--rec); } .dragread b { font-variant-numeric: tabular-nums; color: var(--accent); }
  .paused { font-size: 14px; color: var(--warn); letter-spacing: .1em; }
  .sendvid { width: 100%; padding: 10px; font-weight: 600; }
  .proj { color: var(--muted); font-size: 13px; } .recent { width: 130px; }
  .armrow { display: flex; justify-content: space-between; align-items: center; margin-top: 6px; font-size: 12px; } .armrow label { margin: 0; }
  .small { font-size: 11px; } button.small { padding: 3px 8px; }
  .rec-banner { border-color: var(--warn); } .row2 { display: flex; gap: 8px; margin-top: 8px; } .row2 button { flex: 1; }
  .hint2 { text-align: center; padding: 6px; }
  .mixbar { display: flex; gap: 8px; align-items: center; }
  .mixbar .follow { margin-left: auto; display: inline-flex; align-items: center; gap: 10px; }
  .mixbar .fhold { color: var(--muted); font-style: italic; }
  .virthint { background: rgb(var(--accent-rgb) / 0.07); border: 1px solid rgb(var(--accent-rgb) / 0.27); border-radius: 8px; padding: 8px; font-size: 11px; color: var(--muted); }
  .segs { max-height: 220px; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; margin-top: 8px; }
  .seg { text-align: left; font-size: 12px; padding: 4px 6px; background: transparent; border: none; border-radius: 6px; }
  .seg:hover { background: var(--panel2); } .seg b { color: var(--accent); margin-right: 6px; font-variant-numeric: tabular-nums; }
  .track { background: var(--hw-bg); border: var(--hw-border); border-radius: var(--hw-radius); box-shadow: var(--hw-shadow); padding: 10px 12px; }
  .thead { display: flex; align-items: center; gap: 10px; margin-bottom: 6px; }
  .play { width: 34px; height: 34px; padding: 0; border-radius: 50%; }
  .tname { font-weight: 800; text-transform: uppercase; letter-spacing: .05em; font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 40%; }
  .tag { font-size: 10px; text-transform: uppercase; padding: 2px 7px; border-radius: 10px; background: rgb(var(--accent-rgb) / 0.13); color: var(--accent); }
  .track.multi { box-shadow: inset 3px 0 0 var(--blue); }
  .vbtn { padding: 1px 7px; border-radius: 10px; background: #ffffff0d; border: 1px solid #ffffff1a; cursor: pointer; } .vbtn.on { border-color: var(--accent); color: var(--accent); }
  .rename { width: 180px; padding: 2px 6px; font-weight: 600; }
  .vsel { padding: 0 20px 0 6px; background-position: right 6px center; height: 22px; width: auto !important; flex: none; min-width: 0; }
  .tag.labels { background: rgb(var(--warn-rgb) / 0.13); color: #ffd27a; }
  .tag.audacity { background: rgb(var(--blue-rgb) / 0.13); color: #7fb2ff; }
  button.tag.flag { border: 1px solid rgb(var(--warn-rgb) / 0.40); background: rgb(var(--warn-rgb) / 0.08); color: var(--warn); cursor: pointer; padding: 0 6px; font-size: 11px; }
  .drophint { position: fixed; inset: 12px; border: 3px dashed var(--accent); border-radius: 14px; background: rgb(var(--bg-rgb) / 0.80); display: grid; place-items: center; z-index: 60; pointer-events: none; font-size: 18px; }
  .aupbusy { position: fixed; right: 16px; bottom: 16px; z-index: 55; width: 320px; }
  .aupbusy .bar { height: 6px; background: #ffffff1a; border-radius: 3px; margin: 8px 0 4px; overflow: hidden; } .aupbusy .bar div { height: 100%; background: var(--accent); }
  .modal-back { position: fixed; inset: 0; background: #0008; display: grid; place-items: center; z-index: 70; }
  .modal { max-width: 560px; max-height: 80vh; overflow: auto; } .modal p { margin: 6px 0; white-space: pre-wrap; }
  .tag.obs { background: rgb(var(--purple-rgb) / 0.13); color: #a99cff; } .tag.recovered { background: rgb(var(--warn-rgb) / 0.13); color: var(--warn); }
  .x { margin-left: auto; background: transparent; border: none; color: var(--muted); }
  .trimbtn { background: var(--accent); color: #002; border-color: var(--accent); }
  .seg { display: flex; gap: 4px; } .seg button { flex: 1; padding: 6px 4px; font-size: 12px; } .seg .sel { background: var(--accent); color: #002; border-color: var(--accent); }
  .chk { display: flex; gap: 8px; align-items: center; } .chk input { width: auto; }
  .vtrow { display: grid; grid-template-columns: 1fr 70px; gap: 6px; align-items: center; font-size: 12px; }
  .vtrow label { margin: 0; }
  .toasts { position: fixed; bottom: 16px; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; gap: 6px; }
  .toast { background: linear-gradient(180deg, #2b2e33, #222428); border: 1px solid var(--line-dark); box-shadow: 0 6px 20px #000a; border-left: 4px solid var(--accent); padding: 9px 14px; border-radius: 8px; font-size: 13px; }
  .toast.warn { border-left-color: var(--warn); } .toast.err { border-left-color: var(--rec); }
</style>
