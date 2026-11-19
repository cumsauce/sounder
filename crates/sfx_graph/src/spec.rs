//! The brief as data: what a sound is meant to be made of.
//!
//! A [`Spec`] lists the sound's components, each a role (the layer it
//! fills), the node that makes it, and optionally its material, its time
//! slot and its band. It is written before the graph is built (a component
//! with no node is a layer still to build) and checked against the graph:
//! `summary` shows which components are built, and `verify` (the `spec`
//! check) renders each component's node and measures its slot and band.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The roles a component may fill (the same words as a part's role).
pub const ROLES: &[&str] = &[
    "pre", "crack", "body", "weight", "after", "bed", "texture", "whole",
];

/// What a sound is meant to be made of.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// The request in words ("a heavy wooden door slammed shut").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub brief: String,
    /// The layers, in the order they sound.
    #[serde(default)]
    pub components: Vec<Component>,
}

/// One layer of the sound.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// pre, crack, body, weight, after, bed, texture or whole.
    pub role: String,
    /// The node that makes this layer (its last node before the mix, e.g.
    /// its `gain`). Left out: the layer is not built yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// wood, metal, glass, ... : checked against the `material` fields of
    /// the node and everything it reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// [start, end] seconds: 80 % of the layer's energy must fall inside
    /// (10 ms of margin each side).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
    /// [low, high] Hz: at least half of the layer's energy must fall
    /// inside. Left out: the role's band (see [`role_band`]); `after`,
    /// `bed`, `texture` and `whole` have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<[f32; 2]>,
    /// [min, max] ms: how long the layer lasts, measured as the span that
    /// holds the middle 80 % of its energy. In the approved door slam: the
    /// crack 1 ms, the noise-struck modal panel 5 ms, the band-passed noise
    /// body 94 ms, the weight 73 ms. A modal body is mostly its strike; a
    /// hit's length comes from a noise body and the weight under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_ms: Option<[f32; 2]>,
    /// What this layer is, in words ("the door brushing its frame").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The band a role's energy should sit in when the component names none:
/// the table in AGENTS.md "Layering a designed sound", widened so the
/// approved door slam's layers pass (brown-noise weight carries most of
/// its energy under 60 Hz; a crack's white noise runs to 12 kHz).
pub fn role_band(role: &str) -> Option<[f32; 2]> {
    match role {
        "pre" => Some([1000.0, 8000.0]),
        "crack" => Some([1500.0, 16000.0]),
        "body" => Some([100.0, 3000.0]),
        "weight" => Some([20.0, 400.0]),
        _ => None,
    }
}

impl Component {
    /// The band to check: the component's, else its role's.
    pub fn band(&self) -> Option<[f32; 2]> {
        self.band.or_else(|| role_band(&self.role))
    }

    /// `role` (`node`, material, at a-b s, lo-hi Hz).
    pub fn line(&self) -> String {
        let mut parts = vec![match &self.node {
            Some(n) => format!("`{n}`"),
            None => "no node yet".into(),
        }];
        if let Some(m) = &self.material {
            parts.push(m.clone());
        }
        if let Some([a, b]) = self.at {
            parts.push(format!("at {a}-{b} s"));
        }
        if let Some([lo, hi]) = self.length_ms {
            parts.push(format!("lasting {lo}-{hi} ms"));
        }
        if let Some([lo, hi]) = self.band() {
            parts.push(format!(
                "{}-{} Hz{}",
                lo,
                hi,
                if self.band.is_none() { " (role)" } else { "" }
            ));
        }
        let mut s = format!("{} ({})", self.role, parts.join(", "));
        if !self.note.is_empty() {
            s += &format!(": {}", self.note);
        }
        s
    }
}

impl Spec {
    /// Applies `spec_update` items: `{"index": i, ...fields}` merges into
    /// component i; `index` equal to the count appends one (it needs a role).
    pub fn update(&mut self, items: &[Value]) -> Result<(), String> {
        for (k, item) in items.iter().enumerate() {
            let at = format!("spec_update #{k}");
            let Value::Object(m) = item else {
                return Err(format!(
                    "{at}: give an object {{\"index\": 0, \"node\": \"crk_g\"}}"
                ));
            };
            let Some(i) = m.get("index").and_then(Value::as_u64).map(|i| i as usize) else {
                return Err(format!(
                    "{at}: `index` (the component's position, from 0) is missing"
                ));
            };
            let n = self.components.len();
            if i > n {
                return Err(format!(
                    "{at}: index {i}: the spec has {n} components (0..{}; {n} appends one)",
                    n.saturating_sub(1)
                ));
            }
            let mut v = if i < n {
                serde_json::to_value(&self.components[i]).map_err(|e| e.to_string())?
            } else {
                Value::Object(Default::default())
            };
            let obj = v.as_object_mut().ok_or("component is not an object")?;
            for (key, val) in m {
                if key == "index" {
                    continue;
                }
                if val.is_null() {
                    obj.remove(key);
                } else {
                    obj.insert(key.clone(), val.clone());
                }
            }
            let c: Component = serde_json::from_value(v).map_err(|e| format!("{at}: {e}"))?;
            if i < n {
                self.components[i] = c;
            } else {
                self.components.push(c);
            }
        }
        Ok(())
    }

