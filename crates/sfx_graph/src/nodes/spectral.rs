//! Offline pitch, time and spectral nodes: a phase vocoder (Hann windows, 75 %
//! overlap, identity phase locking) and spectral edits on the same STFT.
//! All are acausal; every output has exactly the document's length.

use std::f32::consts::TAU;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, In, Outs};

type Spec = Vec<Complex<f32>>;

const DEFAULT_WINDOW: usize = 2048;

/// Validates a window size: a power of two in 64..=16384.
fn window_size(w: Option<usize>) -> Result<usize, String> {
    let n = w.unwrap_or(DEFAULT_WINDOW);
    if (64..=16384).contains(&n) && n.is_power_of_two() {
        Ok(n)
    } else {
        Err(format!(
            "window: {n}: give a power of two from 64 to 16384 (default {DEFAULT_WINDOW})"
        ))
    }
}

/// Windowed FFT frames over a padded signal, and the way back.
struct Stft {
    n: usize,
    hop: usize,
    win: Vec<f32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
}

impl Stft {
    fn new(n: usize) -> Stft {
        let mut planner = RealFftPlanner::<f32>::new();
        Stft {
            n,
            hop: n / 4,
            win: (0..n)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos())
                .collect(),
            fwd: planner.plan_fft_forward(n),
            inv: planner.plan_fft_inverse(n),
        }
    }

    /// Frames start at `k * hop` in the signal padded with `n` zeros in
    /// front; frame `k` is centred on real sample `k * hop - n / 2`.
    fn frames_for(&self, len: usize) -> usize {
        (len + self.n) / self.hop + 1
    }

    fn analyze(&self, x: &[f32]) -> Result<Vec<Spec>, String> {
        let mut padded = vec![0.0f32; self.n + x.len() + 2 * self.n];
        padded[self.n..self.n + x.len()].copy_from_slice(x);
        let k = self.frames_for(x.len());
        let mut out = Vec::with_capacity(k);
        let mut buf = vec![0.0f32; self.n];
        for f in 0..k {
            let s = f * self.hop;
            for i in 0..self.n {
                buf[i] = padded[s + i] * self.win[i];
            }
            let mut spec = self.fwd.make_output_vec();
            self.fwd
                .process(&mut buf, &mut spec)
                .map_err(|e| format!("fft: {e}"))?;
            out.push(spec);
        }
        Ok(out)
    }

    /// The windowed time frame of `spec` (consumed as scratch).
    fn frame(&self, spec: &mut Spec) -> Result<Vec<f32>, String> {
        if let Some(c) = spec.first_mut() {
            c.im = 0.0;
        }
        if let Some(c) = spec.last_mut() {
            c.im = 0.0;
        }
        let mut t = self.inv.make_output_vec();
        self.inv
            .process(spec, &mut t)
            .map_err(|e| format!("fft: {e}"))?;
        let g = 1.0 / self.n as f32;
        for (v, w) in t.iter_mut().zip(&self.win) {
            *v *= g * w;
        }
        Ok(t)
    }

    /// Overlap-adds `specs` at spacing `hs`, normalizes by the summed squared
    /// window, and returns `len` real samples starting at synthesis index
    /// `off`. `gain(k)` scales frame `k`.
    fn synth(
        &self,
        mut specs: Vec<Spec>,
        hs: usize,
        off: usize,
        len: usize,
        gain: impl Fn(usize) -> f32,
    ) -> Result<Vec<f32>, String> {
        let total = specs.len().saturating_sub(1) * hs + self.n;
        let mut out = vec![0.0f32; total];
        let mut norm = vec![0.0f32; total];
        for (k, spec) in specs.iter_mut().enumerate() {
            let t = self.frame(spec)?;
            let g = gain(k);
            for i in 0..self.n {
                out[k * hs + i] += t[i] * g;
                norm[k * hs + i] += self.win[i] * self.win[i];
            }
        }
        Ok((0..len)
            .map(|j| {
                let i = j + off;
                if i < total {
                    out[i] / norm[i].max(1e-4)
                } else {
                    0.0
                }
            })
            .collect())
    }

    /// Gain that restores the level of a frame given random phases:
    /// sqrt(sum w^2 / mean w^2).
    fn random_phase_gain(&self) -> f32 {
        let sumsq: f32 = self.win.iter().map(|w| w * w).sum();
        let mean = sumsq / self.n as f32;
        let per_hop = sumsq / self.hop as f32;
        (per_hop / mean).sqrt()
    }
}

fn wrap(x: f32) -> f32 {
    x - TAU * (x / TAU).round()
}

