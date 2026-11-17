//! S9: modulators and control. `chaos` and `fbm` make control signals,
//! `curve` shapes one, `switch` picks between sounds, and `pitch_follow` /
//! `centroid_follow` turn audio into a Hz control signal (acausal: they look
//! at the whole buffer).

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use serde::Deserialize;

use super::{mono, non_negative, one, positive};
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Maps a value in -1..1 onto `range` (default: unchanged), checking the
/// range is finite.
fn check_range(range: &Option<[f32; 2]>) -> Result<(), String> {
    if let Some([a, b]) = range
        && !(a.is_finite() && b.is_finite())
    {
        return Err(format!(
            "range: [{a}, {b}]: give two finite numbers [lo, hi]"
        ));
    }
    Ok(())
}

fn ranged(v: f32, range: Option<[f32; 2]>) -> f32 {
    match range {
        Some([a, b]) => a + (b - a) * (0.5 * v + 0.5),
        None => v,
    }
}

// ------------------------------------------------------------------- chaos

/// Which chaotic system `chaos` runs.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChaosModel {
    /// The logistic map x' = r x (1 - x), one value per step.
    #[default]
    Logistic,
    /// The Lorenz system (sigma 10, beta 8/3), integrated with RK4.
    Lorenz,
    /// The Henon map (b 0.3), one value per step.
    Henon,
}

/// Longest Lorenz integration step (system time units) before a sample is
/// split into sub-steps, so RK4 stays accurate and stable.
const LORENZ_MAX_DT: f64 = 0.01;
/// Fastest Lorenz integration speed, time units a second: 5 sub-steps a
/// sample at most.
const LORENZ_MAX_RATE: f32 = 2000.0;

/// A chaotic control signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chaos {
    /// logistic (default), lorenz or henon.
    #[serde(default)]
    pub model: ChaosModel,
    /// logistic / henon: map steps a second, 0 < rate <= the sample rate
    /// (values are joined linearly between steps). lorenz: integration speed
    /// in Lorenz time units a second, 0 < rate <= 2000 (about 1 unit is one
    /// swing of the attractor's lobes).
    pub rate: f32,
    /// The model's parameter. logistic: r, 0..4 (default 3.9; chaotic above
    /// about 3.57). lorenz: rho, 0..100 (default 28; settles to a constant
    /// below about 24.7). henon: a, 0..1.5 (default 1.4).
    #[serde(default)]
    pub param: Option<f32>,
    /// Map the output onto [lo, hi] (default -1..1). The output never leaves
    /// the range: it is clamped.
    #[serde(default)]
    pub range: Option<[f32; 2]>,
    /// Mixed with the document seed and the node name (default 0): picks the
    /// start state.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Chaos {
    const BLURB: &'static str =
        "chaotic control signal (logistic, lorenz or henon map), -1..1 or a range, seeded";
    fn example() -> serde_json::Value {
        serde_json::json!({"model": "lorenz", "rate": 4, "range": [200, 800], "seed": 1})
    }
}

