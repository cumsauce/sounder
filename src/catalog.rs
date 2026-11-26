//! The catalog: saved sounds, as `<library>/sounds/<name>.json` plus an index
//! `<library>/catalog.json` (description, tags, prompt, kind, length, LUFS).
//! Adapted from modeler's `catalog.rs` (one kind: sounds).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sfx_graph::{Kind, SoundDoc};

use crate::store::atomic_write;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CatalogEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// What the human asked for, in their words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub kind: Kind,
    /// Seconds, as last rendered.
    #[serde(default)]
    pub length: f32,
    #[serde(default)]
    pub lufs: f32,
    /// Macro names (knobs) it carries.
    #[serde(default)]
    pub macros: Vec<String>,
    /// The layer it fills in a designed sound (`parts::ROLES`): set on a
    /// part, absent on an ordinary saved sound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// The human approved it by ear (only the window sets this; saving a
    /// changed document clears it).
    #[serde(default)]
    pub approved: bool,
    /// Shipped with the tool (`parts/`), not saved in this library.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
    /// The project and group it is filed under (`projects`), as the
    /// document says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

fn sounds_dir(lib: &Path) -> PathBuf {
    lib.join("sounds")
}

fn index_path(lib: &Path) -> PathBuf {
    lib.join("catalog.json")
}

pub fn list(lib: &Path) -> Vec<CatalogEntry> {
    std::fs::read(index_path(lib))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The `n` most recently saved sounds, newest first, with the time each file
/// was written.
pub fn recent(lib: &Path, n: usize) -> Vec<(CatalogEntry, std::time::SystemTime)> {
    let mut v: Vec<(CatalogEntry, std::time::SystemTime)> = list(lib)
        .into_iter()
        .filter_map(|e| {
            let t = std::fs::metadata(sounds_dir(lib).join(format!("{}.json", e.name)))
                .and_then(|m| m.modified())
                .ok()?;
            Some((e, t))
        })
        .collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.truncate(n);
    v
}

fn write_index(lib: &Path, entries: &[CatalogEntry]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    atomic_write(&index_path(lib), &bytes)
}

/// Saves `doc` under its own name with `entry`'s metadata.
pub fn save(lib: &Path, doc: &SoundDoc, mut entry: CatalogEntry) -> Result<(), String> {
    entry.name = doc.name.clone();
    entry.kind = doc.kind;
    entry.macros = doc.macros.iter().map(|m| m.name.clone()).collect();
    let bytes = serde_json::to_vec_pretty(doc).map_err(|e| e.to_string())?;
    atomic_write(&sounds_dir(lib).join(format!("{}.json", doc.name)), &bytes)?;
    let mut entries = list(lib);
    match entries.iter().position(|e| e.name == entry.name) {
        Some(i) => entries[i] = entry,
        None => entries.push(entry),
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    write_index(lib, &entries)
}

/// Files a saved sound under `project` / `group`, in its file and the index,
/// keeping its approval (the sound itself is unchanged).
pub fn set_filing(
    lib: &Path,
    name: &str,
    project: Option<String>,
    group: Option<String>,
) -> Result<(), String> {
    let mut doc = load(lib, name)?;
    let mut all = list(lib);
    let Some(e) = all.iter_mut().find(|e| e.name == name) else {
        return Err(format!("`{name}` is not saved in this library"));
    };
    doc.project = project.clone();
    doc.group = group.clone();
    e.project = project;
    e.group = group;
    let json = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    atomic_write(&sounds_dir(lib).join(format!("{name}.json")), &json)?;
    let idx = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
    atomic_write(&index_path(lib), &idx)
}

/// Sets or clears the human's approval of a saved sound.
pub fn set_approved(lib: &Path, name: &str, approved: bool) -> Result<(), String> {
    let mut entries = list(lib);
    let Some(e) = entries.iter_mut().find(|e| e.name == name) else {
        return Err(format!(
            "no saved sound `{name}`: save it to the catalog first"
        ));
    };
    e.approved = approved;
    write_index(lib, &entries)
}

/// The library's sounds plus the built-in parts a library sound does not
/// shadow.
pub fn list_with_builtin(lib: &Path) -> Vec<CatalogEntry> {
    let mut v = list(lib);
    for p in crate::parts::builtin() {
        if !v.iter().any(|e| e.name == p.doc.name) {
            v.push(p.entry());
        }
    }
    v
}

/// What `catalog search` filters on. `text` is split into words; an entry
/// matches when at least one word matches a word of its name, description,
/// tags, role or material (case-insensitive, simple plurals and endings
/// folded, a word of 3+ letters also matches the start of a longer word), and
/// entries matching more words come first.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: Option<String>,
    pub role: Option<String>,
    pub material: Option<String>,
    pub approved: Option<bool>,
    /// Only sounds filed under this project (and group).
    pub project: Option<String>,
    pub group: Option<String>,
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "of", "for", "and", "or", "to", "in", "on", "with", "my", "some",
];

/// Lower-case words of `text`, split on anything that is not a letter or digit.
fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect()
}

