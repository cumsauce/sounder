//! Ambient beds from the foley project (MIT / Apache-2.0): `leaves`, the
//! brush of `foley-synth/src/plants.rs` kept running by the wind, and
//! `underwater`, the submerged bed of `foley-dsp/src/underwater.rs`
//! (bubbles rising past the ear over a 50 Hz pressure tone).
//!
//! `wind` (the five-band air bed) and `cloth` (power-law rustle bursts) are
//! other nodes; `leaves` is the foliage's own sound, to be mixed under
//! `wind`, not a copy of either.

use std::f32::consts::TAU;

use foley_dsp::Bubble;
use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::{Biquad, FilterMode, Rng};
use crate::node::{Cx, Describe, Eval, In, Outs};

fn rate(cx: &Cx) -> u32 {
    (cx.sr.round() as u32).max(1)
}

fn exponential(rng: &mut Rng, rate: f32) -> f32 {
    -(1.0 - rng.unit()).max(1e-9).ln() / rate.max(1e-6)
}

/// What the wind blows through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Foliage {
    /// Grass, ferns, reeds, flowers: many thin blades flicking, bright.
    #[serde(alias = "reeds", alias = "fern")]
    Grass,
    /// Leaves, leafy bushes, a tree in leaf: fewer, bigger leaves, lower.
    #[default]
    #[serde(alias = "leaf", alias = "tree", alias = "bush")]
    Leaves,
    /// Dry stalks (a dead bush, ripe crops, autumn litter): crisp, crackling.
    #[serde(alias = "stalks", alias = "crops")]
    Dry,
}

/// Wind moving through foliage: a swish of stems sliding past each other
/// plus a sparse scatter of ticks, each a blade or leaf flicked and ringing
/// for a millisecond.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Leaves {
    /// Wind speed, m/s, 0..40. A number or a signal. 0 is silence; the
    /// brush of foley's model peaks at about 8 m/s (the leg at full push).
    pub wind: In,
    /// What it blows through (default `leaves`): grass (`reeds`, `fern`),
    /// leaves (`tree`, `bush`), dry (`stalks`, `crops`).
    #[serde(default)]
    pub foliage: Foliage,
    /// Gustiness, 0..1 (default 0.3): slow swings of the level (about
    /// 0.4 Hz), up to +-60 % at 1.
    #[serde(default)]
    pub gust: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Leaves {
    const BLURB: &'static str = "leaves (after the foley project's plant brush): wind (m/s, a signal) through grass / leaves / dry stalks, a band-passed swish plus flicked blade ticks; mix under wind; loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["foliage"];
    fn example() -> serde_json::Value {
        serde_json::json!({"wind": 6, "foliage": "leaves", "gust": 0.3, "seed": 1})
    }
}

/// Foley's `brush` constants per plant: swish band and Q, tick band, ticks a
/// second at full push, tick decay a sample (at 48 kHz), swish level.
struct Plant {
    band: f32,
    q: f32,
    tick_band: f32,
    ticks: f32,
    decay: f32,
    swish: f32,
}

fn plant(f: Foliage) -> Plant {
    let (band, q, tick_band, ticks, decay, swish) = match f {
        Foliage::Grass => (3500.0, 0.8, 5200.0, 450.0, 0.94, 0.5),
        Foliage::Leaves => (1500.0, 0.7, 2300.0, 150.0, 0.985, 0.7),
        Foliage::Dry => (4200.0, 1.0, 6500.0, 650.0, 0.9, 0.2),
    };
    Plant {
        band,
        q,
        tick_band,
        ticks,
        decay,
        swish,
    }
}

/// The wind speed at which the brush's swell shape reaches 1.
const LEAVES_REF_MS: f32 = 8.0;
/// Scales the raw model to a sensible peak (foley normalizes each brush to 0.3).
const LEAVES_LEVEL: f32 = 0.22;

/// A one-pole low-pass (foley's `LowPass`).
struct Lp {
    a: f32,
    y: f32,
}

