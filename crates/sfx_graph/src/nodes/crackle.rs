//! `crumple` (R2-D): paper, foil, plastic or dry leaves being crumpled, as a
//! population of short band-passed noise bursts. Unlike `cloth` the number
//! of bursts follows `density` all the way up (no saturation), and the
//! arrivals are a jittered grid rather than a Poisson process, so a count of
//! fine onsets tracks the density instead of merging neighbours. Tests in
//! `crates/sfx_analysis/tests/crackle.rs`.

use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::{FilterMode, Svf, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Highest `density`, bursts per second.
const MAX_DENSITY: f32 = 400.0;
/// Most bursts ringing at once; a burst that would be the next is skipped.
/// (The longest a burst rings is 24 ms * ln 1000 = 166 ms, at 40 per s or
/// fewer: 7 at once; above 40 the decay shortens as 1 / density.)
const MAX_ACTIVE: usize = 128;
/// Fraction of bursts that are snaps (louder, brighter, shorter). A snap
/// replaces a burst, so it does not change the count.
const SNAP_SHARE: f32 = 0.06;
/// Spread of an arrival around the centre of its slot, in slots: gaps run
/// from 1 - JITTER to 1 + JITTER slots.
const JITTER: f32 = 0.4;
/// Pareto exponent of the burst amplitudes (P(A > a) = a^-alpha, a >= 1) at
/// size 0 and size 1.
const ALPHA: [f32; 2] = [3.0, 2.0];
/// The largest burst amplitude relative to the smallest (8 dB): a wider
/// range makes the sound too peaky for `output lufs` and hides bursts under
/// the tail of a big one in the fine onset count.
const MAX_AMP: f32 = 2.5;
/// A snap's level: this times the square root of its burst amplitude.
const SNAP_LEVEL: f32 = 2.0;
/// Density, bursts per second, up to which a burst keeps its full decay.
const DENSITY_REF: f32 = 40.0;
/// Attack of a burst, seconds.
const ATTACK: f32 = 0.0002;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Crumpled paper, foil, plastic or dry leaves.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Crumple {
    /// Bursts per second (default 40, a number or a signal). 0 is silence;
    /// up to 400. Arrivals are a jittered grid, so over 1 s there are
    /// `density` bursts give or take one, not a Poisson scatter. Above 40 the
    /// bursts shorten as 40 / density and keep their level, so they stay
    /// distinct and the energy per second stays level (not growing); below
    /// 40 the level per burst rises as 1/sqrt(density) (at most 2x), so a
    /// sparse crumple is not much quieter. A number outside 0..400 is an
    /// error; a signal is clamped.
    #[serde(default)]
    pub density: Option<In>,
    /// How much paper is being crumpled, 0..1 (default 0.5; a number):
    /// larger is lower (centres fall 0.75 octave from size 0.5 to 1, rise
    /// 0.75 octave to 0), longer (decays 0.5x to 2x) and has a heavier tail
    /// of big bursts. A number outside 0..1 is
    /// an error.
    #[serde(default)]
    pub size: Option<f32>,
    /// How stiff the material is, 0..1 (default 0.5; a number): 0 is soft
    /// paper or leaf (centres 1.5-4.5 kHz, decay time constant 12 ms), 1 is
    /// foil or plastic (3-9 kHz, 1 ms), at size 0.5 and density 40 or less. A number outside 0..1 is an error.
    #[serde(default)]
    pub hardness: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Crumple {
    const BLURB: &'static str = "crumpled paper/foil/plastic/leaf: a power-law population of short band-passed bursts, density per second all the way to 400, plus sparse louder snaps";
    fn example() -> serde_json::Value {
        serde_json::json!({"density": 40, "size": 0.5, "hardness": 0.5, "seed": 1})
    }
}

/// One ringing burst.
struct Burst {
    filter: Svf,
    hz: f32,
    q: f32,
    level: f32,
    /// Per-sample amplitude decay factor.
    decay: f32,
    env: f32,
    age: usize,
}

impl Eval for Crumple {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let density = cx.mono_or("density", &self.density, 40.0)?;
        if let Some(d) = density.constant()
            && !(0.0..=MAX_DENSITY).contains(&d)
        {
            return Err(format!(
                "density: {d}: give a number from 0 to {MAX_DENSITY} bursts per second (40 is a slow crumple, 160 a fast one)"
            ));
        }
        let size = self.size.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&size) {
            return Err(format!("size: {size}: give a number from 0 to 1"));
        }
        let hardness = self.hardness.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&hardness) {
            return Err(format!("hardness: {hardness}: give a number from 0 to 1"));
        }
        let sr = cx.sr;
        let mut rng = cx.rng(self.seed);
        // Centre band, Hz: log-uniform between lo and hi, moved down by size.
        let lo = 1500.0 * 2f32.powf(hardness);
        let hi = 4500.0 * 2f32.powf(hardness);
        let fall = 2f32.powf(-1.5 * (size - 0.5));
        // Decay time constant of the amplitude, seconds: 12 ms soft, 1 ms hard.
        let tau = 0.012 * (1.0f32 / 12.0).powf(hardness) * 2f32.powf(2.0 * (size - 0.5));
        let alpha = lerp(ALPHA[0], ALPHA[1], size);
        let attack = (ATTACK * sr).max(1.0);
        let top = max_hz(sr);

        let mut active: Vec<Burst> = Vec::new();
        let mut out = vec![0.0f32; cx.n];
        // Slots of density accumulated so far, and the position of the next
        // arrival (slot centre plus jitter).
        let mut acc = 0.0f32;
        let mut count = 0.0f32;
        let mut next = 0.5 + (rng.unit() - 0.5) * JITTER;
        for (i, o) in out.iter_mut().enumerate() {
            let d = density.at(i);
            let d = if d.is_finite() {
                d.clamp(0.0, MAX_DENSITY)
            } else {
                0.0
            };
            acc += d / sr;
            while acc >= next {
                count += 1.0;
                next = count + 0.5 + (rng.unit() - 0.5) * JITTER;
                if active.len() >= MAX_ACTIVE {
                    continue;
                }
                let snap = rng.unit() < SNAP_SHARE;
                let u = rng.unit();
                let amp = (1.0 - u.min(0.999_999)).powf(-1.0 / alpha).min(MAX_AMP);
                let v = rng.unit();
                let hz = if snap {
                    hi * 1.3
                } else {
                    lo * (hi / lo).powf(v)
                } * fall;
                // Past 40 per s the bursts shorten as 40 / density, so a tail
                // never buries the next burst (the fine detector, 5 ms apart,
                // still counts them).
                let ring = if snap { 0.4 } else { 1.0 } * (DENSITY_REF / d.max(1.0)).min(1.0);
                let level = if snap { SNAP_LEVEL * amp.sqrt() } else { amp };
                // Level per burst: 1/sqrt(density) below DENSITY_REF (so a
                // sparse crumple is not much quieter), constant above it,
                // where the bursts also shorten as 1/density: the energy per
                // second then stays level instead of piling up.
                let scale = 0.35 * (DENSITY_REF / d.max(1.0)).sqrt().clamp(1.0, 2.0);
                active.push(Burst {
                    filter: Svf::default(),
                    hz: hz.clamp(200.0, top),
                    q: if snap { 0.8 } else { 1.4 },
                    level: level * scale,
                    decay: (-1.0 / (tau * ring * sr)).exp(),
                    env: 1.0,
                    age: 0,
                });
            }
            let mut sum = 0.0;
            for b in &mut active {
                let n = rng.signed();
                let a = if (b.age as f32) < attack {
                    (b.age as f32 + 1.0) / attack
                } else {
                    1.0
                };
                sum += b.filter.tick(n, FilterMode::Bp, b.hz, b.q, 0.0, sr) * b.env * b.level * a;
                b.env *= b.decay;
                b.age += 1;
            }
            active.retain(|b| b.env > 1e-3);
            *o = sum;
        }
        Ok(one(Buf::Audio(out)))
    }
}
