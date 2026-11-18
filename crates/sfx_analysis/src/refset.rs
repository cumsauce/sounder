//! Reference sets: several recordings of one kind of sound
//! (`<project>/refs/<set>/*.wav`) to aim at. One recording is a poor target:
//! in Phase 0 a real door slam sat 10.4 dB (STFT distance) from two other
//! real slams, further than a synthetic one (`docs/history/phase0-results.md`),
//! so the distance to the nearest recording says little below ~10 dB. A set
//! gives ranges instead: each feature (energy by role, crest, ring-out,
//! centroid, voicing, ringing partials) is measured on every recording, and
//! the sound is checked against the span they cover, with a tolerance.

use std::path::{Path, PathBuf};

use sfx_graph::Audio;

use crate::checks::ringing_partials;
use crate::descriptors::{Descriptors, describe};
use crate::reference::{Comparison, compare};

/// One recording of a set.
pub struct SetMember {
    pub name: String,
    pub audio: Audio,
}

/// Where a set lives: `<project>/refs/<set>/` for a bare name, the path
/// itself (relative to the project) for one with a `/`, or absolute.
pub fn set_dir(set: &str, base: Option<&Path>) -> PathBuf {
    let p = Path::new(set);
    let rel = if p.is_absolute() || set.contains('/') {
        p.to_path_buf()
    } else {
        Path::new("refs").join(set)
    };
    match base {
        Some(b) if rel.is_relative() => b.join(rel),
        _ => rel,
    }
}

/// The WAVs of a set (sorted by name); at least two.
pub fn load_set(set: &str, base: Option<&Path>) -> Result<Vec<SetMember>, String> {
    let dir = set_dir(set, base);
    let rd = std::fs::read_dir(&dir).map_err(|_| {
        let refs = base.map_or(PathBuf::from("refs"), |b| b.join("refs"));
        let sets: Vec<String> = std::fs::read_dir(&refs)
            .map(|r| {
                r.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "reference set `{set}`: no folder {} (a set is a folder of WAVs under <project>/refs/; sets here: {})",
            dir.display(),
            if sets.is_empty() { "none".to_string() } else { sets.join(", ") }
        )
    })?;
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_str().is_some_and(sfx_graph::node::is_file))
        .collect();
    files.sort();
    if files.len() < 2 {
        return Err(format!(
            "reference set `{set}`: {} holds {} WAV: a set needs at least two (for one, use reference)",
            dir.display(),
            files.len()
        ));
    }
    files
        .iter()
        .map(|f| {
            let audio = sfx_graph::wav::read(f)?;
            Ok(SetMember {
                name: f
                    .file_name()
                    .map_or(String::new(), |n| n.to_string_lossy().into_owned()),
                audio,
            })
        })
        .collect()
}

/// One feature of the sound against the set's span.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetFeature {
    pub name: &'static str,
    pub unit: &'static str,
    pub sound: f32,
    pub min: f32,
    pub max: f32,
    /// How far outside [min, max] still passes.
    pub tolerance: f32,
    pub pass: bool,
    /// What to change when it fails.
    pub fix: &'static str,
}

impl SetFeature {
    pub fn words(&self) -> String {
        let v = |x: f32| fmt(x, self.unit);
        let span = format!("{}..{}", v(self.min), v(self.max));
        if self.pass {
            format!("{} {} (the set spans {span})", self.name, v(self.sound))
        } else {
            format!(
                "{} {} is {} every recording (they span {span}, tolerance {}): {}",
                self.name,
                v(self.sound),
                if self.sound > self.max {
                    "above"
                } else {
                    "below"
                },
                v(self.tolerance),
                self.fix
            )
        }
    }
}

fn fmt(x: f32, unit: &str) -> String {
    match unit {
        "%" => format!("{x:.1} %"),
        "dB" => format!("{x:.1} dB"),
        "ms" => format!("{x:.0} ms"),
        "Hz" => format!("{x:.0} Hz"),
        _ => format!("{x:.2}"),
    }
}

/// The result of [`compare_set`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetComparison {
    pub set: String,
    /// (file, STFT distance dB), in the set's order.
    pub distances: Vec<(String, f32)>,
    /// Index of the nearest recording in `distances`.
    pub nearest: usize,
    /// The mean distance between the set's own recordings: the spread a
    /// sound "in the family" sits within.
    pub spread_db: f32,
    pub features: Vec<SetFeature>,
    /// The comparison with the nearest recording (for the picture).
    #[serde(skip)]
    pub nearest_cmp: Comparison,
}

