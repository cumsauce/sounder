//! Blasts: `explosion` and `gunshot`, finished-recipe models in the style of
//! `thunder`. Each is a few physical fields in and tuned DSP inside, all
//! computed offline from the seed (nothing is read from another node except
//! the optional `debris` event stream).
//!
//! Shared structure (both nodes):
//!
//! * a Friedlander pressure pulse `p(t) = (1 - t/t+) exp(-t/t+)` (zero net
//!   area), the positive phase `t+` set by `size` / `caliber`;
//! * a sine thump whose pitch falls exponentially;
//! * a noise body: white noise through a three-pole low-pass whose cutoff
//!   falls with the same envelope that fades the noise;
//! * a tail: dense noise, low-passed, decaying more slowly;
//! * propagation: the whole blast arrives `distance / 343` s late (plus a
//!   fixed 2.5 ms lead so the output stage's 2 ms fade-in never eats the
//!   attack), is low-passed by air absorption (two biquad poles at
//!   `16 kHz / (1 + d / 40 m)`, never under 120 Hz) and loses level as
//!   `1 / (1 + d / 100 m)`;
//! * a final `tanh(0.7 x)` so the output is always inside +-1.
//!
//! `gunshot` adds the supersonic crack: an N-wave (a fast fall from +1 to -1
//! over a fraction of a millisecond) that rides the bullet, so it arrives at
//! `distance / (0.9 * muzzle velocity)`, ahead of the muzzle blast at
//! `distance / 343`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::one;
use crate::buf::{Buf, Event};
use crate::dsp::{Biquad, FilterMode, flush, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound the delays use, m/s.
const SOUND_SPEED: f32 = 343.0;
/// Every blast starts this many seconds in, so the output stage's default
/// 2 ms fade-in does not touch the attack.
const LEAD: f32 = 0.0025;
/// Largest `distance`, metres (a 30 km blast arrives after 87 s).
const MAX_DISTANCE: f32 = 30_000.0;
/// Largest `debris` stream, events.
const MAX_DEBRIS: usize = 20_000;
/// Air low-pass distance scale of `explosion`, metres: the cutoff is
/// `16 kHz / (1 + d / this)`.
const EXPLOSION_AIR_M: f32 = 40.0;
/// The same for `gunshot`: 400 m keeps the mechanism and crack detail at
/// 50-120 m (cutoff 14.2 / 12.3 kHz; it was 7.1 / 4.0 kHz with 40 m, heard
/// as "lo-fi, no high end").
const GUNSHOT_AIR_M: f32 = 400.0;

/// Muzzle velocity of a gunshot, m/s, from the barrel length in metres: 380
/// m/s at 5 cm rising to 900 m/s for a very long barrel. `suppressed` caps
/// it at 310 (subsonic ammunition).
fn muzzle_velocity(barrel: f32, suppressed: bool) -> f32 {
    let v = 300.0 + 600.0 * (1.0 - (-barrel / 0.35).exp());
    if suppressed { v.min(310.0) } else { v }
}

/// The speed the crack is timed with: the bullet slows on the way.
fn mean_velocity(barrel: f32, suppressed: bool) -> f32 {
    0.9 * muzzle_velocity(barrel, suppressed)
}

fn friedlander(t: f32, tp: f32) -> f32 {
    let x = t / tp;
    (1.0 - x) * (-x).exp()
}

/// Three one-pole low-passes in series with a per-sample cutoff.
#[derive(Default)]
struct Lp3([f32; 3]);

impl Lp3 {
    /// Filters `x`, then scales by the noise-power compensation so band-
    /// limited noise keeps about the level of white noise at any cutoff.
    fn tick(&mut self, x: f32, fc: f32, sr: f32) -> f32 {
        let fc = fc.clamp(20.0, max_hz(sr));
        let a = 1.0 - (-TAU * fc / sr).exp();
        for s in &mut self.0 {
            *s = flush(*s + a * (x - *s));
        }
        self.0[2] * (0.5 * sr / fc).sqrt()
    }
}

/// Seconds to a sample shift, or None when the blast arrives after the end.
fn shift(delay: f32, cx: &Cx) -> Option<usize> {
    let s = (delay * cx.sr).round();
    (s < cx.n as f32).then_some(s as usize)
}

/// Air absorption and spreading loss for `distance` metres, then `dry`
/// added into `out` from sample `at`.
fn arrive(out: &mut [f32], dry: &mut [f32], at: usize, distance: f32, sr: f32, air_m: f32) {
    if distance > 0.0 {
        let fc = (16_000.0 / (1.0 + distance / air_m)).clamp(120.0, max_hz(sr));
        let mut a = Biquad::new();
        let mut b = Biquad::new();
        a.set(FilterMode::Lp, fc, 0.707, 0.0, sr);
        b.set(FilterMode::Lp, fc, 0.707, 0.0, sr);
        let g = 1.0 / (1.0 + distance / 100.0);
        for v in dry.iter_mut() {
            *v = b.tick(a.tick(*v)) * g;
        }
    }
    for (o, v) in out[at..].iter_mut().zip(dry.iter()) {
        *o += v;
    }
}

/// Clips the sum softly and removes any non-finite sample.
fn finish(out: &mut [f32]) {
    for v in out {
        *v = if v.is_finite() {
            (0.7 * *v).tanh()
        } else {
            0.0
        };
    }
}

fn check_distance(distance: f32) -> Result<f32, String> {
    if distance.is_finite() && (0.0..=MAX_DISTANCE).contains(&distance) {
        Ok(distance)
    } else {
        Err(format!(
            "distance: {distance}: give metres from 0 to {MAX_DISTANCE}"
        ))
    }
}

/// Adds one tick per event into `dry`: a 2.5 ms exponentially decaying noise
/// burst, high-passed by a first difference, scaled by the event value.
fn add_debris(dry: &mut [f32], events: &[Event], rng: &mut crate::dsp::Rng, sr: f32) {
    let len = (0.015 * sr) as usize;
    let tau = 0.0025 * sr;
    for e in events {
        if !(e.t.is_finite() && e.t >= 0.0) {
            continue;
        }
        let start = (e.t * f64::from(sr)).round();
        if start >= dry.len() as f64 {
            continue;
        }
        let start = start as usize;
        let amp = 0.3 * if e.value.is_finite() { e.value } else { 0.0 };
        let mut prev = 0.0f32;
        for j in 0..len.min(dry.len() - start) {
            let w = rng.signed();
            let v = (w - 0.7 * prev) * (-(j as f32) / tau).exp();
            prev = w;
            dry[start + j] += amp * v;
        }
    }
}

// ---------------------------------------------------------------- explosion

/// An explosion: a Friedlander pulse, a falling sub thump, a noise body and a
/// dense decaying tail, delayed and darkened by distance.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Explosion {
    /// Fireball radius, metres, 0.1 ..= 100. Larger is longer, lower and
    /// has more sub bass: the pressure pulse lasts `3 ms * size`, the thump
    /// starts at `160 Hz * size^-0.35` (at most 160, at least 22) and falls
    /// to 0.3 of that, the tail lasts `0.3 s * size^0.5`.
    pub size: f32,
    /// Metres from the blast to the listener, 0 ..= 30000. The blast
    /// arrives `distance / 343` s later (plus a 2.5 ms lead), air takes the
    /// highs off and the level falls as `1 / (1 + distance / 100)`.
    pub distance: f32,
    /// An event stream node: a tick of flying debris at each event's time
    /// (seconds after the blast, so they are delayed with it), scaled by the
    /// event's value. At most 20000 events. Default: none.
    #[serde(default)]
    pub debris: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Explosion {
    const BLURB: &'static str = "an explosion from its size and distance: pressure pulse, falling sub thump, noise body, long tail, optional debris ticks from an event stream";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 5, "distance": 100, "debris": "ev", "seed": 1})
    }
}

