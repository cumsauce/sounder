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
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sfx_analysis::{CheckCx, CheckResult, Descriptors, Expect};
use sfx_graph::{
    ApplyReport, Audio, DocEdit, Kind, KnobMove, RenderOpts, Rendered, SoundDoc, SoundSource,
};

use crate::history::{History, Step};
use crate::store;

mod window_state;

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
    ///
    /// A sound that instances others also keys on the session's generation:
    /// editing an instanced sound changes nothing in this document.
    pub fn doc_hash(&self, generation: u64) -> u64 {
        hash_of(&(&self.doc, self.salt(generation)))
    }

    /// The generation matters only to a sound that instances others; every
    /// other sound's render and checks survive edits elsewhere.
    fn salt(&self, generation: u64) -> u64 {
        let instances = self
            .doc
            .nodes
            .iter()
            .any(|n| n.get("type").and_then(|t| t.as_str()) == Some("sound"));
        if instances { generation } else { 0 }
    }

    /// The (cached) render of the document as it stands.
    ///
    /// `salt` is the session's generation (see `Session::generation`): it changes
    /// whenever any sound is edited, opened, closed or saved, so a render that
    /// instances another sound never outlives an edit to it.
    pub fn rendered(&mut self, opts: &RenderOpts, salt: u64) -> Result<&Rendered, String> {
        let salt = self.salt(salt);
        let key = hash_of(&(&self.doc, &opts.base_dir, salt));
        if self.render.as_ref().is_none_or(|(k, _)| *k != key) {
            let r = self.doc.render(opts)?;
            self.render = Some((key, r));
        }
        match &self.render {
            Some((_, r)) => Ok(r),
            None => Err("no render".into()),
        }
    }

    /// Stores a render the caller already made for this document.
    pub fn set_render(&mut self, opts: &RenderOpts, salt: u64, r: Rendered) {
        self.render = Some((hash_of(&(&self.doc, &opts.base_dir, self.salt(salt))), r));
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
        // Knob moves and claim results were about the graph we left.
        self.knob_moves.clear();
        self.macro_results.clear();
    }
}

/// What a call credits and notes for one connected agent.
#[derive(Debug, Default)]
struct ClientState {
    active: Option<String>,
    /// When this client's last call ended.
    last: Option<Instant>,
    /// History node count of each sound at this client's last call.
    seen: HashMap<String, usize>,
}

