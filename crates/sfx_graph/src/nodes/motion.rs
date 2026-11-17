//! Motion and stereo placement: `doppler` (a straight-line fly-by: delay,
//! pitch bend, 1 / r level, air absorption, pan), `haas` (a mono sound made
//! stereo with a short delay on one side) and `decorrelate` (mono to stereo
//! through two different seeded all-pass FIR filters).

use std::f32::consts::{FRAC_PI_4, TAU};

use foley_synth::absorption_cutoff;
use serde::Deserialize;

use super::{fft_convolve, one, per_channel};
use crate::buf::Buf;
use crate::dsp::{DelayLine, FilterMode, Svf};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound used by `doppler`, m/s (air at 20 C).
const C: f64 = 343.0;
/// The longest delay `doppler` will allocate, seconds.
const MAX_DELAY_S: f64 = 30.0;
/// The fastest `doppler` `speed`, m/s (0.87 c: an upward shift of 8x).
const MAX_SPEED: f32 = 300.0;
/// The largest `closest`, metres.
const MAX_CLOSEST: f32 = 5000.0;
/// Absorption cutoffs are recomputed every this many samples.
const BLOCK: usize = 32;

// ---------------------------------------------------------------- doppler

/// A source passing the listener on a straight line at constant speed: the
/// delay from the distance (so the pitch bends), a 1 / distance level, an
/// optional air absorption low-pass and an equal-power pan from the bearing.
/// Moves from the left to the right, passing the listener at `closest`
/// metres at the moment `at`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Doppler {
    /// The sound the source emits. Mono; with `pan: false` stereo is also
    /// accepted (each channel is delayed alike).
    pub input: In,
    /// Source speed, m/s: 0..300 (default 30; the speed of sound is 343).
    /// Not with `distance`.
    #[serde(default)]
    pub speed: Option<f32>,
    /// Distance of the path from the listener at its nearest, metres:
    /// above 0 and at most 5000 (default 5). Below 1 m the level stays at
    /// its 1 m value. Not with `distance`.
    #[serde(default)]
    pub closest: Option<f32>,
    /// The time, seconds, at which the sound emitted at the point of closest
    /// approach reaches the listener: the loudest instant, and where the pan
    /// crosses the centre. Default: half the sound's length. Not with
    /// `distance`. The source starts out far enough away that this much of
    /// the approach is heard; a long `at` at high `speed` starts it more than
    /// 30 s of sound-travel away and is an error.
    #[serde(default)]
    pub at: Option<f32>,
    /// Distance to the listener in metres, over time, driving the effect
    /// directly instead of `speed` / `closest` / `at`: a number or a signal
    /// (0..10290, clamped; the delay is distance / 343 as heard at that
    /// moment). Mono out, no pan.
    #[serde(default)]
    pub distance: Option<In>,
    /// Pan from the bearing, equal power, left to right (default true when
    /// the path is given by `speed`, and mono output when `distance` is).
    /// `true` with `distance` is an error: there is no bearing.
    #[serde(default)]
    pub pan: Option<bool>,
    /// Air absorption of the highs with distance (default true): a 12 dB per
    /// octave low-pass at the frequency where that distance costs 6 dB, as
    /// the `air` node (50 % humidity, 15 C); none under about 10 m.
    #[serde(default)]
    pub air: Option<bool>,
}

impl Describe for Doppler {
    const BLURB: &'static str = "a fly-by: delay (pitch bend), 1/r level, air absorption and pan for a source passing at speed and closest approach (or driven by a distance signal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "speed": 30, "closest": 5, "pan": true, "air": true})
    }
}

/// The geometry of one render: per sample the delay in samples, the level,
/// the distance (for absorption) and the pan position (-1..1).
struct Path {
    delay: Vec<f32>,
    gain: Vec<f32>,
    dist: Vec<f32>,
    pan: Vec<f32>,
}

