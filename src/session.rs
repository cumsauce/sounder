//! The session: the open sounds, their history, comments and renders; which
//! agent is working in which sound; autosave.
//!
//! A small reimplementation of what modeler's `Session` does for models
//! (`begin_call` / `end_call`, per-client active documents, credit for
//! edits, "what others changed in your doc"), tied to sounds.
//!
//! Everything here is plain synchronous code on one thread; the MCP server
//! reaches it through a channel (`bridge`), so the window (phase 5) can drain
//! the same commands on its own loop.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sfx_analysis::{CheckCx, CheckResult, Descriptors, Expect};
use sfx_graph::{ApplyReport, Audio, DocEdit, Kind, KnobMove, RenderOpts, Rendered, SoundDoc};

use crate::history::{History, Step};
use crate::store;

/// Autosave happens this long after the last edit.
pub const AUTOSAVE_AFTER: Duration = Duration::from_millis(500);

/// A comment from the human (or an agent): on a node, a time / frequency
/// region, or the sound as a whole.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub id: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Seconds [t0, t1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<[f32; 2]>,
    /// Hz [f0, f1].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hz: Option<[f32; 2]>,
    #[serde(default)]
    pub resolved: bool,
    /// What was done about it, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
}

impl Annotation {
    /// What it is on: `node lp`, `0.06-0.14 s 751-5000 Hz`, or `the sound`.
    pub fn target(&self) -> String {
        let mut parts = Vec::new();
        if let Some(n) = &self.node {
            parts.push(format!("node `{n}`"));
        }
        if let Some([a, b]) = self.time {
            parts.push(format!("{a:.2}-{b:.2} s"));
        }
        if let Some([a, b]) = self.hz {
            parts.push(format!("{a:.0}-{b:.0} Hz"));
        }
        if parts.is_empty() {
            "the sound".into()
        } else {
            parts.join(" ")
        }
    }

    pub fn line(&self) -> String {
        let mut s = format!(
            "#{} on {}{}: '{}'",
            self.id,
            self.target(),
            self.by
                .as_deref()
                .map(|b| format!(" by {b}"))
                .unwrap_or_default(),
            self.text
        );
        if self.resolved {
            s += &format!(
                " -> resolved{}: '{}'",
                self.resolved_by
                    .as_deref()
                    .map(|b| format!(" by {b}"))
                    .unwrap_or_default(),
                self.reply.as_deref().unwrap_or("")
            );
        }
        s
    }
}

/// One open sound: the document, its history, comments, the human's knob
/// moves, and what is cached from rendering it.
#[derive(Debug)]
pub struct Sound {
    pub doc: SoundDoc,
    pub history: History<Step>,
    pub annotations: Vec<Annotation>,
    /// The human's knob moves since an agent last read `summary`.
    pub knob_moves: Vec<KnobMove>,
    /// Differs from what was last saved to the catalog.
    pub unsaved: bool,
    /// The catalog name it was loaded from or saved as.
    pub catalog_name: Option<String>,
    render: Option<(u64, Rendered)>,
    /// The previous render per `listen` request, for `diff`.
    pub last_listen: HashMap<String, Audio>,
    /// The last macros check result per macro: (doc hash, line).
    pub macro_results: BTreeMap<String, (u64, bool, String)>,
}

/// What goes to disk for one sound (autosave).
#[derive(Serialize, Deserialize)]
struct SavedSound {
    doc: SoundDoc,
    history: History<Step>,
    #[serde(default)]
    annotations: Vec<Annotation>,
    #[serde(default)]
    knob_moves: Vec<KnobMove>,
    #[serde(default)]
    unsaved: bool,
    #[serde(default)]
    catalog_name: Option<String>,
}

fn hash_of<T: Serialize>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    h.finish()
}

impl Sound {
    pub fn new(name: &str, kind: Kind, by: Option<&str>) -> Sound {
        let mut doc = SoundDoc::named(name);
        doc.kind = kind;
        let mut history = History::default();
        history.record(Step(doc.clone()), "new sound");
        if let Some(n) = history.nodes.last_mut() {
            n.author = by.map(String::from);
        }
        Sound {
            doc,
            history,
            annotations: Vec::new(),
            knob_moves: Vec::new(),
            unsaved: true,
            catalog_name: None,
            render: None,
            last_listen: HashMap::new(),
            macro_results: BTreeMap::new(),
        }
    }

