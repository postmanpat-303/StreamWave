# Changelog

All notable changes to StreamWave are documented here. The format is loosely based on [Keep a Changelog](https://keepachangelog.com/).

## Unreleased

### Transcript setup
- **The Transcript panel helps you set up whisper.cpp** instead of the old "whisper.cpp not found" message. That message also suggested `winget install whisper.cpp`, a package that doesn't exist. The panel now:
  - explains what whisper-cli is and what "on PATH" means;
  - shows the right install command for your system with a **Copy** button: `scoop install whisper-cpp` on Windows, `brew install whisper.cpp` on Mac, `pacman`/`apt` on distros that package it, or building from source;
  - has **Open terminal**: on Linux the command is typed in for you to run; on Windows and Mac it is copied for pasting. Nothing runs until you press Enter.
- **Set it up for me** (Windows and Linux): downloads the official whisper.cpp build from the ggml-org/whisper.cpp GitHub releases and a speech model from Hugging Face. A confirm dialog lists the files, where they come from and their sizes, and lets you pick the model. You get progress and Cancel. Files are checked by sha256 and size, then installed into StreamWave's own folder and used from there, so no PATH change is needed. On Windows you can optionally add that folder to your user PATH. **Remove** undoes everything.
- **I already have it…**: choose whisper-cli yourself (checked before it's saved), then **Check again**.
- Safer lookup. In order:
  1. the file you chose;
  2. StreamWave's copy;
  3. `whisper-cli` next to the app;
  4. `whisper-cli` on PATH.

  A generic `main` or bare `whisper` program is never run. Downloads only go over HTTPS to GitHub and Hugging Face, and redirects are checked too.

## 0.11.1 (2026-10-20)

### Fixed
- **New / Open / Close / Recover no longer leave the old project playing.** Playback stops (with a short fade) after you confirm "Discard unsaved changes?", the transport and playhead reset, and the old project's audio is freed. If a recording is running you are asked first; the take is saved and added to the old project before it closes. Cancelling any prompt or the Open dialog changes nothing.
- New **File > Close Project** (Ctrl+W).

### Follow playhead
- The timeline scrolls smoothly to keep the playhead in view while playing or recording. Toggle it with the **FOLLOW** LED on the transport row, **View > Follow Playhead** or **F**. It is on by default and remembered.
- Scrolling or dragging in the timeline pauses following. It resumes when the playhead is back in view, or on the next Play. The ruler and the preview move with it.

### Faster Play and Stop
- Play now starts in about 30 ms instead of 120–1000 ms. The output stream stays open in shared mode, so other apps can still use the device. Unchanged audio is not re-rendered or re-measured, and imported audio is kept decoded.
- Stop fades out over 6 ms instead of cutting (no click) and is ready for the next Play at once.
- While a track plays, its waveform no longer redraws 30 times a second, which keeps the window responsive.

## 0.11.0 (2026-10-20)

### New look
- **Hardware-style design**: dark brushed panels with screws, knurled rotary knobs with an orange tick ring, LED level meters, pill buttons, captioned dropdowns and LED on/off toggles. Labels are uppercase and the accent colour is amber. The app stays dark and keeps the whale logo, every shortcut and every setting.
- **Knobs everywhere you shape sound**: the effects rack, Layering Effects and the effect dialogs use knobs instead of sliders. Drag up/right (or scroll, or use the arrow keys) to turn, hold **Shift** for fine steps, double-click or press Delete to reset. Each knob shows its value and range; bipolar ones (EQ gains) light up from the centre. Each change is still one undo step.
- **Colour per effect**: every effect type has its own colour (EQ blue, gate violet, compressor pink, loudness amber, limiter orange…), used in the rack, the layer list and the new workspace.
- **Meters**: the input meter is an LED row with a dB scale, plus a vertical level meter. The timecode uses an LED-style display, and the record button sits in a bezel.

### Timeline ruler
- A **time ruler** above the tracks scrolls with them and adapts its steps to the zoom level (m:ss, or h:mm:ss on long projects). It shows the playhead, cursor, selection and markers.
- **Click or drag on the ruler** to move the cursor (it seeks while playing). With the ruler focused, ←/→ step (Shift = bigger steps), Home/End jump to the start/end.

### Selected-audio preview
- A **preview strip** under the recorder shows the whole active track, the selection, the playhead and the part of the timeline currently in view, with LEN / START / END / SEL readouts.
- Click to move the cursor, drag to select. **Zoom to selection** makes the selection fill the timeline; **Fit** shows everything.

### Effects Layer workspace
- A **Settings | Effects Layer** switch in the header. **Effects Layer** replaces the side panel with a large effects editor; **Settings** brings the classic panel back exactly as it was. Switching never touches the project or playback.
- **Left effects list**: the active track's effects and the layers of the clip under the cursor, in processing order. Click one to edit it. The list collapses to a narrow strip (« / ») and does so automatically on small windows.
- A **signal-flow strip** shows the full order: clip layers → clip gain/fades → track effects → mix.
- The selected effect opens as a **colour-coded module** with big knobs, an on/off LED, run earlier/later and remove. Add track effects or clip layers, and use the track presets, from the same place.
- Turning a track-effect knob is heard live while playing. It is the same data as the rack and the ◧ panel, so nothing is applied twice.

### Fades and crossfades
- **Fade handles**: drag the small square at a clip's top-left or top-right corner to set its fade-in or fade-out. The fade's shape is drawn over the waveform.
- **Crossfades between neighbouring clips**: drag the small ⨯ handle at the bottom of a cut outwards, or click it for a small panel with a **Length** knob, the **Curve** (Equal power for different material, Linear for the same material or room tone) and **Remove crossfade**.
- A crossfade is centred on the cut and uses the audio just beyond each clip's edge, so nothing on the timeline moves. When there is no spare audio there (two whole files back to back), the clips are made to overlap by the missing amount: later audio moves earlier, and StreamWave tells you so.
- Playback, the mix and export all use the same render code, and a test checks that a crossfade plays exactly as it exports. Split, join, undo/redo, save and reopen keep crossfades. A cut or move that changes which clip a crossfade fades from removes it.

### Projects
- Projects that use crossfades or non-default fade shapes are saved as `.twt` **format version 3**. StreamWave 0.10 and older refuse them with "made by a newer StreamWave" instead of opening them wrongly. Projects without them still save as version 1 or 2, and every older `.twt` still opens and sounds the same.

### Fixes
- The crossfade handle sits above the overlay scrollbar used on Linux, so clicking it on a zoomed timeline no longer scrolls the view.

### Limits (honest list)
- Crossfades only work between **neighbouring clips on one track**; there is no free stacking of three or more clips.
- There is no master/output effect bus: the mix is the sum of the tracks after their effects.
- The ruler shows time only. There is no bars/beats ruler because projects have no tempo.
- Normalize and Loudness *clip layers* on a crossfaded clip measure the clip including the extra audio used by the crossfade.
- Playing a selection that starts inside a crossfade plays the incoming clip without the first part of the fade.

## 0.10.0 (2026-10-20)

### Importing several files at once
- **Choose how 2+ files are laid out.** Importing two or more audio files together (File → Import, the Import button, or dropping them on the window) now asks first:
  - **One after another on one track**: in the order shown (reorder with ↑/↓), starting at 0:00 or at the cursor. Each clip starts exactly where the previous one ends, at the decoded length of its file, with no gaps or overlaps, even when the files have different formats and sample rates.
  - **One track per file**: every file starts at the same time, on its own track named after the file.
- **Cancel** leaves the project untouched. A single file imports exactly as before. Files are referenced, not copied, and the whole import is one undo step.

### Moving clips
- **Drag a clip by its title bar** to move it along its track, or onto another track. You can drop it past the end of the project; the timeline grows, and the project length is the end of the last clip.
- While dragging you see a ghost of the clip, the target track lights up, and a readout shows the exact start time and track. The original clip is dimmed. **Esc** cancels.
- **Snapping** to markers, clip edges, the cursor and 0:00 is on by default; hold **Alt** to place freely.
- **Overlaps are refused.** A clip can't be dropped where it would overlap audio on the target track: the ghost turns red with "overlaps audio" and nothing changes. Empty space between clips is real silence (it plays and exports as silence).
- Moves only change where a clip sits. The audio, trims, gain, fades and effects go with the clip. Positions are whole samples at the track's rate, so moving clips around never drifts. Undo/redo, save and reopen all work.

### Layering Effects (per-clip effect stacks)
- **◧ button** on every clip's title bar, and **Layering Effects…** in the clip's right-click menu, open a panel with that clip's own effects as a stack of layers.
- The top layer runs first: list order is processing order. Each layer has an on/off (bypass) toggle and can be removed. Click a layer to edit its settings. Reorder by dragging the ⋮⋮ grip, with the ↑/↓ buttons, or from the keyboard (↑/↓ to pick, **Alt+↑/↓** to move, Space toggles, Del removes). Each change is one undo step.
- Layers use the same effects as track effects (EQ, compressor, gate, de-esser, limiter, loudness, normalize, gain, fades, AI denoise), running in the same engine. Noise reduction stays a track effect because it needs a learned noise profile.
- **Order**: clip layers → clip gain/fades → track effects → output. The clip's ◧ button shows how many layers are active.
- **Ownership**: layers belong to that one clip. Copy/paste gives the copy its own stack, and splitting a clip gives each piece its own copy, so editing one never changes another.
- **Live while playing**: editing a layer during playback hands the new audio to the player without stopping it (in the rare case the audio format changes, playback restarts instead). Playback and export run the same code, and a test checks they produce identical samples.

### Projects
- `.twt` files that use free clip positions or clip layers are saved as format version 2. Older StreamWave versions refuse those with a clear message instead of opening them wrongly. Projects without them still save as version 1. Every older `.twt` still opens.

### Credit and licence note
- The layer list was designed after looking at how the open-source Vital synth (GPL-3.0) presents its effect order: a compact reorderable list with on/off toggles, kept separate from the parameter editor. No Vital code was used. StreamWave's layers are its own code on its existing effects engine.

## 0.9.0 (2026-10-19)

### Recording
- **Safety track** (Settings): every recording is also written as a `*_safety.wav` copy at −12 dB, in lock-step with the main file (same pre-roll, rollover and crash recovery). Rescue clipped takes by boosting the safety copy.
- **Voice trigger** (Settings): level-activated recording. Starts when the input stays above a threshold (dB) for a hold time (ms), stops ~1.5 s after it drops back below. Indicator shown next to the record button.

### Editing
- **Silence auto-trim**: "✂ Silences" on a track scans for pauses (auto threshold = noise floor + 8 dB, 250 ms minimum, 80 ms padding, with a dynamic-range guard), then "Trim N silences" cuts them in one undoable step.
- **Per-clip gain and fades**: select a range, set gain (dB) and fade in/out (ms) in the Selection panel; applied non-destructively to the clips and baked on export. Clips with different settings are kept split.
- **Mixdown playback**: "▶ Mix all" renders every track with its effects and plays the combined mix.

### Routing
- **Virtual output hint**: when a virtual audio device (VB-CABLE, VoiceMeeter, loopback/Stereo Mix, BlackHole on Mac) is in the output list, Settings explains how to send StreamWave's sound into OBS, Discord or Zoom. The user guide has the setup steps.

### Export & projects
- **Batch export**: "⇩⇩ Batch" exports every track as a 24-bit WAV into a chosen folder, appending/replacing a Loudness slot at the target chosen in Settings (−14 / −16 / −23 LUFS).
- **`.twt` file association**: double-clicking a project file opens it in StreamWave (NSIS registers the association per-user).

### AI / transcription
- **Whisper transcription** (side panel): transcribe a track with an external whisper.cpp CLI (`whisper-cli` next to the app or on PATH; ggml model set in Settings). Click a segment to play from there. No model or binary is bundled.

### Audio engine
- Playback resampler upgraded from linear interpolation to FFT resampling via **rubato** (fixed input+output blocks, anti-aliased; identity fast-path when rates match).

### Evaluated, not adopted
- **DeepFilterNet** noise suppression: the current `deep_filter` crate on crates.io is a years-old 0.2.x line whose runtime is not included by default; newer releases are not published. RNNoise (existing "AI denoise" effect) remains the in-app AI denoise, and Whisper can denoise externally. Revisit when the crate situation improves.

## 0.8.4 (2026-10-18)
### Added
- **StreamWave for Mac.** One universal app for Apple Silicon and Intel Macs, macOS 11 Big Sur or later, as `StreamWave_0.8.4_universal.dmg` (drag into Applications) and `StreamWave_0.8.4_universal.app.zip`. It's ad-hoc signed but not notarised yet, so the first time right-click the app and choose **Open** (on newer macOS: **System Settings → Privacy & Security → Open Anyway**). FFmpeg is found in the Homebrew and MacPorts folders too (`brew install ffmpeg`), because apps opened from Finder get a short `PATH`.
- **Linux packages for every family of distros**, named like the other Wave apps: `StreamWave-0.8.4-linux-x86_64.deb` (Debian, Ubuntu, Mint, Pop!_OS), `.rpm` (Fedora, RHEL, openSUSE) and `.pkg.tar.zst` (Arch, CachyOS, Manjaro, EndeavourOS; built with `makepkg` from `packaging/arch/streamwave-bin/PKGBUILD`), next to the AppImage. The .deb and .rpm now declare the ALSA library and recommend FFmpeg. `scripts/package-linux.sh` builds them all.
- The website, README and user guide show macOS next to Windows and Linux, with system requirements, the first-launch steps and install commands per Linux distro.

### Changed
- **Updates on Mac are manual for now.** There's no in-app updater on Mac yet, so instead of a failed check, **Help → Check for updates** (and the quiet startup check) reads the latest release from GitHub and, when there's a newer version, shows what's new with an **Open download page** button instead of Download & install. The same goes for the Linux .deb, .rpm and Arch packages, which the in-app updater can't replace: the check tells you about the new version and opens the download page instead of offering a download that would fail. Windows and the Linux AppImage keep the signed in-app updater; `latest.json` still lists only those two.

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
