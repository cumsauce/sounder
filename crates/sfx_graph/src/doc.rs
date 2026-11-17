//! A sound document: settings plus a list of raw JSON nodes, and the
//! atomic upsert batch that edits it (modeler's `MatDoc::apply`).
//!
//! History (undo / redo), annotations and the catalog live in the `sounder`
//! crate (phase 2): it wraps [`SoundDoc`] and records a step whenever
//! [`ApplyReport::changed`] is true, labelled with [`ApplyReport::label`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::buf::{Audio, Buf};
use crate::eval::{self, Ctx, Evaluated};
use crate::params::Params;
use crate::{gain_to_db, loudness};

/// On-disk schema version of a sound document.
pub const SOUND_FORMAT_VERSION: u32 = 1;

/// Longest a sound renders when it has no duration and never decays.
pub const MAX_AUTO_SECONDS: f32 = 30.0;
/// Silence below this (dBFS) ends an auto-duration sound.
pub const SILENCE_DB: f32 = -80.0;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Plays once (default). Without a duration it renders until it decays.
    #[default]
    Oneshot,
    /// Repeats seamlessly; needs a duration.
    Loop,
}

/// A named game-facing knob, read as `$name` like a param and recorded in
/// the export manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
    pub default: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// What the knob means to the game ("impact speed, m/s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

impl InputSpec {
    fn clamp(&self, v: f64) -> f64 {
        let v = self.min.map_or(v, |m| v.max(m));
        self.max.map_or(v, |m| v.min(m))
    }
}

/// A procedural sound: a graph of nodes ending in one `output` node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundDoc {
    pub name: String,
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default = "default_rate")]
    pub sample_rate: u32,
    /// Seconds. A oneshot without one renders until its output stays under
    /// -80 dBFS for 50 ms (at most 30 s); a loop needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Raw node JSON, each `{"type", "name", ...}` (unresolved templates).
    #[serde(default)]
    pub nodes: Vec<Value>,
    #[serde(default)]
    pub params: Params,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputSpec>,
    /// Changes with every recorded edit.
    #[serde(default)]
    pub version: u64,
}

fn default_rate() -> u32 {
    48_000
}
fn default_seed() -> u64 {
    1
}

impl Default for SoundDoc {
    fn default() -> Self {
        SoundDoc {
            name: "sound".into(),
            format_version: SOUND_FORMAT_VERSION,
            kind: Kind::Oneshot,
            sample_rate: default_rate(),
            duration: None,
            seed: default_seed(),
            nodes: Vec::new(),
            params: Params::new(),
            inputs: BTreeMap::new(),
            version: 0,
        }
    }
}

/// How to render.
#[derive(Clone, Debug)]
pub struct RenderOpts {
    /// Use the memo cache (false proves a cached render matches a fresh one).
    pub cached: bool,
    /// Render with this seed instead of the document's (variations).
    pub seed: Option<u64>,
    /// Override input values (clamped to their ranges).
    pub inputs: BTreeMap<String, f64>,
    /// Relative file paths (`sample`, `convolve`) resolve here.
    pub base_dir: Option<PathBuf>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts {
            cached: true,
            seed: None,
            inputs: BTreeMap::new(),
            base_dir: None,
        }
    }
}

/// A rendered sound.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// The output node's samples (trimmed when the length was automatic).
    pub audio: Audio,
    /// Every node's buffers at the rendered length (before any trim), for
    /// listening to one node.
    pub eval: Evaluated,
    /// The length was found automatically.
    pub auto_length: bool,
    /// Every node could run live (no lookahead or whole-buffer step).
    pub causal: bool,
    /// Acausal nodes, when not causal.
    pub acausal: Vec<String>,
    pub warnings: Vec<String>,
    pub seed: u64,
    /// Integrated loudness, LUFS, and sample peak, dBFS, of `audio`.
    pub lufs: f32,
    pub peak_db: f32,
}

impl Rendered {
    /// A node's samples as audio (the output's when `node` is None).
    pub fn node_audio(&self, node: Option<&str>) -> Result<Audio, String> {
        match node {
            None => Ok(self.audio.clone()),
            Some(n) => Audio::from_buf(self.audio.sample_rate, &*self.eval.buf(n)?),
        }
    }
}

/// Settings changed alongside a batch of node edits.
#[derive(Clone, Debug, Default)]
pub struct DocEdit {
    pub params: Option<Params>,
    pub inputs: Option<BTreeMap<String, InputSpec>>,
    pub kind: Option<Kind>,
    /// Some(None) clears the duration (automatic length).
    pub duration: Option<Option<f32>>,
    pub seed: Option<u64>,
    pub sample_rate: Option<u32>,
}

