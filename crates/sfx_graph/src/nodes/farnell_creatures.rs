//! Creature nodes ported from the Pure Data patches of Andy Farnell's
//! *Designing Sound* (see `docs/porting-farnell.md`): `roar` is
//! MAMMALS/roar.pd (with articulate.pd's envelope and vowels.pd's formant
//! table), `fly` is INSECTS/buzzing-housefly.pd with flywing.pd (fly1.pd is
//! the same wing in one patch, flywing-waveform.pd the measured wing
//! table), `cicada` is INSECTS/cicada1.pd, cicada2.pd and cicada3.pd.
//!
//! The filters are exact copies of Pd's (`bp~`, `lop~`, `hip~` before
//! 0.44), as in `farnell.rs`.

use std::f32::consts::{PI, TAU};

use serde::Deserialize;

use super::{mono, one};
use crate::node::{Cx, Describe, Eval, Outs};

/// Pd's `bp~` (centre Hz, Q); `set` retunes it as a control message would,
/// keeping its state.
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
    fn set(&mut self, f: f32, q: f32, sr: f32) {
        let n = PdBp::new(f, q, sr);
        self.c1 = n.c1;
        self.c2 = n.c2;
        self.gain = n.gain;
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

fn range(field: &str, x: f32, lo: f32, hi: f32) -> Result<f32, String> {
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give {lo} to {hi}"))
    }
}

/// `lo` at 0, `mid` at 0.5, `hi` at 1.
fn mid3(a: f32, lo: f32, mid: f32, hi: f32) -> f32 {
    if a < 0.5 {
        lo + (mid - lo) * a * 2.0
    } else {
        mid + (hi - mid) * (a - 0.5) * 2.0
    }
}

/// The patches' noise lowpassed at a few Hz was tuned at 44.1 kHz; the
/// filtered noise's level goes as the square root of the rate, so scale the
/// white noise to keep the wander the book's.
fn rate_comp(sr: f32) -> f32 {
    (sr / 44100.0).sqrt()
}

// -------------------------------------------------------------------- roar

/// Vowel formants (F1, F2, F3 Hz) from vowels.pd's message boxes.
const VOWELS: [(&str, [f32; 3]); 9] = [
    ("i", [280.0, 2250.0, 2900.0]),
    ("I", [400.0, 1900.0, 2550.0]),
    ("E", [550.0, 1770.0, 2490.0]),
    ("ae", [690.0, 1660.0, 2490.0]),
    ("^", [640.0, 1190.0, 2390.0]),
    ("u", [310.0, 870.0, 2250.0]),
    ("U", [450.0, 1030.0, 2380.0]),
    ("&", [500.0, 1500.0, 2500.0]),
    ("A", [710.0, 1100.0, 2640.0]),
];

/// A roar or growl (MAMMALS/roar.pd): vocal cords, then a throat. The cord
/// model is a phasor at the pitch whose cosine-bump pulse is squashed to
/// `1 / (1 + (width x pulse x ripple cosine)^2)` and mixed with noise; the
/// throat is eight band-passes (`bp~`, Q 44 to 60) at `base`, then each
/// `separation x 1.414^k` higher, summed. articulate.pd's `line` runs a
/// rise and a fall (`cos` shaped) and drives pitch, ripple, base,
/// separation and resonance, and the loudness is the square root of the
/// part of it above one half, so the roar swells out of silence.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Roar {
    /// Body size, 0.2 to 5 (default 1, the patch: a big cat or bear). Pitch
    /// and throat formants go as 1 / size, so 2 is an octave down.
    #[serde(default)]
    pub size: Option<f32>,
    /// 0 to 1 (default 0.5, the patch's sliders). More aggression is
    /// noisier, rougher (cord width and ripple), swings further in pitch,
    /// attacks faster and rings less in the throat.
    #[serde(default)]
    pub aggression: Option<f32>,
    /// The throat: `"roar"` (default, the patch's eight-band comb) or a
    /// vowel from vowels.pd, three formants at Q 34: `i I E ae ^ u U & A`.
    #[serde(default)]
    pub vowel: Option<String>,
    /// Seconds the roar is audible, 0.1 to 10 (default 1). The patch's
    /// `duration` slider (0.1 to 3 s, default 1.83 s) times a rise and fall
    /// of which only the top third is above its loudness gate, so the
    /// audible part is a third of it: this is that third, and the silent
    /// two thirds of the rise are cut.
    #[serde(default)]
    pub length: Option<f32>,
    /// How alive the voice is, 0 to 1 (default 0.6). 0 is the patch: one
    /// clean rise and fall of pitch and level, which a listener heard as "a
    /// cow ... too digital, goes up then immediately down perfectly with no
    /// wavering". Above 0 the pitch wanders slowly (breath surges, a few Hz)
    /// and jitters fast, the level shudders, alternate cord pulses are
    /// louder (a growl's subharmonic), and the fall sags unevenly.
    #[serde(default)]
    pub wobble: Option<f32>,
    /// Start time, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Roar {
    const BLURB: &'static str = "a roar or growl (Farnell): a squashed cosine-pulse cord model with ripple and noise through an eight-band throat comb (or a vowel's three formants), swelling out of silence; size, aggression, vowel, length";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 1, "aggression": 0.5, "vowel": "roar", "length": 1})
    }
}