impl Eval for Chaos {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = positive("rate", self.rate)?;
        let cap = match self.model {
            ChaosModel::Lorenz => LORENZ_MAX_RATE,
            _ => cx.sr,
        };
        if rate > cap {
            return Err(match self.model {
                ChaosModel::Lorenz => format!(
                    "rate: {rate}: lorenz integrates at most {LORENZ_MAX_RATE} time units a second: lower it"
                ),
                _ => format!(
                    "rate: {rate}: a step per sample at most ({} a second): lower it",
                    cx.sr
                ),
            });
        }
        check_range(&self.range)?;
        let mut rng = cx.rng(self.seed);
        let range = self.range;
        match self.model {
            ChaosModel::Logistic => {
                let r = f64::from(self.param.unwrap_or(3.9));
                if !(0.0..=4.0).contains(&r) {
                    return Err(format!(
                        "param: {r}: the logistic r is 0 to 4 (3.9 is chaotic)"
                    ));
                }
                let fresh = |rng: &mut Rng| 0.1 + 0.8 * f64::from(rng.unit());
                let mut x = fresh(&mut rng);
                let step = |x: f64, rng: &mut Rng| {
                    let y = r * x * (1.0 - x);
                    // Floating point can fall into the fixed points 0 and 1
                    // (r = 4 from 0.5): restart from a seeded value.
                    if y.is_finite() && y > 1e-9 && y < 1.0 - 1e-9 {
                        y
                    } else {
                        fresh(rng)
                    }
                };
                let mut next = step(x, &mut rng);
                let mut phase = 0.0f64;
                let per = f64::from(rate / cx.sr);
                Ok(one(mono(cx.n, |_| {
                    let v = x + (next - x) * phase;
                    phase += per;
                    while phase >= 1.0 {
                        phase -= 1.0;
                        x = next;
                        next = step(x, &mut rng);
                    }
                    ranged((2.0 * v - 1.0).clamp(-1.0, 1.0) as f32, range)
                })))
            }
            ChaosModel::Henon => {
                let a = f64::from(self.param.unwrap_or(1.4));
                if !(0.0..=1.5).contains(&a) {
                    return Err(format!(
                        "param: {a}: the henon a is 0 to 1.5 (1.4 is the classic attractor)"
                    ));
                }
                let b = 0.3;
                let start =
                    |rng: &mut Rng| (0.1 * f64::from(rng.signed()), 0.1 * f64::from(rng.signed()));
                let step = |s: (f64, f64), rng: &mut Rng| {
                    let n = (1.0 - a * s.0 * s.0 + s.1, b * s.0);
                    // Above a of about 1.42 orbits can escape: restart.
                    if n.0.is_finite() && n.1.is_finite() && n.0.abs() < 4.0 && n.1.abs() < 4.0 {
                        n
                    } else {
                        start(rng)
                    }
                };
                let mut s = start(&mut rng);
                let mut next = step(s, &mut rng);
                let mut phase = 0.0f64;
                let per = f64::from(rate / cx.sr);
                Ok(one(mono(cx.n, |_| {
                    let v = s.0 + (next.0 - s.0) * phase;
                    phase += per;
                    while phase >= 1.0 {
                        phase -= 1.0;
                        s = next;
                        next = step(s, &mut rng);
                    }
                    ranged((v / 1.3).clamp(-1.0, 1.0) as f32, range)
                })))
            }
            ChaosModel::Lorenz => {
                let rho = f64::from(self.param.unwrap_or(28.0));
                if !(0.0..=100.0).contains(&rho) {
                    return Err(format!(
                        "param: {rho}: the lorenz rho is 0 to 100 (28 is the classic attractor)"
                    ));
                }
                let (sigma, beta) = (10.0f64, 8.0f64 / 3.0);
                let f = |s: [f64; 3]| {
                    [
                        sigma * (s[1] - s[0]),
                        s[0] * (rho - s[2]) - s[1],
                        s[0] * s[1] - beta * s[2],
                    ]
                };
                let start = |rng: &mut Rng| {
                    [
                        1.0 + 0.5 * f64::from(rng.signed()),
                        1.0 + 0.5 * f64::from(rng.signed()),
                        1.0 + 0.5 * f64::from(rng.signed()),
                    ]
                };
                let mut s = start(&mut rng);
                // Time units a sample, split into steps of at most
                // LORENZ_MAX_DT.
                let dt = f64::from(rate / cx.sr);
                let sub = (dt / LORENZ_MAX_DT).ceil().max(1.0) as usize;
                let h = dt / sub as f64;
                Ok(one(mono(cx.n, |_| {
                    for _ in 0..sub {
                        let k1 = f(s);
                        let k2 = f(std::array::from_fn(|j| s[j] + 0.5 * h * k1[j]));
                        let k3 = f(std::array::from_fn(|j| s[j] + 0.5 * h * k2[j]));
                        let k4 = f(std::array::from_fn(|j| s[j] + h * k3[j]));
                        let next: [f64; 3] = std::array::from_fn(|j| {
                            s[j] + h / 6.0 * (k1[j] + 2.0 * k2[j] + 2.0 * k3[j] + k4[j])
                        });
                        s = if next.iter().all(|v| v.is_finite() && v.abs() < 1e4) {
                            next
                        } else {
                            start(&mut rng)
                        };
                    }
                    ranged((s[0] / 20.0).clamp(-1.0, 1.0) as f32, range)
                })))
            }
        }
    }
}

