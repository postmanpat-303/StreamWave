# StreamWave – test checklist for Del's PC

Tick each box. If something fails, note what you did and copy any red toast text.

## 0. Install / first run
- [ ] Build or install (README "Windows"). The installer runs per-user; WebView2 is fetched automatically if missing.
- [ ] Upgrading over an older version (via Help → Check for updates, or by running the new installer): it installs into the same folder, and Settings → Apps / Add or Remove Programs shows **one** StreamWave entry with Publisher "Del (postmanpat)".
- [ ] First launch shows the **Welcome** card: pick your mic and watch the meter move when you talk.
- [ ] Step 2: enter the OBS WebSocket password, click "Test connection". You should see "Connected to OBS x.y".
- [ ] Step 3 shows whether FFmpeg was found. If not, `winget install Gyan.FFmpeg`, restart the app, and it should say found.
- [ ] Restart the app: the welcome card does **not** come back.

## 1. Recording (mic shared with OBS/Discord)
- [ ] Open Discord voice and OBS (mic source active) **first**, then press `R` in StreamWave. All three get audio at once. Nothing should say "device in use".
- [ ] Record 30 s, then press `R` again. A track appears and the WAV is in the recordings folder.
- [ ] `Ctrl+P` pauses and resumes the recording. The paused gap is not in the file.
- [ ] Pre-roll: talk for 10 s without recording, then click "Save last 30 s". You get a track with what you just said.
- [ ] Mic check: tap the mic hard to clip it (Clipping warning). Mute your interface (Silence after ~3 s). With a mono mic on a 2-channel interface, you see "One input channel is silent".
- [ ] Pull the USB mic out mid-take: the take so far is saved, and the app falls back to default. Plug it back in: it switches back.
- [ ] Kill the app in Task Manager mid-take, then reopen: "Recovered 1 interrupted recording".
- [ ] (Optional, long) A take over ~3 h at 48 kHz stereo splits into `_part2` files with a toast and no gap.

## 2. Playback & live effects
- [ ] `Space` plays. `Space` again pauses and resumes **at the same spot**. `Esc` or ■ stops.
- [ ] While playing, move the EQ, compressor and gain sliders. You hear the change within ~0.1 s without a restart or click.
- [ ] Press A/B while playing: it switches between original and processed instantly.
- [ ] Undo/redo of a cut while playing restarts at the current spot (expected).
- [ ] Unicode/space paths: import a file from `C:\Users\Del\Música\test file 🎙.wav` and it plays and exports.

## 3. Editing
- [ ] Select a range with the mouse, then `Ctrl+X` / `Ctrl+V` / `Delete` / `Ctrl+Z` / `Ctrl+Y`.
- [ ] `M` adds a marker. `[` and `]` jump between markers.
- [ ] `Ctrl+S` saves a `.twt`. Close and reopen it from File > Recent: everything is the same.

## 4. Studio Sound
- [ ] Record a noisy room take, then run Studio Sound (Medium). The noise drops and loudness ends near -16 LUFS.
- [ ] A/B while playing compares before and after.

## 5. OBS
- [ ] Connect in the OBS panel (green dot).
- [ ] With "Follow OBS" on, start, pause and stop in OBS. StreamWave records, pauses and stops with it.
- [ ] Switching scenes during a take adds "Scene: …" markers.
- [ ] Auto-import: when OBS stops, each audio track of the MKV/MP4 arrives as its own track.
- [ ] **Close OBS** while connected: the dot turns orange with "Reconnecting, attempt N, next try in Ns". Reopen OBS: it goes green on its own within 30 s.
- [ ] "Stop trying" ends the reconnect loop.
- [ ] Send to video: edit the audio, then use "Send back to video". The new MP4/MKV keeps the video untouched and has chapters from your markers.

## 6. Export
- [ ] WAV 24-bit, FLAC, MP3 320, AAC, Opus. Each file plays in another player and the tags show.
- [ ] Export with effects equals what you heard during playback (same engine).
- [ ] Export to a folder with spaces and unicode in its name.

## 7. Hotkeys summary
`Space` play/pause · `Esc` stop · `R` record · `Ctrl+P` pause rec · `M` marker · `[ ]` markers · `Ctrl+Z/Y` undo/redo · `Ctrl+X/C/V` · `Del` · `Ctrl+A` · `+/-` zoom · `Ctrl+S/O/N` · `Ctrl+E` export · `Ctrl+I` import
