//! Decode any symphonia-supported file and build min/max peak summaries for drawing.
use serde::Serialize;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Clone)]
pub struct Decoded { pub samples: Vec<f32>, pub channels: usize, pub sample_rate: u32 }

pub fn decode_file(path: &Path) -> anyhow::Result<Decoded> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) { hint.with_extension(ext); }
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| anyhow::anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(48000);
    let mut channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id { continue }
        match dec.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                sample_rate = spec.rate; channels = spec.channels.count();
                let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
                sb.copy_interleaved_ref(buf);
                samples.extend_from_slice(sb.samples());
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip corrupt packet
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Decoded { samples, channels: channels.max(1), sample_rate })
}

#[derive(Serialize)]
pub struct Waveform {
    pub path: String,
    pub name: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_secs: f64,
    /// interleaved [min, max] pairs, one per bucket (all channels mixed)
    pub peaks: Vec<f32>,
}

pub fn peaks(d: &Decoded, buckets: usize) -> Vec<f32> {
    let frames = d.samples.len() / d.channels;
    let buckets = buckets.max(1).min(frames.max(1));
    let per = (frames as f64 / buckets as f64).max(1.0);
    let mut out = Vec::with_capacity(buckets * 2);
    for b in 0..buckets {
        let s = (b as f64 * per) as usize;
        let e = (((b + 1) as f64 * per) as usize).min(frames);
        let (mut mn, mut mx) = (0f32, 0f32);
        for f in s..e {
            for c in 0..d.channels {
                let v = d.samples[f * d.channels + c];
                mn = mn.min(v); mx = mx.max(v);
            }
        }
        out.push(mn); out.push(mx);
    }
    out
}

#[allow(dead_code)]
pub fn analyze(path: &Path, buckets: usize) -> anyhow::Result<Waveform> { Ok(waveform_of(path, &decode_file(path)?, buckets)) }
/// Overview of already-decoded audio (import decodes through the playback cache, so the first Play is warm).
pub fn waveform_of(path: &Path, d: &Decoded, buckets: usize) -> Waveform {
    Waveform {
        path: path.to_string_lossy().into(),
        name: path.file_name().map(|n| n.to_string_lossy().into()).unwrap_or_default(),
        sample_rate: d.sample_rate, channels: d.channels,
        duration_secs: (d.samples.len() / d.channels) as f64 / d.sample_rate as f64,
        peaks: peaks(d, buckets),
    }
}