impl Lp {
    fn new(hz: f32, sr: f32) -> Lp {
        Lp {
            a: 1.0 - (-TAU * hz.max(1.0) / sr).exp(),
            y: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

fn band(hz: f32, q: f32, sr: f32) -> Biquad {
    let mut b = Biquad::new();
    b.set(FilterMode::Bp, hz, q, 0.0, sr);
    b
}

impl Eval for Leaves {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let wind = cx.mono("wind", &self.wind)?;
        if let Some(v) = wind.constant()
            && !(v.is_finite() && (0.0..=40.0).contains(&v))
        {
            return Err(format!("wind: {v}: give metres a second from 0 to 40"));
        }
        let gust = self.gust.unwrap_or(0.3);
        if !(0.0..=1.0).contains(&gust) {
            return Err(format!("gust: {gust}: give a number from 0 to 1"));
        }
        let n = cx.n;
        let sr = cx.sr;
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let p = plant(self.foliage);
        // foley's constants are per sample at 48 kHz.
        let decay = p.decay.powf(48_000.0 / sr);
        let mut sweep = band(p.band, p.q, sr);
        let mut tick = band(p.tick_band, 1.5, sr);
        let mut flutter = Lp::new(20.0, sr);
        let mut gusts = Lp::new(0.4, sr);
        let mut envelope = 0.0f32;
        let mut until = 0.0f32;
        let tail = if wrap { (0.5 * sr) as usize } else { 0 };
        let dt = 1.0 / sr;
        let mut out = vec![0.0f32; n + tail];
        // Uniform noise through a one-pole at 0.4 Hz has a standard deviation
        // of sqrt(a / 6): scale it to about one.
        let gust_scale = 1.0 / ((1.0 - (-TAU * 0.4 / sr).exp()) / 6.0).sqrt();
        // Settle the gust and flutter filters before sample 0.
        for _ in 0..(3.0 * sr) as usize {
            gusts.tick(rng.signed());
        }
        for (i, o) in out.iter_mut().enumerate() {
            let w = wind.at(i % n);
            let w = if w.is_finite() {
                w.clamp(0.0, 40.0)
            } else {
                0.0
            };
            let swing = 1.0 + 0.6 * gust * (gusts.tick(rng.signed()) * gust_scale).clamp(-1.0, 1.0);
            // The brush's swell, 1 at the reference wind (it goes on
            // growing, as the leg pushing harder would).
            let shape = ((w / LEAVES_REF_MS).powf(1.5) * swing).max(0.0);
            until -= dt;
            if until <= 0.0 {
                envelope += 0.3 + 0.7 * rng.unit();
                until = exponential(&mut rng, p.ticks * shape.max(0.05));
            }
            envelope *= decay;
            // foley's Wobble(20 Hz, depth 3, 0.4..1.6)
            let wobble = (1.0 + 3.0 * flutter.tick(rng.signed())).clamp(0.4, 1.6);
            let noise = rng.signed();
            let swish = shape * p.swish * wobble * sweep.tick(noise);
            let ticks = envelope.min(2.0) * tick.tick(rng.signed());
            *o = if w > 0.0 {
                LEAVES_LEVEL * (swish + ticks * shape.min(1.0))
            } else {
                0.0
            };
        }
        if wrap {
            for j in 0..tail {
                let w = j as f32 / tail as f32;
                let t = out[n + j];
                out[j] = out[j] * w.sqrt() + t * (1.0 - w).sqrt();
            }
            out.truncate(n);
        }
        Ok(one(Buf::Audio(out)))
    }
}

/// The pressure tone's frequency (Hz): foley's 50 Hz sine.
const PRESSURE_HZ: f32 = 50.0;
/// Its level at full submersion: small, beneath the bubbles.
const PRESSURE_LEVEL: f32 = 0.25;
/// At most this many bubbles ring at once; past it a new one is dropped.
const MAX_VOICES: usize = 64;

/// The submerged bed.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Underwater {
    /// Depth of the listener under the surface, metres, 0..100. A number or
    /// a signal (the bed glides to it with a 0.3 s time constant, so a
    /// `ramp` dives and surfaces). The level is `1 - exp(-depth / 1.5)`:
    /// 0 at the surface is silence, 3 m is 0.86, 6 m is 0.98.
    pub depth: In,
    /// Bubble rate multiplier, 0..4 (default 1). Foley's rate is
    /// `level * (4 + 16 * level)` a second: 0 to 20 as the level rises.
    #[serde(default)]
    pub bubbles: Option<f32>,
    /// Level of the 50 Hz pressure tone, 0..2 (default 1 = foley's 0.25
    /// at full level).
    #[serde(default)]
    pub pressure: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam (the pressure tone is nudged
    /// to a whole number of cycles). Unset: yes in a `kind: loop` document,
    /// no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Underwater {
    const BLURB: &'static str = "underwater (after the foley project's submerged bed): bubbles of 0.5-6 mm rising past the ear over a 50 Hz pressure tone, both following depth (m, a signal); bubbles and pressure scale the two; loop: true wraps without a seam";
    fn example() -> serde_json::Value {
        serde_json::json!({"depth": 3, "bubbles": 1, "pressure": 1, "seed": 1})
    }
}

impl Eval for Underwater {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let depth = cx.mono("depth", &self.depth)?;
        if let Some(v) = depth.constant()
            && !(v.is_finite() && (0.0..=100.0).contains(&v))
        {
            return Err(format!("depth: {v}: give metres from 0 to 100"));
        }
        let bubbles = self.bubbles.unwrap_or(1.0);
        if !(0.0..=4.0).contains(&bubbles) {
            return Err(format!("bubbles: {bubbles}: give a multiplier from 0 to 4"));
        }
        let pressure = self.pressure.unwrap_or(1.0);
        if !(0.0..=2.0).contains(&pressure) {
            return Err(format!("pressure: {pressure}: give a level from 0 to 2"));
        }
        let (n, sr) = (cx.n, cx.sr);
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let sample_rate = rate(cx);
        let dt = 1.0 / sr;
        let glide = dt / 0.3;
        let target = |i: usize| {
            let d = depth.at(i % n);
            let d = if d.is_finite() {
                d.clamp(0.0, 100.0)
            } else {
                0.0
            };
            1.0 - (-d / 1.5).exp()
        };
        let tail = if wrap { (0.3 * sr) as usize } else { 0 };
        // A looped tone gets a whole number of cycles in the buffer.
        let hz = if wrap {
            (PRESSURE_HZ * n as f32 / sr).round().max(1.0) * sr / n as f32
        } else {
            PRESSURE_HZ
        };
        let mut heard = target(0);
        // The first bubble is drawn like the rest (foley's is born at 0),
        // so a loop has no onset at its seam.
        let level0 = target(0);
        let mut until_next = exponential(&mut rng, bubbles * level0 * (4.0 + 16.0 * level0));
        let mut voices: Vec<Bubble> = Vec::with_capacity(MAX_VOICES);
        let mut out = vec![0.0f32; n + tail];
        for (i, o) in out.iter_mut().enumerate() {
            heard += (target(i) - heard) * glide;
            let level = heard;
            let rate_now = bubbles * level * (4.0 + 16.0 * level);
            // (No births in the wrap tail: it only carries the ringing over.)
            if rate_now > 0.0 && i < n {
                until_next -= dt;
                while until_next <= 0.0 {
                    until_next += exponential(&mut rng, rate_now);
                    if voices.len() < MAX_VOICES {
                        // Log-uniform 0.5 to 6 mm, like the water bed's
                        // babbling bubbles.
                        let radius = 0.0005 * 12.0f32.powf(rng.unit());
                        voices.push(Bubble::new_at(
                            radius,
                            0.02 * (0.5 + rng.unit()),
                            sample_rate,
                        ));
                    }
                }
            }
            let mut mix = 0.0;
            for v in &mut voices {
                mix += v.sample();
            }
            if i % 64 == 63 {
                voices.retain(|v| !v.done());
            }
            // The tone's phase from the sample index: exact at any length.
            let ph = ((f64::from(hz) * i as f64) / f64::from(sr)).fract() as f32;
            if i < n {
                // (Not in the wrap tail: the tone is already periodic.)
                mix += PRESSURE_LEVEL * pressure * level * (TAU * ph).sin();
            }
            *o = if level > 1e-4 { level * mix } else { 0.0 };
        }
        if wrap {
            // The last bubbles' ringing goes onto the start.
            for j in 0..tail {
                let t = out[n + j];
                out[j] += t;
            }
            out.truncate(n);
        }
        Ok(one(Buf::Audio(out)))
    }
}
