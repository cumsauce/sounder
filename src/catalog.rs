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

pub fn load(lib: &Path, name: &str) -> Result<SoundDoc, String> {
    let path = sounds_dir(lib).join(format!("{name}.json"));
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
            "- `{}`: {} [{}] {:?}, {:.2} s, {:.1} LUFS{}\n",
            e.name,
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
