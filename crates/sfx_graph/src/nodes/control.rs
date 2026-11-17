//! Control signals: LFOs, random motion, sample and hold, slew, ramps.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{CurveSpec, mono, non_negative, one, per_channel};
use crate::dsp::interp;
use crate::node::{Cx, Eval, In, Outs};

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LfoShape {
    #[default]
    Sine,
    Tri,
    Saw,
    Square,
}

fn ranged(v: f32, range: Option<[f32; 2]>) -> f32 {
    match range {
        Some([a, b]) => a + (b - a) * (0.5 * v + 0.5),
        None => v,
    }
}

/// A low-frequency oscillator.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lfo {
    /// sine (default), tri, saw (rising), square.
    #[serde(default)]
    pub shape: LfoShape,
    /// Hz (may be a signal).
    pub freq: In,
    /// Start phase 0..1 (default 0).
    #[serde(default)]
    pub phase: Option<f32>,
    /// Map -1..1 onto [lo, hi] (default: -1..1).
    #[serde(default)]
    pub range: Option<[f32; 2]>,
}

impl Eval for Lfo {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let freq = cx.mono("freq", &self.freq)?;
        let mut ph = f64::from(self.phase.unwrap_or(0.0).rem_euclid(1.0));
        let shape = self.shape;
        Ok(one(mono(cx.n, |i| {
            let t = ph as f32;
            let v = match shape {
                LfoShape::Sine => (TAU * t).sin(),
                LfoShape::Tri => 1.0 - 4.0 * ((t + 0.25).rem_euclid(1.0) - 0.5).abs(),
                LfoShape::Saw => 2.0 * t - 1.0,
                LfoShape::Square => {
                    if t < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
            };
            ph = (ph + f64::from(freq.at(i) / cx.sr)).rem_euclid(1.0);
            ranged(v, self.range)
        })))
    }
}

/// Smooth random motion: random points `rate` times a second joined by
/// smooth (cubic Hermite) curves. Foley's `Wobble` low-passes white noise
/// instead; points give a bounded, rate-true wander that an agent can reason
/// about, so this differs on purpose.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Wander {
    /// New targets a second.
    pub rate: f32,
    /// Map -1..1 onto [lo, hi] (default: -1..1).
    #[serde(default)]
    pub range: Option<[f32; 2]>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Eval for Wander {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = super::positive("rate", self.rate)?;
        if rate > cx.sr {
            return Err(format!(
                "rate: {rate}: a point per sample at most ({} a second): lower it",
                cx.sr
            ));
        }
        let mut rng = cx.rng(self.seed);
        let per = cx.sr / rate;
        let count = (cx.n as f32 / per).ceil() as usize + 4;
        let pts: Vec<f32> = (0..count).map(|_| rng.signed()).collect();
        Ok(one(mono(cx.n, |i| {
            let x = i as f32 / per;
            let k = x.floor() as usize;
            let f = x - k as f32;
            let p = |j: usize| pts[j.min(count - 1)];
            let (p0, p1, p2, p3) = (p(k), p(k + 1), p(k + 2), p(k + 3));
            // Catmull-Rom through p1 -> p2, clamped to -1..1.
            let v = 0.5
                * ((2.0 * p1)
                    + (-p0 + p2) * f
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * f * f
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * f * f * f);
            ranged(v.clamp(-1.0, 1.0), self.range)
        })))
    }
}

/// Samples and holds.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SampleHold {
    pub input: In,
    /// Samples a second (or give trigger).
    #[serde(default)]
    pub rate: Option<In>,
    /// Samples when this signal rises above 0.5 (or give rate).
    #[serde(default)]
    pub trigger: Option<In>,
}

impl Eval for SampleHold {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let mut held = input.at(0);
        match (&self.rate, &self.trigger) {
            (Some(r), None) => {
                let r = cx.mono("rate", r)?;
                let mut ph = 1.0f64;
                Ok(one(mono(cx.n, |i| {
                    if ph >= 1.0 {
                        held = input.at(i);
                        ph -= ph.floor();
                    }
                    ph += f64::from(r.at(i).max(0.0) / cx.sr);
                    held
                })))
            }
            (None, Some(t)) => {
                let t = cx.mono("trigger", t)?;
                let mut was = false;
                Ok(one(mono(cx.n, |i| {
                    let on = t.at(i) > 0.5;
                    if on && !was {
                        held = input.at(i);
                    }
                    was = on;
                    held
                })))
            }
            _ => Err("give exactly one of rate (samples a second) or trigger (a signal)".into()),
        }
    }
}

/// Limits how fast a signal changes.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Slew {
    pub input: In,
    /// Seconds to rise by 1 (0 = instant).
    pub up: f32,
    /// Seconds to fall by 1 (0 = instant).
    pub down: f32,
}

impl Eval for Slew {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let up = non_negative("up", self.up)?;
        let down = non_negative("down", self.down)?;
        let step = |t: f32| {
            if t <= 0.0 {
                f32::INFINITY
            } else {
                1.0 / (t * cx.sr)
            }
        };
        let (su, sd) = (step(up), step(down));
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut y = x.first().copied().unwrap_or(0.0);
            x.iter()
                .map(|v| {
                    let d = (v - y).clamp(-sd, su);
                    y += d;
                    y
                })
                .collect()
        })))
    }
}

/// A sweep from one value to another.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ramp {
    pub from: f32,
    pub to: f32,
    /// Seconds the sweep takes.
    pub time: f32,
    /// exp (default: geometric, right for pitch and cutoff), linear, smooth.
    #[serde(default)]
    pub curve: Option<CurveSpec>,
    /// Seconds before the sweep starts (holds `from` until then; default 0).
    #[serde(default)]
    pub at: Option<f32>,
}

impl Eval for Ramp {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let time = non_negative("time", self.time)?;
        let at = self.at.unwrap_or(0.0);
        let curve = self.curve.unwrap_or(CurveSpec::Exp).curve();
        Ok(one(mono(cx.n, |i| {
            let t = i as f32 / cx.sr - at;
            let x = if time <= 0.0 {
                if t >= 0.0 { 1.0 } else { 0.0 }
            } else {
                t / time
            };
            interp(self.from, self.to, x, curve)
        })))
    }
}
