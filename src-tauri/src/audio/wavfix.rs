//! Crash-safe recording helpers. Recordings are streamed to `*.partial.wav` and the
//! header is refreshed periodically. If the app dies, `recover_dir` patches the RIFF/data
//! sizes from the real file length and renames the file to `*.recovered.wav`.
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub fn fix_header(path: &Path) -> anyhow::Result<()> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    let mut hdr = [0u8; 12];
    f.read_exact(&mut hdr)?;
    anyhow::ensure!(&hdr[0..4] == b"RIFF" && &hdr[8..12] == b"WAVE", "not a WAV file");
    // walk chunks to find "data"
    let mut pos = 12u64;
    let mut block_align = 1u64;
    loop {
        anyhow::ensure!(pos + 8 <= len, "no data chunk");
        f.seek(SeekFrom::Start(pos))?;
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch)?;
        let size = u32::from_le_bytes(ch[4..8].try_into()?) as u64;
        if &ch[0..4] == b"fmt " {
            let mut fmt = [0u8; 16];
            f.read_exact(&mut fmt)?;
            block_align = u16::from_le_bytes([fmt[12], fmt[13]]).max(1) as u64;
        }
        if &ch[0..4] == b"data" {
            let mut data_len = len - (pos + 8);
            data_len -= data_len % block_align; // drop a torn final frame
            f.seek(SeekFrom::Start(pos + 4))?;
            f.write_all(&(data_len.min(u32::MAX as u64) as u32).to_le_bytes())?;
            f.seek(SeekFrom::Start(4))?;
            f.write_all(&((pos + 8 + data_len - 8).min(u32::MAX as u64) as u32).to_le_bytes())?;
            f.set_len(pos + 8 + data_len)?;
            f.flush()?;
            return Ok(());
        }
        pos += 8 + size + (size & 1);
    }
}

/// Repair every leftover partial recording in `dir`. Returns the recovered paths.
pub fn recover_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if name.ends_with(".partial.wav") && fix_header(&p).is_ok() {
            let newp = p.with_file_name(name.replace(".partial.wav", ".recovered.wav"));
            if std::fs::rename(&p, &newp).is_ok() { out.push(newp) }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn recovers_unfinalized_wav() {
        let dir = std::env::temp_dir().join("tw_fix_test"); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.partial.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 48000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for i in 0..9600 { w.write_sample((i as f32 / 100.0).sin()).unwrap(); }
        w.flush().unwrap();
        std::mem::forget(w); // simulate a crash: no finalize
        // append unflushed garbage + a torn frame
        use std::io::Write; std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(&[0u8; 8 * 100 + 3]).unwrap();
        let out = super::recover_dir(&dir);
        assert_eq!(out.len(), 1);
        let d = crate::waveform::decode_file(&out[0]).unwrap();
        assert_eq!(d.samples.len(), 9600 + 200);
        assert!(crate::waveform::peaks(&d, 100).len() == 200);
    }
}
