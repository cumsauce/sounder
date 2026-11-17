//! Macros: named 0..1 knobs an agent builds so the human can fit a sound by
//! ear (the agent cannot hear: the human turns, the agent reads the moves).
//!
//! A macro maps its value onto fields of nodes. Applied to the *resolved*
//! nodes (after `$param` substitution) before evaluation, so a mapped field's
//! own value is overridden. The name is a claim: `claim` says which measure
//! of the sound moves which way over 0..1, and `sfx_analysis`'s `macros`
//! check verifies it (a macro without a claim is not swept). A map entry may
//! also write one of the sound's `params` (`{"param": "size", ...}`), which a
//! `voices` patch and its `map` read. The mock `design/window-concept.html` has a JS version
//! (`applyMacros`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a macro's claim measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Measure {
    /// Spectral centroid, Hz (brightness, weight).
    Centroid,
    /// Ring-out: seconds from the last loud moment (within 6 dB of the peak)
    /// until the level is 40 dB under the peak (length, ring).
    Decay,
    /// Peak over RMS, dB (punch, snap).
    Crest,
    /// Integrated loudness, LUFS (intensity).
    Lufs,
    /// Spectral flatness 0..1 (tonal to noisy; grit, howl).
    Flatness,
    /// p90 - p10 of short-term loudness, LU (steady to gusty).
    Swing,
    /// Mean deviation of the strongest partials from integer multiples of
    /// the pitch, cents (clean to bell-like).
    Inharmonicity,
    /// Percent of the loudness envelope's energy at 20-150 Hz (smooth to
    /// rough, beating).
    Roughness,
    /// Events per second over the audible span, counted at the fine 1 ms
    /// hop of the descriptors' `density` line (a 1 ms envelope above 1 kHz,
    /// an event is a 6 dB rise over the last 3 ms, 4 ms apart at the least):
    /// a rate knob, rain, crackle, a ticking dial. High-passed, so a bass
    /// thump does not count.
    Density,
    /// Attack, milliseconds: from the first sample over -60 dBFS to the
    /// first 0.25 ms frame at 90 % of the sample peak (the descriptors'
    /// `attack to 90 %`). For a train it is the time to the loudest hit's
    /// peak from the very start, so it suits one swell or one hit.
    Attack,
    /// Spacing, seconds: the median gap between the strongest onsets (events
    /// over 150 Hz within 20 dB of the loudest, 30 ms apart at the least).
    /// For a bounce, a drum roll or a footfall pair: a longer first gap
    /// widens it. 0 under two onsets.
    Spacing,
    /// Per-hit ring-out, milliseconds: the median over the strong onsets
    /// (at least 0.25 of the loudest) of each hit's own fall to -20 dB under
    /// its peak, stopping at the next onset. Unlike `decay` it reads one hit
    /// at a time, so it suits a knock, a typewriter or a footfall train whose
    /// whole-sound decay is set by the last hit.
    Ring,
}

impl Measure {
    pub const ALL: [Measure; 12] = [
        Measure::Centroid,
        Measure::Decay,
        Measure::Crest,
        Measure::Lufs,
        Measure::Flatness,
        Measure::Swing,
        Measure::Inharmonicity,
        Measure::Roughness,
        Measure::Density,
        Measure::Attack,
        Measure::Spacing,
        Measure::Ring,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Measure::Centroid => "centroid",
            Measure::Decay => "decay",
            Measure::Crest => "crest",
            Measure::Lufs => "lufs",
            Measure::Flatness => "flatness",
            Measure::Swing => "swing",
            Measure::Inharmonicity => "inharmonicity",
            Measure::Roughness => "roughness",
            Measure::Density => "density",
            Measure::Attack => "attack",
            Measure::Spacing => "spacing",
            Measure::Ring => "ring",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Measure::Centroid => "Hz",
            Measure::Decay => "s",
            Measure::Crest => "dB",
            Measure::Lufs => "LUFS",
            Measure::Flatness => "",
            Measure::Swing => "LU",
            Measure::Inharmonicity => "cents",
            Measure::Roughness => "%",
            Measure::Density => "events/s",
            Measure::Attack => "ms",
            Measure::Spacing => "s",
            Measure::Ring => "ms",
        }
    }
}