// --------------------------------------------------------------------- fbm

/// Most octaves `fbm` stacks.
const FBM_MAX_OCTAVES: u32 = 12;

/// Fractal (fBm) random motion.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fbm {
    /// New random targets a second in the slowest octave; octave k runs at
    /// rate * 2^k. 0 < rate, and the fastest octave (rate * 2^(octaves-1))
    /// must not pass the sample rate.
    pub rate: f32,
    /// Octaves stacked, 1 to 12 (default 4).
    #[serde(default)]
    pub octaves: Option<u32>,
    /// Amplitude of each octave relative to the one below, 0..1 (default
    /// 0.5). Lower is smoother.
    #[serde(default)]
    pub gain: Option<f32>,
    /// Map -1..1 onto [lo, hi] (default -1..1). The output never leaves the
    /// range.
    #[serde(default)]
    pub range: Option<[f32; 2]>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Fbm {
    const BLURB: &'static str =
        "fractal random motion: octaves of smooth random points, each faster and weaker";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 2, "octaves": 4, "gain": 0.5, "range": [0.5, 1.5], "seed": 3})
    }
}

impl Eval for Fbm {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = positive("rate", self.rate)?;
        let octaves = self.octaves.unwrap_or(4);
        if !(1..=FBM_MAX_OCTAVES).contains(&octaves) {
            return Err(format!("octaves: {octaves}: give 1 to {FBM_MAX_OCTAVES}"));
        }
        let gain = self.gain.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&gain) {
            return Err(format!("gain: {gain}: give a number from 0 to 1"));
        }
        let fastest = f64::from(rate) * f64::from(2u32.pow(octaves - 1));
        if fastest > f64::from(cx.sr) {
            return Err(format!(
                "rate: {rate} with octaves {octaves}: the fastest octave would run at {fastest} points a second, past the sample rate {}: lower rate or octaves",
                cx.sr
            ));
        }
        check_range(&self.range)?;
        // Per octave: random points and the amplitude.
        let mut layers: Vec<(Vec<f32>, f32, f32)> = Vec::new();
        let mut total = 0.0f32;
        for k in 0..octaves {
            let mut rng = cx.rng(crate::mix_seed(self.seed, u64::from(k) + 1));
            let per = cx.sr / (rate * 2f32.powi(k as i32));
            let count = (cx.n as f32 / per).ceil() as usize + 4;
            let pts: Vec<f32> = (0..count).map(|_| rng.signed()).collect();
            let amp = gain.powi(k as i32);
            total += amp;
            layers.push((pts, per, amp));
        }
        let total = total.max(1e-12);
        let range = self.range;
        Ok(one(mono(cx.n, |i| {
            let mut sum = 0.0f32;
            for (pts, per, amp) in &layers {
                let x = i as f32 / per;
                let k = x.floor() as usize;
                let f = x - k as f32;
                let last = pts.len() - 1;
                let p = |j: usize| pts[j.min(last)];
                let (p0, p1, p2, p3) = (p(k), p(k + 1), p(k + 2), p(k + 3));
                let v = 0.5
                    * ((2.0 * p1)
                        + (-p0 + p2) * f
                        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f * f * f);
                sum += amp * v.clamp(-1.0, 1.0);
            }
            ranged((sum / total).clamp(-1.0, 1.0), range)
        })))
    }
}

// ------------------------------------------------------------------- curve

/// How `curve` joins its points.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LookupCurve {
    /// Straight lines between points.
    #[default]
    Linear,
    /// A cosine ease between each pair (flat at every point).
    Smooth,
}

/// Most points `curve` takes.
const CURVE_MAX_POINTS: usize = 4096;

