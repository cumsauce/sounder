//! `stream` and `pour`: running and poured water from Andy Farnell's
//! *Designing Sound* Pd patches (RUNNINGWATER/waterflow.pd with its
//! precursors running-water2/3.pd and water1/4.pd; POUREDLIQUIDS/pour1.pd).
//!
//! Both are built on the book's `waterflow` abstraction: a sine whose
//! frequency jumps to `depth + 1600 b` every `rate` ms (b a bipolar
//! exponential random, mostly tiny, sometimes 0.35) over a 2.689 ms ramp, and
//! whose loudness is the square of the low-passed, clipped *rise* of that
//! frequency. Water is a sine whose upward pitch glides are loud. `pour`
//! adds the book's vessel: a four-voice bubble factory, two waterflows
//! (and the bubbles) through four narrow band-passes at 1, 3, 5, 7 x a cavity
//! frequency that rises as the vessel fills.
//!
//! The Pd filters are copies of Pd's own (`bp~`, `lop~`), as in `farnell`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// The sample rate the patches were written at: their per-sample numbers
/// (the clip of the frequency slope) are scaled to it.
const PD_SR: f32 = 44_100.0;
/// Make-up gain: the patches leave the sine at about -48 dB rms (the book
/// raises the volume by hand), which no output stage should have to find.
const STREAM_GAIN: f32 = 20.0;
const POUR_GAIN: f32 = 12.0;

/// Pd's `lop~`: y += k (x - y), k = 2 pi f / sr.
#[derive(Default, Clone)]
struct PdLop {
    k: f32,
    y: f32,
}

