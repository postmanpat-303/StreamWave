# StreamWave edit model (developer note)

How tracks, clips, samples, markers, projects and effects fit together (state as of 0.9.0, before the
0.10.0 changes). Section "0.10.0 additions" at the end describes what changed.

## Project and history (frontend, `src/lib/editModel.ts`)
- A `Project` is an immutable value: `{ tracks: EditTrack[], markers?: Marker[] }`. Every edit builds a new
  value and `History.apply(label, next)` pushes the old one, so undo/redo are exact pointer swaps (500 steps).
- Original audio files are never written. Everything below is metadata pointing into them.

## Tracks and clips
- `EditTrack { id, name, kind, clips: Clip[], fx?: FxSlot[], muted?, solo?, view?, labels?, video? }`.
  `kind: "labels"` is a label track (no audio, `labels: {id,t0,t1,text}[]` in timeline seconds).
- A track is a **contiguous** list of clips: clip *i* starts where clip *i-1* ends; the first starts at 0.
  There are no explicit positions. `trackLen` = sum of clip lengths.
- `Clip { src, start, end, split?, gain_db?, fade_in_ms?, fade_out_ms? }`: `start/end` are seconds *inside the
  source file*. `split` marks a deliberate boundary (Ctrl+K); without it, adjacent pieces of the same source
  with identical props are merged back (`merge`), so internal splits from range edits leave no trace.
- Edits (cut/copy/paste/delete/trim/split/join/apply-clip-props/replaceRanges) work on time ranges via
  `partition` (split at a and b, classify by midpoint) and ripple: deleting shifts later clips left.
- Multi-track edits (`*Many`) apply one range to every selected track; label tracks shift their labels.

## Samples and rendering (Rust, `src-tauri/src/render.rs`)
- `render(cache, clips)` concatenates clips into one buffer. Output rate/channels = the first clip's source.
  The running timeline position `tl` is in seconds; each clip covers output frames
  `round(tl*rate) .. round((tl+len)*rate)`, so a split anywhere renders sample-identical to the whole clip.
  Sources at other rates are read with linear interpolation; channel counts are mapped `k % src_ch`.
- Per-clip gain and raised-cosine fades (0.9.0) are applied inside `render`.
- `render_mix(tracks)` renders each track, runs its track effects, conforms rate/channels to the first track and sums.
- Decoded sources live in a bounded MRU `DecodeCache` (1.5 GB).

## Effects (Rust `src-tauri/src/fx.rs`, TS `src/lib/fx.ts`)
- `FxSlot { id, bypass, fx: Effect }` (serde tag `type`), stored per track (`EditTrack.fx`). Order = DSP order.
- One streaming engine, `Chain`: stateful, block-size independent stages; look-ahead stages hide their latency.
  `process_chain` (export/measure) and live playback use the same code and are tested bit-identical.
- Whole-file stages (Normalize, Loudness) get a static gain from `resolve_gains` first.
- Apply-effect-to-selection (0.8.1, context menu > Effects) renders the selection through one effect into a new
  WAV under `<recordings>/Edits` and swaps it in with `replaceRanges` (undoable; the original stays on disk).

## Playback (Rust `src-tauri/src/audio/playback.rs`)
- `play_clips`: `render` the dry clips (with clip gain/fades) to an `Arc<Decoded>` off the audio thread, then a
  render-ahead thread runs the track `Chain` in 512-frame blocks and the resampler, and pushes into an `rtrb`
  lock-free ring. The cpal callback only pops from the ring (no locks, no allocation).
- Track effect changes go to the render-ahead thread over a channel (`Ctl::Fx`) and apply within ~80 ms.
- "Mix all" (`play_mix`) renders the whole mix offline (`render_mix`) and plays it without live effects.
- Clip edits restart playback at the current position.

## Markers and labels
- `Project.markers: {id,time,label}[]` are absolute timeline seconds, shared by all tracks (chapters on export,
  Send Back to Video). Label tracks hold their own labels. Neither is attached to clips.

## Project file (`src/lib/project.ts`, `src-tauri/src/project.rs`)
- `.twt` = JSON `{ format: "streamwave-project", version, nextId, tracks, markers, sources }`, written atomically
  (tmp + fsync + rename, `.bak` kept). The frontend owns the schema; Rust only checks the format tag.
  `sources` lists referenced files (peaks recomputed on open). Older "truewave-project" files still open.

