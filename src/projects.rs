//! Projects and groups: how the library's sounds are organized, and the
//! style context agents read before building a sound that has to fit.
//!
//! A **project** is a whole body of work (a game, a video series); a
//! **group** is any set of sounds inside it (one machine's noises, the UI
//! set, the forest ambience). Each carries a `style` (the guide, replaced
//! when rewritten) and `notes` (what was learned, appended: "the listener
//! wants the hits drier"). Kept in `<library>/projects.json`; a sound says
//! where it belongs with its own `project` / `group` fields.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store::atomic_write;

/// One learned fact, kept in the order it was added.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub text: String,
    /// The client (agent) or "window" that added it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Seconds since the Unix epoch.
    #[serde(default)]
    pub at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// How this group's sounds should come out: shared character, levels,
    /// the layers they all have, what to avoid.
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The project's sound style: what every sound in it shares.
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub groups: Vec<Group>,
}

impl Project {
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }
    pub fn group_mut(&mut self, name: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.name == name)
    }
}

fn path(lib: &Path) -> PathBuf {
    lib.join("projects.json")
}

/// Every project, in the order they were made.
pub fn list(lib: &Path) -> Vec<Project> {
    std::fs::read(path(lib))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_all(lib: &Path, all: &[Project]) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?;
    atomic_write(&path(lib), &json)
}

pub fn get(lib: &Path, name: &str) -> Option<Project> {
    list(lib).into_iter().find(|p| p.name == name)
}

/// A project or group name: what a person would type; letters, digits,
/// spaces, '_' and '-', 1..48 characters.
pub fn valid_name(what: &str, n: &str) -> Result<(), String> {
    let ok = !n.trim().is_empty()
        && n.len() <= 48
        && n == n.trim()
        && n.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{what} name `{n}`: use 1..48 letters, digits, spaces, '_' or '-' (no leading or trailing space)"
        ))
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Checks that `project` (and `group` in it) exist: the error lists what
/// does, so an agent can pick or create one.
pub fn check(lib: &Path, project: &str, group: Option<&str>) -> Result<(), String> {
    let all = list(lib);
    let Some(p) = all.iter().find(|p| p.name == project) else {
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        return Err(format!(
            "project `{project}` does not exist ({}): make it with project {{action: \"create\", name: \"{project}\"}}",
            if names.is_empty() {
                "there are none yet".to_string()
            } else {
                format!("projects: {}", names.join(", "))
            }
        ));
    };
    if let Some(g) = group
        && p.group(g).is_none()
    {
        let names: Vec<&str> = p.groups.iter().map(|g| g.name.as_str()).collect();
        return Err(format!(
            "group `{g}` is not in project `{project}` ({}): make it with project {{action: \"create\", name: \"{project}\", group: \"{g}\"}}",
            if names.is_empty() {
                "it has no groups yet".to_string()
            } else {
                format!("groups: {}", names.join(", "))
            }
        ));
    }
    Ok(())
}

fn notes_text(notes: &[Note], max: usize) -> String {
    let skip = notes.len().saturating_sub(max);
    let mut t = String::new();
    if skip > 0 {
        t += &format!("  ({skip} older notes: project {{action: \"describe\"}} lists them)\n");
    }
    for n in &notes[skip..] {
        t += &format!(
            "  - {}{}\n",
            n.text,
            n.by.as_deref()
                .map(|b| format!(" ({b})"))
                .unwrap_or_default()
        );
    }
    t
}

/// The style context for a sound in `project` / `group`: what an agent
/// reads before building (new_sound, summary). Empty when there is none.
pub fn context(lib: &Path, project: Option<&str>, group: Option<&str>, max_notes: usize) -> String {
    let Some(pn) = project else {
        return String::new();
    };
    let Some(p) = get(lib, pn) else {
        return format!("project `{pn}` (not in projects.json: it was deleted or renamed)\n");
    };
    let mut t = format!(
        "project `{}`{}\n",
        p.name,
        if p.description.is_empty() {
            String::new()
        } else {
            format!(": {}", p.description)
        }
    );
    if !p.style.is_empty() {
        t += &format!("  style: {}\n", p.style);
    }
    t += &notes_text(&p.notes, max_notes);
    if let Some(gn) = group {
        match p.group(gn) {
            Some(g) => {
                t += &format!(
                    "group `{}`{}\n",
                    g.name,
                    if g.description.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", g.description)
                    }
                );
                if !g.style.is_empty() {
                    t += &format!("  style: {}\n", g.style);
                }
                t += &notes_text(&g.notes, max_notes);
            }
            None => t += &format!("group `{gn}` (not in the project any more)\n"),
        }
    }
    if p.style.is_empty()
        && group
            .and_then(|g| p.group(g))
            .is_none_or(|g| g.style.is_empty())
    {
        t += "  no style written yet: once the human says what they want, record it with project {action: \"update\", style: ...} or a note\n";
    }
    t
}
