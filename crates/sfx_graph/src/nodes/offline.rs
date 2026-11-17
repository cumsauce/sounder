//! Offline (acausal) processing and the output stage.

use std::f32::consts::PI;

use serde::Deserialize;

use super::{non_negative, one};
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{db_to_gain, loudness};

/// Plays the input backwards.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reverse {
    pub input: In,
}

impl Describe for Reverse {
    const BLURB: &'static str = "plays the input backwards";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "verb"})
    }
}

impl Eval for Reverse {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        for c in &mut ch {
            c.reverse();
        }
        Ok(one(Buf::from_channels(ch)))
    }
}

/// Gain that brings `ch` to a peak or a loudness.
fn normalize_gain(
    ch: &[Vec<f32>],
    peak_db: Option<f32>,
    lufs: Option<f32>,
    sr: f32,
) -> Result<f32, String> {
    match (peak_db, lufs) {
        (Some(p), None) => {
            let peak = ch.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
            Ok(if peak > 1e-9 {
                db_to_gain(p) / peak
            } else {
                1.0
            })
        }
        (None, Some(l)) => {
            let now = loudness::integrated(ch, sr as u32);
            Ok(if now > -69.0 {
                db_to_gain(l - now)
            } else {
                1.0
            })
        }
        _ => Err("give exactly one of peak_db or lufs".into()),
    }
}

/// Scales to a peak or a loudness.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Normalize {
    pub input: In,
    /// Target sample peak, dBFS (or give lufs).
    #[serde(default)]
    pub peak_db: Option<f32>,
    /// Target integrated loudness, LUFS (or give peak_db).
    #[serde(default)]
    pub lufs: Option<f32>,
}

impl Describe for Normalize {
    const BLURB: &'static str = "scales to a peak or a loudness";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "peak_db": -1})
    }
}

impl Eval for Normalize {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        let g = normalize_gain(&ch, self.peak_db, self.lufs, cx.sr)?;
        for v in ch.iter_mut().flatten() {
            *v *= g;
        }
        Ok(one(Buf::from_channels(ch)))
    }
}

/// Removes leading silence.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Trim {
    pub input: In,
    /// Samples below this are silence, dBFS (default -60).
    #[serde(default)]
    pub threshold_db: Option<f32>,
}

impl Describe for Trim {
    const BLURB: &'static str = "removes leading silence (the end is padded with silence)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "threshold_db": -60})
    }
}

impl Eval for Trim {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let th = db_to_gain(self.threshold_db.unwrap_or(-60.0));
        let ch = input.channels(cx.n);
        let first = (0..cx.n)
            .find(|&i| ch.iter().any(|c| c[i].abs() >= th))
            .unwrap_or(0);
        Ok(one(Buf::from_channels(
            ch.into_iter()
                .map(|c| {
                    let mut v = c[first..].to_vec();
                    v.resize(cx.n, 0.0);
                    v
                })
                .collect(),
        )))
    }
}

/// Raised-cosine fades at the edges of `ch` (`fin`, `fout` seconds).
fn fade_edges(ch: &mut [Vec<f32>], fin: f32, fout: f32, sr: f32) {
    let n = ch.first().map_or(0, Vec::len);
    let a = ((fin * sr) as usize).min(n);
    let b = ((fout * sr) as usize).min(n);
    for c in ch.iter_mut() {
        for i in 0..a {
            c[i] *= 0.5 - 0.5 * (PI * i as f32 / a as f32).cos();
        }
        for k in 0..b {
            // k = 0 is the last sample: exactly silent.
            c[n - 1 - k] *= 0.5 - 0.5 * (PI * k as f32 / b as f32).cos();
        }
    }
}

/// Fades in and out.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    pub input: In,
    /// Fade-in seconds (default 0).
    #[serde(default, rename = "in")]
    pub fade_in: Option<f32>,
    /// Fade-out seconds at the very end (default 0).
    #[serde(default)]
    pub out: Option<f32>,
}

impl Describe for Fade {
    const BLURB: &'static str = "raised-cosine fade in and out";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "in": 0.01, "out": 0.2})
    }
}

impl Eval for Fade {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        fade_edges(
            &mut ch,
            non_negative("in", self.fade_in.unwrap_or(0.0))?,
            non_negative("out", self.out.unwrap_or(0.0))?,
            cx.sr,
        );
        Ok(one(Buf::from_channels(ch)))
    }
}

/// The sound.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub input: In,
    /// Gain first, dB (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Then normalize to this integrated loudness, LUFS (default: leave).
    #[serde(default)]
    pub lufs: Option<f32>,
    /// Edge fades, seconds (defaults 0.002 in, 0.005 out; 0 for a loop).
    #[serde(default)]
    pub fade_in: Option<f32>,
    #[serde(default)]
    pub fade_out: Option<f32>,
    /// Lookahead limiter ceiling, dBFS (default -1).
    #[serde(default)]
    pub ceiling_db: Option<f32>,
}

impl Output {
    /// The output stage over a whole buffer (also re-run by the renderer
    /// after the auto-duration length is found).
    pub fn process(
        &self,
        mut ch: Vec<Vec<f32>>,
        sr: f32,
        looped: bool,
    ) -> Result<Vec<Vec<f32>>, String> {
        let g = db_to_gain(self.gain_db.unwrap_or(0.0));
        if let Some(l) = self.lufs {
            let g2 = normalize_gain(&ch, None, Some(l), sr)?;
            for v in ch.iter_mut().flatten() {
                *v *= g * g2;
            }
        } else if g != 1.0 {
            for v in ch.iter_mut().flatten() {
                *v *= g;
            }
        }
        let mut ch =
            super::nonlinear::limit(ch, db_to_gain(self.ceiling_db.unwrap_or(-1.0)), 0.05, sr);
        let (fi, fo) = if looped { (0.0, 0.0) } else { (0.002, 0.005) };
        fade_edges(
            &mut ch,
            non_negative("fade_in", self.fade_in.unwrap_or(fi))?,
            non_negative("fade_out", self.fade_out.unwrap_or(fo))?,
            sr,
        );
        Ok(ch)
    }
}

impl Describe for Output {
    const BLURB: &'static str =
        "the sound: gain, loudness target, edge fades and a ceiling limiter; exactly one";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "mix1", "lufs": -16})
    }
}

impl Eval for Output {
    fn causal(&self) -> bool {
        self.lufs.is_none()
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let ch = self.process(input.channels(cx.n), cx.sr, cx.looped)?;
        Ok(one(Buf::from_channels(ch)))
    }
}
