//! Nonlinear: waveshaping and dynamics.

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::Buf;
use crate::dsp::{Follower, limiter_gain};
use crate::node::{Cx, Eval, In, Outs};
use crate::{db_to_gain, gain_to_db};

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShapeMode {
    #[default]
    Tanh,
    Clip,
    Fold,
    Bitcrush,
    Rectify,
}

/// A waveshaper.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub input: In,
    /// tanh (default: warm saturation), clip (hard), fold (wavefolder:
    /// bright, metallic), bitcrush (quantize to `bits`), rectify (absolute
    /// value: an octave up).
    #[serde(default)]
    pub mode: ShapeMode,
    /// Gain before the shaper (default 1; may be a signal).
    #[serde(default)]
    pub drive: Option<In>,
    /// bitcrush: bits (default 8).
    #[serde(default)]
    pub bits: Option<f32>,
}

impl Eval for Shape {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let drive = cx.mono_or("drive", &self.drive, 1.0)?;
        let levels = 2f32.powf(self.bits.unwrap_or(8.0).clamp(1.0, 24.0) - 1.0);
        let mode = self.mode;
        Ok(one(per_channel(&input, cx.n, |x| {
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let u = v * drive.at(i);
                    match mode {
                        ShapeMode::Tanh => u.tanh(),
                        ShapeMode::Clip => u.clamp(-1.0, 1.0),
                        ShapeMode::Fold => ((u + 1.0).rem_euclid(4.0) - 2.0).abs() - 1.0,
                        ShapeMode::Bitcrush => (u * levels).round() / levels,
                        ShapeMode::Rectify => u.abs(),
                    }
                })
                .collect()
        })))
    }
}

/// A feed-forward compressor (stereo channels linked).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Compress {
    pub input: In,
    /// Level above which gain is reduced, dBFS.
    pub threshold_db: f32,
    /// Input dB over the threshold per output dB (4 = 4:1).
    pub ratio: f32,
    /// Seconds to clamp down (default 0.005).
    #[serde(default)]
    pub attack: Option<f32>,
    /// Seconds to let go (default 0.1).
    #[serde(default)]
    pub release: Option<f32>,
    /// Gain after, dB (default 0).
    #[serde(default)]
    pub makeup_db: Option<f32>,
    /// Soft knee width, dB (default 6).
    #[serde(default)]
    pub knee_db: Option<f32>,
}

impl Eval for Compress {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let ratio = self.ratio.max(1.0);
        let (t, knee) = (self.threshold_db, self.knee_db.unwrap_or(6.0).max(0.0));
        let (a, r) = (self.attack.unwrap_or(0.005), self.release.unwrap_or(0.1));
        let makeup = db_to_gain(self.makeup_db.unwrap_or(0.0));
        let chans = input.channels(cx.n);
        let mut f = Follower::new();
        let gain: Vec<f32> = (0..cx.n)
            .map(|i| {
                let peak = chans.iter().map(|c| c[i].abs()).fold(0.0, f32::max);
                let level = gain_to_db(f.tick(peak, a, r, cx.sr));
                let over = level - t;
                let reduce = if knee > 0.0 && over.abs() <= knee / 2.0 {
                    (1.0 / ratio - 1.0) * (over + knee / 2.0).powi(2) / (2.0 * knee)
                } else if over > 0.0 {
                    (1.0 / ratio - 1.0) * over
                } else {
                    0.0
                };
                db_to_gain(reduce) * makeup
            })
            .collect();
        Ok(one(Buf::from_channels(
            chans
                .into_iter()
                .map(|c| c.iter().zip(&gain).map(|(x, g)| x * g).collect())
                .collect(),
        )))
    }
}

/// A lookahead peak limiter (acausal).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    pub input: In,
    /// No sample goes above this, dBFS (default -1).
    #[serde(default)]
    pub ceiling_db: Option<f32>,
    /// Seconds to recover after a peak (default 0.05).
    #[serde(default)]
    pub release: Option<f32>,
}

impl Eval for Limit {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let chans = input.channels(cx.n);
        Ok(one(Buf::from_channels(limit(
            chans,
            db_to_gain(self.ceiling_db.unwrap_or(-1.0)),
            self.release.unwrap_or(0.05),
            cx.sr,
        ))))
    }
}

/// Limits channels (linked) to `ceiling`.
pub fn limit(chans: Vec<Vec<f32>>, ceiling: f32, release: f32, sr: f32) -> Vec<Vec<f32>> {
    let n = chans.first().map_or(0, Vec::len);
    let peaks: Vec<f32> = (0..n)
        .map(|i| chans.iter().map(|c| c[i].abs()).fold(0.0, f32::max))
        .collect();
    let g = limiter_gain(&peaks, ceiling, release, sr);
    chans
        .into_iter()
        .map(|c| c.iter().zip(&g).map(|(x, g)| x * g).collect())
        .collect()
}