    /// A sound loaded from a saved document (history starts at it).
    pub fn from_doc(doc: SoundDoc, by: Option<&str>) -> Sound {
        let mut s = Sound::new(&doc.name.clone(), doc.kind, by);
        s.doc = doc.clone();
        s.history = History::default();
        s.history.record(Step(doc), "loaded");
        if let Some(n) = s.history.nodes.last_mut() {
            n.author = by.map(String::from);
        }
        s.unsaved = false;
        s
    }

    fn from_saved(s: SavedSound) -> Sound {
        Sound {
            doc: s.doc,
            history: s.history,
            annotations: s.annotations,
            knob_moves: s.knob_moves,
            unsaved: s.unsaved,
            catalog_name: s.catalog_name,
            render: None,
            last_listen: HashMap::new(),
            macro_results: BTreeMap::new(),
        }
    }

    fn saved(&self) -> SavedSound {
        SavedSound {
            doc: self.doc.clone(),
            history: self.history.clone(),
            annotations: self.annotations.clone(),
            knob_moves: self.knob_moves.clone(),
            unsaved: self.unsaved,
            catalog_name: self.catalog_name.clone(),
        }
    }

    /// The doc hash a cached render or check result is keyed by.
    pub fn doc_hash(&self) -> u64 {
        hash_of(&self.doc)
    }

    /// The (cached) render of the document as it stands.
    pub fn rendered(&mut self, base_dir: Option<&Path>) -> Result<&Rendered, String> {
        let key = hash_of(&(&self.doc, base_dir));
        if self.render.as_ref().is_none_or(|(k, _)| *k != key) {
            let r = self.doc.render(&RenderOpts {
                base_dir: base_dir.map(Path::to_path_buf),
                ..RenderOpts::default()
            })?;
            self.render = Some((key, r));
        }
        match &self.render {
            Some((_, r)) => Ok(r),
            None => Err("no render".into()),
        }
    }

    /// Stores a render the caller already made for this document.
    pub fn set_render(&mut self, base_dir: Option<&Path>, r: Rendered) {
        self.render = Some((hash_of(&(&self.doc, base_dir)), r));
    }

    pub fn open_annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.annotations.iter().filter(|a| !a.resolved)
    }

    /// Re-hangs the document on the history node `id` (keeps comments and
    /// the cursor's history).
    fn restore(&mut self, doc: SoundDoc) {
        self.doc = doc;
        self.unsaved = true;
        self.render = None;
    }
}

/// What a call credits and notes for one connected agent.
#[derive(Debug, Default)]
struct ClientState {
    active: Option<String>,
    /// History node count of each sound at this client's last call.
    seen: HashMap<String, usize>,
}

/// The tool call log entry (for `session_log`).
#[derive(Clone, Debug)]
pub struct CallRecord {
    pub tool: String,
    pub ok: bool,
    pub ms: u128,
}

/// All open sounds.
#[derive(Debug, Default)]
pub struct Session {
    pub sounds: BTreeMap<String, Sound>,
    /// Open order, oldest first.
    pub order: Vec<String>,
    clients: HashMap<String, ClientState>,
    /// The client making the current call.
    current: Option<String>,
    /// The sound untagged callers work in.
    shared_active: Option<String>,
    pub project: Option<PathBuf>,
    pub calls: Vec<CallRecord>,
    dirty_since: Option<Instant>,
    /// This server may write the autosave (it holds `session.lock`).
    pub autosave_ok: bool,
    /// Autosave file slot (`session.<slot>.json`) when another server owns the main one.
    pub slot: Option<String>,
}

impl Session {
    pub fn new(project: Option<PathBuf>) -> Session {
        let s = Session {
            autosave_ok: true,
            project,
            ..Session::default()
        };
        s.point_feedback();
        s
    }

