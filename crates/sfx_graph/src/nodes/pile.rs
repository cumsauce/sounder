//! `grind`: the scraping, grinding and rolling of a pile that is still
//! moving, or of one object driven by a speed signal. Built from `rolling`
//! (the surface force) and `modal` (the body that rings it), with an
//! activity envelope that rises with hits and falls as the pile settles.

use serde::Deserialize;

use super::{Modal, Rolling, non_negative, one, positive};
use crate::buf::{Buf, Sig};
use crate::node::{Cx, Describe, Eval, In};

/// Most events one `grind` may take.
const MAX_GRIND_EVENTS: usize = 200_000;

/// The port name `grind` injects its force and micro-impacts under.
const GRIND_INPUT: &str = "__grind_input";

/// Per material: roughness, grit size (m), body f0 (Hz), modal noise,
/// noise width, loss.
fn preset(material: &str) -> (f32, f32, f32, f32, f32, f32) {
    match material {
        "wood" => (0.5, 0.003, 500.0, 0.6, 0.25, 2.0),
        "metal" => (0.4, 0.002, 1500.0, 0.4, 0.1, 0.5),
        "glass" => (0.3, 0.0015, 2500.0, 0.5, 0.15, 0.8),
        _ => (0.7, 0.004, 700.0, 0.6, 0.25, 2.0),
    }
}

/// Scraping, grinding and rolling that follows how much is still moving.
/// Give `events` (impacts: each raises the activity by its value, and it
/// falls over `settle` seconds), a `speed` signal (m/s of one rolling or
/// sliding object, or a motion energy; it is used as it is), or both.
/// The activity drives a `rolling` force through a `modal` body of the
/// material, plus micro-impacts (small hits of the grit) at `micro_rate`
/// per second at full activity.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grind {
    /// Impacts (a collapse, shatter, poisson...): value = speed 0..1.
    #[serde(default)]
    pub events: Option<In>,
    /// Speed over the surface, m/s, a number or a signal (a `ramp` is a
    /// ball that speeds up, a `follower` a motion energy). Added to the
    /// activity of `events`.
    #[serde(default)]
    pub speed: Option<In>,
    /// `stone` (default), `wood`, `metal` or `glass`: sets the defaults of
    /// `roughness`, `size`, `f0`, and the body's noise and loss.
    #[serde(default)]
    pub material: Option<String>,
    /// Surface roughness 0..1, as `rolling` (default by material).
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Grit size, metres, as `rolling` (default by material).
    #[serde(default)]
    pub size: Option<f32>,
    /// Lowest mode of the ringing body, Hz (default by material).
    #[serde(default)]
    pub f0: Option<f32>,
    /// Seconds for the activity from `events` to fall 60 dB (default 1.5).
    #[serde(default)]
    pub settle: Option<f32>,
    /// Speed over the surface at activity 1 for `events`, m/s (default 1).
    #[serde(default)]
    pub top_speed: Option<f32>,
    /// Micro-impacts per second at activity 1 (default 40; 0 = none).
    #[serde(default)]
    pub micro_rate: Option<f32>,
    /// Level of the micro-impacts against the scrape, dB (default -6).
    #[serde(default)]
    pub micro_db: Option<f32>,
    /// Output level, dB (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Grind {
    const BLURB: &'static str = "scraping, grinding and rolling that follows how much is moving: an activity envelope from events (rises with hits, falls over `settle`) and/or a speed signal drives rolling + modal + micro-impacts; material-aware; for a settling pile or one rolling ball";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "material": "stone", "settle": 1.5, "micro_rate": 40})
    }
}

/// Evaluates `node` with the audio `buf` filled in under [`GRIND_INPUT`].
fn eval_with<N: Eval>(cx: &Cx, node: &N, buf: Vec<f32>) -> Result<Vec<f32>, String> {
    let buf = std::sync::Arc::new(Buf::Audio(buf));
    let get = |i: &In| -> Result<Sig, String> {
        match i {
            In::Name(n) if n == GRIND_INPUT => Ok(Sig::Buf(buf.clone())),
            other => (cx.get)(other),
        }
    };
    let sub = Cx {
        sr: cx.sr,
        n: cx.n,
        seed: cx.seed,
        looped: cx.looped,
        base_dir: cx.base_dir,
        sources: cx.sources,
        params: cx.params,
        sounds: cx.sounds,
        stack: cx.stack,
        get: &get,
    };
    match node.eval(&sub)?.into_iter().next() {
        Some((_, Buf::Audio(v))) => Ok(v),
        _ => Err("grind: building block gave no mono output".into()),
    }
}

