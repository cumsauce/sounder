//! A sound document: settings plus a list of raw JSON nodes, and the
//! atomic upsert batch that edits it (modeler's `MatDoc::apply`).
//!
//! History (undo / redo), annotations and the catalog live in the `sounder`
//! crate: it wraps [`SoundDoc`] and records a step whenever
//! [`ApplyReport::changed`] is true, labelled with [`ApplyReport::label`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::buf::{Audio, Buf};
use crate::eval::{self, Ctx, Evaluated};
use crate::macros::{self, Macro};
use crate::params::{self, Params};
use crate::spec::Spec;
use crate::{gain_to_db, loudness};

/// On-disk schema version of a sound document.
pub const SOUND_FORMAT_VERSION: u32 = 1;

/// Longest a sound renders when it has no duration and never decays.
pub const MAX_AUTO_SECONDS: f32 = 30.0;
/// Silence below this (dBFS) ends an auto-duration sound.
pub const SILENCE_DB: f32 = -80.0;

/// The sound's own tail floor, dB under its peak: automatic length stops at
/// the higher of this and [`SILENCE_DB`], so a slow modal ring does not run
/// the length out and the length does not depend on the level.
pub const AUTO_TAIL_DB: f32 = -60.0;

fn auto_threshold(a: &Audio) -> f32 {
    crate::db_to_gain(SILENCE_DB).max(a.peak() * crate::db_to_gain(AUTO_TAIL_DB))
}

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
    /// -80 dBFS or 60 dB under its peak for 50 ms (at most 30 s); a loop needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Raw node JSON, each `{"type", "name", ...}` (unresolved templates).
    #[serde(default)]
    pub nodes: Vec<Value>,
    #[serde(default)]
    pub params: Params,
    /// Named 0..1 knobs (see [`crate::macros`]); `game` ones are the
    /// game-facing inputs.
    #[serde(default)]
    pub macros: Vec<Macro>,
    /// The brief as data: the components the sound is meant to have
    /// (see [`crate::spec`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<Spec>,
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
            macros: Vec::new(),
            spec: None,
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
    /// Override macro values (clamped to 0..1): sweeps, the game's inputs.
    pub macros: BTreeMap<String, f64>,
    /// Relative file paths (`sample`, `convolve`) resolve here.
    pub base_dir: Option<PathBuf>,
    /// Then in these folders: the project's `assets/sources/` and the
    /// library's `sources/` (the session fills them).
    pub sources: Vec<PathBuf>,
    /// How `sound` nodes reach other documents (the session provides it).
    pub sounds: Option<crate::instance::SoundSource>,
    /// The documents already being rendered above this one (instance cycles).
    pub stack: Vec<String>,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts {
            cached: true,
            seed: None,
            macros: BTreeMap::new(),
            base_dir: None,
            sources: Vec::new(),
            sounds: None,
            stack: Vec::new(),
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
    /// Upsert macros by name; a name in `delete_macros` removes one.
    pub macros: Option<Vec<Macro>>,
    pub delete_macros: Vec<String>,
    pub kind: Option<Kind>,
    /// Some(None) clears the duration (automatic length).
    pub duration: Option<Option<f32>>,
    pub seed: Option<u64>,
    pub sample_rate: Option<u32>,
    /// Some(None) removes the spec.
    pub spec: Option<Option<Spec>>,
    /// Edits to single components, after `spec`: each `{"index": i,
    /// ...fields}` merges the fields into component i (`null` clears an
    /// optional one); `index` equal to the count appends a new component.
    pub spec_update: Vec<Value>,
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

    /// The sound's params as evaluated: macros' param entries written at their
    /// values (`overrides` replace macro values).
    pub fn effective_params(&self, overrides: &BTreeMap<String, f64>) -> Params {
        macros::effective_params(&self.params, &self.macros, overrides)
    }

    /// The graph as evaluated: nodes with `$param`s resolved and macros
    /// applied (`overrides` replace macro values).
    pub fn graph(&self, overrides: &BTreeMap<String, f64>) -> Result<Vec<Value>, String> {
        for k in overrides.keys() {
            if !self.macros.iter().any(|m| &m.name == k) {
                let known: Vec<&str> = self.macros.iter().map(|m| m.name.as_str()).collect();
                return Err(format!(
                    "no macro named `{k}` (macros: {})",
                    if known.is_empty() {
                        "none".into()
                    } else {
                        known.join(", ")
                    }
                ));
            }
        }
        let eff = self.effective_params(overrides);
        let resolved: Vec<Value> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(k, n)| params::resolve(n, &eff).map_err(|e| format!("node #{k}: {e}")))
            .collect::<Result<_, _>>()?;
        macros::apply(&resolved, &self.macros, overrides)
    }

    /// Renders the sound.
    pub fn render(&self, opts: &RenderOpts) -> Result<Rendered, String> {
        let nodes = self.graph(&opts.macros)?;
        let eff = self.effective_params(&opts.macros);
        let params = Params::new();
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
            sources: opts.sources.clone(),
            params: eff.clone(),
            sounds: opts.sounds.clone(),
            stack: {
                let mut s = opts.stack.clone();
                s.push(self.name.clone());
                s
            },
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
                let ev = eval::evaluate(&nodes, &params, &ctx(d), opts.cached)?;
                let a = out_of(&ev)?;
                (ev, a, false)
            }
            None if looped => {
                return Err("a loop needs a duration (seconds): set duration in sound_ops".into());
            }
            None => {
                let mut secs = 2.0f32;
                loop {
                    let ev = eval::evaluate(&nodes, &params, &ctx(secs), opts.cached)?;
                    let a = out_of(&ev)?;
                    let tail = (0.05 * sr as f32) as usize;
                    let th = auto_threshold(&a);
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
            let th = auto_threshold(&audio);
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
        warnings.extend(self.output_stage_warnings(&ev, &audio, lufs, looped));
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

    /// The output stage's own warnings: a missed `lufs` target (with the
    /// numbers) and a fade-in that cuts the first hit.
    fn output_stage_warnings(
        &self,
        ev: &Evaluated,
        audio: &Audio,
        lufs: f32,
        looped: bool,
    ) -> Vec<String> {
        let mut w = Vec::new();
        let sr = self.sample_rate as f32;
        if let Some(p) = ev.nodes.iter().find(|p| p.ty == "output")
            && let crate::node::Node::Output(o) = &p.node
            && let Some(input) = p.body.get("input").and_then(Value::as_str)
            && let Ok(buf) = ev.buf(input)
            && let Ok(mut a) = Audio::from_buf(self.sample_rate, &buf)
        {
            a.truncate(audio.len());
            w.extend(o.missed_target_warning(a.channels.clone(), sr, looped, lufs));
            w.extend(o.fade_in_warning(a.channels, sr, looped, &audio.channels, lufs));
        }
        w
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
            if p.ty == "modal" {
                let mat = p
                    .body
                    .get("material")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                if (mat == "wood" || mat == "metal") && p.body.get("noise").is_none() {
                    // Clean modes were heard as a marimba / tuned gong / digital bell in every listen.
                    let hint = if mat == "wood" {
                        "0.6 (tilt 4)"
                    } else {
                        "0.4 (noise_width 0.1, loss 0.3-0.6)"
                    };
                    w.push(format!(
                        "node `{}`: a {mat} modal with no `noise` has clean modes, heard as a tuned instrument: try noise {hint}",
                        p.name
                    ));
                }
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
        if self.kind == Kind::Loop {
            w.extend(loop_seam_warnings(&ev.nodes));
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
            // With `peak_db` the stage scales the input to the target, so a
            // hot input is not squashed: warn only when the delivered peak
            // is short of the target (gain_db pushed it over the ceiling).
            let met = match &ev.nodes.iter().find(|p| p.ty == "output").map(|p| &p.node) {
                Some(crate::node::Node::Output(o)) => o.peak_db.is_some_and(|t| {
                    gain_to_db(audio.peak()) >= t + o.gain_db.unwrap_or(0.0) - 0.2
                }),
                _ => false,
            };
            if peak > 1.0 && !met {
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

    /// Renders each named macro over [`macros::SWEEP`] (the other knobs where
    /// they are); the first render error as "macro `m` at 0.75: error". With
    /// `needs_a_pass`, a macro that fails at every position is not blamed (the
    /// graph is broken, not the knob).
    fn sweep_failure(
        &self,
        names: &[String],
        opts: &RenderOpts,
        needs_a_pass: bool,
    ) -> Option<String> {
        for m in self.macros.iter().filter(|m| names.contains(&m.name)) {
            let mut first: Option<(f64, String)> = None;
            let mut passes = 0;
            for x in macros::SWEEP {
                let at = RenderOpts {
                    macros: [(m.name.clone(), x)].into(),
                    ..opts.clone()
                };
                match self.render(&at) {
                    Ok(_) => passes += 1,
                    Err(e) => first = first.or(Some((x, e))),
                }
            }
            if let Some((x, e)) = first
                && (passes > 0 || !needs_a_pass)
            {
                return Some(format!("macro `{}` at {x}: {e}", m.name));
            }
        }
        None
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
        let param_edits = edit.params.clone().unwrap_or_default();
        if let Some(p) = edit.params {
            next.params.extend(p);
        }
        // A name in both `delete_macros` and `macros` is a replacement: the
        // delete runs first, so the new definition is the one that stays.
        let upserted: Vec<String> = edit
            .macros
            .as_ref()
            .map(|l| l.iter().map(|m| m.name.clone()).collect())
            .unwrap_or_default();
        for name in &edit.delete_macros {
            let before = next.macros.len();
            next.macros.retain(|m| &m.name != name);
            if next.macros.len() == before && !upserted.contains(name) {
                return Err(format!("delete_macros: no macro named `{name}`"));
            }
        }
        if let Some(list) = edit.macros {
            for mut m in list {
                if m.default.is_none() {
                    m.default = Some(m.value);
                }
                // An agent setting a macro sets both where it sits and its default.
                m.value = m.value.clamp(0.0, 1.0);
                match next.macros.iter().position(|x| x.name == m.name) {
                    Some(at) => next.macros[at] = m,
                    None => next.macros.push(m),
                }
            }
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
        if let Some(sp) = edit.spec {
            if let Some(sp) = &sp {
                sp.validate()?;
            }
            next.spec = sp;
        }
        if !edit.spec_update.is_empty() {
            let sp = next.spec.get_or_insert_with(Spec::default);
            sp.update(&edit.spec_update)?;
            sp.validate()?;
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
        // No nodes yet: settings and the spec are kept without a render, so
        // the brief can be written down before the graph is built.
        if next.nodes.is_empty() && next.macros.is_empty() {
            let mut cmp = self.clone();
            cmp.version = next.version;
            let same = cmp == next;
            if !same {
                next.version = self.version + 1;
                next.format_version = SOUND_FORMAT_VERSION;
            }
            let label = if removed.is_empty() {
                "sound settings".to_string()
            } else {
                removed
                    .iter()
                    .map(|n| format!("-{n}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let text = format!(
                "v{} {}: no nodes yet, so nothing is rendered{}\n",
                next.version,
                label,
                if next.spec.is_some() {
                    "; the spec is kept: summary lists the layers to build"
                } else {
                    ""
                }
            );
            let rendered = Rendered {
                audio: Audio {
                    sample_rate: next.sample_rate,
                    channels: vec![vec![0.0]],
                },
                eval: Evaluated {
                    nodes: Vec::new(),
                    ports: Default::default(),
                    runs: 0,
                },
                auto_length: true,
                causal: true,
                acausal: Vec::new(),
                warnings: Vec::new(),
                seed: next.seed,
                lufs: -70.0,
                peak_db: -120.0,
            };
            *self = next;
            return Ok(ApplyReport {
                added,
                changed_nodes: changed,
                removed,
                label,
                changed: !same,
                rendered,
                text,
            });
        }
        // The graph at the macros' current values: plain graph errors come
        // out as they are; one only the macros cause names them, with a hint.
        let hint = |e: String| {
            if e.starts_with("macro ")
                || e.starts_with("with every macro")
                || e.starts_with("with the macros")
            {
                format!(
                    "{e} (fix the macro with sound_ops macros, or remove it with delete_macros)"
                )
            } else {
                e
            }
        };
        macros::validate_shape(&next.nodes, &next.params, &next.macros).map_err(hint)?;
        let eff = macros::effective_params(&next.params, &next.macros, &BTreeMap::new());
        let resolved: Vec<Value> = next
            .nodes
            .iter()
            .enumerate()
            .map(|(k, n)| params::resolve(n, &eff).map_err(|e| format!("node #{k}: {e}")))
            .collect::<Result<_, _>>()?;
        let current = macros::apply(&resolved, &next.macros, &BTreeMap::new()).map_err(hint)?;
        if let Err(e) = eval::parse(&current, &Params::new()) {
            let macros_to_blame =
                !next.macros.is_empty() && eval::parse(&resolved, &Params::new()).is_ok();
            return Err(if macros_to_blame {
                hint(format!("with the macros at their values: {e}"))
            } else {
                e
            });
        }
        macros::validate_ends(&next.nodes, &next.params, &next.macros).map_err(hint)?;
        // Render for real: kernel errors (a filter fed an event stream)
        // surface now, and leave the document untouched.
        let rendered = match next.render(opts) {
            Ok(r) => r,
            // The render at the knobs' values failed: if a knob made here is
            // the cause (it renders at some positions but not this one), say
            // which and where.
            Err(e) => {
                return Err(match next.sweep_failure(&upserted, opts, true) {
                    Some(blame) => hint(blame),
                    None => e,
                });
            }
        };
        // A knob made or redefined here is swept as `verify` would: a render
        // that fails at some position (too many events at 0.75, say) is
        // reported now, with the position, not found later.
        if let Some(blame) = next.sweep_failure(&upserted, opts, false) {
            return Err(hint(blame));
        }
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
        // A macro overrides the fields it maps: an edit to one of them changes
        // nothing audible. Say so (the agent would otherwise think it worked).
        let mut macro_notes: Vec<String> = Vec::new();
        // A macro defined in this very call (or a node added in it) was
        // written together with its map: the agent knows the field is driven.
        let new_macros: Vec<&str> = upserted
            .iter()
            .map(String::as_str)
            .filter(|m| !self.macros.iter().any(|x| x.name == *m))
            .collect();
        for name in added.iter().chain(&changed) {
            let find = |nodes: &[Value]| {
                nodes
                    .iter()
                    .find(|n| Self::node_name(n) == Some(name.as_str()))
                    .cloned()
            };
            let (new, old) = (find(&next.nodes), find(&self.nodes));
            let Some(new) = new else { continue };
            for m in &next.macros {
                let same_call = new_macros.contains(&m.name.as_str())
                    || (upserted.contains(&m.name) && added.contains(name));
                if same_call {
                    continue;
                }
                for e in m.map.iter().filter(|e| &e.node == name) {
                    let (n, o) = (
                        new.get(&e.field),
                        old.as_ref().and_then(|o| o.get(&e.field)),
                    );
                    if n.is_some() && n != o {
                        macro_notes.push(format!(
                            "node `{name}` field `{}` is driven by macro `{}` ({} -> {} over 0..1): your value is ignored; edit the macro (sound_ops macros) or delete it",
                            e.field,
                            m.name,
                            e.from,
                            e.to
                        ));
                    }
                }
            }
        }
        // Likewise a param a macro writes: setting it changes nothing audible.
        for (k, v) in &param_edits {
            if self.params.get(k).is_none_or(|old| old == v) {
                continue; // new, or unchanged
            }
            for m in &next.macros {
                if new_macros.contains(&m.name.as_str()) {
                    continue;
                }
                for e in m.map.iter().filter(|e| e.param.as_deref() == Some(k)) {
                    macro_notes.push(format!(
                        "param `{k}` is driven by macro `{}` ({} -> {} over 0..1): your value is ignored; edit the macro (sound_ops macros) or delete it",
                        m.name, e.from, e.to
                    ));
                }
            }
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
        for n in &macro_notes {
            text += &format!("WARNING {n}\n");
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

/// Filters and other recursive nodes start from zero state, so a node of
/// this kind reading a periodic looped source (`water`, `fire`, `wind`,
/// `spark`) breaks the loop seam unless a `crossfade_loop` follows it.
fn loop_seam_warnings(nodes: &[crate::eval::Parsed]) -> Vec<String> {
    const SOURCES: &[&str] = &["water", "fire", "wind", "spark"];
    const RECURSIVE: &[&str] = &[
        "filter", "onepole", "comb", "allpass", "delay", "reverb", "ladder", "eq", "chorus",
        "flanger", "phaser",
    ];
    use std::collections::{BTreeMap, BTreeSet};
    let by_name: BTreeMap<&str, &crate::eval::Parsed> =
        nodes.iter().map(|p| (p.name.as_str(), p)).collect();
    // Does `name` read (transitively) a looped source, without passing a crossfade_loop?
    fn reaches_source(
        name: &str,
        by_name: &BTreeMap<&str, &crate::eval::Parsed>,
        seen: &mut BTreeSet<String>,
    ) -> Option<String> {
        if !seen.insert(name.to_string()) {
            return None;
        }
        let p = by_name.get(name)?;
        if p.ty == "crossfade_loop" {
            return None;
        }
        if SOURCES.contains(&p.ty.as_str()) {
            return Some(p.name.clone());
        }
        p.reads
            .iter()
            .find_map(|r| reaches_source(r, by_name, seen))
    }
    // Is a crossfade_loop downstream of `name`?
    fn has_crossfade_after(name: &str, nodes: &[crate::eval::Parsed]) -> bool {
        let mut frontier = vec![name.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(n) = frontier.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            for p in nodes {
                if p.reads.contains(&n) {
                    if p.ty == "crossfade_loop" {
                        return true;
                    }
                    frontier.push(p.name.clone());
                }
            }
        }
        false
    }
    let mut out = Vec::new();
    for p in nodes {
        if !RECURSIVE.contains(&p.ty.as_str()) {
            continue;
        }
        let mut seen = BTreeSet::new();
        for r in &p.reads {
            if let Some(src) = reaches_source(r, &by_name, &mut seen)
                && !has_crossfade_after(&p.name, nodes)
            {
                out.push(format!(
                    "node `{}` ({}) filters the looped source `{src}`: its state starts at zero, so the loop seam may click; add a crossfade_loop after it (or keep the source unfiltered)",
                    p.name, p.ty
                ));
                break;
            }
        }
    }
    out
}
