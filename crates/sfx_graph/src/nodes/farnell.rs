//! Nodes ported from the Pure Data patches of Andy Farnell's *Designing
//! Sound* (MIT Press), each checked against the patch rendered by real Pd in
//! 2008-era compatibility mode (see `docs/porting-farnell.md`). `bell` is BELL/A4-bell-telephone.pd, `ball_bounce`
//! BOUNCINGBALL/bb1.pd, `cricket` INSECTS/cricket2.pd, `twang`
//! MRBOINGY/twang.pd.
//!
//! The filters below are exact copies of Pd's (`d_filter.c`, `d_osc.h`), so
//! the patches' gains and resonances carry over unchanged: `bp~` is a plain
//! two-pole resonator whose centre gain is not 1, `hip~` is the pre-0.44
//! un-normalised one-pole, `vcf~` a complex one-pole.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{mono, non_negative, one};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Pd's `bp~` (centre Hz, Q).
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
    // match Pd's bit for bit.
    #[allow(clippy::approx_constant)]
    fn new(f: f32, q: f32, sr: f32) -> PdBp {
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
        PdBp {
            c1: 2.0 * qcos(omega) * r,
            c2: -r * r,
            gain: 2.0 * omr * (omr + r * omega),
            x1: 0.0,
            x2: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let o = x + self.c1 * self.x1 + self.c2 * self.x2;
        self.x2 = self.x1;
        self.x1 = o;
        self.gain * o
    }
}

/// Pd's `hip~` before 0.44 (no gain normalisation).
#[derive(Default, Clone)]
struct PdHip {
    coef: f32,
    last: f32,
}

impl PdHip {
    fn new(f: f32, sr: f32) -> PdHip {
        PdHip {
            coef: (1.0 - f * TAU / sr).clamp(0.0, 1.0),
            last: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.coef >= 1.0 {
            return x;
        }
        let new = x + self.coef * self.last;
        let out = new - self.last;
        self.last = new;
        out
    }
}

/// Pd's `lop~`.
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

/// Pd's `vcf~` (the real output), centre given per sample.
#[derive(Default, Clone)]
struct PdVcf {
    re: f32,
    im: f32,
}

impl PdVcf {
    fn tick(&mut self, x: f32, hz: f32, q: f32, sr: f32) -> f32 {
        let qinv = if q > 0.0 { 1.0 / q } else { 0.0 };
        let amp = 2.0 - 2.0 / (q + 2.0);
        let cf = (hz * TAU / sr).max(0.0);
        let r = if qinv > 0.0 {
            (1.0 - cf * qinv).max(0.0)
        } else {
            0.0
        };
        let (cr, ci) = (r * cf.cos(), r * cf.sin());
        let re2 = self.re;
        self.re = amp * (1.0 - r) * x + cr * re2 - ci * self.im;
        self.im = ci * re2 + cr * self.im;
        self.re
    }
}

/// Strike times: a number or a list of seconds (default one at 0.05 s).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Times {
    One(f32),
    Many(Vec<f32>),
}

fn times(t: &Option<Times>, default: f32, field: &str) -> Result<Vec<f32>, String> {
    let v = match t {
        None => vec![default],
        Some(Times::One(x)) => vec![*x],
        Some(Times::Many(v)) => v.clone(),
    };
    for x in &v {
        non_negative(field, *x)?;
    }
    Ok(v)
}

// -------------------------------------------------------------------- bell

/// A telephone bell (BELL/A4-bell-telephone.pd): five groups of sine partials
/// over `freq`, each with its own envelope from one strike, plus the strike
/// itself through the bell's body (a 59 ms loop through band-passes at 287,
/// 431 and 1243 Hz, Q 12, clipped -0.2..0.5), the sum clipped at +-0.9.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bell {
    /// Fundamental, Hz, 50 to 3000 (default 750). Partials sit at 0.5, 1,
    /// 2, 2.002, 2.5, 2.57, 3, 6.24, 9.6, 11, 12.5, 16 and 24 times it.
    #[serde(default)]
    pub freq: Option<f32>,
    /// Strike time(s), seconds: a number or a list (default 0.05).
    #[serde(default)]
    pub at: Option<Times>,
    /// Ring length, 0.1 to 10 times the book's (default 1: the longest
    /// partials fall to 0 over 720 ms).
    #[serde(default)]
    pub ring: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Bell {
    const BLURB: &'static str = "a telephone bell (Farnell): partial groups at fixed ratios of freq with their own envelopes, a strike and the bell's body resonance; at = strike time(s)";
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 750, "at": [0.05, 1.55], "ring": 1})
    }
}

