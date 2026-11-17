//! Fracture: `shatter`, an event generator for breaking things. One crack at
//! `at`, then fragments whose sizes follow a power law and whose times crowd
//! right after the break and thin out. Feed it to `voices` (a small `modal`
//! per fragment) or `grains`.

use serde::Deserialize;

use super::{non_negative, one, positive};
use crate::buf::{Buf, Event};
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// Most fragments one node may make (an error past it, not a clamp).
const MAX_PIECES: usize = 200_000;

/// Largest fragment over smallest: sizes lie in `1 / SIZE_RATIO ..= 1`.
const SIZE_RATIO: f64 = 100.0;

/// How fast the fragments thin out: the time density falls as
/// `e^(-TIME_RATE x)` over the spread (x = 0..1), so 99.3 % of them come
/// before the end.
const TIME_RATE: f64 = 5.0;

/// A uniform draw in [0, 1) with 53 bits.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A breaking object: `pieces` events, the first at `at` with value 1 (the
/// crack), the rest scattered over `spread` seconds after it, dense early.
/// Each event's `value` is a fragment size in 1/100 .. 1 drawn from a power
/// law: the number of fragments larger than s goes as s^-`exponent`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shatter {
    /// How many events (fragments including the first crack), 1..200000.
    pub pieces: usize,
    /// Seconds from the crack to the end of the cascade (default 0.4).
    #[serde(default)]
    pub spread: Option<f32>,
    /// Time of the crack, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Power-law exponent of the size distribution, 0.5..6 (default 2:
    /// brittle fracture of a flat sheet; bigger = fewer large fragments).
    #[serde(default)]
    pub exponent: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Shatter {
    const BLURB: &'static str = "a breaking cascade of events: a crack at `at`, then power-law fragment sizes (values) crowded early over `spread`; drives voices";
    fn example() -> serde_json::Value {
        serde_json::json!({"pieces": 40, "spread": 0.4, "at": 0.0, "exponent": 2.0})
    }
}

impl Eval for Shatter {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if self.pieces == 0 || self.pieces > MAX_PIECES {
            return Err(format!("pieces: {}: give 1..{MAX_PIECES}", self.pieces));
        }
        let spread = f64::from(non_negative("spread", self.spread.unwrap_or(0.4))?);
        let at = f64::from(non_negative("at", self.at.unwrap_or(0.0))?);
        let b = f64::from(positive("exponent", self.exponent.unwrap_or(2.0))?);
        if !(0.5..=6.0).contains(&b) {
            return Err(format!("exponent: {b}: give 0.5..6"));
        }
        let mut rng = cx.rng(self.seed);
        let floor = SIZE_RATIO.powf(-b);
        let mut events: Vec<(f64, f32)> = Vec::with_capacity(self.pieces);
        events.push((at, 1.0));
        for _ in 1..self.pieces {
            // Time: a truncated exponential over 0..1.
            let x = -(1.0 - unit(&mut rng) * (1.0 - (-TIME_RATE).exp())).ln() / TIME_RATE;
            // Size: inverse CDF of a Pareto truncated to 1..ratio,
            // F(s) = (1 - s^-b) / (1 - ratio^-b), scaled to 1/ratio..1.
            let u = unit(&mut rng);
            let size = (1.0 - u * (1.0 - floor)).powf(-1.0 / b) / SIZE_RATIO;
            events.push((at + spread * x, size as f32));
        }
        let len = cx.n as f64 / f64::from(cx.sr);
        events.retain(|(t, _)| *t >= 0.0 && *t < len);
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let out = events
            .into_iter()
            .enumerate()
            .map(|(i, (t, value))| Event {
                t,
                value,
                id: i as u32,
            })
            .collect();
        Ok(one(Buf::Events(out)))
    }
}
