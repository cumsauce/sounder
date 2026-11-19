//! Vehicle nodes ported from Farnell's *Designing Sound* Pd patches:
//! `helicopter` (HELICOPTER/heli1.pd with mainrotor.pd, mdel.pd, vrdel.pd)
//! and `rocket` (ROCKETLAUNCHER/rocket.pd, the `rocket` sub-patch: the motor
//! only, not the launcher's chamber, reload or handling sounds).
//!
//! The Pd filters are copied from `farnell.rs` (they must keep Pd's gains)
//! with a `tune` that moves a filter's coefficients without clearing its
//! state, as Pd does when a control message changes a `bp~`, `hip~` or
//! `lop~`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{mono, non_negative, one};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Pd's `bp~` (centre Hz, Q).
#[derive(Default, Clone)]
struct Bp {
    c1: f32,
    c2: f32,
    gain: f32,
    x1: f32,
    x2: f32,
}

impl Bp {
    fn new(f: f32, q: f32, sr: f32) -> Bp {
        let mut b = Bp::default();
        b.tune(f, q, sr);
        b
    }
    // Pd's d_filter.c writes pi as 3.14159; keep it so the coefficients
    // match Pd's.
    #[allow(clippy::approx_constant)]
    fn tune(&mut self, f: f32, q: f32, sr: f32) {
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

/// Pd's `hip~` before 0.44 (no gain normalisation).
#[derive(Default, Clone)]
struct Hip {
    coef: f32,
    last: f32,
}

impl Hip {
    fn new(f: f32, sr: f32) -> Hip {
        let mut h = Hip::default();
        h.tune(f, sr);
        h
    }
    fn tune(&mut self, f: f32, sr: f32) {
        self.coef = (1.0 - f * TAU / sr).clamp(0.0, 1.0);
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
struct Lop {
    k: f32,
    y: f32,
}

impl Lop {
    fn new(f: f32, sr: f32) -> Lop {
        let mut l = Lop::default();
        l.tune(f, sr);
        l
    }
    fn tune(&mut self, f: f32, sr: f32) {
        self.k = (f * TAU / sr).clamp(0.0, 1.0);
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// Pd's `vcf~` (the real output), centre given per sample.
#[derive(Default, Clone)]
struct Vcf {
    re: f32,
    im: f32,
}

impl Vcf {
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

/// `delwrite~` / `vd~`: a ring buffer read at a whole or fractional delay.
struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    fn new(max_samples: usize) -> Line {
        Line {
            buf: vec![0.0; max_samples.max(4) + 2],
            pos: 0,
        }
    }
    /// The value pushed `k` samples ago, counting the next push as 0 (so
    /// `back(1)` is the last one pushed).
    fn back(&self, k: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.pos + n - k % n) % n]
    }
    fn push(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % self.buf.len();
    }
    /// After a push: the signal `d` samples ago (0 = the sample just
    /// pushed), linearly interpolated.
    fn tap(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.buf.len() - 3) as f32);
        let k = d as usize;
        let fr = d - k as f32;
        self.back(k + 1) * (1.0 - fr) + self.back(k + 2) * fr
    }
}

/// `mdel` (mdel.pd): the input plus `gain` times a copy that is low-passed
/// at `lp` Hz and delayed by 25 ms plus a signal (ms).
struct Mdel {
    lp: Lop,
    line: Line,
    gain: f32,
    sr: f32,
}

impl Mdel {
    fn new(lp: f32, gain: f32, sr: f32) -> Mdel {
        Mdel {
            lp: Lop::new(lp, sr),
            line: Line::new((0.3 * sr) as usize),
            gain,
            sr,
        }
    }
    fn tick(&mut self, x: f32, delay_ms: f32) -> f32 {
        let l = self.lp.tick(x);
        self.line.push(l);
        let d = (25.0 + delay_ms) * 0.001 * self.sr;
        x + self.gain * self.line.tap(d)
    }
}

/// The ratio of the book's engine, tail rotor and mix sliders as saved in
/// heli1.pd (a `hsl` of width 64 runs 0..6300): engine-mix 1800, gear-mix
/// 1000, tail-mix 2800, rotor-mix 6300, gear-ratio 6300, tail-speed 3700.
const ENGINE_MIX: f32 = 1800.0 / 6300.0;
const GEAR_MIX: f32 = 0.1 * 1000.0 / 6300.0;
const GEAR_RATIO: f32 = 14.0;
const TAIL_RATIO: f32 = 2.0 * 3700.0 / 6300.0;
/// Pd's block, which a `vd~` read in a loop cannot undercut.
const BLOCK: usize = 64;

// -------------------------------------------------------------- helicopter

/// A helicopter (HELICOPTER/heli1.pd): the engine (a pulse from a phasor,
/// `1 / (1 + (98 p^2 (1 - p))^2)`, jittered by clipped noise, through a
/// four-delay feedback network with a `sin` waveshaper and a 100 Hz `bp~`,
/// high-passed twice at 200 Hz), three gear-whine sines, the tail rotor (a
/// `cos` of the tail speed squared into an amplitude `1 / (1 + 2.25 cos^2)`
/// and a `vcf~` swept 100 +- 500 Hz over noise), and the main rotor
/// (mainrotor.pd: per blade a skewed `sin` pulse that sweeps a `vcf~` over
/// noise through three echoing `mdel`s 25 ms and more long). The sum goes
/// through `vrdel` (3 kHz low-pass, a delay of 25 ms + 0.2 x the blade pulse,
/// so every blade pass bends the pitch) and the book's `distance`: three
/// one-poles at 6000 (1 - d) + 50 Hz and a ground echo of 2 + 3 d ms.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Helicopter {
    /// Main-rotor blade passes a second, 2 to 40 (default 14.2: the book's
    /// 70 ms period). A signal: a node ramps it for a spool-up.
    #[serde(default)]
    pub rotor_rate: Option<In>,
    /// Engine speed, 0 to 1 (default 0.7: the book's 64 Hz). The engine's
    /// pulse runs at 1 + 90 x speed Hz (through a 0.6 Hz low-pass, so a
    /// change glides); the gear whine and the tail rotor follow it. A signal.
    #[serde(default)]
    pub engine: Option<In>,
    /// Tail rotor level, 0 to 1 (default 0.44: the book's slider; 0 is off).
    #[serde(default)]
    pub tail: Option<f32>,
    /// How far away, 0 to 1 (default 0): the whole sound is low-passed at
    /// 6000 (1 - d) + 50 Hz and gets a ground echo. A signal: ramp it up for
    /// a helicopter flying off.
    #[serde(default)]
    pub distance: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Helicopter {
    const BLURB: &'static str = "a helicopter (Farnell): engine pulse network, gear whine, tail rotor and a main rotor whose blade pulses sweep noise through echoes; rotor_rate (Hz, signal), engine (0..1 signal), tail, distance (0..1 signal); set a duration";
    fn example() -> serde_json::Value {
        serde_json::json!({"rotor_rate": 14.2, "engine": 0.7, "tail": 0.44, "distance": 0, "seed": 1})
    }
}

impl Eval for Helicopter {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = cx.mono_or("rotor_rate", &self.rotor_rate, 14.2)?;
        let engine = cx.mono_or("engine", &self.engine, 0.7)?;
        let dist = cx.mono_or("distance", &self.distance, 0.0)?;
        let tail_level = self.tail.unwrap_or(2800.0 / 6300.0);
        if !(tail_level.is_finite() && (0.0..=1.0).contains(&tail_level)) {
            return Err(format!("tail: {tail_level}: give 0 to 1"));
        }
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);

        // Engine.
        let mut speed_lp = Lop::new(0.6, sr);
        let mut ph_e = 0.0f32;
        let mut n_bp = Bp::new(500.0, 5.0, sr);
        let mut hp30 = Hip::new(30.0, sr);
        let mut e1a = Line::new(BLOCK + 2);
        let mut e1b = Line::new(BLOCK + 2);
        let mut e2a = Line::new(BLOCK + 2);
        let mut e2b = Line::new(BLOCK + 2);
        let mut bp100 = Bp::new(100.0, 2.0, sr);
        let mut lp200 = Lop::new(200.0, sr);
        let mut hp12 = Hip::new(12.0, sr);
        let mut out_hp = [Hip::new(200.0, sr), Hip::new(200.0, sr)];
        // Gears.
        let mut ph_g = [0.0f32; 3];
        // Tail rotor.
        let mut ph_t = 0.0f32;
        let mut tail_vcf = Vcf::default();
        // Main rotor.
        let skew = 0.6 * 2500.0 / 6300.0 + 0.2;
        let k_norm = 3.0 + 27.0 * (skew - 0.5) * (skew - 0.5);
        let q_rot: f32 = 1200.0 / 6300.0 * 30.0;
        let amp_q = 0.1 + q_rot.sqrt() * 0.6;
        let shift = 1000.0 / 6300.0 * 300.0;
        let (basef, devf) = (100.0 / 6300.0 * 400.0, 3100.0 / 6300.0 * 600.0);
        let mut rot_vcf = Vcf::default();
        let mut ph_r = 1.0f32;
        let (mut t_pulse, mut dur) = (0.0f32, 0.02f32);
        let mut m34 = Mdel::new(400.0, 0.7, sr);
        let mut m30 = Mdel::new(400.0, 0.7, sr);
        let mut m35 = Mdel::new(400.0, 0.5, sr);
        // vrdel and the distance patch.
        let mut v_lp = Lop::new(3000.0, sr);
        let mut v_line = Line::new((0.1 * sr) as usize);
        let mut d_lp = [Lop::default(), Lop::default(), Lop::default()];
        let mut d_line = Line::new((0.02 * sr) as usize);
        let mut dc = Hip::new(20.0, sr);
        Ok(one(mono(cx.n, |i| {
            // ---- engine
            let f_e = speed_lp.tick(1.0 + 90.0 * engine.at(i).clamp(0.0, 1.0));
            ph_e = (ph_e + f_e / sr).fract();
            let q = 98.0 * ph_e * ph_e * (1.0 - ph_e);
            let pulse = 1.0 / (1.0 + q * q);
            let jitter = 0.15 * (12.0 * n_bp.tick(rng.signed())).clamp(-0.5, 0.5);
            let in_e1a = hp30.tick(pulse * (1.0 + jitter) - 0.5);
            let (x1, z1, x2, y) = (
                e1a.back(BLOCK),
                e2a.back(BLOCK),
                e1b.back(BLOCK),
                e2b.back(BLOCK),
            );
            let shaped = |x: f32| 0.17 * (x + (TAU * 0.125 * x).sin());
            let fb = hp12.tick(lp200.tick(shaped(x1) + shaped(x2)));
            e1a.push(in_e1a + 0.6 * y);
            e2a.push(x1 + fb);
            e1b.push(bp100.tick(z1));
            e2b.push(x2 + fb);
            let y1 = out_hp[0].tick(y);
            let engine_out = out_hp[1].tick(y1);

            // ---- gears: three sines at fixed multiples of the gear speed
            let fg = f_e * GEAR_RATIO;
            let mut gears = 0.0;
            for (k, (m, a)) in [(1.45f32, 0.2f32), (2.81, 0.33), (9.41, 0.14)]
                .iter()
                .enumerate()
            {
                ph_g[k] = (ph_g[k] + fg * m / sr).fract();
                if fg * m < 0.5 * sr {
                    gears += a * (TAU * ph_g[k]).sin();
                }
            }

            // ---- tail rotor
            ph_t = (ph_t + f_e * TAIL_RATIO / sr).fract();
            let o = (TAU * ph_t).cos();
            let env = 1.0 / (1.0 + 2.25 * o * o);
            let vc = tail_vcf.tick(rng.signed(), 100.0 + 500.0 * o, 6.0, sr);
            let tail = 0.1 * env + 0.3 * env * vc;

            // ---- main rotor
            let r = rate.at(i).clamp(2.0, 40.0);
            ph_r += r / sr;
            t_pulse += 1.0 / sr;
            if ph_r >= 1.0 {
                ph_r -= 1.0;
                t_pulse = 0.0;
                dur = 1.0 / r / 3.0;
            }
            let v = if t_pulse < dur { t_pulse / dur } else { 0.0 };
            let tent = ((1.0 - skew) * v).min(skew * (1.0 - v));
            let c = (std::f32::consts::FRAC_PI_2 * k_norm * tent).sin();
            let fc = devf * c + basef;
            let a_f = (5.0 - fc * fc * 1.1e-7).max(1e-6).sqrt().ln();
            let flat = rot_vcf.tick(rng.signed(), fc, q_rot, sr) * a_f * amp_q;
            let s34 = m34.tick(c * flat, 0.9 * shift * c);
            let s30 = m30.tick(s34 * c, 1.1 * shift * c);
            let s35 = m35.tick(s30, 1.2 * shift * c);
            let rotor = s34 + s30 + s35;

            // ---- body, vrdel, distance
            let body = ENGINE_MIX * engine_out + GEAR_MIX * gears + tail_level * tail + rotor;
            v_line.push(v_lp.tick(body));
            let moved = v_line.tap((25.0 + 0.2 * c) * 0.001 * sr);
            let d = dist.at(i).clamp(0.0, 1.0);
            let cut = 6000.0 * (1.0 - d) + 50.0;
            let mut l = moved;
            for p in d_lp.iter_mut() {
                p.tune(cut, sr);
                l = p.tick(l);
            }
            d_line.push(l);
            let echo = d_line.tap((2.0 + 3.0 * d) * 0.001 * sr);
            dc.tick(l + 0.5 * echo)
        })))
    }
}

