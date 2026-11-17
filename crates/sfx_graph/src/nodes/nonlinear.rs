//! Nonlinear: waveshaping and dynamics.

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::Buf;
use crate::dsp::{Follower, Oversampler, limiter_gain};
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{db_to_gain, gain_to_db};

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShapeMode {
    #[default]
    Tanh,
    Clip,
    Fold,
    Bitcrush,
    Rectify,
    Asym,
    Diode,
    Tube,
    Table,
}

/// The most points a `table` shape takes.
const MAX_TABLE_POINTS: usize = 4096;

/// A waveshaper.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub input: In,
    /// tanh (default: warm saturation), clip (hard), fold (wavefolder:
    /// bright, metallic), bitcrush (quantize to `bits`), rectify (absolute
    /// value: an octave up), asym (tanh of the signal plus `bias`, the
    /// resting value taken off: even harmonics), diode (unity slope at 0,
    /// the positive half saturates at +1 exponentially, the negative at
    /// -0.5: even harmonics and a DC offset), tube (positive half
    /// saturates at +1 with a sharp knee, negative at -0.7 with a soft
    /// one: even harmonics at high drive), table (the curve in `points`).
    #[serde(default)]
    pub mode: ShapeMode,
    /// Gain before the shaper (default 1; may be a signal).
    #[serde(default)]
    pub drive: Option<In>,
    /// bitcrush: bits (default 8).
    #[serde(default)]
    pub bits: Option<f32>,
    /// asym: where on the tanh curve the signal rests (default 0.5; 0 is
    /// plain tanh; any finite value; the further from 0, the more even
    /// harmonics and the more DC: follow with a high-pass).
    #[serde(default)]
    pub bias: Option<f32>,
    /// table: the curve as `[[x, y], ...]` (x increasing, at most 4096
    /// points), linear between points and held at the ends. When no x is
    /// negative the curve is odd-extended (y(-x) = -y(x)), and a point at
    /// x = 0 must have y = 0.
    #[serde(default)]
    pub points: Option<Vec<[f32; 2]>>,
    /// Times the shaper runs faster than the sample rate: 1 (default), 2, 4
    /// or 8. More removes aliasing from the harmonics the curve makes and
    /// costs that many times the work (the shaper's input is band-limited
    /// to 0.45 of the sample rate on the way up). Not allowed inside a
    /// feedback loop.
    #[serde(default)]
    pub oversample: Option<u32>,
    /// Wet / dry, 0..1 (default 1; may be a signal). 0 is the input,
    /// before `drive`.
    #[serde(default)]
    pub mix: Option<In>,
}

impl Describe for Shape {
    const BLURB: &'static str =
        "waveshaper tanh/clip/fold/bitcrush/rectify/asym/diode/tube/table, oversampling, mix";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "mode": "tanh", "drive": 3.0, "oversample": 2})
    }
}

/// A shaping curve ready to run: the mode's constants, validated once.
/// [`Shape`] and its feedback-loop ticker both use it, so a mode behaves the
/// same in both.
#[derive(Clone, Debug)]
pub struct ShapeKernel {
    mode: ShapeMode,
    levels: f32,
    bias: f32,
    bias_rest: f32,
    xs: Vec<f64>,
    ys: Vec<f64>,
    slopes: Vec<f64>,
}

impl ShapeKernel {
    /// One sample through the curve. Always finite.
    #[inline]
    pub fn apply(&self, u: f32) -> f32 {
        // Keeps fold's rem_euclid and the exponentials finite.
        let u = u.clamp(-1e9, 1e9);
        let y = match self.mode {
            ShapeMode::Tanh => u.tanh(),
            ShapeMode::Clip => u.clamp(-1.0, 1.0),
            ShapeMode::Fold => ((u + 1.0).rem_euclid(4.0) - 2.0).abs() - 1.0,
            ShapeMode::Bitcrush => (u * self.levels).round() / self.levels,
            ShapeMode::Rectify => u.abs(),
            ShapeMode::Asym => (u + self.bias).tanh() - self.bias_rest,
            ShapeMode::Diode => {
                if u >= 0.0 {
                    1.0 - (-u).exp()
                } else {
                    -0.5 * (1.0 - (u * 2.0).exp())
                }
            }
            ShapeMode::Tube => {
                if u >= 0.0 {
                    let u2 = u * u;
                    u / (1.0 + u2 * u2).sqrt().sqrt()
                } else {
                    0.7 * (u / 0.7).tanh()
                }
            }
            ShapeMode::Table => self.table(f64::from(u)) as f32,
        };
        if y.is_finite() { y } else { 0.0 }
    }

