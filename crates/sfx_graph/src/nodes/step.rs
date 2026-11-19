//! `step`: one footstep or a walk, after foley's `contact(Contact::Step, ..)`
//! (`foley-synth/src/modal.rs` of the foley project, MIT / Apache-2.0),
//! called as it stands. A heel of about 8 kg strikes the ground through a
//! Hertz contact (soft sole on the surface: the contact time sets how dull
//! it is); the surface's own plate modes ring under it; the sole scuffs as
//! the foot rolls; the ball of the foot lands 60-120 ms later, softer.
//! Gravel and sand crunch in grains, earth crumbles, grass swishes round the
//! shoe, snow crackles as its bonds break, a wooden floor now and then
//! creaks. Each step in `at` is its own seed. Added because the `footstep`
//! node on concrete, gravel and grass was heard as a snare, a pitter-patter
//! and a blow (labels batch 11).

use foley_synth::{Contact, Material, contact};
use serde::Deserialize;

use super::{OneOrMany, one};
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, Outs};

const MAX_STEPS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepSurface {
    /// Concrete, stone, pavement (foley's stone: a 25 cm slab).
    #[serde(alias = "stone", alias = "pavement")]
    Concrete,
    /// Gravel and sand: grains crushed under the heel.
    #[serde(alias = "sand")]
    Gravel,
    /// Grass, leaves, plants: blades brushing the shoe.
    #[serde(alias = "leaves", alias = "plant")]
    Grass,
    /// Dirt, mud, clay: a dull thud that crumbles.
    #[serde(alias = "earth", alias = "mud")]
    Dirt,
    /// A floor of 2.5 cm boards.
    Wood,
    /// Steel plate.
    Metal,
    /// Ice.
    Ice,
    /// Settled snow: crunch and squeak.
    Snow,
    /// Carpet, wool, rubber matting.
    #[serde(alias = "soft")]
    Carpet,
}

impl StepSurface {
    fn material(self) -> Material {
        match self {
            StepSurface::Concrete => Material::Stone,
            StepSurface::Gravel => Material::Granular,
            StepSurface::Grass => Material::Plant,
            StepSurface::Dirt => Material::Earth,
            StepSurface::Wood => Material::Wood,
            StepSurface::Metal => Material::Metal,
            StepSurface::Ice => Material::Ice,
            StepSurface::Snow => Material::Snow,
            StepSurface::Carpet => Material::Soft,
        }
    }
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// concrete (default), gravel, grass, dirt, wood, metal, ice, snow, carpet.
    #[serde(default)]
    pub surface: Option<StepSurface>,
    /// Heel speed, m/s, 0.2..3 (default 1, a walk; 0.5 a careful step, 2 a
    /// run): faster is a shorter, brighter contact and more crunch.
    #[serde(default)]
    pub speed: Option<f32>,
    /// Seconds each step lands: one number or a list (default 0.005).
    #[serde(default)]
    pub at: Option<OneOrMany>,
    /// Level, dB (default 0; each step is foley's level: 0.4 peak on stone).
    #[serde(default)]
    pub gain_db: Option<f32>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Step {
    const BLURB: &'static str = "footsteps (after the foley project's contact model): a Hertz heel strike on the surface's own modes, the sole's scuff, the ball of the foot 60-120 ms later; gravel crunches, grass swishes, snow squeaks; surface, speed m/s, at (one time or a list: a walk), gain_db";
    const WORDS: &'static [&'static str] = &["surface"];
    fn example() -> serde_json::Value {
        serde_json::json!({"surface": "gravel", "speed": 1, "at": [0.005, 0.55], "seed": 1})
    }
}

impl Eval for Step {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = self.speed.unwrap_or(1.0);
        if !(speed.is_finite() && (0.2..=3.0).contains(&speed)) {
            return Err(format!(
                "speed: {speed}: give the heel speed in m/s from 0.2 to 3 (1 a walk, 2 a run)"
            ));
        }
        let times = self.at.as_ref().map_or(vec![0.005], |a| a.values());
        if times.len() > MAX_STEPS || times.iter().any(|t| !t.is_finite() || *t < 0.0) {
            return Err(format!(
                "at: give up to {MAX_STEPS} times in seconds, each 0 or more"
            ));
        }
        let gain = 10f32.powf(self.gain_db.unwrap_or(0.0) / 20.0);
        let material = self.surface.unwrap_or(StepSurface::Concrete).material();
        let sr = (cx.sr.round() as u32).max(1);
        let mut rng = cx.rng(self.seed);
        let mut out = vec![0.0f32; cx.n];
        for t in times {
            let s = contact(Contact::Step, material, speed, rng.next_u64(), sr);
            let start = (t * cx.sr) as usize;
            for (o, x) in out.iter_mut().skip(start).zip(&s) {
                *o += gain * x;
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}
