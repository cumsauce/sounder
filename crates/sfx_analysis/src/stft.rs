//! Short-time spectra.

use realfft::RealFftPlanner;

/// Power spectra of Hann-windowed frames centred every `hop` samples (frame
/// k is centred on sample k * hop). Bin powers are scaled so they sum to a
/// signal's mean-square power: a sine of amplitude A sums to A^2 / 2 over its
/// bins, so a full-scale sine reads -3 dB.
#[derive(Debug, Clone)]
pub struct Spectra {
    pub sr: u32,
    pub win: usize,
    pub hop: usize,
    /// frames[k][bin], bins 0..=win/2.
    pub frames: Vec<Vec<f32>>,
}

impl Spectra {
    pub fn bins(&self) -> usize {
        self.win / 2 + 1
    }

    /// Hz of `bin`.
    pub fn hz(&self, bin: usize) -> f32 {
        bin as f32 * self.sr as f32 / self.win as f32
    }

    /// Bin nearest to `hz`.
    pub fn bin(&self, hz: f32) -> usize {
        ((hz * self.win as f32 / self.sr as f32).round().max(0.0) as usize).min(self.bins() - 1)
    }

    /// Seconds of frame `k`'s centre.
    pub fn time(&self, k: usize) -> f32 {
        (k * self.hop) as f32 / self.sr as f32
    }

    /// Frames centred in [t0, t1) seconds.
    pub fn frames_in(&self, t0: f32, t1: f32) -> std::ops::Range<usize> {
        let a = ((t0 * self.sr as f32 / self.hop as f32).ceil().max(0.0) as usize)
            .min(self.frames.len());
        let b = ((t1 * self.sr as f32 / self.hop as f32).ceil().max(0.0) as usize)
            .min(self.frames.len());
        a..b.max(a)
    }

    /// The mean power spectrum of frames `r` (all frames when empty).
    pub fn mean(&self, r: std::ops::Range<usize>) -> Vec<f32> {
        let r = if r.is_empty() {
            0..self.frames.len()
        } else {
            r
        };
        let mut out = vec![0.0f32; self.bins()];
        let count = r.len().max(1) as f32;
        for f in &self.frames[r] {
            for (o, p) in out.iter_mut().zip(f) {
                *o += p / count;
            }
        }
        out
    }
}

/// The Hann window of `n` samples.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
        .collect()
}

/// Spectra of `x` with window `win` (a power of two) every `hop` samples.
pub fn stft(x: &[f32], sr: u32, win: usize, hop: usize) -> Spectra {
    let win = win.max(16);
    let hop = hop.max(1);
    let w = hann(win);
    let norm = 1.0 / (win as f32 * w.iter().map(|v| v * v).sum::<f32>());
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(win);
    let mut input = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    let count = x.len().div_ceil(hop).max(1);
    let half = win / 2;
    let mut frames = Vec::with_capacity(count);
    for k in 0..count {
        let c = (k * hop) as isize - half as isize;
        for (j, v) in input.iter_mut().enumerate() {
            let i = c + j as isize;
            *v = if i >= 0 && (i as usize) < x.len() {
                x[i as usize] * w[j]
            } else {
                0.0
            };
        }
        if fft.process(&mut input, &mut spec).is_err() {
            frames.push(vec![0.0; half + 1]);
            continue;
        }
        frames.push(
            spec.iter()
                .enumerate()
                .map(|(b, z)| {
                    let edge = b == 0 || b == half;
                    z.norm_sqr() * norm * if edge { 1.0 } else { 2.0 }
                })
                .collect(),
        );
    }
    Spectra {
        sr,
        win,
        hop,
        frames,
    }
}

/// dB of a power (floored at -120).
#[inline]
pub fn db(p: f32) -> f32 {
    10.0 * p.max(1e-12).log10()
}

/// Multi-resolution STFT distance between two sounds: the mean absolute
/// difference of their log spectra (windows 256, 1024 and 4096; levels
/// floored at -90 dB), in dB. 0 = identical; a few dB = audibly different.
/// The shorter sound is padded with silence.
pub fn distance(a: &[f32], b: &[f32], sr: u32) -> f32 {
    let n = a.len().max(b.len());
    let pad = |x: &[f32]| {
        let mut v = x.to_vec();
        v.resize(n, 0.0);
        v
    };
    let (a, b) = (pad(a), pad(b));
    let mut total = 0.0f64;
    let mut count = 0usize;
    for win in [256usize, 1024, 4096] {
        let (sa, sb) = (stft(&a, sr, win, win / 4), stft(&b, sr, win, win / 4));
        let mut sum = 0.0f64;
        let mut k = 0usize;
        for (fa, fb) in sa.frames.iter().zip(&sb.frames) {
            for (p, q) in fa.iter().zip(fb) {
                let (x, y) = (db(*p).max(-90.0), db(*q).max(-90.0));
                if x > -90.0 || y > -90.0 {
                    sum += f64::from((x - y).abs());
                    k += 1;
                }
            }
        }
        if k > 0 {
            total += sum / k as f64;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (total / count as f64) as f32
    }
}
