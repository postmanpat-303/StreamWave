# Audacity 3 test projects

These `.aup3` files were saved by **Audacity 3.7.3** (Debian package), driven through `mod-script-pipe` on 18 Oct 2026. They're used only as test inputs for StreamWave's read-only importer (`src-tauri/src/aup3.rs`).

| File | Contents |
|---|---|
| `fixture16.aup3` | 8 kHz, 16-bit project. **"Stereo Voice"** is a stereo 440 Hz sine at 0.5, 0–3 s. 1.0–1.5 s was split-deleted and the second clip moved to 2.5 s, so it has hidden, trimmed audio. The track is at −6 dB with solo on. **"Music"** is a mono 1 kHz sine at 0.8 from 0.5 to 2 s, muted, +3 dB, pan −50%, with an envelope going from 0 at 0.6 s to 1 at 1.0 s. A label track holds "Intro" (0.25 s) and "Outro" (2.5–3 s). |
| `fixture24.aup3` | 8 kHz, 24-bit project: 1 s of 500 Hz square wave at 0.9. |
| `fixture32f.aup3` | 8 kHz, 32-bit float project: 1 s of 500 Hz square wave at 0.9. |
| `voice_mix.wav` | Audacity's own export (File → Export, 16-bit stereo) of a copy of `fixture16.aup3` as saved: Voice solo, Music muted. |
| `music_mix.wav` | Audacity's own export of the same copy with Music unmuted and soloed and Voice muted. |

The tests check that StreamWave's extracted clips, multiplied by the track gain and laid out with the gaps, match these exports within 0.001. That covers trims, clip moves, stereo, pan and the envelope curve.
