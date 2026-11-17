//! The MCP tool surface. Every tool ships a closure to the session thread
//! and awaits its reply, so all state changes happen on one thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData};
use rmcp::{tool, tool_handler, tool_router};
use serde_json::{Value, json};
use sfx_analysis::{Column, DiffColumn, View, describe, listen_png_diff, report};
use sfx_graph::{Audio, DocEdit, Kind, RenderOpts, wav};
use sounder::player::{PlayRequest, PlayerControl, do_play};
use sounder::session::{Annotation, Session, Travel};
use sounder::{catalog, export, feedback};
use tokio::sync::oneshot;

use super::args::*;
use super::bridge::Cmd;

/// What a tool returns: text, optional structured JSON, optional PNGs.
pub(crate) struct Out {
    text: String,
    json: Option<Value>,
    images: Vec<Vec<u8>>,
}

impl Out {
    fn text(t: impl Into<String>) -> Out {
        Out {
            text: t.into(),
            json: None,
            images: Vec::new(),
        }
    }
    fn with_json(mut self, j: Value) -> Out {
        self.json = Some(j);
        self
    }
}

/// Ok, or an error message (with optional JSON shown either way).
type R = Result<Out, (String, Option<Value>)>;

fn fail<T>(e: impl Into<String>) -> Result<T, (String, Option<Value>)> {
    Err((e.into(), None))
}

fn plain<T>(r: Result<T, String>) -> Result<T, (String, Option<Value>)> {
    r.map_err(|e| (e, None))
}

fn to_result(r: Result<Out, (String, Option<Value>)>) -> CallToolResult {
    let (mut res, json) = match r {
        Ok(o) => {
            let mut content = vec![Content::text(o.text)];
            for png in o.images {
                content.push(Content::image(B64.encode(png), "image/png"));
            }
            (CallToolResult::success(content), o.json)
        }
        Err((m, j)) => (CallToolResult::error(vec![Content::text(m)]), j),
    };
    res.structured_content = j_or_none(json);
    res
}

fn j_or_none(j: Option<Value>) -> Option<Value> {
    j
}

pub(crate) struct Tools {
    tx: mpsc::Sender<Cmd>,
    /// The speakers (`--audio` or the window); None when headless.
    player: Option<Arc<dyn PlayerControl>>,
    /// Failures of each tool in a row (for the friction log).
    streaks: Arc<Mutex<HashMap<String, u32>>>,
}

impl Tools {
    pub(crate) fn new(tx: mpsc::Sender<Cmd>, player: Option<Arc<dyn PlayerControl>>) -> Self {
        Self {
            tx,
            player,
            streaks: Arc::default(),
        }
    }

    async fn run(
        &self,
        f: impl FnOnce(&mut Session) -> R + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let cmd = Cmd::Exec(Box::new(move |s: &mut Session| {
            let _ = reply_tx.send(f(s));
        }));
        self.tx
            .send(cmd)
            .map_err(|_| ErrorData::internal_error("the session thread is not running", None))?;
        match tokio::time::timeout(std::time::Duration::from_secs(120), reply_rx).await {
            Ok(Ok(r)) => Ok(to_result(r)),
            Ok(Err(_)) => Err(ErrorData::internal_error(
                "the session dropped the reply",
                None,
            )),
            Err(_) => Err(ErrorData::internal_error(
                "timed out after 120 s waiting for the session: the render may be stuck; retry or restart the server",
                None,
            )),
        }
    }
}

fn parse_kind(k: &Option<String>) -> Result<Option<Kind>, String> {
    match k.as_deref() {
        None => Ok(None),
        Some("oneshot") => Ok(Some(Kind::Oneshot)),
        Some("loop") => Ok(Some(Kind::Loop)),
        Some(o) => Err(format!("kind `{o}`: use \"oneshot\" or \"loop\"")),
    }
}

/// The audio of one node (or the output), cut to the output's length.
fn listen_audio(r: &sfx_graph::Rendered, node: &Option<String>) -> Result<Audio, String> {
    let mut a = r.node_audio(node.as_deref())?;
    if node.is_some() {
        a.truncate(r.audio.len());
    }
    Ok(a)
}

