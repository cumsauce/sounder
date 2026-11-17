//! Reference compare: how a rendered sound differs from a recording (or an
//! earlier render), in numbers and in words.
//!
//! The reference is first made comparable: resampled to the sound's rate,
//! shifted so its first onset meets the sound's, and scaled to the sound's
//! integrated loudness. Then [`compare`] reports the multi-resolution STFT
//! distance, the biggest cells of the band x time table that differ ("0-20%
//! of the sound: 2-4 kHz is 9 dB quieter than the reference") and the
//! differences in length, attack, decay and centroid. The difference
//! spectrogram is drawn by [`crate::image::listen_png_diff`].
//!
//! Modeled on modeler's `band_deviations` / `top_deviations`: a few ranked
//! sentences that say what to change, not a wall of numbers.

use std::path::{Path, PathBuf};

use sfx_graph::{Audio, db_to_gain, gain_to_db, loudness, wav};

use crate::descriptors::{BANDS, Descriptors, SEGMENTS, describe};
use crate::stft;

/// Smallest band-table difference worth a sentence, dB.
pub const MIN_DEVIATION_DB: f32 = 3.0;
/// How many deviations are listed.
pub const TOP_DEVIATIONS: usize = 5;

/// One cell of the band x time table where the sound and the reference
/// differ.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Deviation {
    /// Octave band index into [`BANDS`].
    pub band: usize,
    /// Time segment 0..SEGMENTS (fifths of the sound).
    pub segment: usize,
    /// Sound minus reference, dB (negative = quieter than the reference).
    pub diff_db: f32,
    /// The sentence.
    pub words: String,
}

/// The result of [`compare`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct Comparison {
    /// Multi-resolution STFT distance after alignment and loudness
    /// matching, dB (0 = identical).
    pub distance_db: f32,
    /// Reference onset minus the sound's onset, seconds (positive: the
    /// reference starts later and was moved earlier by this much).
    pub shift_s: f32,
    /// Gain applied to the reference to match the sound's loudness, dB.
    pub gain_db: f32,
    /// The biggest band-table differences, largest first.
    pub deviations: Vec<Deviation>,
    /// Length / attack / decay / centroid / format differences, in words.
    pub notes: Vec<String>,
    /// The reference as compared: resampled, aligned, scaled, cut or padded
    /// to the length of the longer of the two.
    #[serde(skip)]
    pub aligned: Audio,
    /// The sound padded to the same length.
    #[serde(skip)]
    pub padded: Audio,
}

/// Reads a WAV: relative paths resolve against `base` (the project).
pub fn load(file: &str, base: Option<&Path>) -> Result<Audio, String> {
    let p = Path::new(file);
    let path: PathBuf = match (base, p.is_relative()) {
        (Some(root), true) => root.join(p),
        _ => p.to_path_buf(),
    };
    if !path.is_file() {
        return Err(format!(
            "reference `{file}`: no such file ({}); give a WAV path, relative to the project or absolute",
            path.display()
        ));
    }
    let audio = wav::read(&path)?;
    if audio.len() < 64 {
        return Err(format!(
            "reference `{file}`: only {} samples; give a recording at least a few milliseconds long",
            audio.len()
        ));
    }
    Ok(audio)
}

/// `audio` at `sr` (windowed-sinc, Blackman window of 48 taps each side; the
/// cutoff drops to the lower Nyquist when downsampling).
pub fn resample(audio: &Audio, sr: u32) -> Audio {
    if audio.sample_rate == sr || audio.is_empty() {
        return Audio {
            sample_rate: sr,
            channels: audio.channels.clone(),
        };
    }
    let ratio = f64::from(audio.sample_rate) / f64::from(sr);
    let cutoff = (1.0 / ratio).min(1.0);
    const HALF: isize = 48;
    let n_out = (audio.len() as f64 / ratio).round() as usize;
    let channels = audio
        .channels
        .iter()
        .map(|x| {
            (0..n_out)
                .map(|i| {
                    let pos = i as f64 * ratio;
                    let c = pos.floor() as isize;
                    let mut acc = 0.0f64;
                    for j in (c - HALF + 1)..=(c + HALF) {
                        if j < 0 || j as usize >= x.len() {
                            continue;
                        }
                        let d = pos - j as f64;
                        let u = d * cutoff;
                        let sinc = if u.abs() < 1e-9 {
                            1.0
                        } else {
                            (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u)
                        };
                        let ph = std::f64::consts::PI * d / (HALF as f64 + 1.0);
                        let w = 0.42 + 0.5 * ph.cos() + 0.08 * (2.0 * ph).cos();
                        acc += f64::from(x[j as usize]) * sinc * cutoff * w;
                    }
                    acc as f32
                })
                .collect()
        })
        .collect();
    Audio {
        sample_rate: sr,
        channels,
    }
}