impl Eval for Bell {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let f = self.freq.unwrap_or(750.0);
        if !(f.is_finite() && (50.0..=3000.0).contains(&f)) {
            return Err(format!("freq: {f}: give Hz from 50 to 3000"));
        }
        let ring = self.ring.unwrap_or(1.0);
        if !(ring.is_finite() && (0.1..=10.0).contains(&ring)) {
            return Err(format!("ring: {ring}: give 0.1 to 10"));
        }
        let mut hits: Vec<usize> = times(&self.at, 0.05, "at")?
            .iter()
            .map(|t| (t * cx.sr) as usize)
            .collect();
        hits.sort_unstable();
        let sr = cx.sr;
        // pd envelopes: 600 ms x 1.2, 0.9, 1/2, 1/7, 1/9 (x ring).
        let lens = [720.0, 540.0, 300.0, 600.0 / 7.0, 600.0 / 9.0].map(|ms| ms * 0.001 * ring * sr);
        // (ratio, gain, group): fund+sub, primaries, secondaries, circulars, strike.
        let partials: [(f32, f32, usize); 11] = [
            (1.0, 0.02, 0),
            (0.5001, 0.1 * 0.02, 0),
            (2.002, 0.2 * 0.04, 1),
            (9.6, 0.5 * 0.04, 1),
            (3.0, 0.1 * 0.04, 1),
            (11.0, 0.2 * 0.2, 2),
            (2.571, 0.1 * 0.2, 2),
            (2.4999, 0.1 * 0.2, 2),
            (6.242, 0.05, 3),
            (12.483, 0.05, 3),
            (3.0, 0.1 * 0.05, 3),
        ];
        let nyq = 0.5 * sr;
        let mut noise = cx.rng(self.seed);
        let mut age = [f32::INFINITY; 5];
        let mut next = 0usize;
        let mut ph = [0.0f64; 11];
        let (mut p16, mut p24) = (0.0f64, 0.0f64);
        let d = (0.059 * sr) as usize;
        let mut line = vec![0.0f32; d.max(1)];
        let mut body = [
            PdBp::new(1243.0, 12.0, sr),
            PdBp::new(287.0, 12.0, sr),
            PdBp::new(431.0, 12.0, sr),
        ];
        let mut hip = PdHip::new(10.0, sr);
        Ok(one(mono(cx.n, |i| {
            while next < hits.len() && hits[next] <= i {
                age = [0.0; 5];
                next += 1;
            }
            // line~ 1 -> 0 over each length; groups 2 and 4 are squared
            // twice (x^4), group 3 once (x^2).
            let mut env = [0.0f32; 5];
            for g in 0..5 {
                let x = (1.0 - age[g] / lens[g]).max(0.0);
                env[g] = match g {
                    2 | 4 => x * x * x * x,
                    3 => x * x,
                    _ => x,
                };
                age[g] += 1.0;
            }
            let mut parts = 0.0f32;
            for (k, (r, a, g)) in partials.iter().enumerate() {
                let hz = r * f;
                ph[k] = (ph[k] + f64::from(hz / sr)).fract();
                if hz < nyq {
                    parts += a * env[*g] * (TAU * ph[k] as f32).cos();
                }
            }
            // pd strike: (osc 16 f + osc 24 f + noise) x 0.03 x env^4.
            p16 = (p16 + f64::from(16.0 * f / sr)).fract();
            p24 = (p24 + f64::from(24.0 * f / sr)).fract();
            let tone = |ph: f64, hz: f32| {
                if hz < nyq {
                    (TAU * ph as f32).cos()
                } else {
                    0.0
                }
            };
            let strike =
                0.03 * (tone(p16, 16.0 * f) + tone(p24, 24.0 * f) + noise.signed()) * env[4];
            let parts = parts + strike;
            // bodyresonance: delwrite(strike + 0.4 bp(clip(delread 59)))
            let j = i % line.len();
            let delayed = line[j].clamp(-0.2, 0.5);
            let bsum: f32 = body.iter_mut().map(|b| b.tick(delayed)).sum();
            line[j] = strike + 0.4 * bsum;
            let y = (0.9 * parts + 1.6 * bsum).clamp(-0.9, 0.9);
            hip.tick(y)
        })))
    }
}

// ------------------------------------------------------------- ball_bounce

