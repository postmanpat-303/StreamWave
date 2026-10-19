//! Read-only import of Audacity 3 project files (.aup3).
//!
//! An .aup3 file is an SQLite database (application_id 'AUDY'). The project document is a
//! compact "binary XML" stream stored in the `project` table (or `autosave`, which holds newer
//! unsaved changes) as two blobs, `dict` (tag/attribute names) followed by `doc`. Audio lives in
//! the `sampleblocks` table, one row per block of int16 / int24 (stored in 4 bytes) / float32
//! samples, referenced from `<waveblock blockid=…>` elements.
//!
//! This is an independent implementation written from the documented file layout; no Audacity
//! code is used. The source file is opened with SQLite's `immutable` read-only mode, so nothing is
//! ever written next to it (no journal, WAL or lock files).
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const APPLICATION_ID: i64 = 0x4155_4459; // 'AUDY'
pub const FMT_INT16: i64 = 0x0002_0001;
pub const FMT_INT24: i64 = 0x0004_0001;
pub const FMT_FLOAT: i64 = 0x0004_000F;

// ---------------------------------------------------------------- binary XML ----

#[derive(Clone, Debug, PartialEq)]
pub enum Val { Str(String), Int(i64), Float(f64), Bool(bool) }
impl Val {
    pub fn f64(&self) -> Option<f64> { match self { Val::Int(i) => Some(*i as f64), Val::Float(f) => Some(*f), Val::Bool(b) => Some(*b as i64 as f64), Val::Str(s) => s.trim().parse().ok() } }
    pub fn i64(&self) -> Option<i64> { match self { Val::Int(i) => Some(*i), Val::Float(f) => Some(*f as i64), Val::Bool(b) => Some(*b as i64), Val::Str(s) => s.trim().parse().ok() } }
    pub fn str(&self) -> String { match self { Val::Str(s) => s.clone(), Val::Int(i) => i.to_string(), Val::Float(f) => f.to_string(), Val::Bool(b) => (*b as u8).to_string() } }
}

#[derive(Clone, Debug, Default)]
pub struct Elem { pub name: String, pub attrs: Vec<(String, Val)>, pub children: Vec<Elem> }
impl Elem {
    pub fn attr(&self, k: &str) -> Option<&Val> { self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v) }
    pub fn f(&self, k: &str) -> Option<f64> { self.attr(k).and_then(Val::f64) }
    pub fn i(&self, k: &str) -> Option<i64> { self.attr(k).and_then(Val::i64) }
    pub fn s(&self, k: &str) -> Option<String> { self.attr(k).map(Val::str) }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Elem> + 'a { self.children.iter().filter(move |c| c.name == name) }
}

struct Rd<'a> { b: &'a [u8], p: usize }
impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.p + n > self.b.len() { bail!("project document is truncated") }
        let s = &self.b[self.p..self.p + n]; self.p += n; Ok(s)
    }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into()?)) }
    fn i32(&mut self) -> Result<i32> { Ok(i32::from_le_bytes(self.take(4)?.try_into()?)) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into()?)) }
    fn i64(&mut self) -> Result<i64> { Ok(i64::from_le_bytes(self.take(8)?.try_into()?)) }
    fn f32(&mut self) -> Result<f32> { Ok(f32::from_le_bytes(self.take(4)?.try_into()?)) }
    fn f64(&mut self) -> Result<f64> { Ok(f64::from_le_bytes(self.take(8)?.try_into()?)) }
    fn len(&mut self) -> Result<usize> { let n = self.i32()?; if n < 0 { bail!("bad length in project document") } Ok(n as usize) }
}

fn decode_str(bytes: &[u8], char_size: u8) -> String {
    match char_size {
        2 => String::from_utf16_lossy(&bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()),
        4 => bytes.chunks_exact(4).map(|c| char::from_u32(u32::from_le_bytes([c[0], c[1], c[2], c[3]])).unwrap_or('\u{FFFD}')).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Decode Audacity's binary XML (dict bytes followed by doc bytes) into an element tree.
pub fn decode_doc(data: &[u8]) -> Result<Elem> {
    let mut r = Rd { b: data, p: 0 };
    let mut char_size = 1u8;
    let mut names: HashMap<u16, String> = HashMap::new();
    let mut name_stack: Vec<HashMap<u16, String>> = vec![];
    let mut stack: Vec<Elem> = vec![Elem { name: "#root".into(), ..Default::default() }];
    let look = |names: &HashMap<u16, String>, id: u16| names.get(&id).cloned().ok_or_else(|| anyhow!("project document refers to an unknown name"));
    macro_rules! attr { ($k:expr, $v:expr) => {{ let k = $k; let v = $v; if let Some(top) = stack.last_mut() { top.attrs.push((k, v)); } }} }
    while r.p < data.len() {
        match r.u8()? {
            0 => char_size = r.u8()?,                                   // CharSize
            1 => { let n = look(&names, r.u16()?)?; stack.push(Elem { name: n, ..Default::default() }); } // StartTag
            2 => {                                                        // EndTag
                let n = look(&names, r.u16()?)?;
                if stack.len() < 2 || stack.last().map(|e| e.name != n).unwrap_or(true) { bail!("project document is damaged (unbalanced <{n}>)") }
                let e = stack.pop().unwrap(); stack.last_mut().unwrap().children.push(e);
            }
            3 => { let k = look(&names, r.u16()?)?; let n = r.len()?; let s = decode_str(r.take(n)?, char_size); attr!(k, Val::Str(s)) } // String
            4 => { let k = look(&names, r.u16()?)?; attr!(k, Val::Int(r.i32()? as i64)) }       // Int
            5 => { let k = look(&names, r.u16()?)?; attr!(k, Val::Bool(r.u8()? != 0)) }         // Bool
            6 => { let k = look(&names, r.u16()?)?; attr!(k, Val::Int(r.i32()? as i64)) }       // Long (32-bit on disk)
            7 => { let k = look(&names, r.u16()?)?; attr!(k, Val::Int(r.i64()?)) }              // LongLong
            8 => { let k = look(&names, r.u16()?)?; attr!(k, Val::Int(r.u32()? as i64)) }       // SizeT (32-bit on disk)
            9 => { let k = look(&names, r.u16()?)?; let v = r.f32()? as f64; r.i32()?; attr!(k, Val::Float(v)) } // Float + digits
            10 => { let k = look(&names, r.u16()?)?; let v = r.f64()?; r.i32()?; attr!(k, Val::Float(v)) }      // Double + digits
            11 | 12 => { let n = r.len()?; r.take(n)?; }                  // Data / Raw (character data, unused)
            13 => name_stack.push(std::mem::take(&mut names)),            // Push
            14 => names = name_stack.pop().unwrap_or_default(),           // Pop
            15 => { let id = r.u16()?; let n = r.u16()? as usize; let s = decode_str(r.take(n)?, char_size); names.insert(id, s); } // Name
            16 => { r.u16()?; let n = r.len()?; r.take(n)?; }             // Blob (unused)
            t => bail!("project document has an unknown field type {t}"),
        }
    }
    while stack.len() > 1 { let e = stack.pop().unwrap(); stack.last_mut().unwrap().children.push(e); }
    let root = stack.pop().unwrap();
    root.children.into_iter().find(|e| e.name == "project").ok_or_else(|| anyhow!("no <project> in the Audacity file"))
}

// ---------------------------------------------------------------- database ----

/// Open the .aup3 strictly read-only. `immutable=1` stops SQLite creating -wal/-shm/journal files.
pub fn open_readonly(path: &Path) -> Result<Connection> {
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    if std::fs::metadata(&wal).map(|m| m.len() > 0).unwrap_or(false) {
        bail!("This Audacity project looks like it is still open in Audacity (or Audacity didn't close cleanly). Close it in Audacity, then try again.");
    }
    let mut url = url_path(path);
    url.insert_str(0, "file:");
    url.push_str("?mode=ro&immutable=1");
    let c = Connection::open_with_flags(&url, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("Couldn't open {}", path.display()))?;
    let app: i64 = c.query_row("PRAGMA application_id", [], |r| r.get(0)).context("This file isn't an Audacity project")?;
    if app != APPLICATION_ID { bail!("This file isn't an Audacity 3 project (.aup3)") }
    Ok(c)
}

fn url_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::new();
    if s.len() > 1 && s.as_bytes()[1] == b':' { out.push('/'); } // C:/… → /C:/…
    for ch in s.chars() {
        match ch { '?' => out.push_str("%3f"), '#' => out.push_str("%23"), '%' => out.push_str("%25"), _ => out.push(ch) }
    }
    out
}

/// Returns the project tree and whether it came from the autosave (unsaved changes) table.
pub fn read_doc(c: &Connection) -> Result<(Elem, bool)> {
    let get = |table: &str| -> Result<Option<Vec<u8>>> {
        let has: bool = c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |r| r.get::<_, i64>(0)).map(|n| n > 0)?;
        if !has { return Ok(None) }
        Ok(c.query_row(&format!("SELECT dict, doc FROM {table} WHERE id = 1"), [], |r| {
            let mut d: Vec<u8> = r.get::<_, Option<Vec<u8>>>(0)?.unwrap_or_default();
            d.extend(r.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default());
            Ok(d)
        }).optional()?)
    };
    if let Some(a) = get("autosave")?.filter(|d| !d.is_empty()) { return Ok((decode_doc(&a)?, true)) }
    let p = get("project")?.filter(|d| !d.is_empty()).ok_or_else(|| anyhow!("This Audacity project is empty (it has never been saved)"))?;
    Ok((decode_doc(&p)?, false))
}