/// A claim: [measure, direction] where direction is 1 (rises over 0..1) or
/// -1 (falls).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Claim(pub Measure, pub i8);

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum MapCurve {
    #[default]
    Linear,
    /// Geometric between from and to (both must be > 0): pitch, cutoff.
    Exp,
}

/// One field (or one doc param) a macro writes: give `node` and `field`, or
/// `param`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapEntry {
    /// The node. Empty when the entry writes a `param`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub node: String,
    /// Its field. Empty when the entry writes a `param`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub field: String,
    /// Instead of `node` + `field`: the name of one of the sound's `params`.
    /// Nodes (and a `voices` patch and its `map`) that read `$name` see the
    /// knob's value; the param's own value is overridden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// The value at macro 0 and at macro 1.
    pub from: f64,
    pub to: f64,
    #[serde(default, skip_serializing_if = "is_linear")]
    pub curve: MapCurve,
    /// A table field ([[hz, decay, gain], ...]): multiply this column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
    /// A list field (mix `inputs`, a [lo, hi] range): set this element...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// ...or this field of the element (mix inputs: "gain", "pan", "at").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

fn is_linear(c: &MapCurve) -> bool {
    *c == MapCurve::Linear
}

impl MapEntry {
    /// The value at `x` in 0..1.
    pub fn at(&self, x: f64) -> f64 {
        match self.curve {
            MapCurve::Exp => self.from * (self.to / self.from).powf(x),
            MapCurve::Linear => self.from + (self.to - self.from) * x,
        }
    }

    /// Exactly one of `node` + `field` or `param`; a param entry takes no
    /// `col` / `index` / `key`.
    pub fn check_shape(&self) -> Result<(), String> {
        match (&self.param, self.node.is_empty(), self.field.is_empty()) {
            (Some(_), true, true) => {
                if self.col.is_some() || self.index.is_some() || self.key.is_some() {
                    return Err(format!(
                        "{}: a param entry takes no col / index / key",
                        self.target()
                    ));
                }
                Ok(())
            }
            (Some(p), ..) => Err(format!(
                "entry writes both param `{p}` and node `{}` field `{}`: give one or the other",
                self.node, self.field
            )),
            (None, false, false) => Ok(()),
            (None, ..) => Err(
                "entry needs \"node\" and \"field\" (a node's field), or \"param\" (one of the sound's params)"
                    .into(),
            ),
        }
    }

    /// `node.field` with column / index, or `$param`.
    pub fn target(&self) -> String {
        if let Some(p) = &self.param {
            return format!("${p}");
        }
        let mut s = format!("{}.{}", self.node, self.field);
        if let Some(c) = self.col {
            s += &format!(" col {c}");
        }
        if let Some(i) = self.index {
            s += &format!("[{i}]");
        }
        if let Some(k) = &self.key {
            s += &format!(".{k}");
        }
        s
    }
}

/// A named 0..1 knob. Deserializing names the macro in every error.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Macro {
    /// An adjective for what the knob does: brightness, weight, grit.
    pub name: String,
    /// Where the knob sits now, 0..1 (the human moves it; the agent sets it
    /// with the macro).
    #[serde(default = "half")]
    pub value: f64,
    /// Where the agent set it (the window's reset). Defaults to `value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<f64>,
    /// What the knob does, in a phrase the human reads on hover.
    #[serde(default)]
    pub doc: String,
    /// The name's claim; checked by `verify` (macros). Optional: a macro
    /// without one is not swept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<Claim>,
    /// Written to the export manifest as a runtime input the game sets.
    #[serde(default)]
    pub game: bool,
    /// The fields (or params) it moves.
    pub map: Vec<MapEntry>,
}