/// A lookup table: the input is x, the output is y.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CurveLookup {
    /// The x signal (mono audio or control).
    pub input: In,
    /// [[x, y], ...]: 2 to 4096 pairs, x strictly increasing. Below the
    /// first x the output is the first y, above the last x the last y.
    pub points: Vec<[f32; 2]>,
    /// linear (default) or smooth (cosine ease between points).
    #[serde(default)]
    pub curve: LookupCurve,
}

impl Describe for CurveLookup {
    const BLURB: &'static str =
        "lookup table: maps the input through breakpoints [[x, y], ..] (clamped at the ends)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "lfo1", "points": [[-1, 0], [0, 0.2], [1, 1]], "curve": "linear"})
    }
}

impl Eval for CurveLookup {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let pts = &self.points;
        if pts.len() < 2 || pts.len() > CURVE_MAX_POINTS {
            return Err(format!(
                "points: {} pairs: give 2 to {CURVE_MAX_POINTS} [x, y] pairs",
                pts.len()
            ));
        }
        for (i, p) in pts.iter().enumerate() {
            if !(p[0].is_finite() && p[1].is_finite()) {
                return Err(format!(
                    "points[{i}]: [{}, {}]: give two finite numbers",
                    p[0], p[1]
                ));
            }
            if i > 0 && p[0] <= pts[i - 1][0] {
                return Err(format!(
                    "points[{i}]: x {} is not above the previous x {}: sort the points by x, each x different",
                    p[0],
                    pts[i - 1][0]
                ));
            }
        }
        let smooth = matches!(self.curve, LookupCurve::Smooth);
        let last = pts.len() - 1;
        Ok(one(mono(cx.n, |i| {
            let x = input.at(i);
            if !x.is_finite() || x <= pts[0][0] {
                return pts[0][1];
            }
            if x >= pts[last][0] {
                return pts[last][1];
            }
            // First point whose x is above the input; the segment is the
            // pair before it, so an x equal to a point gives that point's y.
            let hi = pts.partition_point(|p| p[0] <= x);
            let (a, b) = (pts[hi - 1], pts[hi]);
            let t = (x - a[0]) / (b[0] - a[0]);
            let t = if smooth {
                0.5 - 0.5 * (std::f32::consts::PI * t).cos()
            } else {
                t
            };
            a[1] + (b[1] - a[1]) * t
        })))
    }
}

// ------------------------------------------------------------------ switch

/// Most inputs `switch` takes.
const SWITCH_MAX_INPUTS: usize = 64;

/// Picks one of several sounds by a control signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    /// 2 to 64 sounds (mono or stereo; if any is stereo the output is, with
    /// the mono ones on both sides).
    pub inputs: Vec<In>,
    /// Which input plays: the value rounded to the nearest index (0 is the
    /// first input), clamped to the list. A signal or a number.
    pub select: In,
    /// Seconds to crossfade (linear, gains sum to 1) from one input to the
    /// next when `select` changes (default 0.005). 0 switches at once.
    #[serde(default)]
    pub fade: Option<f32>,
    /// true: `select` blends the two inputs either side of it by its
    /// fraction (1.25 is 75% of input 1 and 25% of input 2); `fade` is then
    /// ignored (default false).
    #[serde(default)]
    pub blend: bool,
}

impl Describe for Switch {
    const BLURB: &'static str =
        "picks one of several inputs by a select signal (index), crossfaded or blended";
    fn example() -> serde_json::Value {
        serde_json::json!({"inputs": ["tone", "noise1"], "select": "steps", "fade": 0.005})
    }
}

