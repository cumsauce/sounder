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
use crate::dsp::{Biquad, FilterMode, OnePole, Rng, Svf};
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
    const BLURB: &'static str = "fire: fluttering low roar + a little hiss + unpitched noise-burst wood snaps (power-law sizes, high-passed) under one shared control; size, intensity, crackle; loop: true wraps without a seam";
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
        // Flame flutter: fast random amplitude motion (two octaves, 23 and
        // 61 cells a second) so the roar flickers instead of sweeping
        // smoothly like wind.
        let flut_a = Lattice::new(&mut r_lat, 23.0, n, sr, wrap);
        let flut_b = Lattice::new(&mut r_lat, 61.0, n, sr, wrap);

        // The shared control: intensity and the slow turbulence, per sample.
        let drive: Vec<f32> = (0..n)
            .map(|i| sane(intensity.at(i), 0.0, 1.0, 0.5))
            .collect();
        let turb: Vec<f32> = (0..n).map(|i| slow.at(i)).collect();

        // Crackles: a Poisson process whose rate follows the drive and the
        // turbulence, sizes a power law (a Pareto with exponent 1, capped).
        // Each is a wood snap, not a ring: one to four sub-clicks within
        // 4 ms, each a 0.2..2.5 ms burst of white noise under a fast
        // exponential fall, so nothing in it has a pitch (the old two-pole
        // rings at 0.9-5.5 kHz read as a gurgling liquid). The snaps are
        // high-passed (two one-pole sections, corner 700 Hz x the size
        // scale) after they are summed. A snap past the end is written to the
        // start when the buffer wraps.
        let mut pops = vec![0.0f32; n];
        let pitch_scale = (1.0 / self.size).powf(0.15);
        for i in 0..n {
            let d = drive[i];
            let rate = 2.0 * crackle * (3.0 + 90.0 * d * d) * (0.6 + 0.4 * (1.0 + turb[i]));
            if r_pop.unit() >= rate / sr {
                continue;
            }
            let h = (0.1 / r_pop.unit().max(1e-4)).min(1.5);
            let level = 0.6 * h * (0.4 + 1.2 * d);
            let sign = if r_pop.unit() < 0.5 { -1.0 } else { 1.0 };
            let subs = 1 + (r_pop.unit() * 4.0 * h.min(1.0).sqrt()) as usize;
            let mut off = 0usize;
            for k in 0..subs {
                let tau = (0.0002 + 0.0023 * r_pop.unit()) / 3.0;
                let len = ((3.0 * tau * sr) as usize).max(2);
                let a = level
                    * if k == 0 {
                        1.0
                    } else {
                        0.3 + 0.5 * r_pop.unit()
                    };
                // Round 2: no full-height first sample (heard as "clicky /
                // underrunny"): a 0.15 ms rise, and each sub-click through its
                // own one-pole low-pass at 2-7 kHz, so snaps vary in colour.
                let lp_hz = 2000.0 + 5000.0 * r_pop.unit();
                let lc = 1.0 - (-std::f32::consts::TAU * lp_hz / sr).exp();
                let rise = 0.00015 * sr;
                let mut ly = 0.0f32;
                let len = len + (0.0004 * sr) as usize;
                for j in 0..len {
                    let x = if j == 0 { sign } else { r_pop.signed() };
                    ly += lc * (x - ly);
                    let att = 1.0 - (-(j as f32 + 1.0) / rise).exp();
                    let y = 2.0 * a * ly * att * (-(j as f32) / (tau * sr)).exp();
                    let at = i + off + j;
                    if at < n {
                        pops[at] += y;
                    } else if wrap {
                        pops[at % n] += y;
                    }
                }
                off += (r_pop.unit() * 0.0013 * sr) as usize + len;
            }
        }
        {
            let corner = (700.0 * pitch_scale).min(0.4 * sr);
            let c = (-std::f32::consts::TAU * corner / sr).exp();
            for _ in 0..2 {
                let (mut px, mut py) = (if wrap { pops[n.saturating_sub(1)] } else { 0.0 }, 0.0f32);
                for v in pops.iter_mut() {
                    let y = c * (py + *v - px);
                    px = *v;
                    py = y;
                    *v = y;
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
        let mut lp3 = Biquad::new();
        lp3.set(FilterMode::Lp, (roar_hz * 3.0).min(0.4 * sr), 0.5, 0.0, sr);
        let mut out = vec![0.0f32; n];
        for _pass in 0..if wrap { 2 } else { 1 } {
            for i in 0..n {
                let d = drive[i];
                let flutter = 1.0 + 0.3 * flut_a.at(i) + 0.2 * flut_b.at(i);
                let am = ((1.0 + 0.5 * turb[i]) * flutter).max(0.05);
                let roar = lp2.tick(lp1.tick(roar_n[i])) * roar_amp * (0.25 + d) * am;
                let hiss = hp2.tick(hp1.tick(hiss_n[i])) * 0.025 * d * d * (1.0 + 0.4 * fast.at(i));
                // Round 3: halfway back (round 2 read "too dark, a distant
                // highway"): body 0.15, hiss 0.025. Round 2: a darker bed (the round-1 bed read bright): a
                // pink-ish low-mid layer, the roar noise through one low-pass
                // at 3x the roar corner, under the same flutter.
                let body = lp3.tick(roar_n[i]) * roar_amp * 0.15 * (0.25 + d) * am;
                out[i] = roar + body + hiss;
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
///
/// Fitted to refs/wind_set (four CC0 wind recordings): the recordings hold
/// 0.01-0.12 % of their energy above 2 kHz and a centroid of 40-520 Hz; the
/// old bands (Q 2.5-4.5, weights 1 / 0.8 / 0.6 / 0.4 / 0.25) put 13 % above
/// 2 kHz, a 1.06 kHz centroid and steady lines at 138 / 350 / 740 Hz that the
/// batch-3 listener heard as "a large truck". The bands are now broad (Q
/// 0.8-1.4), each drifts on its own (+-35 %), and the upper ones are far
/// quieter; a turbulent low rumble (below) carries the weight.
const WIND_BANDS: [(f32, f32, f32); 5] = [
    (120.0, 0.8, 1.0),
    (280.0, 1.0, 0.55),
    (600.0, 1.1, 0.25),
    (1200.0, 1.2, 0.08),
    (2400.0, 1.4, 0.02),
];

/// The rumble: white noise through two one-pole low-passes at
/// `25 + 3 * speed` Hz, scaled by `WIND_RUMBLE * (speed / 20)^1.2` and by a
/// turbulence envelope `(0.5 + 0.5 * fast)^2` (a 6 Hz lattice): the buffeting
/// a real wind recording carries under 100 Hz.
const WIND_RUMBLE: f32 = 3.0;

/// The band sum goes through two one-pole low-passes at `500 + 40 * speed`
/// Hz (fitted to refs/wind_set: the broad bands' skirts otherwise leak 10 %
/// of the energy above 2 kHz).
const WIND_TOP_HZ: (f32, f32) = (500.0, 40.0);

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
        let mut r_fit = split(&mut rng);
        let rumble_noise = white(&mut r_fit, n);
        let turb = Lattice::new(&mut r_fit, 6.0, n, sr, wrap);
        let drift: Vec<Lattice> = (0..WIND_BANDS.len())
            .map(|k| Lattice::new(&mut r_fit, 0.25 + 0.15 * k as f32, n, sr, wrap))
            .collect();
        let mut rum = [OnePole::default(), OnePole::default()];
        let mut top = [OnePole::default(), OnePole::default()];

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
                        let hz = (base * shift * (1.0 + 0.35 * drift[k].at(i))).min(0.45 * sr);
                        // Peak gain of this bandpass is q: divide it out,
                        // then scale to equal power per band.
                        let norm =
                            (3.0 * 0.5 * sr * q / (std::f32::consts::FRAC_PI_2 * hz)).sqrt() / q;
                        let gain = w * sg.powf(0.8 + 0.3 * k as f32) * 0.06 * norm;
                        y += bands[k].tick(noise[i], FilterMode::Bp, hz, *q, 0.0, sr) * gain;
                    }
                }
                let tc = WIND_TOP_HZ.0 + WIND_TOP_HZ.1 * vel[i].min(50.0);
                let (a, _) = top[0].tick(y, tc, sr);
                let (y2, _) = top[1].tick(a, tc, sr);
                let mut y = y2;
                let fc = 25.0 + 3.0 * vel[i].min(50.0);
                let (a, _) = rum[0].tick(rumble_noise[i], fc, sr);
                let (b, _) = rum[1].tick(a, fc, sr);
                let t = 0.5 + 0.5 * turb.at(i);
                y += b * WIND_RUMBLE * sg.powf(1.2) * t * t;
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

/// Spark: a faulty high-voltage transformer, after Farnell (Designing Sound,
/// ch. 39): a hum of two saws at twice the mains +-0.2 Hz (beating at
/// 0.4 Hz), clipped hard, under a slow random level; arcing chirps (one per
/// hum cycle, a reversed phasor squashed by a random factor 3..12, squared
/// and cos'd, high-passed at 300 Hz) switched by a random gate (~3 Hz); and
/// snaps (a noise blast with a quartic 0.5 s fall plus a sine swept 7 kHz to
/// 20 Hz in 10 ms) through a formant bank (4.6, 7.2, 0.48, 0.72 kHz, then a
/// wide 2.5 kHz band); all into a 12 ms comb with feedback 0.2. The earlier
/// model (strikes of impulses into a resonator) was heard as "ripping
/// plastic"; this one was preferred in an A/B (book check 1).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spark {
    /// Snaps a second, 0.1..200 (Poisson): how often a big spark cracks over
    /// the hum and arcing. Each snap lasts about 0.5 s.
    pub rate: f32,
    /// Mains frequency, Hz: 50 or 60 (default 50). The hum and the arcing
    /// run at twice this (100 or 120 Hz).
    #[serde(default)]
    pub buzz: Option<f32>,
    /// Shifts the snap's formant bank, Hz, 200..12000 (default 3000 = the
    /// book's bank: 4.6, 7.2, 0.48, 0.72 kHz and a 2.5 kHz band, all times
    /// body / 3000; bands over 0.45 x the sample rate are dropped).
    #[serde(default)]
    pub body: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam (the last 0.25 s crossfaded
    /// into the start). Unset: yes in a `kind: loop` document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Spark {
    const BLURB: &'static str = "electric spark / arc (Farnell's transformer recipe): a beating 100/120 Hz hum, gated arcing chirps, and snaps through a spark formant bank, in a short comb; rate = snaps a second; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 6, "buzz": 50, "body": 3000, "seed": 1})
    }
}

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
        // A wrapping buffer is made periodic in n (modulators, snaps and the
        // hum's two tones all repeat every n samples), rendered twice over,
        // and the second pass kept, so the filters and the comb start from
        // the end's state.
        let m = if wrap { 2 * n } else { n };
        let secs = n as f32 / sr;
        let tune = |f: f32| {
            if wrap {
                (f * secs).round().max(1.0) / secs
            } else {
                f
            }
        };
        let hum_hz = 2.0 * buzz;
        let (f1, f2) = (tune(hum_hz - 0.2), tune(hum_hz + 0.2));
        let scale = body / 3000.0;

        // Smooth random modulators (cosine-interpolated random points).
        let slow = |rng: &mut Rng, rate: f32, lo: f32, hi: f32| -> Vec<f32> {
            let mut step = (sr / rate).max(1.0);
            let count = if wrap {
                let c = (n as f32 / step).round().max(1.0) as usize;
                step = n as f32 / c as f32;
                c
            } else {
                (m as f32 / step) as usize + 3
            };
            let pts: Vec<f32> = (0..count).map(|_| lo + (hi - lo) * rng.unit()).collect();
            (0..m)
                .map(|i| {
                    let x = i as f32 / step;
                    let j = x as usize;
                    let f = 0.5 - 0.5 * (std::f32::consts::PI * x.fract()).cos();
                    let (a, b) = (pts[j % count], pts[(j + 1) % count]);
                    a + (b - a) * f
                })
                .collect()
        };
        let hum_level = slow(&mut rng, 2.0, 0.0, 1.0);
        let squash = slow(&mut rng, 0.1, 3.0, 12.0);
        let gate_w = slow(&mut rng, 3.0, -1.0, 1.0);

        // Snap excitation: a noise blast with a quartic 0.5 s fall plus a
        // sine swept 7 kHz -> 20 Hz in 10 ms; Poisson at `rate`.
        let mut exc = vec![0.0f32; m];
        // A wrapping buffer takes one snap in each 1 / rate cell (a jittered
        // grid), so a short loop's two ends hold about as many snaps.
        let mut cell = 0usize;
        let mut next = |rng: &mut Rng, prev: f32| -> f32 {
            if wrap {
                cell += 1;
                (cell as f32 - 1.0 + rng.unit()) / self.rate
            } else {
                prev + rng.exponential(self.rate)
            }
        };
        let mut t = next(&mut rng, 0.0);
        let slen = (0.5 * sr) as usize;
        while ((t * sr) as usize) < if wrap { n } else { m } {
            let i0 = (t * sr) as usize;
            let amp = 0.6 + 0.4 * rng.unit();
            let mut ph = 0.0f32;
            let span = if wrap { slen } else { slen.min(m - i0) };
            for k in 0..span {
                let u = k as f32 / sr;
                let l = (1.0 - u / 0.5).max(0.0);
                let l4 = l * l * l * l;
                let sweep = 7020.0 - 7000.0 * (u / 0.01).min(1.0);
                ph = (ph + sweep / sr).fract();
                let sd = (-u * 1000.0f32.ln() / 0.03).exp();
                let j = if wrap { (i0 + k) % n } else { i0 + k };
                exc[j] += amp * (0.5 * l4 * rng.signed() + sd * (std::f32::consts::TAU * ph).sin());
            }
            t = next(&mut rng, t);
        }
        if wrap {
            exc.copy_within(0..n, n);
        }
        // The spark formant bank, shifted by `body` (3 kHz = the book's).
        let nyq = 0.45 * sr;
        let bands = [
            (4600.0, 5.0, 2.0),
            (7200.0, 5.0, 1.0),
            (480.0, 7.0, 1.2),
            (720.0, 8.0, 2.5),
        ];
        let mut fb: Vec<(Biquad, f32)> = bands
            .iter()
            .filter(|(f, _, _)| f * scale < nyq)
            .map(|&(f, q, g)| {
                let mut b = Biquad::new();
                b.set(FilterMode::Bp, f * scale, q, 0.0, sr);
                (b, g)
            })
            .collect();
        let mut post = Biquad::new();
        post.set(FilterMode::Bp, (2500.0 * scale).min(nyq), 0.5, 0.0, sr);
        let mut arc_hp = Biquad::new();
        arc_hp.set(FilterMode::Hp, 300.0, 0.707, 0.0, sr);
        let mut gate_lp = OnePole::default();

        // A short comb (the box the spark is in): 12 ms, feedback 0.2.
        let d = ((0.012 * sr) as usize).max(1);
        let mut line = vec![0.0f32; d];
        let mut damp = 0.0f32;
        let (mut p1, mut p2) = (0.0f32, 0.0f32);
        let mut out = vec![0.0f32; m];
        for i in 0..m {
            p1 = (p1 + f1 / sr).fract();
            p2 = (p2 + f2 / sr).fract();
            // Hum: two beating saws, clipped hard, under a slow level.
            let saws = (p1 + p2) - 1.0;
            let hum = (4.0 * saws).clamp(-1.0, 1.0) * hum_level[i] * hum_level[i];
            // Arcing: the reversed phasor squashed by a random factor,
            // squared and cos'd (a chirp per hum cycle), random-gated ~3 Hz.
            let x = ((1.0 - p1) * squash[i]).min(1.0) + 0.1;
            let chirp = arc_hp.tick((std::f32::consts::TAU * 12.0 * x * x).cos());
            let (g, _) = gate_lp.tick(if gate_w[i] > 0.2 { 1.0 } else { 0.0 }, 30.0, sr);
            let arc = chirp * g;
            let mut snap = 0.0;
            for (b, gain) in &mut fb {
                snap += *gain * b.tick(exc[i]);
            }
            let snap = post.tick(snap);
            let dry = 0.25 * hum + 0.3 * arc + 3.0 * snap;
            let j = i % d;
            damp = 0.7 * line[j] + 0.3 * damp;
            let y = dry + 0.2 * damp;
            line[j] = y;
            out[i] = 0.5 * y;
        }
        if wrap {
            out.drain(..n);
        }
        Ok(one(crate::buf::Buf::Audio(out)))
    }
}