fn do_listen(s: &mut Session, a: ListenArgs) -> R {
    if a.reference.is_some() && (a.seeds.is_some() || a.sweep.is_some() || a.diff == Some(true)) {
        return fail("reference compares one render with one recording: drop seeds / sweep / diff");
    }
    if a.reference_set.is_some()
        && (a.reference.is_some() || a.seeds.is_some() || a.sweep.is_some() || a.diff == Some(true))
    {
        return fail(
            "reference_set compares one render with a set: drop reference / seeds / sweep / diff",
        );
    }
    let opts = s.render_opts();
    let salt = s.generation();
    let name = plain(s.target_name(a.doc.as_deref()))?;
    let sound = plain(s.sound(Some(&name)))?;
    let is_loop = sound.doc.kind == sfx_graph::Kind::Loop;
    let doc = sound.doc.clone();
    let mut cols: Vec<(String, Audio)> = Vec::new();
    match (&a.seeds, &a.sweep) {
        (Some(_), Some(_)) => return fail("give seeds or sweep, not both"),
        (Some(seeds), None) => {
            if seeds.is_empty() {
                return fail("seeds: give at least one, e.g. [1, 2, 3]");
            }
            for sd in seeds {
                let r = plain(doc.render(&RenderOpts {
                    seed: Some(*sd),
                    ..opts.clone()
                }))?;
                cols.push((format!("seed {sd}"), plain(listen_audio(&r, &a.node))?));
            }
        }
        (None, Some(sw)) => {
            if sw.values.is_empty() {
                return fail("sweep.values: give at least one value");
            }
            let is_macro = doc.macros.iter().any(|m| m.name == sw.param);
            if !is_macro && !doc.params.contains_key(&sw.param) {
                let m: Vec<&str> = doc.macros.iter().map(|m| m.name.as_str()).collect();
                let p: Vec<&str> = doc.params.keys().map(String::as_str).collect();
                return fail(format!(
                    "sweep.param `{}` is neither a macro nor a param (macros: {}; params: {})",
                    sw.param,
                    if m.is_empty() {
                        "none".into()
                    } else {
                        m.join(", ")
                    },
                    if p.is_empty() {
                        "none".into()
                    } else {
                        p.join(", ")
                    }
                ));
            }
            for v in &sw.values {
                let r = if is_macro {
                    doc.render(&RenderOpts {
                        macros: [(sw.param.clone(), *v)].into(),
                        ..opts.clone()
                    })
                } else {
                    let mut d = doc.clone();
                    d.params.insert(sw.param.clone(), *v);
                    d.render(&opts)
                };
                let r = plain(r)?;
                cols.push((
                    format!("{}={}", sw.param, v),
                    plain(listen_audio(&r, &a.node))?,
                ));
            }
        }
        (None, None) => {
            let sound = plain(s.sound(Some(&name)))?;
            let r = plain(sound.rendered(&opts, salt))?.clone();
            cols.push((
                a.node.clone().unwrap_or_else(|| name.clone()),
                plain(listen_audio(&r, &a.node))?,
            ));
        }
    }
    if cols.len() > 8 {
        return fail(format!(
            "{} columns: show up to 8 at once (fewer seeds / sweep values)",
            cols.len()
        ));
    }
    let key = format!(
        "{:?}|{:?}|{:?}",
        a.node,
        a.seeds,
        a.sweep.as_ref().map(|s| (&s.param, &s.values))
    );
    let mut note = String::new();
    let mut shown: Vec<(String, Audio)> = cols.clone();
    let mut diff_col: Option<(String, Audio, Audio)> = None;
    let mut ref_json: Option<serde_json::Value> = None;
    let sound = plain(s.sound(Some(&name)))?;
    if a.diff == Some(true) {
        if cols.len() != 1 {
            return fail("diff compares one render with the last one: drop seeds / sweep");
        }
        match sound.last_listen.get(&key) {
            Some(prev) => {
                shown = vec![
                    ("before".to_string(), prev.clone()),
                    ("now".to_string(), cols[0].1.clone()),
                ];
                diff_col = Some((
                    "now minus before (dB)".to_string(),
                    cols[0].1.clone(),
                    prev.clone(),
                ));
            }
            None => note = "diff: no earlier render of this request to compare; this one is stored for next time\n".into(),
        }
    }
    sound.last_listen.insert(key, cols[0].1.clone());
    let mut text = note;
    if let Some(file) = &a.reference {
        let rf = plain(sfx_analysis::reference::load(
            file,
            s.project.as_deref(),
            &s.source_dirs(),
        ))?;
        let c = sfx_analysis::compare(&cols[0].1, &rf);
        text += &format!("== comparison with {file} ==\n{}", c.text());
        shown = vec![
            (cols[0].0.clone(), cols[0].1.clone()),
            ("reference (aligned)".to_string(), c.aligned.clone()),
        ];
        diff_col = Some((
            "sound minus reference (dB)".to_string(),
            c.padded.clone(),
            c.aligned.clone(),
        ));
        ref_json = Some(serde_json::to_value(&c).unwrap_or_default());
    }
    if let Some(set) = &a.reference_set {
        let members = plain(sfx_analysis::refset::load_set(set, s.project.as_deref()))?;
        let c = sfx_analysis::refset::compare_set(&cols[0].1, set, &members);
        text += &format!("== {} ==\n", c.text());
        let near = &c.distances[c.nearest].0;
        shown = vec![
            (cols[0].0.clone(), cols[0].1.clone()),
            (
                format!("nearest: {near} (aligned)"),
                c.nearest_cmp.aligned.clone(),
            ),
        ];
        diff_col = Some((
            "sound minus nearest (dB)".to_string(),
            c.nearest_cmp.padded.clone(),
            c.nearest_cmp.aligned.clone(),
        ));
        ref_json = Some(serde_json::to_value(&c).unwrap_or_default());
    }
    let view = View {
        t: a.zoom.as_ref().and_then(|z| z.t),
        hz: a.zoom.as_ref().and_then(|z| z.hz),
    };
    let dcol = diff_col.as_ref().map(|(l, x, y)| DiffColumn {
        label: l.clone(),
        a: x,
        b: y,
    });
    let png = plain(listen_png_diff(
        &shown
            .iter()
            .map(|(l, au)| Column {
                label: l.clone(),
                audio: au,
            })
            .collect::<Vec<_>>(),
        dcol.as_ref(),
        view,
    ))?;
    let mut descs = Vec::new();
    if a.descriptors != Some(false) {
        for (label, au) in &cols {
            let d = match view.t {
                Some([t0, t1]) => {
                    sfx_analysis::descriptors::descriptors_window(au, t0, t1).with_loop(is_loop)
                }
                None => describe(au).with_loop(is_loop),
            };
            if cols.len() > 1 {
                text += &format!("== {label} ==\n");
            }
            if let Some([t0, t1]) = view.t {
                text += &format!(
                    "(zoom window {t0:.3}-{t1:.3} s: the numbers below describe this window; times count from {t0:.3} s)\n"
                );
            }
            text += &d.table();
            for l in sfx_analysis::measure::extra(au).lines() {
                text += &l;
                text.push('\n');
            }
            descs.push(json!({"label": label, "descriptors": d}));
        }
    }
    let mut out = Out::text(text).with_json(json!({"columns": descs, "reference": ref_json}));
    out.images.push(png);
    Ok(out)
}

fn do_sound_ops(s: &mut Session, a: SoundOpsArgs) -> R {
    let kind = plain(parse_kind(&a.kind))?;
    let mut doc = a.doc.clone();
    if let Some(name) = &a.sound {
        plain(s.ensure_sound(name, kind))?;
        doc = Some(name.clone());
    }
    let edit = DocEdit {
        params: a.params,
        macros: a.macros,
        delete_macros: a.delete_macros,
        kind,
        duration: a.duration.map(|d| d.filter(|d| *d > 0.0)),
        seed: a.seed,
        sample_rate: a.sample_rate,
    };
    let report = plain(s.apply_ops(doc.as_deref(), a.ops, edit))?;
    let mut text = report.text.clone();
    let json = json!({
        "label": report.label,
        "changed": report.changed,
        "added": report.added,
        "changed_nodes": report.changed_nodes,
        "removed": report.removed,
        "seconds": report.rendered.audio.seconds(),
        "peak_db": report.rendered.peak_db,
        "lufs": report.rendered.lufs,
        "warnings": report.rendered.warnings,
    });
    if let Some(e) = a.expect {
        let results = plain(s.check(doc.as_deref(), Some(&e)))?;
        text += "\nexpect:\n";
        text += &report_text(&results);
        if results.iter().any(|c| !c.pass) {
            return Err((text, Some(json)));
        }
    }
    Ok(Out::text(text).with_json(json))
}

fn report_text(results: &[sfx_analysis::CheckResult]) -> String {
    report(results)
}