/// Phase vocoder: stretches `x` by `hs / ha` (frames analysed `ha` apart,
/// resynthesised `hs` apart) with identity phase locking. Bins from
/// `cutoff` up are dropped. Returns `len` samples, zero-padded.
fn stretch(st: &Stft, x: &[f32], hs: usize, cutoff: usize, len: usize) -> Result<Vec<f32>, String> {
    let ha = st.hop;
    let n = st.n;
    let mut frames = st.analyze(x)?;
    let bins = n / 2 + 1;
    let mut prev_ana = vec![0.0f32; bins];
    let mut syn = vec![0.0f32; bins];
    let mut ana = vec![0.0f32; bins];
    let mut mag = vec![0.0f32; bins];
    for (f, spec) in frames.iter_mut().enumerate() {
        for k in 0..bins {
            mag[k] = spec[k].norm();
            ana[k] = spec[k].arg();
        }
        // Peaks: local maxima over +-2 bins.
        let peaks: Vec<usize> = (0..bins)
            .filter(|&k| {
                mag[k] > 0.0
                    && (k.saturating_sub(2)..(k + 3).min(bins)).all(|j| j == k || mag[j] < mag[k])
            })
            .collect();
        let prop = |k: usize, syn_prev: f32| {
            let omega = TAU * k as f32 / n as f32;
            let d = wrap(ana[k] - prev_ana[k] - omega * ha as f32);
            syn_prev + (omega + d / ha as f32) * hs as f32
        };
        if f == 0 {
            syn.copy_from_slice(&ana);
        } else if peaks.is_empty() {
            for k in 0..bins {
                syn[k] = wrap(prop(k, syn[k]));
            }
        } else {
            let mut next = syn.clone();
            let mut pi = 0;
            for k in 0..bins {
                while pi + 1 < peaks.len() && k > (peaks[pi] + peaks[pi + 1]) / 2 {
                    pi += 1;
                }
                let p = peaks[pi];
                if k == p {
                    next[k] = wrap(prop(p, syn[p]));
                }
            }
            pi = 0;
            for k in 0..bins {
                while pi + 1 < peaks.len() && k > (peaks[pi] + peaks[pi + 1]) / 2 {
                    pi += 1;
                }
                let p = peaks[pi];
                if k != p {
                    next[k] = next[p] + ana[k] - ana[p];
                }
            }
            syn = next;
        }
        for k in 0..bins {
            spec[k] = if k >= cutoff {
                Complex::new(0.0, 0.0)
            } else {
                Complex::from_polar(mag[k], syn[k])
            };
        }
        prev_ana.copy_from_slice(&ana);
    }
    let r = hs as f32 / ha as f32;
    // Real sample j of the output sits at synthesis index j + off, where
    // frame centres map x -> r * x.
    let off = ((n as f32 / 2.0) * (1.0 + r)).round() as usize;
    st.synth(frames, hs, off, len, |_| 1.0)
}

/// Catmull-Rom read of `x` at fractional position `p` (zero outside).
fn cubic(x: &[f32], p: f64) -> f32 {
    let i = p.floor();
    let t = (p - i) as f32;
    let i = i as isize;
    let at = |j: isize| {
        if j >= 0 && (j as usize) < x.len() {
            x[j as usize]
        } else {
            0.0
        }
    };
    let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

fn finite(v: Vec<f32>) -> Vec<f32> {
    v.into_iter()
        .map(|x| if x.is_finite() { x } else { 0.0 })
        .collect()
}

/// Median of a non-empty slice (reorders it).
fn median(v: &mut [f32]) -> f32 {
    let m = v.len() / 2;
    *v.select_nth_unstable_by(m, f32::total_cmp).1
}

/// A real-FFT length for whole-signal processing: at least `2 n + extra`,
/// a power of two, so circular wrap-around stays in the zero padding.
fn big_len(n: usize, extra: usize) -> usize {
    (2 * n + extra).max(1024).next_power_of_two()
}

/// Largest `|time|` (seconds) `smear` takes.
const MAX_SMEAR_S: f32 = 2.0;
/// Median kernel limits for `hps`, in frames and bins.
const MAX_KERNEL: usize = 101;
const DEFAULT_KERNEL: usize = 17;
/// Largest `inharmonicity` `stretch` takes.
const MAX_INHARM: f32 = 2.0;

/// Frequency shift: every component moves by `hz[i]` Hz (single sideband:
/// the analytic signal times a complex exponential). Components that would
/// leave 0..Nyquist are removed before the shift, so nothing folds.
fn shift_channel(x: &[f32], hz: &[f32], sr: f32) -> Result<Vec<f32>, String> {
    let n = x.len();
    let m = big_len(n, 0);
    let mut planner = RealFftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(m), planner.plan_fft_inverse(m));
    let mut buf = vec![0.0f32; m];
    buf[..n].copy_from_slice(x);
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec)
        .map_err(|e| format!("fft: {e}"))?;
    let (hmin, hmax) = hz
        .iter()
        .fold((0.0f32, 0.0f32), |(lo, hi), &h| (lo.min(h), hi.max(h)));
    let nyq = sr / 2.0;
    let mut re = spec.clone();
    let mut hil = spec;
    let last = re.len() - 1;
    for k in 0..=last {
        let f = k as f32 * sr / m as f32;
        let keep = f + hmax < nyq && f + hmin >= 0.0;
        if !keep {
            re[k] = Complex::new(0.0, 0.0);
            hil[k] = Complex::new(0.0, 0.0);
        } else if k == 0 || k == last {
            re[k].im = 0.0;
            hil[k] = Complex::new(0.0, 0.0);
        } else {
            // Hilbert: multiply by -j.
            hil[k] = Complex::new(hil[k].im, -hil[k].re);
        }
    }
    let g = 1.0 / m as f32;
    let mut xb = vec![0.0f32; m];
    let mut hb = vec![0.0f32; m];
    inv.process(&mut re, &mut xb)
        .map_err(|e| format!("fft: {e}"))?;
    inv.process(&mut hil, &mut hb)
        .map_err(|e| format!("fft: {e}"))?;
    let mut phase = 0.0f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (s, c) = phase.sin_cos();
        out.push((xb[i] as f64 * c - hb[i] as f64 * s) as f32 * g);
        phase += std::f64::consts::TAU * hz[i] as f64 / sr as f64;
        phase -= std::f64::consts::TAU * (phase / std::f64::consts::TAU).floor();
    }
    Ok(out)
}