impl Eval for Roar {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = range("size", self.size.unwrap_or(1.0), 0.2, 5.0)?;
        let ag = range("aggression", self.aggression.unwrap_or(0.5), 0.0, 1.0)?;
        let d = range("length", self.length.unwrap_or(1.0), 0.1, 10.0)?;
        let at = range("at", self.at.unwrap_or(0.0), 0.0, 3600.0)?;
        let wob = range("wobble", self.wobble.unwrap_or(0.6), 0.0, 1.0)?;
        let vowel = match self.vowel.as_deref() {
            None | Some("roar") => None,
            Some(v) => match VOWELS.iter().find(|(n, _)| *n == v) {
                Some((_, f)) => Some(*f),
                None => {
                    return Err(format!(
                        "vowel: `{v}`: give roar, i, I, E, ae, ^, u, U, & or A"
                    ));
                }
            },
        };
        let sr = cx.sr;
        let k = 1.0 / size;
        // The sliders' saved values at aggression 0.5.
        let base = 60.7 * k;
        let frange = mid3(ag, 20.0, 65.2, 200.0) * k;
        let cordr = mid3(ag, 0.8, 1.86, 4.0);
        let cordw = mid3(ag, 20.0, 42.3, 90.0);
        let cordnz = mid3(ag, 0.05, 0.268, 0.7);
        let (tractb, tractbm) = (338.0 * k, 179.8 * k);
        let (tractl, traclm) = (123.7 * k, 41.2 * k);
        let (tractres, tractrm) = (mid3(ag, 50.0, 44.4, 36.0), 25.9);
        let ad = mid3(ag, 0.8, 0.601, 0.3);
        // articulate.pd's duration: the audible third is `d`, so the patch's
        // is 3 d, and the first two thirds of its rise are silent.
        let (tr, tf) = (3.0 * d * ad, 3.0 * d * (1.0 - ad));
        let shift = 2.0 / 3.0 * tr;
        let start = (at * sr) as usize;
        let nyq = 0.45 * sr;
        let mut rng = cx.rng(self.seed);
        let mut ripple = PdLop::new(10.0, sr);
        let mut hip = PdHip::new(1.0, sr);
        let mut comb = vec![PdBp::default(); 8];
        let mut forms = vec![PdBp::default(); 3];
        if let Some(f) = vowel {
            for (b, hz) in forms.iter_mut().zip(f) {
                b.set(hz * k, 34.0, sr);
            }
        }
        let block = ((0.01 * sr) as usize).max(1);
        let mut phase = 0.0f64;
        let (mut amp, mut a, mut dead) = (0.0f32, 0.25f32, false);
        let mut freqs = [0.0f32; 8];
        // Wobble: a slow wander (new target every 60-180 ms, slewed), a fast
        // jitter (low-passed noise), and a level shudder.
        let mut wander = (0.0f32, 0.0f32, 0usize);
        let mut jit = PdLop::new(30.0, sr);
        let mut shud = PdLop::new(18.0, sr);
        let mut pulse_n = 0u32;
        let mut last_p = 0.0f32;
        let slew = 1.0 - (-1.0 / (0.05 * sr)).exp();
        Ok(one(mono(cx.n, |i| {
            if i < start || dead {
                return 0.0;
            }
            let t_rel = (i - start) as f32 / sr;
            let t = t_rel + shift;
            if t_rel >= d {
                dead = true;
                return 0.0;
            }
            if (i - start).is_multiple_of(block) {
                // articulate.pd: line 0 -> 1 over d x ad, then 1 -> 0.
                let l = if t < tr { t / tr } else { 1.0 - (t - tr) / tf };
                a = 0.75 - 0.5 * (PI / 2.0 * l.clamp(0.0, 1.0)).cos();
                amp = (2.0 * (a - 0.5)).max(0.0).sqrt();
                if vowel.is_none() {
                    let q = tractres + a * tractrm;
                    // base, then steps of separation x 1, 1.414, 2, 2.83, ...
                    let mut f = tractb + a * tractbm;
                    let mut step = tractl + a * traclm;
                    for (j, fr) in freqs.iter_mut().enumerate() {
                        if j > 0 {
                            f += step;
                            step *= std::f32::consts::SQRT_2;
                        }
                        *fr = f;
                    }
                    for (bp, fr) in comb.iter_mut().zip(freqs) {
                        if fr < nyq {
                            bp.set(fr, q, sr);
                        }
                    }
                }
            }
            // The cord model (pd cord model), with the wobble on its pitch.
            if wob > 0.0 {
                if wander.2 == 0 {
                    wander.1 = rng.signed();
                    wander.2 = ((0.06 + 0.12 * rng.unit()) * sr) as usize;
                }
                wander.2 -= 1;
                wander.0 += slew * (wander.1 - wander.0);
            }
            let jitter = jit.tick(rng.signed()) * 6.0;
            let sag = if t >= tr {
                1.0 - 0.12 * wob * ((t - tr) / tf).min(1.0)
            } else {
                1.0
            };
            let bend = 1.0 + wob * (0.22 * (0.5 + ag) * wander.0 + 0.06 * jitter);
            let f0 = ((base + a * frange) * bend * sag).max(5.0);
            phase = (phase + f64::from(f0 / sr)).rem_euclid(1.0);
            let p = phase as f32;
            if p < last_p {
                pulse_n += 1;
            }
            last_p = p;
            let sub = if pulse_n.is_multiple_of(2) {
                1.0
            } else {
                1.0 - 0.7 * wob * ag.max(0.4)
            };
            let shudder = 1.0 + wob * (0.3 * shud.tick(rng.signed()) * 4.0 + 0.15 * wander.0);
            let r = ripple.tick(a * cordr);
            let g = 0.5 * (1.0 - (TAU * p).cos());
            let x = cordw * g * (TAU * (p - 0.5) * r).cos();
            let y = 1.0 / (1.0 + x * x);
            let nz = (cordnz + 0.25 * wob).min(0.85);
            let cord = hip.tick(y * (1.0 - nz) + y * nz * rng.signed());
            let e = 0.1 * cord * sub * shudder.max(0.0);
            let tract = if vowel.is_some() {
                forms.iter_mut().map(|b| b.tick(e)).sum::<f32>()
            } else {
                comb.iter_mut()
                    .zip(&freqs)
                    .map(|(b, fr)| if *fr < nyq { b.tick(e) } else { 0.0 })
                    .sum::<f32>()
            };
            32.0 * amp * tract
        })))
    }
}