/// A bouncing ball (BOUNCINGBALL/bb1.pd): a line L falls 1 -> 0 over
/// `length`; a bounce every 300 L ms, each a sine thump of `pitch` + 130 L
/// v^e Hz (v its own line 1 -> 0 over 200 L ms) wobbled by 70 L Hz at 120
/// Hz, at level L v^2. The bounces speed up, shorten and fade together.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BallBounce {
    /// Seconds from the drop to rest, 0.3 to 20 (default 3).
    #[serde(default)]
    pub length: Option<f32>,
    /// The first gap between bounces, seconds, 0.05 to 2 (default 0.3).
    #[serde(default)]
    pub gap: Option<f32>,
    /// Base pitch of a bounce, Hz, 20 to 1000 (default 80): lower is a
    /// bigger, softer ball.
    #[serde(default)]
    pub pitch: Option<f32>,
    /// Seconds before the first bounce (default 0.01).
    #[serde(default)]
    pub at: Option<f32>,
}

impl Describe for BallBounce {
    const BLURB: &'static str = "a bouncing ball (Farnell): sine thumps that come faster, shorter and quieter as a line falls over length; gap = first interval, pitch = base Hz";
    fn example() -> serde_json::Value {
        serde_json::json!({"length": 3, "gap": 0.3, "pitch": 80})
    }
}

impl Eval for BallBounce {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let len = self.length.unwrap_or(3.0);
        if !(len.is_finite() && (0.3..=20.0).contains(&len)) {
            return Err(format!("length: {len}: give seconds from 0.3 to 20"));
        }
        let gap = self.gap.unwrap_or(0.3);
        if !(gap.is_finite() && (0.05..=2.0).contains(&gap)) {
            return Err(format!("gap: {gap}: give seconds from 0.05 to 2"));
        }
        let pitch = self.pitch.unwrap_or(80.0);
        if !(pitch.is_finite() && (20.0..=1000.0).contains(&pitch)) {
            return Err(format!("pitch: {pitch}: give Hz from 20 to 1000"));
        }
        let at = non_negative("at", self.at.unwrap_or(0.01))?;
        let sr = cx.sr;
        let l_at = |t: f32| (1.0 - (t - at) / len).clamp(0.0, 1.0);
        // metro: a bang now, the next 300 L ms later (L when it fires).
        let mut bounces: Vec<(f32, f32)> = Vec::new(); // (time, fall seconds)
        let mut t = at;
        while t < at + len && t * sr < cx.n as f32 {
            let l = l_at(t);
            bounces.push((t, 0.2 / 0.3 * gap * l));
            t += (gap * l).max(0.001);
        }
        let (mut ph5, mut ph120) = (0.0f64, 0.0f64);
        let mut cur: Option<(f32, f32)> = None;
        let mut k = 0usize;
        Ok(one(mono(cx.n, |i| {
            let ts = i as f32 / sr;
            while k < bounces.len() && bounces[k].0 <= ts {
                cur = Some(bounces[k]);
                k += 1;
            }
            let l = if ts >= at { l_at(ts) } else { 0.0 };
            // vline~: 1 in 1 ms, then 0 over the fall.
            let v = match cur {
                Some((t0, fall)) => {
                    let u = ts - t0;
                    if u < 0.001 {
                        u / 0.001
                    } else if fall > 0.0 {
                        (1.0 - (u - 0.001) / fall).max(0.0)
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            };
            ph120 = (ph120 + 120.0 / f64::from(sr)).fract();
            let wob = 70.0 * l * (TAU * ph120 as f32).cos();
            // Pd-extended pow~: right ^ left, here v ^ e.
            let hz = pitch + 130.0 * l * v.powf(std::f32::consts::E) + wob;
            ph5 = (ph5 + f64::from(hz / sr)).rem_euclid(1.0);
            0.2 * l * v * v * (TAU * ph5 as f32).cos()
        })))
    }
}

// ----------------------------------------------------------------- cricket

/// A cricket (INSECTS/cricket2.pd): a counter ticks every `tick` s; on the
/// first `pulses` of every `period` ticks it fires a 0.1 ms pulse of rising
/// height ((k + 2) / 9), low-passed at 1 kHz, into very narrow band-passes
/// at 4500 / 4550 / 4600 Hz (Q 1200, x 100) and 9000 Hz (Q 500, x 42),
/// high-passed at 4 kHz.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cricket {
    /// Seconds between ticks, 0.005 to 0.1 (default 0.017).
    #[serde(default)]
    pub tick: Option<f32>,
    /// Pulses per chirp, 1 to 30 (default 7).
    #[serde(default)]
    pub pulses: Option<u32>,
    /// Ticks per chirp cycle, 2 to 400 (default 41: a chirp every 0.7 s).
    #[serde(default)]
    pub period: Option<u32>,
    /// The song's pitch, Hz, 1000 to 10000 (default 4500); the upper band
    /// sits at twice it.
    #[serde(default)]
    pub pitch: Option<f32>,
    /// How long each pulse rings, 0.05 to 1 times the book's (default 0.3,
    /// the listener's pick). At 1 (Q 1200 / 500, about 85 ms, longer than
    /// the gap between pulses) the three close bands blur into one tone
    /// beating at 50 Hz, heard as metallic; 0.12 was heard as more like a
    /// cicada. The band levels are kept.
    #[serde(default)]
    pub ring: Option<f32>,
}

