//! `crumple`: paper, foil, plastic or dry leaves being crumpled, as a
//! population of short band-passed noise bursts. Unlike `cloth` the number
//! of bursts follows `density` all the way up (no saturation), and the
//! arrivals are a jittered grid rather than a Poisson process, so a count of
//! fine onsets tracks the density instead of merging neighbours. Tests in
//! `crates/sfx_analysis/tests/it/surface.rs`.
//!
//! `buckle` (sheet metal snapping through under load) and `tear` (ductile
//! sheet metal tearing along a line) are the same family for metal: event
//! processes with an audio port, a `force` port for a body and an `events`
//! port. Reference: `docs/nodes/sheet_metal.md`; tests:
//! `crates/sfx_analysis/tests/it/sheet_metal.rs`.

use serde::Deserialize;

use super::one;
use crate::buf::{Buf, Event, EventExtra, Sig};
use crate::dsp::{FilterMode, OnePole, Svf, max_hz};
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

// ------------------------------------------------------------ sheet metal

/// What a buckling or tearing sheet is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SheetMaterial {
    /// Mild steel: E 200 GPa, 7850 kg/m^3.
    Steel,
    /// Aluminium alloy (aircraft skin, cans, foil): E 70 GPa, 2700 kg/m^3.
    /// Alias `aluminum`.
    #[serde(alias = "aluminum")]
    Aluminium,
    /// Copper: E 120 GPa, 8960 kg/m^3.
    Copper,
    /// Titanium alloy: E 110 GPa, 4500 kg/m^3.
    Titanium,
}

impl SheetMaterial {
    /// (Young's modulus Pa, density kg/m^3).
    fn props(self) -> (f32, f32) {
        match self {
            Self::Steel => (200e9, 7850.0),
            Self::Aluminium => (70e9, 2700.0),
            Self::Copper => (120e9, 8960.0),
            Self::Titanium => (110e9, 4500.0),
        }
    }
    /// Lowest mode of a simply supported square plate of side l and
    /// thickness h is `plate_k() * h / l^2` Hz: pi * sqrt(E / (12 rho
    /// (1 - nu^2))), nu = 0.3.
    fn plate_k(self) -> f32 {
        let (e, rho) = self.props();
        std::f32::consts::PI * (e / (12.0 * rho * 0.91)).sqrt()
    }
    /// Longitudinal bar speed sqrt(E / rho), m/s.
    fn bar_speed(self) -> f32 {
        let (e, rho) = self.props();
        (e / rho).sqrt()
    }
}

/// Errors unless `x` is finite and within lo..=hi.
fn within(field: &str, x: f32, lo: f32, hi: f32) -> Result<f32, String> {
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give a number from {lo} to {hi}"))
    }
}

/// A signal read at sample i, finite and clamped to lo..=hi (0 when not finite).
fn clamped(s: &Sig, i: usize, lo: f32, hi: f32) -> f32 {
    let v = s.at(i);
    if v.is_finite() { v.clamp(lo, hi) } else { 0.0 }
}

/// Most events one `buckle` or `tear` may emit on its `events` port (an
/// error past it, as `shatter`).
const MAX_SHEET_EVENTS: usize = 200_000;

/// Smallest facet that snaps, in sheet thicknesses. With it a 0.02 mm foil's
/// smallest facets ring near 10 kHz and its typical ones at 3-6 kHz, where
/// `crumple` puts foil; a 2 mm steel panel's smallest are 0.3 m and ring
/// near 100 Hz.
const FACET_MIN_H: f32 = 150.0;
/// Events per second at full load per (sheet size / smallest facet): 10 per
/// second for a 1 m, 2 mm panel, 200 for a 0.2 m sheet of foil.
const BUCKLE_RATE_K: f32 = 3.0;
/// Highest event rate, per second (a `rate` signal or the law).
const MAX_BUCKLE_RATE: f32 = 2000.0;
/// Compressive strain at load 1, which sets the dimple depth: the Euler
/// buckle of a strip of length l under strain e rises (2 / pi) l sqrt(e).
const STRAIN_MAX: f32 = 0.01;
/// Most snaps sounding at once (a snap that would be the next is skipped
/// for the audio; it still goes on the events port).
const MAX_SNAPS: usize = 256;