impl Doppler {
    /// The delay, level and bearing of a straight pass. The sound heard at
    /// time t was emitted at te with te + r(te) / c = t; Newton on that
    /// (convex, increasing) function, warm-started from the last sample.
    fn pass(&self, cx: &Cx) -> Result<Path, String> {
        let speed = self.speed.unwrap_or(30.0);
        let closest = self.closest.unwrap_or(5.0);
        if !(speed.is_finite() && (0.0..=MAX_SPEED).contains(&speed)) {
            return Err(format!(
                "speed: {speed}: give a number from 0 to {MAX_SPEED} m/s (sound travels at 343)"
            ));
        }
        if !(closest.is_finite() && closest > 0.0 && closest <= MAX_CLOSEST) {
            return Err(format!(
                "closest: {closest}: give a distance above 0 and at most {MAX_CLOSEST} metres"
            ));
        }
        let length = cx.n as f32 / cx.sr;
        let at = self.at.unwrap_or(0.5 * length);
        if !(at.is_finite() && at >= 0.0) {
            return Err(format!("at: {at}: give a time in seconds, 0 or more"));
        }
        let (v, d, sr) = (f64::from(speed), f64::from(closest), f64::from(cx.sr));
        let te0 = f64::from(at) - d / C;
        let mut path = Path {
            delay: Vec::with_capacity(cx.n),
            gain: Vec::with_capacity(cx.n),
            dist: Vec::with_capacity(cx.n),
            pan: Vec::with_capacity(cx.n),
        };
        let mut te = -d / C;
        let mut worst = 0.0f64;
        for i in 0..cx.n {
            let t = i as f64 / sr;
            let f = |te: f64| te + (v * (te - te0)).hypot(d) / C - t;
            let mut converged = false;
            for _ in 0..60 {
                let x = v * (te - te0);
                let r = x.hypot(d);
                let fe = te + r / C - t;
                let slope = 1.0 + v * v * (te - te0) / (C * r);
                let step = fe / slope;
                te -= step;
                if step.abs() < 1e-12 {
                    converged = true;
                    break;
                }
            }
            if !converged || !f(te).is_finite() {
                // Newton on a convex function does not fail; fall back to
                // the non-moving answer rather than a non-finite sample.
                te = t - d / C;
            }
            let x = v * (te - te0);
            let r = x.hypot(d);
            let tau = t - te;
            worst = worst.max(tau);
            path.delay.push((tau * sr) as f32);
            path.gain.push((1.0 / r.max(1.0)) as f32);
            path.dist.push(r as f32);
            path.pan.push((x / r) as f32);
        }
        if worst > MAX_DELAY_S {
            return Err(format!(
                "at: {at}: with speed {speed} m/s the source starts {:.0} m away ({worst:.1} s of travel, the limit is {MAX_DELAY_S} s): give a smaller at or speed",
                worst * C
            ));
        }
        Ok(path)
    }

    /// The delay and level of a distance signal.
    fn follow(&self, cx: &Cx, distance: &In) -> Result<Path, String> {
        let sig = cx.mono("distance", distance)?;
        let top = (MAX_DELAY_S * C) as f32;
        if let Some(d) = sig.constant()
            && !(d.is_finite() && (0.0..=top).contains(&d))
        {
            return Err(format!(
                "distance: {d}: give a number from 0 to {top} metres"
            ));
        }
        let mut path = Path {
            delay: Vec::with_capacity(cx.n),
            gain: Vec::with_capacity(cx.n),
            dist: Vec::with_capacity(cx.n),
            pan: vec![0.0; cx.n],
        };
        for i in 0..cx.n {
            let d = sig.at(i);
            let d = if d.is_finite() {
                d.clamp(0.0, top)
            } else {
                0.0
            };
            path.delay
                .push((f64::from(d) / C * f64::from(cx.sr)) as f32);
            path.gain.push(1.0 / d.max(1.0));
            path.dist.push(d);
        }
        Ok(path)
    }
}

