//! `fire`, `wind` and `spark`: three finished-recipe models of the elements
//! (wave 2, card M3). Each takes a few physical fields and runs tuned DSP
//! inside, sample by sample, with no lookahead.
//!
//! Every one can render a **periodic** buffer (`loop`), so a `kind: loop`
//! sound built from it passes `verify` `loop_seam` with no hand tuning:
//!
//! - every internal slow random control (turbulence, gusts) is a lattice with
//!   a whole number of cells in the buffer, indexed modulo that number;
//! - every random impulse that rings past the end is written back to the
//!   start of the buffer;
//! - filters run over the whole buffer twice with the same input and the
//!   second pass is kept, so the state at sample 0 is the state at the end
//!   (exact up to the filters' ring-down, which is milliseconds);
//! - phase accumulators (the wind whistle) get a linear correction of at most
//!   half a cycle over the buffer, so the phase wraps to a whole number of
//!   cycles; the mains envelope of `spark` is retuned to a whole number of
//!   cycles in the buffer (at most `0.5 / duration` Hz off).
//!
//! `loop` unset follows the document: a `kind: loop` document gets the
//! periodic render, a oneshot does not. `loop: false` forces the plain
//! render even in a loop document; `loop: true` forces the periodic render.

use serde::Deserialize;

use super::one;
use crate::dsp::{Biquad, FilterMode, Resonator, Rng, Svf};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// A finite sample clamped to `lo..=hi`, else `default`.
fn sane(x: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if x.is_finite() {
        x.clamp(lo, hi)
    } else {
        default
    }
}

/// Seeded smooth random motion in -1..1: Catmull-Rom through random points
/// `rate` times a second. With `wrap` the buffer holds a whole number of
/// cells (`rate` is rounded to fit, at least one cell) and the lattice is
/// indexed modulo that number, so the value at `n` equals the value at 0.
struct Lattice {
    pts: Vec<f32>,
    per: f64,
    wrap: bool,
}

impl Lattice {
    fn new(rng: &mut Rng, rate: f32, n: usize, sr: f32, wrap: bool) -> Lattice {
        if wrap {
            let k = ((n as f32 / sr * rate).round() as usize).max(1);
            Lattice {
                pts: (0..k).map(|_| rng.signed()).collect(),
                per: n as f64 / k as f64,
                wrap,
            }
        } else {
            let per = f64::from(sr) / f64::from(rate);
            let count = (n as f64 / per).ceil() as usize + 4;
            Lattice {
                pts: (0..count).map(|_| rng.signed()).collect(),
                per,
                wrap,
            }
        }
    }

    fn at(&self, i: usize) -> f32 {
        let x = i as f64 / self.per;
        let k = x.floor() as usize;
        let f = (x - k as f64) as f32;
        let m = self.pts.len();
        let p = |j: usize| {
            if self.wrap {
                self.pts[j % m]
            } else {
                self.pts[j.min(m - 1)]
            }
        };
        let (p0, p1, p2, p3) = (p(k), p(k + 1), p(k + 2), p(k + 3));
        let v = 0.5
            * ((2.0 * p1)
                + (-p0 + p2) * f
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f * f * f);
        v.clamp(-1.0, 1.0)
    }
}

/// Three octaves of lattice motion (rates `r`, 2.7 r, 7.3 r, weights 1, 0.5,
/// 0.25), normalised to about -1..1: fractal turbulence.
struct Fbm([Lattice; 3]);

impl Fbm {
    fn new(rng: &mut Rng, r: f32, n: usize, sr: f32, wrap: bool) -> Fbm {
        Fbm([
            Lattice::new(rng, r, n, sr, wrap),
            Lattice::new(rng, r * 2.7, n, sr, wrap),
            Lattice::new(rng, r * 7.3, n, sr, wrap),
        ])
    }

    fn at(&self, i: usize) -> f32 {
        (self.0[0].at(i) + 0.5 * self.0[1].at(i) + 0.25 * self.0[2].at(i)) / 1.75
    }
}

/// Resolves the `loop` field against the document.
fn wraps(field: Option<bool>, cx: &Cx) -> bool {
    field.unwrap_or(cx.looped)
}