/// Seconds of the first sample within 30 dB of the sound's peak (0 for
/// silence).
pub fn onset_s(audio: &Audio) -> f32 {
    let peak = audio.peak();
    if peak <= 0.0 {
        return 0.0;
    }
    let th = peak * db_to_gain(-30.0);
    let first = audio
        .channels
        .iter()
        .filter_map(|c| c.iter().position(|v| v.abs() >= th))
        .min()
        .unwrap_or(0);
    first as f32 / audio.sample_rate as f32
}

/// Integrated loudness, LUFS; a sound too short or quiet for the gate falls
/// back to its RMS (so the gain stays finite).
fn level_lufs(audio: &Audio) -> f32 {
    let l = loudness::integrated(&audio.channels, audio.sample_rate);
    if l > -69.9 {
        return l;
    }
    let (sum, n) = audio
        .channels
        .iter()
        .flatten()
        .fold((0.0f64, 0usize), |(s, n), v| {
            (s + f64::from(*v) * f64::from(*v), n + 1)
        });
    gain_to_db(((sum / n.max(1) as f64).sqrt()) as f32) - 0.7
}

/// Moves `x` by `shift` samples (positive: later, padding the front).
fn shifted(x: &[f32], shift: isize) -> Vec<f32> {
    if shift >= 0 {
        let mut v = vec![0.0; shift as usize];
        v.extend_from_slice(x);
        v
    } else {
        let cut = ((-shift) as usize).min(x.len());
        x[cut..].to_vec()
    }
}

/// `x` cut or zero-padded to `n`.
fn fitted(mut x: Vec<f32>, n: usize) -> Vec<f32> {
    x.resize(n, 0.0);
    x
}

/// Hz range text of octave band `k` ("1.4-2.8 kHz").
pub fn band_range(k: usize, sr: u32) -> String {
    let c = BANDS[k];
    let lo = c / std::f32::consts::SQRT_2;
    let hi = if k == BANDS.len() - 1 {
        sr as f32 / 2.0
    } else {
        c * std::f32::consts::SQRT_2
    };
    let f = |v: f32| {
        if v >= 1000.0 {
            let k = v / 1000.0;
            if (k - k.round()).abs() < 0.05 {
                format!("{k:.0}")
            } else {
                format!("{k:.1}")
            }
        } else {
            format!("{v:.0}")
        }
    };
    if hi >= 1000.0 && lo < 1000.0 {
        format!("{} Hz-{} kHz", f(lo), f(hi))
    } else if hi >= 1000.0 {
        format!("{}-{} kHz", f(lo), f(hi))
    } else {
        format!("{}-{} Hz", f(lo), f(hi))
    }
}

/// The `segment`-th fifth as "0-20% of the sound".
pub fn segment_words(segment: usize) -> String {
    let w = 100 / SEGMENTS;
    format!("{}-{}% of the sound", segment * w, (segment + 1) * w)
}

