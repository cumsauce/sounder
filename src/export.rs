//! Export: Ogg Vorbis (default) or WAV files plus a JSON manifest the game reads.
//!
//! `<name>_<i>.ogg` per variation (seeds 1..N; `<name>.ogg` for one; `.wav`
//! with `Format::Wav`) and
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
    pub format: Format,
    /// Render at this rate instead of the document's.
    pub sample_rate: Option<u32>,
    pub base_dir: Option<PathBuf>,
    /// Folders searched after `base_dir` (`Session::source_dirs`).
    pub sources: Vec<PathBuf>,
    /// How `sound` nodes reach other sounds.
    pub sounds: Option<sfx_graph::SoundSource>,
    pub catalog_name: Option<String>,
    /// The files' base name (`<file_name>.ogg`, `<file_name>.sfx.json`);
    /// None: the sound's name.
    pub file_name: Option<String>,
    /// Placing the file on a video timeline (default: as rendered).
    pub timeline: Timeline,
}

/// Lines a file up with a video clip: `offset` seconds of leading silence
/// (frame 0 of the clip is sample 0 of the file), then padded with silence or
/// trimmed to `length` seconds (offset included) when given.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timeline {
    pub offset: f64,
    pub length: Option<f64>,
}

impl Timeline {
    fn is_identity(&self) -> bool {
        self.offset == 0.0 && self.length.is_none()
    }

    /// `channels` shifted by the offset and fitted to the length; a trim
    /// fades the last 5 ms out so the cut does not click.
    pub fn apply(&self, channels: &mut [Vec<f32>], sample_rate: u32) {
        if self.is_identity() {
            return;
        }
        let sr = f64::from(sample_rate);
        let lead = (self.offset * sr).round().max(0.0) as usize;
        for c in channels.iter_mut() {
            if lead > 0 {
                c.splice(0..0, std::iter::repeat_n(0.0, lead));
            }
            if let Some(len) = self.length {
                let n = (len * sr).round().max(1.0) as usize;
                if c.len() > n {
                    c.truncate(n);
                    let fade = ((0.005 * sr) as usize).min(n);
                    for k in 0..fade {
                        c[n - fade + k] *= 1.0 - (k + 1) as f32 / fade as f32;
                    }
                } else {
                    c.resize(n, 0.0);
                }
            }
        }
    }
}

/// What the files are written as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    /// Ogg Vorbis at VBR quality 0..1. Sample-exact length (granule
    /// positions), so the manifest's loop points hold.
    Ogg {
        quality: f32,
    },
    Wav(wav::Bits),
}

impl Default for Format {
    fn default() -> Self {
        Format::Ogg {
            quality: crate::ogg::DEFAULT_QUALITY,
        }
    }
}

impl Format {
    fn ext(self) -> &'static str {
        match self {
            Format::Ogg { .. } => "ogg",
            Format::Wav(_) => "wav",
        }
    }
}