impl Eval for Switch {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let k = self.inputs.len();
        if !(2..=SWITCH_MAX_INPUTS).contains(&k) {
            return Err(format!("inputs: {k} inputs: give 2 to {SWITCH_MAX_INPUTS}"));
        }
        let sigs: Vec<_> = self
            .inputs
            .iter()
            .enumerate()
            .map(|(i, s)| cx.audio(&format!("inputs[{i}]"), s))
            .collect::<Result<_, _>>()?;
        let select = cx.mono("select", &self.select)?;
        let fade = non_negative("fade", self.fade.unwrap_or(0.005))?;
        if fade > 10.0 {
            return Err(format!("fade: {fade}: give at most 10 seconds"));
        }
        let blend = self.blend;
        let step = if fade * cx.sr >= 1.0 {
            1.0 / (fade * cx.sr)
        } else {
            1.0
        };
        let stereo = sigs.iter().any(|s| s.is_stereo());
        let top = (k - 1) as f32;
        let mut w = vec![0.0f32; k];
        w[0] = 1.0;
        let mut out = vec![[0.0f32; 2]; cx.n];
        for (i, o) in out.iter_mut().enumerate() {
            let s = select.at(i);
            let s = if s.is_finite() {
                s.clamp(0.0, top)
            } else {
                0.0
            };
            if blend {
                w.fill(0.0);
                let lo = s.floor();
                let f = s - lo;
                let lo = lo as usize;
                w[lo] = 1.0 - f;
                if lo + 1 < k {
                    w[lo + 1] = f;
                }
            } else {
                let t = (s + 0.5).floor().min(top) as usize;
                if w[t] < 1.0 {
                    let wt = (w[t] + step).min(1.0);
                    let others: f32 = w.iter().sum::<f32>() - w[t];
                    if wt >= 1.0 || others <= 1e-9 {
                        w.fill(0.0);
                        w[t] = 1.0;
                    } else {
                        let scale = (1.0 - wt) / others;
                        for (j, v) in w.iter_mut().enumerate() {
                            if j != t {
                                *v *= scale;
                            }
                        }
                        w[t] = wt;
                    }
                }
            }
            for (c, v) in o.iter_mut().enumerate().take(if stereo { 2 } else { 1 }) {
                *v = w
                    .iter()
                    .zip(&sigs)
                    .filter(|(g, _)| **g != 0.0)
                    .map(|(g, s)| g * s.ch(c, i))
                    .sum();
            }
        }
        Ok(one(if stereo {
            Buf::Stereo(out)
        } else {
            Buf::Audio(out.into_iter().map(|p| p[0]).collect())
        }))
    }
}

// ------------------------------------------------------------ pitch_follow

/// YIN's threshold on the cumulative mean normalized difference.
const YIN_THRESHOLD: f32 = 0.15;

/// Fundamental frequency of the input over time, as a control signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PitchFollow {
    /// The sound to follow (mono).
    pub input: In,
    /// Lowest pitch looked for, Hz, 20 or more (default 50). Also sets the
    /// analysis window: one period of this (and the input must be at least
    /// that long plus the shortest period, or the track is 0).
    #[serde(default)]
    pub min: Option<f32>,
    /// Highest pitch looked for, Hz, above `min` and at most a quarter of
    /// the sample rate (default 2000).
    #[serde(default)]
    pub max: Option<f32>,
    /// Smoothing time constant, seconds, 0 to 10 (default 0.02), applied
    /// forwards and backwards so it adds no lag. 0 is unsmoothed.
    #[serde(default)]
    pub smooth: Option<f32>,
}

impl Describe for PitchFollow {
    const BLURB: &'static str = "follows the input's pitch (YIN) as a Hz control signal; holds the last confident value (acausal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "min": 80, "max": 1000, "smooth": 0.02})
    }
}

/// Everything a pitch pass needs, so frames reuse their FFT buffers.
struct YinFrames {
    sr: f32,
    tmin: usize,
    tmax: usize,
    w: usize,
}