impl PdLop {
    fn new(f: f32, sr: f32) -> PdLop {
        PdLop {
            k: (f * TAU / sr).clamp(0.0, 1.0),
            y: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// Pd's `bp~` (centre Hz, Q): a two-pole resonator, centre gain about 1.
#[derive(Default, Clone)]
struct PdBp {
    c1: f32,
    c2: f32,
    gain: f32,
    x1: f32,
    x2: f32,
}

impl PdBp {
    // Pd's d_filter.c writes pi as 3.14159; keep it so the coefficients
    // match Pd's.
    #[allow(clippy::approx_constant)]
    fn set(&mut self, f: f32, q: f32, sr: f32) {
        // Pd's polynomial cosine, which is 0 above a quarter of the rate.
        let qcos = |f: f32| {
            if (-(0.5 * 3.14159)..=0.5 * 3.14159).contains(&f) {
                let g = f * f;
                ((g * g * g * (-1.0 / 720.0) + g * g * (1.0 / 24.0)) - g * 0.5) + 1.0
            } else {
                0.0
            }
        };
        let f = if f < 0.001 { 10.0 } else { f };
        let omega = f * (2.0 * 3.14159) / sr;
        let mut omr = if q < 0.001 { 1.0 } else { omega / q };
        if omr > 1.0 {
            omr = 1.0;
        }
        let r = 1.0 - omr;
        self.c1 = 2.0 * qcos(omega) * r;
        self.c2 = -r * r;
        self.gain = 2.0 * omr * (omr + r * omega);
    }
    fn tick(&mut self, x: f32) -> f32 {
        let o = x + self.c1 * self.x1 + self.c2 * self.x2;
        self.x2 = self.x1;
        self.x1 = o;
        self.gain * o
    }
}

/// The patches' `bilexp`: `exp(9 m) / 23000` with a random sign, m uniform in
/// 0..1 (from `random 8192`: its `mod 4096`, and `> 4096` for the sign).
fn bilexp(rng: &mut Rng) -> f32 {
    let r = (rng.next_u64() % 8192) as f32;
    let m = (r % 4096.0) / 4096.0;
    let sign = if r > 4096.0 { 1.0 } else { -1.0 };
    sign * (9.0 * m).exp() / 23000.0
}

/// One `waterflow` abstraction: a mono layer `n` samples long.
///
/// `depth(t)` is the base Hz and `period_ms(t)` the metro period, both read
/// when a tick fires (the patch's inlets are cold). With `wrap` the period is
/// rounded to a whole number of ticks in the buffer, the trajectory runs twice
/// and the second pass is kept, and the phase is nudged (at most half a cycle
/// over the buffer) to a whole number of cycles, so the buffer repeats with no
/// seam.
fn waterflow(
    n: usize,
    sr: f32,
    rng: &mut Rng,
    depth: impl Fn(f32) -> f32,
    period_ms: impl Fn(f32) -> f32,
    range: f32,
    slew_ms: f32,
    wrap: bool,
) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    // Ticks: (sample, target Hz).
    let mut ticks: Vec<(usize, f32)> = Vec::new();
    if wrap {
        let step0 = (period_ms(0.0) * 0.001 * sr).max(1.0);
        let count = ((n as f32 / step0).round() as usize).max(1);
        let step = n as f64 / count as f64;
        for k in 0..count {
            let i = (k as f64 * step) as usize;
            let t = i as f32 / sr;
            ticks.push((i, depth(t) + range * bilexp(rng)));
        }
    } else {
        let mut at = 0.0f64;
        while (at as usize) < n {
            let i = at as usize;
            let t = i as f32 / sr;
            ticks.push((i, depth(t) + range * bilexp(rng)));
            at += f64::from((period_ms(t) * 0.001 * sr).max(1.0));
        }
    }
    let ramp = (slew_ms * 0.001 * sr).max(1.0);
    let slope_scale = sr / PD_SR;
    let mut f = depth(0.0);
    let mut prev = f;
    let (mut step, mut left) = (0.0f32, 0.0f32);
    let mut lop = PdLop::new(10.0, sr);
    let passes = if wrap { 2 } else { 1 };
    let mut freq = vec![0.0f32; n];
    let mut env = vec![0.0f32; n];
    for _ in 0..passes {
        let mut next = 0usize;
        for i in 0..n {
            while next < ticks.len() && ticks[next].0 <= i {
                step = (ticks[next].1 - f) / ramp;
                left = ramp;
                next += 1;
            }
            if left > 0.0 {
                f += step;
                left -= 1.0;
            }
            // fexpr~ $x1 - $x1[-1] -> clip~ 0 1 -> lop~ 10 -> * 0.9, squared.
            let d = ((f - prev) * slope_scale).clamp(0.0, 1.0);
            prev = f;
            let e = 0.9 * lop.tick(d);
            freq[i] = f;
            env[i] = e * e;
        }
    }
    // osc~ is a cosine; `wrap` nudges the frequency to a whole number of cycles.
    let cycles: f64 = freq.iter().map(|&x| f64::from(x) / f64::from(sr)).sum();
    let nudge = if wrap {
        (cycles.round() - cycles) * f64::from(sr) / n as f64
    } else {
        0.0
    };
    let mut phase = 0.0f64;
    (0..n)
        .map(|i| {
            let y = 0.3 * env[i] * (TAU * phase as f32).cos();
            phase = (phase + (f64::from(freq[i]) + nudge) / f64::from(sr)).fract();
            y
        })
        .collect()
}

fn check_range(field: &str, x: f32, lo: f32, hi: f32, unit: &str) -> Result<f32, String> {
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give {unit} from {lo} to {hi}"))
    }
}

// ------------------------------------------------------------------ stream

/// A brook (RUNNINGWATER/waterflow.pd, running-water3.pd; the controls of
/// water4.pd): `layers` independent copies of the book's `waterflow`, summed.
/// Each is a sine whose pitch jumps every `6 / flow` ms to `depth` plus a
/// random excursion of up to 0.35 x `range` Hz (mostly a few Hz, now and then
/// hundreds), over a 2.689 ms glide; only the upward glides are loud.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stream {
    /// Flow rate, 0.1 to 10 (default 1: a pitch jump every 6 ms, the
    /// patch's metro). More is a faster, denser brook; 0.2 a trickle.
    #[serde(default)]
    pub flow: Option<f32>,
    /// Base pitch of the burble, Hz, 100 to 5000 (default 800, the patch's
    /// `depth`): lower reads as a deeper, bigger stream.
    #[serde(default)]
    pub depth: Option<f32>,
    /// Width of the pitch excursions, Hz, 0 to 5000 (default 1600: the
    /// patch's x 1600); water4's `range`.
    #[serde(default)]
    pub range: Option<f32>,
    /// Independent waterflows summed, 1 to 6 (default 2, as in pour1.pd).
    /// The sum is divided by sqrt(layers), so more layers thicken the brook
    /// without making it louder.
    #[serde(default)]
    pub layers: Option<u32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that repeats with no seam. Unset: yes in a `kind:
    /// loop` document, no otherwise. The pitch-jump period is rounded to a
    /// whole number in the duration and the phase nudged by at most half a
    /// cycle over it.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Stream {
    const BLURB: &'static str = "a brook (Farnell's waterflow): sines whose pitch jumps at random, loud only on the upward glides; flow = jumps a second / 167, depth = base Hz, range = excursion Hz, layers; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"flow": 1, "depth": 800, "range": 1600, "layers": 2, "seed": 1})
    }
}