// --------------------------------------------------------------------- fly

/// The measured wing waveform of flywing-waveform.pd (after Bennet and
/// Ewing 1968): one cycle, 256 points (the patch's table has 3 guard points
/// for `tabosc4~`).
const WING: [f32; 256] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.005,
    0.01, 0.015, 0.015, 0.02, 0.03, 0.03, 0.04, 0.0425, 0.045, 0.05, 0.06, 0.065, 0.07, 0.08,
    0.085, 0.09, 0.095, 0.11, 0.11, 0.125, 0.14, 0.145, 0.165, 0.18, 0.195, 0.2, 0.22, 0.23, 0.245,
    0.26, 0.275, 0.305, 0.31, 0.32, 0.33, 0.38, 0.395, 0.42, 0.43, 0.455, 0.48, 0.495, 0.515, 0.53,
    0.55, 0.56, 0.585, 0.6, 0.62, 0.645, 0.665, 0.69, 0.695, 0.73, 0.75, 0.78, 0.79, 0.83, 0.845,
    0.885, 0.905, 0.925, 0.95, 0.96, 0.985, 0.985, 0.96, 0.94, 0.895, 0.84, 0.8, 0.74, 0.715,
    0.655, 0.595, 0.495, 0.45, 0.38, 0.36, 0.31, 0.24, 0.215, 0.15, 0.12, 0.07, 0.04, -0.015,
    -0.04, -0.095, -0.125, -0.15, -0.185, -0.195, -0.2, -0.23, -0.265, -0.285, -0.31, -0.32, -0.34,
    -0.355, -0.36, -0.37, -0.375, -0.39, -0.39, -0.4, -0.4, -0.41, -0.415, -0.42, -0.425, -0.43,
    -0.435, -0.435, -0.44, -0.44, -0.44, -0.445, -0.445, -0.445, -0.445, -0.445, -0.445, -0.445,
    -0.445, -0.445, -0.445, -0.44, -0.44, -0.4375, -0.43, -0.43, -0.425, -0.42, -0.415, -0.41,
    -0.395, -0.39, -0.38, -0.36, -0.36, -0.345, -0.335, -0.315, -0.305, -0.265, -0.255, -0.19,
    -0.165, -0.15, -0.105, -0.075, -0.025, -0.015, 0.015, 0.025, 0.045, 0.055, 0.08, 0.115, 0.125,
    0.16, 0.165, 0.195, 0.2, 0.215, 0.22, 0.22, 0.23, 0.235, 0.235, 0.235, 0.235, 0.2325, 0.225,
    0.22, 0.21, 0.205, 0.195, 0.18, 0.175, 0.165, 0.16, 0.14, 0.13, 0.11, 0.095, 0.07, 0.05, 0.035,
    0.015, 0.009999, -0.005001, -0.02, -0.04, -0.045, -0.055, -0.065, -0.07, -0.08, -0.08, -0.085,
    -0.085, -0.085, -0.085, -0.085, -0.085, -0.085, -0.085, -0.085, -0.08, -0.08, -0.07, -0.065,
    -0.055, -0.05, -0.035, -0.03, -0.025, -0.025, -0.02, -0.02, -0.02, -0.02, -0.015, -0.015,
    -0.01, -0.01, -0.005001, -0.005001, -0.005001, -0.005001, 0.004999, 0.004999, 0.015, 0.015,
    0.015,
];

