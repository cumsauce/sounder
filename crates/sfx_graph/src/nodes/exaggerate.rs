//! Exaggeration layers (Phase 6): finished layers a cinematic or game hit
//! adds on top of the real event. `subdrop` is a sine sweeping down under a
//! hit; `riser` is a swell of noise and a rising tone that peaks at `at`, the
//! moment of the hit. Both are tonal or synthetic on purpose: a realistic
//! impact (the Phase 0 door slam) is atonal, and a sine sub read there as "a
//! tuned drum". Use them where the brief asks for a larger-than-life hit.

use std::f32::consts::{PI, TAU};

use serde::Deserialize;

use super::{non_negative, one, positive};
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, Outs};

fn finite_in(field: &str, x: f32, lo: f32, hi: f32, what: &str) -> Result<f32, String> {
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give {what} {lo}..{hi}"))
    }
}

/// A sine sweeping down under a hit, with its own envelope.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subdrop {
    /// Starting pitch, Hz (default 90, 20..400).
    #[serde(default)]
    pub from: Option<f32>,
    /// Final pitch, Hz (default 32, 15..400).
    #[serde(default)]
    pub to: Option<f32>,
    /// Seconds the sweep takes, exponential (default 0.6, 0.01..10); then
    /// it holds `to`.
    #[serde(default)]
    pub time: Option<f32>,
    /// Seconds to rise (default 0.004, 0..1): a click-free start.
    #[serde(default)]
    pub attack: Option<f32>,
    /// Seconds to fall 60 dB after the attack (default 1.2, 0.01..30).
    #[serde(default)]
    pub decay: Option<f32>,
    /// When it starts, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// 0..1 (default 0): tanh saturation that adds the 2nd and 3rd
    /// harmonics, so the drop is heard on speakers with no sub (the level is
    /// kept at the same peak).
    #[serde(default)]
    pub drive: Option<f32>,
    /// Peak level, linear (default 0.8).
    #[serde(default)]
    pub amp: Option<f32>,
}

impl Describe for Subdrop {
    const BLURB: &'static str = "a sine sweeping down under a hit (from -> to Hz, own envelope, drive for small speakers): a cinematic layer, tonal on purpose";
    fn example() -> serde_json::Value {
        serde_json::json!({"from": 90, "to": 32, "time": 0.6, "decay": 1.2, "at": 0.01, "drive": 0.3})
    }
}

impl Eval for Subdrop {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let from = finite_in("from", self.from.unwrap_or(90.0), 20.0, 400.0, "Hz")?;
        let to = finite_in("to", self.to.unwrap_or(32.0), 15.0, 400.0, "Hz")?;
        let time = finite_in("time", self.time.unwrap_or(0.6), 0.01, 10.0, "seconds")?;
        let attack = finite_in("attack", self.attack.unwrap_or(0.004), 0.0, 1.0, "seconds")?;
        let decay = finite_in("decay", self.decay.unwrap_or(1.2), 0.01, 30.0, "seconds")?;
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let drive = finite_in("drive", self.drive.unwrap_or(0.0), 0.0, 1.0, "")?;
        let amp = finite_in("amp", self.amp.unwrap_or(0.8), 0.0, 4.0, "a linear level")?;
        let start = cx.at(at);
        let k = 4.0 * drive;
        let norm = if k > 0.0 { 1.0 / k.tanh() } else { 1.0 };
        let mut phase = 0.0f32;
        let mut out = vec![0.0f32; cx.n];
        for (i, o) in out.iter_mut().enumerate().skip(start) {
            let t = (i - start) as f32 / cx.sr;
            let f = if t < time {
                from * (to / from).powf(t / time)
            } else {
                to
            };
            let env = if t < attack {
                0.5 - 0.5 * (PI * t / attack.max(1e-6)).cos()
            } else {
                10f32.powf(-3.0 * (t - attack) / decay)
            };
            let s = phase.sin();
            let shaped = if k > 0.0 { (k * s).tanh() * norm } else { s };
            *o = amp * env * shaped;
            phase = (phase + TAU * f / cx.sr) % TAU;
        }
        Ok(one(Buf::Audio(out)))
    }
}

