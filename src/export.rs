//! Export: WAV files plus a JSON manifest the game reads.
//!
//! `<name>_<i>.wav` per variation (seeds 1..N; `<name>.wav` for one) and
//! `<name>.sfx.json`: files with their length / LUFS / peak, kind, loop
//! points, the game-facing knobs (macros with `game: true`), the seeds, and
//! the catalog entry it came from.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sfx_graph::{Kind, RenderOpts, SoundDoc, wav};

pub struct ExportOpts {
    pub dir: PathBuf,
    /// Seeds 1..=variations.
    pub variations: u32,
    pub bits: wav::Bits,
    /// Render at this rate instead of the document's.
    pub sample_rate: Option<u32>,
    pub base_dir: Option<PathBuf>,
    pub catalog_name: Option<String>,
}

pub struct Exported {
    pub files: Vec<PathBuf>,
    pub manifest_path: PathBuf,
    pub manifest: Value,
}

pub fn export(doc: &SoundDoc, o: &ExportOpts) -> Result<Exported, String> {
    if o.variations == 0 || o.variations > 64 {
        return Err(format!("variations: {}: use 1..64", o.variations));
    }
    let mut doc = doc.clone();
    if let Some(r) = o.sample_rate {
        doc.sample_rate = r;
    }
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("creating {}: {e}", o.dir.display()))?;
    let mut files = Vec::new();
    let mut entries = Vec::new();
    let mut causal = true;
    for i in 1..=o.variations {
        let seed = if o.variations == 1 {
            doc.seed
        } else {
            u64::from(i)
        };
        let r = doc.render(&RenderOpts {
            seed: Some(seed),
            base_dir: o.base_dir.clone(),
            ..RenderOpts::default()
        })?;
        causal &= r.causal;
        let file = if o.variations == 1 {
            format!("{}.wav", doc.name)
        } else {
            format!("{}_{i}.wav", doc.name)
        };
        let path = o.dir.join(&file);
        wav::write(&path, &r.audio, o.bits)?;
        entries.push(json!({
            "file": file,
            "seed": seed,
            "seconds": r.audio.seconds(),
            "samples": r.audio.len(),
            "lufs": r.lufs,
            "peak_db": r.peak_db,
        }));
        files.push(path);
    }
    let game: Vec<Value> = doc
        .macros
        .iter()
        .filter(|m| m.game)
        .map(|m| json!({"name": m.name, "default": m.default_value(), "doc": m.doc, "map": m.map}))
        .collect();
    let baked: Vec<Value> = doc
        .macros
        .iter()
        .filter(|m| !m.game)
        .map(|m| json!({"name": m.name, "value": m.value}))
        .collect();
    let first_samples = entries[0]["samples"].as_u64().unwrap_or(0);
    let manifest = json!({
        "name": doc.name,
        "kind": doc.kind,
        "sample_rate": doc.sample_rate,
        "bits": match o.bits { wav::Bits::Int16 => 16, wav::Bits::Int24 => 24, wav::Bits::Float32 => 32 },
        "files": entries,
        "loop": (doc.kind == Kind::Loop).then(|| json!({"start": 0, "end": first_samples})),
        "inputs": game,
        "baked_macros": baked,
        "seeds": entries.iter().map(|e| e["seed"].clone()).collect::<Vec<_>>(),
        "causal": causal,
        "source": o.catalog_name,
    });
    let manifest_path = o.dir.join(format!("{}.sfx.json", doc.name));
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    crate::store::atomic_write(&manifest_path, &bytes)?;
    Ok(Exported {
        files,
        manifest_path,
        manifest,
    })
}

/// Where a path argument lands (see `store::resolve_export_dir`).
pub fn dir_for(project: Option<&Path>, path: Option<&str>) -> PathBuf {
    crate::store::resolve_export_dir(project, path)
}
