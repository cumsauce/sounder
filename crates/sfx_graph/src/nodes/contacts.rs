//! `contacts`: an event stream read from a contact / impact log (CSV or
//! JSON) that a physics simulation wrote, so a scene's sound follows its
//! own collisions. The schema is engine-neutral: one row per contact, the
//! column names configurable (`docs/nodes/contacts.md`).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::buf::{Buf, ContactKind, Event, EventExtra};
use crate::node::{Cx, Describe, Eval, Outs};

/// Most contacts one `contacts` may read.
const MAX_CONTACTS: usize = 200_000;

/// The log fields and the column names tried for each, first match wins.
const DEFAULTS: &[(&str, &[&str])] = &[
    ("time", &["time", "t", "seconds", "timestamp"]),
    ("frame", &["frame", "step", "tick"]),
    (
        "strength",
        &["strength", "impulse", "speed", "velocity", "force"],
    ),
    ("body", &["body", "body_id", "object", "id"]),
    ("size", &["size", "radius", "scale"]),
    ("material", &["material", "mat", "surface"]),
    ("kind", &["kind", "type", "event"]),
    ("position", &["position", "pos"]),
    ("x", &["x", "px", "pos_x"]),
    ("y", &["y", "py", "pos_y"]),
    ("z", &["z", "pz", "pos_z"]),
];

/// Contact events read from a file (CSV with a header row, or JSON: an
/// array of objects, or an object holding one such array).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Contacts {
    /// The log: relative paths resolve against the project, then the
    /// sources folders. `.json` is JSON, anything else CSV.
    pub file: String,
    /// Column names for the fields (`time frame strength body size material
    /// kind position x y z`), overriding the defaults: {"strength":
    /// "impulse_ns", "time": "sim_t"}.
    #[serde(default)]
    pub columns: Option<BTreeMap<String, String>>,
    /// Frames per second, when the log gives `frame` and no `time`.
    #[serde(default)]
    pub fps: Option<f64>,
    /// The strength that maps to value 1 (default: the strongest kept
    /// contact). Value = |strength| / reference, clamped to 0..1.
    #[serde(default)]
    pub reference: Option<f64>,
    /// Drop contacts weaker than this, in the log's own units (default 0).
    #[serde(default)]
    pub min_strength: Option<f64>,
    /// Keep only these kinds (impact, slide, roll; default all).
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    /// Keep only these materials, and number them in this order for
    /// `$material` (default: every material, numbered in sorted order).
    #[serde(default)]
    pub materials: Option<Vec<String>>,
    /// The listener's position [x, y, z] (default the origin): positions,
    /// pans and distances are relative to it. x is right, z forward.
    #[serde(default)]
    pub listener: Option<[f32; 3]>,
    /// Seconds added to every time (default 0); negative moves the log
    /// earlier (contacts before 0 are dropped).
    #[serde(default)]
    pub at: Option<f64>,
}

impl Describe for Contacts {
    const BLURB: &'static str = "contact events from a physics log (CSV or JSON: time or frame, strength, body, size, material, position, kind); ports impact slide roll; drives voices or debris";
    // Nothing here reads a node: the filters and the column names (keyed by
    // field) are words.
    const WORDS: &'static [&'static str] = &[
        "kinds",
        "materials",
        "time",
        "frame",
        "strength",
        "body",
        "size",
        "kind",
        "position",
        "x",
        "y",
        "z",
    ];
    fn example() -> serde_json::Value {
        serde_json::json!({"file": "sim/contacts.csv", "reference": 12.0, "min_strength": 0.5, "kinds": ["impact"]})
    }
}

/// One row: field name -> raw value (a number or a string).
type Row = BTreeMap<String, Value>;

fn parse_csv(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    let split = |l: &str| -> Vec<String> {
        l.split(',')
            .map(|c| c.trim().trim_matches('"').to_string())
            .collect()
    };
    let header = split(lines.next().ok_or("the CSV is empty: give a header row")?);
    let mut rows = Vec::new();
    for (i, l) in lines.enumerate() {
        let cells = split(l);
        if cells.len() != header.len() {
            return Err(format!(
                "row {}: {} cells but the header has {}",
                i + 1,
                cells.len(),
                header.len()
            ));
        }
        let row = header
            .iter()
            .zip(cells)
            .map(|(h, c)| {
                let v = c
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::String(c), Value::Number);
                (h.clone(), v)
            })
            .collect();
        rows.push(row);
    }
    Ok(rows)
}