/// Folds plurals and common endings: motors -> motor, whirring -> whir,
/// clicked -> click, boxes -> box. Short words are left alone.
fn stem(w: &str) -> String {
    let mut s = w.to_string();
    for suf in ["ing", "ed", "es", "s"] {
        if s.len() >= suf.len() + 3 && s.ends_with(suf) && !s.ends_with("ss") {
            s.truncate(s.len() - suf.len());
            let b = s.as_bytes();
            let n = b.len();
            // whirr -> whir, stopp -> stop
            if suf != "s" && suf != "es" && n >= 2 && b[n - 1] == b[n - 2] && b[n - 1] != b'l' {
                s.pop();
            }
            break;
        }
    }
    s
}

fn word_hit(q: &str, hay: &[String]) -> bool {
    hay.iter().any(|h| {
        h == q
            || (q.len() >= 3 && h.starts_with(q))
            || (h.len() >= 4 && q.starts_with(h.as_str()) && q.len() <= h.len() + 2)
    })
}

pub fn search(entries: &[CatalogEntry], q: &Query) -> Vec<CatalogEntry> {
    let words: Vec<String> = words_of(q.text.as_deref().unwrap_or(""))
        .into_iter()
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect();
    let mut hits: Vec<(usize, CatalogEntry)> = entries
        .iter()
        .filter(|e| {
            q.role.as_ref().is_none_or(|r| e.role.as_ref() == Some(r))
                && q.material
                    .as_ref()
                    .is_none_or(|m| e.material.as_deref().is_some_and(|x| x == m || x == "any"))
                && q.approved.is_none_or(|a| e.approved == a)
                && q.project
                    .as_ref()
                    .is_none_or(|p| e.project.as_ref() == Some(p))
                && q.group.as_ref().is_none_or(|g| e.group.as_ref() == Some(g))
        })
        .filter_map(|e| {
            if words.is_empty() {
                return Some((0, e.clone()));
            }
            let hay = words_of(&format!(
                "{} {} {} {} {}",
                e.name,
                e.description,
                e.tags.join(" "),
                e.role.as_deref().unwrap_or(""),
                e.material.as_deref().unwrap_or("")
            ));
            let n = words.iter().filter(|w| word_hit(w, &hay)).count();
            (n > 0).then(|| (n, e.clone()))
        })
        .collect();
    // More word hits first, then approved, then by name.
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.approved.cmp(&a.1.approved))
            .then(a.1.name.cmp(&b.1.name))
    });
    hits.into_iter().map(|(_, e)| e).collect()
}

/// Words a brief carries that say nothing about which node fits it.
const BRIEF_NOISE: &[&str] = &[
    "sound", "sfx", "effect", "like", "game", "into", "from", "by",
];

fn brief_words(text: &str) -> Vec<String> {
    words_of(text)
        .into_iter()
        .filter(|w| !STOP_WORDS.contains(&w.as_str()) && !BRIEF_NOISE.contains(&w.as_str()))
        .collect()
}