/// The momentum of a snap-through: mass rho h l^2 moving the dimple depth
/// w = (2 / pi) l sqrt(strain) in half a period of the facet's lowest mode
/// f = k h / l^2, so p = (4 / pi) rho k h^2 l sqrt(strain).
fn snap_momentum(m: SheetMaterial, h: f32, l: f32, strain: f32) -> f32 {
    let (_, rho) = m.props();
    4.0 / std::f32::consts::PI * rho * m.plate_k() * h * h * l * strain.max(0.0).sqrt()
}

/// The momentum an event value of 1 stands for: a 1 m facet of 6 mm steel
/// at full load. `value` = (p / this)^(1/4), so 16x the momentum is 2x the
/// value (+6 dB).
fn momentum_ref() -> f32 {
    snap_momentum(SheetMaterial::Steel, 0.006, 1.0, STRAIN_MAX)
}

/// Sheet metal oil-canning and buckling under load.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Buckle {
    /// How hard the sheet is pushed, 0..1 (default 0.5; a number or a
    /// signal). Sets the event rate (when `rate` is not given) and the strain
    /// each facet snaps under (the strength goes as sqrt(load)). Alias
    /// `force`. A number outside 0..1 is an error; a signal is clamped.
    #[serde(default, alias = "force")]
    pub load: Option<In>,
    /// Events per second (a number or a signal, 0..2000), in place of the
    /// rate the load and sheet give. A number outside it is an error; a
    /// signal is clamped.
    #[serde(default)]
    pub rate: Option<In>,
    /// Side of the sheet, metres (default 1, 0.01..20): the largest facet is
    /// half of it, and a bigger sheet has more room for facets (rate grows
    /// as size / smallest facet).
    #[serde(default)]
    pub size: Option<f32>,
    /// Sheet thickness, metres (default 0.002, 0.00001..0.05). The smallest
    /// facet is 150 thicknesses (but never over half the sheet); thicker is
    /// fewer, stronger, lower snaps. Alias `gauge`.
    #[serde(default, alias = "gauge")]
    pub thickness: Option<f32>,
    /// steel (default), aluminium (alias aluminum), copper or titanium.
    #[serde(default)]
    pub material: Option<SheetMaterial>,
    /// Power-law exponent of the facet sizes, 0.5..6 (default 2): the number
    /// of facets larger than l goes as l^-exponent. Bigger = fewer big ones.
    #[serde(default)]
    pub exponent: Option<f32>,
    /// How much of each facet's own ring (band-passed noise at its lowest
    /// mode) is on `out`, 0..1 (default 0.5). The `force` port never has it.
    #[serde(default)]
    pub ring: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Buckle {
    const BLURB: &'static str = "sheet metal buckling / oil-canning under `load`: snap-through events with power-law facet sizes set by size, thickness and material (thick steel: few low bangs; foil: many small ticks, as crumple); out = force pulses + facet rings, `force` port for modal / plate, `events` port (value = strength, $size = facet) for voices / debris";
    const WORDS: &'static [&'static str] = &["material"];
    fn example() -> serde_json::Value {
        serde_json::json!({"load": 0.6, "size": 1.0, "thickness": 0.002, "material": "steel", "seed": 1})
    }
}

/// One sounding snap: a force pulse and the facet's ring.
struct Snap {
    age: usize,
    /// Samples of the rise and of the fall of the pulse.
    rise: usize,
    fall: usize,
    amp: f32,
    filter: Svf,
    hz: f32,
    ring: f32,
    env: f32,
    decay: f32,
}

