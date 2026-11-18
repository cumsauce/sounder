//! Spaces: the room, plate and spring models behind `reverb.model`, and the
//! `ir` node that makes an impulse response for `convolve`.
//!
//! Tests: `crates/sfx_analysis/tests/space.rs`. Docs: `docs/nodes/space.md`.

use std::f32::consts::{PI, TAU};

use serde::Deserialize;

use super::time::fdn_tail;
use crate::buf::Buf;
use crate::dsp::{Biquad, DelayLine, FilterMode, flush};
use crate::node::{Cx, Describe, Eval, Outs};

/// Speed of sound, m/s.
const C: f32 = 343.0;

// ------------------------------------------------------------ shoebox room

/// A shoebox room, its listener and its source, for the image-source method
/// (Allen and Berkley 1979): each wall reflection mirrors the source.
struct Shoebox {
    dims: [f32; 3],
    /// Mean wall absorption, 0..1 (energy).
    alpha: f32,
    src: [f32; 3],
    /// The listener's centre; the ears sit 9 cm either side along y.
    ear: [f32; 3],
}

/// One arrival: `k` wall hits, `t` seconds after the earliest direct sound,
/// pressure `amp` relative to the direct sound at the nearer ear.
struct Tap {
    k: u32,
    t: f32,
    amp: f32,
}

/// Highest number of wall hits an image source may have. 3 gives 63 images
/// per ear: a fixed cost.
const ORDER: i32 = 3;

impl Shoebox {
    /// A room whose longest side is `longest` metres (sides 1 : 0.8 : 0.4).
    fn new(longest: f32, alpha: f32) -> Shoebox {
        let dims = [longest, longest * 0.8, longest * 0.4];
        Shoebox {
            dims,
            alpha,
            src: [dims[0] * 0.3, dims[1] * 0.45, dims[2] * 0.4],
            ear: [dims[0] * 0.7, dims[1] * 0.5, dims[2] * 0.4],
        }
    }

    fn volume(&self) -> f32 {
        self.dims[0] * self.dims[1] * self.dims[2]
    }

    fn surface(&self) -> f32 {
        let [a, b, c] = self.dims;
        2.0 * (a * b + b * c + a * c)
    }