    fn point_feedback(&self) {
        if let Ok(d) = self.library_dir() {
            crate::feedback::set_dir(d);
        }
    }

    pub fn library_dir(&self) -> Result<PathBuf, String> {
        store::library_dir(self.project.as_deref())
    }

    // ------------------------------------------------------- bookkeeping

    /// Start of a call: switches to the caller's sound, and says what other
    /// agents changed in it since the caller's last call.
    pub fn begin_call(
        &mut self,
        client: Option<&str>,
        pin: Option<&str>,
        project: Option<Option<PathBuf>>,
    ) -> Option<String> {
        if let Some(p) = project
            && p != self.project
        {
            self.project = p;
            self.point_feedback();
        }
        self.current = client.map(String::from);
        let c = client?;
        let state = self.clients.entry(c.to_string()).or_default();
        if let Some(p) = pin
            && self.sounds.contains_key(p)
        {
            state.active = Some(p.to_string());
        }
        let name = state.active.clone()?;
        let sound = self.sounds.get(&name)?;
        let seen = *state.seen.get(&name)?;
        let others: Vec<String> = sound
            .history
            .nodes
            .iter()
            .skip(seen)
            .filter(|n| n.author.as_deref() != Some(c))
            .map(|n| match &n.author {
                Some(a) => format!("{} (by {a})", n.label),
                None => n.label.clone(),
            })
            .collect();
        if others.is_empty() {
            None
        } else {
            Some(format!(
                "NOTE: since your last call, others changed `{name}`: {}",
                others.join("; ")
            ))
        }
    }

    pub fn end_call(&mut self, client: Option<&str>) {
        if let Some(c) = client
            && let Some(state) = self.clients.get_mut(c)
            && let Some(name) = state.active.clone()
            && let Some(sound) = self.sounds.get(&name)
        {
            state.seen.insert(name, sound.history.nodes.len());
        }
        self.current = None;
    }

