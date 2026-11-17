//! Envelopes.

use serde::Deserialize;

use super::{CurveSpec, mono, non_negative, one, per_channel, positive};
use crate::dsp::{Follower as FollowerState, interp};
use crate::node::{Cx, Eval, In, Outs};

/// A breakpoint envelope.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Env {
    /// [[seconds, value], ...] in time order; holds the first value before
    /// and the last value after.
    pub points: Vec<[f32; 2]>,
    /// Between points: linear (default), exp (geometric when both ends are
    /// positive), smooth (raised cosine).
    #[serde(default)]
    pub curve: CurveSpec,
}

impl Eval for Env {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if self.points.is_empty() {
            return Err("points: give at least one [t, v]".into());
        }
        if self.points.windows(2).any(|w| w[1][0] < w[0][0]) {
            return Err("points: times must not go backwards; sort the [t, v] pairs by t".into());
        }
        let curve = self.curve.curve();
        let p = &self.points;
        let mut k = 0usize;
        Ok(one(mono(cx.n, |i| {
            let t = i as f32 / cx.sr;
            while k + 1 < p.len() && p[k + 1][0] <= t {
                k += 1;
            }
            if t <= p[0][0] {
                p[0][1]
            } else if k + 1 >= p.len() {
                p[p.len() - 1][1]
            } else {
                let (a, b) = (p[k], p[k + 1]);
                let span = b[0] - a[0];
                let x = if span > 0.0 { (t - a[0]) / span } else { 1.0 };
                interp(a[1], b[1], x, curve)
            }
        })))
    }
}

/// Attack / decay / sustain / release.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Adsr {
    /// Seconds to rise to 1.
    pub attack: f32,
    /// Seconds to fall (60 dB of the way) to sustain.
    pub decay: f32,
    /// Level held while the gate is on, 0..1.
    pub sustain: f32,
    /// Seconds to fall (60 dB) to silence once the gate is off.
    pub release: f32,
    /// Without a gate: seconds the sustain is held after attack + decay
    /// (default 0).
    #[serde(default)]
    pub hold: Option<f32>,
    /// A gate signal: on while above 0.5 (each rise retriggers). Without
    /// one, the envelope is triggered once at `at`.
    #[serde(default)]
    pub gate: Option<In>,
    /// Without a gate: the trigger time, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
}

impl Eval for Adsr {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let attack = non_negative("attack", self.attack)?;
        let decay = non_negative("decay", self.decay)?;
        let release = non_negative("release", self.release)?;
        let sustain = self.sustain.clamp(0.0, 1.0);
        let start = cx.at(self.at.unwrap_or(0.0));
        let on_len = ((attack + decay + self.hold.unwrap_or(0.0).max(0.0)) * cx.sr) as usize;
        let gate = match &self.gate {
            Some(g) => Some(cx.mono("gate", g)?),
            None => None,
        };
        let coef = |t: f32| {
            if t <= 0.0 {
                0.0
            } else {
                (-(1000f32.ln()) / (t * cx.sr)).exp()
            }
        };
        let (cd, cr) = (coef(decay), coef(release));
        let rise = if attack <= 0.0 {
            1.0
        } else {
            1.0 / (attack * cx.sr)
        };
        #[derive(PartialEq)]
        enum Stage {
            Idle,
            Attack,
            Decay,
            Release,
        }
        let mut stage = Stage::Idle;
        let mut level = 0.0f32;
        let mut was_on = false;
        Ok(one(mono(cx.n, |i| {
            let on = match &gate {
                Some(g) => g.at(i) > 0.5,
                None => i >= start && i < start + on_len.max(1),
            };
            if on && !was_on {
                stage = Stage::Attack;
            } else if !on && was_on {
                stage = Stage::Release;
            }
            was_on = on;
            match stage {
                Stage::Idle => {}
                Stage::Attack => {
                    level += rise;
                    if level >= 1.0 {
                        level = 1.0;
                        stage = Stage::Decay;
                    }
                }
                Stage::Decay => level = sustain + cd * (level - sustain),
                Stage::Release => level *= cr,
            }
            level
        })))
    }
}

/// Exponential fall from 1.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Decay {
    /// Seconds to fall 60 dB (exp) or to reach 0 (linear).
    pub time: f32,
    /// exp (default) or linear.
    #[serde(default)]
    pub curve: Option<CurveSpec>,
    /// Seconds before the fall starts; silent before it (default 0).
    #[serde(default)]
    pub at: Option<f32>,
}

impl Eval for Decay {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let time = positive("time", self.time)?;
        let start = cx.at(self.at.unwrap_or(0.0));
        let linear = matches!(self.curve, Some(CurveSpec::Linear));
        let k = -(1000f32.ln()) / (time * cx.sr);
        Ok(one(mono(cx.n, |i| {
            if i < start {
                return 0.0;
            }
            let j = (i - start) as f32;
            if linear {
                (1.0 - j / (time * cx.sr)).max(0.0)
            } else {
                (k * j).exp()
            }
        })))
    }
}

/// The input's level over time.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Follower {
    pub input: In,
    /// Seconds to follow a rise (default 0.005).
    #[serde(default)]
    pub attack: Option<f32>,
    /// Seconds to follow a fall (default 0.1).
    #[serde(default)]
    pub release: Option<f32>,
}

impl Eval for Follower {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let (a, r) = (self.attack.unwrap_or(0.005), self.release.unwrap_or(0.1));
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = FollowerState::new();
            x.iter().map(|v| f.tick(v.abs(), a, r, cx.sr)).collect()
        })))
    }
}
