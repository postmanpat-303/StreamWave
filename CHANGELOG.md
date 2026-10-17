# Changelog

All notable changes to StreamWave are documented here. The format is loosely based on [Keep a Changelog](https://keepachangelog.com/).

## 0.5.1 (2026-10-17)
### Added
- **About StreamWave** (Help → About StreamWave, or the link at the bottom of Settings). Shows the logo, version, "Created by Derek Heasley", the MIT licence and FFmpeg LGPL notice, the GitHub link, and a **☕ Buy me a coffee** button.
- External links open in the system browser through the Tauri opener plugin. They are limited to an allowlist of exactly two URLs (the PayPal.me donation link and the GitHub repo).
- Donation link in the README, the website footer and GitHub Sponsors (`FUNDING.yml`).
### Fixed
- Windows installer: Start-menu and desktop shortcuts now point at an explicit icon (`streamwave.ico`, installed next to the app) instead of `,0`. Shortcuts from older installs are rewritten on upgrade, the uninstaller uses the whale icon too, and the shell icon cache is refreshed after install/uninstall (`SHChangeNotify` + `ie4uinit -show`), so Windows stops showing a stale cached icon.
### Changed
- Copyright: Derek Heasley.

## 0.5.0 - first public release (2026-10-17)

First public release for Windows 10/11 and Linux.

### Recording
- One-button recording (`R`) to 32-bit float WAV on a real-time, lock-free audio thread.
- **Shared mic, always**: inputs are opened in shared mode only (WASAPI shared, PipeWire/PulseAudio/JACK on Linux). OBS and Discord keep using the same mic.
- **Crash-safe**: streamed `.partial.wav` files with headers refreshed every 0.5 s, automatic repair and recovery on the next start. Long takes roll over to `_partN.wav` before the 4 GB WAV limit.
- **Never miss a take**: rolling pre-record buffer (0-60 s), "Save last N s", and an optional "prepend buffer on record".
- Live mic check on every channel: clipping, too quiet, silence, dead channel, DC offset, 50/60 Hz hum.
- Friendly device names, every host API (optional ASIO/JACK), hot-plug with automatic fallback and return, latency profiles with automatic back-off on dropouts.

### Editing & effects
- Non-destructive clip editing: cut/copy/paste/delete, select all, 500-step undo/redo, zoom, markers.
- Effect chain: Gain, EQ, Compressor, Gate, Noise reduction (learned profile), AI Denoise (RNNoise), De-esser, Normalize, Loudness (EBU R128, with ceiling), Fade, Limiter. Includes presets, bypass, reorder and A/B.
- **Real-time effects during playback**: slider moves and A/B apply in ~80 ms with no restart. Playback and export share one engine and are bit-identical.
- Pause/resume playback in place, Stop (`Esc`).
- **Studio Sound**: one-click voice analysis that builds an editable chain to -16 LUFS. Strengths: Subtle / Medium / Strong.

### OBS integration (obs-websocket v5)
- Connect, start/pause/stop, live status, auto-reconnect with backoff and visible status.
- Follow OBS (record/pause/stop together), scene changes as markers.
- Auto-import OBS recordings with each audio track split into its own track.
- Send back to video: remux the edited audio into a copy of the video (picture not re-encoded), with markers as chapters.

### Projects & export
- `.twt` StreamWave project files with atomic save + `.bak`, 30 s autosave, crash recovery, recent projects.
- Export WAV (16/24-bit dithered, 32-bit float), FLAC, WavPack, MKA, MP3, Ogg Vorbis, Opus, WebM, Speex with tags (via FFmpeg).

### App
- Official app icon: an engraved humpback whale singing as it breaches a wave, on orange. Used for the window, taskbar, shortcuts and installer.
- First-run setup: mic with live meter, optional OBS connection test, FFmpeg detection.
- Windows NSIS installer (per-user, WebView2 bootstrapper, FFmpeg LGPL bundled) and Linux AppImage.

### Known limitations
- Unsigned Windows installer (SmartScreen: *More info → Run anyway*).
- Imported files are decoded fully into memory; very long files open slowly.
- Toggling bypass on a look-ahead effect during playback can shift audio once by up to ~40 ms (export unaffected).
- Double-clicking a `.twt` file doesn't open it yet; use File → Open.