/// One cycle of the table at phase `p` (0..1), four-point (cubic) as
/// `tabosc4~`.
fn wing_table(p: f32) -> f32 {
    let x = p * 256.0;
    let i = x.floor();
    let f = x - i;
    let i = i as isize;
    let at = |k: isize| WING[(k.rem_euclid(256)) as usize];
    let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let cminusb = c - b;
    b + f
        * (cminusb
            - 0.166_666_7 * (1.0 - f) * ((d - a - 3.0 * cminusb) * f + (d + 2.0 * a - 3.0 * b)))
}

/// One housefly wing (flywing.pd). `p` is the wingbeat phase, `r` the wing
/// resonance (cycles of ripple over the downstroke).
fn wing_model(p: f32, r: f32) -> f32 {
    // A sawtooth with a slow rise and a fast return: 12 min(0.2 p, 1 - p) - 1.
    let s = 12.0 * (0.2 * p).min(1.0 - p) - 1.0;
    let (down, up) = (s.min(0.0), s.max(0.0));
    let w = r * down;
    let ripple = (TAU * (w - w.floor())).cos();
    down * (1.0 + 0.5 * ripple) + 2.0 * up.powi(4)
}

/// A buzzing housefly (INSECTS/buzzing-housefly.pd): two wings
/// (flywing.pd), each a phasor at the wingbeat whose sawtooth is split into
/// a downstroke that rings `res` cycles of a cosine ("wing resonance") and
/// an upstroke raised to the fourth power, high-passed at 700 Hz. The
/// wingbeat wanders by 700 x slow noise Hz (a few Hz), the resonance by 40 x
/// slow noise, and the second wing sits a hair away from the first.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fly {
    /// Wingbeat, Hz, 20 to 1000 (default 220, a housefly's).
    #[serde(default)]
    pub freq: Option<f32>,
    /// Wing resonance, cycles of ripple per downstroke, 0 to 20 (default 5;
    /// the ripple sits at about `res` x 2.4 x `freq` Hz, 2.6 kHz at the
    /// defaults).
    #[serde(default)]
    pub res: Option<f32>,
    /// How much the wingbeat and resonance wander, 0 to 5 times the patch's
    /// (default 1: a few Hz). 0 is a steady machine.
    #[serde(default)]
    pub flutter: Option<f32>,
    /// `"model"` (default, flywing.pd) or `"table"` (the measured wing
    /// waveform of flywing-waveform.pd, no resonance).
    #[serde(default)]
    pub wave: Option<String>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Fly {
    const WORDS: &'static [&'static str] = &["wave"];
    const BLURB: &'static str = "a buzzing housefly (Farnell): two wings, each a wingbeat sawtooth with a ringing downstroke and a fourth-power upstroke, high-passed at 700 Hz, wandering a few Hz; freq, res, flutter";
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 220, "res": 5, "flutter": 1, "wave": "model"})
    }
}