/// A brief word against a node or template name: stricter than `word_hit`
/// ("rev" from "revving" must not name `reverb`), and a compound brief word
/// ends with the name ("footstep" names `step`).
fn name_hit(q: &str, name: &[String]) -> bool {
    name.iter().any(|h| {
        h == q
            || (q.len() >= 4 && h.starts_with(q))
            || (h.len() >= 4
                && (q.starts_with(h.as_str()) && q.len() <= h.len() + 2 || q.ends_with(h.as_str())))
    })
}

/// Templates and ready-made node types whose name (or a template's brief,
/// a node's one-line blurb) shares words with `text`: the routing that
/// AGENTS.md's table does by hand. A name hit counts three times a blurb
/// hit, so `fire` finds the `fire` node before nodes that mention fire.
/// Returns (templates, nodes), best first, at most `max` nodes.
pub fn brief_matches(text: &str, max: usize) -> (Vec<String>, Vec<(String, &'static str)>) {
    let words = brief_words(text);
    if words.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let score = |name: &str, body: &str| {
        let n = words_of(&name.replace('_', " "));
        let b = words_of(body);
        words
            .iter()
            .map(|w| {
                if name_hit(w, &n) {
                    3
                } else if word_hit(w, &b) {
                    1
                } else {
                    0
                }
            })
            .sum::<usize>()
    };
    let templates = crate::session::TEMPLATES
        .iter()
        .filter_map(|(n, body)| {
            let brief = serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v["spec"]["brief"].as_str().map(str::to_string))
                .unwrap_or_default();
            // A template needs a name word or two brief words: "a heavy door" yes, "heavy" no.
            let sc = score(n, &brief);
            (sc >= 2).then(|| (sc, n.to_string()))
        })
        .collect::<Vec<_>>();
    let mut templates = templates;
    templates.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut nodes: Vec<(usize, String, &'static str)> = sfx_graph::NODE_TYPES
        .iter()
        .filter(|t| **t != "output")
        .filter_map(|t| {
            let bl = sfx_graph::blurb(t);
            let sc = score(t, bl);
            (sc > 0).then(|| (sc, t.to_string(), bl))
        })
        .collect();
    nodes.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    // A name hit wins: then only nodes whose name matched stay. Without one a
    // node needs two shared words, so one common blurb word does not flood it.
    let named = nodes.first().is_some_and(|n| n.0 >= 3);
    nodes.retain(|n| if named { n.0 >= 3 } else { n.0 >= 2 });
    nodes.truncate(max);
    (
        templates.into_iter().map(|(_, n)| n).collect(),
        nodes.into_iter().map(|(_, n, b)| (n, b)).collect(),
    )
}

/// `brief_matches` as text lines for a tool result ("" when nothing fits).
pub fn format_brief_matches(text: &str, max: usize) -> String {
    let (templates, nodes) = brief_matches(text, max);
    let mut out = String::new();
    for t in &templates {
        out += &format!(
            "- template `{t}`: new_sound {{template: \"{t}\"}} starts from the approved sound (do not rebuild it)\n"
        );
    }
    for (n, b) in &nodes {
        out += &format!("- node `{n}`: {b} (describe_op {{kind: \"{n}\"}})\n");
    }
    out
}

/// Built-in parts whose graph uses node type `ty` (a `voices` patch included).
pub fn parts_using(ty: &str) -> Vec<String> {
    fn uses(nodes: &[serde_json::Value], ty: &str) -> bool {
        nodes.iter().any(|n| {
            n["type"].as_str() == Some(ty) || n["patch"].as_array().is_some_and(|p| uses(p, ty))
        })
    }
    let mut v: Vec<String> = crate::parts::builtin()
        .into_iter()
        .filter(|p| uses(&p.doc.nodes, ty))
        .map(|p| p.doc.name)
        .collect();
    v.sort();
    v
}

pub fn format_search(hits: &[CatalogEntry]) -> String {
    if hits.is_empty() {
        return "no match (catalog search with no arguments lists every part and saved sound)"
            .into();
    }
    let mut out = format!(
        "{} match(es); use one as a layer with {{\"type\": \"sound\", \"name\": \"x\", \"sound\": \"<name>\", \"at\": s, \"gain_db\": dB, \"macros\": {{knob: 0..1}}}}\n",
        hits.len()
    );
    for e in hits {
        out += &format!(
            "- `{}` [{}{}{}{}]: {}{}\n",
            e.name,
            e.role.as_deref().unwrap_or("sound"),
            e.material
                .as_deref()
                .map(|m| format!(", {m}"))
                .unwrap_or_default(),
            if e.approved {
                ", approved"
            } else {
                ", not approved"
            },
            if e.builtin { ", built in" } else { "" },
            e.description,
            if e.macros.is_empty() {
                String::new()
            } else {
                format!(" (knobs: {})", e.macros.join(", "))
            }
        );
    }
    out
}

pub fn load(lib: &Path, name: &str) -> Result<SoundDoc, String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "sound name `{name}`: use letters, digits, '_' and '-'"
        ));
    }
    let path = sounds_dir(lib).join(format!("{name}.json"));
    if !path.exists()
        && let Some(p) = crate::parts::find(name)
    {
        return Ok(p.doc);
    }
    let bytes = std::fs::read(&path).map_err(|_| {
        let names: Vec<String> = list(lib).into_iter().map(|e| e.name).collect();
        format!(
            "no saved sound `{name}` in {} (saved: {})",
            lib.display(),
            if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            }
        )
    })?;
    let doc: SoundDoc =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    doc.check_format()?;
    Ok(doc)
}

