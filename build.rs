//! Embeds `ui/` (the window's page, script, style, fonts, fixture) into the
//! binary: writes `$OUT_DIR/ui_assets.rs`, a table of `(path, bytes)`. A file
//! added to `ui/` needs no code change. An absent `ui/` yields an empty
//! table (the window then serves its built-in fallback page).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let name = rel.to_string_lossy().replace('\\', "/");
            if !name.starts_with('.') && !name.contains("/.") {
                out.push((name, p));
            }
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("ui");
    println!("cargo:rerun-if-changed=ui");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut src = String::from("pub static UI_ASSETS: &[(&str, &[u8])] = &[\n");
    for (name, path) in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let _ = writeln!(
            src,
            "    ({name:?}, include_bytes!({:?})),",
            path.display().to_string()
        );
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default()).join("ui_assets.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo:warning=could not write {}: {e}", out.display());
    }
}