impl Eval for Fly {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let freq = range("freq", self.freq.unwrap_or(220.0), 20.0, 1000.0)?;
        let res = range("res", self.res.unwrap_or(5.0), 0.0, 20.0)?;
        let flutter = range("flutter", self.flutter.unwrap_or(1.0), 0.0, 5.0)?;
        let table = match self.wave.as_deref() {
            None | Some("model") => false,
            Some("table") => true,
            Some(w) => return Err(format!("wave: `{w}`: give model or table")),
        };
        let sr = cx.sr;
        let nc = rate_comp(sr);
        let mut rng = cx.rng(self.seed);
        let (mut a1, mut a2) = (PdLop::new(4.0, sr), PdLop::new(4.0, sr));
        let (mut b1, mut b2) = (PdLop::new(5.0, sr), PdLop::new(5.0, sr));
        let mut hip = [PdHip::new(700.0, sr), PdHip::new(700.0, sr)];
        let mut ph = [0.0f64; 2];
        Ok(one(mono(cx.n, |_| {
            let na = a2.tick(a1.tick(nc * rng.signed()));
            let nb = b2.tick(b1.tick(nc * rng.signed()));
            let f1 = freq + flutter * 700.0 * na;
            let f2 = f1 + flutter * 3.0 * nb;
            let r = res + flutter * 40.0 * nb;
            let mut sum = 0.0;
            for (w, f) in [f1, f2].into_iter().enumerate() {
                ph[w] = (ph[w] + f64::from(f / sr)).rem_euclid(1.0);
                let p = ph[w] as f32;
                sum += if table {
                    0.5 * wing_table(p)
                } else {
                    hip[w].tick(wing_model(p, r))
                };
            }
            0.1 * sum
        })))
    }
}

// ------------------------------------------------------------------ cicada

/// The three call types of cicada3.pd: (name, call rate Hz, gate depth).
const CALLS: [(&str, f32, f32); 3] = [
    ("trill", 30.0, 3.0),
    ("chirp", 8.0, 6.0),
    ("tick", 0.5, 128.0),
];

/// A cicada (INSECTS/cicada1.pd, cicada2.pd, cicada3.pd): noise in the 5 to
/// 8 kHz band shaped by a fast pulse train. Where `cricket` rings very
/// narrow tones, these are broadband tymbal buzzes.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cicada {
    /// `"buzz"` (default, cicada1: band noise times two wandering sawtooth
    /// pulse trains at about 90 and 20 Hz), `"rattle"` (cicada2: a 120 Hz
    /// wandering sawtooth hit through four 5 to 8 kHz bands, gated by a
    /// 12 Hz half wave) or `"call"` (cicada3: noise at 5.5 and 7.5 kHz
    /// gated by `1 / (1 + (depth cos)^2)` pulses).
    #[serde(default)]
    pub kind: Option<String>,
    /// Scales every band's centre, 0.5 to 2 (default 1: the book's 5 to
    /// 8 kHz).
    #[serde(default)]
    pub pitch: Option<f32>,
    /// Scales the pulse and call rates, 0.25 to 4 (default 1).
    #[serde(default)]
    pub rate: Option<f32>,
    /// For `kind: "call"`: `"cycle"` (default: the patch picks one of the
    /// three at random every 5 s), `"trill"` (30 Hz, shallow), `"chirp"`
    /// (8 Hz) or `"tick"` (one pulse a second).
    #[serde(default)]
    pub call: Option<String>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Cicada {
    const WORDS: &'static [&'static str] = &["kind", "call"];
    const BLURB: &'static str = "a cicada (Farnell): broadband 5-8 kHz tymbal noise in pulse trains; kind buzz / rattle / call (the three cicada patches), pitch, rate; set a duration";
    fn example() -> serde_json::Value {
        serde_json::json!({"kind": "buzz", "pitch": 1, "rate": 1})
    }
}