impl Eval for Doppler {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let path = match &self.distance {
            Some(d) => {
                for (name, set) in [
                    ("speed", self.speed.is_some()),
                    ("closest", self.closest.is_some()),
                    ("at", self.at.is_some()),
                ] {
                    if set {
                        return Err(format!(
                            "{name}: not used with `distance` (it gives the path directly): remove {name} or remove distance"
                        ));
                    }
                }
                if self.pan == Some(true) {
                    return Err("pan: true needs a bearing, and `distance` has none: remove pan or give speed and closest instead of distance".into());
                }
                self.follow(cx, d)?
            }
            None => self.pass(cx)?,
        };
        let pan = self.distance.is_none() && self.pan.unwrap_or(true);
        if pan && input.is_stereo() {
            return Err("input: stereo, but a panned doppler takes mono: add a mix node with \"to\": \"mono\" in between, or set pan false".into());
        }
        let air = self.air.unwrap_or(true);
        // Absorption cutoffs once per block (a bisection each).
        let blocks = cx.n / BLOCK + 1;
        let cutoff: Vec<f32> = (0..blocks)
            .map(|b| {
                let d = path.dist[(b * BLOCK).min(cx.n.saturating_sub(1))];
                absorption_cutoff(d.max(0.0), 15.0, 50.0).unwrap_or(f32::MAX)
            })
            .collect();
        let max_delay = path.delay.iter().fold(0.0f32, |m, d| m.max(*d));
        let run = |x: &[f32]| -> Vec<f32> {
            let mut line = DelayLine::new(max_delay.ceil() as usize + 4);
            let mut lp = Svf::default();
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    line.write(*v);
                    let y = line.read_lagrange3(path.delay[i]);
                    let y = if air {
                        let c = cutoff[i / BLOCK];
                        // Keep the filter's state warm while it is bypassed.
                        let f = lp.tick(
                            y,
                            FilterMode::Lp,
                            c.min(0.45 * cx.sr),
                            std::f32::consts::FRAC_1_SQRT_2,
                            0.0,
                            cx.sr,
                        );
                        if c == f32::MAX { y } else { f }
                    } else {
                        y
                    };
                    y * path.gain[i]
                })
                .collect()
        };
        if pan {
            let wet = run(&input.channels(cx.n)[0]);
            Ok(one(Buf::Stereo(
                wet.iter()
                    .zip(&path.pan)
                    .map(|(y, p)| {
                        let a = (p.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
                        [y * a.cos(), y * a.sin()]
                    })
                    .collect(),
            )))
        } else {
            Ok(one(per_channel(&input, cx.n, run)))
        }
    }
}

// ------------------------------------------------------------------- haas

/// Which channel carries the delayed copy.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HaasSide {
    Left,
    #[default]
    Right,
}

/// A mono sound made stereo: one channel is the input, the other the input
/// delayed by a few milliseconds (the precedence effect: the image sits
/// towards the earlier channel).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Haas {
    /// The sound: mono.
    pub input: In,
    /// The delay of the late channel, seconds: 0..0.05 (default 0.012). A
    /// number outside the range is an error; a signal is clamped per sample.
    /// Beyond about 30 ms the copy is heard as an echo.
    #[serde(default)]
    pub delay: Option<In>,
    /// `side`: the channel that gets the delayed copy, `left` or `right`
    /// (default right; the other channel is the input unchanged).
    #[serde(default)]
    pub side: Option<HaasSide>,
}

impl Describe for Haas {
    const BLURB: &'static str =
        "mono to stereo with a short delay (precedence effect) on one side; delay 0..0.05 s";
    const WORDS: &'static [&'static str] = &["side"];
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "delay": 0.012, "side": "right"})
    }
}

const HAAS_MAX: f32 = 0.05;

impl Eval for Haas {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let delay = cx.mono_or("delay", &self.delay, 0.012)?;
        if let Some(d) = delay.constant()
            && !(d.is_finite() && (0.0..=HAAS_MAX).contains(&d))
        {
            return Err(format!(
                "delay: {d}: give a number from 0 to {HAAS_MAX} seconds"
            ));
        }
        let mut line = DelayLine::new((HAAS_MAX * cx.sr).ceil() as usize + 4);
        let late: Vec<f32> = (0..cx.n)
            .map(|i| {
                line.write(input.at(i));
                let d = delay.at(i);
                let d = if d.is_finite() {
                    d.clamp(0.0, HAAS_MAX)
                } else {
                    0.0
                };
                line.read_lagrange3(d * cx.sr)
            })
            .collect();
        let left = matches!(self.side, Some(HaasSide::Left));
        Ok(one(Buf::Stereo(
            late.iter()
                .enumerate()
                .map(|(i, l)| {
                    let x = input.at(i);
                    if left { [*l, x] } else { [x, *l] }
                })
                .collect(),
        )))
    }
}

// ------------------------------------------------------------ decorrelate

