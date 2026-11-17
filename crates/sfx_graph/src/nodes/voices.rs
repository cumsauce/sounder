//! `voices` (a sub-patch rendered once per event) and `sound` (an instance of
//! another document): how events become many sounds, and how sounds are
//! layered from separate documents.

use std::collections::BTreeMap;

use rayon::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use super::{non_negative, one, positive};
use crate::buf::{Audio, Buf, Event};
use crate::eval::{Ctx, evaluate};
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{RenderOpts, mix_seed, params};

/// A sub-patch rendered once per event, summed at the event times.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Voices {
    /// An event node (poisson, pattern, bounce, burst, once, on_threshold).
    pub events: In,
    /// The nodes of one voice: a self-contained graph (it cannot read nodes
    /// outside). Its sound is the `output` node if it has one, else its last
    /// node. Inside it, `$value`, `$index`, `$rand` (0..1, seeded per event)
    /// and `$t` (the event's time) are params, plus the ones `map` makes, and
    /// the sound's own params.
    pub patch: Vec<Value>,
    /// Per-event params: {"pitch": "300 + $rand * 600", "amp": "$value * 0.5"}.
    /// Plain expressions (no leading `=`) over `$value $index $rand $t` and
    /// the sound's params; the patch reads each as `$pitch`.
    #[serde(default)]
    pub map: Option<BTreeMap<String, String>>,
    /// Most events rendered (default 64); more is an error, not a silent cut.
    #[serde(default)]
    pub max: Option<usize>,
    /// Seconds each voice renders for (default 1): give it as short as the
    /// voice needs, the work is per voice.
    #[serde(default)]
    pub length: Option<f32>,
    /// Linear gain on every voice (default 1).
    #[serde(default)]
    pub gain: Option<f32>,
    /// Mixed with the document seed and the node name: another draw of
    /// every voice's `$rand` and seeded nodes (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Voices {
    const BLURB: &'static str = "renders a sub-patch once per event (rain, gravel, shards, gunfire), summed at the event times";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "length": 0.3, "map": {"pitch": "300 + $rand * 600"},
            "patch": [{"type": "osc", "name": "tone", "freq": "$pitch", "amp": "$value"},
                      {"type": "decay", "name": "amp", "time": 0.08},
                      {"type": "math", "name": "hit", "op": "mul", "a": "tone", "b": "amp"}]})
    }
}

impl Eval for Voices {
    fn random(&self) -> bool {
        true
    }

    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        let max = self.max.unwrap_or(64);
        if events.len() > max {
            return Err(format!(
                "events: {} events but max is {max}: raise max (it bounds the work), shorten the stream, or lower its rate",
                events.len()
            ));
        }
        if self.patch.is_empty() {
            return Err("patch: give at least one node".into());
        }
        let length = positive("length", self.length.unwrap_or(1.0))?;
        let gain = self.gain.unwrap_or(1.0);
        let vn = ((length * cx.sr).round() as usize).clamp(1, cx.n.max(1));
        let out_name = self
            .patch
            .iter()
            .find(|n| n.get("type").and_then(Value::as_str) == Some("output"))
            .or_else(|| self.patch.last())
            .and_then(|n| n.get("name"))
            .and_then(Value::as_str)
            .ok_or("patch: every node needs a \"name\"")?
            .to_string();
        let map = self.map.clone().unwrap_or_default();
        let voice = |e: &Event| -> Result<Vec<Vec<f32>>, String> {
            let vseed = mix_seed(mix_seed(cx.seed, self.seed), u64::from(e.id));
            let rand = foley_rand(vseed);
            let mut p = cx.params.clone();
            p.insert("value".into(), f64::from(e.value));
            p.insert("index".into(), f64::from(e.id));
            p.insert("rand".into(), rand);
            p.insert("t".into(), e.t);
            let base = p.clone();
            for (k, src) in &map {
                let v = params::eval(src.trim_start_matches('='), &base)
                    .map_err(|er| format!("map.{k}: {er}"))?;
                p.insert(k.clone(), v);
            }
            let nodes: Vec<Value> = self
                .patch
                .iter()
                .map(|n| params::resolve(n, &p))
                .collect::<Result<_, _>>()
                .map_err(|er| format!("patch (event #{}): {er}", e.id))?;
            let vctx = Ctx {
                sample_rate: cx.sr as u32,
                n: vn,
                seed: vseed,
                looped: false,
                base_dir: cx.base_dir.map(std::path::Path::to_path_buf),
                sources: cx.sources.to_vec(),
                params: p,
                sounds: cx.sounds.cloned(),
                stack: cx.stack.to_vec(),
            };
            let ev = evaluate(&nodes, &params::Params::new(), &vctx, true)
                .map_err(|er| format!("patch (event #{}): {er}", e.id))?;
            let buf = ev.buf(&out_name).map_err(|er| format!("patch: {er}"))?;
            Ok(Audio::from_buf(cx.sr as u32, &buf)?.channels)
        };
        // Voices are independent and seeded per event: render them in
        // parallel, then sum in event order so the result never depends on
        // the thread count.
        let rendered: Vec<Result<Vec<Vec<f32>>, String>> = events.par_iter().map(voice).collect();
        let mut voices = Vec::with_capacity(rendered.len());
        for r in rendered {
            voices.push(r?);
        }
        Ok(one(sum_voices(cx, &events, &voices, gain)))
    }
}

