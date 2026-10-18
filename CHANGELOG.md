# Changelog

All notable changes to StreamWave are documented here. The format is loosely based on [Keep a Changelog](https://keepachangelog.com/).

## 0.8.3 (2026-10-18)
### Changed
- The author, publisher and copyright name is now **Del (postmanpat)** everywhere: the About screen, the Windows installer and the "Publisher" column in Add/Remove Programs, the licence, the website and the docs. The app identifier is unchanged, so settings, projects and updates carry over.

### Fixed
- **Dropdowns were unreadable on Linux.** WebKitGTK drew every drop-down list (microphone, project, export format and options, effect picker, spectrogram scale and colours) as a white box with light text, so the chosen value was invisible. Dropdowns are now drawn by StreamWave in the dark theme on Linux and Windows, with a dark, readable option list, and the app declares `color-scheme: dark` for native controls.
- **Windows showed a generic installer icon instead of the whale** for the downloaded setup file, and sometimes for shortcuts. Every size in the app icon was stored as PNG, which Explorer and Edge often can't draw at small sizes. The icon now keeps 16–128 px as classic BMP frames (256 px stays PNG). It's used for the app, the installer, the uninstaller and the shortcut icon, and a test stops it from being regenerated as all-PNG.
- **Windows upgrades after a publisher change.** The installer now finds an existing install under the current publisher key, the 0.5.0 "Del" key, and any earlier publisher key that points at a StreamWave install (plus the Add/Remove Programs entry and `%LOCALAPPDATA%\StreamWave` as before). After installing it removes the leftover key from the old publisher name, so upgrades stay in the same folder and Add/Remove Programs shows a single StreamWave entry. Tested under Wine: 0.8.2 → 0.8.3 via the in-app updater's `/P /UPDATE` and via the interactive "uninstall before installing" path, 0.5.0 → 0.8.3, and an install in a custom folder.

## 0.8.2 (2026-10-18)
### Fixed
- **Right-click submenus (Effects, Track) could be impossible to reach** when the menu had to be moved to fit the window, such as when right-clicking near the bottom. Moving the mouse diagonally towards the submenu crossed another menu item and closed it. Menus now use hover intent (a "safe triangle"): while the pointer is heading for an open submenu, crossing other items doesn't close it. If the pointer stops on another item, that item takes over after a moment.
- **Menu positioning was rewritten:**
  - The main menu opens at the pointer and flips up or left when there's no room, so it stays fully inside the window.
  - Submenus are placed beside their parent item. They flip to the left when there's no room on the right and shift up so their bottom fits. They never cover the parent menu's items, and they overlap only its padding, so there's no gap to fall into.
  - A menu taller than the window gets a maximum height and scrolls.
- A submenu now also opens on click and stays open until you hover or click another item.
- The menu could close by itself straight after opening when right-clicking near the edge of a zoomed or scrollable timeline, because it closed on any scroll. It now closes only when you scroll with the wheel outside it, click elsewhere, press Esc, or the window loses focus or is resized.
- Disabled menu items no longer swallow hover. Moving over them behaves like any other item.

## 0.8.1 (2026-10-18)
### Fixed
- Right-clicking the waveform on Windows showed WebView2's browser menu ("Save image as", "Copy image"). The browser context menu is now switched off app-wide, except in text boxes, which keep Cut/Copy/Paste.
- Browser shortcuts that don't belong in an app are blocked: F5 / Ctrl+R reload, Ctrl+F find, Ctrl+G, Ctrl+H, Ctrl+U view source, F3, F7 caret browsing, Ctrl+= / Ctrl+- / Ctrl+0 and Ctrl+scroll page zoom, Alt+← / → back/forward, and F12 / Ctrl+Shift+I / Ctrl+Shift+C dev tools in release builds. Ctrl+P (print) no longer opens a print dialog; it stays the app's Pause. App hotkeys are unaffected.
- An empty label track was drawn only a sliver wide. Label lanes now span the whole timeline.
### Added
- **Right-click menu** on waveforms and clips, in StreamWave's own style, with shortcut hints, greyed-out items that don't apply, and full keyboard control (arrows, Home/End, Enter, →/← for submenus, Esc, type-ahead).
  - Cut, Copy, Paste, Delete, Split at cursor / selection edges, Join clips, Select whole clip, Add label here, Add marker here, Trim to selection, Silence selection.
  - **Effects ▸** lists every effect. Picking one opens its settings dialog and applies it to just the selection: StreamWave renders the range into a new WAV (`media/Edits`) and swaps it in as one undoable edit, and your originals are never changed. "Add to track effects instead" keeps it live in the track chain. Also in this submenu: Fade in, Fade out, Normalize to −1 dB and Studio Sound.
  - **Track ▸**: Rename, Mute, Solo, Waveform/Spectrogram, Delete track. The same menu opens from the track header, and double-clicking a track name renames it.
  - Right-clicking outside the current selection first moves the cursor to the spot and track you clicked.
- Right-click menus on **label tracks** (edit, select or delete label; add label; labels → markers; export; rename or delete the track) and on the **empty timeline area** (paste as new track, import audio, import Audacity project, new label track).
- **Trim to selection** also trims label tracks, and **Silence selection** inserts real silence as its own clip.

## 0.8.0 (2026-10-18)
### Added (Audacity-style editing, batch 1)
- **Spectrogram view** per track: use the 〰 / ▤ button, or **View → Waveform / Spectrogram** (Ctrl+Shift+G) for the selected tracks.
  - Log or linear frequency scale, with a frequency axis.
  - Classic, Inferno or Greyscale colours.
  - Computed in the background from the edited audio (2048-point FFT, Hann window, −100 to 0 dB), and saved with the project.
- **Multi-track selection.** Drag up or down across tracks, or Ctrl-click (⌘-click) a track to add or remove it. **Ctrl+A** now selects every track. Cut, copy, paste, delete, split and join apply to all selected tracks. Copying from several tracks and pasting puts each piece back in its own track.
- **Label tracks:**
  - **Ctrl+B** (Edit → Add Label at Selection) adds a point or range label and lets you type straight away. A label track is created if there isn't one.
  - Labels are editable and deletable, and ▾ selects a label's range.
  - Labels move with the audio when you cut, delete or paste across them.
  - **File → Export Labels…** saves Audacity-compatible text; **File → Import Labels…** reads it.
  - **→ Markers** / Edit → Copy Labels to Markers turns labels into markers, which become chapters on export and Send Back to Video.
- **Split and join clips:** **Ctrl+K** (Edit → Split Clip) splits at the cursor or at both ends of a selection. **Ctrl+J** (Edit → Join Clips) removes the splits inside the selection.
  - Ctrl+I stays *Import*, so Split uses Ctrl+K instead of Audacity's Ctrl+I. Ctrl+J matches Audacity.
  - Splits are shown on the waveform, and double-click selects a whole clip.
  - Clips imported from Audacity projects keep their boundaries as splits.
- All tracks now share one timeline, so a 2-minute track is drawn half as long as a 4-minute one, and selections line up across tracks.
### Fixed
- Rendering a clip list that was cut at arbitrary points is now sample-exact. Previously each clip's length was rounded on its own, so a cut could drop or double one sample. A test checks that split clips render identically to the whole clip.

## 0.7.0 (2026-10-18)
### Added
- **Import Audacity 3 projects (`.aup3`).** Use **File → Import Audacity Project…**, pick an `.aup3` in the Import dialog, or drag one onto the window.
  - Brings across tracks and names, clip positions and trims (gaps are kept as silence), the audio in 16-bit, 24-bit and 32-bit float, stereo pairs, track gain (as a Gain effect), pan, mute and solo, clip envelopes, and labels (as markers).
  - Pan and envelopes are applied to the extracted audio, using Audacity's own exponential envelope curve. The result was checked against Audacity 3.7.3's own exports and matches within 0.001.
  - Effects, note/time tracks and pitch/speed changes are skipped, and a summary lists exactly what wasn't imported.
  - **The `.aup3` is opened strictly read-only and never changed.** A project that is still open in Audacity is refused with a friendly message. If Audacity left unsaved autosave data, that newer version is used, as Audacity itself would.
  - The audio is extracted to WAV files in `<your project> media/` next to a saved project, or under *Audacity imports* in your recordings folder.
  - Written from scratch from the file format. No Audacity code is used.
- **Drag and drop.** Drop audio or video files to add tracks, an `.aup3` to import it, or a `.twt` to open it.
- `muted` / `solo` track tags (click to clear). Muted tracks are left out of **Send back to video**.
- README FAQ: *Can StreamWave replace Audacity?*

## 0.6.1 (2026-10-18)
### Added
- **▶ YouTube channel** button in About, next to Buy Me a Coffee and PayPal. It opens [Del's channel](https://www.youtube.com/@whatgoesaround1551), with demos and promos for his apps. The URL is added to the app's link allowlist, which now has four exact URLs.
- **Watch the promo:** a clickable promo thumbnail in the README and on the website, linking to https://youtu.be/ZGsCz0rjXeg, plus YouTube channel links next to the donation links.

## 0.6.0 (2026-10-17)
### Added
- **In-app updates.** Use **Help → Check for updates…** or the button in About. StreamWave reads the latest GitHub release.
  - If a newer version exists, it shows the installed and new versions and the release notes (rendered safely, with links shown as text).
  - It offers **Download & install**, **Not now** and **Skip this version**.
  - Downloads show progress and are verified against the app's built-in signing key. On Windows the installer runs passively and reopens StreamWave. The AppImage offers **Restart now**.
  - Installing is blocked while recording, and you're warned about unsaved changes.
  - If you're up to date you get a friendly confirmation. If you're offline or the check fails, you get a calm message and nothing breaks.
- **Quiet startup check** (Settings → "Check for updates at startup", on by default). It runs at most once a day, stays silent unless there's an update, and respects "Skip this version". A later version is offered again.
- Uses Tauri's updater and process plugins. Release assets now include `.sig` files and `latest.json`.
### Notes
- 0.5.x has no updater, so install 0.6.0 by hand once. Later versions can be installed from inside the app.

## 0.5.4 (2026-10-17)
### Changed
- **☕ Buy me a coffee** (About screen, README, website, GitHub Sponsors button) now opens Buy Me a Coffee: https://buymeacoffee.com/postmanpat.
- PayPal (https://paypal.me/derekh100) stays as a smaller secondary **Donate via PayPal** button.
- The app's link allowlist is now exactly these three URLs: Buy Me a Coffee, PayPal and the GitHub repo.
- Windows installer built from the fixed 0.5.3 template, so upgrades from any 0.5.x work.

## 0.5.3 (2026-10-17)
### Fixed
- **Windows upgrade from 0.5.0 failed with "Error launching installer".** When upgrading, the installer finds the old version's folder and runs its uninstaller as `uninstall.exe _?=<folder>`. It only looked for that folder under `HKCU\Software\<publisher>\StreamWave`. 0.5.0 was published as "Del" and 0.5.1+ under a different publisher name, so the lookup came back empty and the old uninstaller was started with `_?=` and no folder.
- The installer now checks, in order, accepting a folder only if it contains `uninstall.exe`:
  1. the publisher key
  2. the old `Software\Del\StreamWave` key
  3. `InstallLocation` (quotes stripped)
  4. the folder containing `UninstallString` (quotes stripped)
  5. `%LOCALAPPDATA%\StreamWave`
- `InstallLocation` is now written without quotes. Quoted values written by 0.5.0–0.5.2 are still read correctly.
- The failed registry lookups no longer leave the NSIS error flag set, which would otherwise have been reported as "unable to uninstall".
- The 0.5.1 and 0.5.2 installers have the same problem when upgrading from 0.5.0. Fresh installs of them, and upgrades from 0.5.1/0.5.2, are fine.

## 0.5.2 (2026-10-17) – upgrades from 0.5.0 affected, use 0.5.3
### Fixed
- **Windows installer:** 0.5.1's installer could fail on Windows 11 with "Error launching installer". It is replaced by 0.5.2, built on the stock Tauri NSIS template. (The real cause turned out to be the upgrade path, which was fixed in 0.5.3.)
  - Removed the customised template and the post-install icon-cache refresh, which ran `ie4uinit.exe` and called `SHChangeNotify` through the System plugin.
  - The installer now does only plain NSIS work. It installs `streamwave.ico`, points the Start-menu and desktop shortcuts at it, and uses the whale icon for the uninstaller.
- Tested under Wine: fresh install, and upgrades from 0.5.0 and 0.5.1. All succeed, with shortcuts pointing at `streamwave.ico`.

## 0.5.1 (2026-10-17) – Windows installer withdrawn, use 0.5.2
### Added
- **About StreamWave** (Help → About StreamWave, or the link at the bottom of Settings). Shows the logo, version, "Created by Del (postmanpat)", the MIT licence and FFmpeg LGPL notice, the GitHub link, and a **☕ Buy me a coffee** button.
- External links open in the system browser through the Tauri opener plugin. They are limited to an allowlist of exactly two URLs (the PayPal.me donation link and the GitHub repo).
- Donation link in the README, the website footer and GitHub Sponsors (`FUNDING.yml`).
### Fixed
- Windows installer: Start-menu and desktop shortcuts now point at an explicit icon (`streamwave.ico`, installed next to the app) instead of `,0`. Shortcuts from older installs are rewritten on upgrade, the uninstaller uses the whale icon too, and the shell icon cache is refreshed after install/uninstall (`SHChangeNotify` + `ie4uinit -show`), so Windows stops showing a stale cached icon.
### Changed
- Copyright: Del (postmanpat).

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