impl Eval for Stream {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.looped != Some(true)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let flow = check_range("flow", self.flow.unwrap_or(1.0), 0.1, 10.0, "a number")?;
        let depth = check_range("depth", self.depth.unwrap_or(800.0), 100.0, 5000.0, "Hz")?;
        let range = check_range("range", self.range.unwrap_or(1600.0), 0.0, 5000.0, "Hz")?;
        let layers = self.layers.unwrap_or(2);
        if !(1..=6).contains(&layers) {
            return Err(format!("layers: {layers}: give 1 to 6"));
        }
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let mut sum = vec![0.0f32; cx.n];
        for _ in 0..layers {
            let mut r = Rng::new(rng.next_u64());
            let y = waterflow(
                cx.n,
                cx.sr,
                &mut r,
                |_| depth,
                |_| 6.0 / flow,
                range,
                2.689,
                wrap,
            );
            for (s, v) in sum.iter_mut().zip(&y) {
                *s += v;
            }
        }
        let norm = STREAM_GAIN / (layers as f32).sqrt();
        for s in &mut sum {
            *s *= norm;
        }
        Ok(one(Buf::Audio(sum)))
    }
}

// -------------------------------------------------------------------- pour

/// Liquid poured into a vessel (POUREDLIQUIDS/pour1.pd): two waterflows and a
/// four-voice bubble factory go through four band-passes (Q 30) at 1, 3, 5
/// and 7 x a cavity frequency, an air column that shortens as the vessel
/// fills, so the pitch rises over `fill_time`. The stream thins as it fills
/// (depth 900 -> 600 Hz) and, as the pour stops, speeds up for 0.8 s; the
/// whole is faded in over 0.3 s and out for 1.2 s from the end, through a
/// 0.5 Hz low-pass.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pour {
    /// Seconds the vessel takes to fill, 0.5 to 60 (default 9, the patch's
    /// 9000 ms). The pitch sweep, the bubbles' pitch and the stream's
    /// thinning follow it; the sound lasts this plus about 1.5 s.
    #[serde(default)]
    pub fill_time: Option<f32>,
    /// Vessel size, 0.25 to 4 (default 1: the cavity sweeps 100 -> 2100 Hz,
    /// the patch). The cavity, the bubbles and the stream's depth are all
    /// divided by it: 2 is a vessel twice as tall, an octave lower.
    #[serde(default)]
    pub vessel: Option<f32>,
    /// Bubble rate, 0 to 3 (default 1: the patch's 29 % chance every 20 ms,
    /// about 14 a second). 0 is a silent bubble factory.
    #[serde(default)]
    pub bubbles: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Pour {
    const BLURB: &'static str = "liquid poured into a vessel (Farnell's pour1): two waterflows and bubbles through resonators at 1,3,5,7 x a cavity frequency that rises as it fills; fill_time = seconds to fill, vessel = size (pitch / vessel), bubbles";
    fn example() -> serde_json::Value {
        serde_json::json!({"fill_time": 3, "vessel": 1, "bubbles": 1, "seed": 1})
    }
}

/// Time constant of Pd's `line` (its default grain), seconds.
const GRAIN: f32 = 0.02;
/// Ramp-in and fade-out of the overall level, seconds (the patch's
/// `1 300` and `0 1200`), and the length of the speed-up at the end.
const FADE_IN: f32 = 0.3;
const FADE_OUT: f32 = 1.2;
const SPEED_UP: f32 = 0.8;
/// Level of the bubble factory into the pipe (the patch's `*~ 0.05`).
const BUBBLE_LEVEL: f32 = 0.05;

/// One bubble voice of `bubblefactory`.
#[derive(Default, Clone)]
struct Bubble {
    live: bool,
    /// Samples since its trigger.
    age: f32,
    /// Length of its envelope, samples.
    len: f32,
    /// Hz added to the sweep: 800 base (1 + 2 a).
    start_hz: f32,
    /// 0.6 + vel / 3, the amplitude of the x^4 fall.
    amp: f32,
    /// 1.1 - a: bigger bubbles are quieter.
    size_gain: f32,
    phase: f64,
    lop: Option<PdLop>,
}

