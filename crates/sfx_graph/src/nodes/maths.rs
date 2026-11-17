//! Maths on signals.

use serde::Deserialize;

use super::one;
use crate::buf::{Buf, Sig};
use crate::db_to_gain;
use crate::node::{Cx, Eval, In, Outs};

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MathOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Pow,
    Abs,
    Mix,
}

/// Element-wise `f` over up to three signals: stereo when any is.
fn zip3(n: usize, sigs: [&Sig; 3], f: impl Fn(f32, f32, f32) -> f32) -> Buf {
    let stereo = sigs.iter().any(|s| s.is_stereo());
    let k = if stereo { 2 } else { 1 };
    Buf::from_channels(
        (0..k)
            .map(|c| {
                (0..n)
                    .map(|i| f(sigs[0].ch(c, i), sigs[1].ch(c, i), sigs[2].ch(c, i)))
                    .collect()
            })
            .collect(),
    )
}

/// Arithmetic on signals or numbers.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Math {
    /// add, sub, mul, div (x / 0 = 0), min, max, pow (sign-preserving), abs
    /// (of a), mix (a + (b - a) * t).
    pub op: MathOp,
    pub a: In,
    /// Second operand (all but abs).
    #[serde(default)]
    pub b: Option<In>,
    /// mix: the blend 0 (a) .. 1 (b).
    #[serde(default)]
    pub t: Option<In>,
}

impl Eval for Math {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let a = cx.audio("a", &self.a)?;
        let b = match (&self.b, self.op) {
            (Some(b), _) => cx.audio("b", b)?,
            (None, MathOp::Abs) => Sig::Const(0.0),
            (None, op) => {
                return Err(format!("b: `{op:?}` needs a second operand b").to_lowercase());
            }
        };
        let t = match (&self.t, self.op) {
            (Some(t), _) => cx.audio("t", t)?,
            (None, MathOp::Mix) => return Err("t: mix needs t (0 = a, 1 = b)".into()),
            (None, _) => Sig::Const(0.0),
        };
        let op = self.op;
        Ok(one(zip3(cx.n, [&a, &b, &t], |a, b, t| match op {
            MathOp::Add => a + b,
            MathOp::Sub => a - b,
            MathOp::Mul => a * b,
            MathOp::Div => {
                if b.abs() < 1e-12 {
                    0.0
                } else {
                    a / b
                }
            }
            MathOp::Min => a.min(b),
            MathOp::Max => a.max(b),
            MathOp::Pow => a.signum() * a.abs().powf(b),
            MathOp::Abs => a.abs(),
            MathOp::Mix => a + (b - a) * t,
        })))
    }
}

/// Maps one range onto another.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Remap {
    pub input: In,
    /// The input range [a, b].
    pub from: [f32; 2],
    /// The output range [c, d].
    pub to: [f32; 2],
    /// Clamp to the output range (default false).
    #[serde(default)]
    pub clamp: bool,
}

impl Eval for Remap {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let [a, b] = self.from;
        if (b - a).abs() < 1e-12 {
            return Err("from: the two ends are equal; give a range [a, b] with a != b".into());
        }
        let [c, d] = self.to;
        let (lo, hi) = (c.min(d), c.max(d));
        let z = Sig::Const(0.0);
        Ok(one(zip3(cx.n, [&input, &z, &z], |x, _, _| {
            let y = c + (x - a) / (b - a) * (d - c);
            if self.clamp { y.clamp(lo, hi) } else { y }
        })))
    }
}

/// Scales a signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gain {
    pub input: In,
    /// Gain in dB (or give gain).
    #[serde(default)]
    pub db: Option<In>,
    /// Linear gain (or give db).
    #[serde(default)]
    pub gain: Option<In>,
}

impl Eval for Gain {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let (g, is_db) = match (&self.db, &self.gain) {
            (Some(d), None) => (cx.mono("db", d)?, true),
            (None, Some(g)) => (cx.mono("gain", g)?, false),
            _ => return Err("give exactly one of db or gain".into()),
        };
        let z = Sig::Const(0.0);
        Ok(one(zip3(cx.n, [&input, &g, &z], |x, g, _| {
            x * if is_db { db_to_gain(g) } else { g }
        })))
    }
}
