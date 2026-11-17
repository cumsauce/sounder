//! `finish`: the bus chain a designed sound ends with, as one node with a few
//! knobs (AGENTS.md "Layering a designed sound"). It runs existing nodes in
//! order, each only when its field is set: `transient` (punch, tail), `compress`
//! (glue), `shape` tube oversampled 4x plus a 20 Hz high-pass (warmth), `eq`
//! shelves (weight, air), and on a stereo input `multiband` + `width` (the
//! band under 150 Hz summed to mono, the rest widened). No reverb: SFX are
//! dry by default. With no field set the output is the input, bit for bit.

use std::sync::Arc;

use serde::Deserialize;

use super::{
    Compress, EqBand, Equalizer, FilterModeSpec, Multiband, Onepole, PoleMode, Shape, ShapeMode,
    Transient, Width, one,
};
use crate::buf::{Buf, Sig};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// The name the stages read their input by (not a legal node name: the
/// document never sees it).
const STAGE: &str = "<finish stage>";
/// Attack boost at `punch` 1, dB.
const PUNCH_DB: f32 = 8.0;
/// The transient stage's reaction time, seconds: fast enough to catch the
/// first milliseconds of a hit (the `transient` default, 0.02, boosts after
/// the peak has passed).
const PUNCH_SPEED: f32 = 0.005;
/// Width splits here: below it the sound is summed to mono.
const WIDTH_SPLIT_HZ: f32 = 150.0;
/// The weight shelf's and the air shelf's corners, Hz.
const WEIGHT_HZ: f32 = 120.0;
const AIR_HZ: f32 = 6000.0;
/// Largest shelf boost or cut, dB.
const MAX_SHELF_DB: f32 = 12.0;

/// The finishing bus: punch, glue, warmth, weight, air and width in one node.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finish {
    /// Mono or stereo audio: the mixed layers.
    pub input: In,
    /// 0..1: boost on the attack of every hit, up to +8 dB (a `transient`
    /// with `attack_db` 8 x punch, `speed` 0.005). Raises the crest.
    #[serde(default)]
    pub punch: Option<f32>,
    /// Gain on the tail of every hit (the falling part), dB (-12..12; a
    /// `transient` `sustain_db`, default speed): negative tightens a hit, positive lets it
    /// ring.
    #[serde(default)]
    pub tail_db: Option<f32>,
    /// 0..1: compression that pulls the layers together (threshold
    /// 4 + 16 x glue dB under the input's peak, ratio 1.5 + 2.5 x glue,
    /// instant attack, 120 ms release, 6 dB knee). Lowers the crest.
    #[serde(default)]
    pub glue: Option<f32>,
    /// 0..1: tube saturation, oversampled 4x (drive 1 + 2 x warmth, mix
    /// 0.4 x warmth), then a 20 Hz high-pass for the DC the tube leaves.
    #[serde(default)]
    pub warmth: Option<f32>,
    /// Low shelf at 120 Hz, dB (-12..12).
    #[serde(default)]
    pub weight_db: Option<f32>,
    /// High shelf at 6 kHz, dB (-12..12).
    #[serde(default)]
    pub air_db: Option<f32>,
    /// Stereo input only, 0..2: the band above 150 Hz gets this width (0
    /// mono, 1 as is, 2 twice as wide); the band below is summed to mono.
    /// A mono input stays mono: SFX are mono by default, so widen a stereo
    /// mix (a `pan`ned layer, `haas`, `decorrelate`) and finish that.
    #[serde(default)]
    pub width: Option<f32>,
}

impl Describe for Finish {
    const BLURB: &'static str = "the bus a designed sound ends with: punch, glue, warmth, weight / air shelves, width above 150 Hz (no reverb)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "punch": 0.4, "glue": 0.3, "warmth": 0.5, "weight_db": 2})
    }
}

fn unit(field: &str, x: Option<f32>, hi: f32) -> Result<Option<f32>, String> {
    match x {
        None => Ok(None),
        Some(v) if v.is_finite() && (0.0..=hi).contains(&v) => Ok(Some(v)),
        Some(v) => Err(format!("{field}: {v}: give a number between 0 and {hi}")),
    }
}

fn shelf(field: &str, x: Option<f32>) -> Result<Option<f32>, String> {
    match x {
        None => Ok(None),
        Some(v) if v.is_finite() && v.abs() <= MAX_SHELF_DB => Ok(Some(v)),
        Some(v) => Err(format!(
            "{field}: {v}: give dB between -{MAX_SHELF_DB} and {MAX_SHELF_DB}"
        )),
    }
}

/// Runs one stage: `f` evaluates a node whose input is `STAGE`, which reads
/// `buf`; every other input resolves as it does for `finish` itself.
fn stage(
    cx: &Cx,
    buf: Buf,
    f: impl FnOnce(&Cx, In) -> Result<Outs, String>,
) -> Result<Outs, String> {
    let held = Arc::new(buf);
    let get = |i: &In| match i {
        In::Name(s) if s.as_str() == STAGE => Ok(Sig::Buf(held.clone())),
        other => (cx.get)(other),
    };
    let inner = Cx {
        sr: cx.sr,
        n: cx.n,
        seed: cx.seed,
        looped: cx.looped,
        base_dir: cx.base_dir,
        params: cx.params,
        sounds: cx.sounds,
        stack: cx.stack,
        get: &get,
    };
    f(&inner, In::Name(STAGE.into()))
}

