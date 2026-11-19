//! `frog`: calling frogs, after `foley-synth/src/life.rs` of the foley
//! project (github.com/cumsauce/foley, MIT / Apache-2.0). Glottal pulses,
//! 60-150 a second (a trill of 180-260 in a small frog), drive an inflated
//! vocal sac that rings like a Helmholtz resonator: lower the bigger the
//! frog (about 2.5 kHz for a 3 cm tree frog to 500 Hz for a 15 cm bullfrog),
//! in notes of 0.1 to 0.5 s with a breath drawn back between them. The call
//! itself is foley's `frog`, called as it stands; this node adds the repeat
//! (`rate`) and the chorus.
//!
//! Birds are not here: foley's birds (`birds.rs`, `birds/`) are the model the
//! `syrinx` node already wraps (`birdsong2`, 57 species), so a `birdsong` node
//! would be a second name for it.

use std::f32::consts::PI;

use foley_dsp::BandPass;
use serde::Deserialize;

use super::{one, positive};
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

const MAX_CHORUS: u32 = 24;
/// Each call is levelled to this peak (foley's call is not normalized: its
/// level follows the sac's resonance, so a bullfrog and a tree frog differ by
/// several dB).
const CALL_PEAK: f32 = 0.4;

/// Frogs calling.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Frog {
    /// Body length, cm, 2 to 20 (default 6): the sac's pitch falls as the
    /// frog grows (3 cm about 2.5 kHz, 15 cm about 500 Hz). Under 8 cm a
    /// call is sometimes a trill (fast pulses, one long note) instead of a
    /// croak (slow pulses, one to three notes).
    #[serde(default)]
    pub size: Option<f32>,
    /// Calls a second per frog, 0.05 to 4 (default 0.5): each call follows
    /// the last by about 1/rate s (a tenth of it either way).
    #[serde(default)]
    pub rate: Option<f32>,
    /// How many frogs (default 1, 1 to 24). Each is its own individual: its
    /// size within -20 % to +25 % of `size`, its own pace, distance and
    /// start; the sum is levelled by 1/sqrt(n).
    #[serde(default)]
    pub chorus: Option<u32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Frog {
    const BLURB: &'static str = "frogs calling (after the foley project's life.rs): glottal pulses ringing a vocal sac, lower the bigger the frog; size cm, rate calls/s, chorus frogs";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 6, "rate": 0.5, "chorus": 3, "seed": 1})
    }
}