fn foley_rand(seed: u64) -> f64 {
    let mut r = crate::dsp::Rng::new(mix_seed(seed, 0x5EED_BEEF));
    f64::from(r.unit())
}

/// Sums each voice at its event time into a mono or stereo buffer.
fn sum_voices(cx: &Cx, events: &[Event], voices: &[Vec<Vec<f32>>], gain: f32) -> Buf {
    let n = cx.n;
    let stereo = voices.iter().any(|v| v.len() >= 2);
    let place = |e: &Event| (e.t.max(0.0) * f64::from(cx.sr)).round() as usize;
    if stereo {
        let mut out = vec![[0.0f32; 2]; n];
        for (e, v) in events.iter().zip(voices) {
            let start = place(e);
            for i in 0..v[0].len().min(n.saturating_sub(start)) {
                out[start + i][0] += v[0][i] * gain;
                out[start + i][1] += v[v.len().min(2) - 1][i] * gain;
            }
        }
        Buf::Stereo(out)
    } else {
        let mut out = vec![0.0f32; n];
        for (e, v) in events.iter().zip(voices) {
            let start = place(e);
            for i in 0..v[0].len().min(n.saturating_sub(start)) {
                out[start + i] += v[0][i] * gain;
            }
        }
        Buf::Audio(out)
    }
}

/// An instance of another sound (open or saved in the catalog).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SoundNode {
    /// The sound's name. Its own `output` stage applies (loudness target,
    /// ceiling), so layer sounds by mixing instances with `mix` gains.
    pub sound: String,
    /// Seconds into this sound where the instance starts (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Override the instance's params.
    #[serde(default)]
    pub params: Option<BTreeMap<String, f64>>,
    /// Override the instance's macro values (0..1).
    #[serde(default)]
    pub macros: Option<BTreeMap<String, f64>>,
    /// Mixed with this sound's seed for the instance (default 0): two
    /// instances of one sound with different seeds differ.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Gain on the instance, dB (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
}

impl Describe for SoundNode {
    const BLURB: &'static str = "an instance of another sound (open or saved): layer a transient, a body and a tail from separate documents";
    fn example() -> serde_json::Value {
        serde_json::json!({"sound": "thud", "at": 0.05, "gain_db": -6})
    }
}

impl Eval for SoundNode {
    fn random(&self) -> bool {
        true
    }

    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let Some(src) = cx.sounds else {
            return Err("sound: this render cannot reach other sounds (it has no sound source: render through the sounder server, or inline the nodes)".into());
        };
        if cx.stack.iter().any(|s| s == &self.sound) {
            return Err(format!(
                "sound: `{}` would instance itself: {} -> {}",
                self.sound,
                cx.stack.join(" -> "),
                self.sound
            ));
        }
        if cx.stack.len() >= crate::instance::MAX_DEPTH {
            return Err(format!(
                "sound: instances nest more than {} deep: {}",
                crate::instance::MAX_DEPTH,
                cx.stack.join(" -> ")
            ));
        }
        let Some(mut doc) = src.get(&self.sound) else {
            let names: Vec<&str> = src.names().iter().map(String::as_str).collect();
            return Err(format!(
                "sound: no sound named `{}` (open or saved sounds: {}); new_sound / catalog load make one available",
                self.sound,
                if names.is_empty() {
                    "none".into()
                } else {
                    names.join(", ")
                }
            ));
        };
        if let Some(p) = &self.params {
            doc.params.extend(p.clone());
        }
        let r = doc
            .render(&RenderOpts {
                seed: Some(mix_seed(cx.seed, self.seed.unwrap_or(0))),
                macros: self.macros.clone().unwrap_or_default(),
                base_dir: cx.base_dir.map(std::path::Path::to_path_buf),
                sources: cx.sources.to_vec(),
                sounds: Some(src.clone()),
                stack: cx.stack.to_vec(),
                ..RenderOpts::default()
            })
            .map_err(|e| format!("sound `{}`: {e}", self.sound))?;
        let g = crate::db_to_gain(self.gain_db.unwrap_or(0.0));
        let start = cx.at(at);
        let chans = &r.audio.channels;
        let fit = |c: &[f32]| {
            let mut v = vec![0.0f32; cx.n];
            for (i, x) in c.iter().take(cx.n.saturating_sub(start)).enumerate() {
                v[start + i] = x * g;
            }
            v
        };
        Ok(one(Buf::from_channels(
            chans.iter().map(|c| fit(c)).collect(),
        )))
    }
}
