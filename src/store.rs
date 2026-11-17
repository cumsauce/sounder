//! Where things live on disk, and the small IO helpers. Adapted from
//! modeler's `src/catalog.rs` (`atomic_write`, project resolution, the
//! session lock).
//!
//! - In a game project (a folder with `.sounder/`, or a `Cargo.toml` beside
//!   `assets/`) the library is `<project>/.sounder/` and exports go to
//!   `<project>/assets/sounds/`.
//! - Otherwise `~/Library/Application Support/friendslop/sounder/` (or the
//!   XDG data dir). `FRIENDSLOP_DATA_DIR` overrides both (tests set it to a
//!   tempdir).

use std::fs;
use std::path::{Path, PathBuf};

/// The project folder name.
pub const PROJECT_DIR: &str = ".sounder";

/// The nearest ancestor of `start` that is a sounder project.
pub fn find_project(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| {
            d.join(PROJECT_DIR).is_dir()
                || (d.join("Cargo.toml").is_file() && d.join("assets").is_dir())
        })
        .map(PathBuf::from)
}

fn native_data_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME")
        .map_err(|_| "HOME is not set: set FRIENDSLOP_DATA_DIR".to_string())?;
    if cfg!(target_os = "macos") {
        return Ok(PathBuf::from(home).join("Library/Application Support"));
    }
    Ok(std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(home).join(".local/share")))
}

/// The library folder: `.sounder/` of `project`, or the shared data dir.
pub fn library_dir(project: Option<&Path>) -> Result<PathBuf, String> {
    if let Ok(d) = std::env::var("FRIENDSLOP_DATA_DIR")
        && !d.is_empty()
    {
        return Ok(PathBuf::from(d).join("friendslop").join("sounder"));
    }
    if let Some(p) = project {
        return Ok(p.join(PROJECT_DIR));
    }
    Ok(native_data_dir()?.join("friendslop").join("sounder"))
}

/// Where relative export paths land: `<project>/assets/sounds/`, or the
/// working directory outside a project (absolute paths stay as given).
pub fn resolve_export_dir(project: Option<&Path>, path: Option<&str>) -> PathBuf {
    match (path, project) {
        (Some(p), _) if Path::new(p).is_absolute() => PathBuf::from(p),
        (Some(p), Some(root)) => root.join("assets").join("sounds").join(p),
        (None, Some(root)) => root.join("assets").join("sounds"),
        (Some(p), None) => PathBuf::from(p),
        (None, None) => PathBuf::from("sounds"),
    }
}

/// Writes `bytes` to a sibling `.tmp` file and renames it over `path`, so a
/// crash mid-write never leaves a torn file behind.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("renaming {}: {e}", tmp.display()))
}

/// Keeps per-machine state out of the project's git history (written once,
/// when `.sounder/` is first created; the catalog's sounds stay tracked).
pub fn ensure_project_gitignore(project: &Path) {
    let dir = project.join(PROJECT_DIR);
    let file = dir.join(".gitignore");
    if dir.is_dir() && !file.exists() {
        let _ = fs::write(
            &file,
            "session*.json\nsession.lock\nfeedback.jsonl\ndaemon.log*\n",
        );
    }
}

// --------------------------------------------------------------- the lock

/// Who may autosave: the outcome of taking `session.lock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockOutcome {
    /// This process holds the lock and autosaves.
    Owner,
    /// Another live server (this pid) holds it; this one does not autosave.
    HeldBy(u32),
}

/// Whether a process with this pid exists.
pub fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists() || process_exists(pid)
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    // `kill -0` through the shell tool avoids unsafe code: exit 0 = exists.
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(unix))]
fn process_exists(_pid: u32) -> bool {
    false
}

fn lock_holder(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Takes the lock file at `path` for `pid` unless another live pid holds it.
/// A lock left by a dead process is taken over. The file appears with its
/// content in one step (hard link of a written temp file), so a reader never
/// sees it empty.
pub fn acquire_lock_at(path: &Path, pid: u32) -> Result<LockOutcome, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{pid}.tmp"));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, format!("{pid}\n")).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    let mut outcome = Err(format!(
        "could not take {} (it keeps changing hands)",
        path.display()
    ));
    for _ in 0..3 {
        match fs::hard_link(&tmp, path) {
            Ok(()) => {
                outcome = Ok(LockOutcome::Owner);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match lock_holder(path) {
                Some(h) if h == pid => {
                    outcome = Ok(LockOutcome::Owner);
                    break;
                }
                Some(h) if pid_alive(h) => {
                    outcome = Ok(LockOutcome::HeldBy(h));
                    break;
                }
                // Dead holder or unreadable file: stale, take it over.
                _ => {
                    let _ = fs::remove_file(path);
                }
            },
            Err(e) => {
                outcome = Err(format!("linking {}: {e}", path.display()));
                break;
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    outcome
}

/// Removes the lock at `path` if `pid` holds it (never another server's).
pub fn release_lock_at(path: &Path, pid: u32) {
    if lock_holder(path) == Some(pid) {
        let _ = fs::remove_file(path);
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