    /// The table in f64, so the identity curve returns its input exactly.
    fn table(&self, u: f64) -> f64 {
        let n = self.xs.len();
        match self.xs.partition_point(|&x| x <= u) {
            0 => self.ys[0],
            i if i == n => self.ys[n - 1],
            i => self.ys[i - 1] + (u - self.xs[i - 1]) * self.slopes[i - 1],
        }
    }
}

impl Shape {
    /// Checks the fields against the mode and builds the curve.
    pub fn kernel(&self) -> Result<ShapeKernel, String> {
        let mode = self.mode;
        if self.points.is_some() && mode != ShapeMode::Table {
            return Err("points: only the `table` mode uses points: set mode to table".into());
        }
        if self.bias.is_some() && mode != ShapeMode::Asym {
            return Err("bias: only the `asym` mode uses bias: set mode to asym".into());
        }
        let bias = self.bias.unwrap_or(0.5);
        if !bias.is_finite() {
            return Err(format!("bias: {bias}: give a finite number"));
        }
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        if mode == ShapeMode::Table {
            let pts = self
                .points
                .as_ref()
                .ok_or("points: the table mode needs points, e.g. [[-1, -1], [1, 1]]")?;
            if pts.len() > MAX_TABLE_POINTS {
                return Err(format!(
                    "points: {} points: at most {MAX_TABLE_POINTS}",
                    pts.len()
                ));
            }
            for (i, p) in pts.iter().enumerate() {
                if !(p[0].is_finite() && p[1].is_finite()) {
                    return Err(format!("points: point {i} is {p:?}: give finite numbers"));
                }
                if i > 0 && p[0] <= pts[i - 1][0] {
                    return Err(format!(
                        "points: point {i} has x = {} which is not after {}: give x in increasing order",
                        p[0],
                        pts[i - 1][0]
                    ));
                }
            }
            let odd = pts.first().is_some_and(|p| p[0] >= 0.0);
            if odd {
                if pts[0][0] == 0.0 && pts[0][1] != 0.0 {
                    return Err(format!(
                        "points: x >= 0 only is odd-extended, so the point at x = 0 needs y = 0, not {}",
                        pts[0][1]
                    ));
                }
                for p in pts.iter().rev().filter(|p| p[0] > 0.0) {
                    xs.push(f64::from(-p[0]));
                    ys.push(f64::from(-p[1]));
                }
                if pts[0][0] == 0.0 {
                    xs.push(0.0);
                    ys.push(0.0);
                }
            }
            for p in pts.iter().filter(|p| !odd || p[0] > 0.0) {
                xs.push(f64::from(p[0]));
                ys.push(f64::from(p[1]));
            }
            if xs.len() < 2 {
                return Err(
                    "points: give at least two points (or one with x > 0, which is mirrored)"
                        .into(),
                );
            }
        }
        let slopes = xs
            .windows(2)
            .zip(ys.windows(2))
            .map(|(x, y)| (y[1] - y[0]) / (x[1] - x[0]))
            .collect();
        Ok(ShapeKernel {
            mode,
            levels: 2f32.powf(self.bits.unwrap_or(8.0).clamp(1.0, 24.0) - 1.0),
            bias,
            bias_rest: bias.tanh(),
            xs,
            ys,
            slopes,
        })
    }
}