/// A seeded white-noise buffer.
fn white(rng: &mut Rng, n: usize) -> Vec<f32> {
    (0..n).map(|_| rng.signed()).collect()
}

/// A stream of its own, split from `rng` (draw order fixes the result).
fn split(rng: &mut Rng) -> Rng {
    Rng::new(rng.next_u64())
}

// ---------------------------------------------------------------------- fire

/// Fire: a roar, a hiss and crackle pops under one shared control.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fire {
    /// Flame size, metres across, 0.02..10 (a candle is 0.03, a campfire 0.6,
    /// a bonfire 3). Bigger: the roar moves down in pitch and rises in
    /// level, and the pops get lower.
    pub size: f32,
    /// How fiercely it burns, 0..1 (default 0.5). A number or a signal
    /// (clamped to 0..1 per sample). Raises the roar, the hiss (faster) and
    /// the pop rate together.
    #[serde(default)]
    pub intensity: Option<In>,
    /// How much it pops, 0..1 (default 0.5): scales the pop rate; 0 is no
    /// pops at all. A number.
    #[serde(default)]
    pub crackle: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise. Turbulence is rounded to a whole number of
    /// cells in the duration.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Fire {
    const BLURB: &'static str = "fire: low roar + hiss + crackle pops (power-law sizes) under one shared control; size, intensity, crackle; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 0.8, "intensity": 0.5, "crackle": 0.5, "seed": 1})
    }
}

/// The fire roar's lowest and highest low-pass, Hz.
const ROAR_MIN_HZ: f32 = 40.0;
const ROAR_MAX_HZ: f32 = 1500.0;
/// Hiss high-pass corner, Hz.
const HISS_HZ: f32 = 2500.0;

impl Eval for Fire {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.looped != Some(true)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if !(self.size.is_finite() && (0.02..=10.0).contains(&self.size)) {
            return Err(format!(
                "size: {}: give metres from 0.02 to 10 (candle 0.03, campfire 0.6, bonfire 3)",
                self.size
            ));
        }
        let intensity = cx.mono_or("intensity", &self.intensity, 0.5)?;
        if let Some(v) = intensity.constant()
            && !(0.0..=1.0).contains(&v)
        {
            return Err(format!("intensity: {v}: give a number from 0 to 1"));
        }
        let crackle = self.crackle.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&crackle) {
            return Err(format!("crackle: {crackle}: give a number from 0 to 1"));
        }
        let (n, sr) = (cx.n, cx.sr);
        let wrap = wraps(self.looped, cx);
        let mut rng = cx.rng(self.seed);
        let (mut r_roar, mut r_hiss, mut r_pop, mut r_lat) = (
            split(&mut rng),
            split(&mut rng),
            split(&mut rng),
            split(&mut rng),
        );
        let roar_n = white(&mut r_roar, n);
        let hiss_n = white(&mut r_hiss, n);
        let slow = Fbm::new(&mut r_lat, 1.3, n, sr, wrap);
        let fast = Lattice::new(&mut r_lat, 11.0, n, sr, wrap);

        // The shared control: intensity and the slow turbulence, per sample.
        let drive: Vec<f32> = (0..n)
            .map(|i| sane(intensity.at(i), 0.0, 1.0, 0.5))
            .collect();
        let turb: Vec<f32> = (0..n).map(|i| slow.at(i)).collect();

        // Pops: a Poisson process whose rate follows the drive and the
        // turbulence, sizes a power law (a Pareto with exponent 1, capped),
        // each a short two-pole ring at a random frequency. A pop that rings
        // past the end is written to the start when the buffer wraps.
        let mut pops = vec![0.0f32; n];
        let pitch_scale = (1.0 / self.size).powf(0.15);
        for i in 0..n {
            let d = drive[i];
            let rate = 2.0 * crackle * (3.0 + 90.0 * d * d) * (0.6 + 0.4 * (1.0 + turb[i]));
            if r_pop.unit() >= rate / sr {
                continue;
            }
            let h = (0.1 / r_pop.unit().max(1e-4)).min(1.5);
            let hz = 900.0 * (5500.0f32 / 900.0).powf(r_pop.unit()) * pitch_scale;
            let sign = if r_pop.unit() < 0.5 { -1.0 } else { 1.0 };
            let t60 = 0.002 + 0.006 * h;
            let mut ring = Resonator::new(hz, t60, 0.25 * h * sign * (0.4 + 1.2 * d), sr);
            let life = (t60 * sr) as usize;
            for j in 0..life {
                let y = ring.tick(if j == 0 { 1.0 } else { 0.0 });
                let at = i + j;
                if at < n {
                    pops[at] += y;
                } else if wrap {
                    pops[at % n] += y;
                }
            }
        }

        // Roar: low-passed noise (24 dB an octave) at a corner that falls
        // with size, level held independent of the corner and rising with
        // size and drive, with the turbulence as slow amplitude modulation.
        let roar_hz = (260.0 / self.size.sqrt()).clamp(ROAR_MIN_HZ, ROAR_MAX_HZ);
        let roar_amp = 0.22 * (24000.0 / roar_hz).sqrt() * 0.5 * self.size.powf(0.35);
        let (mut lp1, mut lp2, mut hp1, mut hp2) =
            (Biquad::new(), Biquad::new(), Biquad::new(), Biquad::new());
        lp1.set(FilterMode::Lp, roar_hz, 0.6, 0.0, sr);
        lp2.set(FilterMode::Lp, roar_hz, 0.6, 0.0, sr);
        hp1.set(FilterMode::Hp, HISS_HZ, 0.7, 0.0, sr);
        hp2.set(FilterMode::Hp, HISS_HZ, 0.7, 0.0, sr);
        let mut out = vec![0.0f32; n];
        for _pass in 0..if wrap { 2 } else { 1 } {
            for i in 0..n {
                let d = drive[i];
                let am = (1.0 + 0.55 * turb[i]).max(0.05);
                let roar = lp2.tick(lp1.tick(roar_n[i])) * roar_amp * (0.25 + d) * am;
                let hiss = hp2.tick(hp1.tick(hiss_n[i])) * 0.08 * d * d * (1.0 + 0.4 * fast.at(i));
                out[i] = roar + hiss;
            }
        }
        for (o, p) in out.iter_mut().zip(&pops) {
            *o += p;
        }
        Ok(one(crate::buf::Buf::Audio(out)))
    }
}