fn do_export(s: &mut Session, a: ExportArgs) -> R {
    let project = s.project.clone();
    let base = s.base_dir();
    let sources = s.source_dirs();
    let sounds = Some(s.sound_source());
    let sound = plain(s.sound(a.doc.as_deref()))?;
    let bits = match a.bits.unwrap_or(16) {
        16 => wav::Bits::Int16,
        24 => wav::Bits::Int24,
        32 => wav::Bits::Float32,
        b => return fail(format!("bits: {b}: use 16, 24 or 32")),
    };
    let doc = sound.doc.clone();
    let opts = export::ExportOpts {
        dir: export::dir_for(project.as_deref(), a.path.as_deref()),
        variations: a.variations.unwrap_or(1),
        bits,
        sample_rate: a.sample_rate,
        base_dir: base,
        sources,
        sounds,
        catalog_name: sound.catalog_name.clone(),
    };
    let ex = plain(export::export(&doc, &opts))?;
    let mut text = format!("exported `{}` to {}\n", doc.name, opts.dir.display());
    for f in ex.manifest["files"].as_array().into_iter().flatten() {
        text += &format!(
            "- {} (seed {}, {:.3} s, {:.1} LUFS, peak {:.1} dBFS)\n",
            f["file"].as_str().unwrap_or(""),
            f["seed"],
            f["seconds"].as_f64().unwrap_or(0.0),
            f["lufs"].as_f64().unwrap_or(0.0),
            f["peak_db"].as_f64().unwrap_or(0.0),
        );
    }
    text += &format!("manifest: {}\n", ex.manifest_path.display());
    for w in &ex.warnings {
        text += &format!("WARNING {w}\n");
    }
    let inputs = ex.manifest["inputs"].as_array().map_or(0, Vec::len);
    if inputs > 0 {
        text += &format!("{inputs} game input(s) in the manifest (macros with game: true)\n");
    }
    Ok(Out::text(text).with_json(json!({
        "files": ex.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        "manifest_path": ex.manifest_path.display().to_string(),
        "manifest": ex.manifest,
        "warnings": ex.warnings,
    })))
}

fn do_catalog(s: &mut Session, mut a: CatalogArgs) -> R {
    match (&a.name, &a.doc) {
        (Some(n), Some(d)) if n != d => {
            return fail(format!(
                "catalog: `name` (\"{n}\") and `doc` (\"{d}\") differ: they are the same argument, pass one"
            ));
        }
        (None, Some(d)) => a.name = Some(d.clone()),
        _ => {}
    }
    let lib = plain(s.library_dir())?;
    let force = a.force == Some(true);
    match a.action.as_str() {
        "list" => Ok(Out::text(catalog::format_list(&catalog::list(&lib)))),
        "sources" => Ok(Out::text(s.sources_text())),
        "describe" => {
            let Some(n) = &a.name else {
                return fail("describe needs name");
            };
            let entry = catalog::list(&lib).into_iter().find(|e| &e.name == n);
            let doc = plain(catalog::load(&lib, n))?;
            let mut t = format!(
                "`{n}`: {}\n",
                entry.as_ref().map_or(String::new(), |e| format!(
                    "{} [{}]{}",
                    e.description,
                    e.tags.join(", "),
                    e.prompt
                        .as_deref()
                        .map(|p| format!(" prompt: \"{p}\""))
                        .unwrap_or_default()
                ))
            );
            t += &format!(
                "{:?}, {} node(s), {} macro(s)\n",
                doc.kind,
                doc.nodes.len(),
                doc.macros.len()
            );
            for n in &doc.nodes {
                t += &format!("- {}\n", sounder::session::node_line(n));
            }
            for m in &doc.macros {
                t += &format!("knob {} {:.2}: {}\n", m.name, m.value, m.doc);
            }
            Ok(Out::text(t))
        }
        "save" => {
            let opts = s.render_opts();
            let salt = s.generation();
            let sound = plain(s.sound(a.name.as_deref()))?;
            let (length, lufs) = {
                let r = plain(sound.rendered(&opts, salt))?;
                (r.audio.seconds(), r.lufs)
            };
            let name = sound.doc.name.clone();
            let prior = catalog::list(&lib).into_iter().find(|e| e.name == name);
            let entry = catalog::CatalogEntry {
                name: name.clone(),
                description: a.description.unwrap_or_else(|| {
                    prior
                        .as_ref()
                        .map(|e| e.description.clone())
                        .unwrap_or_default()
                }),
                tags: a
                    .tags
                    .unwrap_or_else(|| prior.as_ref().map(|e| e.tags.clone()).unwrap_or_default()),
                prompt: a.prompt.or_else(|| prior.and_then(|e| e.prompt)),
                kind: sound.doc.kind,
                length,
                lufs,
                macros: Vec::new(),
            };
            plain(catalog::save(&lib, &sound.doc, entry))?;
            sound.unsaved = false;
            sound.catalog_name = Some(name.clone());
            s.mark_dirty();
            Ok(Out::text(format!(
                "saved `{name}` to {}",
                lib.join("sounds").display()
            )))
        }
        "load" => {
            let Some(n) = &a.name else {
                return fail("load needs name");
            };
            let doc = plain(catalog::load(&lib, n))?;
            Ok(Out::text(plain(s.open_doc(doc, n, force))?))
        }
        "delete" => {
            let Some(n) = &a.name else {
                return fail("delete needs name");
            };
            plain(catalog::delete(&lib, n))?;
            s.mark_dirty();
            Ok(Out::text(format!(
                "deleted `{n}` from the catalog (an open copy stays open)"
            )))
        }
        "close" => {
            let name = plain(s.target_name(a.name.as_deref()))?;
            Ok(Out::text(plain(s.close_sound(&name, force))?))
        }
        o => fail(format!(
            "action `{o}`: use list, sources, describe, save, load, delete or close"
        )),
    }
}

/// The keys each tool takes, read from its input schema (so they cannot
/// drift from the argument structs).
fn accepted_keys() -> &'static HashMap<String, Vec<String>> {
    static KEYS: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    KEYS.get_or_init(|| {
        Tools::tool_router()
            .list_all()
            .into_iter()
            .map(|t| {
                let keys = t
                    .input_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|p| p.keys().cloned().collect())
                    .unwrap_or_default();
                (t.name.to_string(), keys)
            })
            .collect()
    })
}

/// An error message when the call carries a key its tool does not take.
fn unknown_args(tool: &str, args: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    let args = args?;
    let accepted = accepted_keys().get(tool)?;
    sounder::session::check_tool_args(tool, args, accepted).err()
}

