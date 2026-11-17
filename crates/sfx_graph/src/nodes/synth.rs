//! Synthesis: two-operator FM and an additive wavetable.

use std::f64::consts::TAU;

use serde::Deserialize;

use super::{mono, one};
use crate::dsp::poly_blep;
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Most harmonics a `wavetable` takes.
pub const MAX_HARMONICS: usize = 256;

/// Errors unless `x` is finite.
fn finite(field: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give a finite number"))
    }
}

/// The modulator's wave.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModShape {
    #[default]
    Sine,
    Saw,
    Square,
    Tri,
}

impl ModShape {
    /// The wave at cycle position `x` (any real; its fractional part is
    /// used), `dt` cycles per sample for the band-limiting correction.
    #[inline]
    fn at(self, x: f64, dt: f32) -> f32 {
        let t = x.rem_euclid(1.0);
        match self {
            ModShape::Sine => (TAU * t).sin() as f32,
            ModShape::Tri => {
                let t = t as f32;
                if t < 0.25 {
                    4.0 * t
                } else if t < 0.75 {
                    2.0 - 4.0 * t
                } else {
                    4.0 * t - 4.0
                }
            }
            ModShape::Saw => {
                let t = t as f32;
                2.0 * t - 1.0 - poly_blep(t, dt)
            }
            ModShape::Square => {
                let t = t as f32;
                let v = if t < 0.5 { 1.0 } else { -1.0 };
                v + poly_blep(t, dt) - poly_blep((t + 0.5).rem_euclid(1.0), dt)
            }
        }
    }
}

/// Two-operator FM: one modulator phase-modulates one sine carrier.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fm {
    /// Base frequency, Hz (a signal: a node here sweeps the pitch). The
    /// carrier runs at `freq * ratio`, the modulator at `freq * mod_ratio`.
    pub freq: In,
    /// Carrier frequency as a multiple of `freq` (default 1; any finite
    /// number, negative runs the carrier backwards).
    #[serde(default)]
    pub ratio: Option<f32>,
    /// Modulation index: the peak phase deviation of the carrier in radians
    /// (a signal; default 0 = a pure sine). Sideband k has amplitude
    /// J_k(index).
    #[serde(default)]
    pub index: Option<In>,
    /// Modulator self-feedback, radians of phase modulation per unit of the
    /// modulator's last two outputs averaged (default 0; 0..2 is the useful
    /// range, higher turns the modulator noisy).
    #[serde(default)]
    pub feedback: Option<f32>,
    /// Modulator frequency as a multiple of `freq` (default 1).
    #[serde(default)]
    pub mod_ratio: Option<f32>,
    /// Modulator wave: sine (default), saw, square, tri.
    #[serde(default)]
    pub mod_shape: ModShape,
    /// Linear amplitude (a signal; default 1).
    #[serde(default)]
    pub amp: Option<In>,
}

impl Describe for Fm {
    const BLURB: &'static str = "two-operator FM (phase modulation) with modulator feedback; freq, index and amp are signals";
    const WORDS: &'static [&'static str] = &["mod_shape"];
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 220.0, "ratio": 1.0, "mod_ratio": 2.0, "index": 3.0,
            "feedback": 0.3, "amp": 0.5})
    }
}

impl Eval for Fm {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let freq = cx.mono("freq", &self.freq)?;
        let index = cx.mono_or("index", &self.index, 0.0)?;
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let ratio = finite("ratio", self.ratio.unwrap_or(1.0))?;
        let mod_ratio = finite("mod_ratio", self.mod_ratio.unwrap_or(1.0))?;
        let feedback = self.feedback.unwrap_or(0.0);
        if !feedback.is_finite() || feedback < 0.0 {
            return Err(format!("feedback: {feedback}: give a number >= 0"));
        }
        let fb = f64::from(feedback);
        let shape = self.mod_shape;
        let sr = f64::from(cx.sr);
        let (mut pc, mut pm) = (0.0f64, 0.0f64);
        let (mut y1, mut y2) = (0.0f64, 0.0f64);
        Ok(one(mono(cx.n, |i| {
            let f = f64::from(freq.at(i));
            let dc = (f * f64::from(ratio) / sr).clamp(-0.5, 0.5);
            let dm = (f * f64::from(mod_ratio) / sr).clamp(-0.5, 0.5);
            // The modulator, its phase pushed by its own last two outputs.
            let arg = pm + fb * 0.5 * (y1 + y2) / TAU;
            let m = f64::from(shape.at(arg, dm.abs() as f32));
            y2 = y1;
            y1 = m;
            let out = (TAU * pc + f64::from(index.at(i)) * m).sin();
            pc = (pc + dc).rem_euclid(1.0);
            pm = (pm + dm).rem_euclid(1.0);
            out as f32 * amp.at(i)
        })))
    }
}

