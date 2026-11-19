//! The catalog: saved sounds, as `<library>/sounds/<name>.json` plus an index
//! `<library>/catalog.json` (description, tags, prompt, kind, length, LUFS).
//! Adapted from modeler's `catalog.rs` (one kind: sounds).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sfx_graph::{Kind, SoundDoc};

use crate::store::atomic_write;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

/// What `catalog search` filters on. Every word of `text` must appear in
/// the name, description, tags, role or material (case-insensitive).
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub text: Option<String>,
    pub role: Option<String>,
    pub material: Option<String>,
    pub approved: Option<bool>,
}

pub fn search(entries: &[CatalogEntry], q: &Query) -> Vec<CatalogEntry> {
    let words: Vec<String> = q
        .text
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let mut hits: Vec<CatalogEntry> = entries
        .iter()
        .filter(|e| {
            let hay = format!(
                "{} {} {} {} {}",
                e.name,
                e.description,
                e.tags.join(" "),
                e.role.as_deref().unwrap_or(""),
                e.material.as_deref().unwrap_or("")
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w))
                && q.role.as_ref().is_none_or(|r| e.role.as_ref() == Some(r))
                && q.material
                    .as_ref()
                    .is_none_or(|m| e.material.as_deref().is_some_and(|x| x == m || x == "any"))
                && q.approved.is_none_or(|a| e.approved == a)
        })
        .cloned()
        .collect();
    // Approved first, then by name.
    hits.sort_by(|a, b| b.approved.cmp(&a.approved).then(a.name.cmp(&b.name)));
    hits
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
