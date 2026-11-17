//! Mixing and stereo.

use std::f32::consts::FRAC_PI_4;

use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, In, Outs};

/// One `mix` input: a node name, or a node with gain, pan and offset.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum MixIn {
    Name(String),
    Full(MixInFull),
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixInFull {
    pub input: In,
    /// Linear gain (default 1; may be a signal).
    #[serde(default)]
    pub gain: Option<In>,
    /// -1 (left) .. 1 (right); makes the mix stereo.
    #[serde(default)]
    pub pan: Option<f32>,
    /// Seconds to delay this input by (layering a tail after a transient).
    #[serde(default)]
    pub at: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum To {
    Mono,
    Stereo,
}

/// Equal-power pan gains for `p` in -1..1.
fn pan_gains(p: f32) -> [f32; 2] {
    let a = (p.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    [a.cos(), a.sin()]
}

/// Sums inputs.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mix {
    /// ["node", {"input": "node", "gain": 0.5, "pan": -0.3, "at": 0.1}, ...].
    pub inputs: Vec<MixIn>,
    /// mono or stereo (default: stereo if any input is stereo or panned).
    #[serde(default)]
    pub to: Option<To>,
}

impl Describe for Mix {
    const BLURB: &'static str = "sums inputs with gain, pan and time offset; mono or stereo";
    fn example() -> serde_json::Value {
        serde_json::json!({"inputs": ["body", {"input": "click", "gain": 0.5, "pan": -0.3, "at": 0.01}], "to": "stereo"})
    }
}

impl Eval for Mix {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if self.inputs.is_empty() {
            return Err("inputs: give at least one node".into());
        }
        let mut parts = Vec::new();
        for (k, m) in self.inputs.iter().enumerate() {
            let field = format!("inputs[{k}]");
            let (input, gain, pan, at) = match m {
                MixIn::Name(s) => (In::Name(s.clone()), None, None, None),
                MixIn::Full(f) => (f.input.clone(), f.gain.clone(), f.pan, f.at),
            };
            let sig = cx.audio(&field, &input)?;
            let g = cx.mono_or(&format!("{field}.gain"), &gain, 1.0)?;
            parts.push((sig, g, pan, cx.at(at.unwrap_or(0.0))));
        }
        let stereo = match self.to {
            Some(To::Mono) => false,
            Some(To::Stereo) => true,
            None => parts
                .iter()
                .any(|(s, _, p, _)| s.is_stereo() || p.is_some()),
        };
        let n = cx.n;
        if stereo {
            let mut out = vec![[0.0f32; 2]; n];
            for (s, g, pan, off) in &parts {
                let pg = pan.map_or([1.0, 1.0], |p| {
                    // Panning a stereo input balances it; a mono input is
                    // placed with equal power.
                    let e = pan_gains(p);
                    if s.is_stereo() {
                        [
                            e[0] * std::f32::consts::SQRT_2,
                            e[1] * std::f32::consts::SQRT_2,
                        ]
                        .map(|v| v.min(1.0))
                    } else {
                        e
                    }
                });
                for i in *off..n {
                    let j = i - off;
                    let gi = g.at(i);
                    out[i][0] += s.ch(0, j) * gi * pg[0];
                    out[i][1] += s.ch(1, j) * gi * pg[1];
                }
            }
            Ok(one(Buf::Stereo(out)))
        } else {
            let mut out = vec![0.0f32; n];
            for (s, g, _, off) in &parts {
                for i in *off..n {
                    out[i] += s.at(i - off) * g.at(i);
                }
            }
            Ok(one(Buf::Audio(out)))
        }
    }
}

/// Places a sound left to right.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pan {
    pub input: In,
    /// -1 (left) .. 1 (right); may be a signal (auto-pan, fly-bys).
    pub pan: In,
}

impl Describe for Pan {
    const BLURB: &'static str = "places mono in stereo (or balances stereo)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "pan": "lfo1"})
    }
}

impl Eval for Pan {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let pan = cx.mono("pan", &self.pan)?;
        let stereo = input.is_stereo();
        Ok(one(Buf::Stereo(
            (0..cx.n)
                .map(|i| {
                    let g = pan_gains(pan.at(i));
                    if stereo {
                        let b = [
                            (g[0] * std::f32::consts::SQRT_2).min(1.0),
                            (g[1] * std::f32::consts::SQRT_2).min(1.0),
                        ];
                        [input.ch(0, i) * b[0], input.ch(1, i) * b[1]]
                    } else {
                        let x = input.at(i);
                        [x * g[0], x * g[1]]
                    }
                })
                .collect(),
        )))
    }
}

/// Stereo width (mid/side).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Width {
    pub input: In,
    /// 0 = mono, 1 = unchanged, 2 = twice as wide (may be a signal).
    pub amount: In,
}

impl Describe for Width {
    const BLURB: &'static str = "stereo width (0 = mono, 1 = as is, 2 = wider)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "verb", "amount": 1.5})
    }
}

impl Eval for Width {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let amount = cx.mono("amount", &self.amount)?;
        Ok(one(Buf::Stereo(
            (0..cx.n)
                .map(|i| {
                    let (l, r) = (input.ch(0, i), input.ch(1, i));
                    let (m, s) = (0.5 * (l + r), 0.5 * (l - r) * amount.at(i));
                    [m + s, m - s]
                })
                .collect(),
        )))
    }
}
