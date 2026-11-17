//! `water` and `splash`: running water and a single splash, as populations of
//! Minnaert bubbles summed in one pass.
//!
//! Both nodes reuse the maths of foley's `Bubble` (the `bubble` node): a
//! sine at the Minnaert resonance `f = 3.29 kHz / radius_mm`, decaying at
//! `pi f 0.045` a second, rising in pitch by `0.1 * decay rate` a second. The
//! difference is how they are summed: every bubble of a population is
//! written straight into one output buffer (no per-voice graph, no
//! `voices`), so thousands of bubbles a second are cheap.
//!
//! **Level law.** `bubble` scales amplitude with `r^1.5`, so its energy goes
//! as `r^4` and one big bubble outweighs a hundred small ones; a population
//! drawn from a log-normal radius spread then has its level set by its few
//! largest members and the output stage's limiter does the rest. Here a
//! bubble of radius `r` born into a population with median radius `r_med`
//! has peak amplitude `A0 * sqrt(r / r_med)` (A0 = 0.1 for `water`). Its
//! energy is amplitude squared times its decay time, so it goes as `r^2`
//! relative to the median, and a +-1 sigma (0.4 in ln r) spread moves the
//! amplitude by only about +-22 %. The stream's level then follows the
//! bubble rate and the median size, not a lucky large bubble.

use std::f32::consts::{PI, TAU};

use foley_dsp::minnaert_hz;
use serde::Deserialize;