impl Eval for PitchFollow {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let min = self.min.unwrap_or(50.0);
        let max = self.max.unwrap_or(2000.0);
        let smooth = self.smooth.unwrap_or(0.02);
        if !(min.is_finite() && min >= 20.0) {
            return Err(format!("min: {min}: give a number of 20 Hz or more"));
        }
        let top = cx.sr / 4.0;
        if !(max.is_finite() && max > min && max <= top) {
            return Err(format!(
                "max: {max}: give a number above min ({min}) and at most {top} Hz (a quarter of the sample rate)"
            ));
        }
        if !(smooth.is_finite() && (0.0..=10.0).contains(&smooth)) {
            return Err(format!("smooth: {smooth}: give seconds from 0 to 10"));
        }
        let x: Vec<f32> = (0..cx.n)
            .map(|i| {
                let v = input.at(i);
                if v.is_finite() { v } else { 0.0 }
            })
            .collect();
        let g = YinFrames {
            sr: cx.sr,
            tmin: ((cx.sr / max).floor() as usize).max(2),
            tmax: (cx.sr / min).ceil() as usize,
            w: (cx.sr / min).ceil() as usize,
        };
        let hop = ((cx.sr * 0.005) as usize).max(1);
        let mut frames = yin_track(&x, &g, hop);
        extend_ends(&mut frames, cx.n, g.w + g.tmax, hop, min, max);
        let track = frames_to_samples(&frames, cx.n, 0.0);
        let track = if smooth > 0.0 {
            zero_phase_smooth(track, smooth, cx.sr)
        } else {
            track
        };
        Ok(one(Buf::Audio(track)))
    }
}