    /// Distance from the source to an ear (`side` -1 left, +1 right), m.
    fn ear_distance(&self, side: f32) -> f32 {
        let mut e = self.ear;
        e[1] += side * 0.09;
        (0..3)
            .map(|a| (self.src[a] - e[a]).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Distance from the source to the nearer ear, metres: the direct
    /// sound there has amplitude 1.
    fn distance(&self) -> f32 {
        self.ear_distance(-1.0).min(self.ear_distance(1.0))
    }

    /// Sabine reverberation time, seconds: 0.161 V / (S alpha).
    fn t60(&self) -> f32 {
        0.161 * self.volume() / (self.surface() * self.alpha)
    }

    /// The arrivals at one ear (`side` -1 left, +1 right) with up to
    /// [`ORDER`] wall hits, the direct sound (k = 0) included. `t0` is the
    /// earliest direct arrival over both ears, seconds-of-flight.
    fn taps(&self, side: f32, t0: f32, r_ref: f32) -> Vec<Tap> {
        let beta = (1.0 - self.alpha).max(0.0).sqrt();
        let mut ear = self.ear;
        ear[1] += side * 0.09;
        let mut out = Vec::new();
        for nx in -ORDER..=ORDER {
            for ny in -ORDER..=ORDER {
                for nz in -ORDER..=ORDER {
                    for q in 0..8u32 {
                        let (qx, qy, qz) = ((q & 1) as i32, ((q >> 1) & 1) as i32, (q >> 2) as i32);
                        let k = (nx - qx).abs()
                            + nx.abs()
                            + (ny - qy).abs()
                            + ny.abs()
                            + (nz - qz).abs()
                            + nz.abs();
                        if k > ORDER {
                            continue;
                        }
                        let img = [
                            (1 - 2 * qx) as f32 * self.src[0] + 2.0 * nx as f32 * self.dims[0],
                            (1 - 2 * qy) as f32 * self.src[1] + 2.0 * ny as f32 * self.dims[1],
                            (1 - 2 * qz) as f32 * self.src[2] + 2.0 * nz as f32 * self.dims[2],
                        ];
                        let r = (0..3)
                            .map(|a| (img[a] - ear[a]).powi(2))
                            .sum::<f32>()
                            .sqrt()
                            .max(0.05);
                        out.push(Tap {
                            k: k as u32,
                            t: (r / C - t0).max(0.0),
                            amp: beta.powi(k) * r_ref / r,
                        });
                    }
                }
            }
        }
        out
    }

    /// (taps of the left ear, taps of the right ear), times measured from
    /// the earlier ear's direct sound.
    fn ears(&self) -> [Vec<Tap>; 2] {
        let r_ref = self.distance();
        let t0 = r_ref / C;
        [self.taps(-1.0, t0, r_ref), self.taps(1.0, t0, r_ref)]
    }
}

/// Early reflections (no direct sound) of `mono_in` as a stereo signal, and
/// the time (samples, before `pre`) of the last tap. Sparse taps summed
/// straight into the output: no allocation beyond the output.
fn early_reflections(
    mono_in: &[f32],
    pre: usize,
    room: &Shoebox,
    sr: f32,
) -> (Vec<[f32; 2]>, usize, usize) {
    let n = mono_in.len();
    let mut er = vec![[0.0f32; 2]; n];
    let (mut first, mut last) = (usize::MAX, 0usize);
    for (ch, taps) in room.ears().iter().enumerate() {
        for tap in taps.iter().filter(|t| t.k > 0) {
            let t = (tap.t * sr).round() as usize;
            first = first.min(t);
            last = last.max(t);
            let d = pre.saturating_add(t);
            for i in d..n {
                er[i][ch] += tap.amp * mono_in[i - d];
            }
        }
    }
    (er, if first == usize::MAX { 0 } else { first }, last)
}

/// `reverb` model room: the shoebox's early reflections, then the fdn.
pub(crate) fn room_wet(
    mono_in: &[f32],
    pre: usize,
    size: f32,
    decay: f32,
    damp: f32,
    er_mix: f32,
    sr: f32,
) -> Vec<[f32; 2]> {
    let longest = 3.0 + 17.0 * size;
    // The wall absorption that gives `decay` by Sabine's formula, so the
    // reflections and the tail agree on how fast the room rings out.
    let base = Shoebox::new(longest, 0.5);
    let alpha = (0.161 * base.volume() / (base.surface() * decay)).clamp(0.01, 0.95);
    let room = Shoebox::new(longest, alpha);
    let (er, _first, last) = early_reflections(mono_in, pre, &room, sr);
    // The tail starts halfway through the reflections.
    let tail = fdn_tail(mono_in, pre.saturating_add(last / 2), size, decay, damp, sr);
    const ER_GAIN: f32 = 0.25;
    tail.iter()
        .zip(&er)
        .map(|(t, e)| {
            [
                t[0] * (1.0 - er_mix) + e[0] * er_mix * ER_GAIN,
                t[1] * (1.0 - er_mix) + e[1] * er_mix * ER_GAIN,
            ]
        })
        .collect()
}

// ------------------------------------------------------------------ plate

/// A Schroeder allpass (unit gain at every frequency): diffuses an impulse.
struct Diffuser {
    line: DelayLine,
    len: usize,
    g: f32,
}

impl Diffuser {
    fn new(ms: f32, g: f32, sr: f32) -> Diffuser {
        let len = ((ms * 0.001 * sr) as usize).max(2);
        Diffuser {
            line: DelayLine::new(len + 2),
            len,
            g,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let back = self.line.read((self.len - 1) as f32);
        let v = x - self.g * back;
        self.line.write(v);
        self.g * v + back
    }
}

/// `reverb` model plate: four input diffusers into eight short lines.
pub(crate) fn plate_wet(
    mono_in: &[f32],
    pre: usize,
    size: f32,
    decay: f32,
    damp: f32,
    sr: f32,
) -> Vec<[f32; 2]> {
    // Line lengths (ms): shorter than the fdn's, so echoes pile up sooner.
    const BASE: [f32; 8] = [11.3, 13.9, 17.1, 19.7, 23.3, 27.1, 29.9, 31.7];
    let scale = 0.6 + 0.8 * size;
    let lens: Vec<usize> = BASE
        .iter()
        .map(|ms| ((ms * scale * 0.001 * sr) as usize).max(8))
        .collect();
    let gains: Vec<f32> = lens
        .iter()
        .map(|l| 10f32.powf(-3.0 * *l as f32 / (decay * sr)))
        .collect();
    let mut lines: Vec<DelayLine> = lens.iter().map(|l| DelayLine::new(*l + 2)).collect();
    let mut diff = [
        Diffuser::new(4.7, 0.75, sr),
        Diffuser::new(3.6, 0.75, sr),
        Diffuser::new(12.7, 0.625, sr),
        Diffuser::new(9.3, 0.625, sr),
    ];
    let mut lp = [0.0f32; 8];
    let n = mono_in.len();
    let mut wet = vec![[0.0f32; 2]; n];
    for i in 0..n {
        let mut x = if i >= pre { mono_in[i - pre] } else { 0.0 };
        for d in &mut diff {
            x = d.tick(x);
        }
        let mut v = [0.0f32; 8];
        for k in 0..8 {
            let back = lines[k].read((lens[k] - 1) as f32);
            lp[k] = flush(back + damp * (lp[k] - back));
            v[k] = lp[k] * gains[k];
        }
        let mut h = 1;
        while h < 8 {
            for j in (0..8).step_by(h * 2) {
                for k in j..j + h {
                    let (a, b) = (v[k], v[k + h]);
                    v[k] = a + b;
                    v[k + h] = a - b;
                }
            }
            h *= 2;
        }
        let norm = 1.0 / 8f32.sqrt();
        for k in 0..8 {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            lines[k].write(v[k] * norm + sign * x * 0.5);
        }
        let l = lp[0] + lp[2] + lp[4] + lp[6];
        let r = lp[1] + lp[3] + lp[5] + lp[7];
        wet[i] = [l * 0.5, r * 0.5];
    }
    wet
}

// ----------------------------------------------------------------- spring

/// Second-order allpass sections in series, all tuned alike: a group delay
/// that peaks near `hz`, so one pass spreads an impulse into a chirp.
struct Dispersion {
    r: f64,
    theta: f64,
    a1: f64,
    /// Direct form II transposed states, two per section.
    s: Vec<[f64; 2]>,
}

impl Dispersion {
    /// `sections` allpasses with poles at radius `r`, angle 2 pi hz / sr:
    /// (r^2 + a1 z^-1 + z^-2) / (1 + a1 z^-1 + r^2 z^-2).
    fn new(sections: usize, hz: f32, r: f32, sr: f32) -> Dispersion {
        let theta = f64::from(TAU * hz / sr);
        let r = f64::from(r);
        Dispersion {
            r,
            theta,
            a1: -2.0 * r * theta.cos(),
            s: vec![[0.0; 2]; sections],
        }
    }

    /// Group delay of the whole chain at `hz`, samples. Each conjugate pole
    /// at r e^(+-j theta) adds (1 - r^2) / (1 - 2 r cos(w -+ theta) + r^2).
    fn delay_at(&self, hz: f32, sr: f32) -> f32 {
        let w = f64::from(TAU * hz / sr);
        let one =
            |d: f64| (1.0 - self.r * self.r) / (1.0 - 2.0 * self.r * d.cos() + self.r * self.r);
        (self.s.len() as f64 * (one(w - self.theta) + one(w + self.theta))) as f32
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut v = f64::from(x);
        let r2 = self.r * self.r;
        for st in &mut self.s {
            let y = r2 * v + st[0];
            st[0] = self.a1 * v - self.a1 * y + st[1];
            st[1] = v - r2 * y;
            v = y;
        }
        v as f32
    }
}

/// Sections in the spring's dispersion chain.
const SPRING_SECTIONS: usize = 40;
/// Pole radius of each section.
const SPRING_R: f32 = 0.8;
/// Centre of the chain's group-delay peak, Hz.
const SPRING_HZ: f32 = 4000.0;

/// `reverb` model spring: a loop of delay, dispersion chain and damping.
pub(crate) fn spring_wet(
    mono_in: &[f32],
    pre: usize,
    size: f32,
    decay: f32,
    damp: f32,
    sr: f32,
) -> Vec<[f32; 2]> {
    let len = (((0.020 + 0.060 * size) * sr) as usize).max(8);
    let mut chain = Dispersion::new(SPRING_SECTIONS, SPRING_HZ, SPRING_R, sr);
    // The second channel reads the loop through a differently tuned chain.
    let mut side = Dispersion::new(SPRING_SECTIONS, SPRING_HZ * 1.15, SPRING_R, sr);
    // One round trip is the line plus the chain's delay near the middle of
    // the spectrum; the loop gain makes that round trip lose 60 dB per
    // `decay` seconds.
    let trip = len as f32 + chain.delay_at(1500.0, sr);
    let g = 10f32.powf(-3.0 * trip / (decay * sr)).min(0.999);
    let mut line = DelayLine::new(len + 2);
    let mut lp = 0.0f32;
    let n = mono_in.len();
    let mut wet = vec![[0.0f32; 2]; n];
    for i in 0..n {
        let x = if i >= pre { mono_in[i - pre] } else { 0.0 };
        let back = line.read((len - 1) as f32);
        let c = chain.tick(back);
        lp = flush(c + damp * (lp - c));
        line.write(x + g * lp);
        wet[i] = [lp, side.tick(back)];
    }
    wet
}

// -------------------------------------------------------------------- ir

/// What the impulse response models.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IrModel {
    /// A shoebox room: image-source early reflections, then decaying noise
    /// with Sabine's T60. Stereo (two ears).
    #[default]
    Room,
    /// A pipe open at both ends: a comb of modes at k c / (2 size).
    Tube,
    /// An enclosure: three combs, one per axis (sides 1 : 0.8 : 0.4).
    Box,
    /// A speaker cabinet: 80 Hz to 5 kHz with a low resonance.
    Cabinet,
    /// A telephone line: 250 Hz to 3.8 kHz, six poles each side.
    Phone,
}

/// An impulse response for `convolve`: a source node, mono (stereo for
/// `room`), with the direct sound at sample 0.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ir {
    /// What to model: room (default), tube, box, cabinet, phone.
    #[serde(default)]
    pub model: IrModel,
    /// Metres, 0.05 to 100. room: the longest side (default 6; the others
    /// are 0.8x and 0.4x); tube: its length (default 1); box: the longest
    /// side (default 0.5); cabinet: the cabinet's width, which sets the low
    /// resonance at 70 / size Hz (default 0.6). An error for phone.
    #[serde(default)]
    pub size: Option<f32>,
    /// Mean absorption, 0.005 to 1 (default: room 0.3, tube 0.1, box 0.3).
    /// room: the walls (Sabine T60 = 0.161 V / (S absorption)); tube and
    /// box: the share of pressure lost at each end per round trip. An error
    /// for cabinet and phone.
    #[serde(default)]
    pub absorption: Option<f32>,
    /// Seconds of response, 0.01 to 30 (default: room 1.2 x T60 up to 10;
    /// tube and box until -60 dB up to 5; cabinet 0.1; phone 0.05). A
    /// response is also cut at the document's length.
    #[serde(default)]
    pub length: Option<f32>,
    /// Which noise the room's tail uses (default 0); mixed with the
    /// document seed.
    #[serde(default)]
    pub seed: Option<u64>,
}

impl Describe for Ir {
    const BLURB: &'static str = "impulse response for convolve: room (image sources + Sabine tail), tube, box, cabinet, phone";
    fn example() -> serde_json::Value {
        serde_json::json!({"model": "room", "size": 6.0, "absorption": 0.3})
    }
}

/// A comb loop: an impulse at 0, then repeats every `trip` samples, each
/// `rho` times the last and darkened by a one-pole low-pass (pole `lp`).
fn comb_response(trip: f32, rho: f32, lp: f32, len: usize) -> Vec<f32> {
    let mut line = DelayLine::new(trip.ceil() as usize + 2);
    let mut state = 0.0f32;
    let mut out = vec![0.0f32; len];
    for (i, o) in out.iter_mut().enumerate() {
        // The low-pass delays low frequencies by lp / (1 - lp) samples;
        // take that out of the line so the modes sit at k c / (2 L).
        let back = line.read((trip - 1.0 - lp / (1.0 - lp)).max(0.0));
        state = flush(back + lp * (state - back));
        let y = if i == 0 { 1.0 } else { 0.0 } + rho * state;
        line.write(y);
        *o = y;
    }
    out
}

/// A series of RBJ biquads run on a unit impulse.
fn impulse_through(stages: &[(FilterMode, f32, f32, f32)], len: usize, sr: f32) -> Vec<f32> {
    let mut bq: Vec<Biquad> = stages
        .iter()
        .map(|(m, hz, q, g)| {
            let mut b = Biquad::new();
            b.set(*m, *hz, *q, *g, sr);
            b
        })
        .collect();
    (0..len)
        .map(|i| {
            let mut v = if i == 0 { 1.0 } else { 0.0 };
            for b in &mut bq {
                v = b.tick(v);
            }
            v
        })
        .collect()
}

/// The Q of each biquad in a Butterworth cascade of order 6.
const BUTTER6: [f32; 3] = [0.5176, std::f32::consts::FRAC_1_SQRT_2, 1.9319];

impl Ir {
    fn range(&self, field: &str, v: Option<f32>, lo: f32, hi: f32) -> Result<(), String> {
        if let Some(x) = v
            && !(x.is_finite() && x >= lo && x <= hi)
        {
            return Err(format!("{field}: {x}: give a number from {lo} to {hi}"));
        }
        Ok(())
    }