pub fn delete(lib: &Path, name: &str) -> Result<(), String> {
    let mut entries = list(lib);
    let Some(i) = entries.iter().position(|e| e.name == name) else {
        return Err(format!("no saved sound `{name}`"));
    };
    entries.remove(i);
    let _ = std::fs::remove_file(sounds_dir(lib).join(format!("{name}.json")));
    write_index(lib, &entries)
}

pub fn format_list(entries: &[CatalogEntry]) -> String {
    if entries.is_empty() {
        return "catalog is empty".into();
    }
    let mut out = format!("{} saved sound(s)\n", entries.len());
    for e in entries {
        out += &format!(
            "- `{}`{}: {} [{}] {:?}, {:.2} s, {:.1} LUFS{}\n",
            e.name,
            match (&e.role, e.approved) {
                (Some(r), true) => format!(" ({r} part, approved)"),
                (Some(r), false) => format!(" ({r} part)"),
                (None, true) => " (approved)".into(),
                (None, false) => String::new(),
            },
            e.description,
            e.tags.join(", "),
            e.kind,
            e.length,
            e.lufs,
            if e.macros.is_empty() {
                String::new()
            } else {
                format!(" (knobs: {})", e.macros.join(", "))
            }
        );
    }
    out
}

#[cfg(test)]
mod brief_tests {
    use super::*;

    fn nodes(b: &str) -> Vec<String> {
        brief_matches(b, 5).1.into_iter().map(|(n, _)| n).collect()
    }

    #[test]
    fn briefs_route_to_templates_and_nodes() {
        let (t, _) = brief_matches("a heavy wooden door slammed shut", 5);
        assert_eq!(t, ["door_slam"]);
        assert_eq!(nodes("a campfire crackling, fire"), ["fire"]);
        assert_eq!(nodes("an explosion far away"), ["explosion"]);
        assert_eq!(nodes("a car engine revving"), ["engine"]);
        assert!(nodes("footsteps on gravel").contains(&"step".to_string()));
        assert!(nodes("a metal sign hit while the wind pushes it").contains(&"wind".to_string()));
        // `plate` is a node of its own (the nonlinear one), and a name hit wins;
        // its blurb points back at `modal` for the linear plate.
        assert!(nodes("a struck plate").contains(&"plate".to_string()));
        assert!(sfx_graph::blurb("plate").contains("modal"));
        assert!(nodes("a struck object ringing its modes").contains(&"modal".to_string()));
        assert!(brief_matches("the", 5).1.is_empty());
        assert!(brief_matches("heavy", 5).0.is_empty());
    }

    #[test]
    fn parts_using_finds_the_impact_parts() {
        let p = parts_using("impact");
        assert!(p.contains(&"impact_bowed_bar".to_string()), "{p:?}");
        assert!(parts_using("no_such_node").is_empty());
    }
}