/// (sample time, Hz) of each confident frame, times strictly increasing.
fn yin_track(x: &[f32], g: &YinFrames, hop: usize) -> Vec<(f32, f32)> {
    let (tmin, tmax, w) = (g.tmin, g.tmax, g.w);
    let span = w + tmax;
    let mut out: Vec<(f32, f32)> = Vec::new();
    if x.len() < w + tmin {
        return out;
    }
    // Zero-padded so the last frames can start close to the end; a frame is
    // kept only if the two windows it compares lie inside the signal.
    let mut padded = x.to_vec();
    padded.resize(x.len() + span, 0.0);
    let size = span.next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mut a = fwd.make_input_vec();
    let mut b = fwd.make_input_vec();
    let mut sa = fwd.make_output_vec();
    let mut sb = fwd.make_output_vec();
    let mut cmnd = vec![1.0f32; tmax + 2];
    let mut d = vec![0.0f64; tmax + 2];
    let mut e2 = vec![0.0f64; tmax + 2];
    let mut start = 0usize;
    while start + w + tmin <= x.len() {
        let seg = &padded[start..start + span];
        let c = start as f32;
        start += hop;
        let e1: f64 = seg[..w].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        // Quieter than -80 dBFS rms: not a pitch.
        if e1 / (w as f64) < 1e-8 {
            continue;
        }
        a[..w].copy_from_slice(&seg[..w]);
        a[w..].fill(0.0);
        b[..span].copy_from_slice(seg);
        b[span..].fill(0.0);
        let _ = fwd.process(&mut a, &mut sa);
        let _ = fwd.process(&mut b, &mut sb);
        for (p, q) in sa.iter_mut().zip(&sb) {
            *p = p.conj() * *q;
        }
        // The DC and Nyquist bins of a real transform have no imaginary
        // part; the inverse wants that exactly.
        sa[0].im = 0.0;
        if let Some(l) = sa.last_mut() {
            l.im = 0.0;
        }
        let mut corr = inv.make_output_vec();
        let _ = inv.process(&mut sa, &mut corr);
        // e2[t] = energy of seg[t..t + w].
        let mut run: f64 = seg[..w].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        for t in 0..=tmax {
            e2[t] = run;
            if t < tmax {
                let out_v = f64::from(seg[t]);
                let in_v = f64::from(seg[t + w]);
                run += in_v * in_v - out_v * out_v;
            }
        }
        let inv_n = 1.0 / size as f64;
        for t in 1..=tmax {
            d[t] = (e1 + e2[t] - 2.0 * f64::from(corr[t]) * inv_n).max(0.0);
        }
        let mut cum = 0.0f64;
        for t in 1..=tmax {
            cum += d[t];
            cmnd[t] = if cum > 0.0 {
                (d[t] * t as f64 / cum) as f32
            } else {
                1.0
            };
        }
        // First dip under the threshold, walked down to its minimum.
        let mut tau = None;
        let mut t = tmin;
        while t < tmax {
            if cmnd[t] < YIN_THRESHOLD {
                while t + 1 < tmax && cmnd[t + 1] < cmnd[t] {
                    t += 1;
                }
                tau = Some(t);
                break;
            }
            t += 1;
        }
        let Some(t) = tau else { continue };
        // Parabolic refinement on the difference function.
        let (y0, y1, y2) = (d[t - 1], d[t], d[(t + 1).min(tmax)]);
        let den = y0 - 2.0 * y1 + y2;
        let off = if den.abs() > 1e-20 {
            (0.5 * (y0 - y2) / den).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let lag = t as f64 + off;
        if c as usize + w + t + 1 > x.len() {
            continue;
        }
        // The pair of windows being compared centres half a lag past the
        // middle of the first window.
        let time = c + (w as f32 + lag as f32) * 0.5;
        out.push((time, (f64::from(g.sr) / lag) as f32));
        if let Some(prev) = out.len().checked_sub(2)
            && out[prev].0 >= time
        {
            let fix = out[prev].0 + 1.0;
            let n = out.len();
            out[n - 1].0 = fix;
        }
    }
    out
}

/// The first and last frame sit about half an analysis window inside the
/// signal, so a sweep would be held flat there. When the four frames at an
/// end are consecutive, continue their slope (in log Hz) to the sample 0 /
/// n - 1, at most a factor of 2 and inside min..max.
fn extend_ends(
    frames: &mut Vec<(f32, f32)>,
    n: usize,
    span: usize,
    hop: usize,
    min: f32,
    max: f32,
) {
    if frames.len() < 4 || n < 2 {
        return;
    }
    let near = |a: &[(f32, f32)]| a.windows(2).all(|w| w[1].0 - w[0].0 <= 3.0 * hop as f32);
    let slope = |a: &[(f32, f32)]| {
        let (f, l) = (a[0], a[a.len() - 1]);
        (l.1.ln() - f.1.ln()) / (l.0 - f.0).max(1.0)
    };
    let k = frames.len();
    let tail = frames[k - 4..].to_vec();
    let head = frames[..4].to_vec();
    if near(&tail) && tail[3].0 >= (n - 1).saturating_sub(span) as f32 && tail[3].0 < (n - 1) as f32
    {
        let last = tail[3];
        let f = (slope(&tail) * ((n - 1) as f32 - last.0))
            .exp()
            .clamp(0.5, 2.0);
        frames.push(((n - 1) as f32, (last.1 * f).clamp(min, max)));
    }
    if near(&head) && head[0].0 <= span as f32 && head[0].0 > 0.0 {
        let first = head[0];
        let f = (slope(&head) * (0.0 - first.0)).exp().clamp(0.5, 2.0);
        frames.insert(0, (0.0, (first.1 * f).clamp(min, max)));
    }
}

/// Linear interpolation of (time, value) frames onto samples; before the
/// first frame and after the last the value holds; no frames gives `empty`.
fn frames_to_samples(frames: &[(f32, f32)], n: usize, empty: f32) -> Vec<f32> {
    if frames.is_empty() {
        return vec![empty; n];
    }
    let mut out = Vec::with_capacity(n);
    let mut j = 0usize;
    for i in 0..n {
        let t = i as f32;
        while j + 1 < frames.len() && frames[j + 1].0 <= t {
            j += 1;
        }
        let v = if t <= frames[0].0 {
            frames[0].1
        } else if j + 1 >= frames.len() {
            frames[j].1
        } else {
            let (t0, v0) = frames[j];
            let (t1, v1) = frames[j + 1];
            v0 + (v1 - v0) * ((t - t0) / (t1 - t0).max(1e-9))
        };
        out.push(v);
    }
    out
}

/// A one-pole low-pass run forwards then backwards (no net delay). The ends
/// are padded by odd reflection (a straight slope continues through them)
/// so a ramp is not flattened there.
fn zero_phase_smooth(x: Vec<f32>, time: f32, sr: f32) -> Vec<f32> {
    let n = x.len();
    if n < 2 {
        return x;
    }
    let pad = ((5.0 * time * sr).ceil() as usize).min(n - 1);
    let mut y: Vec<f32> = Vec::with_capacity(n + 2 * pad);
    y.extend((1..=pad).rev().map(|k| 2.0 * x[0] - x[k]));
    y.extend_from_slice(&x);
    y.extend((1..=pad).map(|k| 2.0 * x[n - 1] - x[n - 1 - k]));
    let a = 1.0 - (-1.0 / (time * sr)).exp();
    let mut s = y[0];
    for v in &mut y {
        s += a * (*v - s);
        *v = s;
    }
    let mut s = y[y.len() - 1];
    for v in y.iter_mut().rev() {
        s += a * (*v - s);
        *v = s;
    }
    y[pad..pad + n].to_vec()
}

// ---------------------------------------------------------- centroid_follow

/// Most samples in a `centroid_follow` window.
const CENTROID_MAX_WINDOW: usize = 32_768;

/// Spectral centroid of the input over time, as a control signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CentroidFollow {
    /// The sound to follow (mono).
    pub input: In,
    /// Analysis window, seconds (Hann; default 0.02): at least 32 samples
    /// and at most 32768 samples. Longer resolves low sounds, shorter
    /// follows fast changes. Frames overlap by 75 %.
    #[serde(default)]
    pub window: Option<f32>,
}