/// [`Macro`]'s wire shape; its errors get the macro's name.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MacroBody {
    name: String,
    #[serde(default = "half")]
    value: f64,
    #[serde(default)]
    default: Option<f64>,
    #[serde(default)]
    doc: String,
    #[serde(default)]
    claim: Option<Claim>,
    #[serde(default)]
    game: bool,
    map: Vec<MapEntry>,
}

impl<'de> Deserialize<'de> for Macro {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = Value::deserialize(d)?;
        let what = match v.get("name").and_then(Value::as_str) {
            Some(n) => format!("macro `{n}`"),
            None => "macro (no \"name\" string)".to_string(),
        };
        let b: MacroBody =
            serde_json::from_value(v).map_err(|e| D::Error::custom(format!("{what}: {e}")))?;
        for (i, e) in b.map.iter().enumerate() {
            e.check_shape()
                .map_err(|why| D::Error::custom(format!("{what}: map[{i}]: {why}")))?;
        }
        Ok(Macro {
            name: b.name,
            value: b.value,
            default: b.default,
            doc: b.doc,
            claim: b.claim,
            game: b.game,
            map: b.map,
        })
    }
}

/// The knob positions `verify` sweeps (and `sound_ops` checks renders at when
/// it creates a macro).
pub const SWEEP: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn half() -> f64 {
    0.5
}

impl Macro {
    pub fn default_value(&self) -> f64 {
        self.default.unwrap_or(self.value)
    }

    /// Nodes this macro writes.
    pub fn nodes(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .map
            .iter()
            .filter(|m| m.param.is_none())
            .map(|m| m.node.as_str())
            .collect();
        v.dedup();
        v
    }
}

/// `params` with every macro's param entries written at the macro's value
/// (`overrides` replaces named macros' values). Resolve the nodes with this,
/// and hand it to the render as the sound's params (a `voices` patch reads it).
pub fn effective_params(
    params: &crate::Params,
    macros: &[Macro],
    overrides: &BTreeMap<String, f64>,
) -> crate::Params {
    let mut out = params.clone();
    for m in macros {
        let x = overrides
            .get(&m.name)
            .copied()
            .unwrap_or(m.value)
            .clamp(0.0, 1.0);
        for e in &m.map {
            if let Some(p) = &e.param {
                out.insert(p.clone(), e.at(x));
            }
        }
    }
    out
}

/// `nodes` (already param-resolved) with every macro's node entries applied
/// at its value; `overrides` replaces named macros' values (sweeps, the
/// game's inputs). Param entries are [`effective_params`]'s.
pub fn apply(
    nodes: &[Value],
    macros: &[Macro],
    overrides: &BTreeMap<String, f64>,
) -> Result<Vec<Value>, String> {
    let mut out: Vec<Value> = nodes.to_vec();
    for m in macros {
        let x = overrides
            .get(&m.name)
            .copied()
            .unwrap_or(m.value)
            .clamp(0.0, 1.0);
        for e in m.map.iter().filter(|e| e.param.is_none()) {
            let node = find_node_mut(&mut out, &e.node)
                .map_err(|why| path_error(&m.name, &e.node, nodes, why))?;
            let v = e.at(x);
            write(node, e, v).map_err(|why| format!("macro `{}`: {} {why}", m.name, e.target()))?;
        }
    }
    Ok(out)
}

/// Why a node path did not resolve.
enum PathError {
    /// No top-level node has that name.
    Missing,
    /// The first part is a node but not a `voices` one (so it has no patch).
    NoPatch(String),
    /// The path goes into a `voices` patch that has no node of that name; the
    /// patch's node names.
    NotInPatch { owner: String, nodes: Vec<String> },
}

fn name_of(n: &Value) -> Option<&str> {
    n.get("name").and_then(Value::as_str)
}