#[tool_router]
impl Tools {
    /// Everything about the sound: kind, length, nodes (one line each, with
    /// what each reads), params, macros (value, default, claim and whether
    /// verify last confirmed it), the knob moves the human made since you
    /// last read this (reading clears them), open comments in full (they are
    /// requests: deal with each, then resolve it), warnings, whether it is
    /// causal, and the descriptor table of the last render. Call it first.
    #[tool]
    async fn summary(
        &self,
        Parameters(a): Parameters<DocOnly>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            let (text, json) = plain(s.summary(a.doc.as_deref()))?;
            Ok(Out::text(text).with_json(json))
        })
        .await
    }

    /// Build or edit a sound: an atomic batch of node upserts (and
    /// `{"type": "delete", "name": "x"}`, one item per node; there is no
    /// top-level `delete`), plus params, macros (the knobs the human
    /// turns to fit it by ear), kind, duration, seed. Creates the sound when
    /// `sound` names a new one. The sound is rendered for real, so a node that
    /// cannot render fails the whole batch and changes nothing. Returns what
    /// changed (`+ ~ -`), length, peak, LUFS and warnings; `expect` runs checks
    /// in the same call (a failed check is an error but the batch stays).
    /// One history step per call that changes something.
    #[tool]
    async fn sound_ops(
        &self,
        Parameters(a): Parameters<SoundOpsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| do_sound_ops(s, a)).await
    }

    /// Listen with your eyes and numbers: one image (a log-frequency
    /// spectrogram with octave lines, the waveform with peak and RMS
    /// envelopes, and the loudness curve; several sounds tile as labelled
    /// columns) plus the descriptor table (level, LUFS, attack / decay, onsets,
    /// pitch, spectral centroid / rolloff / flatness, an octave-band x time
    /// table, modes, stereo). `node` listens to one node, `seeds` tiles
    /// variations, `sweep` tiles a param or macro at several values, `zoom`
    /// magnifies a region, `diff` compares with your previous render of the
    /// same request (before, now, and now minus before in dB), `reference`
    /// compares with a WAV (sound, reference, and their dB difference, plus
    /// the distance and the biggest differences in words).
    #[tool]
    async fn listen(
        &self,
        Parameters(a): Parameters<ListenArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| do_listen(s, a)).await
    }

    /// Run checks, each PASS / FAIL with the measured value (a FAIL says what
    /// to change): peak_db, true_peak_db, lufs, duration, silent_edges, clicks,
    /// dc, finite, loop_seam, bands, pitch, onsets, variations (bare: 4
    /// seeds; compares peak when the output uses peak_db, else loudness),
    /// rebuild, reference {file, max_distance} (STFT distance to a WAV),
    /// reference_set {set, max_distance?} (features against a set of recordings),
    /// macros (every knob keeps its claim), layers (a designed sound: energy
    /// by role, voiced share, ringing partials, crest, tail, no reverb;
    /// {preset: "impact"} or bare in checks), and the synthesis checks
    /// aliasing, inharmonicity {min, max}, modulation {rate_hz} and
    /// roughness {min, max} (these four go in `expect`). With no arguments: finite, peak_db
    /// max -0.3, silent_edges, dc, rebuild, and macros when the sound has
    /// knobs. `checks: ["clicks"]` adds checks that need no settings to those
    /// defaults, `only: ["clicks"]` runs just them, and `expect` (the same
    /// shape as `sound_ops` `expect`) runs checks with settings:
    /// `{"expect": {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1,
    /// "max": 3}, "lufs": {"min": -20, "max": -14}, "clicks": true}}`.
    /// `onsets` also takes `gap: [lo, hi]` seconds (every gap between
    /// consecutive onsets in range: three hits 0.15-0.3 s apart is `{"min": 3,
    /// "max": 3, "gap": [0.15, 0.3]}`). `seeds: N` (2..8) runs the same checks
    /// on renders with seeds 1..N (the document's seed is replaced for those
    /// renders only; nothing is saved) and groups the results by seed
    /// ("seed 3: loop_seam"). Any FAIL makes the result an error.
    #[tool]
    async fn verify(
        &self,
        Parameters(a): Parameters<VerifyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            let mut explicit: Vec<&sfx_analysis::Expect> = vec![&a.expect];
            explicit.extend(a.explicit.as_ref());
            let results =
                plain(s.verify_seeds(a.doc.as_deref(), &a.checks, &a.only, &explicit, a.seeds))?;
            let ok = results.iter().all(|c| c.pass);
            let text = report_text(&results);
            let json = json!({"ok": ok, "checks": results});
            if ok {
                Ok(Out::text(text).with_json(json))
            } else {
                Err((text, Some(json)))
            }
        })
        .await
    }

    /// With no `kind`: every node type, one line each. With a kind (`type` and
    /// `node` also work): an example and the JSON schema of its fields.
    #[tool]
    async fn describe_op(
        &self,
        Parameters(a): Parameters<DescribeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |_| {
            let given: Vec<&String> = [&a.kind, &a.type_, &a.node]
                .into_iter()
                .flatten()
                .collect();
            if given.windows(2).any(|w| w[0] != w[1]) {
                return fail(format!(
                    "describe_op: kind, type and node are the same argument and disagree ({}): give one",
                    given
                        .iter()
                        .map(|g| format!("`{g}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            plain(sfx_graph::describe(given.first().map(|g| g.as_str()))).map(Out::text)
        })
        .await
    }

    /// Start from a real sound: finds the strongest partials of a WAV in the
    /// project or the sources folders and their decay times, and returns them
    /// as a ready `resonators` node (feed it an impulse or a noise burst to
    /// hear the object). `residual: "name.wav"` also writes what the modes
    /// miss (the recording minus them: the hit's noise, rattle, room) into the
    /// sources folder and returns a rebuild that mixes the two.
    #[tool]
    async fn extract_modes(
        &self,
        Parameters(a): Parameters<ExtractModesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            let sources = s.source_dirs();
            let path = sfx_graph::node::resolve_file(&a.file, s.project.as_deref(), &sources);
            let audio = plain(wav::read(&path).map_err(|e| {
                format!("{e} (looked in the project, then the sources folders; `catalog sources` lists the paths to use)")
            }))?;
            if audio.len() < 2048 {
                return fail(format!("{}: only {} samples: give a recording at least a few hundredths of a second long", a.file, audio.len()));
            }
            let count = a.count.unwrap_or(8).clamp(1, 32);
            let (modes, kind) = sfx_analysis::modal::extract_fit(&audio, count);
            let node = sfx_analysis::modes_node_from(&modes, "modes", "hit");
            let n = node["modes"].as_array().map_or(0, Vec::len);
            let mut text = format!(
                "{n} mode(s) from {} ({} Hz, {:.2} s); add it with an excitation named `hit`:\n{}\n",
                a.file,
                audio.sample_rate,
                audio.seconds(),
                serde_json::to_string(&node).unwrap_or_default()
            );
            if kind == sfx_analysis::modal::FitKind::Envelope {
                text += "NOTE the recording is not modal (a slam, a noisy hit: its partials, fitted one by one, cancel each other and would play far louder than it): these are its strongest spectral peaks with the decay of its own envelope around each, levelled to its peak. They are a tonal sketch, not the object's modes; pair them with the residual, or use the recording as a `sample` layer\n";
            }
            if let Ok(over) = sfx_analysis::modal::modes_over_db(&audio, &node)
                && over > sfx_analysis::modal::MODES_OVER_DB
            {
                text += &format!(
                    "WARNING these modes play {over:.0} dB louder than the recording from a unit impulse: {}\n",
                    sfx_analysis::modal::not_modal_advice(&node)
                );
            }
            let Some(name) = a.residual else {
                return Ok(Out::text(text).with_json(node));
            };
            let file = std::path::Path::new(&name);
            if !sfx_graph::node::is_file(&name) || file.components().count() != 1 {
                return fail(format!(
                    "residual: `{name}`: give a file name ending in .wav (no folder): it is written into the sources folder"
                ));
            }
            let Some(dir) = sources.first() else {
                return fail("residual: no sources folder (no project and no library)");
            };
            let (residual, at) = plain(sfx_analysis::modal::residual_after(&audio, &node))?;
            plain(std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display())))?;
            let out = dir.join(file);
            let res_audio = sfx_graph::Audio {
                sample_rate: audio.sample_rate,
                channels: vec![residual],
            };
            plain(wav::write(&out, &res_audio, wav::Bits::Float32))?;
            let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let rec_peak = peak(&audio.mix_mono());
            let res_peak = peak(&res_audio.channels[0]);
            let rebuild = serde_json::json!([
                {"type": "impulse", "name": "hit", "at": at},
                node,
                {"type": "sample", "name": "residual", "file": name},
                {"type": "mix", "name": "rebuild", "inputs": ["modes", "residual"]}
            ]);
            if at < 0.0049 {
                text += &format!(
                    "the recording has only {:.1} ms before its hit: give the output fade_in 0.0005, or the default 2 ms fade-in cuts the hit\n",
                    at * 1000.0
                );
            }
            text += &format!(
                "residual (the recording minus these modes as the resonators play them, from its onset; peak {:.1} dB under the recording's; retuning the modes later leaves their old partials in it) written to {}\nrebuild = modes + residual (the residual keeps {:.1} ms of lead-in, so the impulse fires at {at:.4} s and the default fade-in misses the hit; the sample is found by name in the sources folder):\n{}\n",
                sfx_graph::gain_to_db(rec_peak.max(1e-9)) - sfx_graph::gain_to_db(res_peak.max(1e-9)),
                out.display(),
                at * 1000.0,
                serde_json::to_string(&rebuild).unwrap_or_default()
            );
            Ok(Out::text(text).with_json(serde_json::json!({"modes": node, "residual": out, "rebuild": rebuild})))
        })
        .await
    }

    /// Play the sound on the speakers for the human (node: just that node;
    /// seed: re-render with it; loop; stop: true stops). Needs the window or
    /// `sounder --headless --audio`; otherwise it errors and you use listen.
    /// You cannot hear it: the human does.
    #[tool]
    async fn play(&self, Parameters(a): Parameters<PlayArgs>) -> Result<CallToolResult, ErrorData> {
        let player = self.player.clone();
        self.run(move |s| {
            let Some(p) = player else {
                return fail(
                    "play needs a speaker: run `sounder` (the window) or `sounder --headless --audio`; use listen to check the sound by numbers",
                );
            };
            let req = PlayRequest {
                doc: a.doc,
                node: a.node,
                seed: a.seed,
                looped: a.looping,
                stop: a.stop == Some(true),
            };
            plain(do_play(s, &req, p.as_ref())).map(Out::text)
        })
        .await
    }

    /// Start a new empty sound beside the open ones and make it your active
    /// one; `kind`, `duration` (a loop needs one) and `seed` can be set here.
    /// A name that is already open is refused.
    #[tool]
    async fn new_sound(
        &self,
        Parameters(a): Parameters<NewSoundArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            let kind = plain(parse_kind(&a.kind))?.unwrap_or_default();
            plain(s.new_sound_with(&a.name, kind, a.duration, a.seed)).map(Out::text)
        })
        .await
    }

    /// Saved sounds: action list, sources (the WAVs in the sources folders:
    /// recordings and impulse responses nodes find by file name),
    /// describe (name), save (your active sound,
    /// with description / tags / prompt), load (name; force replaces an open
    /// one), delete (name), close (an open sound; force drops unsaved work).
    #[tool]
    async fn catalog(
        &self,
        Parameters(a): Parameters<CatalogArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| do_catalog(s, a)).await
    }

    /// Write WAV files and a `<name>.sfx.json` manifest (files with length,
    /// LUFS and peak, kind, loop points, the game-facing knobs, seeds, source).
    /// `variations` N writes <name>_1.wav .. <name>_N.wav from seeds 1..N.
    /// Relative paths land in <project>/assets/sounds/.
    #[tool]
    async fn export(
        &self,
        Parameters(a): Parameters<ExportArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| do_export(s, a)).await
    }

    /// Undo your last step (one `sound_ops` call is one step). Another
    /// agent's step is refused unless force.
    #[tool]
    async fn undo(&self, Parameters(a): Parameters<UndoArgs>) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            plain(s.travel(a.doc.as_deref(), Travel::Undo, a.force == Some(true))).map(Out::text)
        })
        .await
    }

    /// Redo: the newest step on this branch.
    #[tool]
    async fn redo(&self, Parameters(a): Parameters<UndoArgs>) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            plain(s.travel(a.doc.as_deref(), Travel::Redo, a.force == Some(true))).map(Out::text)
        })
        .await
    }

    /// Jump to any history step by id (`history` lists them); the sound is
    /// restored exactly.
    #[tool]
    async fn jump(&self, Parameters(a): Parameters<JumpArgs>) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            plain(s.travel(a.doc.as_deref(), Travel::Jump(a.id), a.force == Some(true)))
                .map(Out::text)
        })
        .await
    }

    /// The history tree: one line per step (id, parent, label `+a ~b -c`,
    /// who made it, version) with the cursor marked.
    #[tool]
    async fn history(
        &self,
        Parameters(a): Parameters<HistoryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            plain(s.history_text(a.doc.as_deref(), a.limit.unwrap_or(30))).map(Out::text)
        })
        .await
    }

    /// Comment on the sound (pin it to a node and / or a time and frequency
    /// region), or answer one: `resolve: id` with `text` = what you did (the
    /// human's comments are requests; resolve them, do not delete them), or
    /// `reopen: id`.
    #[tool]
    async fn add_annotation(
        &self,
        Parameters(a): Parameters<AnnotationArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            if let Some(id) = a.resolve {
                let Some(text) = a.text.as_deref().filter(|t| !t.trim().is_empty()) else {
                    return fail("resolve needs text: say what you did about the comment");
                };
                return plain(s.resolve_annotation(a.doc.as_deref(), id, text, false))
                    .map(Out::text);
            }
            if let Some(id) = a.reopen {
                return plain(s.resolve_annotation(a.doc.as_deref(), id, "", true)).map(Out::text);
            }
            let Some(text) = a.text.filter(|t| !t.trim().is_empty()) else {
                return fail("text is required (or resolve / reopen a comment by id)");
            };
            plain(s.add_annotation(
                a.doc.as_deref(),
                Annotation {
                    id: 0,
                    text,
                    by: None,
                    node: a.node,
                    time: a.time,
                    hz: a.hz,
                    resolved: false,
                    reply: None,
                    resolved_by: None,
                },
            ))
            .map(Out::text)
        })
        .await
    }

    /// Recent calls (tool, ok / ERROR, ms). `feedback` records a note for the
    /// maintainers when something surprised you; `report` folds the friction
    /// log into what to fix first.
    #[tool]
    async fn session_log(
        &self,
        Parameters(a): Parameters<SessionLogArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |s| {
            let recent: Vec<String> = s
                .calls
                .iter()
                .rev()
                .take(a.limit.unwrap_or(20))
                .rev()
                .map(|c| {
                    format!(
                        "{} {} ({} ms)",
                        c.tool,
                        if c.ok { "ok" } else { "ERROR" },
                        c.ms
                    )
                })
                .collect();
            let mut text = String::new();
            if let Some(note) = a.feedback.filter(|n| !n.trim().is_empty()) {
                feedback::record(feedback::Entry {
                    kind: "note".into(),
                    tool: s.calls.last().map(|c| c.tool.clone()).unwrap_or_default(),
                    detail: note,
                    recent: recent.iter().rev().take(5).rev().cloned().collect(),
                    ..Default::default()
                });
                text += "noted, thank you\n";
            }
            if a.report == Some(true) {
                text += &feedback::report(&feedback::read(), None);
                text += "\n";
            }
            text += &format!("{} recent call(s):\n{}\n", recent.len(), recent.join("\n"));
            Ok(Out::text(text))
        })
        .await
    }
}

