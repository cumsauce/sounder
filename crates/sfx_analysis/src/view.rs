//! The bulk data the window draws: the same spectrogram, waveform and
//! loudness the `listen` image shows, as numbers instead of pixels, so the
//! human sees what the agent sees.
//!
//! Every function takes a time range in seconds (`t0..t1`) and returns
//! exactly the requested number of columns. The binary encoders write the
//! formats in `design/window-api.md`: little-endian, no compression.

use sfx_graph::{Audio, loudness};

use crate::raster::colormap;
use crate::stft::{self, db};

/// Floor of the spectrogram grid, dB.
pub const DB_FLOOR: f32 = -120.0;
/// The graph card's mini waveform has this many samples.
pub const SPARK_LEN: usize = 92;
/// Columns / rows are capped so a bad query cannot ask for gigabytes.
pub const MAX_DIM: usize = 4096;

/// A time range in seconds clamped into the sound and made non-empty.
pub fn time_range(audio: &Audio, t0: Option<f32>, t1: Option<f32>) -> (f32, f32) {
    let total = audio.seconds().max(1.0 / audio.sample_rate.max(1) as f32);
    let a = t0.unwrap_or(0.0).clamp(0.0, total);
    let b = t1.unwrap_or(total).clamp(0.0, total);
    if b > a + 1e-4 { (a, b) } else { (a, a + 1e-3) }
}

/// A frequency range clamped to `[1 Hz, Nyquist]`, default 20 Hz..20 kHz.
pub fn hz_range(audio: &Audio, hz0: Option<f32>, hz1: Option<f32>) -> (f32, f32) {
    let nyq = audio.sample_rate as f32 / 2.0;
    let f0 = hz0.unwrap_or(20.0).max(1.0);
    let f1 = hz1.unwrap_or(20_000.0f32.min(nyq)).min(nyq).max(f0 * 1.01);
    (f0, f1)
}

/// dB (-120..0) per cell of a `w` x `h` grid: row 0 is the highest
/// frequency, rows log-spaced between `hz0` and `hz1`, columns evenly in
/// time from `t0` to `t1`. Same window and hop rule as the `listen` image.
/// Cells past the end of the sound read [`DB_FLOOR`].
pub fn spectrogram_grid(
    audio: &Audio,
    w: usize,
    h: usize,
    t0: f32,
    t1: f32,
    hz0: f32,
    hz1: f32,
) -> Vec<f32> {
    let (w, h) = (w.clamp(1, MAX_DIM), h.clamp(1, MAX_DIM));
    let mut grid = vec![DB_FLOOR; w * h];
    let sr = audio.sample_rate;
    let mono = audio.mix_mono();
    let i0 = ((t0.max(0.0) * sr as f32) as usize).min(mono.len());
    let i1 = ((t1 * sr as f32).ceil() as usize).min(mono.len()).max(i0);
    let seg = &mono[i0..i1];
    if seg.is_empty() {
        return grid;
    }
    let nyq = sr as f32 / 2.0;
    let (f0, f1) = (hz0.max(1.0), hz1.min(nyq).max(hz0.max(1.0) * 1.01));
    let win = crate::descriptors::window_for(seg.len(), sr);
    let span = (seg.len() as f32 / w as f32).max(1.0);
    let hop = (span as usize).min(win / 4).max(1);
    let spec = stft::stft(seg, sr, win, hop);
    let y_hz = |y: usize| f0 * (f1 / f0).powf(1.0 - y as f32 / h as f32);
    for x in 0..w {
        let a0 = (x as f32 * span / hop as f32) as usize;
        let a1 = (((x + 1) as f32 * span / hop as f32) as usize)
            .max(a0 + 1)
            .min(spec.frames.len());
        if a0 >= spec.frames.len() {
            break;
        }
        let avg = spec.mean(a0..a1);
        for y in 0..h {
            let (hi, lo) = (y_hz(y), y_hz(y + 1));
            let (b0, b1) = (spec.bin(lo), spec.bin(hi));
            let p = if b1 > b0 + 1 {
                avg[b0..b1].iter().copied().fold(0.0, f32::max)
            } else {
                let fb = 0.5 * (lo + hi) * spec.win as f32 / sr as f32;
                let k = (fb.floor() as usize).min(avg.len().saturating_sub(2));
                let f = fb - k as f32;
                avg[k] + (avg[k + 1] - avg[k]) * f
            };
            grid[y * w + x] = db(p).clamp(DB_FLOOR, 0.0);
        }
    }
    grid
}

