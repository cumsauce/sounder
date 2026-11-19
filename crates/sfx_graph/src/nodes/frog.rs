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

use foley_synth::frog;
use serde::Deserialize;

use super::{one, positive};
use crate::buf::Buf;
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
                let mut call = frog(own, rng.next_u64(), sr);
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