impl Eval for Pour {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let fill = check_range(
            "fill_time",
            self.fill_time.unwrap_or(9.0),
            0.5,
            60.0,
            "seconds",
        )?;
        let vessel = check_range("vessel", self.vessel.unwrap_or(1.0), 0.25, 4.0, "a size")?;
        let bubbles = check_range("bubbles", self.bubbles.unwrap_or(1.0), 0.0, 3.0, "a rate")?;
        let (n, sr) = (cx.n, cx.sr);
        let mut rng = cx.rng(self.seed);
        // x: the fill line 0 -> 1 over fill_time.
        let x_at = move |t: f32| (t / fill).clamp(0.0, 1.0);
        // The rate line: 0, then 1 over 0.8 s once the pour stops.
        let rate_at = move |t: f32| ((t - fill) / SPEED_UP).clamp(0.0, 1.0);
        let depth = move |t: f32| (600.0 + 300.0 * (1.0 - x_at(t))) / vessel;
        let period = move |t: f32| 4.0 + 24.0 * rate_at(t);
        let mut mix = vec![0.0f32; n];
        for _ in 0..2 {
            let mut r = Rng::new(rng.next_u64());
            let y = waterflow(n, sr, &mut r, depth, period, 1600.0, 2.689, false);
            for (m, v) in mix.iter_mut().zip(&y) {
                *m += v;
            }
        }
        // bubblefactory: a metro 20 draws a gate, then a size a and a velocity
        // v; an open gate starts the next of 4 voices (poly 4 1 steals the
        // oldest, so they take turns).
        let mut r = Rng::new(rng.next_u64());
        let tick = (GRAIN * sr) as usize;
        let open_below = (29.0 * bubbles).round() as u64;
        let mut voices: Vec<Bubble> = vec![Bubble::default(); 4];
        let mut turn = 0usize;
        for i in 0..n {
            if i % tick.max(1) == 0 {
                let gate = r.next_u64() % 100;
                let a = (r.next_u64() % 1000) as f32;
                let v = (r.next_u64() % 1000) as f32;
                if gate < open_below && v > 0.0 {
                    let a = a / 1000.0;
                    let b0 = x_at(i as f32 / sr);
                    let vo = &mut voices[turn];
                    turn = (turn + 1) % 4;
                    *vo = Bubble {
                        live: true,
                        age: 0.0,
                        len: 80.0 / (a + 0.4) * 0.001 * sr,
                        start_hz: 800.0 * b0 * (1.0 + 2.0 * a),
                        amp: 0.6 + v / 3000.0,
                        size_gain: 1.1 - a,
                        phase: vo.phase,
                        lop: vo.lop.take(),
                    };
                }
            }
            let mut s = 0.0f32;
            for vo in voices.iter_mut().filter(|v| v.live) {
                // vline~ 1 -> 0 over len; x^4 is the loudness, 1 - x^4 the
                // pitch's rise to +2000 Hz.
                let u = (1.0 - vo.age / vo.len).max(0.0);
                let u4 = u * u * u * u;
                let hz = (vo.start_hz + 2000.0 * (1.0 - u4)) / vessel;
                vo.phase = (vo.phase + f64::from(hz / sr)).fract();
                let lop = vo.lop.get_or_insert_with(|| PdLop::new(120.0, sr));
                let a = lop.tick(u4 * vo.amp);
                s += (TAU * vo.phase as f32).cos() * a * vo.size_gain;
                vo.age += 1.0;
                if vo.age > vo.len * 1.5 {
                    vo.live = false;
                }
            }
            mix[i] += BUBBLE_LEVEL * s;
        }
        // openpipe: bp~ at L, 3L, 5L, 7L (Q 30), L = (100 + 2000 x) / vessel,
        // re-tuned on Pd's 20 ms line grain; amp = lop~ 0.5 of the level line.
        let mut bp = vec![PdBp::default(); 4];
        let grain = ((GRAIN * sr) as usize).max(1);
        let mut amp_lop = PdLop::new(0.5, sr);
        let mut line_amp = 0.0f32;
        let mut out = vec![0.0f32; n];
        let mut mute = [false; 4];
        for i in 0..n {
            let t = i as f32 / sr;
            if i % grain == 0 {
                let l = (100.0 + 2000.0 * x_at(t)) / vessel;
                for (k, f) in bp.iter_mut().zip([1.0f32, 3.0, 5.0, 7.0]).enumerate() {
                    let hz = l * f.1;
                    // The patch's bp~ breaks above a quarter of the rate; keep
                    // the partials well under Nyquist and mute the rest.
                    mute[k] = hz > 0.4 * sr;
                    if !mute[k] {
                        f.0.set(hz, 30.0, sr);
                    }
                }
                // line: 0 -> 1 over 0.3 s, then 1 -> 0 over 1.2 s from fill_time.
                line_amp = if t < fill {
                    (t / FADE_IN).min(1.0)
                } else {
                    (1.0 - (t - fill) / FADE_OUT)
                        .clamp(0.0, 1.0)
                        .min((t / FADE_IN).min(1.0))
                };
            }
            let a = amp_lop.tick(line_amp);
            let x = mix[i];
            let mut y = 0.0;
            for (k, f) in bp.iter_mut().enumerate() {
                if !mute[k] {
                    y += f.tick(x);
                }
            }
            out[i] = POUR_GAIN * y * a;
        }
        Ok(one(Buf::Audio(out)))
    }
}