impl Eval for Explosion {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = self.size;
        if !(size.is_finite() && (0.1..=100.0).contains(&size)) {
            return Err(format!(
                "size: {size}: give metres of fireball radius from 0.1 to 100"
            ));
        }
        let distance = check_distance(self.distance)?;
        let events = match &self.debris {
            Some(d) => {
                let e = cx.events("debris", d)?;
                if e.len() > MAX_DEBRIS {
                    return Err(format!(
                        "debris: {} events: at most {MAX_DEBRIS}; thin the stream (poisson rate, burst count)",
                        e.len()
                    ));
                }
                e
            }
            None => Vec::new(),
        };
        let mut out = vec![0.0f32; cx.n];
        if let Some(at) = shift(LEAD + distance / SOUND_SPEED, cx) {
            let mut dry = explosion_dry(cx, size, cx.n - at, &events, self.seed);
            arrive(&mut out, &mut dry, at, distance, cx.sr, EXPLOSION_AIR_M);
        }
        finish(&mut out);
        Ok(one(Buf::Audio(out)))
    }
}

fn explosion_dry(cx: &Cx, size: f32, len: usize, debris: &[Event], seed: u64) -> Vec<f32> {
    let sr = cx.sr;
    let mut body_rng = cx.rng(seed);
    let mut tail_rng = cx.rng(seed.wrapping_add(0x9E37_79B9));
    let mut debris_rng = cx.rng(seed.wrapping_add(0x2545_F491));
    let tp = 0.003 * size;
    let f0 = (160.0 * size.powf(-0.35)).clamp(22.0, 160.0);
    let f1 = (0.3 * f0).max(16.0);
    let tau_pitch = 0.06 * size.powf(0.3);
    let tau_thump = 0.05 * size.powf(0.45);
    let tau_body = 0.12 * size.powf(0.4);
    let fc_body = 5000.0 * size.powf(-0.4);
    let tau_tail = 0.3 * size.sqrt();
    let fc_tail = 500.0 * size.powf(-0.2);
    let mut phase = 0.0f32;
    let (mut lp_body, mut lp_tail) = (Lp3::default(), Lp3::default());
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let pulse = if t < 20.0 * tp {
            0.7 * friedlander(t, tp)
        } else {
            0.0
        };
        let fq = f1 + (f0 - f1) * (-t / tau_pitch).exp();
        phase = (phase + TAU * fq / sr).fract();
        let thump = 0.6 * (-t / tau_thump).exp() * (TAU * phase).sin();
        let eb = (-t / tau_body).exp();
        let body = 0.35 * eb * lp_body.tick(body_rng.signed(), (fc_body * eb).max(80.0), sr);
        let et = (-t / tau_tail).exp() * (1.0 - (-t / 0.02).exp());
        let tail = 0.3 * et * lp_tail.tick(tail_rng.signed(), fc_tail * (0.3 + 0.7 * et), sr);
        *y = pulse + thump + body + tail;
    }
    add_debris(&mut dry, debris, &mut debris_rng, sr);
    dry
}