/// A buffer's channels (one or two), `n` samples each.
fn chans(b: &Buf, n: usize) -> Vec<Vec<f32>> {
    Sig::Buf(Arc::new(b.clone())).channels(n)
}

/// The default port of a stage's outputs.
fn out(outs: Outs) -> Result<Buf, String> {
    outs.into_iter()
        .next()
        .map(|(_, b)| b)
        .ok_or_else(|| "finish: a stage made no output".to_string())
}

fn port(outs: &Outs, name: &str) -> Result<Buf, String> {
    outs.iter()
        .find(|(p, _)| *p == name)
        .map(|(_, b)| b.clone())
        .ok_or_else(|| format!("finish: a stage has no port `{name}`"))
}

impl Eval for Finish {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let punch = unit("punch", self.punch, 1.0)?;
        let tail_db = shelf("tail_db", self.tail_db)?;
        let glue = unit("glue", self.glue, 1.0)?;
        let warmth = unit("warmth", self.warmth, 1.0)?;
        let weight_db = shelf("weight_db", self.weight_db)?;
        let air_db = shelf("air_db", self.air_db)?;
        let width = unit("width", self.width, 2.0)?;
        let input = cx.audio("input", &self.input)?;
        if width.is_some() && !input.is_stereo() {
            return Err(
                "width: the input is mono: finish keeps a mono sound mono (SFX are mono by default); to widen, make the mix stereo first (a mix input's `pan`, `haas`, `decorrelate`) and finish that, or leave `width` out"
                    .into(),
            );
        }
        let mut buf = Buf::from_channels(input.channels(cx.n));

        // Two transient stages: the attack wants a fast detector (it must
        // catch the first milliseconds), the tail a slow one (on a slow decay
        // a fast detector barely sees the level falling).
        if let Some(p) = punch.filter(|p| *p > 0.0) {
            buf = out(stage(cx, buf, |cx, input| {
                Transient {
                    input,
                    attack_db: Some(PUNCH_DB * p),
                    sustain_db: None,
                    speed: Some(PUNCH_SPEED),
                }
                .eval(cx)
            })?)?;
        }
        if let Some(d) = tail_db.filter(|d| *d != 0.0) {
            buf = out(stage(cx, buf, |cx, input| {
                Transient {
                    input,
                    attack_db: None,
                    sustain_db: Some(d),
                    speed: None,
                }
                .eval(cx)
            })?)?;
        }
        if let Some(g) = glue.filter(|g| *g > 0.0) {
            let peak = chans(&buf, cx.n)
                .iter()
                .flat_map(|c| c.iter())
                .fold(0.0f32, |m, x| m.max(x.abs()));
            if peak > 0.0 {
                let peak_db = crate::gain_to_db(peak);
                buf = out(stage(cx, buf, |cx, input| {
                    Compress {
                        input,
                        threshold_db: peak_db - 4.0 - 16.0 * g,
                        ratio: 1.5 + 2.5 * g,
                        attack: Some(0.0),
                        release: Some(0.12),
                        makeup_db: None,
                        knee_db: Some(6.0),
                        key: None,
                    }
                    .eval(cx)
                })?)?;
            }
        }
        if let Some(w) = warmth.filter(|w| *w > 0.0) {
            buf = out(stage(cx, buf, |cx, input| {
                Shape {
                    input,
                    mode: ShapeMode::Tube,
                    drive: Some(In::Num(1.0 + 2.0 * w)),
                    bits: None,
                    bias: None,
                    points: None,
                    oversample: Some(4),
                    mix: Some(In::Num(0.4 * w)),
                }
                .eval(cx)
            })?)?;
            buf = out(stage(cx, buf, |cx, input| {
                Onepole {
                    input,
                    mode: PoleMode::Hp,
                    freq: In::Num(20.0),
                }
                .eval(cx)
            })?)?;
        }
        let mut bands = Vec::new();
        if let Some(db) = weight_db.filter(|d| *d != 0.0) {
            bands.push(EqBand {
                mode: FilterModeSpec::Lowshelf,
                freq: WEIGHT_HZ,
                gain_db: Some(db),
                q: None,
            });
        }
        if let Some(db) = air_db.filter(|d| *d != 0.0) {
            bands.push(EqBand {
                mode: FilterModeSpec::Highshelf,
                freq: AIR_HZ,
                gain_db: Some(db),
                q: None,
            });
        }
        if !bands.is_empty() {
            buf = out(stage(cx, buf, |cx, input| {
                Equalizer { input, bands }.eval(cx)
            })?)?;
        }
        if let Some(w) = width {
            let split = stage(cx, buf, |cx, input| {
                Multiband {
                    input,
                    crossovers: vec![WIDTH_SPLIT_HZ],
                    bands: None,
                }
                .eval(cx)
            })?;
            let (low, high) = (port(&split, "band0")?, port(&split, "band1")?);
            let wide = out(stage(cx, high, |cx, input| {
                Width {
                    input,
                    amount: In::Num(w),
                }
                .eval(cx)
            })?)?;
            let low = chans(&low, cx.n);
            let wide = chans(&wide, cx.n);
            buf = Buf::Stereo(
                (0..cx.n)
                    .map(|i| {
                        let m = 0.5 * (low[0][i] + low[1][i]);
                        [m + wide[0][i], m + wide[1][i]]
                    })
                    .collect(),
            );
        }
        Ok(one(buf))
    }
}