// ---------------------------------------------------------------- model ----

#[derive(Clone, Debug)]
pub struct Block { pub start: i64, pub len: i64, pub id: i64 }
#[derive(Clone, Debug)]
pub struct Channel { pub num_samples: i64, pub format: i64, pub blocks: Vec<Block> }
#[derive(Clone, Debug)]
pub struct AClip { #[allow(dead_code)] pub name: String, pub offset: f64, pub trim_left: f64, pub trim_right: f64, pub stretch: f64, pub cents: f64, pub envelope: Vec<(f64, f64)>, pub chans: Vec<Channel> }
impl AClip {
    pub fn start(&self) -> f64 { self.offset + self.trim_left }
}
#[derive(Clone, Debug)]
pub struct ATrack { pub name: String, pub rate: f64, pub gain: f64, pub pan: f64, pub mute: bool, pub solo: bool, pub clips: Vec<AClip>, pub effects: Vec<String> }
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ALabel { pub t0: f64, pub t1: f64, pub title: String, pub track: String }
#[derive(Clone, Debug, Default)]
pub struct AProject { pub rate: f64, pub tracks: Vec<ATrack>, pub labels: Vec<ALabel>, pub notes: Vec<String>, pub master_effects: Vec<String> }

fn effect_names(e: &Elem) -> Vec<String> {
    e.kids("effects").flat_map(|l| l.kids("effect")).map(|fx| {
        let id = fx.s("id").unwrap_or_else(|| "effect".into());
        // ids look like "Built-in Effect: Reverb" or "VST3_…_Name"; keep the human part
        id.rsplit(':').next().unwrap_or(&id).trim().to_string()
    }).collect()
}

fn parse_channel_clip(c: &Elem) -> Option<(AClip, Channel)> {
    let seq = c.kids("sequence").next()?;
    let fmt = seq.i("sampleformat").unwrap_or(FMT_FLOAT);
    let num_samples = seq.i("numsamples").unwrap_or(0).max(0);
    let raw: Vec<(i64, Option<i64>, i64)> = seq.kids("waveblock").map(|b| (b.i("start").unwrap_or(0), b.i("length"), b.i("blockid").unwrap_or(0))).collect();
    // Audacity 3.x doesn't store block lengths: a block runs to the next block's start (or the end)
    let blocks = raw.iter().enumerate().map(|(k, (start, len, id))| {
        let next = raw.get(k + 1).map(|n| n.0).unwrap_or(num_samples);
        Block { start: *start, len: len.unwrap_or(next - start).max(0), id: *id }
    }).collect();
    let ch = Channel { num_samples, format: fmt, blocks };
    let envelope = c.kids("envelope").next().map(|e| e.kids("controlpoint").filter_map(|p| Some((p.f("t")?, p.f("val")?))).collect()).unwrap_or_default();
    Some((AClip {
        name: c.s("name").unwrap_or_default(), offset: c.f("offset").unwrap_or(0.0), trim_left: c.f("trimLeft").unwrap_or(0.0).max(0.0), trim_right: c.f("trimRight").unwrap_or(0.0).max(0.0),
        stretch: c.f("clipStretchRatio").unwrap_or(1.0), cents: c.f("centShift").unwrap_or(0.0), envelope, chans: vec![],
    }, ch))
}

/// Turn the decoded document into tracks/labels and collect notes about anything not imported.
pub fn parse_project(root: &Elem) -> AProject {
    let mut p = AProject { rate: root.f("rate").unwrap_or(44100.0), ..Default::default() };
    p.master_effects = effect_names(root);
    let mut skipped: HashMap<&str, usize> = HashMap::new();
    let mut pending: Option<ATrack> = None; // left channel waiting for its right channel
    let mut stretched = 0usize; let mut mismatched = 0usize;
    let kids: Vec<&Elem> = root.children.iter().collect();
    let mut i = 0;
    while i < kids.len() {
        let e = kids[i]; i += 1;
        match e.name.as_str() {
            "wavetrack" => {
                let linked = e.i("linked").unwrap_or(0) != 0;
                let mk = |e: &Elem| ATrack {
                    name: e.s("name").unwrap_or_else(|| "Audio".into()), rate: e.f("rate").unwrap_or(p.rate), gain: e.f("gain").unwrap_or(1.0), pan: e.f("pan").unwrap_or(0.0),
                    mute: e.i("mute").unwrap_or(0) != 0, solo: e.i("solo").unwrap_or(0) != 0, clips: vec![], effects: effect_names(e),
                };
                let clips: Vec<(AClip, Channel)> = e.kids("waveclip").filter_map(parse_channel_clip).collect();
                if let Some(mut left) = pending.take() {
                    // right channel of a stereo pair: zip clips with the left channel's
                    let same = left.clips.len() == clips.len() && left.clips.iter().zip(&clips).all(|(a, (b, ch))| (a.offset - b.offset).abs() < 1e-6 && a.chans[0].num_samples == ch.num_samples);
                    if same {
                        for (c, (_, ch)) in left.clips.iter_mut().zip(clips) { c.chans.push(ch); }
                        p.tracks.push(left);
                    } else {
                        mismatched += 1;
                        let mut right = mk(e);
                        right.clips = clips.into_iter().map(|(mut c, ch)| { c.chans = vec![ch]; c }).collect();
                        left.name = format!("{} (L)", left.name); right.name = format!("{} (R)", right.name);
                        p.tracks.push(left); p.tracks.push(right);
                    }
                    continue;
                }
                let mut t = mk(e);
                t.clips = clips.into_iter().map(|(mut c, ch)| { c.chans = vec![ch]; c }).collect();
                stretched += t.clips.iter().filter(|c| (c.stretch - 1.0).abs() > 1e-6 || c.cents.abs() > 1e-6).count();
                if linked && kids.get(i).map(|n| n.name == "wavetrack").unwrap_or(false) { pending = Some(t) } else { p.tracks.push(t) }
            }
            "labeltrack" => {
                let tn = e.s("name").unwrap_or_else(|| "Labels".into());
                for l in e.kids("label") {
                    let t0 = l.f("t").unwrap_or(0.0);
                    p.labels.push(ALabel { t0, t1: l.f("t1").unwrap_or(t0).max(t0), title: l.s("title").unwrap_or_default(), track: tn.clone() });
                }
            }
            "notetrack" => *skipped.entry("MIDI / note track").or_default() += 1,
            "timetrack" => *skipped.entry("time track").or_default() += 1,
            "tags" | "effects" | "timeline" | "projectTimeSignature" | "project_tempo" => {}
            other if other.ends_with("track") => *skipped.entry("other track").or_default() += 1,
            _ => {}
        }
    }
    if let Some(t) = pending { p.tracks.push(t) }
    p.tracks.retain(|t| !t.clips.is_empty() || { *skipped.entry("empty audio track").or_default() += 1; false });
    let mut kinds: Vec<_> = skipped.into_iter().collect(); kinds.sort();
    for (k, n) in kinds { p.notes.push(format!("Skipped {n} {k}{}", if n > 1 { "s" } else { "" })); }
    if stretched > 0 { p.notes.push(format!("{stretched} clip(s) had pitch/speed changes in Audacity; imported at original pitch and speed")); }
    if mismatched > 0 { p.notes.push(format!("{mismatched} stereo track(s) had different clips in each channel; imported as separate L/R tracks")); }
    let fx: Vec<String> = p.tracks.iter().flat_map(|t| t.effects.iter().map(move |f| format!("{f} (on \"{}\")", t.name))).chain(p.master_effects.iter().map(|f| format!("{f} (master)"))).collect();
    if !fx.is_empty() { p.notes.push(format!("Audacity effects aren't imported: {}. Use StreamWave's effects panel instead.", fx.join(", "))); }
    p
}

// ---------------------------------------------------------------- audio ----

/// Convert one stored block to f32 samples (int24 is stored in 4-byte ints).
pub fn block_to_f32(bytes: &[u8], format: i64) -> Vec<f32> {
    match format {
        FMT_INT16 => bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
        FMT_INT24 => bytes.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 8_388_608.0).collect(),
        _ => bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
    }
}

