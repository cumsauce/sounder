//! Tool arguments. Doc comments become the schema text agents read, so they
//! say what to pass and what comes back.

use sfx_analysis::Expect;

/// Tells a missing field (`None`: leave it) from an explicit `null`
/// (`Some(None)`: clear it).
fn present_or_null<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(d).map(Some)
}

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
    /// constants; "$param" and "=expr" read params (a string with `$` or
    /// number arithmetic, "1.5 * $value", is an expression too). Exactly one
    /// `output` node.
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
    /// lufs, flatness, swing, inharmonicity, roughness, density, attack,
    /// spacing; 1 rises over 0..1, -1 falls; optional; pick one that fits:
    /// density for event-rate knobs (holds to ~500/s), centroid, decay for
    /// single-hit sounds; an output.lufs target renormalizes the level, so a
    /// loudness knob cannot claim lufs) is checked by verify,
    /// which also fails a knob that moves its measure by under 2 %. A map entry may write a sound param instead:
    /// {"param": "size", "from": 0.2, "to": 0.8}. Name knobs with an adjective; 2-4 per sound; the human turns
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
    /// Seconds. Omit (or null) to leave it as it is. A oneshot with no duration renders
    /// until it falls under -80 dBFS or 60 dB under its peak (up to 30 s); a loop needs one. Pass
    /// 0 to clear a fixed duration and go back to automatic length.
    #[serde(default, deserialize_with = "present_or_null")]
    #[schemars(with = "Option<f32>")]
    pub(crate) duration: Option<Option<f32>>,
    /// The sound's randomness (default 1).
    #[serde(default)]
    pub(crate) seed: Option<u64>,
    /// Sample rate (default 48000).
    #[serde(default)]
    pub(crate) sample_rate: Option<u32>,
    /// Checks to run in the same call (the same fields as verify's
    /// `expect`): {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1,
    /// "max": 3, "spacing_ms": 5, "gap": [0.15, 0.3]}, "clicks": true}.
    /// `spacing_ms` (default 40) is the onset merge distance: 5 counts dense
    /// ticks; `gap` [lo, hi] seconds bounds every gap between consecutive
    /// onsets (leave 10 ms of margin). A failed check is an
    /// error but the batch stays applied.
    #[serde(default)]
    pub(crate) expect: Option<Expect>,
    /// The brief as data, replacing any earlier spec (`{}` removes it; null changes nothing):
    /// {"brief": "a heavy wooden door slammed shut", "components": [{"role":
    /// "crack", "node": "crk_g", "at": [0.05, 0.08], "band": [1500, 16000],
    /// "note": "the edge meeting the frame"}, {"role": "body", "node":
    /// "panel", "material": "wood"}]}. Roles: pre crack body weight after bed
    /// texture whole; `node` is the layer's last node before the mix (leave
    /// it out until the layer exists); `at` [start, end] s holds 80 % of the
    /// layer's energy; `length_ms` [min, max] how long it lasts (the span
    /// of the middle 80 % of its energy; a modal body alone is ~5 ms, a
    /// noise body or weight 50-250); `band` [lo, hi] Hz half of it (left out: the role's
    /// band; pre 1-8 kHz, crack 1.5-16 kHz, body 100-3000 Hz, weight
    /// 20-400 Hz). summary lists it; verify runs the `spec` check when there
    /// is one.
    #[serde(default, deserialize_with = "present_or_null")]
    #[schemars(with = "Option<sfx_graph::Spec>")]
    pub(crate) spec: Option<Option<sfx_graph::Spec>>,
    /// Edit single spec components instead of resending the spec:
    /// [{"index": 1, "node": "boards_g", "length_ms": [40, 300]}] merges
    /// the fields into component 1 (null clears one); an index equal to the
    /// count appends a component (give its role). Runs after `spec`.
    #[serde(default)]
    pub(crate) spec_update: Vec<serde_json::Value>,
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
    /// The `heard_as` line (default true when a model is set up): the
    /// closest descriptions an audio-text model finds for each column, and
    /// where the spec's brief ranks. A hint, not a check: it named a crate
    /// drop the listener called great "a snare drum". false skips it.
    #[serde(default)]
    pub(crate) heard_as: Option<bool>,
    /// Render these seeds as side-by-side columns: [1, 2, 3].
    #[serde(default)]
    pub(crate) seeds: Option<Vec<u64>>,
    /// Render a param or a macro at several values as columns (not with seeds).
    #[serde(default)]
    pub(crate) sweep: Option<SweepArg>,
    /// Zoom the picture: {"t": [0, 0.1], "hz": [200, 4000]} (seconds, Hz;
    /// either key alone is fine). With `t` the descriptor text describes
    /// that window too, and its times count from `t[0]`.
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
    /// A reference set: a folder of recordings of one kind of sound
    /// (`<project>/refs/<set>/`, at least two WAVs). Gives the distance to
    /// each, the nearest against the set's own spread, and every feature
    /// (weight / body / crack energy, crest, tail, centroid, voiced, ringing
    /// partials) against the span the recordings cover; the image compares
    /// with the nearest. Not with reference, seeds, sweep or diff.
    #[serde(default)]
    pub(crate) reference_set: Option<String>,
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
    /// variations (4 seeds), rebuild, macros, layers (the impact preset)); the others (lufs, duration, bands,
    /// pitch, onsets, reference, aliasing, inharmonicity, modulation,
    /// roughness) take settings: give them in `expect`.
    #[serde(default)]
    pub(crate) checks: Vec<String>,
    /// Check names to run INSTEAD of the defaults: ["clicks", "dc"].
    #[serde(default)]
    pub(crate) only: Vec<String>,
    /// Checks with settings, the same shape as `sound_ops` `expect`, e.g.
    /// {"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1, "max": 3,
    /// "gap": [0.15, 0.3]},
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
    /// 2..8: re-render with seeds 1..N (the document's seed is replaced for
    /// those renders only; nothing is saved) and run the same checks on
    /// each. FAILs are grouped by seed ("seed 3: loop_seam"). A plain verify
    /// renders only the seed in the document, so a sound that passes only at
    /// its own seed is not finished: run `seeds: 4`.
    #[serde(default)]
    pub(crate) seeds: Option<u32>,
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
    /// A file name ("knock_residual.wav"): also write the residual (the
    /// recording minus the fitted modes: the hit's noise, rattle, room) into
    /// the sources folder (`<project>/assets/sources/`, or the library's
    /// `sources/`) and return a rebuild that mixes it with the modes.
    #[serde(default)]
    pub(crate) residual: Option<String>,
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
    /// Seconds (0..600). A loop needs one before it can render: pass it here
    /// or in the first sound_ops call. Omit for a oneshot (automatic length).
    #[serde(default)]
    pub(crate) duration: Option<f32>,
    /// The sound's randomness (default 1).
    #[serde(default)]
    pub(crate) seed: Option<u64>,
    /// Start from an approved sound instead of an empty one: "door_slam"
    /// (the layered slam the listener approved, with knobs stick, weight,
    /// snap, knock and latch). The whole graph is copied and stays editable.
    #[serde(default)]
    pub(crate) template: Option<String>,
    /// The brief in words ("a pipe dropped on concrete"): the result lists
    /// the parts, templates and ready-made nodes that fit it, best first.
    #[serde(default)]
    pub(crate) brief: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CatalogArgs {
    /// list, search (parts and saved sounds by `text` / `role` / `material`
    /// / `approved`), sources (the recordings and impulse responses in the
    /// sources folders), describe, save, load, delete or close.
    pub(crate) action: String,
    /// The sound (save: defaults to your active sound).
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// Alias for `name`, like every other tool's `doc`; both given and
    /// different is an error.
    #[serde(default)]
    pub(crate) doc: Option<String>,
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
    /// search: words that must all appear in the name, description, tags,
    /// role or material.
    #[serde(default)]
    pub(crate) text: Option<String>,
    /// save: the layer this sound fills when used as a part (pre, crack,
    /// body, weight, after, bed, texture, whole). search: only that role.
    #[serde(default)]
    pub(crate) role: Option<String>,
    /// save: what it is made of (wood, metal, glass, water, ...; `any` for a
    /// layer that fits every material). search: that material or `any`.
    #[serde(default)]
    pub(crate) material: Option<String>,
    /// search only: true for sounds the human approved by ear, false for the
    /// rest. Approval is set by the human in the window, never by `save`.
    #[serde(default)]
    pub(crate) approved: Option<bool>,
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
    /// "ogg" (default: Ogg Vorbis, small) or "wav".
    #[serde(default)]
    pub(crate) format: Option<String>,
    /// Ogg Vorbis VBR quality 0..1 (default 0.5, about 160 kbps stereo).
    #[serde(default)]
    pub(crate) quality: Option<f32>,
    /// WAV only: 16 (default), 24, or 32 (float).
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