// ------------------------------------------------------------------ gunshot

/// A gunshot: a muzzle blast (pressure pulse, thump, noise body, tail) and,
/// for a supersonic round, an N-wave crack that arrives ahead of it.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gunshot {
    /// Bore diameter, millimetres, 2 ..= 60 (9 for a pistol, 5.56 a rifle,
    /// 12.7 a heavy machine gun). Larger is longer and lower: the pressure
    /// pulse lasts `0.15 ms * caliber`, the tail `0.1 s * sqrt(caliber/9)`.
    pub caliber: f32,
    /// Barrel length, metres, 0.05 ..= 1.5 (default 0.4). Longer gives a
    /// faster bullet (`300 + 600 * (1 - exp(-barrel / 0.35))` m/s: 380 at
    /// 5 cm, 708 at 40 cm) and a darker, weaker blast body (cutoff and
    /// level go as `(0.4 / barrel)^0.3` and `^0.45`); the crack exists when
    /// 0.9 times that exceeds 343 m/s.
    #[serde(default)]
    pub barrel: Option<f32>,
    /// A suppressor on subsonic ammunition (default false): no crack and no
    /// pitched thump; a gas puff (noise, 200 Hz - 4 kHz, about 32 dB under
    /// the open shot) and the action: a bright clack 5-8 ms in and its
    /// return 35-55 ms in (seeded), with energy up to 8-12 kHz.
    #[serde(default)]
    pub suppressed: bool,
    /// Metres from the gun to the listener, 0 ..= 30000 (default 0). The
    /// blast arrives after `distance / 343` s; the crack after `distance /
    /// (0.9 * muzzle velocity)`, so it leads the blast by the difference.
    #[serde(default)]
    pub distance: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Gunshot {
    const BLURB: &'static str = "a gunshot from caliber, barrel, suppressor and distance: muzzle blast plus, for a supersonic round, an N-wave crack that leads the blast";
    fn example() -> serde_json::Value {
        serde_json::json!({"caliber": 9, "barrel": 0.1, "suppressed": false, "distance": 50, "seed": 1})
    }
}

