//! Macros: named 0..1 knobs an agent builds so the human can fit a sound by
//! ear (the agent cannot hear: the human turns, the agent reads the moves).
//!
//! A macro maps its value onto fields of nodes. Applied to the *resolved*
//! nodes (after `$param` substitution) before evaluation, so a mapped field's
//! own value is overridden. The name is a claim: `claim` says which measure
//! of the sound moves which way over 0..1, and `sfx_analysis`'s `macros`
//! check verifies it. The mock `design/window-concept.html` has a JS version
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
}

impl Measure {
    pub const ALL: [Measure; 6] = [
        Measure::Centroid,
        Measure::Decay,
        Measure::Crest,
        Measure::Lufs,
        Measure::Flatness,
        Measure::Swing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Measure::Centroid => "centroid",
            Measure::Decay => "decay",
            Measure::Crest => "crest",
            Measure::Lufs => "lufs",
            Measure::Flatness => "flatness",
            Measure::Swing => "swing",
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

/// One field a macro writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapEntry {
    /// The node.
    pub node: String,
    /// Its field.
    pub field: String,
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

    /// `node.field` with column / index.
    pub fn target(&self) -> String {
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

/// A named 0..1 knob.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// The name's claim; checked by `verify` (macros).
    pub claim: Claim,
    /// Written to the export manifest as a runtime input the game sets.
    #[serde(default)]
    pub game: bool,
    /// The fields it moves.
    pub map: Vec<MapEntry>,
}

fn half() -> f64 {
    0.5
}

impl Macro {
    pub fn default_value(&self) -> f64 {
        self.default.unwrap_or(self.value)
    }

    /// Nodes this macro writes.
    pub fn nodes(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.map.iter().map(|m| m.node.as_str()).collect();
        v.dedup();
        v
    }
}

/// `nodes` (already param-resolved) with every macro applied at its value;
/// `overrides` replaces named macros' values (sweeps, the game's inputs).
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
        for e in &m.map {
            let Some(node) = out
                .iter_mut()
                .find(|n| n.get("name").and_then(Value::as_str) == Some(e.node.as_str()))
            else {
                let names: Vec<&str> = nodes
                    .iter()
                    .filter_map(|n| n.get("name").and_then(Value::as_str))
                    .collect();
                return Err(format!(
                    "macro `{}`: no node named `{}` (nodes: {})",
                    m.name,
                    e.node,
                    names.join(", ")
                ));
            };
            let v = e.at(x);
            write(node, e, v).map_err(|why| format!("macro `{}`: {} {why}", m.name, e.target()))?;
        }
    }
    Ok(out)
}

fn num(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
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
            if e.key.is_some() {
                return Err("has key but no index".into());
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
        if m.claim.1 != 1 && m.claim.1 != -1 {
            return Err(format!(
                "macro `{}`: claim direction {} must be 1 (rises over 0..1) or -1 (falls)",
                m.name, m.claim.1
            ));
        }
        if m.map.is_empty() {
            return Err(format!(
                "macro `{}`: map is empty: list the fields it moves",
                m.name
            ));
        }
        for e in &m.map {
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
            if !nodes
                .iter()
                .any(|n| n.get("name").and_then(Value::as_str) == Some(e.node.as_str()))
            {
                let names: Vec<&str> = nodes
                    .iter()
                    .filter_map(|n| n.get("name").and_then(Value::as_str))
                    .collect();
                return Err(format!(
                    "macro `{}`: no node named `{}` (nodes: {})",
                    m.name,
                    e.node,
                    names.join(", ")
                ));
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
    let nodes: Vec<Value> = nodes
        .iter()
        .map(|n| crate::params::resolve(n, params))
        .collect::<Result<_, _>>()?;
    for x in [0.0, 1.0] {
        let all: BTreeMap<String, f64> = macros.iter().map(|m| (m.name.clone(), x)).collect();
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