/// The tool call log entry (for `session_log`).
#[derive(Clone, Debug)]
pub struct CallRecord {
    pub tool: String,
    pub ok: bool,
    pub ms: u128,
    /// When the call finished.
    pub at: Instant,
    /// The calling agent (`_meta` "sounder/client"), when it said.
    pub client: Option<String>,
    /// The sound the call worked in.
    pub doc: Option<String>,
    /// A short line for the window's feed (`laser · ~lp`).
    pub what: Option<String>,
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
    /// Bumps on every change a render may depend on.
    generation: u64,
    /// Bumps on every change anyone can see (the window's `rev`).
    rev: u64,
    /// When each sound last changed.
    touched: HashMap<String, Instant>,
    /// History length the window last showed per sound (what is newer and
    /// by someone else is `unread` in the graph).
    window_seen: HashMap<String, usize>,
    /// The default checks per sound: (doc hash, results).
    check_cache: HashMap<String, (u64, Vec<CheckResult>)>,
    /// Descriptors as JSON per sound: (doc hash, generation, value).
    desc_cache: HashMap<String, (u64, u64, Value)>,
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
        if let Some(c) = client {
            self.clients.entry(c.to_string()).or_default().last = Some(Instant::now());
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

    /// Something changed that renders may depend on: bumps the generation
    /// (see [`Sound::rendered`]) and schedules an autosave.
    pub fn mark_dirty(&mut self) {
        self.generation += 1;
        self.rev += 1;
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Schedules an autosave without invalidating cached renders (reading
    /// `summary` clears knob moves but changes no sound).
    fn mark_saved_state_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Counts every change anyone can see: edits, knob moves, comments,
    /// opening and closing, the agent reading the knob moves. The window
    /// pushes `changed` when it moves.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Bumps [`Session::rev`] without invalidating renders (a check result
    /// arrived, a read cleared something).
    pub fn bump_rev(&mut self) {
        self.rev += 1;
    }

    /// `name` was just edited.
    fn touch(&mut self, name: &str) {
        self.touched.insert(name.to_string(), Instant::now());
    }

    /// The generation renders are cached against.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        self.project.clone()
    }

    /// How a `sound` node reaches other sounds: the open ones first, then the
    /// catalog.
    pub fn sound_source(&self) -> SoundSource {
        let docs: BTreeMap<String, SoundDoc> = self
            .sounds
            .iter()
            .map(|(k, v)| (k.clone(), v.doc.clone()))
            .collect();
        let lib = self.library_dir().ok();
        let mut names: Vec<String> = docs.keys().cloned().collect();
        if let Some(l) = &lib {
            for e in crate::catalog::list(l) {
                if !names.contains(&e.name) {
                    names.push(e.name);
                }
            }
        }
        SoundSource::new(names, move |n| {
            docs.get(n)
                .cloned()
                .or_else(|| lib.as_ref().and_then(|l| crate::catalog::load(l, n).ok()))
        })
    }

    /// Render options for this session: the project folder for relative
    /// files, and a way to reach other sounds.
    pub fn render_opts(&self) -> RenderOpts {
        RenderOpts {
            base_dir: self.base_dir(),
            sounds: Some(self.sound_source()),
            ..RenderOpts::default()
        }
    }

    // ---------------------------------------------------------- sounds

    /// Opens a new sound beside the others and makes it the caller's active.
    pub fn new_sound(&mut self, name: &str, kind: Kind) -> Result<String, String> {
        self.new_sound_with(name, kind, None, None)
    }

    /// [`Session::new_sound`] with a fixed `duration` (seconds; 0 or less
    /// means automatic length) and / or a `seed`. A loop opened without a
    /// duration is allowed (it errors at its first render, naming the field).
    pub fn new_sound_with(
        &mut self,
        name: &str,
        kind: Kind,
        duration: Option<f32>,
        seed: Option<u64>,
    ) -> Result<String, String> {
        valid_sound_name(name)?;
        let duration = match duration {
            Some(d) if !d.is_finite() || d > 600.0 => {
                return Err(format!("duration: {d}: give seconds in 0..600"));
            }
            Some(d) if d > 0.0 => Some(d),
            _ => None,
        };
        if self.sounds.contains_key(name) {
            return Err(format!(
                "`{name}` is already open: pick another name, or work on it with doc: \"{name}\""
            ));
        }
        let by = self.current.clone();
        let mut sound = Sound::new(name, kind, by.as_deref());
        if duration.is_some() || seed.is_some() {
            sound.doc.duration = duration;
            if let Some(sd) = seed {
                sound.doc.seed = sd;
            }
            sound.history = History::default();
            sound.history.record(Step(sound.doc.clone()), "new sound");
            if let Some(n) = sound.history.nodes.last_mut() {
                n.author = by.clone();
            }
        }
        self.sounds.insert(name.to_string(), sound);
        self.order.push(name.to_string());
        self.set_active(name);
        self.touch(name);
        self.mark_dirty();
        let length = match (kind, duration) {
            (_, Some(d)) => format!("{d} s"),
            (Kind::Loop, None) => "needs a duration: pass one to sound_ops".to_string(),
            _ => "its length follows the decay".to_string(),
        };
        Ok(format!(
            "opened `{name}` ({}: {length}): add nodes with sound_ops; describe_op lists the node types",
            if kind == Kind::Loop {
                "loop"
            } else {
                "oneshot"
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
        self.touch(&name);
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
        for (k, item) in items.iter().enumerate() {
            reserved_name(k, item)?;
        }
        let by = self.current.clone();
        let opts = self.render_opts();
        let name = self.target_name(doc)?;
        let sound = self.open_mut(&name)?;
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
        if report.changed {
            self.touch(&name);
        }
        self.mark_dirty();
        let salt = self.generation;
        let sound = self.open_mut(&name)?;
        sound.set_render(&opts, salt, report.rendered.clone());
        Ok(report)
    }

    /// Runs the checks (`expect`, or the defaults) on the sound as it stands.
    pub fn check(
        &mut self,
        doc: Option<&str>,
        expect: Option<&Expect>,
    ) -> Result<Vec<CheckResult>, String> {
        let opts = self.render_opts();
        let salt = self.generation;
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
        let hash = sound.doc_hash(salt);
        let doc_copy = sound.doc.clone();
        let rendered = sound.rendered(&opts, salt)?.clone();
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
        if expect.is_none() {
            self.check_cache.insert(name, (hash, results.clone()));
            self.rev += 1;
        }
        Ok(results)
    }

    /// `verify` with named checks. `checks` run on top of the defaults,
    /// `only` instead of them; `explicit` (the `expect` object and any
    /// top-level check fields, later ones winning) carry settings: given
    /// alone they are the whole set, given with `checks` they run on top of
    /// the defaults. With nothing given this is [`Session::check`] with the
    /// defaults.
    pub fn verify(
        &mut self,
        doc: Option<&str>,
        checks: &[String],
        only: &[String],
        explicit: &[&Expect],
    ) -> Result<Vec<CheckResult>, String> {
        let explicit: Vec<&Expect> = explicit.iter().copied().filter(|e| !e.is_empty()).collect();
        if checks.is_empty() && only.is_empty() && explicit.is_empty() {
            return self.check(doc, None);
        }
        let name = self.target_name(doc)?;
        let defaults = Expect::defaults_for(&self.sound(Some(&name))?.doc);
        let e = plan_checks(&defaults, checks, only, &explicit)?;
        self.check(Some(&name), Some(&e))
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
        let n = sound.doc.name.clone();
        self.touch(&n);
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
        self.touch(&name);
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
        let n = sound.doc.name.clone();
        self.touch(&n);
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

// ----------------------------------------------------- tool arguments

/// A node may not be named `mono` or `stereo`: they are the values of the
/// `to` field of `mix`, so a node reading "mono" would be ambiguous.
fn reserved_name(k: usize, item: &Value) -> Result<(), String> {
    let ty = item.get("type").and_then(Value::as_str).unwrap_or("");
    if ty == "delete" {
        return Ok(());
    }
    match item.get("name").and_then(Value::as_str) {
        Some(n @ ("mono" | "stereo")) => Err(format!(
            "node #{k} ({ty}): name `{n}` is reserved: `mono` and `stereo` are the values of a mix `to` field, so a node cannot be called that; pick another name (e.g. `{n}_mix`)"
        )),
        _ => Ok(()),
    }
}

/// Rejects a tool call that carries keys the tool does not take. `accepted`
/// is the tool's schema keys (everything else, including a typo or a key from
/// another tool, would otherwise be ignored without a word). The message
/// names the tool, each bad key and the keys it takes; a few known mistakes
/// also get the working form.
pub fn check_tool_args(
    tool: &str,
    args: &serde_json::Map<String, Value>,
    accepted: &[String],
) -> Result<(), String> {
    let bad: Vec<&String> = args.keys().filter(|k| !accepted.contains(k)).collect();
    if bad.is_empty() {
        return check_shapes(tool, args);
    }
    let list = bad
        .iter()
        .map(|k| format!("`{k}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut msg = format!(
        "{tool}: unknown argument{} {list} (it takes: {})",
        if bad.len() == 1 { "" } else { "s" },
        accepted.join(", ")
    );
    for k in bad {
        if let Some(h) = arg_hint(tool, k, &args[k.as_str()]) {
            msg += &format!("; {h}");
        }
    }
    Err(msg)
}

/// Nested arguments whose serde error says little: `listen` `zoom` and
/// `sweep`. A wrong one gets the accepted shape.
fn check_shapes(tool: &str, args: &serde_json::Map<String, Value>) -> Result<(), String> {
    if tool != "listen" {
        return Ok(());
    }
    const ZOOM: &str = "zoom {t: [from_s, to_s], hz: [low, high]} (seconds and Hz; either key alone, e.g. zoom {t: [0, 0.1]})";
    const SWEEP: &str = "sweep {param: \"name\", values: [v1, v2, ...]} (a param or macro name; macro values are 0..1; at most 8 values)";
    let pair = |v: &Value| {
        v.as_array()
            .is_some_and(|a| a.len() == 2 && a.iter().all(Value::is_number))
    };
    if let Some(z) = args.get("zoom").filter(|z| !z.is_null()) {
        let why = match z.as_object() {
            None => Some("it must be an object".to_string()),
            Some(o) => o.iter().find_map(|(k, v)| match k.as_str() {
                "t" | "hz" if v.is_null() || pair(v) => None,
                "t" | "hz" => Some(format!("`{k}` must be a two-number array [a, b]")),
                _ => Some(format!("unknown key `{k}`; it takes t and hz")),
            }),
        };
        if let Some(why) = why {
            return Err(format!("listen: bad `zoom` ({why}): use {ZOOM}"));
        }
    }
    if let Some(sw) = args.get("sweep").filter(|z| !z.is_null()) {
        let why = match sw.as_object() {
            None => Some("it must be an object".to_string()),
            Some(o) => {
                if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "param" | "values")) {
                    Some(format!("unknown key `{k}`; it takes param and values"))
                } else if !o.get("param").is_some_and(Value::is_string) {
                    Some("`param` must be a name".to_string())
                } else if !o
                    .get("values")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().all(Value::is_number))
                {
                    Some("`values` must be an array of numbers".to_string())
                } else {
                    None
                }
            }
        };
        if let Some(why) = why {
            return Err(format!("listen: bad `sweep` ({why}): use {SWEEP}"));
        }
    }
    Ok(())
}

/// What to do instead, for mistakes that were made in earnest.
fn arg_hint(tool: &str, key: &str, value: &Value) -> Option<String> {
    match (tool, key) {
        ("sound_ops", "delete" | "delete_nodes" | "remove" | "remove_nodes") => {
            let mut names: Vec<String> = match value {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => Vec::new(),
            };
            if names.is_empty() {
                names.push("x".into());
            }
            let ops: Vec<String> = names
                .iter()
                .map(|n| format!("{{\"type\": \"delete\", \"name\": \"{n}\"}}"))
                .collect();
            Some(format!(
                "nodes are deleted inside `ops`, one item per node: ops: [{}]",
                ops.join(", ")
            ))
        }
        ("sound_ops", "nodes" | "graph") => Some("node upserts go in `ops`".into()),
        ("describe_op", "name") => Some("give the node type as `kind`".into()),
        ("verify", "check" | "names") => {
            Some("name the checks in `checks` (added to the defaults) or `only`".into())
        }
        _ => None,
    }
}

/// What naming a check with no settings means (or why it cannot be named).
fn bare_check(name: &str) -> Result<Value, String> {
    use serde_json::json;
    Ok(match name {
        "finite" | "silent_edges" | "clicks" | "dc" | "loop_seam" | "variations" | "rebuild"
        | "macros" => json!(true),
        "peak_db" => json!({"max": -0.3}),
        "true_peak_db" => json!({"max": -1.0}),
        "lufs" => return needs_settings(name, r#"{"min": -20, "max": -14}"#),
        "duration" => return needs_settings(name, r#"{"min": 0.4, "max": 1.2}"#),
        "onsets" => return needs_settings(name, r#"{"min": 1, "max": 3}"#),
        "bands" => return needs_settings(name, r#"[{"band": "1k", "min_db": -40}]"#),
        "pitch" => return needs_settings(name, r#"{"hz": 440}"#),
        "reference" => {
            return needs_settings(name, r#"{"file": "ref.wav", "max_distance": 6}"#);
        }
        other => {
            return Err(format!(
                "verify: unknown check `{other}`; the checks are peak_db, true_peak_db, lufs, duration, silent_edges, clicks, dc, finite, loop_seam, bands, pitch, onsets, variations, rebuild, reference, macros, aliasing, inharmonicity, modulation, roughness (the last four need settings: use expect)"
            ));
        }
    })
}

fn needs_settings(name: &str, example: &str) -> Result<Value, String> {
    Err(format!(
        "verify: check `{name}` needs settings, so it cannot be named in `checks` / `only`: pass it in `expect`, e.g. verify {{\"expect\": {{\"{name}\": {example}}}}}"
    ))
}

fn expect_map(e: &Expect) -> Result<serde_json::Map<String, Value>, String> {
    match serde_json::to_value(e) {
        Ok(Value::Object(m)) => Ok(m.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        _ => Err("expect: could not be read".into()),
    }
}

/// The check set for `verify`: see [`Session::verify`].
fn plan_checks(
    defaults: &Expect,
    checks: &[String],
    only: &[String],
    explicit: &[&Expect],
) -> Result<Expect, String> {
    let from_defaults = only.is_empty() && (explicit.is_empty() || !checks.is_empty());
    let mut m = if from_defaults {
        expect_map(defaults)?
    } else {
        serde_json::Map::new()
    };
    for n in only.iter().chain(checks) {
        m.insert(n.clone(), bare_check(n)?);
    }
    for e in explicit {
        m.extend(expect_map(e)?);
    }
    serde_json::from_value(Value::Object(m)).map_err(|e| format!("verify: {e}"))
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
    let reads = sfx_graph::node::refs_typed(ty, &body)
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
    /// One node's samples (the output's when `node` is None) of the sound as
    /// it stands, rendered if needed: what the window's bulk data and the
    /// player read.
    pub fn node_audio(&mut self, doc: Option<&str>, node: Option<&str>) -> Result<Audio, String> {
        let opts = self.render_opts();
        let salt = self.generation;
        self.sound(doc)?.rendered(&opts, salt)?.node_audio(node)
    }

    /// The `summary` tool: text and JSON. Reading it clears the human's knob
    /// moves (the agent has now seen them).
    pub fn summary(&mut self, doc: Option<&str>) -> Result<(String, Value), String> {
        let opts = self.render_opts();
        let salt = self.generation;
        let sound = self.sound(doc)?;
        let moves = std::mem::take(&mut sound.knob_moves);
        let name = sound.doc.name.clone();
        let rendered = sound.rendered(&opts, salt).cloned();
        let hash = sound.doc_hash(salt);
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
                    _ if m.claim.is_none() => String::new(),
                    Some((h, pass, why)) if *h == hash => format!(
                        " [{}: {why}]",
                        if *pass { "claim holds" } else { "CLAIM FAILS" }
                    ),
                    _ => " [claim unchecked: verify runs it]".into(),
                };
                out += &format!(
                    "- {} {:.2} (agent default {:.2}){} {}{}; moves {}{}\n",
                    m.name,
                    m.value,
                    m.default_value(),
                    if m.game { " GAME" } else { "" },
                    match m.claim {
                        Some(c) => format!(
                            "claims {} {}",
                            c.0.name(),
                            if c.1 > 0 { "rises" } else { "falls" }
                        ),
                        None => "unclaimed".to_string(),
                    },
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
                let desc: Descriptors =
                    sfx_analysis::describe(&r.audio).with_loop(d.kind == Kind::Loop);
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
        if !moves.is_empty() {
            // The window shows unread moves: reading them changes that.
            self.rev += 1;
        }
        self.mark_saved_state_dirty();
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

    pub fn log_call(&mut self, tool: &str, ok: bool, ms: u128, client: Option<&str>) {
        let doc = client
            .and_then(|c| self.clients.get(c))
            .and_then(|c| c.active.clone())
            .or_else(|| self.shared_active.clone())
            .filter(|d| self.sounds.contains_key(d));
        // What the call did, for the window's feed: the newest history step
        // for tools that make one, else just the sound.
        let what = doc.as_ref().and_then(|d| {
            let h = &self.sounds.get(d)?.history;
            let label = h.nodes.get(h.current?)?.label.clone();
            let edits = matches!(tool, "sound_ops" | "undo" | "redo" | "jump" | "new_sound");
            Some(if edits && ok {
                format!("{d} · {label}")
            } else {
                d.clone()
            })
        });
        self.calls.push(CallRecord {
            tool: tool.into(),
            ok,
            ms,
            at: Instant::now(),
            client: client.map(String::from),
            doc,
            what,
        });
        if self.calls.len() > 500 {
            self.calls.remove(0);
        }
    }
}