impl Eval for Gunshot {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let caliber = self.caliber;
        if !(caliber.is_finite() && (2.0..=60.0).contains(&caliber)) {
            return Err(format!(
                "caliber: {caliber}: give the bore in millimetres from 2 to 60 (9 pistol, 5.56 rifle)"
            ));
        }
        let barrel = self.barrel.unwrap_or(0.4);
        if !(barrel.is_finite() && (0.05..=1.5).contains(&barrel)) {
            return Err(format!(
                "barrel: {barrel}: give the barrel length in metres from 0.05 to 1.5"
            ));
        }
        let distance = check_distance(self.distance.unwrap_or(0.0))?;
        let supp = self.suppressed;
        let mut out = vec![0.0f32; cx.n];
        if let Some(at) = shift(LEAD + distance / SOUND_SPEED, cx) {
            let mut dry = blast_dry(cx, caliber, barrel, supp, cx.n - at, self.seed);
            arrive(&mut out, &mut dry, at, distance, cx.sr, GUNSHOT_AIR_M);
        }
        let vm = mean_velocity(barrel, supp);
        if vm > SOUND_SPEED
            && !supp
            && let Some(at) = shift(LEAD + distance / vm, cx)
        {
            let mut dry = crack_dry(cx, caliber, vm / SOUND_SPEED, cx.n - at);
            arrive(&mut out, &mut dry, at, distance, cx.sr, GUNSHOT_AIR_M);
        }
        finish(&mut out);
        Ok(one(Buf::Audio(out)))
    }
}

fn blast_dry(cx: &Cx, caliber: f32, barrel: f32, supp: bool, len: usize, seed: u64) -> Vec<f32> {
    if supp {
        return suppressed_dry(cx, caliber, len, seed);
    }
    let sr = cx.sr;
    let mut body_rng = cx.rng(seed);
    let mut tail_rng = cx.rng(seed.wrapping_add(0x9E37_79B9));
    let s = caliber / 9.0;
    // A shorter barrel leaves more unburnt powder: a louder, brighter blast.
    let bf = (0.4 / barrel).powf(0.3).clamp(0.5, 2.0);
    let (w_pulse, w_body, w_tail) = (0.5, 0.3, 0.2);
    let tp = 1.5e-4 * caliber;
    let f0 = (350.0 * s.powf(-0.6)).clamp(60.0, 700.0);
    let f1 = 0.25 * f0;
    let tau_pitch = 0.012 * s.sqrt();
    let tau_thump = 0.012 * s.powf(0.7);
    let tau_body = 0.008 * s.powf(0.7) * bf.sqrt();
    let fc_body = 9000.0 * s.powf(-0.35) * bf;
    let tau_tail = 0.1 * s.sqrt();
    let fc_tail = 2500.0 * s.powf(-0.3);
    let mut phase = 0.0f32;
    let (mut lp_body, mut lp_tail) = (Lp3::default(), Lp3::default());
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let pulse = if t < 20.0 * tp {
            w_pulse * friedlander(t, tp)
        } else {
            0.0
        };
        let fq = f1 + (f0 - f1) * (-t / tau_pitch).exp();
        phase = (phase + TAU * fq / sr).fract();
        let thump = 0.4 * (-t / tau_thump).exp() * (TAU * phase).sin();
        let eb = (-t / tau_body).exp();
        let body = w_body
            * bf.powf(1.5)
            * eb
            * lp_body.tick(body_rng.signed(), (fc_body * eb).max(400.0), sr);
        let et = (-t / tau_tail).exp() * (1.0 - (-t / 0.005).exp());
        let tail = w_tail * et * lp_tail.tick(tail_rng.signed(), fc_tail * (0.3 + 0.7 * et), sr);
        *y = pulse + thump + body + tail;
    }
    dry
}