/// Per column `(min, max, rms)` of the mono mix between `t0` and `t1`.
/// Columns past the end of the sound are zero.
pub fn wave_columns(audio: &Audio, w: usize, t0: f32, t1: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let w = w.clamp(1, MAX_DIM);
    let (mut mn, mut mx, mut rms) = (vec![0.0; w], vec![0.0; w], vec![0.0; w]);
    let sr = audio.sample_rate as f32;
    let mono = audio.mix_mono();
    let i0 = ((t0.max(0.0) * sr) as usize).min(mono.len());
    let i1 = ((t1 * sr).ceil() as usize).min(mono.len()).max(i0);
    let n = i1 - i0;
    for x in 0..w {
        let s0 = i0 + x * n / w;
        let s1 = (i0 + (x + 1) * n / w).max(s0 + 1).min(i1);
        if s0 >= s1 {
            continue;
        }
        let chunk = &mono[s0..s1];
        let (lo, hi) = chunk
            .iter()
            .fold((0.0f32, 0.0f32), |m, v| (m.0.min(*v), m.1.max(*v)));
        mn[x] = lo;
        mx[x] = hi;
        rms[x] = (chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32).sqrt();
    }
    (mn, mx, rms)
}

/// Loudness (LUFS, floored at -90) per column. The window is a quarter of
/// the whole sound, 20..400 ms, like the `listen` image; column `x` reads
/// the curve at `t0 + x * (t1 - t0) / w`.
pub fn loudness_columns(audio: &Audio, w: usize, t0: f32, t1: f32) -> Vec<f32> {
    let w = w.clamp(1, MAX_DIM);
    if audio.is_empty() {
        return vec![-90.0; w];
    }
    let total = f64::from(audio.seconds().max(1.0 / audio.sample_rate as f32));
    let window = (total / 4.0).clamp(0.02, 0.4);
    let hop_s = (f64::from(t1 - t0) / w as f64).max(0.001);
    let curve = loudness::curve(&audio.channels, audio.sample_rate, window, hop_s);
    (0..w)
        .map(|x| {
            // Position in curve points (the curve is sampled every hop_s).
            let pos = (f64::from(t0) + x as f64 * f64::from(t1 - t0) / w as f64) / hop_s;
            let k = pos.floor().max(0.0) as usize;
            let f = (pos - pos.floor()) as f32;
            let a = curve.get(k).copied().unwrap_or(-90.0);
            let b = curve.get(k + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}

/// The graph card's mini waveform: the peak of the mono mix in
/// [`SPARK_LEN`] equal slices.
pub fn spark(audio: &Audio) -> [f32; SPARK_LEN] {
    let mut out = [0.0f32; SPARK_LEN];
    let mono = audio.mix_mono();
    if mono.is_empty() {
        return out;
    }
    for (k, o) in out.iter_mut().enumerate() {
        let a = k * mono.len() / SPARK_LEN;
        let b = ((k + 1) * mono.len() / SPARK_LEN).max(a + 1).min(mono.len());
        if a < b {
            *o = mono[a..b].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        }
    }
    out
}

/// The spectrogram colour map: 256 RGBA entries, index 0 = -90 dB (black),
/// 255 = 0 dB (pale yellow).
pub fn colormap_rgba() -> [u8; 1024] {
    let mut out = [0u8; 1024];
    for k in 0..256 {
        out[k * 4..k * 4 + 4].copy_from_slice(&colormap(k as f32 / 255.0));
    }
    out
}

/// Little-endian bytes of `values`.
pub fn encode_f32(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// The spectrogram wire format: `u32 w, u32 h, f32 t0, f32 t1`, then
/// `w * h` `f32` dB values.
pub fn encode_spectrogram(w: usize, h: usize, t0: f32, t1: f32, grid: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + grid.len() * 4);
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&(h as u32).to_le_bytes());
    out.extend_from_slice(&t0.to_le_bytes());
    out.extend_from_slice(&t1.to_le_bytes());
    out.extend_from_slice(&encode_f32(grid));
    out
}

/// The wave wire format: `3 * w` `f32`, the minima, then the maxima, then
/// the RMS values.
pub fn encode_wave(cols: &(Vec<f32>, Vec<f32>, Vec<f32>)) -> Vec<u8> {
    let mut all = Vec::with_capacity(cols.0.len() * 3);
    all.extend_from_slice(&cols.0);
    all.extend_from_slice(&cols.1);
    all.extend_from_slice(&cols.2);
    encode_f32(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, secs: f32, amp: f32) -> Audio {
        let sr = 48_000;
        let n = (secs * sr as f32) as usize;
        Audio::mono(
            sr,
            (0..n)
                .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / sr as f32).sin())
                .collect(),
        )
    }

    fn f32_at(b: &[u8], i: usize) -> f32 {
        f32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
    }

    #[test]
    fn grid_size_and_range() {
        let a = sine(1000.0, 0.5, 0.5);
        let (w, h) = (120, 80);
        let g = spectrogram_grid(&a, w, h, 0.0, 0.5, 20.0, 20_000.0);
        assert_eq!(g.len(), w * h);
        assert!(g.iter().all(|v| (DB_FLOOR..=0.0).contains(v)));
    }

    #[test]
    fn grid_peak_row_matches_the_sine() {
        let a = sine(1000.0, 0.5, 0.5);
        let (w, h) = (100, 200);
        let g = spectrogram_grid(&a, w, h, 0.0, 0.5, 20.0, 20_000.0);
        let x = w / 2;
        let row = (0..h)
            .max_by(|p, q| g[p * w + x].total_cmp(&g[q * w + x]))
            .unwrap_or(0);
        // Row y spans y_hz(y+1)..y_hz(y); the row centre's frequency.
        let hz = 20.0 * (20_000.0f32 / 20.0).powf(1.0 - (row as f32 + 0.5) / h as f32);
        assert!((hz / 1000.0 - 1.0).abs() < 0.08, "peak row at {hz} Hz");
        // A -6 dBFS sine reads about -9 dB (power sums to A^2/2).
        assert!(g[row * w + x] > -14.0, "{}", g[row * w + x]);
        // Far from the tone it is quiet.
        let quiet = g[(h - 5) * w + x];
        assert!(quiet < -60.0, "{quiet}");
    }

    #[test]
    fn grid_zoom_and_silence() {
        let a = sine(440.0, 0.2, 0.5);
        // Past the end of the sound: floor everywhere.
        let g = spectrogram_grid(&a, 10, 10, 5.0, 6.0, 20.0, 20_000.0);
        assert!(g.iter().all(|v| *v == DB_FLOOR));
        // A band zoom around 440 Hz puts the peak in the middle.
        let g = spectrogram_grid(&a, 20, 60, 0.0, 0.2, 300.0, 700.0);
        let row = (0..60)
            .max_by(|p, q| g[p * 20 + 10].total_cmp(&g[q * 20 + 10]))
            .unwrap_or(0);
        let hz = 300.0 * (700.0f32 / 300.0).powf(1.0 - (row as f32 + 0.5) / 60.0);
        assert!((hz - 440.0).abs() < 30.0, "{hz}");
    }

    #[test]
    fn wave_columns_bound_the_signal() {
        let a = sine(100.0, 0.25, 0.5);
        let (mn, mx, rms) = wave_columns(&a, 50, 0.0, 0.25);
        assert_eq!((mn.len(), mx.len(), rms.len()), (50, 50, 50));
        for x in 0..50 {
            assert!(mn[x] >= -0.5 - 1e-4 && mx[x] <= 0.5 + 1e-4);
            assert!(mn[x] <= 0.0 && mx[x] >= 0.0);
            assert!(rms[x] <= mx[x].max(-mn[x]) + 1e-4);
        }
        assert!(mx.iter().cloned().fold(0.0, f32::max) > 0.49);
        assert!(rms.iter().all(|v| *v > 0.2), "a sine's rms is ~0.35");
        // Beyond the end: zeros.
        let (mn, mx, _) = wave_columns(&a, 4, 1.0, 2.0);
        assert!(mn.iter().chain(&mx).all(|v| *v == 0.0));
    }

    #[test]
    fn loudness_columns_match_listen_curve() {
        let mut a = sine(1000.0, 0.8, 0.5);
        // Second half silent so the curve moves.
        let half = a.channels[0].len() / 2;
        for v in &mut a.channels[0][half..] {
            *v = 0.0;
        }
        let w = 100;
        let cols = loudness_columns(&a, w, 0.0, 0.8);
        assert_eq!(cols.len(), w);
        // The listen image's curve, same window rule.
        let total = f64::from(a.seconds());
        let window = (total / 4.0).clamp(0.02, 0.4);
        let hop = 0.8 / w as f64;
        let curve = loudness::curve(&a.channels, a.sample_rate, window, hop);
        for x in [0, 10, 25, 49] {
            assert!(
                (cols[x] - curve[x]).abs() < 0.01,
                "{x}: {} vs {}",
                cols[x],
                curve[x]
            );
        }
        assert!(cols[10] > -20.0 && cols[90] < -60.0);
        assert!(cols.iter().all(|v| (-90.0..=0.0).contains(v)));
    }

    #[test]
    fn spark_is_the_peak_envelope() {
        let mut a = sine(200.0, 0.46, 0.2);
        let n = a.channels[0].len();
        a.channels[0][n - 10] = 0.9;
        let s = spark(&a);
        assert_eq!(s.len(), 92);
        assert!(s[0] > 0.15 && s[0] <= 0.2 + 1e-4);
        assert!((s[91] - 0.9).abs() < 1e-6);
        assert_eq!(spark(&Audio::mono(48_000, vec![])), [0.0; 92]);
    }

    #[test]
    fn colormap_is_256_rgba() {
        let m = colormap_rgba();
        assert_eq!(m.len(), 1024);
        assert_eq!(&m[0..4], &colormap(0.0));
        assert_eq!(&m[1020..1024], &colormap(1.0));
        assert_eq!(m[3], 255);
    }

    #[test]
    fn encoders_write_the_contract_formats() {
        let g = vec![-10.0, -20.0, -30.0, -40.0, -50.0, -60.0];
        let b = encode_spectrogram(3, 2, 0.25, 1.5, &g);
        assert_eq!(b.len(), 16 + 24);
        assert_eq!(u32::from_le_bytes([b[0], b[1], b[2], b[3]]), 3);
        assert_eq!(u32::from_le_bytes([b[4], b[5], b[6], b[7]]), 2);
        assert_eq!(f32_at(&b, 2), 0.25);
        assert_eq!(f32_at(&b, 3), 1.5);
        assert_eq!(f32_at(&b, 4), -10.0);
        assert_eq!(f32_at(&b, 9), -60.0);
        let w = encode_wave(&(vec![-1.0, -2.0], vec![1.0, 2.0], vec![0.5, 0.25]));
        assert_eq!(w.len(), 24);
        assert_eq!(f32_at(&w, 0), -1.0);
        assert_eq!(f32_at(&w, 3), 2.0);
        assert_eq!(f32_at(&w, 5), 0.25);
    }
}
