//! Tool arguments. Doc comments become the schema text agents read, so they
//! say what to pass and what comes back.

use sfx_analysis::Expect;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DocOnly {
    /// Run on this open sound (by name) instead of your active one; errors
    /// if it is not open. Agents sharing a session (subagents) should always
    /// pass it.
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct SoundOpsArgs {
    /// The sound to work on (created if missing, and made your active
    /// one). Omit to use your active sound.
    #[serde(default)]
    pub(crate) sound: Option<String>,
    /// Node upserts: [{"type": "osc", "name": "tone", ...fields}]. Upsert by
    /// name (re-issue to tweak; {"type": "delete", "name": "x"} in this list
    /// removes one, one item per node; there is no top-level `delete`). Nodes
    /// read other nodes by name ("sweep" or a port "verb.wet"); numbers are
    /// constants; "$param" and "=expr" read params. Exactly one `output` node.
    /// describe_op lists every node type with an example. Atomic: any error
    /// changes nothing.
    #[serde(default)]
    pub(crate) ops: Vec<serde_json::Value>,
    /// Named numbers for "$x" / "=$x * 2" in fields: {"base": 220}.
    #[serde(default)]
    pub(crate) params: Option<std::collections::BTreeMap<String, f64>>,
    /// Knobs for the human: upserted by name. {"name": "brightness",
    /// "value": 0.5, "doc": "how cutting it is", "claim": ["centroid", 1],
    /// "game": false, "map": [{"node": "lp", "field": "freq", "from": 900,
    /// "to": 9000, "curve": "exp"}, {"node": "wood", "field": "modes",
    /// "col": 0, "from": 1.6, "to": 0.6}, {"node": "m", "field": "inputs",
    /// "index": 1, "key": "gain", "from": 0.1, "to": 0.9}]}. The name is a
    /// claim: `claim` [measure, direction] (measures centroid, decay, crest,
    /// lufs, flatness, swing; 1 rises over 0..1, -1 falls) is checked by
    /// verify. Name knobs with an adjective; 2-4 per sound; the human turns
    /// them in the window to fit it by ear. game: true exports it as a
    /// runtime input.
    #[serde(default)]
    pub(crate) macros: Option<Vec<sfx_graph::Macro>>,
    /// Names of macros to remove.
    #[serde(default)]
    pub(crate) delete_macros: Vec<String>,
    /// "oneshot" (plays once; its length follows the decay) or "loop"
    /// (needs a duration).
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// Seconds. Omit for a oneshot: it renders until it falls under -80
    /// dBFS (up to 30 s). A loop needs one. Pass 0 to clear it.
    #[serde(default)]
    pub(crate) duration: Option<f32>,
    /// The sound's randomness (default 1).
    #[serde(default)]
    pub(crate) seed: Option<u64>,
    /// Sample rate (default 48000).
    #[serde(default)]
    pub(crate) sample_rate: Option<u32>,
    /// Checks to run in the same call (the same fields as verify's
    /// `expect`): {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1,
    /// "max": 3}, "clicks": true}. A failed check is an error but the batch
    /// stays applied.
    #[serde(default)]
    pub(crate) expect: Option<Expect>,
    /// Run on this open sound instead of your active one (see `sound`).
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SweepArg {
    /// A param name, or a macro name.
    pub(crate) param: String,
    /// Values to render side by side (macro values are 0..1).
    pub(crate) values: Vec<f64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ZoomArg {
    /// Seconds [from, to].
    #[serde(default)]
    pub(crate) t: Option<[f32; 2]>,
    /// Hz [low, high].
    #[serde(default)]
    pub(crate) hz: Option<[f32; 2]>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ListenArgs {
    /// Listen to this node instead of the output.
    #[serde(default)]
    pub(crate) node: Option<String>,
    /// Render these seeds as side-by-side columns: [1, 2, 3].
    #[serde(default)]
    pub(crate) seeds: Option<Vec<u64>>,
    /// Render a param or a macro at several values as columns (not with seeds).
    #[serde(default)]
    pub(crate) sweep: Option<SweepArg>,
    /// Zoom the picture: {"t": [0, 0.1], "hz": [200, 4000]}.
    #[serde(default)]
    pub(crate) zoom: Option<ZoomArg>,
    /// A WAV to compare the sound against (relative paths resolve against the
    /// project, or give an absolute one). The reference is resampled, aligned
    /// by its first onset and scaled to the sound's loudness; the image is
    /// [sound, reference, difference] (difference: blue = the sound is
    /// quieter than the reference, red = louder, grey = same, +-24 dB) and the
    /// text gives the STFT distance (dB, 0 = identical), the 5 biggest band x
    /// time differences in words, and length / attack / decay / centroid
    /// differences. Not with seeds, sweep or diff.
    #[serde(default)]
    pub(crate) reference: Option<String>,
    /// Show the previous render of this same request beside this one, and
    /// the difference spectrogram (now minus before, in dB: blue = quieter
    /// now, red = louder now, grey = same).
    #[serde(default)]
    pub(crate) diff: Option<bool>,
    /// Include the descriptor table (default true).
    #[serde(default)]
    pub(crate) descriptors: Option<bool>,
    /// Run on this open sound instead of your active one.
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct VerifyArgs {
    /// Check names to run IN ADDITION to the defaults: ["clicks"]. Only
    /// checks that need no settings can be named (finite, peak_db (max
    /// -0.3), true_peak_db (max -1), silent_edges, clicks, dc, loop_seam,
    /// variations, rebuild, macros); the others (lufs, duration, bands,
    /// pitch, onsets, reference) take settings: give them in `expect`.
    #[serde(default)]
    pub(crate) checks: Vec<String>,
    /// Check names to run INSTEAD of the defaults: ["clicks", "dc"].
    #[serde(default)]
    pub(crate) only: Vec<String>,
    /// Checks with settings, the same shape as `sound_ops` `expect`, e.g.
    /// {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1, "max": 3},
    /// "lufs": {"min": -20, "max": -14}, "clicks": true}. Given alone, only
    /// these run; with `checks` they run on top of the defaults.
    #[serde(default, rename = "expect")]
    pub(crate) explicit: Option<Expect>,
    /// The same fields as `expect`, at the top level (verify {"clicks":
    /// true}). With none of `checks`, `only`, `expect` or these, the
    /// defaults run (finite, peak_db max -0.3, silent_edges, dc, rebuild, and
    /// macros when the sound has knobs).
    #[serde(flatten)]
    pub(crate) expect: Expect,
    /// Run on this open sound instead of your active one.
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DescribeArgs {
    /// A node type ("filter"): its example and JSON schema. Omit to list
    /// every type, one line each.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// Alias for `kind`.
    #[serde(default, rename = "type")]
    pub(crate) type_: Option<String>,
    /// Alias for `kind`.
    #[serde(default)]
    pub(crate) node: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ExtractModesArgs {
    /// A WAV in the project (a recorded hit, a bell, a knock).
    pub(crate) file: String,
    /// How many modes (default 8, at most 12).
    #[serde(default)]
    pub(crate) count: Option<usize>,
}

/// `play`: needs a speaker (the window, or `--headless --audio`).
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct PlayArgs {
    #[serde(default)]
    pub(crate) node: Option<String>,
    #[serde(default)]
    pub(crate) seed: Option<u64>,
    #[serde(default, rename = "loop")]
    pub(crate) looping: Option<bool>,
    #[serde(default)]
    pub(crate) stop: Option<bool>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct NewSoundArgs {
    /// Letters, digits, '_' and '-': it becomes the file name.
    pub(crate) name: String,
    /// "oneshot" (default) or "loop".
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CatalogArgs {
    /// list, describe, save, load, delete or close.
    pub(crate) action: String,
    /// The sound (save: defaults to your active sound).
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Option<Vec<String>>,
    /// What the human asked for, in their words.
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    /// load over an open sound / close with unsaved work.
    #[serde(default)]
    pub(crate) force: Option<bool>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ExportArgs {
    /// A folder: relative paths land in <project>/assets/sounds/ (default
    /// there). Absolute paths are used as given.
    #[serde(default)]
    pub(crate) path: Option<String>,
    /// Seeds 1..N, one file each (default 1: the sound's own seed).
    #[serde(default)]
    pub(crate) variations: Option<u32>,
    /// 16 (default), 24, or 32 (float).
    #[serde(default)]
    pub(crate) bits: Option<u32>,
    #[serde(default)]
    pub(crate) sample_rate: Option<u32>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct HistoryArgs {
    /// Newest steps shown (default 30).
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct UndoArgs {
    /// Undo / redo another agent's step.
    #[serde(default)]
    pub(crate) force: Option<bool>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct JumpArgs {
    /// A history step id (history lists them).
    pub(crate) id: usize,
    #[serde(default)]
    pub(crate) force: Option<bool>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct AnnotationArgs {
    /// The comment; with `resolve`, what you did about it.
    #[serde(default)]
    pub(crate) text: Option<String>,
    /// Pin it to a node.
    #[serde(default)]
    pub(crate) node: Option<String>,
    /// Pin it to seconds [from, to].
    #[serde(default)]
    pub(crate) time: Option<[f32; 2]>,
    /// ...and Hz [low, high].
    #[serde(default)]
    pub(crate) hz: Option<[f32; 2]>,
    /// Mark comment #id resolved, with `text` as the reply. Resolve, never
    /// delete: the human reads the replies.
    #[serde(default)]
    pub(crate) resolve: Option<u32>,
    /// Reopen comment #id.
    #[serde(default)]
    pub(crate) reopen: Option<u32>,
    #[serde(default)]
    pub(crate) doc: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct SessionLogArgs {
    /// Recent calls shown (default 20).
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    /// Tell the maintainers something surprised you (an error that did not
    /// say what to do, a field that did not work as described): one or two
    /// sentences, what you did and what you expected.
    #[serde(default)]
    pub(crate) feedback: Option<String>,
    /// Fold the friction log into what to fix first.
    #[serde(default)]
    pub(crate) report: Option<bool>,
}