/// The biggest differences between two band tables (sound minus reference),
/// largest first; cells where both are inaudible (under -70 dB) are left
/// out, as are differences under [`MIN_DEVIATION_DB`].
pub fn band_deviations(snd: &Descriptors, rf: &Descriptors, top: usize) -> Vec<Deviation> {
    let mut all = Vec::new();
    for band in 0..BANDS.len() {
        for segment in 0..SEGMENTS {
            let (a, b) = (snd.bands[band][segment], rf.bands[band][segment]);
            if a < -70.0 && b < -70.0 {
                continue;
            }
            let diff_db = a.max(-90.0) - b.max(-90.0);
            if diff_db.abs() < MIN_DEVIATION_DB {
                continue;
            }
            let words = format!(
                "{}: {} is {:.0} dB {} than the reference",
                segment_words(segment),
                band_range(band, snd.sample_rate),
                diff_db.abs(),
                if diff_db < 0.0 { "quieter" } else { "louder" }
            );
            all.push(Deviation {
                band,
                segment,
                diff_db,
                words,
            });
        }
    }
    all.sort_by(|x, y| y.diff_db.abs().total_cmp(&x.diff_db.abs()));
    all.truncate(top);
    all
}

fn ms_or_s(v: f32) -> String {
    if v < 0.1 {
        format!("{:.0} ms", v * 1000.0)
    } else {
        format!("{v:.2} s")
    }
}

fn khz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.1} kHz", v / 1000.0)
    } else {
        format!("{v:.0} Hz")
    }
}

/// Seconds of audible content (between the first and last sample over -60
/// dBFS).
fn audible_s(d: &Descriptors) -> f32 {
    (d.seconds - d.silence_start_s - d.silence_end_s).max(0.0)
}

/// Length / attack / decay / centroid differences in words ("sound vs
/// reference").
fn difference_notes(s: &Descriptors, r: &Descriptors) -> Vec<String> {
    let mut out = Vec::new();
    let (ls, lr) = (audible_s(s), audible_s(r));
    let dl = lr - ls;
    if dl.abs() >= 0.03 && dl.abs() >= 0.05 * ls.max(lr) {
        out.push(format!(
            "the reference is {} {} ({:.2} s vs {:.2} s audible)",
            ms_or_s(dl.abs()),
            if dl > 0.0 { "longer" } else { "shorter" },
            lr,
            ls
        ));
    }
    let (a, b) = (s.attack_s, r.attack_s);
    if (a - b).abs() >= 0.002 && (a - b).abs() >= 0.4 * a.max(b) {
        out.push(format!(
            "attack {} vs {} (sound vs reference): the {} {}",
            ms_or_s(a),
            ms_or_s(b),
            if a > b { "sound" } else { "reference" },
            "is slower"
        ));
    }
    for (name, x, y) in [
        ("decay to -20 dB", s.decay20_s, r.decay20_s),
        ("decay to -60 dB", s.decay60_s, r.decay60_s),
    ] {
        match (x, y) {
            (Some(x), Some(y)) if (x - y).abs() >= 0.03 && (x - y).abs() >= 0.25 * x.max(y) => {
                out.push(format!(
                    "{name} {} vs {} (sound vs reference): the reference rings {}",
                    ms_or_s(x),
                    ms_or_s(y),
                    if y > x { "longer" } else { "shorter" }
                ));
            }
            (Some(x), None) => out.push(format!(
                "{name}: the sound falls in {} but the reference never does within its length",
                ms_or_s(x)
            )),
            (None, Some(y)) => out.push(format!(
                "{name}: the reference falls in {} but the sound never does within its length",
                ms_or_s(y)
            )),
            _ => {}
        }
    }
    let cs = [s.centroid_hz.start, s.centroid_hz.middle, s.centroid_hz.end];
    let cr = [r.centroid_hz.start, r.centroid_hz.middle, r.centroid_hz.end];
    let off = cs
        .iter()
        .zip(&cr)
        .any(|(x, y)| *x > 0.0 && *y > 0.0 && (x / y > 1.25 || y / x > 1.25));
    if off {
        out.push(format!(
            "centroid (start/middle/end) {}/{}/{} vs {}/{}/{} (sound vs reference): the sound is {}",
            khz(cs[0]),
            khz(cs[1]),
            khz(cs[2]),
            khz(cr[0]),
            khz(cr[1]),
            khz(cr[2]),
            if cs.iter().sum::<f32>() < cr.iter().sum::<f32>() {
                "darker"
            } else {
                "brighter"
            }
        ));
    }
    out
}