    pub fn client(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// The caller's active sound name.
    pub fn active_name(&self) -> Option<String> {
        match &self.current {
            Some(c) => self.clients.get(c).and_then(|s| s.active.clone()),
            None => self.shared_active.clone(),
        }
        .or_else(|| self.shared_active.clone())
        .filter(|n| self.sounds.contains_key(n))
    }

    fn set_active(&mut self, name: &str) {
        self.shared_active = Some(name.to_string());
        if let Some(c) = &self.current {
            self.clients.entry(c.clone()).or_default().active = Some(name.to_string());
        }
    }

    /// Resolves the target sound: `doc` (must be open), else the caller's active.
    pub fn target_name(&self, doc: Option<&str>) -> Result<String, String> {
        match doc {
            Some(d) => {
                if self.sounds.contains_key(d) {
                    Ok(d.to_string())
                } else {
                    Err(self.not_open(d))
                }
            }
            None => self.active_name().ok_or_else(|| {
                if self.sounds.is_empty() {
                    "no sound is open: call new_sound {name} (or sound_ops with sound: <name>), or catalog load".into()
                } else {
                    format!(
                        "no active sound: pass doc: one of {}",
                        self.order.join(", ")
                    )
                }
            }),
        }
    }

    fn not_open(&self, d: &str) -> String {
        format!(
            "`{d}` is not open (open sounds: {}); new_sound creates one, catalog load opens a saved one",
            if self.order.is_empty() {
                "none".into()
            } else {
                self.order.join(", ")
            }
        )
    }

    /// The open sound `name` (the name was already checked by `target_name`).
    fn open_mut(&mut self, name: &str) -> Result<&mut Sound, String> {
        if !self.sounds.contains_key(name) {
            return Err(self.not_open(name));
        }
        self.sounds.get_mut(name).ok_or_else(String::new)
    }

    pub fn sound(&mut self, doc: Option<&str>) -> Result<&mut Sound, String> {
        let n = self.target_name(doc)?;
        self.open_mut(&n)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        self.project.clone()
    }

    // ---------------------------------------------------------- sounds

    /// Opens a new sound beside the others and makes it the caller's active.
    pub fn new_sound(&mut self, name: &str, kind: Kind) -> Result<String, String> {
        valid_sound_name(name)?;
        if self.sounds.contains_key(name) {
            return Err(format!(
                "`{name}` is already open: pick another name, or work on it with doc: \"{name}\""
            ));
        }
        let by = self.current.clone();
        self.sounds
            .insert(name.to_string(), Sound::new(name, kind, by.as_deref()));
        self.order.push(name.to_string());
        self.set_active(name);
        self.mark_dirty();
        Ok(format!(
            "opened `{name}` ({}): add nodes with sound_ops; describe_op lists the node types",
            if kind == Kind::Loop {
                "loop: needs a duration"
            } else {
                "oneshot: its length follows the decay"
            }
        ))
    }

    /// Opens (or switches to) `sound`: creates it when missing.
    pub fn ensure_sound(&mut self, name: &str, kind: Option<Kind>) -> Result<(), String> {
        if self.sounds.contains_key(name) {
            self.set_active(name);
            Ok(())
        } else {
            self.new_sound(name, kind.unwrap_or_default()).map(|_| ())
        }
    }

    /// Opens a saved document as a sound; `force` replaces an open one.
    pub fn open_doc(
        &mut self,
        doc: SoundDoc,
        catalog_name: &str,
        force: bool,
    ) -> Result<String, String> {
        let name = doc.name.clone();
        if self.sounds.contains_key(&name) && !force {
            return Err(format!(
                "`{name}` is already open: close it first, or pass force: true to replace it with the saved copy"
            ));
        }
        let by = self.current.clone();
        let mut sound = Sound::from_doc(doc, by.as_deref());
        sound.catalog_name = Some(catalog_name.to_string());
        if !self.order.contains(&name) {
            self.order.push(name.clone());
        }
        self.sounds.insert(name.clone(), sound);
        self.set_active(&name);
        self.mark_dirty();
        Ok(format!("opened `{name}` from the catalog"))
    }

    /// Closes an open sound (unsaved work needs `force`).
    pub fn close_sound(&mut self, name: &str, force: bool) -> Result<String, String> {
        let Some(sound) = self.sounds.get(name) else {
            return Err(self.not_open(name));
        };
        if sound.unsaved && !force {
            return Err(format!(
                "`{name}` has unsaved work: catalog save it first, or pass force: true to drop it"
            ));
        }
        self.sounds.remove(name);
        self.order.retain(|n| n != name);
        if self.shared_active.as_deref() == Some(name) {
            self.shared_active = self.order.last().cloned();
        }
        for c in self.clients.values_mut() {
            if c.active.as_deref() == Some(name) {
                c.active = None;
            }
        }
        self.mark_dirty();
        Ok(format!("closed `{name}`"))
    }

    /// Applies a `sound_ops` batch: one history step when anything changed.
    pub fn apply_ops(
        &mut self,
        doc: Option<&str>,
        items: Vec<Value>,
        edit: DocEdit,
    ) -> Result<ApplyReport, String> {
        let by = self.current.clone();
        let base = self.base_dir();
        let name = self.target_name(doc)?;
        let sound = self.open_mut(&name)?;
        let opts = RenderOpts {
            base_dir: base.clone(),
            ..RenderOpts::default()
        };
        let report = sound.doc.apply(items, edit, &opts)?;
        if report.changed {
            sound
                .history
                .record(Step(sound.doc.clone()), report.label.clone());
            if let Some(n) = sound.history.nodes.last_mut() {
                n.author = by;
            }
            sound.unsaved = true;
            // Knob moves are about the old graph.
            sound.macro_results.clear();
        }
        sound.set_render(base.as_deref(), report.rendered.clone());
        self.mark_dirty();
        Ok(report)
    }

    /// Runs the checks (`expect`, or the defaults) on the sound as it stands.
    pub fn check(
        &mut self,
        doc: Option<&str>,
        expect: Option<&Expect>,
    ) -> Result<Vec<CheckResult>, String> {
        let base = self.base_dir();
        let name = self.target_name(doc)?;
        let sound = self.open_mut(&name)?;
        let defaults;
        let e = match expect {
            Some(e) => e,
            None => {
                defaults = Expect::defaults_for(&sound.doc);
                &defaults
            }
        };
        let opts = RenderOpts {
            base_dir: base.clone(),
            ..RenderOpts::default()
        };
        let hash = sound.doc_hash();
        let doc_copy = sound.doc.clone();
        let rendered = sound.rendered(base.as_deref())?.clone();
        let results = sfx_analysis::run(
            e,
            &CheckCx {
                doc: &doc_copy,
                rendered: &rendered,
                opts: &opts,
            },
        )?;
        for r in &results {
            if let Some(m) = r.name.strip_prefix("macro ") {
                sound
                    .macro_results
                    .insert(m.to_string(), (hash, r.pass, r.detail.clone()));
            }
        }
        Ok(results)
    }

    /// A knob moved by the human (or the window): sets the macro's value and
    /// records the move for the agent. Not a history step.
    pub fn move_knob(&mut self, doc: Option<&str>, name: &str, value: f64) -> Result<(), String> {
        let sound = self.sound(doc)?;
        let Some(m) = sound.doc.macros.iter_mut().find(|m| m.name == name) else {
            let known: Vec<&str> = sound.doc.macros.iter().map(|m| m.name.as_str()).collect();
            return Err(format!(
                "no macro named `{name}` (macros: {})",
                known.join(", ")
            ));
        };
        let value = value.clamp(0.0, 1.0);
        let from = m.value;
        m.value = value;
        match sound.knob_moves.iter_mut().find(|k| k.name == name) {
            Some(k) => {
                // Moves merge: from stays what the agent last saw.
                k.to = value;
            }
            None => sound.knob_moves.push(KnobMove {
                name: name.to_string(),
                from,
                to: value,
            }),
        }
        sound.knob_moves.retain(|k| (k.to - k.from).abs() > 1e-9);
        sound.unsaved = true;
        sound.render = None;
        sound.macro_results.clear();
        self.mark_dirty();
        Ok(())
    }

    // --------------------------------------------------------- history

    fn refuse_foreign(&self, sound: &Sound, id: Option<usize>, force: bool) -> Result<(), String> {
        let (Some(me), Some(id)) = (self.current.as_deref(), id) else {
            return Ok(());
        };
        if let Some(n) = sound.history.nodes.get(id)
            && let Some(a) = n.author.as_deref()
            && a != me
            && !force
        {
            return Err(format!(
                "history step #{id} (`{}`) was made by {a}: undoing it would revert their work; pass force: true if you mean to",
                n.label
            ));
        }
        Ok(())
    }

    /// Moves the history cursor: undo, redo or jump.
    pub fn travel(
        &mut self,
        doc: Option<&str>,
        how: Travel,
        force: bool,
    ) -> Result<String, String> {
        let name = self.target_name(doc)?;
        let current = self.current.clone();
        let sound = self.sounds.get(&name).ok_or_else(|| self.not_open(&name))?;
        let cur = sound.history.current;
        match how {
            Travel::Undo => self.refuse_foreign(sound, cur, force)?,
            Travel::Jump(id) => {
                // Jumping away from another agent's step is the same as undoing it.
                self.refuse_foreign(sound, cur, force)?;
                let _ = id;
            }
            Travel::Redo => {}
        }
        let _ = current;
        let sound = self.open_mut(&name)?;
        let moved = match how {
            Travel::Undo => sound
                .history
                .undo()
                .ok_or("nothing to undo: this is the first step")?,
            Travel::Redo => sound
                .history
                .redo()
                .ok_or("nothing to redo: this is the newest step on this branch")?,
            Travel::Jump(id) => sound.history.jump(id)?,
        };
        let (id, step) = moved;
        sound.restore(step.0);
        let label = sound.history.nodes[id].label.clone();
        self.mark_dirty();
        Ok(format!(
            "now at history step #{id} (`{label}`), version {}",
            self.sounds[&name].doc.version
        ))
    }

    // ------------------------------------------------------ annotations

    pub fn add_annotation(
        &mut self,
        doc: Option<&str>,
        mut a: Annotation,
    ) -> Result<String, String> {
        let by = self.current.clone();
        let sound = self.sound(doc)?;
        if let Some(n) = &a.node
            && !sound
                .doc
                .nodes
                .iter()
                .any(|x| x.get("name").and_then(Value::as_str) == Some(n.as_str()))
        {
            let names: Vec<&str> = sound
                .doc
                .nodes
                .iter()
                .filter_map(|x| x.get("name").and_then(Value::as_str))
                .collect();
            return Err(format!("no node named `{n}` (nodes: {})", names.join(", ")));
        }
        a.id = sound.annotations.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        a.by = a.by.or(by);
        let line = a.line();
        sound.annotations.push(a);
        sound.unsaved = true;
        self.mark_dirty();
        Ok(format!("added {line}"))
    }

    pub fn resolve_annotation(
        &mut self,
        doc: Option<&str>,
        id: u32,
        reply: &str,
        reopen: bool,
    ) -> Result<String, String> {
        let by = self.current.clone();
        let sound = self.sound(doc)?;
        let Some(a) = sound.annotations.iter_mut().find(|a| a.id == id) else {
            let ids: Vec<String> = sound
                .annotations
                .iter()
                .map(|a| format!("#{}", a.id))
                .collect();
            return Err(format!("no comment #{id} (comments: {})", ids.join(", ")));
        };
        if reopen {
            a.resolved = false;
            a.reply = None;
            a.resolved_by = None;
        } else {
            a.resolved = true;
            a.reply = Some(reply.to_string());
            a.resolved_by = by;
        }
        let line = a.line();
        sound.unsaved = true;
        self.mark_dirty();
        Ok(format!("{}{line}", if reopen { "reopened " } else { "" }))
    }

    // ------------------------------------------------------- persistence

    fn autosave_path(&self) -> Result<PathBuf, String> {
        let name = match &self.slot {
            Some(s) => format!("session.{s}.json"),
            None => "session.json".into(),
        };
        Ok(self.library_dir()?.join(name))
    }

    /// Writes the autosave once edits have paused (or `force`).
    pub fn flush_autosave(&mut self, force: bool) {
        let Some(since) = self.dirty_since else {
            return;
        };
        if !force && since.elapsed() < AUTOSAVE_AFTER {
            return;
        }
        self.dirty_since = None;
        if !self.autosave_ok {
            return;
        }
        if let Err(e) = self.write_autosave() {
            eprintln!("sounder: autosave failed: {e}");
        }
    }

    pub fn time_to_autosave(&self) -> Option<Duration> {
        self.dirty_since
            .map(|s| AUTOSAVE_AFTER.saturating_sub(s.elapsed()))
    }

    fn write_autosave(&self) -> Result<(), String> {
        #[derive(Serialize)]
        struct File<'a> {
            order: &'a [String],
            active: Option<&'a String>,
            sounds: BTreeMap<&'a String, SavedSound>,
        }
        let file = File {
            order: &self.order,
            active: self.shared_active.as_ref(),
            sounds: self.sounds.iter().map(|(k, v)| (k, v.saved())).collect(),
        };
        let bytes = serde_json::to_vec(&file).map_err(|e| e.to_string())?;
        store::atomic_write(&self.autosave_path()?, &bytes)
    }