struct ChanReader<'a> { c: &'a Connection, ch: &'a Channel, cache: Option<(usize, Vec<f32>)> }
impl<'a> ChanReader<'a> {
    fn block(&mut self, bi: usize) -> Result<&[f32]> {
        if self.cache.as_ref().map(|(i, _)| *i != bi).unwrap_or(true) {
            let b = &self.ch.blocks[bi];
            let data = if b.id <= 0 { vec![0.0; b.len as usize] } else {
                let (fmt, bytes): (i64, Vec<u8>) = self.c.query_row("SELECT sampleformat, samples FROM sampleblocks WHERE blockid = ?1", [b.id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional()?.ok_or_else(|| anyhow!("The Audacity project is missing some audio (block {}). It may be damaged.", b.id))?;
                let mut v = block_to_f32(&bytes, fmt); v.resize(b.len as usize, 0.0); v
            };
            self.cache = Some((bi, data));
        }
        Ok(&self.cache.as_ref().unwrap().1)
    }
    /// samples [pos, pos+n) of the channel's sequence
    fn read(&mut self, pos: i64, n: usize, out: &mut Vec<f32>) -> Result<()> {
        out.clear(); out.resize(n, 0.0);
        let end = pos + n as i64;
        let first = self.ch.blocks.partition_point(|b| b.start + b.len <= pos);
        for bi in first..self.ch.blocks.len() {
            let (bs, bl) = (self.ch.blocks[bi].start, self.ch.blocks[bi].len);
            if bs >= end { break }
            let s = pos.max(bs); let e = end.min(bs + bl);
            if e <= s { continue }
            let data = self.block(bi)?;
            out[(s - pos) as usize..(e - pos) as usize].copy_from_slice(&data[(s - bs) as usize..(e - bs) as usize]);
        }
        Ok(())
    }
}

const ENV_MIN: f64 = 1e-7;
/// Gain envelope (times relative to the clip's untrimmed start), as Audacity applies it.
pub fn env_at(env: &[(f64, f64)], t: f64) -> f64 {
    match env.len() {
        0 => 1.0,
        _ if t <= env[0].0 => env[0].1,
        n if t >= env[n - 1].0 => env[n - 1].1,
        _ => {
            // wave-clip envelopes interpolate exponentially (linearly in log gain), floored at 1e-7
            let k = env.partition_point(|p| p.0 <= t); let (a, b) = (env[k - 1], env[k]);
            if b.0 - a.0 < 1e-12 { return b.1 }
            let (la, lb) = (a.1.max(ENV_MIN).ln(), b.1.max(ENV_MIN).ln());
            (la + (lb - la) * (t - a.0) / (b.0 - a.0)).exp()
        }
    }
}

/// Audacity's pan law for one channel of the output (left = 0, right = 1).
pub fn pan_gain(pan: f64, out_ch: usize) -> f64 { let pan = pan.clamp(-1.0, 1.0); if out_ch == 0 { if pan > 0.0 { 1.0 - pan } else { 1.0 } } else if pan < 0.0 { 1.0 + pan } else { 1.0 } }

#[derive(Serialize, Debug, Clone)]
pub struct OutClip { pub path: String, pub start: f64, pub end: f64 }
#[derive(Serialize, Debug, Clone)]
pub struct OutTrack { pub name: String, pub clips: Vec<OutClip>, pub gain_db: Option<f64>, pub muted: bool, pub solo: bool, pub channels: u16, pub clip_count: usize }
#[derive(Serialize, Debug, Clone)]
pub struct ImportResult { pub name: String, pub media_dir: String, pub tracks: Vec<OutTrack>, pub labels: Vec<ALabel>, pub notes: Vec<String>, pub from_autosave: bool }

fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let t = t.trim().trim_matches('.').to_string();
    if t.is_empty() { "track".into() } else { t.chars().take(60).collect() }
}

/// Pick a fresh folder `base`, `base (2)`, … so an import never overwrites earlier files.
pub fn unique_dir(base: &Path) -> PathBuf {
    if !base.exists() { return base.to_path_buf() }
    (2..).map(|i| PathBuf::from(format!("{} ({i})", base.display()))).find(|p| !p.exists()).unwrap()
}

/// Import `aup3` into WAV files under `media_dir` (created; must not be inside the source).
pub fn import(aup3: &Path, media_dir: &Path, progress: &mut dyn FnMut(f64)) -> Result<ImportResult> {
    let conn = open_readonly(aup3)?;
    let (root, from_autosave) = read_doc(&conn)?;
    let proj = parse_project(&root);
    if proj.tracks.is_empty() && proj.labels.is_empty() { bail!("This Audacity project has no audio tracks to import") }
    std::fs::create_dir_all(media_dir).with_context(|| format!("Couldn't create {}", media_dir.display()))?;
    let mut notes = proj.notes.clone();
    if from_autosave { notes.insert(0, "The project had unsaved changes in Audacity; imported the latest (autosaved) version".into()); }
    let total: f64 = proj.tracks.iter().flat_map(|t| &t.clips).map(|c| c.chans.iter().map(|ch| ch.num_samples as f64).sum::<f64>()).sum::<f64>().max(1.0);
    let mut done = 0.0;
    let mut out_tracks = vec![];
    let mut buf: Vec<Vec<f32>> = vec![];
    let mut panned = 0;
    for (ti, t) in proj.tracks.iter().enumerate() {
        let rate = t.rate.round().max(1.0) as u32;
        let in_ch = t.clips.iter().map(|c| c.chans.len()).max().unwrap_or(1).clamp(1, 2);
        let pan = if t.pan.abs() > 1e-4 { t.pan.clamp(-1.0, 1.0) } else { 0.0 };
        let out_ch = if pan != 0.0 { 2 } else { in_ch };
        if pan != 0.0 { panned += 1; }
        let mut clips: Vec<(f64, f64, PathBuf, f64)> = vec![]; // (timeline start, len secs, file, file len)
        let mut sorted: Vec<&AClip> = t.clips.iter().collect();
        sorted.sort_by(|a, b| a.start().total_cmp(&b.start()));
        for (ci, c) in sorted.iter().enumerate() {
            let n = c.chans[0].num_samples;
            let l = (c.trim_left * rate as f64).round() as i64;
            let r = (c.trim_right * rate as f64).round() as i64;
            let (s, e) = (l.clamp(0, n), (n - r).clamp(0, n));
            if e <= s { continue }
            let fmt = c.chans.iter().map(|ch| ch.format).max().unwrap_or(FMT_FLOAT);
            let baked = !c.envelope.is_empty() && c.envelope.iter().any(|p| (p.1 - 1.0).abs() > 1e-6) || pan != 0.0;
            let (bits, float) = if baked || fmt == FMT_FLOAT { (32, true) } else if fmt == FMT_INT24 { (24, false) } else { (16, false) };
            let file = media_dir.join(format!("{:02} {} - clip {}.wav", ti + 1, safe_name(&t.name), ci + 1));
            let spec = hound::WavSpec { channels: out_ch as u16, sample_rate: rate, bits_per_sample: bits, sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int } };
            let tmp = file.with_extension("part");
            let mut w = hound::WavWriter::create(&tmp, spec)?;
            let mut readers: Vec<ChanReader> = c.chans.iter().map(|ch| ChanReader { c: &conn, ch, cache: None }).collect();
            buf.resize(readers.len(), vec![]);
            const CH: i64 = 65536;
            let mut pos = s;
            while pos < e {
                let m = (e - pos).min(CH) as usize;
                for (k, rd) in readers.iter_mut().enumerate() { rd.read(pos, m, &mut buf[k])?; }
                for j in 0..m {
                    let g = if baked { env_at(&c.envelope, (pos + j as i64) as f64 / rate as f64) } else { 1.0 };
                    for oc in 0..out_ch {
                        let src = &buf[oc.min(buf.len() - 1)];
                        let mut v = src[j] as f64 * g;
                        if pan != 0.0 { v *= pan_gain(pan, oc) }
                        if float { w.write_sample(v as f32)? }
                        else if bits == 24 { w.write_sample(((v * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0)) as i32)? }
                        else { w.write_sample(((v * 32768.0).round().clamp(-32768.0, 32767.0)) as i16)? }
                    }
                }
                pos += m as i64;
                done += (m * readers.len()) as f64; progress((done / total).min(0.99));
            }
            w.finalize()?;
            std::fs::rename(&tmp, &file)?;
            let len = (e - s) as f64 / rate as f64;
            clips.push((c.start(), len, file, len));
        }
        if clips.is_empty() { continue }
        // StreamWave tracks are a gapless list of clips, so Audacity's gaps become silence clips
        let mut out = vec![]; let mut cursor = 0.0f64; let mut gaps = vec![];
        for (st, len, f, _) in &clips {
            let gap = st - cursor;
            if gap > 1e-4 { gaps.push((out.len(), gap)); out.push(OutClip { path: String::new(), start: 0.0, end: gap }); }
            out.push(OutClip { path: f.to_string_lossy().into(), start: 0.0, end: *len });
            cursor = cursor.max(st + len);
        }
        if !gaps.is_empty() {
            let max_gap = gaps.iter().map(|g| g.1).fold(0.0, f64::max);
            let sil = media_dir.join(format!("{:02} {} - silence.wav", ti + 1, safe_name(&t.name)));
            let spec = hound::WavSpec { channels: out_ch as u16, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
            let mut w = hound::WavWriter::create(&sil, spec)?;
            for _ in 0..((max_gap * rate as f64).ceil() as usize + 1) * out_ch { w.write_sample(0i16)?; }
            w.finalize()?;
            for (k, _) in gaps { out[k].path = sil.to_string_lossy().into(); }
        }
        let gain_db = if (t.gain - 1.0).abs() > 1e-6 && t.gain > 0.0 { Some((20.0 * t.gain.log10()).clamp(-24.0, 24.0)) } else { None };
        out_tracks.push(OutTrack { name: t.name.clone(), clip_count: clips.len(), clips: out, gain_db, muted: t.mute || t.gain <= 0.0, solo: t.solo, channels: out_ch as u16 });
    }
    progress(1.0);
    if panned > 0 { notes.push(format!("Pan was applied to the audio of {panned} track(s), so they're imported in stereo")); }
    if proj.tracks.iter().any(|t| t.clips.iter().any(|c| c.envelope.iter().any(|p| (p.1 - 1.0).abs() > 1e-6))) { notes.push("Volume envelopes (fades) were applied to the imported audio".into()); }
    if proj.tracks.iter().any(|t| t.gain > 0.0 && (20.0 * t.gain.log10()).abs() > 24.0) { notes.push("A track gain beyond ±24 dB was limited to ±24 dB".into()); }
    let name = aup3.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Audacity project".into());
    Ok(ImportResult { name, media_dir: media_dir.to_string_lossy().into(), tracks: out_tracks, labels: proj.labels, notes, from_autosave })
}

// ---------------------------------------------------------------- tests ----

#[cfg(test)]
pub mod fixture {
    //! Writes .aup3 files in the same layout Audacity uses (for tests).
    use super::*;
    pub enum A { S(&'static str, String), I(&'static str, i64), L(&'static str, i64), LL(&'static str, i64), B(&'static str, bool), D(&'static str, f64), Z(&'static str, u32) }
    pub enum N { Start(&'static str, Vec<A>), End(&'static str) }
    pub struct Enc { char_size: u8, names: Vec<String>, dict: Vec<u8>, doc: Vec<u8> }
    impl Enc {
        pub fn new(char_size: u8) -> Self { Enc { char_size, names: vec![], dict: vec![0, char_size], doc: vec![] } }
        fn s(&self, s: &str) -> Vec<u8> { match self.char_size { 2 => s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(), 4 => s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect(), _ => s.as_bytes().to_vec() } }
        fn id(&mut self, n: &str) -> u16 {
            if let Some(i) = self.names.iter().position(|x| x == n) { return i as u16 }
            let id = self.names.len() as u16; self.names.push(n.into());
            let b = self.s(n); self.dict.push(15); self.dict.extend(id.to_le_bytes()); self.dict.extend((b.len() as u16).to_le_bytes()); self.dict.extend(b); id
        }
        pub fn node(&mut self, n: N) {
            match n {
                N::Start(t, attrs) => {
                    let id = self.id(t); self.doc.push(1); self.doc.extend(id.to_le_bytes());
                    for a in attrs {
                        match a {
                            A::S(k, v) => { let id = self.id(k); let b = self.s(&v); self.doc.push(3); self.doc.extend(id.to_le_bytes()); self.doc.extend((b.len() as i32).to_le_bytes()); self.doc.extend(b); }
                            A::I(k, v) => { let id = self.id(k); self.doc.push(4); self.doc.extend(id.to_le_bytes()); self.doc.extend((v as i32).to_le_bytes()); }
                            A::B(k, v) => { let id = self.id(k); self.doc.push(5); self.doc.extend(id.to_le_bytes()); self.doc.push(v as u8); }
                            A::L(k, v) => { let id = self.id(k); self.doc.push(6); self.doc.extend(id.to_le_bytes()); self.doc.extend((v as i32).to_le_bytes()); }
                            A::LL(k, v) => { let id = self.id(k); self.doc.push(7); self.doc.extend(id.to_le_bytes()); self.doc.extend(v.to_le_bytes()); }
                            A::Z(k, v) => { let id = self.id(k); self.doc.push(8); self.doc.extend(id.to_le_bytes()); self.doc.extend(v.to_le_bytes()); }
                            A::D(k, v) => { let id = self.id(k); self.doc.push(10); self.doc.extend(id.to_le_bytes()); self.doc.extend(v.to_le_bytes()); self.doc.extend(8i32.to_le_bytes()); }
                        }
                    }
                }
                N::End(t) => { let id = self.id(t); self.doc.push(2); self.doc.extend(id.to_le_bytes()); }
            }
        }
        pub fn parts(&self) -> (Vec<u8>, Vec<u8>) { (self.dict.clone(), self.doc.clone()) }
    }
    pub fn create_db(path: &Path) -> Connection {
        let _ = std::fs::remove_file(path);
        let c = Connection::open(path).unwrap();
        c.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = 50593792;
            CREATE TABLE project (id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
            CREATE TABLE autosave (id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
            CREATE TABLE sampleblocks (blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER, summin REAL, summax REAL, sumrms REAL, summary256 BLOB, summary64k BLOB, samples BLOB);")).unwrap();
        c
    }
    pub fn add_block(c: &Connection, fmt: i64, samples: &[f32]) -> i64 {
        let bytes: Vec<u8> = match fmt {
            FMT_INT16 => samples.iter().flat_map(|s| ((s * 32768.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes()).collect(),
            FMT_INT24 => samples.iter().flat_map(|s| ((s * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32).to_le_bytes()).collect(),
            _ => samples.iter().flat_map(|s| s.to_le_bytes()).collect(),
        };
        c.execute("INSERT INTO sampleblocks (sampleformat, summin, summax, sumrms, summary256, summary64k, samples) VALUES (?1, 0, 0, 0, x'', x'', ?2)", rusqlite::params![fmt, bytes]).unwrap();
        c.last_insert_rowid()
    }
    pub fn write_doc(c: &Connection, table: &str, e: &Enc) { let (d, doc) = e.parts(); c.execute(&format!("INSERT INTO {table} (id, dict, doc) VALUES (1, ?1, ?2)"), rusqlite::params![d, doc]).unwrap(); }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    fn tmp(name: &str) -> PathBuf { let d = std::env::temp_dir().join("sw_aup3_test").join(name); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap(); d }
    fn read_wav(p: &str) -> (hound::WavSpec, Vec<f32>) {
        let mut r = hound::WavReader::open(p).unwrap(); let s = r.spec();
        let v = match s.sample_format { hound::SampleFormat::Float => r.samples::<f32>().map(|x| x.unwrap()).collect(), _ => { let sc = (1i64 << (s.bits_per_sample - 1)) as f32; r.samples::<i32>().map(|x| x.unwrap() as f32 / sc).collect() } };
        (s, v)
    }
    fn ramp(n: usize, k: f32) -> Vec<f32> { (0..n).map(|i| ((i % 200) as f32 / 200.0 - 0.5) * k).collect() }

    /// Builds a project: stereo int16 track with 2 clips (gap, trim), mono float track with gain/pan/mute,
    /// int24 track with an envelope, labels, a realtime effect and a time track.
    fn build(dir: &Path, char_size: u8, autosave: bool) -> PathBuf {
        let path = dir.join("song.aup3");
        let c = create_db(&path);
        let rate = 1000.0;
        let l1 = ramp(1500, 1.0); let r1 = ramp(1500, 0.5);
        let b = |c: &Connection, fmt, s: &[f32]| -> Vec<(i64, i64, i64)> { let mut v = vec![]; let mut st = 0; for ch in s.chunks(600) { v.push((st, ch.len() as i64, add_block(c, fmt, ch))); st += ch.len() as i64; } v };
        let l1b = b(&c, FMT_INT16, &l1); let r1b = b(&c, FMT_INT16, &r1);
        let l2 = ramp(500, 0.8); let l2b = b(&c, FMT_INT16, &l2); let r2b = b(&c, FMT_INT16, &l2);
        let mono = ramp(1000, 0.6); let mb = b(&c, FMT_FLOAT, &mono);
        let deep = vec![0.5f32; 800]; let db = b(&c, FMT_INT24, &deep);
        let mut e = Enc::new(char_size);
        let seq = |e: &mut Enc, fmt: i64, n: i64, blocks: &[(i64, i64, i64)], silent_tail: bool| {
            e.node(N::Start("sequence", vec![A::Z("maxsamples", 262144), A::Z("sampleformat", fmt as u32), A::Z("effectivesampleformat", fmt as u32), A::LL("numsamples", n)]));
            for (s, l, id) in blocks { e.node(N::Start("waveblock", vec![A::LL("start", *s), A::LL("length", *l), A::LL("blockid", *id)])); e.node(N::End("waveblock")); }
            if silent_tail { e.node(N::Start("waveblock", vec![A::LL("start", n - 100), A::LL("length", 100), A::LL("blockid", -100)])); e.node(N::End("waveblock")); }
            e.node(N::End("sequence"));
        };
        let clip = |e: &mut Enc, name: &str, off: f64, tl: f64, tr: f64, env: &[(f64, f64)]| {
            e.node(N::Start("waveclip", vec![A::D("offset", off), A::D("trimLeft", tl), A::D("trimRight", tr), A::I("centShift", 0), A::L("pitchAndSpeedPreset", 0), A::D("clipStretchRatio", 1.0), A::B("clipStretchToMatchTempo", false), A::S("name", name.into()), A::L("groupId", -1), A::I("colorindex", 0), A::B("isSelected", false)]));
            let _ = env;
        };
        let env_end = |e: &mut Enc, env: &[(f64, f64)]| {
            e.node(N::Start("envelope", vec![A::Z("numpoints", env.len() as u32)]));
            for (t, v) in env { e.node(N::Start("controlpoint", vec![A::D("t", *t), A::D("val", *v)])); e.node(N::End("controlpoint")); }
            e.node(N::End("envelope")); e.node(N::End("waveclip"));
        };
        e.node(N::Start("project", vec![A::S("version", "1.3.0".into()), A::S("audacityversion", "3.7.3".into()), A::D("rate", rate), A::S("selectionformat", "hh:mm:ss".into())]));
        e.node(N::Start("tags", vec![])); e.node(N::End("tags"));
        // stereo track, linked
        for (ch, blocks1, blocks2) in [(0, &l1b, &l2b), (1, &r1b, &r2b)] {
            e.node(N::Start("wavetrack", vec![A::S("name", "Vocals ✓".into()), A::B("isSelected", true), A::B("isFocused", false), A::I("channel", ch), A::I("linked", if ch == 0 { 1 } else { 0 }), A::B("mute", false), A::B("solo", true), A::D("rate", rate), A::D("gain", 1.0), A::D("pan", 0.0), A::L("sampleformat", FMT_INT16)]));
            if ch == 0 { e.node(N::Start("effects", vec![A::B("active", true)])); e.node(N::Start("effect", vec![A::S("id", "Built-in Effect: Reverb".into()), A::S("version", "3.7.3".into())])); e.node(N::End("effect")); e.node(N::End("effects")); }
            // clip 1: sequence 1.5 s at offset 0.5, trimLeft 0.2, trimRight 0.3 -> plays 0.7..1.7
            clip(&mut e, "c1", 0.5, 0.2, 0.3, &[]); seq(&mut e, FMT_INT16, 1500, blocks1, false); env_end(&mut e, &[]);
            // clip 2: 0.5 s at 2.5
            clip(&mut e, "c2", 2.5, 0.0, 0.0, &[]); seq(&mut e, FMT_INT16, 500, blocks2, false); env_end(&mut e, &[]);
            e.node(N::End("wavetrack"));
        }
        e.node(N::Start("wavetrack", vec![A::S("name", "Music".into()), A::I("channel", 2), A::I("linked", 0), A::B("mute", true), A::B("solo", false), A::D("rate", rate), A::D("gain", 0.5), A::D("pan", -0.5), A::L("sampleformat", FMT_FLOAT)]));
        clip(&mut e, "m", 0.0, 0.0, 0.0, &[]); seq(&mut e, FMT_FLOAT, 1100, &mb, true); env_end(&mut e, &[]);
        e.node(N::End("wavetrack"));
        e.node(N::Start("wavetrack", vec![A::S("name", "Deep/24".into()), A::I("channel", 2), A::I("linked", 0), A::D("rate", rate), A::D("gain", 1.0), A::D("pan", 0.0), A::L("sampleformat", FMT_INT24)]));
        clip(&mut e, "d", 1.0, 0.0, 0.0, &[]); seq(&mut e, FMT_INT24, 800, &db, false); env_end(&mut e, &[(0.0, 0.0), (0.4, 1.0)]);
        e.node(N::End("wavetrack"));
        e.node(N::Start("labeltrack", vec![A::S("name", "Labels".into()), A::I("numlabels", 2)]));
        e.node(N::Start("label", vec![A::D("t", 0.7), A::D("t1", 0.7), A::S("title", "Intro".into())])); e.node(N::End("label"));
        e.node(N::Start("label", vec![A::D("t", 2.5), A::D("t1", 3.0), A::S("title", "Chorus <loud>".into())])); e.node(N::End("label"));
        e.node(N::End("labeltrack"));
        e.node(N::Start("timetrack", vec![A::S("name", "Time Track".into())])); e.node(N::End("timetrack"));
        e.node(N::End("project"));
        write_doc(&c, if autosave { "autosave" } else { "project" }, &e);
        if autosave {
            let mut old = Enc::new(char_size);
            old.node(N::Start("project", vec![A::D("rate", rate)])); old.node(N::End("project"));
            write_doc(&c, "project", &old);
        }
        drop(c);
        path
    }

    #[test]
    fn decodes_binary_xml_in_all_char_sizes() {
        for cs in [1u8, 2, 4] {
            let mut e = Enc::new(cs);
            e.node(N::Start("project", vec![A::S("name", "Ünïcødé 🎵".into()), A::I("i", -5), A::B("b", true), A::LL("ll", 1 << 40), A::Z("z", 7), A::D("d", 0.25)]));
            e.node(N::Start("child", vec![])); e.node(N::End("child")); e.node(N::End("project"));
            let (mut d, doc) = e.parts(); d.extend(doc);
            let p = decode_doc(&d).unwrap();
            assert_eq!(p.s("name").unwrap(), "Ünïcødé 🎵", "char size {cs}");
            assert_eq!(p.i("i"), Some(-5)); assert_eq!(p.i("b"), Some(1)); assert_eq!(p.i("ll"), Some(1 << 40)); assert_eq!(p.i("z"), Some(7)); assert_eq!(p.f("d"), Some(0.25));
            assert_eq!(p.children.len(), 1);
        }
        assert!(decode_doc(&[0, 1, 1, 9, 0]).is_err()); // unknown name id
    }

    #[test]
    fn converts_sample_formats() {
        assert_eq!(block_to_f32(&(-16384i16).to_le_bytes(), FMT_INT16), vec![-0.5]);
        assert_eq!(block_to_f32(&4_194_304i32.to_le_bytes(), FMT_INT24), vec![0.5]);
        assert_eq!(block_to_f32(&0.125f32.to_le_bytes(), FMT_FLOAT), vec![0.125]);
        assert_eq!(env_at(&[], 3.0), 1.0);
        assert!((env_at(&[(0.0, 0.01), (1.0, 1.0)], 0.5) - 0.1).abs() < 1e-9); // geometric midpoint
        assert!(env_at(&[(0.0, 0.0), (1.0, 1.0)], 0.25) < 1e-4);
        assert_eq!(env_at(&[(0.5, 0.2), (1.0, 1.0)], 0.0), 0.2);
        assert_eq!(pan_gain(-0.5, 0), 1.0); assert_eq!(pan_gain(-0.5, 1), 0.5); assert_eq!(pan_gain(0.25, 0), 0.75);
    }

    #[test]
    fn imports_tracks_clips_labels_and_never_touches_the_source() {
        for (cs, autosave) in [(4u8, false), (2, true)] {
            let dir = tmp(&format!("full{cs}"));
            let src = build(&dir, cs, autosave);
            let before = std::fs::read(&src).unwrap();
            let mtime = std::fs::metadata(&src).unwrap().modified().unwrap();
            let media = dir.join("media");
            let mut last = 0.0; let r = import(&src, &media, &mut |p| last = p).unwrap();
            assert!((last - 1.0).abs() < 1e-9);
            // source untouched, no sidecar files
            assert_eq!(std::fs::read(&src).unwrap(), before);
            assert_eq!(std::fs::metadata(&src).unwrap().modified().unwrap(), mtime);
            for ext in ["-wal", "-shm", "-journal"] { assert!(!dir.join(format!("song.aup3{ext}")).exists(), "{ext} created"); }
            assert_eq!(r.from_autosave, autosave);
            assert_eq!(r.tracks.len(), 3);
            // stereo track: leading silence 0.7, clip 1.0 s, gap 0.8, clip 0.5 s
            let v = &r.tracks[0];
            assert_eq!(v.name, "Vocals ✓"); assert_eq!(v.channels, 2); assert!(v.solo && !v.muted); assert_eq!(v.clip_count, 2);
            let lens: Vec<f64> = v.clips.iter().map(|c| c.end - c.start).collect();
            assert!(lens.iter().zip([0.7, 1.0, 0.8, 0.5]).all(|(a, b)| (a - b).abs() < 1e-6), "{lens:?}");
            assert!(v.clips[0].path.ends_with("silence.wav") && v.clips[2].path == v.clips[0].path);
            let (spec, s) = read_wav(&v.clips[1].path);
            assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 1000, 16));
            assert_eq!(s.len(), 2000);
            // first frame = sample 200 of the sequence (trimLeft 0.2 s)
            let l1 = ramp(1500, 1.0); let r1 = ramp(1500, 0.5);
            assert!((s[0] - l1[200]).abs() < 1e-4 && (s[1] - r1[200]).abs() < 1e-4);
            assert!((s[1998] - l1[1199]).abs() < 1e-4);
            // mono float track: gain 0.5 -> -6 dB, muted, pan baked to stereo, silent tail block
            let m = &r.tracks[1];
            assert!(m.muted); assert!((m.gain_db.unwrap() + 6.0206).abs() < 1e-3); assert_eq!(m.channels, 2);
            let (spec, s) = read_wav(&m.clips[0].path);
            assert_eq!((spec.channels, spec.bits_per_sample), (2, 32));
            assert_eq!(s.len(), 2200);
            let mono = ramp(1000, 0.6);
            assert!((s[20] - mono[10]).abs() < 1e-6 && (s[21] - mono[10] * 0.5).abs() < 1e-6);
            assert!(s[2100..].iter().all(|x| *x == 0.0));
            // int24 + envelope fade-in 0..0.4 s
            let d = &r.tracks[2];
            assert!((d.clips[0].end - 1.0).abs() < 1e-9 && d.clips[0].path.ends_with("silence.wav"));
            let (spec, s) = read_wav(&d.clips[1].path);
            assert_eq!(spec.bits_per_sample, 32);
            assert!(s[0].abs() < 1e-6 && (s[200] as f64 - 0.5 * 1e-7f64.sqrt()).abs() < 1e-5 && (s[700] - 0.5).abs() < 1e-4);
            // labels and notes
            assert_eq!(r.labels.len(), 2); assert_eq!(r.labels[1].title, "Chorus <loud>"); assert_eq!(r.labels[1].t1, 3.0);
            let notes = r.notes.join("\n");
            assert!(notes.contains("Reverb"), "{notes}"); assert!(notes.contains("time track"), "{notes}"); assert!(notes.contains("Pan"), "{notes}"); assert!(notes.contains("envelopes"), "{notes}");
            assert_eq!(notes.contains("autosaved"), autosave);
        }
    }

    #[test]
    fn rejects_non_audacity_and_open_projects() {
        let dir = tmp("bad");
        let p = dir.join("x.aup3"); std::fs::write(&p, b"not sqlite at all, just text").unwrap();
        assert!(import(&p, &dir.join("m"), &mut |_| {}).is_err());
        let q = dir.join("other.aup3"); { let c = Connection::open(&q).unwrap(); c.execute_batch("CREATE TABLE t(x);").unwrap(); }
        assert!(import(&q, &dir.join("m"), &mut |_| {}).unwrap_err().to_string().contains("isn't an Audacity"));
        let s = build(&dir, 4, false);
        std::fs::write(dir.join("song.aup3-wal"), b"x").unwrap();
        assert!(import(&s, &dir.join("m"), &mut |_| {}).unwrap_err().to_string().contains("still open"));
        // a path with characters that are special in SQLite URIs
        // no '?' in the folder name: it is legal on macOS/Linux but not on Windows
        let dir2 = tmp("we ird #name %20");
        let s2 = build(&dir2, 1, false);
        assert!(import(&s2, &dir2.join("m"), &mut |_| {}).is_ok());
    }

    #[test]
    fn unique_dir_never_reuses() {
        let d = tmp("uniq"); let a = d.join("Song"); std::fs::create_dir_all(&a).unwrap();
        assert_eq!(unique_dir(&a), d.join("Song (2)"));
        assert_eq!(unique_dir(&d.join("New")), d.join("New"));
    }
}

#[cfg(test)]
mod real_audacity {
    //! Fixtures saved by a real Audacity 3.7.3 (driven through mod-script-pipe), see tests/fixtures/audacity/README.md
    use super::*;
    fn fx(name: &str) -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audacity").join(name) }
    fn out(name: &str) -> PathBuf { let d = std::env::temp_dir().join("sw_aup3_real").join(name); let _ = std::fs::remove_dir_all(&d); d }
    fn wav(p: &str) -> (hound::WavSpec, Vec<f32>) {
        let mut r = hound::WavReader::open(p).unwrap(); let s = r.spec();
        let v = match s.sample_format { hound::SampleFormat::Float => r.samples::<f32>().map(|x| x.unwrap()).collect(), _ => { let sc = (1i64 << (s.bits_per_sample - 1)) as f32; r.samples::<i32>().map(|x| x.unwrap() as f32 / sc).collect() } };
        (s, v)
    }
    fn peak(v: &[f32]) -> f32 { v.iter().fold(0.0, |m, x| m.max(x.abs())) }

    #[test]
    fn imports_a_real_audacity_project() {
        let src = fx("fixture16.aup3");
        let before = std::fs::read(&src).unwrap();
        let r = import(&src, &out("16"), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), before, "source changed");
        for ext in ["-wal", "-shm", "-journal"] { assert!(!PathBuf::from(format!("{}{ext}", src.display())).exists()) }
        assert_eq!(r.tracks.len(), 2);
        // "Stereo Voice": 3 s of 440 Hz with 1..1.5 s split-deleted and the second clip moved to 2.5 s
        let v = &r.tracks[0];
        assert_eq!((v.name.as_str(), v.channels, v.solo, v.muted, v.clip_count), ("Stereo Voice", 2, true, false, 2));
        assert!((v.gain_db.unwrap() + 6.0).abs() < 1e-3);
        let lens: Vec<f64> = v.clips.iter().map(|c| c.end - c.start).collect();
        assert!(lens.iter().zip([1.0, 1.5, 1.5]).all(|(a, b)| (a - b).abs() < 1e-6), "{lens:?}");
        let (spec, a) = wav(&v.clips[0].path);
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 8000, 16));
        assert_eq!(a.len(), 2 * 8000);
        let (_, b) = wav(&v.clips[2].path); assert_eq!(b.len(), 2 * 12000);
        // Audacity's own mixdown of this track (File > Export, 16-bit) must match our clips x track gain
        let g = 10f32.powf(v.gain_db.unwrap() as f32 / 20.0);
        let ours: Vec<f32> = a.iter().map(|x| x * g).chain(std::iter::repeat(0.0).take(2 * 12000)).chain(b.iter().map(|x| x * g)).collect();
        let (_, mix) = wav(fx("voice_mix.wav").to_str().unwrap());
        assert_eq!(ours.len(), mix.len());
        let diff = ours.iter().zip(&mix).fold(0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(diff < 1e-3, "voice differs from Audacity's export by {diff}");
        // "Music": mono 1 kHz at 0.5 s, muted, +3 dB, pan -50% (baked to stereo), envelope 0 -> 1 over 0.6..1.0 s
        let m = &r.tracks[1];
        assert_eq!((m.name.as_str(), m.muted, m.channels), ("Music", true, 2));
        assert!((m.gain_db.unwrap() - 3.0).abs() < 1e-3);
        assert!((m.clips[0].end - 0.5).abs() < 1e-9);
        let (_, s) = wav(&m.clips[1].path);
        let g = 10f32.powf(m.gain_db.unwrap() as f32 / 20.0);
        let ours: Vec<f32> = std::iter::repeat(0.0).take(2 * 4000).chain(s.iter().map(|x| x * g)).collect();
        let (_, mix) = wav(fx("music_mix.wav").to_str().unwrap()); // exported from Audacity with this track unmuted and soloed
        assert_eq!(ours.len(), mix.len());
        let diff = ours.iter().zip(&mix).fold(0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(diff < 1e-3, "music (envelope + pan) differs from Audacity's export by {diff}");
        assert_eq!(r.labels, vec![ALabel { t0: 0.25, t1: 0.25, title: "Intro".into(), track: "Labels 1".into() }, ALabel { t0: 2.5, t1: 3.0, title: "Outro".into(), track: "Labels 1".into() }]);
    }

    #[test]
    fn imports_real_24_bit_and_float_projects() {
        for (f, bits) in [("fixture24.aup3", 24), ("fixture32f.aup3", 32)] {
            let r = import(&fx(f), &out(f), &mut |_| {}).unwrap();
            assert_eq!(r.tracks.len(), 1);
            let (spec, s) = wav(&r.tracks[0].clips[0].path);
            assert_eq!((spec.bits_per_sample, spec.channels, s.len()), (bits, 1, 8000));
            // 500 Hz square at 0.9: every sample is ±0.9 (Audacity's generator may soften the very first sample)
            // the same samples Audacity stored (int24 / float), checked via the summary columns it wrote
            let c = open_readonly(&fx(f)).unwrap();
            let (mn, mx): (f64, f64) = c.query_row("SELECT summin, summax FROM sampleblocks", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            let lo = s.iter().cloned().fold(f32::MAX, f32::min) as f64; let hi = s.iter().cloned().fold(f32::MIN, f32::max) as f64;
            assert!((lo - mn).abs() < 1e-6 && (hi - mx).abs() < 1e-6, "{f}: {lo}..{hi} vs {mn}..{mx}");
            assert!(peak(&s) > 0.85, "{f}");
            assert!(r.notes.is_empty(), "{:?}", r.notes);
        }
    }
}