impl Eval for Cicada {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let pitch = range("pitch", self.pitch.unwrap_or(1.0), 0.5, 2.0)?;
        let rate = range("rate", self.rate.unwrap_or(1.0), 0.25, 4.0)?;
        let kind = self.kind.as_deref().unwrap_or("buzz");
        let call = match self.call.as_deref() {
            None | Some("cycle") => None,
            Some(c) => match CALLS.iter().position(|(n, _, _)| *n == c) {
                Some(p) => Some(p),
                None => return Err(format!("call: `{c}`: give cycle, trill, chirp or tick")),
            },
        };
        let sr = cx.sr;
        let nc = rate_comp(sr);
        let mut rng = cx.rng(self.seed);
        // hip~ 5000 x2 then lop~ 8000 x2: the band noise of cicada1 and 3.
        let mut hp = [PdHip::new(5000.0, sr), PdHip::new(5000.0, sr)];
        let mut lp = [PdLop::new(8000.0, sr), PdLop::new(8000.0, sr)];
        let mut band = |w: f32| {
            let a = hp[0].tick(w);
            let b = hp[1].tick(a);
            let c = lp[0].tick(b);
            lp[1].tick(c)
        };
        match kind {
            "buzz" => {
                let (mut m1, mut m2) = (PdLop::new(2.0, sr), PdLop::new(2.0, sr));
                let mut env = PdLop::new(70.0, sr);
                let mut bp = PdBp::new(6000.0 * pitch, 7.0, sr);
                let mut hip = PdHip::new(4000.0 * pitch, sr);
                let (mut p1, mut p2) = (0.0f64, 0.0f64);
                Ok(one(mono(cx.n, |_| {
                    let w = rng.signed();
                    let n = band(w);
                    let m = m2.tick(m1.tick(nc * w));
                    p1 = (p1 + f64::from(-90.0 * rate * (1.0 + 30.0 * m) / sr)).rem_euclid(1.0);
                    p2 = (p2 + f64::from(-20.0 * rate * (1.0 + 10.0 * m) / sr)).rem_euclid(1.0);
                    let (a, b) = (p1 as f32, p2 as f32);
                    let e = env.tick(a.powi(4) + b * b);
                    0.2 * hip.tick(bp.tick(n * e))
                })))
            }
            "rattle" => {
                let mut wander = PdBp::new(120.0, 40.0, sr);
                let mut hip = [PdHip::new(12000.0, sr), PdHip::new(12000.0, sr)];
                let mut bands = [
                    PdBp::new(5000.0 * pitch, 30.0, sr),
                    PdBp::new(6000.0 * pitch, 30.0, sr),
                    PdBp::new(8000.0 * pitch, 30.0, sr),
                    PdBp::new(7000.0 * pitch, 30.0, sr),
                ];
                let mut out = PdBp::new(6000.0 * pitch, 5.0, sr);
                let (mut p, mut gate) = (0.0f64, 0.0f64);
                Ok(one(mono(cx.n, |_| {
                    let y = wander.tick(rng.signed());
                    p = (p + f64::from(-120.0 * rate * (1.0 + 190.0 * y) / sr)).rem_euclid(1.0);
                    gate = (gate + f64::from(12.0 * rate / sr)).fract();
                    let h0 = hip[0].tick(p as f32);
                    let h = hip[1].tick(h0);
                    let s = 2.0 * bands.iter_mut().map(|b| b.tick(h)).sum::<f32>();
                    let g = (TAU * gate as f32).cos().clamp(0.0, 1.0);
                    out.tick(s * g)
                })))
            }
            "call" => {
                let mut b7 = PdBp::new(7500.0 * pitch, 40.0, sr);
                let mut b5 = PdBp::new(5500.0 * pitch, 40.0, sr);
                let mut hip = PdHip::new(4000.0 * pitch, sr);
                let (mut carrier, mut cp) = (0.0f64, 0.0f64);
                let mut kind_now = call.unwrap_or(0);
                let every = (5.0 * sr) as usize;
                Ok(one(mono(cx.n, |i| {
                    // metro 5000: a new random call type every 5 s.
                    if call.is_none() && i.is_multiple_of(every) {
                        kind_now = ((rng.unit() * 3.0) as usize).min(2);
                    }
                    let w = rng.signed();
                    let n = band(w);
                    let (_, cf, depth) = CALLS[kind_now];
                    carrier = (carrier + f64::from(500.0 * pitch / sr)).fract();
                    cp = (cp + f64::from(cf * rate / sr)).fract();
                    let d = depth * (TAU * cp as f32).cos();
                    let gate = 1.0 / (1.0 + d * d);
                    let x = (b7.tick(n) + b5.tick(n)) * (TAU * carrier as f32).cos();
                    0.7 * hip.tick(x * gate)
                })))
            }
            k => Err(format!("kind: `{k}`: give buzz, rattle or call")),
        }
    }
}