pub struct Exported {
    pub files: Vec<PathBuf>,
    pub manifest_path: PathBuf,
    pub manifest: Value,
    /// Things worth knowing about what was written (e.g. identical
    /// variations).
    pub warnings: Vec<String>,
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
    let mut rendered: Vec<Vec<Vec<f32>>> = Vec::new();
    for i in 1..=o.variations {
        let seed = if o.variations == 1 {
            doc.seed
        } else {
            u64::from(i)
        };
        let r = doc.render(&RenderOpts {
            seed: Some(seed),
            base_dir: o.base_dir.clone(),
            sources: o.sources.clone(),
            sounds: o.sounds.clone(),
            ..RenderOpts::default()
        })?;
        let mut r = r;
        o.timeline.apply(&mut r.audio.channels, r.audio.sample_rate);
        causal &= r.causal;
        let ext = o.format.ext();
        let base = o.file_name.as_deref().unwrap_or(&doc.name);
        let file = if o.variations == 1 {
            format!("{base}.{ext}")
        } else {
            format!("{base}_{i}.{ext}")
        };
        let path = o.dir.join(&file);
        match o.format {
            Format::Ogg { quality } => crate::ogg::write(&path, &r.audio, quality)?,
            Format::Wav(bits) => wav::write(&path, &r.audio, bits)?,
        }
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        entries.push(json!({
            "file": file,
            "seed": seed,
            "seconds": r.audio.seconds(),
            "samples": r.audio.len(),
            "lufs": r.lufs,
            "peak_db": r.peak_db,
            "bytes": bytes,
        }));
        files.push(path);
        if o.variations > 1 {
            rendered.push(r.audio.channels.clone());
        }
    }
    let warnings = identical_variations(&rendered);
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
        "format": o.format.ext(),
        "bits": match o.format {
            Format::Wav(wav::Bits::Int16) => json!(16),
            Format::Wav(wav::Bits::Int24) => json!(24),
            Format::Wav(wav::Bits::Float32) => json!(32),
            Format::Ogg { .. } => Value::Null,
        },
        "quality": match o.format { Format::Ogg { quality } => json!(quality), Format::Wav(_) => Value::Null },
        "files": entries,
        "loop": (doc.kind == Kind::Loop).then(|| json!({"start": (o.timeline.offset * f64::from(doc.sample_rate)).round() as u64, "end": first_samples})),
        "inputs": game,
        "baked_macros": baked,
        "seeds": entries.iter().map(|e| e["seed"].clone()).collect::<Vec<_>>(),
        "causal": causal,
        "source": o.catalog_name,
        "fps": doc.fps,
        "offset": o.timeline.offset,
        "start_frame": doc.fps.map(|f| o.timeline.offset * f),
        "length_frames": match (doc.fps, o.timeline.length) {
            (Some(f), Some(l)) => json!(l * f),
            _ => Value::Null,
        },
    });
    let base = o.file_name.as_deref().unwrap_or(&doc.name);
    let manifest_path = o.dir.join(format!("{base}.sfx.json"));
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    crate::store::atomic_write(&manifest_path, &bytes)?;
    Ok(Exported {
        files,
        manifest_path,
        manifest,
        warnings,
    })
}

/// A warning when two variations (seed k = variation k) came out bit for bit
/// the same: nothing in the sound reads the seed, so N files are one sound.
fn identical_variations(rendered: &[Vec<Vec<f32>>]) -> Vec<String> {
    let same = |a: &Vec<Vec<f32>>, b: &Vec<Vec<f32>>| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| {
                x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
            })
    };
    let mut pairs = Vec::new();
    for i in 0..rendered.len() {
        for j in (i + 1)..rendered.len() {
            if same(&rendered[i], &rendered[j]) {
                pairs.push((i + 1, j + 1));
            }
        }
    }
    if pairs.is_empty() {
        return Vec::new();
    }
    let all = pairs.len() == rendered.len() * (rendered.len() - 1) / 2;
    let which = if all {
        format!("all {} variations are", rendered.len())
    } else {
        let list: Vec<String> = pairs.iter().map(|(a, b)| format!("{a} and {b}")).collect();
        format!("variations {} are", list.join("; "))
    };
    vec![format!(
        "{which} bit-identical: no node reads the seed, so the variations are the same; add jitter or a seeded node (noise, a wander, a pattern with jitter, poisson)"
    )]
}

/// Where a path argument lands (see `store::resolve_export_dir`).
pub fn dir_for(project: Option<&Path>, path: Option<&str>) -> PathBuf {
    crate::store::resolve_export_dir(project, path)
}

/// A `path` that names a file (`.../hit.ogg`, `hit.wav`): its folder, the
/// file's base name and the format its extension says. None for a folder.
pub fn split_file_path(path: &str) -> Option<(Option<String>, String, &'static str)> {
    let p = Path::new(path);
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    let fmt = match ext.as_str() {
        "ogg" => "ogg",
        "wav" => "wav",
        _ => return None,
    };
    let stem = p.file_stem()?.to_str()?.to_string();
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .filter(|d| !d.is_empty());
    Some((dir, stem, fmt))
}