use super::{non_negative, one, positive};
use crate::buf::Buf;
use crate::dsp::{Biquad, FilterMode, Rng};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Total damping constant of a millimetre bubble (foley's `DAMPING`).
const DAMPING: f32 = 0.045;
/// Pitch rise, in units of the decay rate over a second (foley's default).
const RISE: f32 = 0.1;
/// Rate cap, bubbles a second at flow 1 (`density` errors above it).
const MAX_DENSITY: f32 = 3000.0;
/// Work cap: bubbles one `water` render may create (a splash counts 40).
const MAX_BUBBLES: f64 = 800_000.0;
/// Largest stream bubble, mm (165 Hz).
const MAX_RADIUS_MM: f32 = 20.0;
/// Largest splash bubble, mm (55 Hz).
const MAX_SPLASH_RADIUS_MM: f32 = 60.0;
/// ln-sigma of the stream's log-normal radius spread.
const SIGMA: f32 = 0.4;
/// Peak amplitude of a median stream bubble.
const A0: f32 = 0.1;
/// RMS of the hiss at flow 1.
const HISS_RMS: f32 = 0.03;
/// Peak a lone `splash` node is scaled to.
const SPLASH_PEAK: f32 = 0.5;
/// Peak of the splashes mixed into a stream by `water.splash`.
const STREAM_SPLASH_PEAK: f32 = 0.3;
/// Pitch rise of a splash bubble, in units of its decay rate over a second
/// (a third of `RISE`).
const SPLASH_RISE: f32 = -0.03;
/// Raised-cosine onset of a splash bubble, seconds (a bare phasor start is a
/// step that shows as broadband clicks).
const SPLASH_ONSET: f32 = 0.001;
/// Amplitude of the impact burst, against the main bubble at 1.
const BURST_AMP: f32 = 1.0;
/// Silence before the burst, seconds: the output's default 2 ms fade-in would
/// otherwise cut the loudest part of the sound.
const LEAD: f32 = 0.0025;
/// Where in the cloud window the main cavity bubble is born.
const MAIN_AT: f32 = 1.0;
/// Longest a splash rings, seconds (cap on its buffer).
const MAX_SPLASH_SECONDS: f32 = 2.5;

/// Smallest radius (metres) that rings below 0.4 of the sample rate and
/// above the 0.7 mm foley says starts to alias.
fn min_radius(sr: f32) -> f32 {
    (0.7e-3f32).max(minnaert_hz(1.0) / (0.4 * sr))
}

/// A standard normal draw (Box-Muller).
fn gauss(rng: &mut Rng) -> f32 {
    let u1 = 1.0 - rng.unit();
    let u2 = rng.unit();
    (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
}

/// One bubble to ring into a buffer.
#[derive(Clone, Copy, Debug)]
struct Pop {
    /// Sample index of its birth.
    start: usize,
    /// Radius, metres.
    radius: f32,
    /// Peak amplitude.
    amp: f32,
}

/// How long a bubble of `radius` metres rings, seconds (to -60 dB).
fn ring_seconds(radius: f32) -> f32 {
    6.9 / (PI * minnaert_hz(radius) * DAMPING)
}

/// Adds `b` to `out`: foley's phasor ring (pitch re-tuned every 32 samples).
/// With `wrap`, the tail continues at the start of the buffer (a loop);
/// without, it is cut at the end. `onset` samples of raised cosine fade the
/// bubble in (0: a bare start).
fn ring(out: &mut [f32], b: &Pop, sr: f32, wrap: bool, rise_rate: f32, onset: usize) {
    let n = out.len();
    if n == 0 || b.start >= n {
        return;
    }
    let f0 = minnaert_hz(b.radius);
    let damping = PI * f0 * DAMPING;
    let rise = rise_rate * damping;
    let step = (-damping / sr).exp();
    let dt = 1.0 / sr;
    let life = (ring_seconds(b.radius) * sr).ceil() as usize;
    let len = if wrap { life } else { life.min(n - b.start) };
    let (mut re, mut im) = (1.0f32, 0.0f32);
    let (mut c, mut s) = (1.0f32, 0.0f32);
    let mut amp = b.amp;
    let mut idx = b.start;
    for k in 0..len {
        if k % 32 == 0 {
            let angle = TAU * f0 * (1.0 + rise * k as f32 * dt) * dt;
            (s, c) = angle.sin_cos();
            let norm = re.hypot(im);
            re /= norm;
            im /= norm;
        }
        (re, im) = (re * c - im * s, re * s + im * c);
        amp *= step;
        let gate = if k < onset {
            0.5 - 0.5 * (PI * k as f32 / onset as f32).cos()
        } else {
            1.0
        };
        out[idx] += amp * im * gate;
        idx += 1;
        if idx == n {
            idx = 0;
        }
    }
}

/// One splash of characteristic radius `size` metres as its own buffer
/// (peak 1): a bright, brief impact burst, then a cloud of bubbles whose size
/// grows with time (so the pitch falls), ending in the main cavity bubble.
///
/// - window `W = 0.06 + 0.12 * sqrt(size / 0.05)` s.
/// - burst: white noise through a low-pass at `16 kHz * (0.05 / size)^0.1`
///   (13 .. 0.4 sr), decaying with time constant `2 ms + 1.5 ms *
///   sqrt(size / 0.05)`, amplitude 0.5, born at 0.
/// - cloud: `10 + 18 * sqrt(size / 0.05)` bubbles (6 .. 80), born at 4 ms + t
///   with t in 0..W, density falling as `exp(-2.5 t / W)`; radius `r_main *
///   (0.1 + 0.7 * (t / W)^0.6)` times a ln-sigma 0.15 spread; amplitude `0.5 *
///   sqrt(r / r_main)` times `exp(-1.0 t / W)`.
/// - main bubble: radius `0.12 * size` (1 .. 60 mm), born at 4 ms + 0.4 W,
///   amplitude 1.
///
/// Every bubble rings with a pitch rise of `SPLASH_RISE` (a third of
/// `water`'s), so a ring does not climb back into the band the cloud left.
fn splash_buffer(size: f32, rng: &mut Rng, sr: f32) -> Vec<f32> {
    let scale = (size / 0.05).max(1e-3);
    let r_main = (0.12 * size).clamp(1e-3, MAX_SPLASH_RADIUS_MM * 1e-3);
    let rmin = min_radius(sr);
    let r_top = r_main.max(rmin);
    let window = 0.06 + 0.12 * scale.sqrt();
    let count = (10.0 + 18.0 * scale.sqrt()).round().clamp(6.0, 80.0) as usize;
    let t0 = LEAD + 0.004;
    let mut pops = vec![Pop {
        start: ((t0 + MAIN_AT * window) * sr) as usize,
        radius: r_top,
        amp: 1.0,
    }];
    let k = 2.5f32;
    for i in 0..count {
        // Density falling as exp(-k t / window): inverse CDF of a stratified
        // draw (one per equal slice of probability, so the cloud is even
        // enough for its size to read as a trend).
        let u = (i as f32 + rng.unit()) / count as f32;
        let t = -window / k * (1.0 - u * (1.0 - (-k).exp())).ln();
        let frac = (t / window).clamp(0.0, 1.0);
        let r =
            (r_top * (0.1 + 0.7 * frac.powf(0.6)) * (0.08 * gauss(rng)).exp()).clamp(rmin, r_top);
        let fall = (-frac).exp();
        pops.push(Pop {
            start: ((t0 + t) * sr) as usize,
            radius: r,
            amp: 0.5 * (r / r_top).sqrt() * fall,
        });
    }
    let cap = (MAX_SPLASH_SECONDS * sr) as usize;
    let len = pops
        .iter()
        .map(|p| p.start + (ring_seconds(p.radius) * sr).ceil() as usize)
        .max()
        .unwrap_or(1)
        .min(cap)
        .max(1);
    let mut out = vec![0.0f32; len];
    for p in &pops {
        ring(
            &mut out,
            p,
            sr,
            false,
            SPLASH_RISE,
            (SPLASH_ONSET * sr) as usize,
        );
    }
    // The impact burst.
    let tau = 0.002 + 0.0015 * scale.sqrt();
    let cut = (16_000.0 * (0.05 / size.max(1e-4)).powf(0.1)).clamp(400.0, 0.4 * sr);
    let mut lp = Biquad::new();
    lp.set(FilterMode::Lp, cut, 0.707, 0.0, sr);
    let burst = ((8.0 * tau * sr) as usize).min(len);
    let lead = (LEAD * sr) as usize;
    for (i, o) in out.iter_mut().enumerate().skip(lead).take(burst) {
        let e = (-((i - lead) as f32) / (tau * sr)).exp();
        *o += BURST_AMP * e * lp.tick(rng.signed());
    }
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 {
        for v in &mut out {
            *v /= peak;
        }
    }
    out
}

// ----------------------------------------------------------------- water

/// Running water: a population of bubbles (Minnaert resonances, log-normal
/// radii) born at a rate and with a median size that follow `flow`, plus a
/// band-limited hiss and, optionally, splashes. One pass, no per-voice graph.
///
/// Per sample, with `flow` clamped to 0..1: bubble rate `density * flow^1.5`
/// a second (a thinned Poisson process, so a moving `flow` is followed);
/// median radius `0.8 mm * 5^flow * 2^(2 * (depth - 0.5))` (flow 0: 0.8 mm,
/// 4.1 kHz; flow 1: 4 mm, 820 Hz; depth 0 halves it, depth 1 doubles it),
/// each bubble `median * exp(0.4 * N(0, 1))` clamped to 0.7 .. 20 mm. Bubble
/// peak amplitude is `0.1 * sqrt(r / median)`: see the level law in the
/// module comment. The hiss is white noise through a 400 Hz high-pass and a
/// low-pass at `10 kHz * 0.25^depth`, scaled to rms `0.03 * flow`. Flow 0
/// is silence. Not causal when it loops (the last bubbles ring over the
/// seam).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Water {
    /// How hard the water runs, 0..1 (a node for a flow that changes; a
    /// signal is clamped per sample, a number outside the range is an
    /// error). 0 is silence; 1 is the full bubble `density`, bigger bubbles.
    pub flow: In,
    /// Depth of the water, 0..1 (default 0.5): scales bubble size (0 halves
    /// the radius, 1 doubles it) and moves the hiss low-pass from 10 kHz
    /// (0) to 2.5 kHz (1). A number.
    #[serde(default)]
    pub depth: Option<f32>,
    /// Splashes mixed in, 0..1 (default 0): at 1, about 4 splashes a second
    /// at flow 1 (the rate also follows `flow`), sizes 2..12 cm. A number.
    #[serde(default)]
    pub splash: Option<f32>,
    /// Bubbles a second at flow 1 (default 300, 0..3000). Above 3000 is an
    /// error. The total a render may create is also capped (800000).
    #[serde(default)]
    pub density: Option<f32>,
    /// Make the result periodic over the document length: bubble tails wrap
    /// to the start and the hiss filter sees the end as its past. Default:
    /// follows the document (`kind: loop` loops). Give a constant `flow`, or
    /// one that returns to its start value, for a clean seam.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Water {
    const BLURB: &'static str = "running water: a bubble population (Minnaert resonances) plus hiss; flow sets rate and size, depth the size and tone; optional splashes and loop";
    fn example() -> serde_json::Value {
        serde_json::json!({"flow": 0.5, "depth": 0.5, "splash": 0.2, "seed": 1})
    }
}

/// `flow` clamped to 0..1, non-finite as 0.
fn clamp_flow(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl Eval for Water {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.looped != Some(true)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let flow = cx.mono("flow", &self.flow)?;
        if let Some(f) = flow.constant()
            && !(0.0..=1.0).contains(&f)
        {
            return Err(format!("flow: {f}: give a number from 0 to 1"));
        }
        let unit = |field: &str, x: Option<f32>, d: f32| -> Result<f32, String> {
            let x = x.unwrap_or(d);
            if (0.0..=1.0).contains(&x) {
                Ok(x)
            } else {
                Err(format!("{field}: {x}: give a number from 0 to 1"))
            }
        };
        let depth = unit("depth", self.depth, 0.5)?;
        let splash = unit("splash", self.splash, 0.0)?;
        let density = self.density.unwrap_or(300.0);
        if !(density.is_finite() && (0.0..=MAX_DENSITY).contains(&density)) {
            return Err(format!(
                "density: {density}: give bubbles a second from 0 to {MAX_DENSITY} (more is not a stream: lower density or use flow)"
            ));
        }
        let looped = self.looped.unwrap_or(cx.looped);
        let sr = cx.sr;
        let n = cx.n;

        // Work cap: the expected number of bubbles, from the rates.
        let mut expect = 0.0f64;
        for i in 0..n {
            let f = f64::from(clamp_flow(flow.at(i)));
            expect += (f64::from(density) * f * f.sqrt() + f64::from(splash) * f * 4.0 * 40.0)
                / f64::from(sr);
        }
        if expect > MAX_BUBBLES {
            return Err(format!(
                "density: {density}: this render would create about {expect:.0} bubbles (limit {MAX_BUBBLES:.0}): lower density, splash or flow, or shorten the sound"
            ));
        }

        let rmin = min_radius(sr);
        let rmax = MAX_RADIUS_MM * 1e-3;
        let depth_scale = 2.0f32.powf(2.0 * (depth - 0.5));
        let mut rng = cx.rng(self.seed);
        let mut out = vec![0.0f32; n];
        if density > 0.0 {
            for i in 0..n {
                let f = clamp_flow(flow.at(i));
                if f <= 0.0 {
                    continue;
                }
                let rate = density * f * f.sqrt();
                if rng.unit() < rate / sr {
                    let median = (0.8e-3 * 5.0f32.powf(f) * depth_scale).clamp(rmin, rmax);
                    let r = (median * (SIGMA * gauss(&mut rng)).exp()).clamp(rmin, rmax);
                    let b = Pop {
                        start: i,
                        radius: r,
                        amp: A0 * (r / median).sqrt(),
                    };
                    ring(&mut out, &b, sr, looped, RISE, 0);
                }
            }
        }

        // Splashes: their own stream of events.
        if splash > 0.0 {
            let mut srng = cx.rng(self.seed ^ 0x5F1A_5C0D_E5F1_A5C1);
            for i in 0..n {
                let f = clamp_flow(flow.at(i));
                if f > 0.0 && srng.unit() < splash * f * 4.0 / sr {
                    let size = 0.02 * 6.0f32.powf(srng.unit());
                    let seg = splash_buffer(size, &mut srng, sr);
                    for (k, v) in seg.iter().enumerate() {
                        let mut j = i + k;
                        if j >= n {
                            if !looped {
                                break;
                            }
                            j %= n;
                        }
                        out[j] += STREAM_SPLASH_PEAK * v;
                    }
                }
            }
        }

        // Each bubble starts in sine phase, so it leaves a small DC; at high
        // density they add. A 12 Hz high-pass (primed from the end of the
        // buffer when looping) takes it out.
        let mut dc = Biquad::new();
        dc.set(FilterMode::Hp, 12.0, 0.707, 0.0, sr);
        if looped {
            let pre = n.min((0.25 * sr) as usize);
            for &x in &out[n - pre..] {
                dc.tick(x);
            }
        }
        for v in &mut out {
            *v = dc.tick(*v);
        }

        // Hiss: band-limited white noise at unit rms, scaled by flow.
        let mut hrng = cx.rng(self.seed ^ 0x4849_5353_0000_0001);
        let noise: Vec<f32> = (0..n).map(|_| hrng.signed()).collect();
        let mut hp = Biquad::new();
        hp.set(FilterMode::Hp, 400.0, 0.707, 0.0, sr);
        let mut lp = Biquad::new();
        lp.set(
            FilterMode::Lp,
            (10_000.0 * 0.25f32.powf(depth)).min(0.4 * sr),
            0.707,
            0.0,
            sr,
        );
        let mut run = |x: f32| lp.tick(hp.tick(x));
        if looped {
            // The end is the filters' past at the start: no seam.
            let pre = n.min((0.25 * sr) as usize);
            for &x in &noise[n - pre..] {
                run(x);
            }
        }
        let shaped: Vec<f32> = noise.iter().map(|&x| run(x)).collect();
        let ms = shaped.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / n.max(1) as f64;
        let norm = if ms > 0.0 {
            (1.0 / ms.sqrt()) as f32
        } else {
            0.0
        };
        for (i, o) in out.iter_mut().enumerate() {
            *o += shaped[i] * norm * HISS_RMS * clamp_flow(flow.at(i));
        }
        Ok(one(Buf::Audio(out)))
    }
}

// ---------------------------------------------------------------- splash

/// A single splash (source, one-shot): a bright, brief impact burst, then a
/// cloud of bubbles that grow with time (so the pitch falls) and end in the
/// main cavity bubble, built from the same bubble maths as `water`. `size` is
/// the splash's radius in metres: 0.02 (a pebble) to 1 (a person jumping in).
/// Bigger: the main bubble's radius goes as `0.12 * size` (a lower, longer
/// ring), the cloud is larger and lasts longer, the burst is longer. The
/// output is scaled to a peak of 0.5 (-6 dBFS) whatever the size, as
/// `thunder` is: set loudness with `gain` or the output's `lufs`. Not causal
/// (a finished one-shot).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Splash {
    /// Splash radius, metres, 0.005 .. 1 (error outside). 0.05 is a stone
    /// dropped in a pond, 0.3 a body in a pool.
    pub size: f32,
    /// Seconds into the sound the splash happens (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Mixed with the document seed and the node name (default 0: another
    /// seed is another splash of the same size).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Splash {
    const BLURB: &'static str = "one splash: a bright brief burst, then bubbles growing in size (the centroid falls) ending in the main cavity bubble (Minnaert); size in metres; peak levelled to -6 dBFS";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 0.08, "at": 0.05, "seed": 1})
    }
}

impl Eval for Splash {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = positive("size", self.size)?;
        if !(0.005..=1.0).contains(&size) {
            return Err(format!(
                "size: {size}: give the splash radius in metres from 0.005 to 1 (0.05 is a stone in a pond)"
            ));
        }
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let mut rng = cx.rng(self.seed);
        let seg = splash_buffer(size, &mut rng, cx.sr);
        let start = cx.at(at);
        let mut v = vec![0.0f32; cx.n];
        let room = cx.n - start;
        let take = seg.len().min(room);
        for (o, s) in v[start..start + take].iter_mut().zip(&seg) {
            *o = s * SPLASH_PEAK;
        }
        if seg.len() > room {
            // Cut at the document's end: 20 ms fade so it does not click.
            let fade = ((0.02 * cx.sr) as usize).min(take);
            for k in 0..fade {
                v[start + take - 1 - k] *= k as f32 / fade as f32;
            }
        }
        Ok(one(Buf::Audio(v)))
    }
}