/// Dispersion: an allpass whose group delay rises linearly from 0 at DC to
/// `time` seconds at Nyquist (negative: falls). Magnitudes are untouched.
fn smear_channel(x: &[f32], time: f32, sr: f32) -> Result<Vec<f32>, String> {
    let n = x.len();
    let extra = (time.abs() * sr).ceil() as usize + 1;
    let m = big_len(n, extra);
    let mut planner = RealFftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(m), planner.plan_fft_inverse(m));
    let mut buf = vec![0.0f32; m];
    buf[..n].copy_from_slice(x);
    let mut spec = fwd.make_output_vec();
    fwd.process(&mut buf, &mut spec)
        .map_err(|e| format!("fft: {e}"))?;
    let last = spec.len() - 1;
    for (k, z) in spec.iter_mut().enumerate() {
        let r = k as f64 / m as f64;
        let theta = -std::f64::consts::TAU * f64::from(time) * r * r * f64::from(sr);
        let rot = Complex::new(theta.cos() as f32, theta.sin() as f32);
        *z *= rot;
        if k == 0 || k == last {
            z.im = 0.0;
        }
    }
    inv.process(&mut spec, &mut buf)
        .map_err(|e| format!("fft: {e}"))?;
    let g = 1.0 / m as f32;
    Ok(buf[..n].iter().map(|v| v * g).collect())
}

/// Inharmonic warp: each spectral peak moves from `f` to
/// `f * sqrt(1 + b * (f / f0)^2)` (the stiff-string law: partial n of a
/// harmonic series at `f0` lands at `n f0 sqrt(1 + b n^2)`), keeping each
/// peak's phase advance at the new frequency (peak picking, identity phase
/// locking, bins moved by whole-bin shifts). Peaks warped past Nyquist are
/// dropped.
fn warp(st: &Stft, x: &[f32], sr: f32, b: f32, f0: f32, len: usize) -> Result<Vec<f32>, String> {
    let (n, ha) = (st.n, st.hop);
    let mut frames = st.analyze(x)?;
    let bins = n / 2 + 1;
    let mut prev_ana = vec![0.0f32; bins];
    let mut syn = vec![0.0f32; bins];
    let mut ana = vec![0.0f32; bins];
    let mut mag = vec![0.0f32; bins];
    for (f, spec) in frames.iter_mut().enumerate() {
        for k in 0..bins {
            mag[k] = spec[k].norm();
            ana[k] = spec[k].arg();
        }
        let peaks: Vec<usize> = (0..bins)
            .filter(|&k| {
                mag[k] > 0.0
                    && (k.saturating_sub(2)..(k + 3).min(bins)).all(|j| j == k || mag[j] < mag[k])
            })
            .collect();
        let mut next = vec![0.0f32; bins];
        let mut out_mag = vec![0.0f32; bins];
        let mut out_ph = vec![0.0f32; bins];
        for (pi, &p) in peaks.iter().enumerate() {
            let lo = if pi == 0 {
                0
            } else {
                (peaks[pi - 1] + p) / 2 + 1
            };
            let hi = if pi + 1 == peaks.len() {
                bins - 1
            } else {
                (p + peaks[pi + 1]) / 2
            };
            let wp = TAU * p as f32 / n as f32;
            let w_in = if f == 0 {
                wp
            } else {
                wp + wrap(ana[p] - prev_ana[p] - wp * ha as f32) / ha as f32
            };
            let hz = f64::from(w_in) / std::f64::consts::TAU * f64::from(sr);
            let hz2 = if hz > 0.0 {
                let r = hz / f64::from(f0);
                hz * (1.0 + f64::from(b) * r * r).sqrt()
            } else {
                hz
            };
            let w_out = (hz2 / f64::from(sr) * std::f64::consts::TAU) as f32;
            let dropped = !(w_out.is_finite() && w_out < std::f32::consts::PI);
            let shift = if dropped {
                0
            } else {
                ((w_out - w_in) * n as f32 / TAU).round() as isize
            };
            let sp = if f == 0 || dropped {
                ana[p]
            } else {
                wrap(syn[p] + w_out * ha as f32)
            };
            for k in lo..=hi {
                let ph = wrap(sp + ana[k] - ana[p]);
                next[k] = ph;
                let t = k as isize + shift;
                if !dropped && t >= 0 && (t as usize) < bins && mag[k] > out_mag[t as usize] {
                    out_mag[t as usize] = mag[k];
                    out_ph[t as usize] = ph;
                }
            }
        }
        syn = if peaks.is_empty() { ana.clone() } else { next };
        for k in 0..bins {
            spec[k] = Complex::from_polar(out_mag[k], out_ph[k]);
        }
        prev_ana.copy_from_slice(&ana);
    }
    st.synth(frames, st.hop, st.n, len, |_| 1.0)
}