/// Runs `u` through `kernel` at `factor` times the rate and returns it
/// aligned with the input in time: the oversampler's delay is taken out
/// (rounded to a whole sample, or, with `fractional`, interpolated by third-order
/// Lagrange so a blend with the dry signal does not comb).
fn shape_oversampled(
    u: &[f32],
    kernel: &ShapeKernel,
    os: &Oversampler,
    fractional: bool,
) -> Vec<f32> {
    let mut os = os.clone();
    let n = u.len();
    let lat = os.latency();
    let pad = lat.ceil() as usize + 3;
    let raw: Vec<f32> = (0..n + pad)
        .map(|i| os.process(u.get(i).copied().unwrap_or(0.0), |s| kernel.apply(s)))
        .collect();
    if !fractional {
        let k = lat.round() as usize;
        return raw[k..k + n].to_vec();
    }
    // y[i] = raw(i + lat), four taps around it.
    let k = lat.floor() as usize;
    let t = lat - k as f32 + 1.0;
    let l0 = -(t - 1.0) * (t - 2.0) * (t - 3.0) / 6.0;
    let l1 = t * (t - 2.0) * (t - 3.0) / 2.0;
    let l2 = -t * (t - 1.0) * (t - 3.0) / 2.0;
    let l3 = t * (t - 1.0) * (t - 2.0) / 6.0;
    (0..n)
        .map(|i| {
            // Taps at raw[i + k - 1 ..= i + k + 2]; the first is before
            // the start for i = 0 when k = 0 (silence there).
            let at = |j: usize| (i + j + k).checked_sub(1).map_or(0.0, |p| raw[p]);
            l0 * at(0) + l1 * at(1) + l2 * at(2) + l3 * at(3)
        })
        .collect()
}

impl Eval for Shape {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let drive = cx.mono_or("drive", &self.drive, 1.0)?;
        let mix = cx.mono_or("mix", &self.mix, 1.0)?;
        let kernel = self.kernel()?;
        let os = Oversampler::new(self.oversample.unwrap_or(1) as usize)?;
        let fractional = mix.constant().is_none_or(|m| m < 1.0);
        Ok(one(per_channel(&input, cx.n, |x| {
            let u: Vec<f32> = x.iter().enumerate().map(|(i, v)| v * drive.at(i)).collect();
            let wet: Vec<f32> = if os.factor() == 1 {
                u.iter().map(|v| kernel.apply(*v)).collect()
            } else {
                shape_oversampled(&u, &kernel, &os, fractional)
            };
            x.iter()
                .zip(&wet)
                .enumerate()
                .map(|(i, (dry, wet))| blend(*dry, *wet, mix.at(i)))
                .collect()
        })))
    }
}

/// `wet` at mix 1 and `dry` at 0 exactly, a linear blend between.
#[inline]
pub(crate) fn blend(dry: f32, wet: f32, mix: f32) -> f32 {
    if mix >= 1.0 {
        wet
    } else if mix <= 0.0 {
        dry
    } else {
        dry * (1.0 - mix) + wet * mix
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
    /// A signal that drives the detector instead of the input (mono or
    /// stereo; the loudest channel counts). The gain it asks for is applied
    /// to the input: ducking, pumping. Default: the input itself.
    #[serde(default)]
    pub key: Option<In>,
}

impl Describe for Compress {
    const BLURB: &'static str = "feed-forward compressor";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "threshold_db": -18, "ratio": 4, "attack": 0.005, "release": 0.1, "key": "hit"})
    }
}

impl Eval for Compress {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let ratio = self.ratio.max(1.0);
        let (t, knee) = (self.threshold_db, self.knee_db.unwrap_or(6.0).max(0.0));
        let (a, r) = (self.attack.unwrap_or(0.005), self.release.unwrap_or(0.1));
        let makeup = db_to_gain(self.makeup_db.unwrap_or(0.0));
        let chans = input.channels(cx.n);
        let key = match &self.key {
            Some(k) => Some(cx.audio("key", k)?.channels(cx.n)),
            None => None,
        };
        let mut f = Follower::new();
        let gain: Vec<f32> = (0..cx.n)
            .map(|i| {
                let det = key.as_ref().unwrap_or(&chans);
                let peak = det.iter().map(|c| c[i].abs()).fold(0.0, f32::max);
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

impl Describe for Limit {
    const BLURB: &'static str = "lookahead peak limiter";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "ceiling_db": -1})
    }
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