impl Eval for Buckle {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let load = cx.mono_or("load", &self.load, 0.5)?;
        if let Some(v) = load.constant() {
            within("load", v, 0.0, 1.0)?;
        }
        let rate = match &self.rate {
            Some(r) => {
                let s = cx.mono("rate", r)?;
                if let Some(v) = s.constant() {
                    within("rate", v, 0.0, MAX_BUCKLE_RATE)?;
                }
                Some(s)
            }
            None => None,
        };
        let size = within("size", self.size.unwrap_or(1.0), 0.01, 20.0)?;
        let h = within("thickness", self.thickness.unwrap_or(0.002), 0.000_01, 0.05)?;
        let b = within("exponent", self.exponent.unwrap_or(2.0), 0.5, 6.0)?;
        let ring = within("ring", self.ring.unwrap_or(0.5), 0.0, 1.0)?;
        let mat = self.material.unwrap_or(SheetMaterial::Steel);
        let sr = cx.sr;
        let l_max = size * 0.5;
        let l_min = (FACET_MIN_H * h).min(l_max);
        let ratio = l_max / l_min;
        let floor = ratio.powf(-b);
        let k = mat.plate_k();
        let p_ref = momentum_ref();
        let law_rate = BUCKLE_RATE_K * size / l_min;
        let top = max_hz(sr) * 0.9;

        let mut rng = cx.rng(self.seed);
        let mut force = vec![0.0f32; cx.n];
        let mut out = vec![0.0f32; cx.n];
        let mut events: Vec<Event> = Vec::new();
        let mut active: Vec<Snap> = Vec::new();
        let mut acc = 0.0f32;
        let mut next = rng.exponential(1.0);
        let mut dc = OnePole::default();
        for i in 0..cx.n {
            let ld = clamped(&load, i, 0.0, 1.0);
            let r = match &rate {
                Some(s) => clamped(s, i, 0.0, MAX_BUCKLE_RATE),
                None => (ld * law_rate).min(MAX_BUCKLE_RATE),
            };
            acc += r / sr;
            while acc >= next {
                acc -= next;
                next = rng.exponential(1.0);
                // Facet span: a Pareto truncated to l_min..l_max.
                let u = rng.unit();
                let l = if ratio > 1.0 {
                    l_min * (1.0 - u * (1.0 - floor)).powf(-1.0 / b)
                } else {
                    l_min
                };
                let hz = (k * h / (l * l)).clamp(20.0, top);
                let strain = ld.max(0.02) * STRAIN_MAX;
                let value = (snap_momentum(mat, h, l, strain) / p_ref)
                    .powf(0.25)
                    .min(1.0);
                if events.len() >= MAX_SHEET_EVENTS {
                    return Err(format!(
                        "more than {MAX_SHEET_EVENTS} snaps: lower `load` / `rate` or shorten the sound"
                    ));
                }
                events.push(Event {
                    t: f64::from(i as f32 / sr),
                    value,
                    id: events.len() as u32,
                    extra: EventExtra {
                        size: Some(l),
                        ..Default::default()
                    },
                });
                if active.len() >= MAX_SNAPS {
                    continue;
                }
                // The snap takes half a period of the facet's lowest mode:
                // it rises over 3/4 of that and stops as the facet lands in
                // its inverted shape. The peak force
                // falls as 1 / l (its impulse, peak x time, grows as l).
                let half = (0.5 / hz * sr).max(2.0);
                let rise = ((half * 0.75) as usize).max(1);
                // The landing is abrupt (at most 0.1 ms): it is what gives a
                // big slow snap its pop.
                let fall = ((half * 0.25).min(0.0001 * sr) as usize).max(1);
                let amp = value * l_min / l;
                active.push(Snap {
                    age: 0,
                    rise,
                    fall,
                    amp,
                    filter: Svf::default(),
                    hz,
                    // White noise through the band (width hz / 2) scaled so
                    // the ring starts at an RMS of `amp` x `ring`.
                    ring: amp * ring / (0.577 * (hz / (2.0 * 0.5 * sr)).sqrt()),
                    env: if ring > 0.0 { 1.0 } else { 0.0 },
                    decay: (-1.0 / ((4.0 / hz).max(0.0008) * sr)).exp(),
                });
            }
            let (mut f, mut o) = (0.0f32, 0.0f32);
            for s in &mut active {
                let p = if s.age < s.rise {
                    0.5 - 0.5 * (std::f32::consts::PI * s.age as f32 / s.rise as f32).cos()
                } else if s.age < s.rise + s.fall {
                    let x = (s.age - s.rise) as f32 / s.fall as f32;
                    0.5 + 0.5 * (std::f32::consts::PI * x).cos()
                } else {
                    0.0
                };
                f += p * s.amp;
                if s.env > 1e-3 {
                    o += s
                        .filter
                        .tick(rng.signed(), FilterMode::Bp, s.hz, 2.0, 0.0, sr)
                        * s.ring
                        * s.env;
                    s.env *= s.decay;
                }
                s.age += 1;
            }
            active.retain(|s| s.age < s.rise + s.fall || s.env > 1e-3);
            force[i] = f;
            // `out` is what the air hears: no DC from the pulses.
            out[i] = dc.tick(f + o, 20.0, sr).1;
        }
        Ok(vec![
            ("out", Buf::Audio(out)),
            ("force", Buf::Audio(force)),
            ("events", Buf::Events(events)),
        ])
    }
}