impl Eval for Frog {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = self.size.unwrap_or(6.0);
        if !(size.is_finite() && (2.0..=20.0).contains(&size)) {
            return Err(format!(
                "size: {size}: give the body length in cm from 2 to 20 (3 a tree frog, 15 a bullfrog)"
            ));
        }
        let rate = positive("rate", self.rate.unwrap_or(0.5))?;
        if !(0.05..=4.0).contains(&rate) {
            return Err(format!("rate: {rate}: give calls a second from 0.05 to 4"));
        }
        let chorus = self.chorus.unwrap_or(1);
        if !(1..=MAX_CHORUS).contains(&chorus) {
            return Err(format!(
                "chorus: {chorus}: give how many frogs, 1 to {MAX_CHORUS}"
            ));
        }
        let sr = (cx.sr.round() as u32).max(1);
        let mut rng = cx.rng(self.seed);
        let mut out = vec![0.0f32; cx.n];
        let level = 1.0 / (chorus as f32).sqrt();
        for k in 0..chorus {
            // The first frog is as asked; the others are its neighbours.
            let own = if k == 0 {
                size
            } else {
                (size * (0.8 + 0.45 * rng.unit())).clamp(2.0, 20.0)
            };
            let pace = rate * (0.8 + 0.4 * rng.unit());
            let distance = if k == 0 {
                1.0
            } else {
                0.35 + 0.65 * rng.unit()
            };
            let mut t = if k == 0 { 0.0 } else { rng.unit() / pace };
            while t < cx.n as f32 / cx.sr {
                let mut call = croak(own, &mut rng, sr);
                let peak = call.iter().fold(0f32, |m, x| m.max(x.abs()));
                if peak > 0.0 {
                    let scale = CALL_PEAK * level * distance / peak;
                    call.iter_mut().for_each(|x| *x *= scale);
                }
                let start = (t * cx.sr) as usize;
                for (o, c) in out.iter_mut().skip(start).zip(&call) {
                    *o += c;
                }
                t += (1.0 + 0.1 * rng.signed()) / pace;
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}

/// One call. foley's `frog` with what a listener missed put back: every call
/// at its own pitch (a frog never croaks twice alike: +-12 % a call, +-6 % a
/// note, falling through the note), pulses that are rasping clicks of noise
/// rather than smooth half-sines, and a broad sac (Q 3, not 8) that rings
/// less like a sung vowel. The sac's pitch law is foley's.
fn croak(size_cm: f32, rng: &mut Rng, sample_rate: u32) -> Vec<f32> {
    let rate = sample_rate as f32;
    let sac = 2500.0 * (3.0 / size_cm.clamp(2.0, 20.0)) * (1.0 + 0.12 * rng.signed());
    let trill = size_cm < 8.0 && rng.unit() < 0.35;
    let pulse_rate = if trill {
        180.0 + 80.0 * rng.unit()
    } else {
        60.0 + 90.0 * rng.unit()
    };
    let notes = if trill {
        1
    } else {
        1 + (rng.unit() * 3.0) as usize
    };
    let mut out = Vec::new();
    let mut resonance = [
        BandPass::new_at(sac, 3.0, sample_rate),
        BandPass::new_at(2.3 * sac, 2.5, sample_rate),
    ];
    let mut breath = BandPass::new_at(sac * 0.5, 1.0, sample_rate);
    let width = ((0.0015 * rate) as usize).max(2);
    for _ in 0..notes {
        let note = sac * (1.0 + 0.06 * rng.signed());
        // Pitch falls 4-15 % through a croak; a trill holds it.
        let fall = if trill {
            0.02
        } else {
            0.04 + 0.11 * rng.unit()
        };
        let seconds = if trill {
            0.6 + 0.8 * rng.unit()
        } else {
            0.1 + 0.4 * rng.unit()
        };
        let samples = (seconds * rate) as usize;
        let mut next = 0usize;
        let mut pulse_at = usize::MAX;
        let mut strength = 1.0;
        for i in 0..samples {
            let t = i as f32 / samples as f32;
            if i >= next {
                pulse_at = i;
                strength = 0.6 + 0.8 * rng.unit();
                // Pulses come quicker in the middle of a note.
                let speed = 1.0 + 0.15 * (PI * t).sin();
                let period = rate / (pulse_rate * speed) * (1.0 + 0.08 * rng.signed());
                next = i + period as usize;
                let f = note * (1.0 + 0.04 - fall * t);
                resonance[0].tune_at(f, 3.0, sample_rate);
                resonance[1].tune_at(2.3 * f, 2.5, sample_rate);
            }
            let envelope = (t * 10.0).min(1.0) * ((1.0 - t) * 6.0).min(1.0);
            let phase = i.wrapping_sub(pulse_at);
            let pulse = if phase < width {
                let k = 1.0 - phase as f32 / width as f32;
                strength * k * k * (0.6 * rng.signed() + if phase == 0 { 1.0 } else { 0.0 })
            } else {
                0.0
            };
            let x = envelope * pulse;
            out.push(3.0 * resonance[0].process(x) + 0.6 * resonance[1].process(x) + 0.04 * x);
        }
        let gap = ((0.05 + 0.15 * rng.unit()) * rate) as usize;
        for i in 0..gap {
            let t = i as f32 / gap as f32;
            out.push(0.04 * (PI * t).sin() * breath.process(rng.signed()));
        }
    }
    out
}