impl Describe for Cricket {
    const BLURB: &'static str = "a cricket (Farnell): chirps of rising pulses ringing very narrow bands at pitch and twice it; tick, pulses, period set the rhythm";
    fn example() -> serde_json::Value {
        serde_json::json!({"tick": 0.017, "pulses": 7, "period": 41, "pitch": 4500, "ring": 0.3})
    }
}

impl Eval for Cricket {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let tick = self.tick.unwrap_or(0.017);
        if !(tick.is_finite() && (0.005..=0.1).contains(&tick)) {
            return Err(format!("tick: {tick}: give seconds from 0.005 to 0.1"));
        }
        let pulses = self.pulses.unwrap_or(7);
        if !(1..=30).contains(&pulses) {
            return Err(format!("pulses: {pulses}: give 1 to 30"));
        }
        let period = self.period.unwrap_or(41);
        if !(2..=400).contains(&period) || period <= pulses {
            return Err(format!("period: {period}: give 2 to 400, more than pulses"));
        }
        let pitch = self.pitch.unwrap_or(4500.0);
        if !(pitch.is_finite() && (1000.0..=10000.0).contains(&pitch)) {
            return Err(format!("pitch: {pitch}: give Hz from 1000 to 10000"));
        }
        let ring = self.ring.unwrap_or(0.3);
        if !(ring.is_finite() && (0.05..=1.0).contains(&ring)) {
            return Err(format!("ring: {ring}: give 0.05 to 1"));
        }
        let sr = cx.sr;
        let s = pitch / 4500.0;
        let mut lop = PdLop::new(1000.0, sr);
        let (ql, qh) = (1200.0 * ring, 500.0 * ring);
        let mut low = [
            PdBp::new(4500.0 * s, ql, sr),
            PdBp::new(4550.0 * s, ql, sr),
            PdBp::new(4600.0 * s, ql, sr),
        ];
        let mut high = PdBp::new(9000.0 * s, qh, sr);
        // Pd's bp~ gain falls with Q; keep the book's band levels.
        let comp = 1.0 / ring;
        let mut hip = PdHip::new(4000.0 * s, sr);
        let tick_n = tick * sr;
        let pulse_n = (0.0001 * sr).max(1.0);
        Ok(one(mono(cx.n, |i| {
            let x = i as f32;
            let k = (x / tick_n) as u32;
            let into = x - k as f32 * tick_n;
            let step = k % period;
            let v = if step < pulses && into < pulse_n {
                (step as f32 + 2.0) / 9.0 * (into + 1.0) / pulse_n
            } else {
                0.0
            };
            let e = lop.tick(v);
            let lo: f32 = low.iter_mut().map(|b| b.tick(e)).sum();
            let y = comp * (100.0 * lo + 42.0 * high.tick(e));
            0.5 * hip.tick(y)
        })))
    }
}

// ------------------------------------------------------------------- twang

/// A ruler twanged on a desk (MRBOINGY/twang.pd): a falling phasor at
/// `freq` / 4 (plus a quickly dying push) drives "clamped" cosine modes at
/// 1, 6.267, 17.55, 34.39 x its phase; that, times noise, excites six
/// "free" modes (Pd vcf~, Q 80) at 1, 2.7565, 5.404, 8.933, 13.34, 18.64 x
/// `freq`, with a 5 Hz vibrato of `vibrato` Hz.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Twang {
    /// Free-mode frequency, Hz, 20 to 800 (default 426): the length of the
    /// overhang.
    #[serde(default)]
    pub freq: Option<f32>,
    /// Vibrato depth, Hz, 0 to 20 (default 6, at 5 Hz).
    #[serde(default)]
    pub vibrato: Option<f32>,
    /// Pluck time, seconds (default 0.01).
    #[serde(default)]
    pub at: Option<f32>,
    /// Ring length, 0.1 to 5 times the book's (default 1: the clamped ring
    /// fades over 1.5 s, the push over 7 s).
    #[serde(default)]
    pub ring: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Twang {
    const BLURB: &'static str = "a ruler twanged on a desk (Farnell): a falling phasor drives clamped cosine modes that excite six free vcf~ modes; freq = overhang pitch";
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 426, "vibrato": 6, "at": 0.01})
    }
}