/// Micro-fractures per second above which the train is a continuous hiss:
/// more are summed into this many, each louder by sqrt(rate / this).
const MAX_MICRO_RATE: f32 = 20_000.0;
/// Pareto exponent of the micro-fracture amplitudes (P(A > a) = a^-2,
/// a >= 1) and the largest one.
const MICRO_ALPHA: f32 = 2.0;
const MICRO_MAX: f32 = 6.0;
/// A ligament snap (a micro-fracture that goes on the `events` port, at
/// MICRO_MAX) comes once per this many thicknesses of tear.
const SNAP_H: f32 = 200.0;
/// Centre of the radiated band, Hz, for 1 mm aluminium at 10 m/s.
const TEAR_HZ: f32 = 2600.0;

/// Ductile sheet metal tearing along a line.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tear {
    /// How fast the tear runs, m/s (default 10; a number or a signal,
    /// 0..300). The micro-fracture rate is speed / `grain`, the hiss swells
    /// and brightens with it, and 0 is silence. A number outside 0..300 is an
    /// error; a signal is clamped.
    #[serde(default)]
    pub speed: Option<In>,
    /// Sheet thickness, metres (default 0.001, 0.0001..0.02): thinner is
    /// brighter (the band centre goes as thickness^-0.3) and quieter.
    /// Alias `gauge`.
    #[serde(default, alias = "gauge")]
    pub thickness: Option<f32>,
    /// aluminium (default; alias aluminum), steel, copper or titanium.
    #[serde(default)]
    pub material: Option<SheetMaterial>,
    /// Tear length per micro-fracture, metres (default half the thickness,
    /// 0.00001..0.1): the void-to-void step of a ductile crack.
    #[serde(default)]
    pub grain: Option<f32>,
    /// Spread of the gaps between micro-fractures, 0..1 (default 1: random
    /// arrivals, no pitch). 0 is a regular train at speed / grain per second
    /// (its random signs and sizes still keep it unpitched; ask for a pitch
    /// with `pitch_m`).
    #[serde(default)]
    pub jitter: Option<f32>,
    /// Spacing of rivets or stringers along the tear, metres (default none,
    /// 0.005..10): each one crossed pops (a strong event, +`pop_db`), so
    /// pops come at speed / pitch_m per second. Alias `pitch`, `rivet_pitch`.
    #[serde(default, alias = "pitch", alias = "rivet_pitch")]
    pub pitch_m: Option<f32>,
    /// Level of a rivet pop over the largest micro-fracture, dB (default 6,
    /// 0..30).
    #[serde(default)]
    pub pop_db: Option<f32>,
    /// Level of the broadband radiated hiss on `out`, 0..1 (default 0.5).
    #[serde(default)]
    pub hiss: Option<f32>,
    /// Level of the sheet's fluttering rumble under 125 Hz on `out`, 0..1
    /// (default 0.3).
    #[serde(default)]
    pub rumble: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Tear {
    const BLURB: &'static str = "ductile sheet metal tearing at `speed` m/s: a dense random train of power-law micro-fractures (speed / grain per second), a radiated hiss that swells and brightens with speed, a faint flutter rumble, optional rivet pops every `pitch_m`; out = the radiated mix, `force` port (the fracture train) for modal / plate, `events` port (ligament snaps and pops) for voices / debris";
    const WORDS: &'static [&'static str] = &["material"];
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 10, "thickness": 0.001, "material": "aluminium", "seed": 1})
    }
}

