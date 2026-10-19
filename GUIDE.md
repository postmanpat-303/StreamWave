# StreamWave – full user guide

A friendly, non-destructive audio recorder and editor: one-button recording, live meters, waveforms, and tight OBS Studio integration.
Built with **Tauri 2** (Rust audio engine) and **Svelte 5 + TypeScript** (Vite).

## Download and system requirements
StreamWave runs on **Windows, Linux and macOS**. Get it from the [latest release](https://github.com/postmanpat-303/StreamWave/releases/latest).

| | Windows | Linux | macOS |
|---|---|---|---|
| **System** | Windows 10 or 11, 64-bit | 64-bit (x86_64), any modern distro (PipeWire, PulseAudio or JACK) | macOS 11 Big Sur or later; one universal app for Apple Silicon and Intel |
| **File** | `StreamWave_<version>_x64-setup.exe` | `StreamWave-<version>-linux-x86_64.deb` / `.rpm` / `.pkg.tar.zst` (Arch), or `StreamWave_<version>_amd64.AppImage` | `StreamWave_<version>_universal.dmg` or `.app.zip` |
| **FFmpeg** | included | `sudo apt install ffmpeg` (or your distro's package) | `brew install ffmpeg` |
| **Updates** | in-app (Help → Check for updates) | in-app for the AppImage; .deb/.rpm/Arch: the check tells you and opens the download page, then install the new package | manual for now: the check tells you about a new version and opens the download page |

**Linux, per distro:**
- Debian / Ubuntu / Mint / Pop!_OS: `sudo apt install ./StreamWave-<version>-linux-x86_64.deb`
- Fedora / RHEL: `sudo dnf install ./StreamWave-<version>-linux-x86_64.rpm` (openSUSE: `sudo zypper install ./StreamWave-<version>-linux-x86_64.rpm`)
- Arch / CachyOS / Manjaro / EndeavourOS: `sudo pacman -U ./StreamWave-<version>-linux-x86_64.pkg.tar.zst`, or `makepkg -si` in `packaging/arch/streamwave-bin/`
- Any distro: `chmod +x StreamWave_<version>_amd64.AppImage` and run it. This is the copy that updates itself.

The packages put StreamWave in your app menu and install the `StreamWave` command. To update a package install, install the new package over it; settings and projects are kept.

**First launch on Windows:** the installer isn't code-signed yet. If SmartScreen appears, click **More info → Run anyway**.
**First launch on Mac:** open the `.dmg` and drag StreamWave into Applications. The app isn't notarised by Apple yet, so the first time **right-click (or Control-click) StreamWave and choose Open**, then click Open again. On newer macOS (Sequoia 15 and later), click Done, then go to **System Settings → Privacy & Security** and click **Open Anyway** next to *"StreamWave" was blocked*. After that it opens normally. Allow microphone access when asked. To update on Mac, download the new `.dmg` and drag StreamWave into Applications again to replace the old one; settings and projects are kept.
On Mac, shortcuts shown here with **Ctrl** use **⌘ Cmd**.

## What's in it (v0.5, see CHANGELOG.md)
- **One-button recording** (big red button or `R`) to 32-bit float WAV on a dedicated audio thread. The audio callback is lock-free: samples go into an `rtrb` ring buffer, and a writer thread streams them to disk.
- **Crash-safe recordings**: files are written as `*.partial.wav` and the header is refreshed every 0.5 s. At startup, any leftover partial files get their headers repaired and are renamed `*.recovered.wav`, then added to the project automatically.
- **Devices**: lists every host API cpal was built with, using friendly names. The choice is remembered (`settings.json` in the app config folder). Includes a live L/R meter for the selected input.
- **Hot-plug**: the device list is checked every 2 s, with a toast when a device is plugged in or unplugged. If the mic disappears mid-recording, the take is saved, the app switches to the default device, and it switches back when your mic returns.
- **Latency profiles**: Low latency (128 frames) / Balanced (512) / Safe (2048). Auto-backoff moves to a safer profile after more than 3 dropouts in 2 s (if you're recording, it waits until you stop).
- **Playback** of any imported file (through the output device, with channel mapping and a simple resampler).
- **Import** via symphonia (WAV, MP3, FLAC, OGG, AAC/M4A, MP4/MKV audio, AIFF, CAF), with min/max waveform peaks drawn on canvas.
- **OBS** (obs-websocket v5 via `obws`): connect with host/port/password, start/stop recording, live status. When OBS finishes a recording, the file is **auto-imported** as a track.

## Editing (non-destructive)
Tracks are lists of **clips** that point into the source files using start/end offsets. Your original recordings are never changed.
| Action | Shortcut |
|---|---|
| Select | click-drag on a waveform; **Shift-click** extends |
| Select several tracks | drag up or down across tracks, or **Ctrl-click** (⌘-click on Mac) a track to add or remove it. Cut, copy, paste, delete, split and join then apply to every selected track, and label tracks move with the audio. |
| Select a whole clip | double-click it |
| Split clip | **Ctrl+K** (Edit → Split Clip): splits at the cursor, or at both ends of a selection. Ctrl+I stays Import. |
| Join clips | **Ctrl+J** (Edit → Join Clips): removes the splits inside the selection |
| Add label | **Ctrl+B** (Edit → Add Label at Selection): type the text and press Enter. A point label is added at the cursor, a range label for a selection. |
| Waveform / spectrogram | the 〰 / ▤ button on each track, or **Ctrl+Shift+G** (View menu) for the selected tracks. Log or linear frequency scale; Classic, Inferno or Greyscale colours. |
| Cut / Copy / Paste | Ctrl+X / Ctrl+C / Ctrl+V (paste replaces the selection or inserts at the cursor) |
| Delete selection | Del or Backspace |
| Select all (every track) | Ctrl+A |
| Undo / Redo | Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z (full command history, 500 steps) |
| Play / Pause | Space (plays the selection if there is one, otherwise from the cursor) |
| Record | R |
| Import / Export WAV | Ctrl+I / Ctrl+E |
**Label tracks:** use **Edit → New Label Track**, or press Ctrl+B and one is created for you. Click a label's text to edit it, and click ▾ to select its range. Delete a label with ✕, or with Backspace in an empty label. **File → Export Labels…** saves Audacity-compatible text (`start⇥end⇥text`). **File → Import Labels…** reads it back. **→ Markers** copies labels to project markers, which become chapters in exports and Send Back to Video.
**Clips:** bright lines with a ▾ show splits. Faint lines show where one source file ends and the next begins. All tracks share one timeline, so a shorter track is drawn shorter.
**Right-click menu:** right-click a waveform for Cut, Copy, Paste, Delete, Split, Join clips, Select whole clip, Add label here (Ctrl+B), Add marker (M), Trim to selection and Silence selection. If you right-click outside the current selection, the cursor moves to that spot on that track first, so the menu acts where you clicked. Inside a selection, the selection is kept.
- **Effects ▸** lists every effect. Picking one opens a small dialog with its settings (Gain, Equalizer, Compressor, Noise gate, Noise reduction, De-esser, AI denoise, Limiter, Loudness, Normalize). **Apply** renders the selection through that effect into a new WAV in the project's `media/Edits` folder and swaps it in as its own clip. It is one undo step, and your original files are never changed. **Add to track effects instead** puts it in the track's live chain instead. With no selection, the effect applies to the whole track.
- **Fade in / Fade out** apply to the selection (with no selection: the first or last second of the clip). **Normalize to −1 dB** applies directly. **Studio Sound** runs the one-click Studio Sound on the track.
- **Track ▸** has Rename, Mute, Solo, Waveform/Spectrogram and Delete track. Right-clicking the track header opens the same menu. Double-click a track name to rename it.
- On a label track, right-click a label to edit, select or delete it. Right-click the lane to add a label, copy labels to markers, export labels, rename or delete the track. On the empty area below the tracks, right-click to paste as a new track, import audio, import an Audacity project or add a label track.
- Submenus open when you hover or click. You can move the mouse diagonally into a submenu: crossing other items on the way doesn't close it. Near the window edges, menus flip up or left so they stay fully visible, and a menu taller than the window scrolls.
- The menu works from the keyboard: arrow keys, Home/End, Enter/Space, → opens a submenu, ←/Esc closes it, and you can type a letter to jump to an item. Items that don't apply are greyed out and show their shortcut.
- Text boxes (label text, settings fields) keep the normal Cut/Copy/Paste menu. Browser shortcuts that make no sense in an app are switched off: F5 / Ctrl+R reload, Ctrl+F find, Ctrl+U view source, Ctrl+= / Ctrl+- / Ctrl+0 / Ctrl+scroll page zoom, Alt+← / → back/forward, and, in release builds, F12 / Ctrl+Shift+I dev tools. The app's own shortcuts are unaffected; Ctrl+P is still Pause.
These shortcuts are also in the **Edit / File / View / Transport menus**. When a text box is focused, the standard clipboard and undo shortcuts act on the text, never on the audio.
Playback and export render the edited clip list to a new 32-bit float WAV.
Tests: `npm test` (edit model, effects chain, project files) and `cargo test` in `src-tauri` (DSP, WAV recovery, rendering, project I/O, pre-roll, mic check, encoders; the FFmpeg tests skip if FFmpeg isn't installed).

## Effects (non-destructive, live)
Each track has an **effect chain** that is saved as settings only. It runs **live during playback**: slider moves, bypass and A/B are heard within ~0.1 s with no restart. Export uses the same engine, so the exported file matches what you heard. Effects are never baked into the source files.
Effects: Gain, Equalizer (low cut, low shelf, 3 bands, high shelf), Compressor, Noise gate, Noise reduction (learns from a selection of background noise), Normalize (peak), Loudness (target LUFS, EBU R128), Fade in/out, Limiter.
The side panel has sliders, on/off (bypass) toggles, up/down to reorder, and an **A/B** button to hear the original. One-click presets: *Podcast voice, Clean up noise, Streaming -14 LUFS, Broadcast -16 LUFS, Warm voice*.
Every change can be undone with Ctrl+Z. "Measure loudness" shows the processed track's LUFS.
Zoom: Ctrl+scroll or `+` / `-` (1-12x, scroll sideways).

## ✨ Studio Sound (one click)
Studio Sound analyses the selected track and builds an editable effect chain:
- **What it measures:** the noise floor (learned automatically from the quietest parts), speech level and SNR, loudness, tone balance against a voice target curve, sibilance, plosives, and an echo/reverb estimate.
- **The chain it builds:** learned noise reduction, then optional **AI denoise (RNNoise, via the pure-Rust `nnnoiseless`)** on Strong, then high-pass (raised if there are pops), then a gentle gap expander, EQ toward the voice curve, a de-esser if needed, a compressor, loudness to **-16 LUFS** (reached *through* the limiter, so it's hit and peaks stay under -1 dBFS), and finally the limiter.
- **Strength:** Subtle / Medium / Strong. The before/after toggle compares with the original, and Ctrl+Z undoes the whole thing.

## OBS sync
- **Follow OBS** (OBS panel): starting, pausing, resuming or stopping in OBS does the same in StreamWave, and vice versa.
- **Markers:** OBS scene changes during a recording add markers automatically ("Scene: Gameplay"). **M** adds one by hand, and **[** / **]** jump between them. Markers are saved in the project and can be undone.
- **Auto-import splits OBS's audio tracks** (mic, desktop, Discord...) into separate labelled tracks, kept in sync. Uses FFmpeg/ffprobe.
- **🎬 Send back to video:** mixes the OBS tracks with their edits and effects and puts the audio into a **copy** of the original video. The picture is copied with `-c:v copy` (never re-encoded), and markers become **chapters** in MKV/MP4/MOV/WebM. Audio codec: FLAC in MKV, AAC in MP4/MOV, Opus in WebM.

## Projects
- **.twt project files** (JSON): tracks, clips, effect chains and learned noise profiles. Ctrl+S save, Ctrl+Shift+S save as, Ctrl+O open, Ctrl+N new. The 📁 menu lists recent projects.
- Saves are atomic: the file is written to a temp file, synced, then renamed, and the previous version is kept as `.twt.bak`.
- **Autosave** every 30 s to the app config folder. After a crash you're offered **Recover / Discard** at the next launch.
- Source audio is referenced by path, not copied. Waveforms are rebuilt when a project opens, and missing files are reported.

## Export
Use **Export** (Ctrl+E) in the side panel: WAV (16/24-bit with dither, or 32-bit float), FLAC, WavPack, MKA, MP3, Ogg Vorbis, Opus, WebM, Speex. You can set bitrate or quality, sample rate, mono/stereo, and tags (title, artist, album, year, genre, comment).
WAV is written by StreamWave itself. All other formats use **FFmpeg**, which StreamWave finds at runtime: first next to the StreamWave executable, then on `PATH` (on Mac also the Homebrew and MacPorts folders `/opt/homebrew/bin`, `/usr/local/bin` and `/opt/local/bin`, because apps opened from Finder get a short `PATH`). If it isn't found, the panel explains this and only WAV is offered.
**Batch export** (⇩⇩ in the toolbar) writes every track as a 24-bit WAV into a folder you pick, with a Loudness slot applied at the target from Settings (−14 / −16 / −23 LUFS).

### Getting / bundling FFmpeg (LGPL build)
- **Windows**: download an *LGPL shared* build (e.g. the `ffmpeg-n7.x-latest-win64-lgpl-shared` zip from github.com/BtbN/FFmpeg-Builds). Put `ffmpeg.exe` and its DLLs next to `streamwave.exe`, or on PATH.
  - Note: LGPL builds leave out GPL-only encoders. Check `ffmpeg -encoders` for `libmp3lame` (LGPL), `libopus`, `libvorbis` and `libspeex` (BSD). MP3 via LAME is LGPL and normally included.
- **Linux**: `sudo apt install ffmpeg` / `sudo dnf install ffmpeg-free` / `sudo pacman -S ffmpeg`.
- **macOS**: `brew install ffmpeg` (or MacPorts `sudo port install ffmpeg`).
- **To bundle it with the installer**: copy the binary to `src-tauri/binaries/ffmpeg-<target-triple>(.exe)`, e.g. `ffmpeg-x86_64-pc-windows-msvc.exe`, and add `"externalBin": ["binaries/ffmpeg"]` under `bundle` in `tauri.conf.json`. Ship FFmpeg's LGPL licence text and offer its source, as the LGPL requires. The Windows installer config `src-tauri/tauri.windows-installer.conf.json` does exactly this (CI downloads the LGPL build automatically).

## Never miss a take + mic check
- While **armed**, StreamWave keeps a rolling **pre-record buffer** of the selected mic (0-60 s, default 30 s, in Settings).
  - **⟲ Save last N s** (or **Ctrl+Shift+R**) saves it as a new take.
  - The option "Add the buffer to the start when I press Record" includes the moments before you hit Record.
- **Safety track** (Settings): also writes a `*_safety.wav` copy of every take at −12 dB. If the main take clips, boost the safety copy to rescue it.
- **Voice trigger** (Settings): starts recording when the input stays above a threshold for a hold time, and stops ~1.5 s after it drops. Handy for hands-free or long unattended sessions.
- **Live mic check**: friendly warnings, and the meter changes colour, for clipping, too quiet, no signal (muted or unplugged mic), DC offset and 50/60 Hz mains hum.
- **The mic is never taken exclusively**: WASAPI is always opened in shared mode. On Linux, only shared sound-server routes are listed (default / PulseAudio / PipeWire / JACK); raw `hw:` devices that would lock the card are hidden. OBS, Discord and other apps keep working with the same mic, buffer included. (ASIO drivers are usually single-client by design, which is why ASIO is an optional build feature.)

## Transcription (Whisper)
The **Transcript** panel transcribes the active track's first source file with an external **whisper.cpp** CLI — nothing AI is bundled:
1. Install whisper.cpp so that `whisper-cli` (or `whisper`) is on `PATH` (on Mac, `brew install whisper-cpp` works; Homebrew and MacPorts folders are searched too), or place the executable next to StreamWave (an old `main` binary is only used from there).
2. Download a ggml model (e.g. `ggml-base.en.bin`) from https://huggingface.co/ggerganov/whisper.cpp and set its path in **Settings → Whisper model** (if it's named `ggml-*.bin` and sits next to the app, it's found automatically).
3. **Transcribe track** → click any line to play from that moment.

## Feeding OBS / Discord / Zoom from StreamWave
StreamWave can't install audio drivers itself, but virtual audio cables work today:
- Install a virtual cable (VB-CABLE, VoiceMeeter, or the "Stereo Mix" that some sound cards already expose).
- StreamWave detects virtual devices and says so in **Settings → Speakers / output**: pick the cable as the output, then choose the same cable as the **microphone** in OBS / Discord / Zoom. Everything StreamWave plays — including the **▶ Mix all** mixdown — comes out there.

## Editing extras
- **✂ Silences** (on a track): scans for pauses below the noise floor + 8 dB (min 250 ms, with 80 ms padding so word tails survive). Review the count, then **Trim N silences** cuts them in one undoable step.
- **Selection panel** (side bar): set per-clip **gain (dB)** and **fade in/out (ms)** for the selected range. Non-destructive — baked on export. Clips with different settings stay split.
- **▶ Mix all**: renders every track with its effects and plays the full mix.
- Double-clicking a `.twt` project file opens it in StreamWave.

## Build on Windows
1. **Rust**: install from https://rustup.rs (choose the MSVC toolchain). You also need **Visual Studio Build Tools 2022** with "Desktop development with C++".
2. **Node.js** 20 LTS or newer: https://nodejs.org
3. **WebView2**: already included in Windows 10 (since 1803) and Windows 11. The installer silently downloads it if it's missing.
4. In this folder:
   ```
   npm install
   npm run tauri dev        # run with hot reload
   npm run tauri build      # installer (.msi / NSIS .exe) in src-tauri\target\release\bundle
   ```
5. First launch opens a short setup (mic, OBS, FFmpeg). Then work through **TESTING.md**.

Note: if the project folder path contains spaces or non-ASCII characters and the MSVC build fails in a C dependency, move the folder to e.g. `C:\dev\streamwave`. This is a build-tool limitation; the app itself handles such paths.

### ASIO (optional, for pro audio interfaces)
ASIO is behind the cargo feature `asio`:
1. Download the Steinberg **ASIO SDK** (https://www.steinberg.net/developers/) and extract it, e.g. to `C:\SDKs\asiosdk`.
2. Install **LLVM** (`winget install LLVM.LLVM`); bindgen needs `libclang`.
3. `set CPAL_ASIO_DIR=C:\SDKs\asiosdk` (and `set LIBCLANG_PATH=C:\Program Files\LLVM\bin` if needed).
4. `npm run tauri dev -- --features asio`
ASIO devices then appear in the device list next to WASAPI ones, tagged `[Asio]`.

### OBS setup
In OBS 28+: **Tools → WebSocket Server Settings → Enable**. Copy the password into StreamWave's OBS panel (default port 4455).

## Build on Linux (any distro)
Release packages: `scripts/package-linux.sh` builds `dist-linux/StreamWave-<version>-linux-x86_64.deb`, `.rpm` and `.pkg.tar.zst` (Arch, with `makepkg` on `packaging/arch/streamwave-bin/PKGBUILD`), and the signed updater AppImage when `TAURI_SIGNING_PRIVATE_KEY` is set. It needs `rpmbuild` and `makepkg` (on Debian/Ubuntu: `sudo apt install rpm pacman-package-manager`).

Install the Tauri prerequisites (https://tauri.app/start/prerequisites/). On Debian/Ubuntu:
```
sudo apt install libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev libasound2-dev build-essential pkg-config libssl-dev
```
(Fedora: `webkit2gtk4.1-devel alsa-lib-devel openssl-devel librsvg2-devel`; Arch: `webkit2gtk-4.1 alsa-lib`.)

Audio goes through ALSA. PulseAudio and PipeWire show up as the `pulse` / `pipewire` / `default` ALSA devices (install `pipewire-alsa` or `alsa-plugins-pulseaudio`). For **JACK**, install `libjack-jackd2-dev` (or `pipewire-jack`) and run with `--features jack`.

`npm run tauri build` produces **AppImage, .deb and .rpm** packages (rpm needs `rpm-build`).

**Flatpak (follow-up):** not configured yet. The plan is a `com.del.streamwave.yml` manifest on `org.gnome.Platform` 47 with `--socket=pulseaudio --device=all --filesystem=xdg-music --filesystem=xdg-videos:ro` (the last one so OBS recordings can be read), built from the release binary.

## Build on macOS
Needs a Mac with **Xcode Command Line Tools** (`xcode-select --install`), Rust and Node.js 20+. For the universal app:
```
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm ci
scripts/package-macos.sh     # dist/StreamWave_<version>_universal.dmg and .app.zip
```
The app is ad-hoc signed (so it runs on Apple Silicon) but not notarised. Without a Mac, use the GitHub **Release** workflow with *platforms = macos* and a draft tag: it builds, smoke-tests and attaches the Mac files to that draft release.

## Layout
```
src/                      Svelte UI (App.svelte, lib/Meter, WaveCanvas, ObsPanel, api.ts)
src-tauri/src/lib.rs      Tauri commands, hot-plug thread, crash recovery at startup
src-tauri/src/audio/      engine (mod.rs), devices.rs, wavfix.rs (header repair)
src-tauri/src/waveform.rs symphonia decode + peak summaries
src-tauri/src/obs.rs      obs-websocket client + RecordStateChanged listener
src-tauri/src/settings.rs persisted settings
```

## Known gaps / next steps
- Exclusive device access is deliberately not supported, so the mic can always be shared with other apps.
- The meter covers the *selected* input only.
- Playback resampling uses rubato (FFT). Note: rubato 5.0.1's `Fft` in fixed-input mode drifts from the true ratio, so StreamWave pins both input and output block sizes (fixed+fixed mode), which keeps the ratio exact.
- Toggling bypass on a look-ahead effect (limiter, noise reduction, RNNoise) during playback can shift audio by up to ~40 ms once. Export is unaffected.
- RNNoise (Denoise) is tuned for 48 kHz. At other rates it still works but is less effective. DeepFilterNet was evaluated as an upgrade but the current crates.io crate is an old 0.2.x line without a usable runtime; revisit later.
- Imported files are decoded fully into memory (about 1 GB per hour of stereo audio). Very long files are slow to open; true streaming decode is a future improvement.
- Long recordings are split into `_partN.wav` every ~3.8 GB (about 3 h at 48 kHz stereo). They are imported as separate tracks.
- Flatpak packaging is a follow-up.
- Auto-backoff restarts the input stream, which is deferred while recording.
