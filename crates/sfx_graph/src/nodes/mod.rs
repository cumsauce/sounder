//! The node types, by family. Each is a strict serde struct
//! (`deny_unknown_fields`) plus its [`Eval`](crate::node::Eval) impl.

mod control;
mod envelopes;
mod events;
mod expr;
mod filters;
mod foley;
mod maths;
mod mixing;
mod modal;
mod nonlinear;
mod offline;
mod sources;
mod spectral;
mod time;
mod voices;

mod ringmod;

mod synth;

mod dynamics;

mod modfx;

mod motion;

mod modulators;

mod sequence;

mod camera;
mod space;

mod lofi;

mod blast;

mod ambience;
mod cave;
mod creature;
mod farnell;
mod farnell_creatures;
mod farnell_liquid;
mod farnell_phone;
mod farnell_scifi;
mod farnell_vehicles;
mod frog;
mod machines;
mod rain;
mod step;
mod surf;

mod elements;

mod water;

mod waveguide;

mod surface;

mod fracture;

mod bands;

mod mechanism;

mod crackle;
// bodies and joints stepped together.
mod rig;
mod rig_waveguide;
// the finishing bus.
mod finish;
// exaggeration layers.
mod exaggerate;

pub use control::*;
pub use envelopes::*;
pub use events::*;
pub use expr::*;
pub use filters::*;
pub use foley::*;
pub use maths::*;
pub use mixing::*;
pub use modal::*;
pub use nonlinear::*;
pub use offline::*;
pub use rig::*;
pub use sources::*;
pub use spectral::*;
pub use time::*;
pub use voices::*;

pub use ringmod::*;

pub use synth::*;

pub use dynamics::*;

pub use modfx::*;

pub use motion::*;

pub use modulators::*;

pub use sequence::*;

pub use camera::*;
pub use space::*;

pub use lofi::*;

pub use blast::*;

pub use ambience::*;
pub use cave::*;
pub use creature::*;
pub use farnell::*;
pub use farnell_creatures::*;
pub use farnell_liquid::*;
pub use farnell_phone::*;
pub use farnell_scifi::*;
pub use farnell_vehicles::*;
pub use frog::*;
pub use machines::*;
pub use rain::*;
pub use step::*;
pub use surf::*;

pub use elements::*;

pub use water::*;

pub use waveguide::*;

pub use surface::*;

pub use fracture::*;

pub use bands::*;

pub use mechanism::*;

pub use crackle::*;
pub use exaggerate::*;
pub use finish::*;

use serde::Deserialize;

use crate::buf::{Buf, Sig};
use crate::dsp::Curve;

/// A number or a list of numbers.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany {
    One(f32),
    Many(Vec<f32>),
}

impl OneOrMany {
    pub fn values(&self) -> Vec<f32> {
        match self {
            OneOrMany::One(x) => vec![*x],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

/// An interpolation curve.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CurveSpec {
    #[default]
    Linear,
    Exp,
    Smooth,
}

impl CurveSpec {
    pub fn curve(self) -> Curve {
        match self {
            CurveSpec::Linear => Curve::Linear,
            CurveSpec::Exp => Curve::Exp,
            CurveSpec::Smooth => Curve::Smooth,
        }
    }
}

/// Runs `f` over each channel of `input` (1 or 2), each with fresh state.
pub(crate) fn per_channel(input: &Sig, n: usize, f: impl Fn(&[f32]) -> Vec<f32>) -> Buf {
    Buf::from_channels(input.channels(n).iter().map(|c| f(c)).collect())
}

/// A mono buffer from a per-sample function.
pub(crate) fn mono(n: usize, f: impl FnMut(usize) -> f32) -> Buf {
    Buf::Audio((0..n).map(f).collect())
}

pub(crate) fn one(b: Buf) -> crate::node::Outs {
    vec![("out", b)]
}

/// Errors unless `x` is finite and > 0.
pub(crate) fn positive(field: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() && x > 0.0 {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give a number > 0"))
    }
}

/// Errors unless `x` is finite and >= 0.
pub(crate) fn non_negative(field: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() && x >= 0.0 {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give a number >= 0"))
    }
}
