//! Breakage: `fracture_on` turns hard hits into bursts of chips (an event
//! node, for `voices` / `debris`), `dust` is the fine grit that hangs in the
//! air after, its level following how much was hitting.

use serde::Deserialize;

use super::{non_negative, one, positive};
use crate::buf::{Buf, Event};
use crate::dsp::{Biquad, FilterMode};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Most events one `fracture_on` may emit (an error past it).
const MAX_CHIPS: usize = 200_000;
/// Largest chip over smallest, as in `shatter`.
const SIZE_RATIO: f64 = 100.0;

/// For each hit with `value` above `threshold`, maybe a burst of chip events.
/// The strength is s = (value - threshold) / (1 - threshold); the hit breaks
/// with probability s^`sharpness`, then makes 1 + round(`chips` x s) chips
/// over `spread` x (0.3 + 0.7 s) seconds, crowded early, sizes from a power
/// law and scaled by the hit's value.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FractureOn {
    /// The impacts (a collapse, poisson, pattern...): value = strength 0..1.
    pub events: In,
    /// Hits at or under this value never break (default 0.4, 0..0.99).
    #[serde(default)]
    pub threshold: Option<f32>,
    /// Probability exponent, 0.1..8 (default 1): 1 is linear in strength,
    /// above 1 only the hardest hits break, below 1 most hits over the
    /// threshold do.
    #[serde(default)]
    pub sharpness: Option<f32>,
    /// Chips from a full-strength hit (default 12, 1..2000).
    #[serde(default)]
    pub chips: Option<usize>,
    /// Seconds the chips of a full-strength hit spread over (default 0.15).
    #[serde(default)]
    pub spread: Option<f32>,
    /// Power-law exponent of the chip sizes, 0.5..6 (default 2).
    #[serde(default)]
    pub exponent: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for FractureOn {
    const BLURB: &'static str = "hard hits break: each impact event over `threshold` (probability rising with strength) makes a burst of power-law chip events (count and spread grow with strength); drives voices / debris";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "threshold": 0.4, "chips": 12, "spread": 0.15})
    }
}

impl Eval for FractureOn {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        let thr = f64::from(self.threshold.unwrap_or(0.4));
        if !(0.0..0.99).contains(&thr) {
            return Err(format!("threshold: {thr}: give 0..0.99"));
        }
        let sharp = f64::from(self.sharpness.unwrap_or(1.0));
        if !(0.1..=8.0).contains(&sharp) {
            return Err(format!("sharpness: {sharp}: give 0.1..8"));
        }
        let chips = self.chips.unwrap_or(12);
        if !(1..=2000).contains(&chips) {
            return Err(format!("chips: {chips}: give 1..2000"));
        }
        let spread = f64::from(non_negative("spread", self.spread.unwrap_or(0.15))?);
        let b = f64::from(positive("exponent", self.exponent.unwrap_or(2.0))?);
        if !(0.5..=6.0).contains(&b) {
            return Err(format!("exponent: {b}: give 0.5..6"));
        }
        let floor = SIZE_RATIO.powf(-b);
        let mut rng = cx.rng(self.seed);
        let mut out: Vec<(f64, f32)> = Vec::new();
        for e in &events {
            let v = f64::from(e.value.clamp(0.0, 1.0));
            if v <= thr {
                continue;
            }
            let s = (v - thr) / (1.0 - thr);
            if f64::from(rng.unit()) >= s.powf(sharp) {
                continue;
            }
            let count = 1 + (chips as f64 * s).round() as usize;
            if out.len() + count > MAX_CHIPS {
                return Err(format!("more than {MAX_CHIPS} chips: lower `chips`"));
            }
            let span = spread * (0.3 + 0.7 * s);
            for k in 0..count {
                // The first chip is the crack, at the hit; the rest crowd early.
                let t = if k == 0 {
                    e.t
                } else {
                    let x = -(1.0 - f64::from(rng.unit()) * (1.0 - (-5.0f64).exp())).ln() / 5.0;
                    e.t + span * x
                };
                let size =
                    (1.0 - f64::from(rng.unit()) * (1.0 - floor)).powf(-1.0 / b) / SIZE_RATIO;
                let size = if k == 0 { 1.0 } else { size };
                out.push((t, (size * v) as f32));
            }
        }
        let len = cx.n as f64 / f64::from(cx.sr);
        out.retain(|(t, _)| *t >= 0.0 && *t < len);
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        let ev = out
            .into_iter()
            .enumerate()
            .map(|(i, (t, value))| Event {
                t,
                value,
                id: i as u32,
            })
            .collect();
        Ok(one(Buf::Events(ev)))
    }
}