/// A suppressed shot: no sine thump (a pitched thump with the gas muffled
/// read as "a metal sheet"), a gas puff of noise band-limited to 200 Hz -
/// 4 kHz, and the action: a bright clack (slide / bolt unlocking) 5-8 ms in
/// and its return 35-55 ms in, each a first-differenced noise click with a
/// 0.5 ms decay and a smaller echo 1-2 ms later, so energy reaches 8-10 kHz.
fn suppressed_dry(cx: &Cx, caliber: f32, len: usize, seed: u64) -> Vec<f32> {
    let sr = cx.sr;
    let mut puff_rng = cx.rng(seed);
    let mut act_rng = cx.rng(seed.wrapping_add(0x2545_F491));
    let s = caliber / 9.0;
    let gain = 0.04;
    let tau_puff = 0.018 * s.sqrt();
    let attack = 0.0004;
    let (mut lp, mut hp_state) = (Lp3::default(), 0.0f32);
    let a_hp = 1.0 - (-TAU * 200.0 / sr).exp();
    let fc_puff = (4000.0 * s.powf(-0.2)).min(max_hz(sr));
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let env = (1.0 - (-t / attack).exp()) * (-t / tau_puff).exp();
        let n = lp.tick(puff_rng.signed(), fc_puff, sr);
        hp_state = flush(hp_state + a_hp * (n - hp_state));
        *y = gain * 0.8 * env * (n - hp_state);
    }
    // Round 2: body (round 1 read as "a small hit"): a noise thump
    // band-limited to 80-400 Hz (two one-pole high-passes at 80 Hz, the
    // Lp3 at 400 Hz), 1 ms attack, a 25 ms x sqrt(caliber / 9) decay. Noise,
    // not a pitched sine (the old sine thump read as "a metal sheet").
    let mut thump_rng = cx.rng(seed.wrapping_add(0x7F4A_7C15));
    let tau_th = 0.025 * s.sqrt();
    let a80 = 1.0 - (-TAU * 80.0 / sr).exp();
    let (mut lpt, mut h1, mut h2) = (Lp3::default(), 0.0f32, 0.0f32);
    let th_len = ((8.0 * tau_th * sr) as usize).min(len);
    for (i, y) in dry.iter_mut().enumerate().take(th_len) {
        let t = i as f32 / sr;
        let env = (1.0 - (-t / 0.001).exp()) * (-t / tau_th).exp();
        let n = lpt.tick(thump_rng.signed(), 400.0, sr);
        h1 = flush(h1 + a80 * (n - h1));
        let x = n - h1;
        h2 = flush(h2 + a80 * (x - h2));
        *y += gain * 2.5 * env * (x - h2);
    }
    // The action, scaled by the caliber (a heavier slide clacks louder and a
    // little longer), placed with seeded jitter.
    let clicks = [
        (0.005 + 0.003 * act_rng.unit(), 0.1),
        (0.035 + 0.02 * act_rng.unit(), 0.07),
    ];
    let tau = 0.0005 * s.powf(0.3);
    for (at, level) in clicks {
        for (dt, l) in [(0.0, 1.0), (0.001 + 0.001 * act_rng.unit(), 0.4)] {
            let start = ((at + dt) * sr).round() as usize;
            let n = ((8.0 * tau * sr) as usize).min(len.saturating_sub(start));
            let mut prev = 0.0f32;
            for j in 0..n {
                let w = act_rng.signed();
                let v = (w - 0.85 * prev) * (-(j as f32) / (tau * sr)).exp();
                prev = w;
                dry[start + j] += gain * level * l * v;
            }
        }
    }
    dry
}

/// The N-wave: +1 falling linearly to -1 over `T`, `T` = 0.12 ms times
/// sqrt(caliber / 9), then 0; amplitude from the Mach number; one pole at
/// 14 kHz takes the edge off the two steps.
fn crack_dry(cx: &Cx, caliber: f32, mach: f32, len: usize) -> Vec<f32> {
    let sr = cx.sr;
    let width = (1.2e-4 * (caliber / 9.0).sqrt() * sr).max(3.0);
    let amp = 0.9 * ((mach - 1.0) / 0.5).clamp(0.4, 1.0);
    let a = 1.0 - (-TAU * 14_000.0f32.min(max_hz(sr)) / sr).exp();
    let mut y = 0.0f32;
    (0..len)
        .map(|i| {
            let x = i as f32;
            let w = if x < width {
                amp * (1.0 - 2.0 * x / width)
            } else {
                0.0
            };
            y = flush(y + a * (w - y));
            y
        })
        .collect()
}