/// A swell that peaks at the moment of a hit: rising noise through a
/// band-pass that climbs with a rising tone.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Riser {
    /// The moment of the hit, seconds: the swell peaks here and stops
    /// (required; at least `length`).
    pub at: f32,
    /// Seconds the swell lasts before `at` (default 1.5, 0.05..20).
    #[serde(default)]
    pub length: Option<f32>,
    /// Pitch of the tone and the noise band at the start, Hz (default 200,
    /// 20..8000).
    #[serde(default)]
    pub from: Option<f32>,
    /// Pitch at `at`, Hz (default 2000, 20..12000).
    #[serde(default)]
    pub to: Option<f32>,
    /// Noise against tone, 0..1 (default 0.6: mostly noise).
    #[serde(default)]
    pub noise: Option<f32>,
    /// How sharply it swells: the level goes as (time / length)^shape
    /// (default 2, 0.5..8; higher holds back longer).
    #[serde(default)]
    pub shape: Option<f32>,
    /// Seconds to fade out after `at` (default 0.01, 0..2): 0 stops dead at
    /// the hit, which is usually what a riser does.
    #[serde(default)]
    pub release: Option<f32>,
    /// Peak level, linear (default 0.8).
    #[serde(default)]
    pub amp: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Riser {
    const BLURB: &'static str = "a swell of noise and a rising tone that peaks at `at` (the hit) and stops: a cinematic anticipation layer";
    fn example() -> serde_json::Value {
        serde_json::json!({"at": 1.5, "length": 1.4, "from": 200, "to": 2400, "noise": 0.6, "shape": 2})
    }
}

impl Eval for Riser {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let length = finite_in("length", self.length.unwrap_or(1.5), 0.05, 20.0, "seconds")?;
        let at = positive("at", self.at)?;
        if at < length {
            return Err(format!(
                "at: {at}: the swell starts `length` ({length} s) before the hit, so `at` must be at least {length} (or shorten length)"
            ));
        }
        let from = finite_in("from", self.from.unwrap_or(200.0), 20.0, 8000.0, "Hz")?;
        let to = finite_in("to", self.to.unwrap_or(2000.0), 20.0, 12000.0, "Hz")?;
        let mix = finite_in("noise", self.noise.unwrap_or(0.6), 0.0, 1.0, "")?;
        let shape = finite_in("shape", self.shape.unwrap_or(2.0), 0.5, 8.0, "")?;
        let release = finite_in("release", self.release.unwrap_or(0.01), 0.0, 2.0, "seconds")?;
        let amp = finite_in("amp", self.amp.unwrap_or(0.8), 0.0, 4.0, "a linear level")?;
        let top = crate::dsp::max_hz(cx.sr);
        let mut rng = cx.rng(self.seed);
        let (start, hit) = (cx.at(at - length), cx.at(at));
        let end = cx.at(at + release);
        let mut phase = 0.0f32;
        // A state-variable band-pass for the noise (q about 2).
        let (mut lo, mut bp) = (0.0f32, 0.0f32);
        let mut out = vec![0.0f32; cx.n];
        for (i, o) in out.iter_mut().enumerate().take(end).skip(start) {
            let u = ((i - start) as f32 / (hit - start).max(1) as f32).min(1.0);
            let f = (from * (to / from).powf(u)).min(top);
            let level = if i < hit {
                u.powf(shape)
            } else {
                1.0 - (i - hit) as f32 / (end - hit).max(1) as f32
            };
            let g = 2.0 * (PI * f / cx.sr).sin();
            let n = rng.signed();
            let hp = n - lo - 0.5 * bp;
            bp += g * hp;
            lo += g * bp;
            let tone = phase.sin();
            phase = (phase + TAU * f / cx.sr) % TAU;
            *o = amp * level * (mix * 1.5 * bp + (1.0 - mix) * tone);
        }
        Ok(one(Buf::Audio(out)))
    }
}