impl Eval for Twang {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let freq = self.freq.unwrap_or(426.0);
        if !(freq.is_finite() && (20.0..=800.0).contains(&freq)) {
            return Err(format!("freq: {freq}: give Hz from 20 to 800"));
        }
        let vib = self.vibrato.unwrap_or(6.0);
        if !(vib.is_finite() && (0.0..=20.0).contains(&vib)) {
            return Err(format!("vibrato: {vib}: give 0 to 20 Hz"));
        }
        let ring = self.ring.unwrap_or(1.0);
        if !(ring.is_finite() && (0.1..=5.0).contains(&ring)) {
            return Err(format!("ring: {ring}: give 0.1 to 5"));
        }
        let at = non_negative("at", self.at.unwrap_or(0.01))?;
        let sr = cx.sr;
        let start = (at * sr) as usize;
        let (l1, l2) = (7.0 * ring * sr, 1.5 * ring * sr);
        let clamped = [
            (1.0f32, 0.5f32),
            (6.267, 0.25),
            (17.55, 0.125),
            (34.39, 0.0625),
        ];
        let free = [1.0f32, 2.7565, 5.40392, 8.93295, 13.3443, 18.6379];
        let mut vcf = vec![PdVcf::default(); free.len()];
        let mut lop = PdLop::new(4000.0, sr);
        let mut hip = PdHip::new(90.0, sr);
        let mut noise = cx.rng(self.seed);
        let (mut p, mut ph5) = (0.0f64, 0.0f64);
        Ok(one(mono(cx.n, |i| {
            ph5 = (ph5 + 5.0 / f64::from(sr)).fract();
            let fr = freq + vib * (TAU * ph5 as f32).cos();
            let w = noise.signed();
            if i < start {
                return 0.0;
            }
            if i == start {
                p = 0.0;
            }
            let u = (i - start) as f32;
            let q1 = (1.0 - u / l1).max(0.0).powi(4);
            let q2 = (1.0 - u / l2).max(0.0).powi(4);
            // Pd-extended pow~: right ^ left, the envelope ^ 15.
            let pw = q1.powi(15);
            p = (p - f64::from((6.0 * pw + fr / 4.0) / sr)).rem_euclid(1.0);
            let pf = p as f32;
            let cl: f32 = clamped
                .iter()
                .map(|(m, a)| a * (TAU * (pf * m).fract()).cos())
                .sum::<f32>()
                * q2;
            let drive = pf * pf * cl;
            let exc = lop.tick(w * drive);
            let base = fr * (1.0 + 0.1 * pw);
            let nyq = 0.45 * sr;
            let mut fm = 0.0;
            for (k, r) in free.iter().enumerate() {
                let hz = base * r;
                if hz < nyq {
                    fm += vcf[k].tick(exc, hz, 80.0, sr);
                }
            }
            4.0 * hip.tick(fm + 0.2 * cl)
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pd_bp_matches_pd_coefficients() {
        // bp~ 1000 10 at 44.1 kHz: omega 0.14248, 1 - r 0.014248.
        let b = PdBp::new(1000.0, 10.0, 44100.0);
        assert!((b.c2 + (1.0f32 - 0.014248).powi(2)).abs() < 1e-5);
    }
}

/// A Pd feedback delay built from `delread~` / `s~` / `r~` / `delwrite~`
/// (dfbe.pd, dfbef.pd): the output is the delayed signal; the feedback goes
/// round through `s~` / `r~`, which adds one 64-sample block, so the loop is
/// `d` + 64 samples long.
struct PdFbDelay {
    buf: Vec<f32>,
    pos: usize,
    d: usize,
    fb: f32,
}

impl PdFbDelay {
    fn new(ms: f32, fb: f32, sr: f32) -> PdFbDelay {
        let d = ((ms * 0.001 * sr) as usize).max(1);
        PdFbDelay {
            buf: vec![0.0; d + 66],
            pos: 0,
            d,
            fb,
        }
    }
    fn read(&self, back: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.pos + n - back % n) % n]
    }
    fn tick(&mut self, x: f32) -> f32 {
        let out = self.read(self.d);
        let fbv = self.read(self.d + 64);
        self.buf[self.pos] = x + self.fb * fbv;
        self.pos = (self.pos + 1) % self.buf.len();
        out
    }
}

// ------------------------------------------------------------------- siren

/// A police siren (POLICE/police3.pd): `logosc~`, a phasor bent into a rise
/// `1 - (1 - 2p)^e` and a fall `(2 - 2p)^(e / 2)` (Pd-extended `pow~`), at
/// `rate` sweeps a second copy between `low` and `high` Hz; that waveform
/// goes through the "plastic horn" (clip +-0.2, `bp~` 1500 Hz Q 4) and,
/// with `echo`, the patch's street (three echoes, 33 / 121 / 165 ms,
/// cross-fed at 0.1).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Siren {
    /// Sweeps a second, 0.05 to 20 (default 3: the book's "yelp"; 0.1 is a
    /// slow wail).
    #[serde(default)]
    pub rate: Option<f32>,
    /// Bottom of the sweep, Hz, 50 to 4000 (default 300).
    #[serde(default)]
    pub low: Option<f32>,
    /// Top of the sweep, Hz, 50 to 8000 (default 1100).
    #[serde(default)]
    pub high: Option<f32>,
    /// The patch's street echoes, 0 to 1 (default 0: dry, the game adds
    /// the space; 1 is the book's environment.pd).
    #[serde(default)]
    pub echo: Option<f32>,
}

impl Describe for Siren {
    const BLURB: &'static str = "a police siren (Farnell): a log-shaped oscillator swept between low and high Hz rate times a second, through a clipped plastic horn; echo adds the patch's street";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 3, "low": 300, "high": 1100, "echo": 0})
    }
}