/// The node a map entry's `node` names: a top-level node, or `voices/patch_node`
/// (deeper for a `voices` inside a patch). Mapping a patch node writes the
/// patch template, so every voice sees it.
fn find_node_mut<'a>(nodes: &'a mut [Value], path: &str) -> Result<&'a mut Value, PathError> {
    let (head, rest) = match path.split_once('/') {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    let Some(node) = nodes.iter_mut().find(|n| name_of(n) == Some(head)) else {
        return Err(PathError::Missing);
    };
    let Some(rest) = rest else {
        return Ok(node);
    };
    let is_voices = node.get("type").and_then(Value::as_str) == Some("voices");
    let owner = head.to_string();
    let Some(patch) = node
        .get_mut("patch")
        .and_then(Value::as_array_mut)
        .filter(|_| is_voices)
    else {
        return Err(PathError::NoPatch(owner));
    };
    let listed: Vec<String> = patch
        .iter()
        .filter_map(|p| name_of(p).map(String::from))
        .collect();
    match find_node_mut(patch, rest) {
        Err(PathError::Missing) => Err(PathError::NotInPatch {
            owner,
            nodes: listed,
        }),
        other => other,
    }
}

fn path_error(macro_name: &str, path: &str, nodes: &[Value], why: PathError) -> String {
    match why {
        PathError::Missing => missing_node(macro_name, path, nodes),
        PathError::NoPatch(owner) => format!(
            "macro `{macro_name}`: `{path}`: node `{owner}` is not a voices node, so it has no patch to reach into (a path `voices/patch_node` needs a voices node first)"
        ),
        PathError::NotInPatch { owner, nodes } => format!(
            "macro `{macro_name}`: `{path}`: the voices patch of `{owner}` has no such node (patch nodes: {})",
            nodes.join(", ")
        ),
    }
}

/// The inside of `nodes`' `voices` patches (recursively): who owns the node
/// called `name`, or None.
fn voices_owner(nodes: &[Value], name: &str) -> Option<String> {
    for n in nodes {
        let Some(patch) = n.get("patch").and_then(Value::as_array) else {
            continue;
        };
        let owner = n.get("name").and_then(Value::as_str).unwrap_or("?");
        if n.get("type").and_then(Value::as_str) == Some("voices")
            && patch
                .iter()
                .any(|p| p.get("name").and_then(Value::as_str) == Some(name))
        {
            return Some(owner.to_string());
        }
        if let Some(o) = voices_owner(patch, name) {
            return Some(o);
        }
    }
    None
}

/// The error for a map entry naming a node the graph does not have.
fn missing_node(macro_name: &str, node: &str, nodes: &[Value]) -> String {
    if let Some(v) = voices_owner(nodes, node) {
        return format!(
            "macro `{macro_name}`: node `{node}` is inside the voices patch of `{v}`: address it as \"{v}/{node}\" ({{\"node\": \"{v}/{node}\", \"field\": ...}}: every voice sees the change), or declare a param with sound_ops params (e.g. \"{node}_amount\": 0.5), use it in the patch as \"=${node}_amount\" (or `$name` in the voices `map`), and map {{\"param\": \"{node}_amount\", \"from\": 0, \"to\": 1}}"
        );
    }
    let names: Vec<&str> = nodes
        .iter()
        .filter_map(|n| n.get("name").and_then(Value::as_str))
        .collect();
    format!(
        "macro `{macro_name}`: no node named `{node}` (nodes: {})",
        names.join(", ")
    )
}

fn num(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    }
}

/// Which number of a breakpoint pair [x, y] a key names.
fn point_slot(key: &str) -> Option<usize> {
    match key {
        "x" | "time" | "t" => Some(0),
        "y" | "value" | "v" => Some(1),
        _ => None,
    }
}