/// Harmonic / percussive separation (median filtering of the magnitude
/// spectrogram, Fitzgerald 2010): a median over time (`kernel` frames) keeps
/// steady partials, a median over frequency (`kernel` bins) keeps clicks.
/// Soft masks `h^2 / (h^2 + p^2)` and its complement, so the two outputs sum
/// to the input.
fn hps_channel(
    st: &Stft,
    x: &[f32],
    kernel: usize,
    len: usize,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    let frames = st.analyze(x)?;
    let bins = st.n / 2 + 1;
    let nf = frames.len();
    let r = kernel / 2;
    let mags: Vec<Vec<f32>> = frames
        .iter()
        .map(|f| f.iter().map(|z| z.norm()).collect())
        .collect();
    // Harmonic estimate: the median over time, bin by bin.
    let mut h = vec![vec![0.0f32; bins]; nf];
    let mut col = vec![0.0f32; nf];
    let mut scratch = Vec::with_capacity(kernel);
    for k in 0..bins {
        for m in 0..nf {
            col[m] = mags[m][k];
        }
        for m in 0..nf {
            scratch.clear();
            scratch.extend_from_slice(&col[m.saturating_sub(r)..=(m + r).min(nf - 1)]);
            h[m][k] = median(&mut scratch);
        }
    }
    // Percussive estimate: the median over frequency, frame by frame.
    let mut hf = frames.clone();
    let mut pf = frames;
    for m in 0..nf {
        for k in 0..bins {
            scratch.clear();
            scratch.extend_from_slice(&mags[m][k.saturating_sub(r)..=(k + r).min(bins - 1)]);
            let p = median(&mut scratch);
            let (hh, pp) = (h[m][k] * h[m][k], p * p);
            let den = hh + pp;
            let mh = if den > 1e-20 { hh / den } else { 0.5 };
            hf[m][k] *= mh;
            pf[m][k] *= 1.0 - mh;
        }
    }
    let harm = finite(st.synth(hf, st.hop, st.n, len, |_| 1.0)?);
    let perc = finite(st.synth(pf, st.hop, st.n, len, |_| 1.0)?);
    Ok((harm, perc))
}

/// One cepstral smoothing of a log-magnitude half spectrum (`n / 2 + 1`
/// points): the low `q` quefrencies kept, with a short half-cosine taper.
fn lifter(
    fwd: &Arc<dyn RealToComplex<f32>>,
    logmag: &[f32],
    n: usize,
    q: usize,
) -> Result<Vec<f32>, String> {
    let bins = n / 2 + 1;
    let mut l: Vec<f32> = (0..n).map(|j| logmag[j.min(n - j)]).collect();
    let mut c = fwd.make_output_vec();
    fwd.process(&mut l, &mut c)
        .map_err(|e| format!("fft: {e}"))?;
    // c is real (l is even): the cepstrum times n.
    let taper = (q / 4).max(1);
    let w = |i: usize| {
        if i < q - taper {
            1.0
        } else if i < q {
            0.5 + 0.5 * (std::f32::consts::PI * (i - (q - taper)) as f32 / taper as f32).cos()
        } else {
            0.0
        }
    };
    let mut lc = vec![0.0f32; n];
    for j in 0..n {
        let i = j.min(n - j);
        lc[j] = c[i].re * w(i);
    }
    let mut e = fwd.make_output_vec();
    fwd.process(&mut lc, &mut e)
        .map_err(|e| format!("fft: {e}"))?;
    Ok((0..bins).map(|k| e[k].re / n as f32).collect())
}

/// Iterations of the true-envelope refinement in `envelope`.
const ENVELOPE_ITERS: usize = 10;

