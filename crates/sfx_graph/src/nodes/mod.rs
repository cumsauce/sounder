//! The node types, by family. Each is a strict serde struct
//! (`deny_unknown_fields`) plus its [`Eval`](crate::node::Eval) impl.

mod control;
mod envelopes;
mod filters;
mod maths;
mod mixing;
mod nonlinear;
mod offline;
mod sources;
mod time;

pub use control::*;
pub use envelopes::*;
pub use filters::*;
pub use maths::*;
pub use mixing::*;
pub use nonlinear::*;
pub use offline::*;
pub use sources::*;
pub use time::*;

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