impl Eval for Tear {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono_or("speed", &self.speed, 10.0)?;
        if let Some(v) = speed.constant() {
            within("speed", v, 0.0, 300.0)?;
        }
        let h = within("thickness", self.thickness.unwrap_or(0.001), 0.0001, 0.02)?;
        let mat = self.material.unwrap_or(SheetMaterial::Aluminium);
        let grain = within("grain", self.grain.unwrap_or(0.5 * h), 0.000_01, 0.1)?;
        let jitter = within("jitter", self.jitter.unwrap_or(1.0), 0.0, 1.0)?;
        let pitch_m = match self.pitch_m {
            Some(p) => Some(within("pitch_m", p, 0.005, 10.0)?),
            None => None,
        };
        let pop = 10f32.powf(within("pop_db", self.pop_db.unwrap_or(6.0), 0.0, 30.0)? / 20.0);
        let hiss = within("hiss", self.hiss.unwrap_or(0.5), 0.0, 1.0)?;
        let rumble = within("rumble", self.rumble.unwrap_or(0.3), 0.0, 1.0)?;
        let sr = cx.sr;
        // Band centre: thinner and stiffer (faster bending waves) is
        // brighter; speed moves it by speed^0.15 at the sample.
        let stiff = (mat.bar_speed() / SheetMaterial::Aluminium.bar_speed()).sqrt();
        let base_hz = TEAR_HZ * (0.001 / h).powf(0.3) * stiff;
        // Level: thicker metal releases more per step (h^0.5, quieter foil).
        let level = 0.08 * (h / 0.001).sqrt();
        let snap_p = (grain / (SNAP_H * h)).min(1.0);
        let micro_floor = MICRO_MAX.powf(-MICRO_ALPHA);

        let mut rng = cx.rng(self.seed);
        let mut force = vec![0.0f32; cx.n];
        let mut out = vec![0.0f32; cx.n];
        let mut events: Vec<Event> = Vec::new();
        // Arrivals: `acc` counts micro-fractures due; the next one comes
        // after a gap of (1 - jitter) + jitter x Exp(1) mean gaps.
        let gap = |rng: &mut crate::dsp::Rng| (1.0 - jitter) + jitter * rng.exponential(1.0);
        let mut acc = 0.0f32;
        let mut next = gap(&mut rng) * (0.5 + 0.5 * rng.unit());
        let mut length = 0.0f32;
        let mut next_pop = pitch_m.map(|p| p * (0.5 + 0.1 * rng.signed()));
        // The pop pulse: a raised cosine of 0.4 ms.
        let pop_len = ((0.0004 * sr) as usize).max(2);
        let mut pops: Vec<(usize, f32)> = Vec::new();
        let (mut hp, mut lp, mut tilt) = (Svf::default(), Svf::default(), OnePole::default());
        let (mut rum_lp, mut rum_lp2) = (Svf::default(), Svf::default());
        let mut env = 0.0f32;
        let mut wob = 0.0f32;
        let mut wob_target = 0.0f32;
        let mut flutter_ph = rng.unit();
        let mut flutter_hz = 8.0 + 8.0 * rng.unit();
        for i in 0..cx.n {
            let v = clamped(&speed, i, 0.0, 300.0);
            length += v / sr;
            // Micro-fractures this sample.
            let rate = v / grain;
            let (eff, boost) = if rate > MAX_MICRO_RATE {
                (MAX_MICRO_RATE, (rate / MAX_MICRO_RATE).sqrt())
            } else {
                (rate, 1.0)
            };
            acc += eff / sr;
            let mut f = 0.0f32;
            while acc >= next {
                acc -= next;
                next = gap(&mut rng).max(1e-3);
                // Summed arrivals stand for rate / eff fractures each, so a
                // snap is that much likelier: one per SNAP_H thicknesses.
                let big = rng.unit() < snap_p * rate / eff.max(1e-9);
                let a = if big {
                    MICRO_MAX
                } else {
                    (1.0 - rng.unit() * (1.0 - micro_floor)).powf(-1.0 / MICRO_ALPHA)
                };
                let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                f += sign * a * boost * level;
                if big {
                    if events.len() >= MAX_SHEET_EVENTS {
                        return Err(format!(
                            "more than {MAX_SHEET_EVENTS} snaps: lower `speed` or shorten the sound"
                        ));
                    }
                    events.push(Event {
                        t: f64::from(i as f32 / sr),
                        value: 0.5,
                        id: events.len() as u32,
                        extra: EventExtra {
                            size: Some(grain),
                            ..Default::default()
                        },
                    });
                }
            }
            if let (Some(p), Some(np)) = (pitch_m, next_pop.as_mut())
                && length >= *np
            {
                *np += p * (1.0 + 0.02 * rng.signed());
                pops.push((0, pop * MICRO_MAX * level));
                if events.len() >= MAX_SHEET_EVENTS {
                    return Err(format!(
                        "more than {MAX_SHEET_EVENTS} events: lower `speed` or shorten the sound"
                    ));
                }
                events.push(Event {
                    t: f64::from(i as f32 / sr),
                    value: 1.0,
                    id: events.len() as u32,
                    extra: EventExtra {
                        size: Some(p),
                        ..Default::default()
                    },
                });
            }
            for (age, amp) in &mut pops {
                let x = *age as f32 / pop_len as f32;
                f += *amp * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos());
                *age += 1;
            }
            pops.retain(|(age, _)| *age < pop_len);
            force[i] = f;