    /// Checks every field against the model.
    fn check(&self) -> Result<(), String> {
        self.range("size", self.size, 0.05, 100.0)?;
        self.range("absorption", self.absorption, 0.005, 1.0)?;
        self.range("length", self.length, 0.01, 30.0)?;
        if self.size.is_some() && self.model == IrModel::Phone {
            return Err(format!(
                "size: {}: a phone has no size: remove it",
                self.size.unwrap_or_default()
            ));
        }
        if self.absorption.is_some() && matches!(self.model, IrModel::Cabinet | IrModel::Phone) {
            return Err(format!(
                "absorption: {}: only room, tube and box use it: remove it",
                self.absorption.unwrap_or_default()
            ));
        }
        Ok(())
    }

    fn room(&self, cx: &Cx) -> (Vec<Vec<f32>>, f32) {
        let room = Shoebox::new(
            self.size.unwrap_or(6.0),
            self.absorption.unwrap_or(0.3).clamp(0.005, 1.0),
        );
        let t60 = room.t60();
        let len = self
            .length
            .unwrap_or((1.2 * t60).clamp(0.01, 10.0))
            .min(cx.n as f32 / cx.sr);
        let len = ((len * cx.sr) as usize).min(cx.n);
        let ears = room.ears();
        let r0 = room.distance();
        // The reverberant energy against the direct sound's: the direct
        // sound at distance r0 equals the diffuse field at the critical
        // distance sqrt(A / 16 pi), A = 0.161 V / T60, so the field carries
        // (r0 / critical)^2 = 16 pi r0^2 / A of the direct energy. The
        // reflections already hold part of it; the tail gets the rest (at
        // least a quarter).
        let e_rev = 16.0 * PI * r0 * r0 * t60 / (0.161 * room.volume());
        let e_er: f32 = ears[0]
            .iter()
            .filter(|t| t.k > 0)
            .map(|t| t.amp * t.amp)
            .sum();
        let e_late = (e_rev - e_er).max(0.25 * e_rev);
        let tau = t60 / (3.0 * std::f32::consts::LN_10);
        let a0 = (2.0 * e_late / (cx.sr * tau)).sqrt();
        let mut rng = cx.rng(self.seed.unwrap_or(0));
        let (mut first, mut last) = (f32::MAX, 0.0f32);
        for t in ears[0].iter().filter(|t| t.k > 0) {
            first = first.min(t.t);
            last = last.max(t.t);
        }
        let mut out = Vec::new();
        for taps in &ears {
            let mut h = vec![0.0f32; len];
            for tap in taps {
                let i = (tap.t * cx.sr).round() as usize;
                if i < len {
                    h[i] += tap.amp;
                }
            }
            // The tail fades in across the reflections, with a low-pass
            // that darkens over time; the variance is normalised so the
            // envelope alone sets the level.
            let mut state = 0.0f32;
            for (i, v) in h.iter_mut().enumerate() {
                let t = i as f32 / cx.sr;
                let fade = ((t - first) / (last - first).max(1e-4)).clamp(0.0, 1.0);
                let fc = (14_000.0 * 0.35f32.powf(t / t60)).clamp(20.0, 0.45 * cx.sr);
                let a = 1.0 - (-TAU * fc / cx.sr).exp();
                let white = rng.signed() * 3f32.sqrt();
                state = flush(state + a * (white - state));
                let norm = (a / (2.0 - a)).sqrt();
                *v += a0 * (-t / tau).exp() * fade * state / norm;
            }
            out.push(h);
        }
        (out, t60)
    }