/// A mono sound made two channels that are uncorrelated but have the same
/// spectrum: each channel is the input through its own all-pass FIR (unit
/// magnitude at every frequency, a seeded group delay that rises with
/// frequency in the left channel and falls in the right).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Decorrelate {
    /// The sound: mono.
    pub input: In,
    /// How far the channels are pulled apart, 0..1 (default 1): scales the
    /// group delay the filters add (at 1: 3.4 ms plus 7 to 11 ms that grows
    /// with frequency in the left channel and shrinks in the right, at 48 kHz).
    /// 0 gives two copies of the input, bit for bit. A number outside 0..1 is
    /// an error.
    #[serde(default)]
    pub amount: Option<f32>,
    /// Another seed gives another pair of filters (default 0); mixed with the
    /// document seed and the node's name.
    #[serde(default)]
    pub seed: Option<u64>,
}

impl Describe for Decorrelate {
    const BLURB: &'static str = "mono to stereo through two different seeded all-pass FIR filters: flat magnitude, uncorrelated channels";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "amount": 1.0, "seed": 1})
    }
}

/// The impulse response of an all-pass with group delay `tau(x)` seconds at
/// x = 0..1 (DC to Nyquist), as an `n`-point inverse FFT of unit magnitude.
fn allpass_ir(n: usize, sr: f32, tau: impl Fn(f32) -> f32) -> Result<Vec<f32>, String> {
    use realfft::RealFftPlanner;
    use realfft::num_complex::Complex;
    let half = n / 2;
    let df = sr / n as f32;
    let mut phase = 0.0f64;
    let mut spec = Vec::with_capacity(half + 1);
    let mut prev = tau(0.0);
    spec.push(Complex::new(1.0f32, 0.0));
    for k in 1..=half {
        let t = tau(k as f32 / half as f32);
        phase -= f64::from(TAU) * f64::from(0.5 * (prev + t)) * f64::from(df);
        prev = t;
        let (s, c) = (phase.sin() as f32, phase.cos() as f32);
        spec.push(if k == half {
            // The Nyquist bin of a real signal is real.
            Complex::new(c.signum(), 0.0)
        } else {
            Complex::new(c, s)
        });
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let inv = planner.plan_fft_inverse(n);
    let mut out = inv.make_output_vec();
    inv.process(&mut spec, &mut out)
        .map_err(|e| format!("fft: {e}"))?;
    let scale = 1.0 / n as f32;
    Ok(out.into_iter().map(|v| v * scale).collect())
}

impl Eval for Decorrelate {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let amount = self.amount.unwrap_or(1.0);
        if !(amount.is_finite() && (0.0..=1.0).contains(&amount)) {
            return Err(format!("amount: {amount}: give a number from 0 to 1"));
        }
        let x: Vec<f32> = (0..cx.n).map(|i| input.at(i)).collect();
        if amount == 0.0 {
            return Ok(one(Buf::Stereo(x.iter().map(|v| [*v, *v]).collect())));
        }
        // 2048 taps (43 ms) at 48 kHz; the group delay spans a quarter of the
        // window at most, which keeps the response within 0.1 dB of flat.
        let n = ((cx.sr * 0.0426) as usize)
            .next_power_of_two()
            .clamp(256, 8192);
        let span = n as f32 / cx.sr;
        // A fixed lead keeps the response inside the window at both ends.
        let lead = 0.08 * span;
        let mut rng = cx.rng(self.seed.unwrap_or(0));
        let (tl, pl) = (
            (0.17 + 0.08 * rng.unit()) * span * amount,
            0.7 + 0.7 * rng.unit(),
        );
        let (tr, pr) = (
            (0.17 + 0.08 * rng.unit()) * span * amount,
            0.7 + 0.7 * rng.unit(),
        );
        let hl = allpass_ir(n, cx.sr, |u| lead + tl * u.powf(pl))?;
        let hr = allpass_ir(n, cx.sr, |u| lead + tr * (1.0 - u).powf(pr))?;
        let l = fft_convolve(&x, &hl, cx.n)?;
        let r = fft_convolve(&x, &hr, cx.n)?;
        Ok(one(Buf::Stereo(
            l.iter().zip(&r).map(|(a, b)| [*a, *b]).collect(),
        )))
    }

    fn random(&self) -> bool {
        true
    }
}
