import type { Aup3Result } from "./aup3Import";
import { invoke } from "@tauri-apps/api/core";
import type { Effect } from "./fx";
type ClipArg = { src: string; start: number; end: number; gain_db?: number; fade_in_ms?: number; fade_out_ms?: number };
type FxArg = { bypass: boolean; fx: Effect };
export type LatencyProfile = "LowLatency" | "Balanced" | "Safe";
export interface DeviceInfo { id: string; host: string; raw_name: string; friendly_name: string; is_input: boolean; is_default: boolean; channels: number; sample_rate: number; }
export interface Tags { title: string; artist: string; album: string; date: string; genre: string; comment: string; }
export interface ExportOptions { format: string; sample_rate: number | null; channels: number | null; bit_depth: number | null; bitrate_kbps: number | null; quality: number | null; tags: Tags; }
export interface FfmpegInfo { found: boolean; path: string | null; version: string | null; message: string; }
export interface MicReport { issues: ("Clipping" | "TooQuiet" | "Silence" | "DcOffset" | "Hum" | "OneChannelSilent")[]; rms_db: number; peak_db: number; channel_peak_db?: number[]; }
export interface Analysis { noise_floor_db: number; speech_level_db: number; snr_db: number; lufs: number | null; balance_db: number[]; sibilance_db: number; plosives: number; reverb_ms: number; notes: string[]; }
export interface SplitTrack { path: string; index: number; label: string; }
export interface Settings { input_device: string | null; output_device: string | null; latency: LatencyProfile; auto_backoff: boolean;
  record_dir: string | null; obs_host: string; obs_port: number; obs_password: string; obs_auto_import: boolean; preroll_secs: number; preroll_prepend: boolean; recent_projects: string[]; follow_obs: boolean; safety_track?: boolean; voice_trigger?: { threshold_db: number; hold_ms: number } | null; whisper_model?: string | null; onboarded?: boolean;
  update_auto_check?: boolean; update_last_check?: number | null; update_skip_version?: string | null; }
export interface Level { left: number; right: number; recording: boolean; paused: boolean; rec_secs: number; xruns: number; playing: boolean; play_paused?: boolean; play_secs: number; play_total: number; }
export interface EngineStatus { input_id: string | null; output_id: string | null; latency: LatencyProfile; buffer_frames: number; sample_rate: number; channels: number; note: string | null; }
export interface Waveform { path: string; name: string; sample_rate: number; channels: number; duration_secs: number; peaks: number[]; }
export interface ObsStatus { connected: boolean; recording: boolean; paused: boolean; duration_ms: number; obs_version: string | null; message: string | null; }