fn parse_json(text: &str) -> Result<Vec<Row>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("JSON: {e}"))?;
    let arr = match v {
        Value::Array(a) => a,
        Value::Object(o) => {
            let arrays: Vec<Vec<Value>> = o
                .into_iter()
                .filter_map(|(_, v)| match v {
                    Value::Array(a) => Some(a),
                    _ => None,
                })
                .collect();
            match <[_; 1]>::try_from(arrays) {
                Ok([a]) => a,
                Err(_) => {
                    return Err(
                        "JSON: give an array of contacts, or an object holding exactly one array"
                            .into(),
                    );
                }
            }
        }
        _ => return Err("JSON: give an array of contacts".into()),
    };
    arr.into_iter()
        .enumerate()
        .map(|(i, r)| match r {
            Value::Object(o) => Ok(o.into_iter().collect()),
            _ => Err(format!("contact #{i}: not an object")),
        })
        .collect()
}

fn kind_of(s: &str) -> Option<ContactKind> {
    match s.to_ascii_lowercase().as_str() {
        "impact" | "hit" | "collision" | "begin" | "start" | "contact" | "bounce" => {
            Some(ContactKind::Impact)
        }
        "slide" | "sliding" | "scrape" | "friction" => Some(ContactKind::Slide),
        "roll" | "rolling" => Some(ContactKind::Roll),
        _ => None,
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Eval for Contacts {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let path = cx.path(&self.file);
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "file: {}: {e} (relative paths resolve against the project, then the sources folders)",
                path.display()
            )
        })?;
        let is_json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let rows = if is_json {
            parse_json(&raw)?
        } else {
            parse_csv(&raw)?
        };
        if rows.len() > MAX_CONTACTS {
            return Err(format!(
                "file: {} contacts: at most {MAX_CONTACTS}",
                rows.len()
            ));
        }
        let cols = self.columns.clone().unwrap_or_default();
        for k in cols.keys() {
            if !DEFAULTS.iter().any(|(f, _)| f == k) {
                let names: Vec<&str> = DEFAULTS.iter().map(|(f, _)| *f).collect();
                return Err(format!(
                    "columns: unknown field `{k}`, expected one of {}",
                    names.join(", ")
                ));
            }
        }
        // The column each field reads: the configured name, else the first
        // default present in the first row.
        let present: Vec<String> = rows
            .first()
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        let column = |field: &str| -> Option<String> {
            if let Some(c) = cols.get(field) {
                return Some(c.clone());
            }
            let (_, names) = DEFAULTS.iter().find(|(f, _)| *f == field)?;
            names
                .iter()
                .find(|n| present.iter().any(|p| p.eq_ignore_ascii_case(n)))
                .and_then(|n| present.iter().find(|p| p.eq_ignore_ascii_case(n)).cloned())
        };
        let c_time = column("time");
        let c_frame = column("frame");
        let c_strength = column("strength");
        if !rows.is_empty() {
            if c_time.is_none() && c_frame.is_none() {
                return Err(format!(
                    "no time column: name one with columns.time (or columns.frame with fps); the log has {}",
                    present.join(", ")
                ));
            }
            if c_strength.is_none() {
                return Err(format!(
                    "no strength column: name one with columns.strength; the log has {}",
                    present.join(", ")
                ));
            }
        }
        let fps = match (c_time.is_some(), self.fps) {
            (false, None) if !rows.is_empty() => {
                return Err("the log gives frames, not times: set fps".into());
            }
            (_, Some(f)) if f <= 0.0 => return Err(format!("fps: {f}: give > 0")),
            (_, f) => f.unwrap_or(0.0),
        };
        let (c_body, c_size, c_mat, c_kind, c_pos) = (
            column("body"),
            column("size"),
            column("material"),
            column("kind"),
            column("position"),
        );
        let (c_x, c_y, c_z) = (column("x"), column("y"), column("z"));
        let kinds: Option<Vec<ContactKind>> = match &self.kinds {
            Some(ks) => Some(
                ks.iter()
                    .map(|k| {
                        kind_of(k)
                            .ok_or_else(|| format!("kinds: `{k}`: give impact, slide or roll"))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            None => None,
        };
        let listener = self.listener.unwrap_or([0.0; 3]);
        let min = self.min_strength.unwrap_or(0.0);
        let at = self.at.unwrap_or(0.0);
        fn get<'a>(r: &'a Row, c: &Option<String>) -> Option<&'a Value> {
            c.as_ref().and_then(|c| r.get(c))
        }

        struct Kept {
            t: f64,
            s: f64,
            mat: Option<String>,
            extra: EventExtra,
        }
        let mut kept = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let bad = |what: &str| format!("contact #{i}: {what}");
            let t = match get(r, &c_time) {
                Some(v) => num(v).ok_or_else(|| bad("time is not a number"))?,
                None => {
                    let f = get(r, &c_frame)
                        .and_then(num)
                        .ok_or_else(|| bad("no time or frame"))?;
                    f / fps
                }
            } + at;
            let s = get(r, &c_strength)
                .and_then(num)
                .ok_or_else(|| bad("strength is not a number"))?
                .abs();
            let kind =
                match get(r, &c_kind) {
                    Some(v) => {
                        let k = text(v);
                        Some(kind_of(&k).ok_or_else(|| {
                            bad(&format!("kind `{k}`: give impact, slide or roll"))
                        })?)
                    }
                    None => None,
                };
            if s < min || !t.is_finite() || t < 0.0 {
                continue;
            }
            if let Some(ks) = &kinds
                && !ks.contains(&kind.unwrap_or(ContactKind::Impact))
            {
                continue;
            }
            let mat = get(r, &c_mat).map(text);
            if let (Some(ms), Some(m)) = (&self.materials, &mat)
                && !ms.iter().any(|x| x.eq_ignore_ascii_case(m))
            {
                continue;
            }
            if self.materials.is_some() && mat.is_none() {
                continue;
            }
            let pos = match get(r, &c_pos) {
                Some(Value::Array(a)) if a.len() == 3 => {
                    let p: Vec<f64> = a.iter().filter_map(num).collect();
                    (p.len() == 3).then(|| [p[0], p[1], p[2]])
                }
                Some(_) => return Err(bad("position: give [x, y, z]")),
                None => match (
                    get(r, &c_x).and_then(num),
                    get(r, &c_y).and_then(num),
                    get(r, &c_z).and_then(num),
                ) {
                    (Some(x), y, z) => Some([x, y.unwrap_or(0.0), z.unwrap_or(0.0)]),
                    _ => None,
                },
            }
            .map(|p| {
                [
                    p[0] as f32 - listener[0],
                    p[1] as f32 - listener[1],
                    p[2] as f32 - listener[2],
                ]
            });
            let extra = EventExtra {
                size: get(r, &c_size).and_then(num).map(|v| v as f32),
                kind: Some(kind.unwrap_or(ContactKind::Impact)),
                body: get(r, &c_body).and_then(num).map(|v| v as u32),
                pos,
                ..Default::default()
            };
            kept.push(Kept { t, s, mat, extra });
        }
        let len = cx.n as f64 / f64::from(cx.sr);
        kept.retain(|k| k.t < len);
        // Material ids: the given order, else sorted names.
        let names: Vec<String> = match &self.materials {
            Some(m) => m.iter().map(|s| s.to_ascii_lowercase()).collect(),
            None => {
                let mut v: Vec<String> = kept
                    .iter()
                    .filter_map(|k| k.mat.as_ref().map(|m| m.to_ascii_lowercase()))
                    .collect();
                v.sort();
                v.dedup();
                v
            }
        };
        let reference = match self.reference {
            Some(r) if r > 0.0 => r,
            Some(r) => return Err(format!("reference: {r}: give > 0")),
            None => kept.iter().map(|k| k.s).fold(0.0, f64::max).max(1e-12),
        };
        kept.sort_by(|a, b| a.t.total_cmp(&b.t));
        let events: Vec<(ContactKind, Event)> = kept
            .into_iter()
            .map(|k| {
                let mut extra = k.extra;
                extra.material = k
                    .mat
                    .and_then(|m| names.iter().position(|n| *n == m.to_ascii_lowercase()))
                    .map(|i| i as u16);
                extra.pan = extra.pan();
                extra.distance = extra.distance();
                let e = Event {
                    t: k.t,
                    value: (k.s / reference).clamp(0.0, 1.0) as f32,
                    id: 0,
                    extra,
                };
                (extra.kind.unwrap_or(ContactKind::Impact), e)
            })
            .collect();
        let stream = |want: Option<ContactKind>| -> Buf {
            Buf::Events(
                events
                    .iter()
                    .filter(|(k, _)| want.is_none_or(|w| w == *k))
                    .enumerate()
                    .map(|(i, (_, e))| Event { id: i as u32, ..*e })
                    .collect(),
            )
        };
        Ok(vec![
            ("out", stream(None)),
            ("impact", stream(Some(ContactKind::Impact))),
            ("slide", stream(Some(ContactKind::Slide))),
            ("roll", stream(Some(ContactKind::Roll))),
        ])
    }
}