// ------------------------------------------------------------------ rocket

/// A rocket motor (ROCKETLAUNCHER/rocket.pd, the `rocket` sub-patch): white
/// noise through the ignition notch (`hip~` at 900 T^145 Hz plus `lop~` 230 Hz
/// under it, so only the first flare of the burn is notched), three delay
/// taps 10 ms plus 4 (1 - T) ms apart, x 10 and clipped at +-1, into a pipe
/// of four `bp~` (Q 4) at 1, 3, 5 and 7 x (40 + 800 (T^30 + 0.3 T^4)) Hz, x
/// the one-pole-smoothed `sqrt T`, x a gain that drifts from 1 to slow
/// bipolar noise over 8 s (the "exhaust jitter"). T, the thrust, is the
/// patch's `line` (0 to 1 in 90 ms, then to 0 over 8 s), read every 20 ms
/// as Pd's `line` does.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rocket {
    /// Thrust, 0 to 1, a signal (a node: a `ramp`, an `env`). Omitted, the
    /// patch's own curve: up in 90 ms after `ignition`, then down to 0 over
    /// `burn` seconds. The pipe's resonance climbs with thrust (40 Hz at 0,
    /// about 1 kHz at 1), so a thrust that rises sweeps the roar upward.
    #[serde(default)]
    pub thrust: Option<In>,
    /// When the motor lights, seconds (default 0.02). Silent before it.
    #[serde(default)]
    pub ignition: Option<f32>,
    /// How long the built-in thrust takes to fall from 1 to 0 after the
    /// 90 ms rise, seconds, 0.5 to 60 (default 8). Ignored with `thrust`.
    #[serde(default)]
    pub burn: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Rocket {
    const BLURB: &'static str = "a rocket motor (Farnell): ignition-notched noise, delay taps, clipped, through a pipe whose resonance rises with thrust (a 0..1 signal; default: 90 ms up, then burn seconds down); silent before ignition; set a duration";
    fn example() -> serde_json::Value {
        serde_json::json!({"ignition": 0.02, "burn": 8, "seed": 1})
    }
}

