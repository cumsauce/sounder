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
}

impl Describe for PitchShift {
    const BLURB: &'static str =
        "phase-vocoder pitch shift by semitones or ratio, same length (acausal)";
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
            chans.push(finite(
                (0..cx.n).map(|i| cubic(&s, i as f64 * eff)).collect(),
            ));
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
}

/// Spectral edits on an STFT: freeze, blur, gate, morph.
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
    const BLURB: &'static str = "spectrum edits: freeze (hold a moment as texture), blur, gate (zero quiet bins), morph (toward another node) (acausal)";
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
        }
        Ok(one(Buf::from_channels(chans)))
    }
}