impl Describe for CentroidFollow {
    const BLURB: &'static str = "follows the input's spectral centroid as a Hz control signal (acausal: windows look ahead)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "window": 0.02})
    }
}

impl Eval for CentroidFollow {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let window = self.window.unwrap_or(0.02);
        if !window.is_finite() {
            return Err(format!("window: {window}: give seconds"));
        }
        let win = (window * cx.sr).round();
        if !(32.0..=CENTROID_MAX_WINDOW as f32).contains(&win) {
            return Err(format!(
                "window: {window}: give between {} and {} seconds (32 to {CENTROID_MAX_WINDOW} samples)",
                32.0 / cx.sr,
                CENTROID_MAX_WINDOW as f32 / cx.sr
            ));
        }
        let win = win as usize;
        let size = win.next_power_of_two();
        let hop = (win / 4).max(1);
        let hann: Vec<f32> = (0..win)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * (i as f32 + 0.5) / win as f32).cos())
            .collect();
        let x: Vec<f32> = (0..cx.n)
            .map(|i| {
                let v = input.at(i);
                if v.is_finite() { v } else { 0.0 }
            })
            .collect();
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(size);
        let mut buf = fwd.make_input_vec();
        let mut spec: Vec<Complex<f32>> = fwd.make_output_vec();
        let bin_hz = cx.sr / size as f32;
        let mut frames: Vec<(f32, f32)> = Vec::new();
        let mut centre = 0usize;
        while centre < cx.n + hop {
            let start = centre as isize - (win / 2) as isize;
            let mut energy = 0.0f64;
            for (j, b) in buf.iter_mut().enumerate() {
                *b = 0.0;
                if j < win {
                    let k = start + j as isize;
                    if k >= 0 && (k as usize) < x.len() {
                        let v = x[k as usize] * hann[j];
                        *b = v;
                        energy += f64::from(v) * f64::from(v);
                    }
                }
            }
            // Under -90 dBFS rms of the windowed segment: silence, hold.
            if energy / win as f64 > 1e-9 {
                let _ = fwd.process(&mut buf, &mut spec);
                let (mut num, mut den) = (0.0f64, 0.0f64);
                for (k, z) in spec.iter().enumerate().skip(1) {
                    let p = f64::from(z.norm_sqr());
                    num += p * f64::from(k as f32 * bin_hz);
                    den += p;
                }
                if den > 0.0 {
                    frames.push((centre as f32, (num / den) as f32));
                }
            }
            centre += hop;
        }
        Ok(one(Buf::Audio(frames_to_samples(&frames, cx.n, 0.0))))
    }
}