// ---------------------------------------------------------------------- wind

/// Wind: noise through five resonant bands that follow the wind speed.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Wind {
    /// Wind speed, m/s, 0..60. A number or a signal (clamped to 0..60 per
    /// sample). 0 is silence. The five band centres rise and the upper bands
    /// gain more than the lower ones as it grows.
    pub speed: In,
    /// Gustiness, 0..1 (default 0.3): how far the speed swings above and
    /// below `speed` (up to +-60 % at 1) with slow fractal turbulence.
    #[serde(default)]
    pub gust: Option<f32>,
    /// Aeolian tone level, 0..1 (default 0 = none): a sine at
    /// `0.2 * speed / diameter` Hz that follows the (gusting) speed.
    #[serde(default)]
    pub whistle: Option<f32>,
    /// Diameter of the object the wind blows past, metres, 0.0005..0.1
    /// (default 0.004, a wire): sets the whistle pitch.
    #[serde(default)]
    pub diameter: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise. Gust cells are rounded to a whole number in
    /// the duration; the whistle phase is nudged by up to half a cycle over
    /// the whole buffer.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Wind {
    const BLURB: &'static str = "wind: noise through 5 resonant bands following speed (m/s, a signal) and fractal gusts; optional Aeolian whistle at 0.2 * speed / diameter; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 12, "gust": 0.3, "whistle": 0.0, "seed": 1})
    }
}

/// The five bands as (base centre Hz, Q, weight). A band sits at its base
/// centre times `0.6 + 0.8 * (speed / 20)^0.8`, so at 20 m/s it is 1.4 times
/// the base.
const WIND_BANDS: [(f32, f32, f32); 5] = [
    (120.0, 2.5, 1.0),
    (280.0, 3.0, 0.8),
    (600.0, 3.5, 0.6),
    (1200.0, 4.0, 0.4),
    (2400.0, 4.5, 0.25),
];