    /// Checks the spec's own shape: known roles, ordered ranges.
    pub fn validate(&self) -> Result<(), String> {
        for (k, c) in self.components.iter().enumerate() {
            let at = format!("spec component #{k} ({})", c.role);
            if !ROLES.contains(&c.role.as_str()) {
                return Err(format!(
                    "{at}: role `{}`: use one of {}",
                    c.role,
                    ROLES.join(", ")
                ));
            }
            if let Some([a, b]) = c.at
                && !(a >= 0.0 && b > a)
            {
                return Err(format!(
                    "{at}: at [{a}, {b}]: give [start, end] seconds with end after start"
                ));
            }
            if let Some([lo, hi]) = c.band
                && !(lo >= 0.0 && hi > lo)
            {
                return Err(format!(
                    "{at}: band [{lo}, {hi}]: give [low, high] Hz with high above low"
                ));
            }
            if let Some([lo, hi]) = c.length_ms
                && !(lo >= 0.0 && hi > lo)
            {
                return Err(format!(
                    "{at}: length_ms [{lo}, {hi}]: give [min, max] ms with max above min"
                ));
            }
        }
        Ok(())
    }

    /// The names a node reaches through what it reads (itself included),
    /// across the raw node list.
    pub fn upstream<'a>(nodes: &'a [Value], start: &str) -> Vec<&'a Value> {
        let by_name = |n: &str| {
            nodes
                .iter()
                .find(|v| v.get("name").and_then(Value::as_str) == Some(n))
        };
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        let mut todo = vec![start.to_string()];
        while let Some(n) = todo.pop() {
            if seen.contains(&n) {
                continue;
            }
            seen.push(n.clone());
            let Some(v) = by_name(&n) else { continue };
            out.push(v);
            refs(v, &mut |r| {
                let base = r.split('.').next().unwrap_or(r);
                if by_name(base).is_some() && !seen.iter().any(|s| s == base) {
                    todo.push(base.to_string());
                }
            });
        }
        out
    }

    /// The graph-only findings for each component, without rendering:
    /// (component line, problems). A problem is a missing or unreached
    /// node, or a material that differs.
    pub fn static_findings(&self, nodes: &[Value]) -> Vec<(String, Vec<String>)> {
        let output = nodes
            .iter()
            .find(|v| v.get("type").and_then(Value::as_str) == Some("output"))
            .and_then(|v| v.get("name").and_then(Value::as_str));
        let reached: Vec<&str> = output
            .map(|o| {
                Self::upstream(nodes, o)
                    .into_iter()
                    .filter_map(|v| v.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        self.components
            .iter()
            .map(|c| {
                let mut p = Vec::new();
                match &c.node {
                    None => p.push("not built: name its node once it exists (sound_ops spec_update [{\"index\": i, \"node\": ...}])".into()),
                    Some(n)
                        if !nodes
                            .iter()
                            .any(|v| v.get("name").and_then(Value::as_str) == Some(n)) =>
                    {
                        p.push(format!("no node named `{n}`"))
                    }
                    Some(n) if !reached.contains(&n.as_str()) => {
                        p.push(format!("`{n}` does not reach the output: mix it in"))
                    }
                    Some(n) => {
                        if let Some(want) = &c.material {
                            let found = materials(nodes, n);
                            if !found.is_empty() && !found.iter().any(|m| m == want) {
                                p.push(format!(
                                    "material {want} wanted, the layer's nodes say {}",
                                    found.join(", ")
                                ));
                            }
                        }
                    }
                }
                (c.line(), p)
            })
            .collect()
    }
}

/// Every `material` field on a node and what it reads (patches included).
pub fn materials(nodes: &[Value], start: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in Spec::upstream(nodes, start) {
        collect_materials(v, &mut out);
    }
    out
}

fn collect_materials(v: &Value, out: &mut Vec<String>) {
    if let Some(m) = v.get("material").and_then(Value::as_str)
        && !out.iter().any(|x| x == m)
    {
        out.push(m.to_string());
    }
    if let Some(Value::Array(p)) = v.get("patch") {
        for n in p {
            collect_materials(n, out);
        }
    }
}

/// Calls `f` with every string in a node's fields (except its type and
/// name): the candidates for references to other nodes.
fn refs(v: &Value, f: &mut impl FnMut(&str)) {
    fn walk(v: &Value, f: &mut impl FnMut(&str)) {
        match v {
            Value::String(s) => f(s),
            Value::Array(a) => a.iter().for_each(|x| walk(x, f)),
            Value::Object(m) => m.values().for_each(|x| walk(x, f)),
            _ => {}
        }
    }
    if let Value::Object(m) = v {
        for (k, x) in m {
            if k != "type" && k != "name" && k != "patch" {
                walk(x, f);
            }
        }
    }
}
