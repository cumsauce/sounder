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
use crate::dsp::{Biquad, FilterMode, OnePole, Rng};
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
/// ln-sigma of the stream's log-normal radius spread.
const SIGMA: f32 = 0.4;
/// Peak amplitude of a median stream bubble.
const A0: f32 = 0.1;
/// RMS of the hiss at flow 1.
const HISS_RMS: f32 = 0.03;
/// Share of the hiss that is steady; the rest follows the bubble envelope.
/// Round 2: 0.45 (0.1 read "crispy, like boiling oil"; the old steady
/// hiss read as wind).
const HISS_FLOOR: f32 = 0.45;
/// Bubble rate (per second) under which the hiss is fully steady, as before.
const HISS_TRICKLE: f32 = 20.0;
/// Hiss level of a dense stream against a sparse one (rms 0.03 x this).
const HISS_DENSE: f32 = 0.8;
/// Bubble rate (per second) under which the hiss turns back to steady.
const HISS_SPARSE: f32 = 80.0;
/// High-pass under the hiss, Hz.
const HISS_HP: f32 = 400.0;
/// Extra high-pass (4th order) on a dense stream's hiss, Hz.
const HISS_DENSE_HP: f32 = 1500.0;
/// Peak a lone `splash` node is scaled to.
const SPLASH_PEAK: f32 = 0.5;
/// Peak of the splashes mixed into a stream by `water.splash`.
const STREAM_SPLASH_PEAK: f32 = 0.3;
/// Silence before the burst, seconds: the output's default 2 ms fade-in would
/// otherwise cut the loudest part of the sound.
const LEAD: f32 = 0.0025;
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
/// (peak 1), after Farnell's raindrop and bubble recipes (Designing Sound,
/// ch. 35 and 38), fitted to refs/splash_set. With `k = sqrt(size / 0.1)`:
///
/// - impact: a positive pressure bump, a raised-cosine pulse `5 ms * k`
///   (3..30 ms) long, high-passed at 40 Hz, level 0.9 * `burst`.
/// - slap: white noise falling 60 dB in `60 ms * k`, band-passed at 3.5 kHz
///   Q 0.7, level 1.4 * `burst`.
/// - spray: the same noise falling 60 dB in `350 ms * k * tail`, band-passed
///   at 3.2 kHz Q 0.6, level 0.6 * `burst`, from 3 ms.
/// - bubbles: `140 * k * tail` (at most 400) over `0.5 s * k * tail`, dense
///   early (time = spread * u^2); each has a size v (0.05..1): a sine whose
///   pitch RISES from `f = (350 + (1 - v) * 2600 * (0.8..1.2)) / k^0.3` to
///   1.7 f in `d = (20 + 90 v) ms * k^0.3`, a linear rise over 0.15 d and a
///   fall of 60 dB in 1.6 d, level `0.1 + v * (0.6..1)`; summed, high-passed
///   at 60 Hz, level 0.07.
///
/// The earlier model (a slap, a swarm of chirping Minnaert bubbles, spray) was
/// heard as "diminishing" against this one in an A/B (book check 1).
fn splash_buffer(size: f32, tail: f32, burst_level: f32, rng: &mut Rng, sr: f32) -> Vec<f32> {
    let k = (size / 0.1).max(1e-3).sqrt();
    let kp = k.powf(0.3);
    let at = |t: f32| (t * sr) as usize;
    let spread = 0.5 * k * tail;
    let count = (140.0 * k * tail).round().min(400.0) as usize;
    let spray_t60 = 0.35 * k * tail;
    let len = (at(LEAD + spread + 0.2 * kp + 0.01))
        .max(at(LEAD + 0.003 + spray_t60))
        .min(at(MAX_SPLASH_SECONDS))
        .max(1);
    let mut out = vec![0.0f32; len];
    let lead = at(LEAD);
    let tau = |t60: f32| t60 / 1000.0f32.ln();

    if burst_level > 0.0 {
        // Impact bump.
        let blen = at((0.005 * k).clamp(0.003, 0.03)).max(2);
        let mut hp = OnePole::default();
        for i in 0..len.saturating_sub(lead) {
            let x = if i < blen {
                0.5 - 0.5 * (TAU * i as f32 / blen as f32).cos()
            } else {
                0.0
            };
            let (_, h) = hp.tick(x, 40.0, sr);
            out[lead + i] += 0.9 * burst_level * h;
            if i > blen && h.abs() < 1e-6 {
                break;
            }
        }
        // Slap and spray from one noise.
        let mut fs = Biquad::new();
        fs.set(FilterMode::Bp, 3500.0f32.min(0.4 * sr), 0.7, 0.0, sr);
        let mut fp = Biquad::new();
        fp.set(FilterMode::Bp, 3200.0f32.min(0.4 * sr), 0.6, 0.0, sr);
        let (ts, tp) = (tau(0.06 * k) * sr, tau(spray_t60.max(1e-3)) * sr);
        let d = at(0.003) as f32;
        for i in 0..len.saturating_sub(lead) {
            let n = rng.signed();
            let x = i as f32;
            let es = (-x / ts).exp();
            let ep = if tail > 0.0 && x >= d {
                (-(x - d) / tp).exp()
            } else {
                0.0
            };
            out[lead + i] +=
                1.4 * burst_level * fs.tick(n * es) + 0.6 * burst_level * fp.tick(n * ep);
        }
    }

    // Bubbles with a rising pitch.
    let mut bub = vec![0.0f32; len];
    for j in 0..count {
        let u = (j as f32 + rng.unit()) / count as f32;
        let t0 = LEAD + 0.005 + spread * u * u;
        let v = 0.05 + 0.95 * rng.unit();
        let s = v * (0.6 + 0.4 * rng.unit());
        let f = (350.0 + (1.0 - v) * 2600.0 * (0.8 + 0.4 * rng.unit())) / kp;
        let dur = (0.02 + 0.09 * v) * kp;
        let (att, t60) = (0.15 * dur, 1.6 * dur);
        let g = 0.1 + s;
        let mut ph = 0.0f32;
        let i0 = at(t0);
        for i in 0..at(t60) {
            let Some(o) = bub.get_mut(i0 + i) else { break };
            let t = i as f32 / sr;
            let hz = f * 1.7f32.powf((t / dur).min(1.0));
            ph = (ph + hz / sr).fract();
            let e = (t / att).min(1.0) * (-t / tau(t60)).exp();
            *o += g * e * (TAU * ph).sin();
        }
    }
    let mut hp = OnePole::default();
    for (o, b) in out.iter_mut().zip(&bub) {
        *o += 0.07 * hp.tick(*b, 60.0, sr).1;
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
/// module comment. The hiss is white noise through a 1.5 kHz high-pass and a
/// low-pass at `10 kHz * 0.25^depth`, at rms `0.02 * flow` times a gain that
/// follows the bubbles' envelope (10 % steady, 90 % riding the pops). Flow 0
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
    /// one that returns to its start value, for a clean seam. The node's own
    /// output is periodic, but a node AFTER it is not: a filter (or any
    /// recursive node) starts from zero state, so its output has a startup
    /// at the wrap and `loop_seam` fails (measured: hp 250 Hz + lp 6 kHz
    /// after it failed 35 % of 480 seeds / flows / densities; with
    /// `crossfade_loop` after the filters, 1 of 480). End the chain with
    /// `crossfade_loop`. `splash` is a sparse random event: in a short loop
    /// a splash in one end window and none in the other can fail the
    /// spectral half of `loop_seam` (about 17 % of cases at splash 0.5):
    /// raise `splash` and `flow`, or crossfade.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
    /// Tone, 0..1 (default 0.5, as before, bit-identical): a high shelf at
    /// 2 kHz of up to +-12 dB (`(tone - 0.5) * 24` dB) over everything the
    /// node makes (the hiss, the bubble pops and the splashes): 1 is 12 dB
    /// brighter above 2 kHz, 0 is 12 dB darker. It is part of the render
    /// (primed from the end of the buffer in a loop), so a loop stays
    /// periodic; a filter after the node would not. The level is not
    /// re-normalised. A number.
    #[serde(default)]
    pub tone: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Water {
    const BLURB: &'static str = "running water: a bubble population (Minnaert resonances) plus hiss; flow sets rate and size, depth the size and tone; optional splashes and loop (periodic by itself; filters after it need crossfade_loop to keep the seam)";
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
        let tone = unit("tone", self.tone, 0.5)?;
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
                    let seg = splash_buffer(size, 1.0, 1.0, &mut srng, sr);
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
        hp.set(FilterMode::Hp, HISS_HP, 0.707, 0.0, sr);
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
        let unit_rms = |x: &[f32]| {
            let ms = x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / n.max(1) as f64;
            if ms > 0.0 {
                (1.0 / ms.sqrt()) as f32
            } else {
                0.0
            }
        };
        let norm = unit_rms(&shaped);
        // A dense stream's hiss has its 400 Hz-1.5 kHz part taken out too:
        // a steady noise bed there is the wind-like band.
        let (mut ha, mut hb) = (Biquad::new(), Biquad::new());
        ha.set(FilterMode::Hp, HISS_DENSE_HP.min(0.4 * sr), 0.707, 0.0, sr);
        hb.set(FilterMode::Hp, HISS_DENSE_HP.min(0.4 * sr), 0.707, 0.0, sr);
        let mut run2 = |x: f32| hb.tick(ha.tick(x));
        if looped {
            let pre = n.min((0.25 * sr) as usize);
            for &x in &shaped[n - pre..] {
                run2(x);
            }
        }
        let thin: Vec<f32> = shaped.iter().map(|&x| run2(x)).collect();
        let norm_thin = unit_rms(&thin);
        // The hiss rides the bubbles: its gain follows their envelope (2 ms
        // up, 25 ms down, normalised to mean 1), over a small steady floor.
        // A steady broadband bed is what the batch-2 listener heard as wind.
        let (up, down) = ((-1.0 / (0.002 * sr)).exp(), (-1.0 / (0.025 * sr)).exp());
        let mut e = 0.0f32;
        let mut follow = |x: f32| {
            let a = x.abs();
            let c = if a > e { up } else { down };
            e = c * e + (1.0 - c) * a;
            e
        };
        if looped {
            let pre = n.min((0.25 * sr) as usize);
            for &x in &out[n - pre..] {
                follow(x);
            }
        }
        let env: Vec<f32> = out.iter().map(|&x| follow(x)).collect();
        let mean = env.iter().map(|v| f64::from(*v)).sum::<f64>() / n.max(1) as f64;
        // Normalise by the envelope's local mean (0.4 s centred, wrapped in a
        // loop) so the hiss follows each pop while its level over a few
        // hundred ms stays even: a loop's two ends then match.
        let mut pre = vec![0.0f64; n + 1];
        for (i, v) in env.iter().enumerate() {
            pre[i + 1] = pre[i] + f64::from(*v);
        }
        let h = ((0.2 * sr) as usize).min(n / 2);
        let local = |i: usize| -> f64 {
            if n == 0 {
                return 0.0;
            }
            let (a, b) = (i as i64 - h as i64, i as i64 + h as i64 + 1);
            let sum = |a: i64, b: i64| pre[b as usize] - pre[a as usize];
            let n_i = n as i64;
            let total = if looped {
                let mut t = sum(a.max(0), b.min(n_i));
                if a < 0 {
                    t += sum((n_i + a).max(0), n_i);
                }
                if b > n_i {
                    t += sum(0, (b - n_i).min(n_i));
                }
                t / (b - a) as f64
            } else {
                let (a, b) = (a.max(0), b.min(n_i));
                sum(a, b) / (b - a).max(1) as f64
            };
            total.max(0.25 * mean)
        };
        for (i, o) in out.iter_mut().enumerate() {
            let m = local(i);
            let inv = if m > 0.0 { (1.0 / m) as f32 } else { 0.0 };
            // A sparse trickle (under HISS_SPARSE bubbles a second) keeps a
            // steadier hiss: with a few pops a second a bubble-gated hiss is
            // mostly silence, and a loop's two ends would differ by chance.
            let f = clamp_flow(flow.at(i));
            let sparse = ((HISS_SPARSE - density * f * f.sqrt()) / (HISS_SPARSE - HISS_TRICKLE))
                .clamp(0.0, 1.0);
            let floor = HISS_FLOOR + (1.0 - HISS_FLOOR) * sparse;
            let g = floor + (1.0 - floor) * (env[i] * inv).min(4.0);
            let h = sparse * shaped[i] * norm + (1.0 - sparse) * thin[i] * norm_thin;
            *o += h * HISS_RMS * (HISS_DENSE + (1.0 - HISS_DENSE) * sparse) * g * f;
        }
        if tone != 0.5 {
            // A 2 kHz high shelf, +-12 dB. In a loop the filter is primed
            // with the end of the (periodic) buffer, so the start has the
            // state it would have had after one more lap.
            let mut shelf = Biquad::new();
            shelf.set(
                FilterMode::HighShelf,
                2000.0f32.min(0.4 * sr),
                0.707,
                (tone - 0.5) * 24.0,
                sr,
            );
            if looped {
                let pre = n.min((0.25 * sr) as usize);
                for &x in &out[n - pre..] {
                    shelf.tick(x);
                }
            }
            for v in &mut out {
                *v = shelf.tick(*v);
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}

// ---------------------------------------------------------------- splash

/// A single splash (source, one-shot), after Farnell's raindrop and bubble
/// recipes: an impact bump, a slap and spray, then a cloud of short bubbles
/// whose pitch rises (see `splash_buffer`). `size` is the splash's radius in
/// metres: 0.02 (a pebble) to 1 (a person jumping in). Bigger: a longer
/// bump, slap and spray, more bubbles over a longer time, lower and longer
/// bubbles. The
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
    /// 0..3, the bubble cloud and spray after the impact (default 1): their
    /// length and the bubble count are times `tail`; 0 is no cloud and no
    /// spray. A splash never rings past 2.5 s.
    #[serde(default)]
    pub tail: Option<f32>,
    /// 0..2, the level of the impact (bump, slap, spray) against the bubbles
    /// (default 1); 0 leaves only the bubbles. The splash is then levelled to
    /// its usual peak, so a louder burst makes the bubbles quieter.
    #[serde(default)]
    pub burst: Option<f32>,
    /// Mixed with the document seed and the node name (default 0: another
    /// seed is another splash of the same size).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Splash {
    const BLURB: &'static str = "one splash (Farnell's raindrop and bubble recipes): an impact bump, a slap and spray, then a cloud of bubbles whose pitch rises; size in metres; peak levelled to -6 dBFS";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 0.08, "at": 0.05, "tail": 1.5, "burst": 1.5, "seed": 1})
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
        let tail = self.tail.unwrap_or(1.0);
        if !(0.0..=3.0).contains(&tail) {
            return Err(format!(
                "tail: {tail}: give 0..3 (1 = the cloud as it is, 2 = twice as long and as many bubbles, 0 = no cloud)"
            ));
        }
        let burst = self.burst.unwrap_or(1.0);
        if !(0.0..=2.0).contains(&burst) {
            return Err(format!(
                "burst: {burst}: give 0..2 (1 = as it is; 2 = the opening burst twice the level of the cavity bubble, so the loudest moment)"
            ));
        }
        let mut rng = cx.rng(self.seed);
        let seg = splash_buffer(size, tail, burst, &mut rng, cx.sr);
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