fn write(node: &mut Value, e: &MapEntry, v: f64) -> Result<(), String> {
    let Some(obj) = node.as_object_mut() else {
        return Err("is not an object".into());
    };
    let field = obj.get_mut(&e.field);
    match (e.col, e.index) {
        (Some(c), _) => {
            let Some(Value::Array(rows)) = field else {
                return Err(format!(
                    "needs a table field `{}` ([[a, b, c], ...])",
                    e.field
                ));
            };
            for row in rows {
                let Some(cell) = row.get_mut(c) else {
                    return Err(format!("has no column {c}"));
                };
                let Some(old) = cell.as_f64() else {
                    return Err(format!("column {c} is not a number"));
                };
                *cell = num(old * v);
            }
            Ok(())
        }
        (None, Some(i)) => {
            let Some(Value::Array(list)) = field else {
                return Err(format!("needs a list field `{}`", e.field));
            };
            let Some(el) = list.get_mut(i) else {
                return Err(format!("has no element {i} (length {})", list.len()));
            };
            match &e.key {
                // A breakpoint pair [x, y]: `x` / `time` and `y` / `value`.
                Some(k) if el.is_array() => {
                    let Some(slot) = point_slot(k) else {
                        return Err(format!(
                            "element {i} is a point [x, y]: key must be one of x, time (the first number) or y, value (the second), not `{k}`"
                        ));
                    };
                    let Some(cell) = el.get_mut(slot) else {
                        return Err(format!("element {i} has no number at {slot}"));
                    };
                    *cell = num(v);
                }
                Some(k) => {
                    // A bare node name becomes {"input": name, key: v}.
                    if let Value::String(s) = el {
                        *el = serde_json::json!({"input": s});
                    }
                    let Some(m) = el.as_object_mut() else {
                        return Err(format!("element {i} is not an object"));
                    };
                    m.insert(k.clone(), num(v));
                }
                None => *el = num(v),
            }
            Ok(())
        }
        (None, None) => {
            if let Some(k) = &e.key {
                // An object field (`expr` `state`): write the named entry,
                // which must exist (a typo would otherwise add a new state).
                let Some(Value::Object(map)) = obj.get_mut(&e.field) else {
                    return Err("has key but no index".into());
                };
                if !map.contains_key(k) {
                    let known: Vec<&str> = map.keys().map(String::as_str).collect();
                    return Err(format!(
                        "has no entry `{k}` in `{}` (entries: {})",
                        e.field,
                        known.join(", ")
                    ));
                }
                map.insert(k.clone(), num(v));
                return Ok(());
            }
            // An absent optional field is set (it overrides the default); the
            // node's own parse rejects a field it does not have.
            obj.insert(e.field.clone(), num(v));
            Ok(())
        }
    }
}