    /// Reopens the sounds a previous server left in the autosave.
    pub fn restore(&mut self) -> Result<usize, String> {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            order: Vec<String>,
            #[serde(default)]
            active: Option<String>,
            #[serde(default)]
            sounds: BTreeMap<String, SavedSound>,
        }
        let path = self.autosave_path()?;
        let Ok(bytes) = std::fs::read(&path) else {
            return Ok(0);
        };
        let file: File = serde_json::from_slice(&bytes)
            .map_err(|e| format!("{}: {e}: delete the file to start clean", path.display()))?;
        let n = file.sounds.len();
        for (k, v) in file.sounds {
            self.sounds.insert(k, Sound::from_saved(v));
        }
        self.order = file
            .order
            .into_iter()
            .filter(|n| self.sounds.contains_key(n))
            .collect();
        for k in self.sounds.keys() {
            if !self.order.contains(k) {
                self.order.push(k.clone());
            }
        }
        self.shared_active = file.active.filter(|a| self.sounds.contains_key(a));
        Ok(n)
    }
}

/// How `travel` moves the history cursor.
#[derive(Clone, Copy, Debug)]
pub enum Travel {
    Undo,
    Redo,
    Jump(usize),
}

/// Names become file names.
pub fn valid_sound_name(n: &str) -> Result<(), String> {
    if n.is_empty()
        || n.len() > 64
        || !n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "sound name `{n}`: use letters, digits, '_' and '-' (up to 64): it becomes the file name"
        ));
    }
    Ok(())
}