const INSTRUCTIONS: &str = r#"Operating guidance for this sound server:
- You cannot hear. You get numbers and images: summary (state + descriptors), listen (spectrogram / waveform / loudness image + descriptor table), verify (PASS / FAIL checks). The human's comments are the only real ears; the senses get you to a plausible sound, the human decides if it is good. Say what you measured, never that it "sounds" like something.
- Call summary first (and after anything confusing). Open comments are requests: deal with each, then add_annotation {resolve: id, text: what you did}; never delete one.
- Build with sound_ops: one atomic batch of nodes [{"type", "name", ...}] upserted by name; {"type": "delete", "name": "x"} inside `ops` removes one (one item per node; there is no top-level `delete`). Exactly one `output` node. Nodes read others by name ("sweep", or a port: "verb.wet", "tune.events"); signal fields take a number, "$param", "=expr" (a string with $ or number arithmetic is an expression too) or a node (audio-rate modulation: "freq": "sweep"). Time in seconds, frequency in Hz, *_db in dB. `duration: null` (or 0) returns to automatic length; new_sound also takes duration, seed and kind, so a loop is one call. Parse errors name the node and field.
- describe_op lists all 112 node types; describe_op {kind: "filter"} gives an example and schema (`type` and `node` also work). Every tool rejects an argument it does not take and lists the ones it does. Check describe_op before guessing a field: wander takes `range: [lo, hi]` (or min and max), ramp `time` (or duration), impulse `at` is a number or a list, ramp defaults to an exp curve, decay `time` is seconds to fall 60 dB (-20 dB takes a third of it).
- Node families: sources osc (unison sync pm) noise (blue violet velvet crackle) fm wavetable impulse sample; control lfo wander ramp slew sample_hold chaos fbm curve switch pitch_follow centroid_follow; events once pattern poisson bounce burst shatter on_threshold, turned into control by trigger_env event_gate seq notes, or into sound by grains cloud voices; filters filter (slope 12 / 24 / 48) ladder eq onepole comb allpass resonators filterbank vocoder multiband; time delay reverb (model fdn room plate spring) ir convolve; nonlinear shape (oversample) compress (key) limit transient gate sub exciter ringmod; the bus finish (punch tail_db glue warmth weight_db air_db width); modal noise (0..1: each mode becomes a noise band at its frequency, noise_width x f wide; wood: noise 0.6, tilt 4, loss ~1.8; thin metal: noise 0.85, noise_width 0.5, tilt 6; clean modes read as a marimba or a tuned gong; click and footstep (wood) take the same noise: click ~0.75 (a small plastic or metal part), wood footstep ~0.35 with ring ~0.6 and weight ~0.25 (the default board reads slammed), scuff (the roll) and creak (the board) sparingly) and strike_glide (~35 cents: a harder hit); exaggeration subdrop and riser and modal rattle only when the brief asks (rattle on wood reads as plastic); modulation chorus flanger phaser freq_shift; space doppler haas decorrelate pan width; lo-fi crossfade_loop resample tape stutter; spectral pitch_shift (formant) time_stretch spectral (freeze blur gate morph shift stretch smear hps); physical modal (shape plate bar tube membrane string shell; loss, tilt; output normalized) contact friction click, and the ready-made models in the table below, plus air glottis tract syrinx. Start from a model node rather than rebuilding it by hand; for an impact, hit or mechanism it is the body layer of a layered sound (next bullet).
- Designed sounds are layers on a dry bus (AGENTS.md "Layering a designed sound", with a door slam, sword clash and UI recipe): pre (friction / noise brush 5-50 ms before the hit, 1.5-5 kHz), crack (noise x fast decay, bp or hp 2-6 kHz, first 5-25 ms), body (modal of the right material struck by a noise burst, or the model node), weight (brown noise x decay through lp slope 24, 60-350 Hz), after (a small second hit, debris, the body's ring); mix mono, then finish {punch 0..1, tail_db, glue 0..1, warmth 0..1, weight_db, air_db; width above 150 Hz on a stereo mix; no reverb} (or by hand: transient, shape tube oversample 4 mix ~0.2, onepole hp 30-40 Hz), output; macros on finish: glue (claim crest -1), punch (claim crest 1, off on noisy layers), tail_db, weight_db. Rules the listener set: dry and mono by default (no reverb unless the brief names a space); impacts are atonal (no sine sub drop, excite modal with noise, wood loss >= 1.2; listen's voiced near 0); click and hard crumple read as metal, keep them out of wood; add the detail before the hit. One node is still right for ambience, loops, UI blips and single textures.
- Ready-made nodes by brief (briefs not listed: build from osc / noise / filter / env / modal, that is fine): footstep {surface concrete|gravel|grass|wood, weight, pace, shoe, body (pitch), swish} one step (chain it with pattern + sound for a walk) | fire, wind, spark {loop} campfire, gusts, arcs | water {flow}, splash {size, tail, burst} (bright burst, then falling bubbles), water also takes tone (shelf at 2 kHz), bubble {radius_mm} drips (voices) | cloth {motion} fabric / foliage rustle, crumple {density to 400/s, size, hardness} paper / foil / plastic, rolling {speed} into modal | shatter into voices of modal: glass, debris | explosion, gunshot {size or caliber, distance} | engine, motor {rpm: ramp} | tube, string {freq} winds, plucks, bows | click {kind latch|switch|button|lock|clasp} mechanism clicks | thunder {distance, brightness, length, rumble, tail (fade: -40 dB x tail at the end)} | modal {shape, size or f0 (lowest mode, Hz), f_min / f_max band, loss, tilt, thickness, gain_db} struck panels, pipes, bells, keys | vocoder, multiband robot voice, band dynamics | cloud grain swarms | glottis into tract vowels, creature calls.
- Layers: ready-made nodes sit at different levels, so balance a scene (fire + footstep + click) with a gain node (db) per layer before one mix and one peak_db / lufs on the output; contact and modal also take gain_db (-60..60). Variation: a seed changes only what reads it (noise, wander, poisson, voices $rand, grains, the foley / surface / click nodes). An osc chain, a pattern or bubble or bounce without jitter are the same for every seed (export variations: N then warns "bit-identical"): add pattern / bounce jitter, poisson, voices $rand, bubble jitter, then verify expect variations. mix inputs take names or objects {"input", "gain" (number or node), "pan", "at" seconds}: layer a tail with at, balance with gain.
- A oneshot with no duration renders until it decays; a loop needs a duration. Give sounds a decay envelope.
- Output stage: lufs normalizes sustained sounds; a short or peaky one (click, hit, splash) cannot reach a high lufs under the -1 dBFS ceiling, so use peak_db (not both). A missed-lufs warning gives the numbers and the cause. lufs / peak_db are met after the edge fades. The default 2 ms fade-in cuts a hit (a click, a bounce, a coin, any first event) that starts at t = 0 (the render warns, unless the target is met): set fade_in 0.0005 (0.5 ms; silent_edges still passes; 0 fails it) or start it at 0.005. The ceiling limiter holds the TRUE peak (4x oversampled), so a noisy hit's delivered sample peak can sit below peak_db; check true_peak_db. The 5 ms fade-out is on by default.
- Loops (kind loop, with a duration): loop_seam is a default check. Noise loops do NOT always pass its spectral half; end the chain with crossfade_loop, give fire / wind / spark / water loop: true (the default in a loop document), and make lfo / oscillator rates multiples of 1 / duration. A filter (or other recursive node) after a looped source starts from zero state and can break the seam (the render warns): put crossfade_loop after it.
- Verify with sound_ops expect or verify; read the FAIL text, it says what to change. Defaults: finite, peak_db <= -0.3, silent_edges (loop: loop_seam), dc, rebuild (+ macros). verify {checks: ["clicks"]} adds checks that need no settings to the defaults (finite peak_db true_peak_db silent_edges clicks dc loop_seam variations (4 seeds) rebuild macros), verify {only: ["clicks"]} runs just them, and verify {seeds: N} (2..8) re-renders seeds 1..N and groups the results by seed (a plain verify checks only the doc's seed), and `expect` (verify's and sound_ops's, same shape) takes checks with settings: {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1, "max": 3, "spacing_ms": 5, "gap": [0.15, 0.3]} (events closer than 40 ms merge unless spacing_ms is lower: 5 counts dense ticks; gap bounds every spacing between hits, leave 10 ms margin), "lufs": {"min": -20, "max": -14}, "pitch": {"hz": 440}, "clicks": true, "variations": {"seeds": 4, "peak_spread_db": 1}} (variations compares the sample peak when the output uses peak_db, else the loudness: lufs_spread). Also in expect: bands, reference {file, max_distance}, and the synthesis checks aliasing (true or {max_db}), inharmonicity {min, max} cents, modulation {rate_hz, mode "amplitude" or "pitch"}, roughness {min, max} percent. For a hit, slam, knock or blow add verify {checks: ["layers"]} (the impact preset: weight 15-60 % of the energy under 200 Hz, body 30-80 % at 200 Hz-2 kHz, crack >= 1.5 % above 2 kHz, voiced <= 0.3, at most 1 strong partial ringing over 0.6 s, crest <= 23 dB, no reverb / convolve); each FAIL names the layer to add. expect layers {preset, weight, body, crack {min, max}, voiced_max, ringing_max, crest_db, tail_ms {min, max}, dry} overrides one measure. summary warns when 80 % of a oneshot's energy sits in one role band.
- listen's table has: level, time (attack, decay, silence), onsets (40 ms merge) and fine onsets (5 ms, with a time list), the one-line `swell/attack (the headline)`, an onset table (time, strength, attack, -20 dB ms, -60 dB ms (`>N`: still sounding at the next onset), centroid, pitch start>end per hit; `n/a` for inharmonic or unvoiced hits, `glide up/down` for 8-60 ms bursts), a `closely spaced events` line (pairs within 20 ms), modes ("N ignored: slower than the sound"), an `inharmonic:` line from partial ratios (the separate inharmonicity line uses an older method: they can disagree), an `events:` line (strong / weak hits, last event), density (events/s; "density estimate" when ticks overlap) and density by tenth, pitch and pitch by tenth, whole-sound centroid / rolloff / flatness (full-band, and of the occupied band: a band-limited texture reads low then high; verify has no check for it, read the lines at the knob's ends with sweep), energy shares, centroid by tenth, the 8 octave-band x 5 time-segment dB table, attack detail, steadiness (loops), and aliasing / inharmonicity / modulation / roughness lines. Use zoom {t: [from_s, to_s], hz: [low, high]} (either key alone) for attacks (with t the text describes that window, times counted from a), diff after a change, sweep {param, values} to tile a knob, reference (a WAV) for a distance in words.
- Macros are the knobs the human turns to fit the sound by ear. Every sound you build ships with at least one, unasked (a sound with none is unfinished; summary says so): pick the dimension(s) that give the most useful control while staying inside what the user requested (the brief sets the default value and the range ends; never a knob that turns it into a different sound), usually the 1-2 fields that dominate the character (size / pitch, brightness, length, density, intensity). After the sound works, add 1-4 with sound_ops macros: name each with an adjective (brightness, weight, grit, snap, length), map it to the fields it moves with sensible from / to ranges. `claim` [measure, direction] is optional (a knob without one is not swept): centroid, decay, crest, lufs, flatness, swing, inharmonicity, roughness, density (events/s; holds to ~500/s, the counter caps near 590), attack (ms), spacing (median gap between hits, s), ring (ms, median per-hit fall to -20 dB, each hit stopped at the next onset; falls as modal loss rises): layer level -> lufs only with a peak_db output (else flatness or no claim), filter -> centroid, envelope time -> attack / decay, event spacing -> spacing / density, per-hit ring in a knock or typewriter -> ring; decay only for single-hit sounds; decay / swing / crest misread multi-hit sounds: leave the claim off there. Under a density knob a pattern with jitter counts steadier than a poisson. An output.lufs target renormalizes level, so a loudness knob with claim lufs cannot move: use peak_db or another claim. verify sweeps each claimed knob and fails a name that does not do what it says or a knob that barely moves it (under 2 %: widen the map), and sound_ops sweeps a new macro at once and reports a position that fails to render. A map entry may also write a sound param ({"param": "size", "from": 0, "to": 1}); a patch node is addressed as {"node": "voices_name/patch_node", "field": ...} (every voice sees it; the "driven by macro" warning does not look inside patches), or declare a param, use it in the patch and map {"param"}. Env / curve points: {"field": "points", "index": 1, "key": "x" or "y"}; expr initial state: {"field": "state", "key": "y1"}. To replace a macro in one call, list its name in both delete_macros and macros. game: true exports a knob as a runtime input. Read the human's knob moves in summary and keep them as new defaults.
- Reference sets: <project>/refs/<set>/ holds several recordings of one kind of sound. listen {reference_set: "door_slam"} gives the distance to each (nearest against the set's own spread: one recording is a poor target, real recordings sit ~10 dB apart) and eight features (weight / body / crack energy, crest, tail, centroid, voiced, ringing) against the span the recordings cover; verify expect reference_set {set, max_distance?} fails each feature outside it, with the fix.
- Recordings and impulse responses: a relative file (sample file, grains / cloud source, convolve ir, listen / verify reference, extract_modes file) is looked for in the project, then <project>/assets/sources/, then <library>/sources/ (by path, then by file name); catalog {action: "sources"} lists them. convolve resamples an IR at another rate. extract_modes {file, residual: "x.wav"} also writes the recording minus the modes into the sources folder and returns a rebuild (impulse + resonators + sample of the residual) that sums back to the recording. Use only material the project may use (own recordings, CC0).
- Several agents may share one server: each keeps its own active sound; pass doc when working on a specific one. undo refuses another agent's step unless force.
- history / undo / redo / jump move a history tree; one sound_ops call is one step.
- export writes WAVs and a manifest into <project>/assets/sounds/ (variations: N warns when no node reads the seed and the files are identical); catalog saves a sound for later.
- When a call surprises you, tell the maintainers with session_log(feedback: "...").
Full details (every node's fields, recipes, measured limits) live in AGENTS.md next to the source, and docs/nodes/*.md."#;

#[tool_handler]
impl rmcp::ServerHandler for Tools {
    /// Dispatches through the router like the generated version, but tags
    /// each call with its agent (`_meta` "sounder/client", "sounder/doc",
    /// "sounder/project"), says what others changed, and records the call.
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
        let name = request.name.to_string();
        let args = request
            .arguments
            .as_ref()
            .map(|a| feedback::clip(&Value::Object(a.clone()).to_string()))
            .unwrap_or_default();
        let started = std::time::Instant::now();
        let meta = |k: &str| -> Option<String> {
            request
                .meta
                .as_ref()
                .and_then(|m| m.get(k).cloned())
                .or_else(|| context.meta.get(k).cloned())
                .and_then(|v| v.as_str().map(String::from))
        };
        let client = meta("sounder/client");
        let arg_doc = request
            .arguments
            .as_ref()
            .and_then(|a| a.get("doc").or_else(|| a.get("sound")))
            .and_then(Value::as_str)
            .map(String::from);
        let pin = arg_doc.or_else(|| meta("sounder/doc"));
        let project =
            meta("sounder/project").map(|p| (!p.is_empty()).then(|| std::path::PathBuf::from(p)));
        let note_rx = {
            let (c, pin) = (client.clone(), pin.clone());
            let (tx, rx) = oneshot::channel();
            let cmd = Cmd::Exec(Box::new(move |s: &mut Session| {
                let _ = tx.send(s.begin_call(c.as_deref(), pin.as_deref(), project));
            }));
            if self.tx.send(cmd).is_ok() {
                Some(rx)
            } else {
                None
            }
        };
        let mut result = match unknown_args(&name, request.arguments.as_ref()) {
            Some(msg) => Ok(to_result(fail(msg))),
            None => {
                let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
                Self::tool_router().call(tcc).await
            }
        };
        let note = match note_rx {
            Some(rx) => rx.await.ok().flatten(),
            None => None,
        };
        let ok = result.as_ref().is_ok_and(|r| r.is_error != Some(true));
        let error = match &result {
            Err(e) => e.message.to_string(),
            Ok(r) if !ok => r
                .content
                .iter()
                .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                .collect::<Vec<_>>()
                .join("\n"),
            Ok(_) => String::new(),
        };
        let ms = started.elapsed().as_millis();
        let (tool, c2) = (name.clone(), client.clone());
        let _ = self.tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
            s.end_call(c2.as_deref());
            s.log_call(&tool, ok, ms, c2.as_deref());
        })));
        if let (Some(note), Ok(r)) = (note, result.as_mut()) {
            r.content.insert(0, Content::text(note));
        }
        // The friction log: failures (with their streak) and slow calls.
        if let Ok(mut streaks) = self.streaks.lock() {
            if ok {
                streaks.remove(&name);
                if ms > feedback::SLOW_MS {
                    feedback::record(feedback::Entry {
                        kind: "slow".into(),
                        tool: name.clone(),
                        detail: format!("{ms} ms"),
                        args: args.clone(),
                        ..Default::default()
                    });
                }
            } else {
                let n = streaks.entry(name.clone()).or_insert(0);
                *n += 1;
                feedback::record(feedback::Entry {
                    kind: "error".into(),
                    tool: name.clone(),
                    detail: error,
                    args,
                    streak: *n,
                    ..Default::default()
                });
            }
        }
        result
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new("sounder", "0.1.0"))
        .with_instructions(INSTRUCTIONS)
    }
}