/// An additive single-cycle oscillator from harmonic amplitudes.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Wavetable {
    /// Base frequency, Hz (a signal).
    pub freq: In,
    /// Amplitudes of harmonics 1, 2, 3, ... (the first entry is the
    /// fundamental); 1 to 256 entries, an error past that.
    pub harmonics: Vec<f32>,
    /// A second set of amplitudes `morph` blends towards (up to 256
    /// entries; the shorter list is padded with zeros).
    #[serde(default)]
    pub harmonics_to: Option<Vec<f32>>,
    /// Blend from `harmonics` (0) to `harmonics_to` (1), clamped to 0..1
    /// (a signal; default 0). Needs `harmonics_to`.
    #[serde(default)]
    pub morph: Option<In>,
    /// Phase of each harmonic in cycles (default all 0: sines); at most as
    /// many entries as the longer harmonic list, missing ones are 0.
    #[serde(default)]
    pub phases: Option<Vec<f32>>,
    /// Linear amplitude (a signal; default 1). The harmonic amplitudes are
    /// not normalized: `[1, 1, 1]` peaks above 1.
    #[serde(default)]
    pub amp: Option<In>,
}

impl Describe for Wavetable {
    const BLURB: &'static str =
        "additive oscillator from harmonic amplitudes (up to 256), morphing to a second set";
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 110.0, "harmonics": [1.0, 0.5, 0.33, 0.25],
            "harmonics_to": [1.0, 0.0, 0.33, 0.0, 0.2], "morph": 0.5, "amp": 0.5})
    }
}

/// Sine and cosine coefficients of one harmonic set, padded to `len`.
fn coefs(amps: &[f32], phases: &[f32], len: usize) -> Vec<(f64, f64)> {
    (0..len)
        .map(|h| {
            let a = f64::from(amps.get(h).copied().unwrap_or(0.0));
            let p = TAU * f64::from(phases.get(h).copied().unwrap_or(0.0));
            // a sin(k t + p) = (a cos p) sin(k t) + (a sin p) cos(k t)
            (a * p.cos(), a * p.sin())
        })
        .collect()
}

/// The sum of a harmonic set at the phasor `(c1, s1)` of harmonic 1:
/// harmonics up to `full` at weight 1, then up to `upto` fading linearly
/// with their frequency `k * af` from `lo` to `hi`.
#[inline]
#[allow(clippy::too_many_arguments)]
fn additive(
    set: &[(f64, f64)],
    c1: f64,
    s1: f64,
    full: usize,
    upto: usize,
    af: f64,
    lo: f64,
    hi: f64,
) -> f64 {
    let (mut c, mut s) = (c1, s1);
    let mut sum = 0.0;
    for (h, &(ks, kc)) in set.iter().enumerate().take(upto) {
        let w = if h < full {
            1.0
        } else {
            (1.0 - ((h + 1) as f64 * af - lo) / (hi - lo)).clamp(0.0, 1.0)
        };
        sum += w * (ks * s + kc * c);
        // z_{h+2} = z_{h+1} * z_1
        let nc = c * c1 - s * s1;
        s = s * c1 + c * s1;
        c = nc;
    }
    sum
}

impl Eval for Wavetable {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let check = |field: &str, v: &[f32]| -> Result<(), String> {
            if v.len() > MAX_HARMONICS {
                return Err(format!(
                    "{field}: {} entries: give at most {MAX_HARMONICS}",
                    v.len()
                ));
            }
            if let Some(x) = v.iter().find(|x| !x.is_finite()) {
                return Err(format!("{field}: {x}: give finite numbers"));
            }
            Ok(())
        };
        check("harmonics", &self.harmonics)?;
        if self.harmonics.is_empty() {
            return Err("harmonics: empty: give at least one amplitude, e.g. [1]".into());
        }
        if let Some(to) = &self.harmonics_to {
            check("harmonics_to", to)?;
        } else if self.morph.is_some() {
            return Err("morph: needs `harmonics_to`: give the amplitudes to morph towards".into());
        }
        let phases = self.phases.clone().unwrap_or_default();
        check("phases", &phases)?;
        let len = self
            .harmonics
            .len()
            .max(self.harmonics_to.as_ref().map_or(0, Vec::len));
        if phases.len() > len {
            return Err(format!(
                "phases: {} entries but the harmonic lists have {len}: give at most that many",
                phases.len()
            ));
        }
        let freq = cx.mono("freq", &self.freq)?;
        let morph = cx.mono_or("morph", &self.morph, 0.0)?;
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let a = coefs(&self.harmonics, &phases, len);
        let b = self.harmonics_to.as_ref().map(|t| coefs(t, &phases, len));
        let sr = f64::from(cx.sr);
        // Harmonics are exact weight 1 up to 0.45 sr and fade linearly to 0
        // at Nyquist, per sample, so a sweep through the top never clicks.
        let (lo, hi) = (0.45 * sr, 0.5 * sr);
        let mut phase = 0.0f64;
        Ok(one(mono(cx.n, |i| {
            let f = f64::from(freq.at(i));
            let af = f.abs().max(1e-3);
            let full = ((lo / af).floor() as usize).min(len);
            let upto = ((hi / af).ceil() as usize).min(len);
            let (s1, c1) = (TAU * phase).sin_cos();
            let m = f64::from(morph.at(i)).clamp(0.0, 1.0);
            let mut y = additive(&a, c1, s1, full, upto, af, lo, hi);
            if let Some(b) = &b
                && m > 0.0
            {
                let yb = additive(b, c1, s1, full, upto, af, lo, hi);
                y = (1.0 - m) * y + m * yb;
            }
            phase = (phase + (f / sr).clamp(-0.5, 0.5)).rem_euclid(1.0);
            y as f32 * amp.at(i)
        })))
    }
}