    fn tube_or_box(&self, cx: &Cx) -> Vec<f32> {
        let tube = self.model == IrModel::Tube;
        let size = self.size.unwrap_or(if tube { 1.0 } else { 0.5 });
        let absorb = self.absorption.unwrap_or(if tube { 0.1 } else { 0.3 });
        let rho = (1.0 - absorb).clamp(0.0, 0.9999);
        // Seconds for the pressure to fall 60 dB: 6.9 round trips per ln.
        let axes: Vec<f32> = if tube {
            vec![size]
        } else {
            vec![size, size * 0.8, size * 0.4]
        };
        let shortest = axes.iter().copied().fold(f32::MAX, f32::min);
        let t60 = if rho > 0.0 {
            (6.908 * 2.0 * shortest / C / -rho.ln()).min(60.0)
        } else {
            0.01
        };
        let len = self.length.unwrap_or(t60.min(5.0)).min(cx.n as f32 / cx.sr);
        let len = ((len * cx.sr) as usize).min(cx.n);
        let mut h = vec![0.0f32; len];
        let scale = 1.0 / axes.len() as f32;
        for l in &axes {
            let trip = (2.0 * l / C * cx.sr).max(2.0);
            let c = comb_response(trip, rho, 0.25, len);
            for (o, v) in h.iter_mut().zip(c) {
                *o += v * scale;
            }
        }
        // All the combs share the direct sound at sample 0, which adds up
        // to the unit impulse (each contributes `scale`).
        h
    }