            // The radiated hiss: noise whose level follows the speed
            // (rising over 30 ms, falling over 80 ms).
            let target = (v / 10.0).min(10.0);
            let t = if target > env { 0.03 } else { 0.08 };
            env += (target - env) * (1.0 - (-1.0 / (t * sr)).exp());
            // A slow random wobble of the band (+-25 %, new target every
            // ~0.1 s): the screech moves, it never holds a pitch.
            if rng.unit() < 10.0 / sr {
                wob_target = rng.signed();
            }
            wob += (wob_target - wob) * (1.0 - (-1.0 / (0.05 * sr)).exp());
            let hz =
                (base_hz * env.max(0.05).powf(0.15) * (1.0 + 0.25 * wob)).min(max_hz(sr) * 0.4);
            let noise = rng.signed() * hiss * level * 3.0 * env.sqrt();
            // Radiation band: high-passed at hz / 3 (12 dB/oct), low-passed
            // at 2.5 hz, then a gentle -6 dB/oct tilt from hz / 2 up.
            let x = f + noise;
            let x = hp.tick(x, FilterMode::Hp, hz / 3.0, 0.7, 0.0, sr);
            let x = lp.tick(x, FilterMode::Lp, hz * 2.5, 0.7, 0.0, sr);
            let (t_lo, t_hi) = tilt.tick(x, hz * 0.5, sr);
            let radiated = t_lo + 0.35 * t_hi;
            // The flapping sheet: noise band-passed to 70-130 Hz, beating at
            // a wandering 8-16 Hz.
            flutter_ph += flutter_hz / sr;
            if flutter_ph >= 1.0 {
                flutter_ph -= 1.0;
                flutter_hz = 8.0 + 8.0 * rng.unit();
            }
            let am = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * flutter_ph).sin();
            let r = rum_lp.tick(rng.signed(), FilterMode::Lp, 130.0, 0.7, 0.0, sr);
            let r = rum_lp2.tick(r, FilterMode::Hp, 70.0, 0.7, 0.0, sr);
            out[i] = radiated + r * am * rumble * level * 12.0 * env.sqrt();
        }
        Ok(vec![
            ("out", Buf::Audio(out)),
            ("force", Buf::Audio(force)),
            ("events", Buf::Events(events)),
        ])
    }
}