/// What the grit is made of: sets the band and the grain density.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DustMaterial {
    /// Plaster, brick and stone dust: a broad hiss around 3 kHz.
    #[default]
    Masonry,
    /// Splinters and sawdust: duller, around 1.8 kHz, sparser.
    Wood,
    /// Glass grit: bright, narrow, around 6.5 kHz.
    Glass,
}

impl DustMaterial {
    /// (centre Hz, q, grains per second at full energy)
    fn tint(self) -> (f32, f32, f32) {
        match self {
            Self::Masonry => (3000.0, 0.8, 6000.0),
            Self::Wood => (1800.0, 0.8, 3000.0),
            Self::Glass => (6500.0, 1.5, 5000.0),
        }
    }
}

/// A fine granular bed: dense tiny crackles (random-sign impulses with a
/// heavy-tailed size, band-passed), whose density and level follow the
/// smoothed energy of an event stream and fall away over `settle`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dust {
    /// The hits that kick the dust up (value = strength 0..1).
    pub events: In,
    /// masonry (default), wood or glass.
    #[serde(default)]
    pub material: DustMaterial,
    /// Centre of the band, Hz (default by material).
    #[serde(default)]
    pub hz: Option<f32>,
    /// Band-pass q, 0.1..10 (default by material).
    #[serde(default)]
    pub q: Option<f32>,
    /// Grains per second at full energy (default by material, 10..100000).
    #[serde(default)]
    pub density: Option<f32>,
    /// Seconds for the dust to fall 60 dB after the last hit (default 0.8).
    #[serde(default)]
    pub settle: Option<f32>,
    /// Seconds the dust takes to build up after a hit (default 0.03).
    #[serde(default)]
    pub rise: Option<f32>,
    /// How much one full-strength hit adds to the energy, 0.05..10
    /// (default 1; the energy is capped at 1.5).
    #[serde(default)]
    pub amount: Option<f32>,
    /// Output level, dB (default -20).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Dust {
    const BLURB: &'static str = "fine grit after breakage: dense tiny band-limited crackles (masonry, wood, glass) whose density and level follow the smoothed energy of an event stream and settle over `settle`";
    const WORDS: &'static [&'static str] = &["material"];
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "material": "masonry", "settle": 0.8, "gain_db": -20})
    }
}

impl Eval for Dust {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        let (m_hz, m_q, m_density) = self.material.tint();
        let hz = positive("hz", self.hz.unwrap_or(m_hz))?;
        if hz >= cx.sr * 0.45 {
            return Err(format!("hz: {hz}: under {:.0}", cx.sr * 0.45));
        }
        let q = self.q.unwrap_or(m_q);
        if !(0.1..=10.0).contains(&q) {
            return Err(format!("q: {q}: give 0.1..10"));
        }
        let density = self.density.unwrap_or(m_density);
        if !(10.0..=100_000.0).contains(&density) {
            return Err(format!("density: {density}: give 10..100000"));
        }
        let settle = positive("settle", self.settle.unwrap_or(0.8))?;
        let rise = non_negative("rise", self.rise.unwrap_or(0.03))?;
        let amount = self.amount.unwrap_or(1.0);
        if !(0.05..=10.0).contains(&amount) {
            return Err(format!("amount: {amount}: give 0.05..10"));
        }
        let mut kick = vec![0.0f32; cx.n];
        for e in &events {
            let i = (e.t.max(0.0) * f64::from(cx.sr)).round() as usize;
            if let Some(k) = kick.get_mut(i) {
                *k += e.value.clamp(0.0, 1.0).powi(2) * amount;
            }
        }
        // Energy: jumps by each kick, falls 60 dB over `settle`; the level
        // the grains see is that energy smoothed over `rise`.
        let a_dec = 10f32.powf(-3.0 / (settle * cx.sr));
        let a_rise = (-1.0 / (rise.max(1e-4) * cx.sr)).exp();
        let mut bp = Biquad::new();
        bp.set(FilterMode::Bp, hz, q, 0.0, cx.sr);
        let p_max = density / cx.sr;
        let mut rng = cx.rng(self.seed);
        let (mut energy, mut level) = (0.0f32, 0.0f32);
        let gain = 10f32.powf(self.gain_db.unwrap_or(-20.0) / 20.0);
        let mut out = vec![0.0f32; cx.n];
        for (o, k) in out.iter_mut().zip(&kick) {
            energy = ((energy + k) * a_dec).min(1.5);
            level = energy * (1.0 - a_rise) + level * a_rise;
            let mut x = 0.0;
            if rng.unit() < p_max * level.min(1.0).sqrt() {
                // Heavy-tailed grain size, random sign.
                let u = rng.unit();
                let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                x = sign * u * u * u * level.sqrt() * 8.0;
            }
            *o = bp.tick(x) * gain;
        }
        Ok(one(Buf::Audio(out)))
    }
}