impl Eval for Grind {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<crate::node::Outs, String> {
        if self.events.is_none() && self.speed.is_none() {
            return Err("give `events`, `speed`, or both".into());
        }
        let material = self.material.as_deref().unwrap_or("stone");
        if !["stone", "wood", "metal", "glass"].contains(&material) {
            return Err(format!(
                "material: {material}: give stone, wood, metal or glass"
            ));
        }
        let (p_rough, p_size, p_f0, p_noise, p_width, p_loss) = preset(material);
        let rough = self.roughness.unwrap_or(p_rough);
        let size = positive("size", self.size.unwrap_or(p_size))?;
        let f0 = positive("f0", self.f0.unwrap_or(p_f0))?;
        let settle = positive("settle", self.settle.unwrap_or(1.5))?;
        let top = positive("top_speed", self.top_speed.unwrap_or(1.0))?;
        let rate = non_negative("micro_rate", self.micro_rate.unwrap_or(40.0))?;
        let micro_gain = 10f32.powf(self.micro_db.unwrap_or(-6.0) / 20.0);
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);

        // Activity: events add their value, the sum falls exp over `settle`
        // (60 dB), soft-saturated to 0..1; a speed signal adds to it.
        let mut kick = vec![0.0f32; cx.n];
        if let Some(e) = &self.events {
            let ev = cx.events("events", e)?;
            if ev.len() > MAX_GRIND_EVENTS {
                return Err(format!("events: {}: at most {MAX_GRIND_EVENTS}", ev.len()));
            }
            for e in &ev {
                let i = (e.t.max(0.0) * f64::from(sr)).round() as usize;
                if let Some(k) = kick.get_mut(i) {
                    *k += e.value.clamp(0.0, 1.0);
                }
            }
        }
        let speed = match &self.speed {
            Some(s) => Some(cx.mono("speed", s)?),
            None => None,
        };
        let fall = (-6.9078 / (settle * sr)).exp();
        let rise = 1.0 - (-1.0 / (0.01 * sr)).exp();
        let (mut state, mut smooth) = (0.0f32, 0.0f32);
        let mut act = vec![0.0f32; cx.n];
        for i in 0..cx.n {
            state = state * fall + kick[i];
            smooth += (state - smooth) * rise;
            let from_events = if self.events.is_some() {
                (1.0 - (-smooth).exp()) * top
            } else {
                0.0
            };
            let from_speed = speed.as_ref().map_or(0.0, |s| s.at(i).abs());
            act[i] = (from_events + from_speed).clamp(0.0, 50.0);
        }

        // The scrape: rolling's force at the activity speed through a body.
        let v_max = (0.25 * sr * size).min(50.0);
        let drive: Vec<f32> = act.iter().map(|a| a.min(v_max)).collect();
        let roll: Rolling = serde_json::from_value(serde_json::json!({
            "speed": GRIND_INPUT, "roughness": rough, "size": size,
            "seed": self.seed ^ 0x6217_0001,
        }))
        .map_err(|e| format!("rolling: {e}"))?;
        let force = eval_with(cx, &roll, drive)?;
        let body = |seed: u64, f: f32| -> Result<Modal, String> {
            serde_json::from_value(serde_json::json!({
                "excite": GRIND_INPUT, "material": material, "shape": "plate",
                "f0": f, "noise": p_noise, "noise_width": p_width, "loss": p_loss,
                "tilt": 2.0, "seed": seed,
            }))
            .map_err(|e| format!("modal: {e}"))
        };
        let mut out = eval_with(cx, &body(self.seed ^ 0x6217_0002, f0)?, force)?;

        // Micro-impacts: grit hits at a rate that follows the activity.
        if rate > 0.0 {
            let mut x = vec![0.0f32; cx.n];
            for i in 0..cx.n {
                let a = (act[i] / top).min(1.0);
                if rng.unit() < rate * a / sr {
                    let amp = a.powf(1.2) * (0.3 + 0.7 * rng.unit());
                    for k in 0..3 {
                        if let Some(o) = x.get_mut(i + k) {
                            *o += amp * [0.5, 1.0, 0.5][k];
                        }
                    }
                }
            }
            let hits = eval_with(cx, &body(self.seed ^ 0x6217_0003, f0 * 1.7)?, x)?;
            for (o, h) in out.iter_mut().zip(hits) {
                *o += h * micro_gain;
            }
        }
        let g = 10f32.powf(self.gain_db.unwrap_or(0.0) / 20.0);
        out.iter_mut().for_each(|v| *v *= g);
        Ok(one(Buf::Audio(out)))
    }
}