/// "True envelope" of a magnitude spectrum: cepstral smoothing of the log
/// magnitude, then repeated with the spectrum raised to the envelope wherever
/// it fell below, so the curve rides the harmonic peaks instead of sagging
/// into the valleys between them.
fn envelope(
    fwd: &Arc<dyn RealToComplex<f32>>,
    mag: &[f32],
    n: usize,
    q: usize,
) -> Result<Vec<f32>, String> {
    let l0: Vec<f32> = mag.iter().map(|m| m.max(1e-7).ln()).collect();
    let mut e = lifter(fwd, &l0, n, q)?;
    for _ in 0..ENVELOPE_ITERS {
        let a: Vec<f32> = l0.iter().zip(&e).map(|(x, y)| x.max(*y)).collect();
        e = lifter(fwd, &a, n, q)?;
    }
    Ok(e)
}

/// Multiplies `shifted` by the ratio of the spectral envelope of `orig` to
/// its own, frame by frame, so the formant peaks stay where they were.
fn keep_envelope(st: &Stft, orig: &[f32], shifted: &[f32], sr: f32) -> Result<Vec<f32>, String> {
    let fa = st.analyze(orig)?;
    let mut fb = st.analyze(shifted)?;
    let q = ((sr / 500.0).round() as usize).clamp(4, st.n / 2);
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(st.n);
    let mag = |f: &Spec| f.iter().map(|z| z.norm()).collect::<Vec<f32>>();
    for (a, b) in fa.iter().zip(fb.iter_mut()) {
        let ea = envelope(&fwd, &mag(a), st.n, q)?;
        let eb = envelope(&fwd, &mag(b), st.n, q)?;
        for (k, z) in b.iter_mut().enumerate() {
            // Clamp the correction to +-30 dB.
            let g = (ea[k] - eb[k]).clamp(-3.45, 3.45).exp();
            *z *= g;
        }
    }
    Ok(finite(st.synth(fb, st.hop, st.n, shifted.len(), |_| 1.0)?))
}

/// Changes pitch without changing length.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PitchShift {
    pub input: In,
    /// Shift in semitones (give this or ratio).
    #[serde(default)]
    pub semitones: Option<f32>,
    /// Frequency ratio, 2 = an octave up (give this or semitones).
    #[serde(default)]
    pub ratio: Option<f32>,
    /// FFT window in samples, a power of two (default 2048).
    #[serde(default)]
    pub window: Option<usize>,
    /// true: put the input's spectral envelope (a cepstral true-envelope
    /// estimate, about 500 Hz resolution) back on the shifted sound, so formants stay where
    /// they were while the pitch moves (default false; at most +-30 dB of
    /// correction per bin).
    #[serde(default)]
    pub formant: bool,
}

impl Describe for PitchShift {
    const BLURB: &'static str = "phase-vocoder pitch shift by semitones or ratio, same length, optional formant keeping (acausal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "semitones": -5})
    }
}

impl Eval for PitchShift {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let ratio = match (self.semitones, self.ratio) {
            (Some(s), None) if s.is_finite() => 2.0f32.powf(s / 12.0),
            (None, Some(r)) if r.is_finite() => r,
            _ => return Err("give exactly one of semitones or ratio".into()),
        };
        if !(0.05..=20.0).contains(&ratio) {
            return Err(format!(
                "pitch ratio {ratio}: give a ratio from 0.05 to 20 (about -52 to +52 semitones)"
            ));
        }
        let st = Stft::new(window_size(self.window)?);
        let input = cx.audio("input", &self.input)?;
        // Stretch by the ratio (a whole number of hops), then read it back
        // at that rate. The effective ratio is the integer one.
        let hs = ((st.hop as f32 * ratio).round() as usize).max(1);
        let eff = f64::from(hs as f32) / st.hop as f64;
        let cutoff = if eff > 1.0 {
            ((st.n / 2 + 1) as f64 / eff) as usize
        } else {
            st.n / 2 + 1
        };
        let mut chans = Vec::new();
        for c in input.channels(cx.n) {
            let longer = (c.len() as f64 * eff).ceil() as usize + 4;
            let s = stretch(&st, &c, hs, cutoff, longer)?;
            let shifted = finite((0..cx.n).map(|i| cubic(&s, i as f64 * eff)).collect());
            chans.push(if self.formant {
                keep_envelope(&st, &c, &shifted, cx.sr)?
            } else {
                shifted
            });
        }
        Ok(one(Buf::from_channels(chans)))
    }
}

/// Changes duration without changing pitch.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeStretch {
    pub input: In,
    /// 2 = twice as long (slower), 0.5 = half as long. The result is cut or
    /// zero-padded to the document's length.
    pub factor: f32,
    /// FFT window in samples, a power of two (default 2048).
    #[serde(default)]
    pub window: Option<usize>,
}

impl Describe for TimeStretch {
    const BLURB: &'static str = "phase-vocoder time stretch (factor > 1 = slower), cut or padded to the document length (acausal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "factor": 1.5})
    }
}