/// `logosc~`'s shape of a phase p in 0..1 (with Pd-extended's pow~).
fn logosc(p: f32) -> f32 {
    if p < 0.5 {
        1.0 - (1.0 - 2.0 * p).max(0.0).powf(std::f32::consts::E)
    } else {
        (2.0 - 2.0 * p).max(0.0).powf(std::f32::consts::E * 0.5)
    }
}

impl Eval for Siren {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = self.rate.unwrap_or(3.0);
        if !(rate.is_finite() && (0.05..=20.0).contains(&rate)) {
            return Err(format!(
                "rate: {rate}: give sweeps a second from 0.05 to 20"
            ));
        }
        let low = self.low.unwrap_or(300.0);
        let high = self.high.unwrap_or(1100.0);
        if !(low.is_finite() && (50.0..=4000.0).contains(&low)) {
            return Err(format!("low: {low}: give Hz from 50 to 4000"));
        }
        if !(high.is_finite() && (50.0..=8000.0).contains(&high) && high > low) {
            return Err(format!("high: {high}: give Hz from 50 to 8000, above low"));
        }
        let echo = self.echo.unwrap_or(0.0);
        if !(echo.is_finite() && (0.0..=1.0).contains(&echo)) {
            return Err(format!("echo: {echo}: give 0 to 1"));
        }
        let sr = cx.sr;
        let (mut ps, mut pf) = (0.0f64, 0.0f64);
        let mut horn = PdBp::new(1500.0, 4.0, sr);
        let taps = [165.0f32, 121.0, 33.0].map(|ms| ((ms * 0.001 * sr) as usize).max(1));
        let len = taps[0] + 70;
        let mut bufs = vec![vec![0.0f32; len]; 3];
        let mut pos = 0usize;
        Ok(one(mono(cx.n, |_| {
            ps = (ps + f64::from(rate / sr)).fract();
            let f = low + (high - low) * logosc(ps as f32);
            pf = (pf + f64::from(f / sr)).fract();
            let x = horn.tick(logosc(pf as f32).clamp(-0.2, 0.2));
            if echo <= 0.0 {
                return x;
            }
            // environment.pd: each echo line is fed the direct sound plus
            // 0.1 x the sum of the three echoes (through s~ / r~, one block
            // later); out = 0.7 direct + 0.5 x that echo sum.
            let rd = |b: &Vec<f32>, back: usize| b[(pos + len - back % len) % len];
            let e: f32 = (0..3).map(|k| rd(&bufs[k], taps[k])).sum::<f32>() * 0.1;
            let el: f32 = (0..3).map(|k| rd(&bufs[k], taps[k] + 64)).sum::<f32>() * 0.1;
            for b in bufs.iter_mut() {
                b[pos] = x + el;
            }
            pos = (pos + 1) % len;
            (1.0 - 0.3 * echo) * x + 0.5 * echo * e
        })))
    }
}

// -------------------------------------------------------------- door_creak

/// A creaking door (CREAKING/doorcreaker.pd): while `force` is over 0.3
/// the hinge sticks and slips: a tick every `3 + 60 (1 - f)` ms plus up to
/// `6 (1 - f)` ms of jitter, each a sharp pulse whose height and length grow
/// with the time since the last; through the wood (`bp~` 62.5 / 125 / 250 /
/// 395 / 560 / 790 Hz) and a square panel (eight feedback delays of 4.5 to
/// 16 ms), high-passed at 125 Hz.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DoorCreak {
    /// How hard the door is pushed, 0 to 1 (a node to swing it). Over 0.3
    /// it creaks; higher is a faster, denser creak.
    pub force: In,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for DoorCreak {
    const BLURB: &'static str = "a creaking door hinge (Farnell): stick-slip ticks whose rate follows force (over 0.3), through wood resonances and a square panel; force is a 0..1 signal";
    fn example() -> serde_json::Value {
        serde_json::json!({"force": 0.6, "seed": 1})
    }
}