    fn filtered(&self, cx: &Cx) -> Vec<f32> {
        use FilterMode::{Hp, Lp, Peak};
        let cabinet = self.model == IrModel::Cabinet;
        let len = self.length.unwrap_or(if cabinet { 0.1 } else { 0.05 });
        let len = ((len.min(cx.n as f32 / cx.sr)) * cx.sr) as usize;
        let mut stages = Vec::new();
        if cabinet {
            let f_res = 70.0 / self.size.unwrap_or(0.6);
            stages.push((Hp, 80.0, std::f32::consts::FRAC_1_SQRT_2, 0.0));
            stages.push((Peak, f_res, 2.5, 6.0));
            stages.push((Lp, 5000.0, std::f32::consts::FRAC_1_SQRT_2, 0.0));
            stages.push((Lp, 5000.0, std::f32::consts::FRAC_1_SQRT_2, 0.0));
        } else {
            for q in BUTTER6 {
                stages.push((Hp, 250.0, q, 0.0));
                stages.push((Lp, 3800.0, q, 0.0));
            }
        }
        impulse_through(&stages, len, cx.sr)
    }
}

impl Eval for Ir {
    fn random(&self) -> bool {
        self.model == IrModel::Room
    }

    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        self.check()?;
        let buf = match self.model {
            IrModel::Room => {
                let (chans, _) = self.room(cx);
                Buf::from_channels(pad(chans, cx.n))
            }
            IrModel::Tube | IrModel::Box => {
                Buf::from_channels(pad(vec![self.tube_or_box(cx)], cx.n))
            }
            IrModel::Cabinet | IrModel::Phone => {
                Buf::from_channels(pad(vec![self.filtered(cx)], cx.n))
            }
        };
        Ok(vec![("out", buf)])
    }
}

/// Zero-pads every channel to `n` samples (the response is at the start).
fn pad(mut chans: Vec<Vec<f32>>, n: usize) -> Vec<Vec<f32>> {
    for c in &mut chans {
        c.resize(n, 0.0);
    }
    chans
}