/// The macro list's own mistakes: unique one-word names, values in 0..1, claim
/// direction, finite ends, exp ends positive, nodes exist, no field written by
/// two macros. `nodes` are the document's raw nodes; `params` resolve `$x`.
pub fn validate_shape(
    nodes: &[Value],
    params: &crate::Params,
    macros: &[Macro],
) -> Result<(), String> {
    if macros.is_empty() {
        return Ok(());
    }
    let nodes: Vec<Value> = nodes
        .iter()
        .map(|n| crate::params::resolve(n, params))
        .collect::<Result<_, _>>()?;
    let nodes = &nodes[..];
    let mut seen: Vec<&str> = Vec::new();
    let mut written: Vec<(
        String,
        String,
        Option<usize>,
        Option<usize>,
        Option<String>,
        String,
    )> = Vec::new();
    let mut written_params: Vec<(String, String)> = Vec::new();
    for m in macros {
        if m.name.trim().is_empty() || m.name.contains(char::is_whitespace) {
            return Err(format!(
                "macro `{}`: name it with one word (an adjective: brightness, weight)",
                m.name
            ));
        }
        if seen.contains(&m.name.as_str()) {
            return Err(format!("macro `{}` appears twice", m.name));
        }
        seen.push(&m.name);
        if !(0.0..=1.0).contains(&m.value) || m.default.is_some_and(|d| !(0.0..=1.0).contains(&d)) {
            return Err(format!("macro `{}`: value and default are 0..1", m.name));
        }
        if let Some(c) = &m.claim
            && c.1 != 1
            && c.1 != -1
        {
            return Err(format!(
                "macro `{}`: claim direction {} must be 1 (rises over 0..1) or -1 (falls)",
                m.name, c.1
            ));
        }
        if m.map.is_empty() {
            return Err(format!(
                "macro `{}`: map is empty: list the fields it moves",
                m.name
            ));
        }
        for e in &m.map {
            e.check_shape()
                .map_err(|why| format!("macro `{}`: {why}", m.name))?;
            if !e.from.is_finite() || !e.to.is_finite() {
                return Err(format!(
                    "macro `{}`: {} from / to must be finite numbers",
                    m.name,
                    e.target()
                ));
            }
            if e.curve == MapCurve::Exp && (e.from <= 0.0 || e.to <= 0.0) {
                return Err(format!(
                    "macro `{}`: {} curve exp needs from and to above 0 (got {} and {}): use linear",
                    m.name,
                    e.target(),
                    e.from,
                    e.to
                ));
            }
            if let Some(p) = &e.param {
                if !params.contains_key(p) {
                    let known: Vec<&str> = params.keys().map(String::as_str).collect();
                    return Err(format!(
                        "macro `{}`: no param named `{p}` (params: {}): declare it first with sound_ops params",
                        m.name,
                        if known.is_empty() {
                            "none".into()
                        } else {
                            known.join(", ")
                        }
                    ));
                }
                if let Some(w) = written_params.iter().find(|w| &w.0 == p) {
                    return Err(format!(
                        "macro `{}` and macro `{}` both write param ${p}: split the param or drop one",
                        w.1, m.name
                    ));
                }
                written_params.push((p.clone(), m.name.clone()));
                continue;
            }
            {
                let mut probe: Vec<Value> = nodes.to_vec();
                find_node_mut(&mut probe, &e.node)
                    .map_err(|why| path_error(&m.name, &e.node, nodes, why))?;
            }
            let key = (
                e.node.clone(),
                e.field.clone(),
                e.col,
                e.index,
                e.key.clone(),
            );
            if let Some(w) = written
                .iter()
                .find(|w| (w.0.clone(), w.1.clone(), w.2, w.3, w.4.clone()) == key)
            {
                return Err(format!(
                    "macro `{}` and macro `{}` both write {}: split the field or drop one",
                    w.5,
                    m.name,
                    e.target()
                ));
            }
            written.push((key.0, key.1, key.2, key.3, key.4, m.name.clone()));
        }
    }
    Ok(())
}

/// Both ends must produce a graph that still parses: every macro at 0, then
/// at 1.
pub fn validate_ends(
    nodes: &[Value],
    params: &crate::Params,
    macros: &[Macro],
) -> Result<(), String> {
    if macros.is_empty() {
        return Ok(());
    }
    for x in [0.0, 1.0] {
        let all: BTreeMap<String, f64> = macros.iter().map(|m| (m.name.clone(), x)).collect();
        let at = effective_params(params, macros, &all);
        let nodes: Vec<Value> = nodes
            .iter()
            .enumerate()
            .map(|(k, n)| {
                crate::params::resolve(n, &at)
                    .map_err(|e| format!("with every macro at {x}: node #{k}: {e}"))
            })
            .collect::<Result<_, _>>()?;
        let moved = apply(&nodes, macros, &all)?;
        crate::eval::parse(&moved, &crate::Params::new())
            .map_err(|e| format!("with every macro at {x}: {e}"))?;
    }
    Ok(())
}

/// [`validate_shape`] then [`validate_ends`].
pub fn validate(nodes: &[Value], params: &crate::Params, macros: &[Macro]) -> Result<(), String> {
    validate_shape(nodes, params, macros)?;
    validate_ends(nodes, params, macros)
}

/// A move the human made to a knob (read by the agent in `summary`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnobMove {
    pub name: String,
    pub from: f64,
    pub to: f64,
}