impl Eval for DoorCreak {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let force = cx.mono("force", &self.force)?;
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);
        let mut wood = [
            PdBp::new(250.0, 2.0, sr),
            PdBp::new(395.0, 2.0, sr),
            PdBp::new(125.0, 1.0, sr),
            PdBp::new(62.5, 1.0, sr),
            PdBp::new(560.0, 3.0, sr),
            PdBp::new(790.0, 3.0, sr),
        ];
        let mut panel: Vec<PdFbDelay> = [16.0f32, 10.12, 7.14, 5.48, 8.0, 6.27, 5.06, 4.52]
            .iter()
            .map(|ms| PdFbDelay::new(*ms, 0.05, sr))
            .collect();
        let mut hip = PdHip::new(125.0, sr);
        // line $1 100: the force glides to each new value over 100 ms.
        let mut f = 0.0f32;
        let (mut next, mut last) = (0.0f32, f32::NEG_INFINITY);
        let (mut a, mut fall, mut age) = (0.0f32, 1.0f32, f32::INFINITY);
        Ok(one(mono(cx.n, |i| {
            let target = force.at(i).clamp(0.0, 1.0);
            f += (target - f).clamp(-1.0 / (0.1 * sr), 1.0 / (0.1 * sr));
            let t = i as f32 / sr * 1000.0;
            if f > 0.3 && t >= next {
                // timer: ms since the last tick, capped at 100.
                let e = ((t - last).min(100.0) / 100.0).max(0.0);
                last = t;
                a = (e.sqrt() + 0.1).sqrt();
                fall = e.sqrt() * 0.001 * sr;
                age = 0.0;
                next = t + 3.0 + 60.0 * (1.0 - f) + rng.unit() * 6.0 * (1.0 - f);
            } else if f <= 0.3 {
                next = t;
            }
            // vline~ $1, 0 $2 then squared.
            let v = if fall > 0.0 && age < fall {
                a * (1.0 - age / fall)
            } else {
                0.0
            };
            age += 1.0;
            let p = v * v;
            let w = 0.2 * p + wood.iter_mut().map(|b| b.tick(p)).sum::<f32>();
            let s: f32 = panel.iter_mut().map(|d| d.tick(w)).sum();
            0.2 * hip.tick(s)
        })))
    }
}

// --------------------------------------------------------------------- jet

/// A jet engine (JETENGINE/jetengine1.pd): `speed` through a 0.2 Hz
/// `lop~` (spool-up), five turbine tones at 3097 / 4495 / 5588 / 7471 /
/// 11000 Hz x speed (clipped at 0.9), and the burner: noise band-passed at 8
/// kHz into a `vcf~` at 150 speed^2 Hz, x 120, clipped, into a `vcf~` at
/// 12000 speed Hz; burner + 0.5 turbine, `lop~` 11 kHz, x 0.1.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Jet {
    /// Throttle, 0.1 to 1 (a node for a spool-up / down; the engine follows
    /// it through a 0.2 Hz low-pass, as the book's does).
    pub speed: In,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Jet {
    const BLURB: &'static str = "a jet engine (Farnell): turbine tones and a clipped, swept burner roar following speed (0.1..1) through a slow spool-up";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 0.8, "seed": 1})
    }
}

impl Eval for Jet {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono("speed", &self.speed)?;
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);
        let mut spool = PdLop::new(0.2, sr);
        let mut bp = PdBp::new(8000.0, 0.5, sr);
        let (mut v1, mut v2) = (PdVcf::default(), PdVcf::default());
        let mut hip = PdHip::new(120.0, sr);
        let mut out_lp = PdLop::new(11000.0, sr);
        let tones = [3097.0f32, 4495.0, 5588.0, 7471.0, 11000.0];
        let gains = [0.25f32, 0.25, 1.0, 0.4, 0.4];
        let mut ph = [0.0f64; 5];
        let nyq = 0.5 * sr;
        Ok(one(mono(cx.n, |i| {
            let s = spool.tick(speed.at(i).clamp(0.1, 1.0));
            let mut t = 0.0f32;
            for k in 0..5 {
                let hz = tones[k] * s;
                ph[k] = (ph[k] + f64::from(hz / sr)).fract();
                if hz < nyq {
                    t += gains[k] * (TAU * ph[k] as f32).cos();
                }
            }
            let turbine = t.clamp(-0.9, 0.9);
            let n = bp.tick(rng.signed());
            let a = hip.tick(v1.tick(n, 150.0 * s * s, 1.0, sr));
            let burn = v2.tick((120.0 * a).clamp(-1.0, 1.0), 12000.0 * s, 0.6, sr);
            0.1 * out_lp.tick(burn + 0.5 * turbine)
        })))
    }
}