/// What [`SoundDoc::apply`] did.
#[derive(Debug, Clone)]
pub struct ApplyReport {
    pub added: Vec<String>,
    pub changed_nodes: Vec<String>,
    pub removed: Vec<String>,
    /// `+a ~b -c`, or "sound settings".
    pub label: String,
    /// The document changed (record a history step).
    pub changed: bool,
    pub rendered: Rendered,
    /// The brief: version, nodes `+ ~ -`, length, peak, LUFS, warnings.
    pub text: String,
}

impl SoundDoc {
    pub fn named(name: &str) -> SoundDoc {
        SoundDoc {
            name: name.into(),
            ..SoundDoc::default()
        }
    }

    pub fn check_format(&self) -> Result<(), String> {
        if self.format_version > SOUND_FORMAT_VERSION {
            return Err(format!(
                "sound `{}` has format_version {} but this build reads up to {SOUND_FORMAT_VERSION}",
                self.name, self.format_version
            ));
        }
        Ok(())
    }

    fn node_name(v: &Value) -> Option<&str> {
        v.get("name").and_then(Value::as_str)
    }

    /// Params plus input values (defaults, or `overrides`), as `$name`s.
    pub fn resolved_params(&self, overrides: &BTreeMap<String, f64>) -> Result<Params, String> {
        let mut p = self.params.clone();
        for (k, spec) in &self.inputs {
            if p.contains_key(k) {
                return Err(format!(
                    "`{k}` is both a param and an input: rename one (inputs are the game-facing knobs)"
                ));
            }
            let v = overrides.get(k).copied().unwrap_or(spec.default);
            p.insert(k.clone(), spec.clamp(v));
        }
        for k in overrides.keys() {
            if !self.inputs.contains_key(k) {
                let known: Vec<&str> = self.inputs.keys().map(String::as_str).collect();
                return Err(format!(
                    "no input named `{k}` (inputs: {})",
                    if known.is_empty() {
                        "none".into()
                    } else {
                        known.join(", ")
                    }
                ));
            }
        }
        Ok(p)
    }

    /// Renders the sound.
    pub fn render(&self, opts: &RenderOpts) -> Result<Rendered, String> {
        let params = self.resolved_params(&opts.inputs)?;
        let seed = opts.seed.unwrap_or(self.seed);
        let sr = self.sample_rate;
        if !(8000..=192_000).contains(&sr) {
            return Err(format!(
                "sample_rate: {sr}: use 8000..192000 (48000 is the default)"
            ));
        }
        let looped = self.kind == Kind::Loop;
        let ctx = |secs: f32| Ctx {
            sample_rate: sr,
            n: ((secs * sr as f32).round() as usize).max(1),
            seed,
            looped,
            base_dir: opts.base_dir.clone(),
        };
        let out_of = |ev: &Evaluated| -> Result<Audio, String> {
            let name = ev.output_name().ok_or_else(|| {
                "no output node: add {\"type\": \"output\", \"name\": \"out\", \"input\": \"<your last node>\"}".to_string()
            })?;
            Audio::from_buf(sr, &*ev.buf(name)?)
        };
        let mut warnings = Vec::new();
        let (ev, mut audio, auto) = match self.duration {
            Some(d) => {
                if !(d > 0.0 && d <= 600.0) {
                    return Err(format!("duration: {d}: give seconds in 0..600"));
                }
                let ev = eval::evaluate(&self.nodes, &params, &ctx(d), opts.cached)?;
                let a = out_of(&ev)?;
                (ev, a, false)
            }
            None if looped => {
                return Err("a loop needs a duration (seconds): set duration in sound_ops".into());
            }
            None => {
                let mut secs = 2.0f32;
                loop {
                    let ev = eval::evaluate(&self.nodes, &params, &ctx(secs), opts.cached)?;
                    let a = out_of(&ev)?;
                    let tail = (0.05 * sr as f32) as usize;
                    let th = crate::db_to_gain(SILENCE_DB);
                    let quiet = a.channels.iter().all(|c| {
                        c[c.len().saturating_sub(tail)..]
                            .iter()
                            .all(|x| x.abs() < th)
                    });
                    if quiet || secs >= MAX_AUTO_SECONDS {
                        if !quiet {
                            warnings.push(format!(
                                "the sound never decays below {SILENCE_DB} dBFS: stopped at {MAX_AUTO_SECONDS} s (give it a decay envelope, or set duration)"
                            ));
                        }
                        break (ev, a, true);
                    }
                    secs = (secs * 2.0).min(MAX_AUTO_SECONDS);
                }
            }
        };
        if auto {
            // Trim the trailing silence, keeping 10 ms.
            let th = crate::db_to_gain(SILENCE_DB);
            let last = (0..audio.len())
                .rev()
                .find(|&i| audio.channels.iter().any(|c| c[i].abs() >= th))
                .unwrap_or(0);
            let keep = (last + 1 + (0.01 * sr as f32) as usize)
                .min(audio.len())
                .max(1);
            audio.truncate(keep);
            // Re-run the output stage at the final length, so its loudness
            // target and edge fades apply to the sound as exported.
            if let Some(p) = ev.nodes.iter().find(|p| p.ty == "output")
                && let crate::node::Node::Output(o) = &p.node
                && let Some(input) = p.body.get("input").and_then(Value::as_str)
            {
                let mut a = Audio::from_buf(sr, &*ev.buf(input)?)?;
                a.truncate(keep);
                audio.channels = o.process(a.channels, sr as f32, looped)?;
            }
        }
        let acausal: Vec<String> = ev
            .nodes
            .iter()
            .filter(|p| !p.node.causal())
            .map(|p| format!("{} ({})", p.name, p.ty))
            .collect();
        warnings.extend(self.warnings(&ev, &audio));
        let lufs = loudness::integrated(&audio.channels, sr);
        if let Some(target) = ev
            .nodes
            .iter()
            .find(|p| p.ty == "output")
            .and_then(|p| p.body.get("lufs"))
            .and_then(Value::as_f64)
            && (f64::from(lufs) - target).abs() > 1.0
        {
            warnings.push(format!(
                "output lufs {target} came out at {lufs:.1} LUFS: the ceiling limiter took the rest back (the sound is too peaky for that loudness: lower the target, raise ceiling_db, or compress / shape before the output)"
            ));
        }
        let peak_db = gain_to_db(audio.peak());
        Ok(Rendered {
            audio,
            causal: acausal.is_empty(),
            acausal,
            eval: ev,
            auto_length: auto,
            warnings,
            seed,
            lufs,
            peak_db,
        })
    }