impl Eval for TimeStretch {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if !(self.factor.is_finite() && (0.05..=20.0).contains(&self.factor)) {
            return Err(format!(
                "factor: {}: give a number from 0.05 to 20",
                self.factor
            ));
        }
        let st = Stft::new(window_size(self.window)?);
        let input = cx.audio("input", &self.input)?;
        let hs = ((st.hop as f32 * self.factor).round() as usize).max(1);
        let cutoff = st.n / 2 + 1;
        let mut chans = Vec::new();
        for c in input.channels(cx.n) {
            chans.push(finite(stretch(&st, &c, hs, cutoff, cx.n)?));
        }
        Ok(one(Buf::from_channels(chans)))
    }
}

/// What a `spectral` node does to the spectrum.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SpectralMode {
    /// Hold the spectrum at `at` (for `length` seconds) as a sustained texture.
    Freeze,
    /// Smooth the magnitude spectrum over frequency and time by `amount`.
    Blur,
    /// Zero every bin quieter than `threshold_db`.
    Gate,
    /// Interpolate magnitudes between `input` and `other` by `amount`.
    Morph,
    /// Move every component up (or down) by `hz` Hz: sidebands, not a pitch
    /// ratio, so harmonic sounds turn inharmonic.
    Shift,
    /// Warp partials to the stiff-string law `f sqrt(1 + inharmonicity (f / f0)^2)`.
    Stretch,
    /// Disperse in time: highs arrive `time` seconds after lows.
    Smear,
    /// Split into steady (`harmonic`) and transient (`percussive`) parts.
    Hps,
}

/// Spectral edits on an STFT: freeze, blur, gate, morph, shift, stretch, smear, hps.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spectral {
    pub input: In,
    pub mode: SpectralMode,
    /// freeze: seconds where the spectrum is taken and held from.
    #[serde(default)]
    pub at: Option<f32>,
    /// freeze: seconds to hold for (default: to the end).
    #[serde(default)]
    pub length: Option<f32>,
    /// blur: 0..1 smoothing (0 = none, 1 = ~375 Hz and ~85 ms); morph: 0..1,
    /// 0 = input, 1 = other. A signal.
    #[serde(default)]
    pub amount: Option<In>,
    /// gate: bins quieter than this (dBFS of a sine of that amplitude) go to
    /// zero.
    #[serde(default)]
    pub threshold_db: Option<f32>,
    /// morph: the node whose magnitudes `amount` moves toward.
    #[serde(default)]
    pub other: Option<In>,
    /// FFT window in samples, a power of two (default 2048).
    #[serde(default)]
    pub window: Option<usize>,
    /// freeze: mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// shift: Hz to move every component by, a number or a signal, from
    /// -sr/2 to sr/2 (negative moves down). Components that would leave
    /// 0..Nyquist are removed, not folded. No `window`: it runs on the whole
    /// signal.
    #[serde(default)]
    pub hz: Option<In>,
    /// stretch: inharmonicity coefficient b, 0..2 (default 0.01): partial n of
    /// a series at `f0` moves to n f0 sqrt(1 + b n^2). 0 leaves the sound
    /// (up to the vocoder's reconstruction).
    #[serde(default)]
    pub inharmonicity: Option<f32>,
    /// stretch: the series' base frequency in Hz, 1..0.49 sr (default 100).
    #[serde(default)]
    pub f0: Option<f32>,
    /// smear: seconds of group delay at Nyquist (0 at DC, linear between),
    /// -2..2; negative sends the highs first. Energy is kept; whatever lands
    /// past the document's end is cut, so give a `duration`. No `window`.
    #[serde(default)]
    pub time: Option<f32>,
    /// hps: median kernel in frames and bins, odd, 3..101 (default 17).
    #[serde(default)]
    pub kernel: Option<usize>,
}

impl Spectral {
    fn only(&self, field: &str, present: bool, mode: SpectralMode) -> Result<(), String> {
        if present && self.mode != mode {
            Err(format!(
                "field `{field}` is only for mode {mode:?}, not {:?}: remove it",
                self.mode
            ))
        } else {
            Ok(())
        }
    }
}

impl Describe for Spectral {
    const BLURB: &'static str = "spectrum edits: freeze, blur, gate, morph, shift (Hz), stretch (inharmonic warp), smear (dispersion), hps (ports harmonic / percussive) (acausal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "mode": "freeze", "at": 0.1, "length": 0.15})
    }
}