// --------------------------------------------------------------- red_alert

/// A starship red-alert klaxon (REDALERT/TOS_redalert.pd): over 900 ms a
/// phasor rises from 360 to 847 Hz (as the square root of a line), shaped
/// `p - 0.5 + 0.3 cos(4 pi p)`, gated for 890 ms; through five feedback
/// delays (11 to 35 ms, x 0.6) summed and fed back through a 61 ms one,
/// clipped at x 4, then formants at 740 / 1400 / 1500 / 1600 Hz (`bp~` Q
/// 12). One whoop per time in `at`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedAlert {
    /// Whoop time(s), seconds: a number or a list (default 0.01).
    #[serde(default)]
    pub at: Option<Times>,
    /// Start and end pitch scale, 0.25 to 4 (default 1: 360 -> 847 Hz).
    #[serde(default)]
    pub pitch: Option<f32>,
}

impl Describe for RedAlert {
    const BLURB: &'static str = "a sci-fi red-alert klaxon (Farnell): a rising buzzy whoop through feedback delays and a clipped formant horn; at = whoop time(s)";
    fn example() -> serde_json::Value {
        serde_json::json!({"at": [0.01, 1.3], "pitch": 1})
    }
}

impl Eval for RedAlert {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let pitch = self.pitch.unwrap_or(1.0);
        if !(pitch.is_finite() && (0.25..=4.0).contains(&pitch)) {
            return Err(format!("pitch: {pitch}: give 0.25 to 4"));
        }
        let mut hits: Vec<usize> = times(&self.at, 0.01, "at")?
            .iter()
            .map(|t| (t * cx.sr) as usize)
            .collect();
        hits.sort_unstable();
        let sr = cx.sr;
        let mut taps: Vec<PdFbDelay> = [15.0f32, 35.0, 11.0, 22.0, 24.0]
            .iter()
            .map(|ms| PdFbDelay::new(*ms, 0.6, sr))
            .collect();
        let mut loop61 = PdFbDelay::new(61.0, 0.6, sr);
        // s~ fbsd / r~ fbsd: the 61 ms output comes back a block later.
        let mut fb_line = vec![0.0f32; 64];
        let mut hip1 = PdHip::new(1.0, sr);
        let mut hip12 = PdHip::new(12.0, sr);
        let mut forms = [
            PdBp::new(1400.0, 12.0, sr),
            PdBp::new(1500.0, 12.0, sr),
            PdBp::new(1600.0, 12.0, sr),
            PdBp::new(740.0, 12.0, sr),
        ];
        let (mut age, mut next) = (f32::INFINITY, 0usize);
        let mut ph = 0.0f64;
        Ok(one(mono(cx.n, |i| {
            while next < hits.len() && hits[next] <= i {
                age = 0.0;
                next += 1;
            }
            let ms = age / sr * 1000.0;
            age += 1.0;
            // vline~ 0, 1 900 0, 0 0 1200: rise over 900 ms, cut at 1200.
            let rise = if ms < 900.0 {
                ms / 900.0
            } else if ms < 1200.0 {
                1.0
            } else {
                0.0
            };
            // vline~ 0, 1 10 0, 0 10 890: on in 10 ms, off over 10 ms at 890.
            let gate = if ms < 10.0 {
                ms / 10.0
            } else if ms < 890.0 {
                1.0
            } else if ms < 900.0 {
                1.0 - (ms - 890.0) / 10.0
            } else {
                0.0
            };
            let hz = (360.0 + 487.0 * rise.sqrt()) * pitch;
            ph = (ph + f64::from(hz / sr)).fract();
            let p = ph as f32;
            let wave = hip1.tick(p - 0.5 + 0.3 * (TAU * (2.0 * p).fract()).cos());
            let fbv = fb_line[i % 64];
            let x = 0.2 * (wave * gate + 0.006 * fbv);
            let y = hip12.tick(taps.iter_mut().map(|d| d.tick(x)).sum());
            fb_line[i % 64] = loop61.tick(y);
            let c = (4.0 * y).clamp(-1.0, 1.0);
            let f: f32 = forms.iter_mut().map(|b| b.tick(c)).sum();
            0.2 * (f + 0.5 * c)
        })))
    }
}
