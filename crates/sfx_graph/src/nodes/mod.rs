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

// slot T0c
mod ringmod;

// slot S1

mod synth;

// slot S3

// slot S4

mod dynamics;

// slot S6
mod modfx;

// slot S7
mod motion;

// slot S8

// slot S9
mod modulators;

// slot S10
mod sequence;

// slot S11
mod space;

// slot S12
mod lofi;

mod blast;

// slot M2
mod machines;

// slot M3
mod elements;

// slot M4
mod water;

// slot M5
mod waveguide;

// slot M6
mod surface;

// slot M7
mod fracture;

// slot M8

// slot X1

// slot X2
mod bands;

// slot X3

// slot R1F
mod mechanism;

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
pub use sources::*;
pub use spectral::*;
pub use time::*;
pub use voices::*;

// slot T0c
pub use ringmod::*;

// slot S1

pub use synth::*;

// slot S3

// slot S4

pub use dynamics::*;

// slot S6
pub use modfx::*;

// slot S7
pub use motion::*;

// slot S8

// slot S9
pub use modulators::*;

// slot S10
pub use sequence::*;

// slot S11
pub use space::*;

// slot S12
pub use lofi::*;

pub use blast::*;

// slot M2
pub use machines::*;

// slot M3
pub use elements::*;

// slot M4
pub use water::*;

// slot M5
pub use waveguide::*;

// slot M6
pub use surface::*;

// slot M7
pub use fracture::*;

// slot M8

// slot X1

// slot X2
pub use bands::*;

// slot X3

// slot R1F
pub use mechanism::*;

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