impl Eval for Spectral {
    fn causal(&self) -> bool {
        false
    }
    fn random(&self) -> bool {
        self.mode == SpectralMode::Freeze
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        use SpectralMode::*;
        self.only("at", self.at.is_some(), Freeze)?;
        self.only("length", self.length.is_some(), Freeze)?;
        self.only("threshold_db", self.threshold_db.is_some(), Gate)?;
        self.only("other", self.other.is_some(), Morph)?;
        self.only("hz", self.hz.is_some(), Shift)?;
        self.only("inharmonicity", self.inharmonicity.is_some(), Stretch)?;
        self.only("f0", self.f0.is_some(), Stretch)?;
        self.only("time", self.time.is_some(), Smear)?;
        self.only("kernel", self.kernel.is_some(), Hps)?;
        if self.window.is_some() && matches!(self.mode, Shift | Smear) {
            return Err(format!(
                "field `window` is not used by mode {:?} (it runs on the whole signal): remove it",
                self.mode
            ));
        }
        if self.amount.is_some() && !matches!(self.mode, Blur | Morph) {
            return Err(format!(
                "field `amount` is only for modes Blur and Morph, not {:?}: remove it",
                self.mode
            ));
        }
        let st = Stft::new(window_size(self.window)?);
        let input = cx.audio("input", &self.input)?;
        let bins = st.n / 2 + 1;
        let mut chans = Vec::new();
        match self.mode {
            Freeze => {
                let at = self.at.ok_or("freeze needs `at` (seconds)")?;
                if !(at.is_finite() && at >= 0.0) {
                    return Err(format!("at: {at}: give seconds >= 0"));
                }
                let length = self.length.unwrap_or(f32::INFINITY);
                if length.is_nan() || length <= 0.0 {
                    return Err(format!("length: {length}: give seconds > 0"));
                }
                let k0 = ((at * cx.sr + st.n as f32 / 2.0) / st.hop as f32).round() as usize;
                let k1 = if length.is_finite() {
                    (((at + length) * cx.sr + st.n as f32 / 2.0) / st.hop as f32).round() as usize
                } else {
                    usize::MAX
                };
                let g = st.random_phase_gain();
                let mut rng = cx.rng(self.seed);
                for c in input.channels(cx.n) {
                    let mut frames = st.analyze(&c)?;
                    let held: Vec<f32> = match frames.get(k0) {
                        Some(f) => f.iter().map(|z| z.norm()).collect(),
                        None => vec![0.0; bins],
                    };
                    for (k, f) in frames.iter_mut().enumerate() {
                        if k >= k0 && k < k1 {
                            for (z, m) in f.iter_mut().zip(&held) {
                                *z = Complex::from_polar(*m, rng.unit() * TAU);
                            }
                        }
                    }
                    let gain = |k: usize| if k >= k0 && k < k1 { g } else { 1.0 };
                    chans.push(finite(st.synth(frames, st.hop, st.n, cx.n, gain)?));
                }
            }
            Blur => {
                let amount = cx.mono(
                    "amount",
                    self.amount.as_ref().ok_or("blur needs `amount` (0..1)")?,
                )?;
                for c in input.channels(cx.n) {
                    let mut frames = st.analyze(&c)?;
                    let mags: Vec<Vec<f32>> = frames
                        .iter()
                        .map(|f| f.iter().map(|z| z.norm()).collect())
                        .collect();
                    let nf = frames.len();
                    // Time pass.
                    let mut timed = mags.clone();
                    for k in 0..nf {
                        let centre = (k * st.hop)
                            .saturating_sub(st.n / 2)
                            .min(cx.n.saturating_sub(1));
                        let a = amount.at(centre).clamp(0.0, 1.0);
                        let rt = (a * 8.0).round() as usize;
                        if rt == 0 {
                            continue;
                        }
                        let (lo, hi) = (k.saturating_sub(rt), (k + rt).min(nf - 1));
                        for b in 0..bins {
                            let s: f32 = (lo..=hi).map(|j| mags[j][b]).sum();
                            timed[k][b] = s / (hi - lo + 1) as f32;
                        }
                    }
                    // Frequency pass, with the original phases.
                    for k in 0..nf {
                        let centre = (k * st.hop)
                            .saturating_sub(st.n / 2)
                            .min(cx.n.saturating_sub(1));
                        let a = amount.at(centre).clamp(0.0, 1.0);
                        let rf = (a * 16.0 * st.n as f32 / DEFAULT_WINDOW as f32).round() as usize;
                        let row = &timed[k];
                        for b in 0..bins {
                            let m = if rf == 0 {
                                row[b]
                            } else {
                                let (lo, hi) = (b.saturating_sub(rf), (b + rf).min(bins - 1));
                                row[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
                            };
                            let z = frames[k][b];
                            frames[k][b] = if z.norm() > 0.0 {
                                z * (m / z.norm())
                            } else {
                                Complex::new(m, 0.0)
                            };
                        }
                    }
                    chans.push(finite(st.synth(frames, st.hop, st.n, cx.n, |_| 1.0)?));
                }
            }
            Gate => {
                let th = self.threshold_db.ok_or("gate needs `threshold_db`")?;
                if !th.is_finite() {
                    return Err(format!("threshold_db: {th}: give a number of dB"));
                }
                let amp = 10.0f32.powf(th / 20.0);
                // A sine of amplitude a peaks at a * sum(w) / 2.
                let limit = amp * st.win.iter().sum::<f32>() / 2.0;
                for c in input.channels(cx.n) {
                    let mut frames = st.analyze(&c)?;
                    for z in frames.iter_mut().flatten() {
                        if z.norm() < limit {
                            *z = Complex::new(0.0, 0.0);
                        }
                    }
                    chans.push(finite(st.synth(frames, st.hop, st.n, cx.n, |_| 1.0)?));
                }
            }
            Morph => {
                let other_in = self.other.as_ref().ok_or("morph needs `other` (a node)")?;
                let other = cx.audio("other", other_in)?;
                let amount = cx.mono(
                    "amount",
                    self.amount.as_ref().ok_or("morph needs `amount` (0..1)")?,
                )?;
                let k = if input.is_stereo() || other.is_stereo() {
                    2
                } else {
                    1
                };
                for c in 0..k {
                    let a: Vec<f32> = (0..cx.n).map(|i| input.ch(c, i)).collect();
                    let b: Vec<f32> = (0..cx.n).map(|i| other.ch(c, i)).collect();
                    let (mut fa, fb) = (st.analyze(&a)?, st.analyze(&b)?);
                    for (k, (x, y)) in fa.iter_mut().zip(&fb).enumerate() {
                        let centre = (k * st.hop)
                            .saturating_sub(st.n / 2)
                            .min(cx.n.saturating_sub(1));
                        let t = amount.at(centre).clamp(0.0, 1.0);
                        for (p, q) in x.iter_mut().zip(y) {
                            let m = p.norm() * (1.0 - t) + q.norm() * t;
                            let src = if t < 0.5 { *p } else { *q };
                            *p = if src.norm() > 0.0 {
                                src * (m / src.norm())
                            } else {
                                Complex::new(m, 0.0)
                            };
                        }
                    }
                    chans.push(finite(st.synth(fa, st.hop, st.n, cx.n, |_| 1.0)?));
                }
            }
            Shift => {
                let hz_in = self.hz.as_ref().ok_or("shift needs `hz` (Hz)")?;
                let hz = cx.mono("hz", hz_in)?;
                let limit = cx.sr / 2.0;
                if let Some(h) = hz.constant()
                    && !(h.is_finite() && h.abs() <= limit)
                {
                    return Err(format!(
                        "hz: {h}: give a number between -{limit} and {limit} Hz (half the sample rate)"
                    ));
                }
                let hs: Vec<f32> = (0..cx.n)
                    .map(|i| {
                        let h = hz.at(i);
                        if h.is_finite() {
                            h.clamp(-limit, limit)
                        } else {
                            0.0
                        }
                    })
                    .collect();
                for c in input.channels(cx.n) {
                    chans.push(finite(shift_channel(&c, &hs, cx.sr)?));
                }
            }
            Stretch => {
                let b = self.inharmonicity.unwrap_or(0.01);
                if !(b.is_finite() && (0.0..=MAX_INHARM).contains(&b)) {
                    return Err(format!(
                        "inharmonicity: {b}: give a number from 0 to {MAX_INHARM}"
                    ));
                }
                let f0 = self.f0.unwrap_or(100.0);
                if !(f0.is_finite() && f0 >= 1.0 && f0 <= 0.49 * cx.sr) {
                    return Err(format!(
                        "f0: {f0}: give a frequency from 1 to {} Hz (0.49 of the sample rate)",
                        0.49 * cx.sr
                    ));
                }
                for c in input.channels(cx.n) {
                    chans.push(finite(warp(&st, &c, cx.sr, b, f0, cx.n)?));
                }
            }
            Smear => {
                let t = self.time.ok_or("smear needs `time` (seconds)")?;
                if !(t.is_finite() && t.abs() <= MAX_SMEAR_S) {
                    return Err(format!(
                        "time: {t}: give seconds from -{MAX_SMEAR_S} to {MAX_SMEAR_S}"
                    ));
                }
                for c in input.channels(cx.n) {
                    chans.push(finite(smear_channel(&c, t, cx.sr)?));
                }
            }
            Hps => {
                let kernel = self.kernel.unwrap_or(DEFAULT_KERNEL);
                if !(3..=MAX_KERNEL).contains(&kernel) || kernel.is_multiple_of(2) {
                    return Err(format!(
                        "kernel: {kernel}: give an odd number from 3 to {MAX_KERNEL} (default {DEFAULT_KERNEL})"
                    ));
                }
                let (mut hs, mut ps) = (Vec::new(), Vec::new());
                for c in input.channels(cx.n) {
                    let (h, p) = hps_channel(&st, &c, kernel, cx.n)?;
                    hs.push(h);
                    ps.push(p);
                }
                let harmonic = Buf::from_channels(hs);
                return Ok(vec![
                    ("out", harmonic.clone()),
                    ("harmonic", harmonic),
                    ("percussive", Buf::from_channels(ps)),
                ]);
            }
        }
        Ok(one(Buf::from_channels(chans)))
    }
}