    fn warnings(&self, ev: &Evaluated, audio: &Audio) -> Vec<String> {
        let mut w = Vec::new();
        let sr = self.sample_rate as f32;
        let read: std::collections::BTreeSet<&str> = ev
            .nodes
            .iter()
            .flat_map(|p| p.reads.iter().map(String::as_str))
            .collect();
        for p in &ev.nodes {
            if p.ty != "output" && !read.contains(p.name.as_str()) {
                w.push(format!(
                    "node `{}` ({}) is not read by anything: mix it into the output or delete it",
                    p.name, p.ty
                ));
            }
            for (field, v) in hz_fields(&p.body) {
                if v > 0.5 * sr {
                    w.push(format!(
                        "node `{}`: {field} {v} Hz is above Nyquist ({} Hz): it is clamped or silent",
                        p.name,
                        0.5 * sr
                    ));
                }
            }
        }
        if let Some(out) = ev.nodes.iter().find(|p| p.ty == "output")
            && out.body.get("lufs").is_none()
            && let Some(input) = out.body.get("input").and_then(Value::as_str)
            && let Ok(b) = ev.buf(input)
        {
            let peak = match &*b {
                Buf::Audio(v) => v.iter().fold(0.0f32, |m, x| m.max(x.abs())),
                Buf::Stereo(v) => v
                    .iter()
                    .fold(0.0f32, |m, x| m.max(x[0].abs()).max(x[1].abs())),
                Buf::Events(_) => 0.0,
            };
            if peak > 1.0 {
                w.push(format!(
                    "the output's input `{input}` peaks at {:+.1} dBFS: the ceiling limiter is squashing it (lower the gain before the output, or set output lufs)",
                    gain_to_db(peak)
                ));
            }
        }
        let peak = audio.peak();
        if peak < crate::db_to_gain(SILENCE_DB) {
            w.push("the output is silent: check the envelope and the gains feeding it".into());
        }
        for (c, ch) in audio.channels.iter().enumerate() {
            let dc = ch.iter().map(|x| f64::from(*x)).sum::<f64>() / ch.len().max(1) as f64;
            if gain_to_db(dc as f32) > -40.0 {
                w.push(format!(
                    "channel {c} has a DC offset of {:.1} dB: add {{\"type\": \"onepole\", \"mode\": \"hp\", \"freq\": 20}} before the output",
                    gain_to_db(dc as f32)
                ));
            }
        }
        if audio.seconds() > 10.0 {
            let frame = (0.01 * sr) as usize;
            let mono = audio.mix_mono();
            let frames: Vec<f32> = mono
                .chunks(frame.max(1))
                .map(|f| (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt())
                .collect();
            let quiet = frames.iter().filter(|r| gain_to_db(**r) < -60.0).count();
            if quiet * 5 > frames.len() * 4 {
                w.push(format!(
                    "the sound is {:.1} s long but over 80% silence: shorten the duration",
                    audio.seconds()
                ));
            }
        }
        w
    }

    /// Applies a batch of node upserts (and `{"type": "delete", "name"}`)
    /// atomically: the result is rendered for real, and on any error nothing
    /// changes. A call that changes nothing records no step.
    pub fn apply(
        &mut self,
        items: Vec<Value>,
        edit: DocEdit,
        opts: &RenderOpts,
    ) -> Result<ApplyReport, String> {
        let started = Instant::now();
        let mut next = self.clone();
        if let Some(p) = edit.params {
            next.params.extend(p);
        }
        if let Some(i) = edit.inputs {
            for (k, spec) in &i {
                if let (Some(a), Some(b)) = (spec.min, spec.max)
                    && a > b
                {
                    return Err(format!("inputs.{k}: min {a} is above max {b}"));
                }
            }
            next.inputs.extend(i);
        }
        if let Some(k) = edit.kind {
            next.kind = k;
        }
        if let Some(d) = edit.duration {
            next.duration = d;
        }
        if let Some(s) = edit.seed {
            next.seed = s;
        }
        if let Some(r) = edit.sample_rate {
            next.sample_rate = r;
        }
        let (mut added, mut changed, mut removed) = (Vec::new(), Vec::new(), Vec::new());
        for (k, item) in items.into_iter().enumerate() {
            let ty = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let Some(name) = Self::node_name(&item).map(str::to_string) else {
                return Err(format!("node #{k} ({ty}): missing \"name\""));
            };
            let at = next
                .nodes
                .iter()
                .position(|n| Self::node_name(n) == Some(&name));
            if ty == "delete" {
                let Some(at) = at else {
                    return Err(format!("node #{k} (delete): no node named `{name}`"));
                };
                next.nodes.remove(at);
                removed.push(name);
                continue;
            }
            match at {
                Some(at) => {
                    let old_ty = next.nodes[at]
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if old_ty != ty && !ty.is_empty() {
                        return Err(format!(
                            "node #{k} ({ty} \"{name}\"): `{name}` is a {old_ty}; delete it first or pick another name"
                        ));
                    }
                    if next.nodes[at] != item {
                        changed.push(name);
                        next.nodes[at] = item;
                    }
                }
                None => {
                    added.push(name);
                    next.nodes.push(item);
                }
            }
        }
        // Render for real: kernel errors (a filter fed an event stream)
        // surface now, and leave the document untouched.
        let rendered = next.render(opts)?;
        let mut cmp = self.clone();
        cmp.version = next.version;
        let same = self.version > 0 && cmp == next;
        let label = {
            let mut l: Vec<String> = Vec::new();
            l.extend(added.iter().map(|n| format!("+{n}")));
            l.extend(changed.iter().map(|n| format!("~{n}")));
            l.extend(removed.iter().map(|n| format!("-{n}")));
            if l.is_empty() {
                "sound settings".to_string()
            } else {
                l.join(" ")
            }
        };
        if !same {
            next.version = self.version + 1;
            next.format_version = SOUND_FORMAT_VERSION;
        }
        *self = next;
        let lv = eval::levels(&rendered.eval.nodes)?;
        let mut text = format!(
            "sound `{}` v{} ({:?}, {} Hz, {:.3} s{}): {} node(s) in {} level(s); {} computed, rest cached; {:.0} ms\n",
            self.name,
            self.version,
            self.kind,
            self.sample_rate,
            rendered.audio.seconds(),
            if rendered.auto_length { " auto" } else { "" },
            rendered.eval.nodes.len(),
            lv.len(),
            rendered.eval.runs,
            started.elapsed().as_secs_f64() * 1000.0
        )
        .replace("Oneshot", "oneshot")
        .replace("Loop", "loop");
        for (sign, list) in [("+", &added), ("~", &changed), ("-", &removed)] {
            if !list.is_empty() {
                text += &format!("{sign} {}\n", list.join(", "));
            }
        }
        text += &format!(
            "peak {:.1} dBFS, {:.1} LUFS, {}\n",
            rendered.peak_db,
            rendered.lufs,
            if rendered.audio.is_stereo() {
                "stereo"
            } else {
                "mono"
            }
        );
        for w in &rendered.warnings {
            text += &format!("WARNING {w}\n");
        }
        Ok(ApplyReport {
            added,
            changed_nodes: changed,
            removed,
            label,
            changed: !same,
            rendered,
            text,
        })
    }
}

/// Numeric frequency fields in a resolved body (for the Nyquist warning).
fn hz_fields(body: &Value) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    if let Some(v) = body.get("freq").and_then(Value::as_f64) {
        out.push(("freq".to_string(), v as f32));
    }
    if let Some(modes) = body.get("modes").and_then(Value::as_array) {
        for m in modes {
            if let Some(hz) = m.get(0).and_then(Value::as_f64) {
                out.push(("mode".to_string(), hz as f32));
            }
        }
    }
    if let Some(v) = body.get("from").and_then(Value::as_f64) {
        out.push(("from".to_string(), v as f32));
    }
    out
}