// ------------------------------------------------------------- summaries

/// One summary line per node: `name type <- reads`.
pub fn node_line(n: &Value) -> String {
    let name = n.get("name").and_then(Value::as_str).unwrap_or("?");
    let ty = n.get("type").and_then(Value::as_str).unwrap_or("?");
    let mut body = n.clone();
    if let Some(o) = body.as_object_mut() {
        o.remove("name");
        o.remove("type");
    }
    let reads = sfx_graph::node::refs(&body)
        .into_iter()
        .map(|(n, _)| n)
        .collect::<std::collections::BTreeSet<_>>();
    // Only real nodes (a mode word like "lp" also parses as a ref).
    let reads: Vec<String> = reads.into_iter().collect();
    format!(
        "{name} ({ty}){}",
        if reads.is_empty() {
            String::new()
        } else {
            format!(" <- {}", reads.join(", "))
        }
    )
}

impl Session {
    /// The `summary` tool: text and JSON. Reading it clears the human's knob
    /// moves (the agent has now seen them).
    pub fn summary(&mut self, doc: Option<&str>) -> Result<(String, Value), String> {
        let base = self.base_dir();
        let sound = self.sound(doc)?;
        let moves = std::mem::take(&mut sound.knob_moves);
        let name = sound.doc.name.clone();
        let rendered = sound.rendered(base.as_deref()).cloned();
        let hash = sound.doc_hash();
        let d = &sound.doc;
        let mut out = String::new();
        let node_names: std::collections::BTreeSet<&str> = d
            .nodes
            .iter()
            .filter_map(|n| n.get("name").and_then(Value::as_str))
            .collect();
        out += &format!(
            "sound `{}` v{} ({:?}, {} Hz, seed {}{}){}\n",
            d.name,
            d.version,
            d.kind,
            d.sample_rate,
            d.seed,
            d.duration
                .map_or(", length automatic".into(), |s| format!(", duration {s} s")),
            if sound.unsaved { " [unsaved]" } else { "" }
        )
        .replace("Oneshot", "oneshot")
        .replace("Loop", "loop");
        out += &format!("nodes ({}):\n", d.nodes.len());
        let node_lines: Vec<String> = d
            .nodes
            .iter()
            .map(|n| {
                let line = node_line(n);
                // Keep only references that are nodes.
                match line.split_once(" <- ") {
                    Some((head, reads)) => {
                        let r: Vec<&str> = reads
                            .split(", ")
                            .filter(|x| node_names.contains(x))
                            .collect();
                        if r.is_empty() {
                            head.to_string()
                        } else {
                            format!("{head} <- {}", r.join(", "))
                        }
                    }
                    None => line,
                }
            })
            .collect();
        for l in &node_lines {
            out += &format!("- {l}\n");
        }
        if !d.params.is_empty() {
            let p: Vec<String> = d.params.iter().map(|(k, v)| format!("${k}={v}")).collect();
            out += &format!("params: {}\n", p.join(", "));
        }
        if !d.macros.is_empty() {
            out += "macros:\n";
            for m in &d.macros {
                let checked = match sound.macro_results.get(&m.name) {
                    Some((h, pass, why)) if *h == hash => format!(
                        " [{}: {why}]",
                        if *pass { "claim holds" } else { "CLAIM FAILS" }
                    ),
                    _ => " [claim unchecked: verify runs it]".into(),
                };
                out += &format!(
                    "- {} {:.2} (agent default {:.2}){} claims {} {}{}; moves {}{}\n",
                    m.name,
                    m.value,
                    m.default_value(),
                    if m.game { " GAME" } else { "" },
                    m.claim.0.name(),
                    if m.claim.1 > 0 { "rises" } else { "falls" },
                    checked,
                    m.map
                        .iter()
                        .map(|e| e.target())
                        .collect::<Vec<_>>()
                        .join(", "),
                    if m.doc.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", m.doc)
                    },
                );
            }
        }
        if !moves.is_empty() {
            out += "the human moved knobs since you last read this (keep them as defaults with sound_ops macros, or re-centre a range):\n";
            for k in &moves {
                out += &format!("- {} {:.2} -> {:.2}\n", k.name, k.from, k.to);
            }
        }
        let open: Vec<&Annotation> = sound.open_annotations().collect();
        if !open.is_empty() {
            out += &format!(
                "open comments ({}): requests from the human; deal with each, then add_annotation {{resolve: id, text: what you did}}:\n",
                open.len()
            );
            for a in &open {
                out += &format!("- {}\n", a.line());
            }
        }
        let done = sound.annotations.len() - open.len();
        if done > 0 {
            out += &format!("({done} resolved comment(s) not shown)\n");
        }
        let mut warnings = Vec::new();
        let mut json = serde_json::json!({
            "name": name,
            "kind": d.kind,
            "version": d.version,
            "nodes": node_lines,
            "params": d.params,
            "macros": d.macros,
            "knob_moves": moves,
            "open_annotations": open,
            "saved": !sound.unsaved,
        });
        match &rendered {
            Ok(r) => {
                warnings.extend(r.warnings.clone());
                out += &format!(
                    "causal: {}\n",
                    if r.causal {
                        "yes (a live runtime could run it)".to_string()
                    } else {
                        format!("no: {}", r.acausal.join(", "))
                    }
                );
                let desc: Descriptors = sfx_analysis::describe(&r.audio);
                out += &format!(
                    "last render: {:.3} s, peak {:.1} dBFS, {:.1} LUFS\n",
                    r.audio.seconds(),
                    r.peak_db,
                    r.lufs
                );
                out += &desc.table();
                json["seconds"] = serde_json::json!(r.audio.seconds());
                json["peak_db"] = serde_json::json!(r.peak_db);
                json["lufs"] = serde_json::json!(r.lufs);
                json["causal"] = serde_json::json!(r.causal);
                json["descriptors"] = serde_json::to_value(&desc).unwrap_or(Value::Null);
            }
            Err(e) => warnings.push(format!("does not render yet: {e}")),
        }
        if warnings.is_empty() {
            out += "warnings: none\n";
        } else {
            out += "warnings:\n";
            for w in &warnings {
                out += &format!("- {w}\n");
            }
        }
        json["warnings"] = serde_json::json!(warnings);
        self.mark_dirty();
        Ok((out, json))
    }

    /// `history`: one line per node, newest `limit`.
    pub fn history_text(&mut self, doc: Option<&str>, limit: usize) -> Result<String, String> {
        let sound = self.sound(doc)?;
        let h = &sound.history;
        let total = h.nodes.len();
        let mut out = format!(
            "{total} history step(s), cursor at #{}\n",
            h.current.unwrap_or(0)
        );
        let from = total.saturating_sub(limit);
        if from > 0 {
            out += &format!("(showing the newest {limit}; limit shows more)\n");
        }
        for (id, node) in h.nodes.iter().enumerate().skip(from) {
            let cursor = if h.current == Some(id) { " <-" } else { "" };
            let parent = node
                .parent
                .map_or("root".to_string(), |p| format!("parent #{p}"));
            let by = node
                .author
                .as_deref()
                .map(|a| format!(" by {a}"))
                .unwrap_or_default();
            out += &format!(
                "#{id} {parent}, `{}`{by}, v{}{cursor}\n",
                node.label, node.model.0.version
            );
        }
        Ok(out)
    }

    pub fn log_call(&mut self, tool: &str, ok: bool, ms: u128) {
        self.calls.push(CallRecord {
            tool: tool.into(),
            ok,
            ms,
        });
        if self.calls.len() > 500 {
            self.calls.remove(0);
        }
    }
}