impl Eval for Wind {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.looped != Some(true)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono("speed", &self.speed)?;
        if let Some(v) = speed.constant()
            && !(0.0..=60.0).contains(&v)
        {
            return Err(format!("speed: {v}: give metres a second from 0 to 60"));
        }
        let gust = self.gust.unwrap_or(0.3);
        if !(0.0..=1.0).contains(&gust) {
            return Err(format!("gust: {gust}: give a number from 0 to 1"));
        }
        let whistle = self.whistle.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&whistle) {
            return Err(format!("whistle: {whistle}: give a number from 0 to 1"));
        }
        let diameter = self.diameter.unwrap_or(0.004);
        if !(0.0005..=0.1).contains(&diameter) {
            return Err(format!(
                "diameter: {diameter}: give metres from 0.0005 to 0.1 (a wire is 0.004)"
            ));
        }
        let (n, sr) = (cx.n, cx.sr);
        let wrap = wraps(self.looped, cx);
        let mut rng = cx.rng(self.seed);
        let (mut r_noise, mut r_lat) = (split(&mut rng), split(&mut rng));
        let noise = white(&mut r_noise, n);
        let gusts = Fbm::new(&mut r_lat, 0.4, n, sr, wrap);
        let waver = Lattice::new(&mut r_lat, 3.0, n, sr, wrap);

        // The effective speed: the field, swung by the gusts.
        let vel: Vec<f32> = (0..n)
            .map(|i| {
                let v = sane(speed.at(i), 0.0, 60.0, 0.0);
                (v * (1.0 + 0.6 * gust * gusts.at(i))).max(0.0)
            })
            .collect();

        let mut bands: Vec<Svf> = (0..WIND_BANDS.len()).map(|_| Svf::default()).collect();
        let mut out = vec![0.0f32; n];
        for _pass in 0..if wrap { 2 } else { 1 } {
            for i in 0..n {
                // Above 50 m/s the levels stop growing (the centres still rise).
                let s = vel[i] / 20.0;
                let sg = s.min(2.5);
                let mut y = 0.0;
                if s > 0.0 {
                    let shift = 0.6 + 0.8 * s.powf(0.8);
                    for (k, (base, q, w)) in WIND_BANDS.iter().enumerate() {
                        let hz = (base * shift).min(0.45 * sr);
                        // Peak gain of this bandpass is q: divide it out,
                        // then scale to equal power per band.
                        let norm =
                            (3.0 * 0.5 * sr * q / (std::f32::consts::FRAC_PI_2 * hz)).sqrt() / q;
                        let gain = w * sg.powf(0.8 + 0.3 * k as f32) * 0.06 * norm;
                        y += bands[k].tick(noise[i], FilterMode::Bp, hz, *q, 0.0, sr) * gain;
                    }
                }
                out[i] = y;
            }
        }

        if whistle > 0.0 {
            // A sine at 0.2 * v / d; the phase is accumulated in f64. A
            // wrapping buffer shifts it by the fraction of a cycle it
            // would otherwise end short of, spread evenly.
            let mut phase = vec![0.0f64; n];
            let mut acc = 0.0f64;
            for i in 0..n {
                phase[i] = acc;
                let f = (0.2 * vel[i] / diameter).min(0.45 * sr);
                acc += f64::from(f / sr);
            }
            let fix = if wrap { acc.round() - acc } else { 0.0 };
            for i in 0..n {
                let s = vel[i] / 20.0;
                let ph = phase[i] + fix * i as f64 / n as f64;
                let am = 1.0 + 0.3 * waver.at(i);
                out[i] += (std::f64::consts::TAU * ph).sin() as f32
                    * whistle
                    * 0.12
                    * s.min(2.5).powi(2)
                    * am;
            }
        }
        Ok(one(crate::buf::Buf::Audio(out)))
    }
}

// --------------------------------------------------------------------- spark