export const api = {
  listDevices: () => invoke<DeviceInfo[]>("list_devices"),
  listHosts: () => invoke<string[]>("list_hosts"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke("save_settings", { settings }),
  recordingsDir: () => invoke<string>("recordings_dir"),
  selectInput: (id: string | null) => invoke("select_input", { id }),
  selectOutput: (id: string | null) => invoke("select_output", { id }),
  setLatency: (profile: LatencyProfile, autoBackoff: boolean) => invoke("set_latency", { profile, autoBackoff }),
  startRecording: () => invoke("start_recording"),
  stopRecording: () => invoke("stop_recording"),
  play: (path: string, start = 0) => invoke("play_file", { path, start }),
  playClips: (clips: ClipArg[], fx: FxArg[], start = 0) => invoke("play_clips", { clips, fx, start }),
  playMix: (tracks: { clips: ClipArg[]; fx: FxArg[]; gain?: number }[], start = 0) => invoke("play_mix", { tracks, start }),
  exportClips: (clips: ClipArg[], fx: FxArg[], path: string) => invoke<string>("export_clips", { clips, fx, path }),
  setPreroll: (secs: number, prepend: boolean) => invoke("set_preroll", { secs, prepend }),
  setSafety: (on: boolean) => invoke("set_safety", { on }),
  setVoiceTrigger: (cfg: { threshold_db: number; hold_ms: number } | null) => invoke("set_voice_trigger", { cfg }),
  saveLast: (secs: number) => invoke("save_last", { secs }),
  projectSave: (path: string, json: string) => invoke("project_save", { path, json }),
  projectOpen: (path: string) => invoke<string>("project_open", { path }),
  recentProjects: () => invoke<string[]>("recent_projects"),
  autosaveWrite: (json: string) => invoke("autosave_write", { json }),
  autosaveCheck: () => invoke<string | null>("autosave_check"),
  autosaveClear: () => invoke("autosave_clear"),
  ffmpegStatus: () => invoke<FfmpegInfo>("ffmpeg_status"),
  installKind: () => invoke<string>("install_kind"),
  exportTrack: (clips: ClipArg[], fx: FxArg[], path: string, options: ExportOptions) => invoke<string>("export_track", { clips, fx, path, options }),
  studioSound: (clips: ClipArg[], strength: "Subtle" | "Medium" | "Strong") => invoke<{ analysis: Analysis; chain: FxArg[] }>("studio_sound", { clips, strength }),
  obsSplit: (path: string) => invoke<SplitTrack[]>("obs_split", { path }),
  sendToVideo: (video: string, tracks: { clips: ClipArg[]; fx: FxArg[]; gain?: number }[], markers: { time: number; label: string }[], out: string) => invoke<string>("send_to_video", { video, tracks, markers, out }),
  pauseRecording: (pause: boolean) => invoke("pause_recording", { pause }),
  obsPause: (pause: boolean) => invoke("obs_pause_record", { pause }),
  learnNoise: (clips: ClipArg[]) => invoke<number[]>("learn_noise", { clips }),
  measureLufs: (clips: ClipArg[], fx: FxArg[]) => invoke<number | null>("measure_lufs", { clips, fx }),
  detectSilence: (clips: ClipArg[], thresholdDb: number | null, minMs: number, padMs: number) => invoke<{ start: number; end: number }[]>("detect_silence", { clips, thresholdDb, minMs, padMs }),
  whisperStatus: () => invoke<{ found: boolean; path: string | null; model_found: boolean; message: string }>("whisper_status"),
  transcribe: (path: string) => invoke<{ start: number; end: number; text: string }[]>("transcribe", { path }),
  stopPlayback: () => invoke("stop_playback"),
  /** Swap the live effect chain of the running playback (no restart, state carried over). */
  updateFx: (fx: FxArg[]) => invoke("update_fx", { fx }),
  pausePlayback: (pause: boolean) => invoke("pause_playback", { pause }),
  renderSelection: (clips: ClipArg[], fx: FxArg[], silence: boolean, name: string, dir: string | null) => invoke<string>("render_selection", { clips, fx, silence, name, dir }),
  spectrogram: (clips: ClipArg[], cols: number, fft = 2048) => invoke<ArrayBuffer>("spectrogram", { clips, cols, fft }),
  saveTextFile: (path: string, contents: string) => invoke("save_text_file", { path, contents }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  importAup3: (path: string, mediaDir: string | null) => invoke<Aup3Result>("import_aup3", { path, mediaDir }),
  importAudio: (path: string) => invoke<Waveform>("import_audio", { path, buckets: 4000 }),
  obsConnect: (host: string, port: number, password: string) => invoke<ObsStatus>("obs_connect", { host, port, password }),
  obsDisconnect: () => invoke("obs_disconnect"),
  obsStatus: () => invoke<ObsStatus>("obs_status"),
  obsStart: () => invoke("obs_start_record"),
  obsStop: () => invoke<string>("obs_stop_record"),
};

export const fmtTime = (s: number) => { const m = Math.floor(s / 60); return `${m}:${(s % 60).toFixed(1).padStart(4, "0")}`; };