/// Compares `audio` with `reference`.
pub fn compare(audio: &Audio, reference: &Audio) -> Comparison {
    let sr = audio.sample_rate;
    let mut notes = Vec::new();
    let mut rf = resample(reference, sr);
    if reference.sample_rate != sr {
        notes.push(format!(
            "the reference is {} Hz; resampled to the sound's {sr} Hz",
            reference.sample_rate
        ));
        if reference.sample_rate < sr {
            notes.push(format!(
                "the reference has nothing above {:.1} kHz (its Nyquist): the sound's content up there counts as a difference",
                reference.sample_rate as f32 / 2000.0
            ));
        }
    }
    if reference.channels.len() != audio.channels.len() {
        notes.push(format!(
            "the sound is {} and the reference {}; compared as mono mixes",
            if audio.is_stereo() { "stereo" } else { "mono" },
            if reference.is_stereo() {
                "stereo"
            } else {
                "mono"
            },
        ));
    }
    // Align by onset: the reference moves, the sound stays.
    let shift_s = onset_s(&rf) - onset_s(audio);
    let shift = (shift_s * sr as f32).round() as isize;
    if shift != 0 {
        for c in &mut rf.channels {
            *c = shifted(c, -shift);
        }
    }
    // Match loudness: the reference to the sound's integrated loudness.
    let gain_db = (level_lufs(audio) - level_lufs(&rf)).clamp(-60.0, 60.0);
    let g = db_to_gain(gain_db);
    for c in &mut rf.channels {
        c.iter_mut().for_each(|v| *v *= g);
    }
    let n = audio.len().max(rf.len());
    let aligned = Audio {
        sample_rate: sr,
        channels: rf.channels.iter().map(|c| fitted(c.clone(), n)).collect(),
    };
    let padded = Audio {
        sample_rate: sr,
        channels: audio
            .channels
            .iter()
            .map(|c| fitted(c.clone(), n))
            .collect(),
    };
    let distance_db = stft::distance(&audio.mix_mono(), &aligned.mix_mono(), sr);
    // Band table over the sound's own length (the reference cut or padded
    // to it); what lies beyond is the length note.
    let snd_d = describe(audio);
    let mut cut = aligned.clone();
    for c in &mut cut.channels {
        c.resize(audio.len(), 0.0);
    }
    let ref_cut = describe(&cut);
    let deviations = band_deviations(&snd_d, &ref_cut, TOP_DEVIATIONS);
    let ref_d = describe(&aligned);
    notes.extend(difference_notes(&snd_d, &ref_d));
    Comparison {
        distance_db,
        shift_s,
        gain_db,
        deviations,
        notes,
        aligned,
        padded,
    }
}

impl Comparison {
    /// The text `listen` prints above the descriptor table.
    pub fn text(&self) -> String {
        let mut o = format!(
            "reference distance {:.2} dB (multi-resolution STFT, windows 256 / 1024 / 4096; 0 = identical)\n",
            self.distance_db
        );
        if self.shift_s.abs() >= 0.0005 {
            o += &format!(
                "aligned: the reference starts {:.3} s {} than the sound; moved {}\n",
                self.shift_s.abs(),
                if self.shift_s > 0.0 {
                    "later"
                } else {
                    "earlier"
                },
                if self.shift_s > 0.0 {
                    "earlier"
                } else {
                    "later"
                }
            );
        } else {
            o += "aligned: both start together\n";
        }
        o += &format!(
            "loudness: reference scaled by {:+.1} dB to match the sound's integrated loudness\n",
            self.gain_db
        );
        if self.deviations.is_empty() {
            o += "band table: no band x time cell differs by 3 dB or more\n";
        } else {
            o += &format!(
                "biggest band x time differences (sound minus reference, {} shown):\n",
                self.deviations.len()
            );
            for d in &self.deviations {
                o += &format!("  - {}\n", d.words);
            }
        }
        for n in &self.notes {
            o += &format!("- {n}\n");
        }
        o
    }

    /// What to change, from the deviations (for a failing check).
    pub fn advice(&self) -> String {
        match self.deviations.first() {
            Some(d) => format!(
                "biggest: {}; change what shapes that band (filter, noise colour, level of the part that sounds there)",
                d.words
            ),
            None => {
                "no single band differs much: look at timing and the notes from listen reference"
                    .into()
            }
        }
    }
}