## 0.10.0 additions
- **Gap clips** (`{ src: "", start: 0, end: len, gap: true }`) are silence. They make free positioning
  possible without changing the contiguous model: a clip "at 12.5 s" on an empty track is `[gap 12.5, clip]`.
  `merge` joins neighbouring gaps and drops trailing ones, so `trackLen` is still "end of the last audio".
  `render` writes zeros for gaps; rate/channels come from the first non-gap clip.
- **Clip moves** (`moveClip`): the clip is replaced by a gap of the same length on its old track (nothing
  else shifts) and placed at the target time on the target track, into a gap or after the end. Dropping onto
  existing audio is refused (no overlap). Positions are snapped to whole samples of the target track's rate.
- **Multi-file import** (`arrangeImport`): sequential on one new track, or one new track per file, from 0 or
  the playhead, one undo step.
- **Clip effects** (`Clip.fx`, "Layering Effects"): an ordered `FxSlot[]` owned by the clip instance, run by the
  same `fx::Chain` inside `render`, before clip gain/fades. Order: clip effects, clip gain/fades, track effects,
  mix. Changes during playback re-render the dry buffer off the audio thread and hand it to the render-ahead
  thread (`Ctl::Dry`), which swaps it at the current position; the audio callback is unchanged.
- `.twt` files that use gaps or clip effects are written as format version 2, so older StreamWave versions
  refuse them with "made by a newer StreamWave" instead of failing on an empty source path. Projects without
  them are still written as version 1.

## Restyle branch additions (crossfades, effect levels, workspaces)

### Where each effect applies
| Level | Edited in | Runs | Notes |
|---|---|---|---|
| **Clip layers** (Layering Effects) | ◧ panel on a clip, or the clip group in the Effects Layer workspace | First, on that clip only, top layer first | Part of `render()`; changing them re-renders the dry audio (`Ctl::Dry` swap, no restart) |
| **Clip gain / fades / crossfades** | Selection panel, lane fade handles, crossfade handle | After the clip's layers | Part of `render()` |
| **Track effects** | Effects rack, or the track group in the Effects Layer workspace | On the whole track after the clips are joined | Streamed by the playback `Chain`; parameter changes go in live (`Ctl::Fx`) |
| **Mix** | – | Tracks summed | No master/output effect bus |

The Effects Layer workspace and the Settings rack are two views of the same data: both call the same
`changeFx` / `setClipFx` paths, so switching views neither re-applies nor duplicates processing, and playback keeps
running. While a knob on a *track* effect is dragged, the draft chain is pushed to the running playback
(`update_fx`, no history entry); one undo step is recorded on release. Clip-layer knobs apply on release (each
change re-renders the clip).

### Fades and crossfades
- A clip's own fades use `fade_curve` (`s_curve` default = the raised cosine every older version used, `linear`,
  `equal_power`). The lane shows them and has a drag handle at each top corner.
- A crossfade is stored on the **incoming** clip: `xfade_ms`, `xfade_curve` (`equal_power` default, `linear`) and
  `xfade_from` (source + end of the clip it fades from). It is centred on the cut: the outgoing clip plays on into
  its source beyond its end and the incoming clip starts early from before its start ("handles"), and the two are
  summed with complementary gains (linear sums to exactly 1, equal power to power 1). Timeline positions do not move.
- Where a source has no spare audio at the cut (two whole files back to back), `setCrossfade` trims the clips by the
  missing amount so they truly overlap; later audio moves earlier by that amount and the UI says so.
- `render()` clamps every crossfade to the handles that really exist and to half of either clip, so no edit can make
  it read past a file or produce a dip; an edge with a crossfade ignores that clip's own fade on that edge.
- Any edit that changes the neighbour a crossfade fades from (cut, paste, move, trimming its end) drops the
  crossfade (`merge()` checks `xfade_from`); split/join keep it on the clip's head.
- Playback, export, mix and every other render use the same `render()`, so crossfades sound identical everywhere
  (tested in `crossfades_play_like_they_export`).
- Projects using crossfades or non-default fade shapes are saved as format **v3**; StreamWave 0.10 refuses v3 with
  "made by a newer StreamWave" instead of mis-reading it. Everything else still saves as v1/v2.

### Limits
- Crossfades are between **neighbouring** clips on one track (no free stacking of three or more clips).
- No master/output effect bus.
- Bars/beats ruler: not available (projects have no tempo); the ruler shows m:ss / h:mm:ss.
- Normalize/Loudness clip layers on a crossfaded clip measure the clip *including* its handles.
- Playing only a selection that starts inside a crossfade plays the incoming clip without the fade-in part.