/// Spark: a mains-powered arc that keeps re-striking.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spark {
    /// Strikes a second, 0.1..200: how often a new arc starts. Each strike
    /// is a snap, then 20 to 70 ms of dense re-striking impulses that the
    /// mains envelope gates.
    pub rate: f32,
    /// Mains frequency, Hz: 50 or 60 (default 50). The arc is gated by a
    /// rectified-sine envelope, so the modulation is strongest at twice this
    /// and an unequal half-cycle puts a weaker line at this.
    #[serde(default)]
    pub buzz: Option<f32>,
    /// The short resonator's centre, Hz, 200..12000 (default 3000); a second
    /// resonance sits at 1.9 times it (when under 0.45 times the sample
    /// rate).
    #[serde(default)]
    pub body: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise. The mains frequency is retuned to a whole
    /// number of cycles in the duration (at most 0.5 / duration Hz off).
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Spark {
    const BLURB: &'static str = "electric spark / arc: sparse strikes of re-striking impulses gated at the mains rate (50 or 60 Hz) into a short resonator; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 6, "buzz": 50, "body": 3000, "seed": 1})
    }
}

/// Re-striking impulses a second inside one strike.
const SPARK_DENSITY: f32 = 3000.0;

impl Eval for Spark {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.looped != Some(true)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if !(self.rate.is_finite() && (0.1..=200.0).contains(&self.rate)) {
            return Err(format!(
                "rate: {}: give strikes a second from 0.1 to 200",
                self.rate
            ));
        }
        let buzz = self.buzz.unwrap_or(50.0);
        if buzz != 50.0 && buzz != 60.0 {
            return Err(format!(
                "buzz: {buzz}: give 50 or 60 (the mains frequency in Hz)"
            ));
        }
        let body = self.body.unwrap_or(3000.0);
        if !(body.is_finite() && (200.0..=12000.0).contains(&body)) {
            return Err(format!(
                "body: {body}: give a frequency from 200 to 12000 Hz"
            ));
        }
        let (n, sr) = (cx.n, cx.sr);
        let wrap = wraps(self.looped, cx);
        let mut rng = cx.rng(self.seed);
        // A whole number of mains cycles in a wrapping buffer.
        let secs = n as f32 / sr;
        let mains = if wrap {
            ((buzz * secs).round().max(1.0)) / secs
        } else {
            buzz
        };
        let gate = |i: usize| -> f32 {
            let cycles = f64::from(mains) * i as f64 / f64::from(sr);
            let half = (cycles * 2.0).floor() as i64;
            let s = (std::f64::consts::TAU * cycles).sin().abs() as f32;
            s.powf(1.5) * if half % 2 == 1 { 0.75 } else { 1.0 }
        };

        let mut exc = vec![0.0f32; n];
        let mut put = |at: usize, v: f32| {
            if at < n {
                exc[at] += v;
            } else if wrap {
                exc[at % n] += v;
            }
        };
        // Poisson strike times; a wrapping buffer takes one strike in each
        // 1 / rate cell instead (a jittered grid), so a short loop's head and
        // tail hold about the same number of strikes.
        let mut cell = 0usize;
        let mut next = |rng: &mut Rng, prev: f32| -> f32 {
            if wrap {
                let t = (cell as f32 + rng.unit()) / self.rate;
                cell += 1;
                t
            } else {
                prev + rng.exponential(self.rate)
            }
        };
        let mut t = if wrap {
            next(&mut rng, 0.0)
        } else {
            rng.exponential(self.rate)
        };
        while t < secs {
            let i0 = (t * sr) as usize;
            let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
            put(i0, sign);
            let dur = 0.02 + 0.05 * rng.unit();
            let count = (dur * SPARK_DENSITY) as usize;
            for _ in 0..count {
                let at = i0 + (rng.unit() * dur * sr) as usize;
                let h = (0.2 + 0.8 * rng.unit()) * if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                // Gate by the mains envelope at the (unwrapped) time.
                let g = if at < n { gate(at) } else { gate(at % n) };
                put(at, h * g);
            }
            t = next(&mut rng, t);
        }

        let mut r1 = Resonator::new(body, 0.003, 1.0, sr);
        let hi = body * 1.9;
        let mut r2 = (hi < 0.45 * sr).then(|| Resonator::new(hi, 0.002, 0.5, sr));
        let mut out = vec![0.0f32; n];
        for _pass in 0..if wrap { 2 } else { 1 } {
            for i in 0..n {
                let x = exc[i];
                let mut y = r1.tick(x);
                if let Some(r) = r2.as_mut() {
                    y += r.tick(x);
                }
                out[i] = 0.15 * y + 0.03 * x;
            }
        }
        Ok(one(crate::buf::Buf::Audio(out)))
    }
}
