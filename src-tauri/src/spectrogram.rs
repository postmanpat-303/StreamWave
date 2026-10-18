//! Spectrogram of an edited clip list, for the per-track spectrogram view.
//! Output is compact binary (sent to the webview as an ArrayBuffer):
//! header of u32 LE [cols, bins, sample_rate, fft], then cols*bins bytes, column-major,
//! bin 0 = DC. Each byte maps dB from FLOOR_DB (0) to 0 dBFS (255).
use crate::waveform::Decoded;
use rustfft::{num_complex::Complex32, FftPlanner};

pub const FLOOR_DB: f32 = -100.0;

pub fn compute(d: &Decoded, cols: usize, fft: usize) -> Vec<u8> {
    let fft = fft.clamp(256, 8192).next_power_of_two();
    let bins = fft / 2;
    let ch = d.channels.max(1);
    let frames = d.samples.len() / ch;
    let cols = cols.clamp(1, 8000);
    let mut out = Vec::with_capacity(16 + cols * bins);
    for v in [cols as u32, bins as u32, d.sample_rate, fft as u32] { out.extend_from_slice(&v.to_le_bytes()); }
    let win: Vec<f32> = (0..fft).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / fft as f32).cos()).collect();
    // amplitude of a full-scale sine -> 0 dB
    let norm = 2.0 / win.iter().sum::<f32>();
    let plan = FftPlanner::<f32>::new().plan_fft_forward(fft);
    let mut buf = vec![Complex32::new(0.0, 0.0); fft];
    for c in 0..cols {
        let centre = ((c as f64 + 0.5) / cols as f64 * frames as f64) as i64;
        let s0 = centre - fft as i64 / 2;
        for (i, b) in buf.iter_mut().enumerate() {
            let f = s0 + i as i64;
            let x = if f >= 0 && (f as usize) < frames {
                let o = f as usize * ch; d.samples[o..o + ch].iter().sum::<f32>() / ch as f32
            } else { 0.0 };
            *b = Complex32::new(x * win[i], 0.0);
        }
        plan.process(&mut buf);
        for b in &buf[..bins] {
            let db = 20.0 * (b.norm() * norm).max(1e-12).log10();
            out.push((((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_sine_lights_up_its_own_bin() {
        let rate = 8000u32; let f = 1000.0f32;
        let samples: Vec<f32> = (0..rate as usize).flat_map(|i| { let x = 0.5 * (2.0 * std::f32::consts::PI * f * i as f32 / rate as f32).sin(); [x, x] }).collect();
        let d = Decoded { samples, channels: 2, sample_rate: rate };
        let v = compute(&d, 10, 1024);
        let u = |o: usize| u32::from_le_bytes(v[o..o + 4].try_into().unwrap());
        assert_eq!((u(0), u(4), u(8), u(12)), (10, 512, 8000, 1024));
        assert_eq!(v.len(), 16 + 10 * 512);
        let col = &v[16 + 5 * 512..16 + 6 * 512];
        let peak = (0..512).max_by_key(|&i| col[i]).unwrap();
        assert_eq!(peak, (1000.0 / (8000.0 / 1024.0)) as usize); // bin 128
        // 0.5 amplitude = -6 dB -> about 240; far bins are near the floor
        assert!((col[peak] as i32 - 240).abs() <= 3, "{}", col[peak]);
        assert!(col[400] < 60, "{}", col[400]);
    }
    #[test]
    fn empty_and_tiny_inputs_are_safe() {
        let d = Decoded { samples: vec![], channels: 1, sample_rate: 44100 };
        assert_eq!(compute(&d, 3, 100).len(), 16 + 3 * 128);
    }
}