/// (name, unit, value, absolute tolerance floor, fix) for one descriptor set.
fn features(d: &Descriptors) -> Vec<(&'static str, &'static str, f32, f32, &'static str)> {
    let [low, mid, hi, air] = d.band_shares;
    vec![
        (
            "weight",
            "%",
            low,
            3.0,
            "change the weight layer (energy under 200 Hz)",
        ),
        (
            "body",
            "%",
            mid,
            3.0,
            "change the body layer (200 Hz-2 kHz)",
        ),
        (
            "crack",
            "%",
            hi + air,
            1.0,
            "change the crack (above 2 kHz)",
        ),
        // The crest of a noise hit moves several dB with the seed alone
        // (Phase 0's slam_b2: peaks over 4.6 dB across four seeds), more
        // than a few recordings show.
        (
            "crest",
            "dB",
            d.crest_db,
            3.0,
            "finish punch raises it, glue lowers it",
        ),
        (
            "tail",
            "ms",
            d.decay20_s.map_or(f32::NAN, |t| t * 1000.0),
            20.0,
            "lengthen or shorten the body and weight decays (or finish tail_db)",
        ),
        (
            "centroid",
            "Hz",
            d.centroid_all_hz,
            0.0,
            "shift energy between the weight, body and crack layers",
        ),
        (
            "voiced",
            "",
            d.voiced,
            0.05,
            "a pitch where the recordings have none: noise for weight, modal struck by noise",
        ),
        (
            "ringing",
            "",
            ringing_partials(d).len() as f32,
            0.0,
            "partials ring longer than in the recordings: raise the modal loss",
        ),
    ]
}

/// Compares `sound` with every member of a set.
pub fn compare_set(sound: &Audio, set: &str, members: &[SetMember]) -> SetComparison {
    let mut distances = Vec::new();
    let mut best: Option<(usize, Comparison)> = None;
    for (i, m) in members.iter().enumerate() {
        let c = compare(sound, &m.audio);
        distances.push((m.name.clone(), c.distance_db));
        if best
            .as_ref()
            .is_none_or(|(_, b)| c.distance_db < b.distance_db)
        {
            best = Some((i, c));
        }
    }
    let (nearest, nearest_cmp) = best.unwrap_or_else(|| (0, compare(sound, sound)));
    let mut pair = Vec::new();
    for i in 0..members.len() {
        for j in (i + 1)..members.len() {
            pair.push(compare(&members[i].audio, &members[j].audio).distance_db);
        }
    }
    let spread_db = if pair.is_empty() {
        0.0
    } else {
        pair.iter().sum::<f32>() / pair.len() as f32
    };
    let ref_feats: Vec<_> = members
        .iter()
        .map(|m| features(&describe(&m.audio)))
        .collect();
    let snd = features(&describe(sound));
    let mut out = Vec::new();
    for (k, (name, unit, value, floor, fix)) in snd.into_iter().enumerate() {
        let vals: Vec<f32> = ref_feats
            .iter()
            .map(|f| f[k].2)
            .filter(|v| v.is_finite())
            .collect();
        if vals.is_empty() || !value.is_finite() {
            continue;
        }
        let min = vals.iter().copied().fold(f32::INFINITY, f32::min);
        let max = vals.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        // A quarter of the span, at least the floor (the centroid's floor is
        // 10 % of the set's middle).
        let floor = if name == "centroid" {
            0.1 * 0.5 * (min + max)
        } else {
            floor
        };
        let tolerance = (0.25 * (max - min)).max(floor);
        let pass = value >= min - tolerance && value <= max + tolerance;
        out.push(SetFeature {
            name,
            unit,
            sound: value,
            min,
            max,
            tolerance,
            pass,
            fix,
        });
    }
    SetComparison {
        set: set.to_string(),
        distances,
        nearest,
        spread_db,
        features: out,
        nearest_cmp,
    }
}

impl SetComparison {
    /// The features outside the set's span.
    pub fn failing(&self) -> Vec<&SetFeature> {
        self.features.iter().filter(|f| !f.pass).collect()
    }

    pub fn text(&self) -> String {
        let mut t = format!(
            "reference set `{}` ({} recordings; they sit {:.2} dB from each other on average):\n",
            self.set,
            self.distances.len(),
            self.spread_db
        );
        for (i, (n, d)) in self.distances.iter().enumerate() {
            t += &format!(
                "- {n}: {d:.2} dB{}\n",
                if i == self.nearest { " (nearest)" } else { "" }
            );
        }
        let (_, dn) = &self.distances[self.nearest];
        t += &format!(
            "nearest {dn:.2} dB against the set's own spread {:.2} dB: {}\n",
            self.spread_db,
            if *dn <= self.spread_db {
                "inside the family (the distance cannot rank closer than this)"
            } else {
                "further than the recordings are from each other"
            }
        );
        t += "features against the set's span:\n";
        for f in &self.features {
            t += &format!("{} {}\n", if f.pass { "  in " } else { "  OUT" }, f.words());
        }
        let n = self.failing().len();
        t += &if n == 0 {
            "every feature is inside the set's span\n".to_string()
        } else {
            format!("{n} feature(s) outside the set's span\n")
        };
        t
    }
}