impl Eval for Rocket {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let ignition = non_negative("ignition", self.ignition.unwrap_or(0.02))?;
        let burn = self.burn.unwrap_or(8.0);
        if !(burn.is_finite() && (0.5..=60.0).contains(&burn)) {
            return Err(format!("burn: {burn}: give seconds from 0.5 to 60"));
        }
        let thrust = match &self.thrust {
            Some(t) => Some(cx.mono("thrust", t)?),
            None => None,
        };
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);
        let grain = (0.02 * sr) as usize;
        let ign = (ignition * sr) as usize;
        // notchbands: hip~ x2 at fc, lop~ x2 at fc - 230.
        let mut hp = [Hip::default(), Hip::default()];
        let mut lp = [Lop::default(), Lop::default()];
        // comb: three taps on a 120 ms line.
        let mut comb = Line::new((0.05 * sr) as usize);
        // pipe.
        let mut pipe: Vec<Bp> = (0..4).map(|_| Bp::new(1.0, 4.0, sr)).collect();
        let mut sqrt_lp = Lop::new(1.0, sr);
        // exhaust jitter.
        let mut j_lp = [Lop::new(9.0, sr), Lop::new(9.0, sr)];
        let (mut t_c, mut d_c) = (0.0f32, 4.0f32);
        Ok(one(mono(cx.n, |i| {
            let age = i as f32 - ign as f32;
            let lit = i >= ign;
            if i % grain == 0 {
                let t = match &thrust {
                    Some(s) => s.at(i).clamp(0.0, 1.0),
                    None if !lit => 0.0,
                    None => {
                        let s = age / sr;
                        if s < 0.09 {
                            s / 0.09
                        } else {
                            (1.0 - (s - 0.09) / burn).max(0.0)
                        }
                    }
                };
                t_c = if lit { t } else { 0.0 };
                let fc = 900.0 * t_c.powf(145.0);
                for h in hp.iter_mut() {
                    h.tune(fc, sr);
                }
                for l in lp.iter_mut() {
                    l.tune(fc - 230.0, sr);
                }
                d_c = 4.0 * (1.0 - (t_c + 0.001)) + 1e-4;
                let l_c = 40.0 + 800.0 * (t_c.powf(30.0) + 0.3 * t_c.powf(4.0));
                for (k, b) in pipe.iter_mut().enumerate() {
                    b.tune(l_c * (2 * k + 1) as f32, 4.0, sr);
                }
            }
            let n = rng.signed();
            let mut hi = n;
            for h in hp.iter_mut() {
                hi = h.tick(hi);
            }
            let mut lo = n;
            for l in lp.iter_mut() {
                lo = l.tick(lo);
            }
            comb.push(hi + lo);
            let ms = 0.001 * sr;
            let w = 0.2;
            let taps = comb.tap((10.0 + d_c) * ms)
                + comb.tap((10.0 + d_c * (1.0 + w)) * ms)
                + comb.tap((10.0 + d_c * (1.0 + 2.0 * w)) * ms);
            let hot = (10.0 * 0.3 * taps).clamp(-1.0, 1.0);
            let pipe_out: f32 = pipe.iter_mut().map(|b| b.tick(hot)).sum();
            let level = sqrt_lp.tick(t_c.sqrt());
            // The jitter line: 0 to 0.8 over 8 s from ignition, ^ 0.25.
            let k = (0.8 * (age / sr / 8.0).clamp(0.0, 1.0)).powf(0.25);
            let j0 = j_lp[0].tick(rng.signed());
            let slow = 30.0 * j_lp[1].tick(j0);
            let gate = if lit { 1.0 } else { 0.0 };
            0.4 * gate * pipe_out * level * ((1.0 - k) + k * slow)
        })))
    }
}
