//! Fracture: `shatter`, an event generator for breaking things. One crack at
//! `at`, then fragments whose sizes follow a power law and whose times crowd
//! right after the break and thin out. Feed it to `voices` (a small `modal`
//! per fragment) or `grains`.

use serde::Deserialize;

use super::{non_negative, one, positive};
use crate::buf::{Buf, Event, EventExtra};
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, In, Outs};

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
                ..Default::default()
            })
            .collect();
        Ok(one(Buf::Events(out)))
    }
}

/// Standard gravity, m/s^2.
const G: f64 = 9.81;

/// Most bricks one `collapse` may hold (rows x cols).
const MAX_BRICKS: usize = 20_000;

/// How a `collapse` fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CollapseMode {
    /// The wall rotates about its base as one slab; each brick leaves it at
    /// an angle that grows toward the bottom rows, so the top lands first.
    #[default]
    Topple,
    /// A failure point (`origin`) gives way and the release spreads through
    /// the wall at `speed`; each brick drops from where it was.
    Crumble,
}

/// A wall (or stack) of bricks falling: the impact times and speeds come
/// from the fall itself (a slab rotating over, free fall from each brick's
/// height, bounces with restitution, bricks landing on bricks), not from a
/// statistical spread.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Collapse {
    /// Courses of bricks, bottom to top (default 10).
    #[serde(default)]
    pub rows: Option<usize>,
    /// Bricks per course (default 10).
    #[serde(default)]
    pub cols: Option<usize>,
    /// Height of one course, metres (default 0.075).
    #[serde(default)]
    pub brick: Option<f32>,
    /// Length of one brick along the wall, metres (default 0.22; crumble only).
    #[serde(default)]
    pub width: Option<f32>,
    /// topple (default) or crumble.
    #[serde(default)]
    pub mode: CollapseMode,
    /// Starting lean of a toppling wall, radians (default 0.15): smaller is
    /// a slower start.
    #[serde(default)]
    pub lean: Option<f32>,
    /// Crumble: where it gives way, [column, row] as 0..1 of the wall
    /// (default [0.5, 0.2]).
    #[serde(default)]
    pub origin: Option<[f32; 2]>,
    /// Crumble: how fast the failure spreads, m/s (default 2).
    #[serde(default)]
    pub speed: Option<f32>,
    /// How loose the bricks are, 0..1 (default 0.3): scatter on each
    /// brick's release (a tight wall at 0 falls in lockstep).
    #[serde(default)]
    pub looseness: Option<f32>,
    /// Bounce restitution, 0..0.9 (default 0.25).
    #[serde(default)]
    pub restitution: Option<f32>,
    /// Most bounces per brick after its first hit (default 2, 0..6).
    #[serde(default)]
    pub bounces: Option<usize>,
    /// Fraction of bricks that land on bricks already down, 0..1 (default
    /// 0.4): a shorter fall, on the `pile` port.
    #[serde(default)]
    pub pile: Option<f32>,
    /// The N fastest first impacts also go on the `hero` port, for a `rig`
    /// or a close `voices` layer (default 0).
    #[serde(default)]
    pub hero: Option<usize>,
    /// Time the wall starts to go, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// How much of the stereo field the wall spans, 0..1 (default 1): each
    /// event's pan is its column, -pan_width (first) .. pan_width (last).
    #[serde(default)]
    pub pan_width: Option<f32>,
    /// Metres from the listener to the wall's middle, straight ahead
    /// (default none): gives each event a position (x along the wall, y 0,
    /// z this) and so a distance.
    #[serde(default)]
    pub distance: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Collapse {
    const BLURB: &'static str = "a wall of bricks falling (topple or crumble): impact events timed by the fall (sqrt(2h/g), bounces, a pile), value = impact speed; ports ground pile bounce hero; drives debris";
    const WORDS: &'static [&'static str] = &["mode"];
    fn example() -> serde_json::Value {
        serde_json::json!({"rows": 10, "cols": 10, "mode": "topple", "looseness": 0.3, "pile": 0.4})
    }
}

/// One impact: (time, speed m/s, port 0 ground / 1 pile / 2 bounce, column).
type Hit = (f64, f64, u8, usize);

/// Seconds to fall `h` metres starting at `v0` m/s downward, and the speed at the end.
fn fall(h: f64, v0: f64) -> (f64, f64) {
    let h = h.max(0.0);
    let t = (-v0 + (v0 * v0 + 2.0 * G * h).sqrt()) / G;
    (t, v0 + G * t)
}

/// A toppling slab of height `hw` from `lean` rad: (time, angle, angular speed)
/// samples every millisecond until it lies flat. A thin rod about its base:
/// theta'' = 3 g / (2 H) sin theta.
fn topple_path(hw: f64, lean: f64) -> Vec<(f64, f64, f64)> {
    let k = 1.5 * G / hw.max(1e-3);
    let (mut th, mut w, mut t) = (lean, 0.0f64, 0.0f64);
    let dt = 0.0005;
    let mut out = vec![(t, th, w)];
    while th < std::f64::consts::FRAC_PI_2 && t < 30.0 {
        w += k * th.sin() * dt;
        th += w * dt;
        t += dt;
        out.push((t, th, w));
    }
    out
}

impl Eval for Collapse {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rows = self.rows.unwrap_or(10);
        let cols = self.cols.unwrap_or(10);
        if rows == 0 || cols == 0 || rows * cols > MAX_BRICKS {
            return Err(format!(
                "rows x cols: {rows} x {cols}: give 1.. each, at most {MAX_BRICKS} bricks"
            ));
        }
        let brick = f64::from(positive("brick", self.brick.unwrap_or(0.075))?);
        let width = f64::from(positive("width", self.width.unwrap_or(0.22))?);
        let lean = f64::from(positive("lean", self.lean.unwrap_or(0.15))?).min(1.2);
        let speed = f64::from(positive("speed", self.speed.unwrap_or(2.0))?);
        let loose = f64::from(self.looseness.unwrap_or(0.3));
        if !(0.0..=1.0).contains(&loose) {
            return Err(format!("looseness: {loose}: give 0..1"));
        }
        let e = f64::from(self.restitution.unwrap_or(0.25));
        if !(0.0..=0.9).contains(&e) {
            return Err(format!("restitution: {e}: give 0..0.9"));
        }
        let bounces = self.bounces.unwrap_or(2);
        if bounces > 6 {
            return Err(format!("bounces: {bounces}: give 0..6"));
        }
        let pile = f64::from(self.pile.unwrap_or(0.4));
        if !(0.0..=1.0).contains(&pile) {
            return Err(format!("pile: {pile}: give 0..1"));
        }
        let at = f64::from(non_negative("at", self.at.unwrap_or(0.0))?);
        let hw = rows as f64 * brick;
        let mut rng = cx.rng(self.seed);
        let mut u = || f64::from(rng.unit());
        // Release time, height and downward speed of each brick.
        let mut releases: Vec<(f64, f64, f64, usize)> = Vec::with_capacity(rows * cols);
        match self.mode {
            CollapseMode::Topple => {
                let path = topple_path(hw, lean);
                let flat = path.last().copied().unwrap_or((0.0, 0.0, 0.0));
                for r in 0..rows {
                    let h0 = (r as f64 + 0.5) * brick;
                    let frac = h0 / hw;
                    for c in 0..cols {
                        // The top courses peel off early, the bottom ones
                        // ride the slab down; looseness scatters the angle.
                        let th_rel =
                            (0.35 + 1.1 * (1.0 - frac)) * (1.0 + loose * 1.2 * (u() - 0.5));
                        let p = path.iter().find(|p| p.1 >= th_rel).copied().unwrap_or(flat);
                        let h = h0 * p.1.cos();
                        let v0 = p.2 * h0 * p.1.sin();
                        releases.push((p.0, h, v0, c));
                    }
                }
            }
            CollapseMode::Crumble => {
                let o = self.origin.unwrap_or([0.5, 0.2]);
                let (oc, or) = (f64::from(o[0]) * cols as f64 * width, f64::from(o[1]) * hw);
                for r in 0..rows {
                    let h0 = (r as f64 + 0.5) * brick;
                    for c in 0..cols {
                        let x = (c as f64 + 0.5) * width;
                        let d = ((x - oc).powi(2) + (h0 - or).powi(2)).sqrt();
                        let t = d / speed * (1.0 + loose * (u() - 0.5));
                        releases.push((t, h0, 0.0, c));
                    }
                }
            }
        }
        // Bricks land in release-plus-fall order; a pile grows under the
        // later ones.
        let mut firsts: Vec<(f64, f64, f64, usize)> = releases
            .iter()
            .map(|&(t, h, v0, c)| {
                let jit = loose * 0.15 * u() * u();
                (t + jit, h, v0, c)
            })
            .collect();
        firsts.sort_by(|a, b| (a.0 + fall(a.1, a.2).0).total_cmp(&(b.0 + fall(b.1, b.2).0)));
        let n = firsts.len() as f64;
        let mut hits: Vec<Hit> = Vec::with_capacity(firsts.len() * (bounces + 1));
        let mut first_hits: Vec<(f64, f64, usize)> = Vec::new();
        for (i, &(t, h, v0, c)) in firsts.iter().enumerate() {
            let on_pile = u() < pile * (i as f64 / n).sqrt();
            let pile_h = if on_pile {
                brick * (1.0 + 2.0 * (i as f64 / n)) * (0.5 + u())
            } else {
                0.0
            };
            let (tf, v) = fall(h - pile_h, v0);
            let t1 = at + t + tf;
            hits.push((t1, v, u8::from(on_pile), c));
            first_hits.push((t1, v, c));
            // Bounces: up at e v, down after 2 e v / g; a tumbling brick
            // loses more than the ideal (the 0.8).
            let (mut tb, mut vb) = (t1, v);
            for _ in 0..bounces {
                vb *= e * (0.8 + 0.4 * u());
                if vb < 0.25 {
                    break;
                }
                tb += 2.0 * vb / G;
                hits.push((tb, vb, 2, c));
            }
        }
        let v_ref = (2.0 * G * hw).sqrt().max(1e-6);
        let len = cx.n as f64 / f64::from(cx.sr);
        let pan_width = self.pan_width.unwrap_or(1.0);
        if !(0.0..=1.0).contains(&pan_width) {
            return Err(format!("pan_width: {pan_width}: give 0..1"));
        }
        let distance = match self.distance {
            Some(d) => Some(positive("distance", d)?),
            None => None,
        };
        let wall_w = cols as f64 * width;
        let extra = |c: usize| {
            let x = ((c as f64 + 0.5) * width - wall_w / 2.0) as f32;
            let side = if cols > 1 {
                (c as f32 + 0.5) / cols as f32 * 2.0 - 1.0
            } else {
                0.0
            };
            EventExtra {
                size: Some(brick as f32),
                pan: Some(side * pan_width),
                pos: distance.map(|z| [x, 0.0, z]),
                ..Default::default()
            }
        };
        let to_events = |mut v: Vec<(f64, f64, usize)>| -> Buf {
            v.retain(|(t, _, _)| *t >= 0.0 && *t < len);
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            Buf::Events(
                v.into_iter()
                    .enumerate()
                    .map(|(i, (t, s, c))| Event {
                        t,
                        value: (s / v_ref).clamp(0.0, 1.0) as f32,
                        id: i as u32,
                        extra: extra(c),
                    })
                    .collect(),
            )
        };
        let port = |k: u8| -> Vec<(f64, f64, usize)> {
            hits.iter()
                .filter(|h| h.2 == k)
                .map(|h| (h.0, h.1, h.3))
                .collect()
        };
        let mut heroes = first_hits.clone();
        heroes.sort_by(|a, b| b.1.total_cmp(&a.1));
        heroes.truncate(self.hero.unwrap_or(0));
        Ok(vec![
            (
                "out",
                to_events(hits.iter().map(|h| (h.0, h.1, h.3)).collect()),
            ),
            ("ground", to_events(port(0))),
            ("pile", to_events(port(1))),
            ("bounce", to_events(port(2))),
            ("hero", to_events(heroes)),
        ])
    }
}

/// Most events one `debris` may take.
const MAX_DEBRIS_EVENTS: usize = 200_000;

/// The excitation port `debris` hands its banks.
const DEBRIS_EXC: &str = "__debris_excite";

/// Many objects through a few shared resonators (stochastic modal synthesis,
/// after Cook's PhISEM): each event is a Hertz contact pulse (shorter and
/// brighter the faster it hits, louder as speed^1.2) sent to one of `banks`
/// `modal` bodies, each a slightly different size; a shared `ground` body
/// takes the floor hits; a `bed` follows the event density. 100 or 10,000
/// hits cost the same few modal rings, and they fuse as a real pile does.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Debris {
    /// The hits (a collapse, shatter, poisson, pattern...): value = speed 0..1.
    pub events: In,
    /// The objects' material (default "stone"), as `modal`.
    #[serde(default)]
    pub material: Option<serde_json::Value>,
    /// Size of one object, metres, as `modal` (give it or `f0`).
    #[serde(default)]
    pub size: Option<f32>,
    /// Lowest mode of one object, Hz (default 900 when `size` is not given:
    /// a brick).
    #[serde(default)]
    pub f0: Option<f32>,
    /// `modal` shape of the objects (default plate).
    #[serde(default)]
    pub shape: Option<serde_json::Value>,
    /// How many shared bodies (default 6, 1..16).
    #[serde(default)]
    pub banks: Option<usize>,
    /// Size spread across the banks, 0..1 (default 0.3: +-15 %).
    #[serde(default)]
    pub spread: Option<f32>,
    /// `modal` noise on the banks (default 0.6: atonal).
    #[serde(default)]
    pub noise: Option<f32>,
    /// `modal` loss on the banks (default 2).
    #[serde(default)]
    pub loss: Option<f32>,
    /// `modal` tilt on the banks (default 2).
    #[serde(default)]
    pub tilt: Option<f32>,
    /// Contact time of a full-speed hit, ms (default 0.6, 0.05..20); a hit at
    /// speed v lasts this x v^-0.2.
    #[serde(default)]
    pub contact_ms: Option<f32>,
    /// Hits on the floor (default none): rings `ground_material` too.
    #[serde(default)]
    pub ground: Option<In>,
    /// The floor's material (default "earth").
    #[serde(default)]
    pub ground_material: Option<serde_json::Value>,
    /// The floor body's size, metres (default 0.8).
    #[serde(default)]
    pub ground_size: Option<f32>,
    /// Level of the floor body, dB (default -10).
    #[serde(default)]
    pub ground_db: Option<f32>,
    /// Level of the density rumble, dB (default none: off): low noise whose
    /// level follows the smoothed energy of the hits.
    #[serde(default)]
    pub bed_db: Option<f32>,
    /// Output level, dB (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Distance at which an event with a `distance` plays at full level,
    /// metres (default 1). Farther hits are quieter (ref / d) and softer
    /// (a longer contact pulse: a gentle high cut); events without a
    /// distance are untouched.
    #[serde(default)]
    pub distance_ref: Option<f32>,
    /// Stereo output panned per event (default false: mono). Only takes
    /// effect when the events carry a pan or a position; costs two rings
    /// per bank.
    #[serde(default)]
    pub stereo: bool,
    /// Size of the event, 0..1 (default none: a pile of bricks, unchanged).
    /// 1 is a building or a hillside: the bodies are 10x lower, contact
    /// pulses 5x longer, the floor body 10x bigger, and the `bed` is a long
    /// (about 1 s) rumble at 30 Hz with infrasonic weight at 15 Hz.
    #[serde(default)]
    pub scale: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Debris {
    const BLURB: &'static str = "many hits through a few shared modal bodies (PhISEM-style): contact pulses by speed, a shared floor, a density bed; for a collapsing wall, a rockfall, a crowd of objects";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "material": "stone", "f0": 900, "banks": 6, "bed_db": -12})
    }
}

/// Adds one Hertz contact pulse (a half sine, roughened by noise) at `start`.
fn add_pulse(x: &mut [f32], start: usize, len: usize, amp: f32, rng: &mut Rng) {
    // The roughness is smooth over the pulse (its 2nd and 3rd harmonics), so
    // the spectrum still ends near 3 / contact time: a fast hit is brighter.
    let rough = (0.4 * (rng.unit() - 0.5), 0.4 * (rng.unit() - 0.5));
    write_pulse(x, start, len, amp, rough);
}

/// One pulse with a drawn roughness (so a stereo pair shares it).
fn write_pulse(x: &mut [f32], start: usize, len: usize, amp: f32, (a, b): (f32, f32)) {
    for k in 0..len {
        let Some(o) = x.get_mut(start + k) else { break };
        let p = std::f32::consts::PI * (k as f32 + 0.5) / len as f32;
        *o += amp * p.sin() * (1.0 + a * (2.0 * p).sin() + b * (3.0 * p).sin());
    }
}

/// Rings `exc` through a `modal` built from `body` (the `excite` filled in).
fn ring_modal(cx: &Cx, mut body: serde_json::Value, exc: Vec<f32>) -> Result<Vec<f32>, String> {
    body["excite"] = serde_json::Value::String(DEBRIS_EXC.into());
    let modal: crate::nodes::Modal =
        serde_json::from_value(body).map_err(|e| format!("bank: {e}"))?;
    let exc = std::sync::Arc::new(Buf::Audio(exc));
    let get = |i: &In| -> Result<crate::buf::Sig, String> {
        match i {
            In::Name(n) if n == DEBRIS_EXC => Ok(crate::buf::Sig::Buf(exc.clone())),
            other => (cx.get)(other),
        }
    };
    let sub = Cx {
        sr: cx.sr,
        n: cx.n,
        seed: cx.seed,
        looped: cx.looped,
        base_dir: cx.base_dir,
        sources: cx.sources,
        params: cx.params,
        sounds: cx.sounds,
        stack: cx.stack,
        get: &get,
    };
    match modal.eval(&sub)?.into_iter().next() {
        Some((_, Buf::Audio(v))) => Ok(v),
        _ => Err("bank: modal gave no mono output".into()),
    }
}

fn material_json(m: &Option<serde_json::Value>, default: &str) -> serde_json::Value {
    m.clone().unwrap_or_else(|| serde_json::json!(default))
}

impl Eval for Debris {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        if events.len() > MAX_DEBRIS_EVENTS {
            return Err(format!(
                "events: {}: at most {MAX_DEBRIS_EVENTS}",
                events.len()
            ));
        }
        let banks = self.banks.unwrap_or(6);
        if !(1..=16).contains(&banks) {
            return Err(format!("banks: {banks}: give 1..16"));
        }
        let (key, base) = match (self.size, self.f0) {
            (Some(_), Some(_)) => return Err("give size or f0, not both".into()),
            (Some(s), None) => ("size", positive("size", s)?),
            (None, f) => ("f0", positive("f0", f.unwrap_or(900.0))?),
        };
        let spread = self.spread.unwrap_or(0.3);
        if !(0.0..=1.0).contains(&spread) {
            return Err(format!("spread: {spread}: give 0..1"));
        }
        let scale = f64::from(self.scale.unwrap_or(0.0));
        if !(0.0..=1.0).contains(&scale) {
            return Err(format!("scale: {scale}: give 0..1"));
        }
        let scale = scale as f32;
        let size_k = 1.0 + 9.0 * scale;
        let contact = positive("contact_ms", self.contact_ms.unwrap_or(0.6))? * (1.0 + 4.0 * scale);
        if !(0.05..=100.0).contains(&contact) {
            return Err(format!("contact_ms: {contact}: give 0.05..20"));
        }
        let mut rng = cx.rng(self.seed);
        let pulse_len = |v: f32| -> usize {
            ((contact * 1e-3 * v.max(0.02).powf(-0.2) * cx.sr).round() as usize).max(2)
        };
        let place = |t: f64| (t.max(0.0) * f64::from(cx.sr)).round() as usize;
        let d_ref = positive("distance_ref", self.distance_ref.unwrap_or(1.0))?;
        let stereo = self.stereo && events.iter().any(|e| e.extra.pan().is_some());
        let mut exc = vec![vec![0.0f32; cx.n]; banks];
        let mut exc_r = if stereo {
            vec![vec![0.0f32; cx.n]; banks]
        } else {
            Vec::new()
        };
        let mut energy = vec![0.0f32; cx.n];
        for e in &events {
            let b = (rng.unit() * banks as f32) as usize % banks;
            let v = e.value.clamp(0.0, 1.0);
            let mut amp = v.powf(1.2);
            let mut len = pulse_len(v);
            if let Some(d) = e.extra.distance() {
                // Level falls as 1 / d (at most +12 dB closer than d_ref);
                // past d_ref the pulse lengthens by half per doubling, which
                // lowers its upper edge: air and grazing angles.
                let d = d.max(d_ref * 0.25);
                amp *= d_ref / d;
                if d > d_ref {
                    len = (len as f32 * (1.0 + 0.5 * (d / d_ref).log2())).round() as usize;
                }
            }
            let start = place(e.t);
            let rough = (0.4 * (rng.unit() - 0.5), 0.4 * (rng.unit() - 0.5));
            if stereo {
                // Constant-power pan; no pan is the middle.
                let th = (e.extra.pan().unwrap_or(0.0).clamp(-1.0, 1.0) + 1.0)
                    * std::f32::consts::FRAC_PI_4;
                write_pulse(&mut exc[b], start, len, amp * th.cos(), rough);
                write_pulse(&mut exc_r[b], start, len, amp * th.sin(), rough);
            } else {
                write_pulse(&mut exc[b], start, len, amp, rough);
            }
            if let Some(o) = energy.get_mut(start) {
                *o += amp * amp;
            }
        }
        let shape = self
            .shape
            .clone()
            .unwrap_or_else(|| serde_json::json!("plate"));
        let material = material_json(&self.material, "stone");
        let mut out = vec![0.0f32; cx.n];
        let mut out_r = if stereo {
            vec![0.0f32; cx.n]
        } else {
            Vec::new()
        };
        let mut exc_r = exc_r.into_iter();
        for (k, x) in exc.into_iter().enumerate() {
            let xr = exc_r.next();
            if x.iter().all(|v| *v == 0.0)
                && xr.as_ref().is_none_or(|r| r.iter().all(|v| *v == 0.0))
            {
                continue;
            }
            let off = if banks > 1 {
                k as f32 / (banks - 1) as f32 - 0.5
            } else {
                0.0
            };
            let body = serde_json::json!({
                "material": material, "shape": shape,

                "noise": self.noise.unwrap_or(0.6), "loss": self.loss.unwrap_or(2.0),
                "tilt": self.tilt.unwrap_or(2.0), "seed": self.seed.wrapping_add(k as u64 * 7919),
            });
            let mut body = body;
            let lowered = if key == "f0" { 1.0 / size_k } else { size_k };
            body[key] = serde_json::json!(base * (1.0 + spread * off) * lowered);
            if let Some(xr) = xr {
                for (o, y) in out_r.iter_mut().zip(ring_modal(cx, body.clone(), xr)?) {
                    *o += y;
                }
            }
            for (o, y) in out.iter_mut().zip(ring_modal(cx, body, x)?) {
                *o += y;
            }
        }
        let mono_banks_l = if stereo { out.clone() } else { Vec::new() };
        if let Some(g) = &self.ground {
            let gev = cx.events("ground", g)?;
            let mut x = vec![0.0f32; cx.n];
            for e in &gev {
                let v = e.value.clamp(0.0, 1.0);
                add_pulse(&mut x, place(e.t), pulse_len(v) * 3, v.powf(1.2), &mut rng);
            }
            let body = serde_json::json!({
                "material": material_json(&self.ground_material, "earth"),
                "size": positive("ground_size", self.ground_size.unwrap_or(0.8))? * size_k,
                "noise": 0.8, "loss": 1.5, "seed": self.seed ^ 0x0067_0001,
            });
            let gain = 10f32.powf(self.ground_db.unwrap_or(-10.0) / 20.0);
            for (o, y) in out.iter_mut().zip(ring_modal(cx, body, x)?) {
                *o += y * gain;
            }
        }
        if let Some(db) = self.bed_db {
            // Energy smoothed over ~60 ms -> amplitude -> low noise (two
            // one-poles at 300 Hz).
            let a_env = (-1.0 / (0.06 * (1.0 + 15.0 * scale) * cx.sr)).exp();
            let fc = 300.0 / size_k;
            let a_lp = (-2.0 * std::f32::consts::PI * fc / cx.sr).exp();
            let a_sub = (-2.0 * std::f32::consts::PI * 15.0 / cx.sr).exp();
            let (mut env, mut l1, mut l2, mut s1, mut s2) =
                (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            // A lower cutoff passes less noise: sqrt(k) puts the level back.
            let gain = 10f32.powf(db / 20.0) * 4.0 * size_k.sqrt();
            for (o, en) in out.iter_mut().zip(&energy) {
                env = en * (1.0 - a_env) * 200.0 + env * a_env;
                let w = rng.unit() * 2.0 - 1.0;
                l1 = w * (1.0 - a_lp) + l1 * a_lp;
                l2 = l1 * (1.0 - a_lp) + l2 * a_lp;
                *o += l2 * env.sqrt() * gain;
                if scale > 0.0 {
                    // Infrasonic weight: the same envelope through two
                    // poles at 15 Hz, its own noise.
                    let w = rng.unit() * 2.0 - 1.0;
                    s1 = w * (1.0 - a_sub) + s1 * a_sub;
                    s2 = s1 * (1.0 - a_sub) + s2 * a_sub;
                    *o += s2 * env.sqrt() * gain * 2.0 * scale;
                }
            }
        }
        let g = 10f32.powf(self.gain_db.unwrap_or(0.0) / 20.0);
        if stereo {
            // `out` holds the left banks plus the shared floor and bed;
            // the floor and bed sit in the middle (-3 dB each side).
            let centre: Vec<f32> = out.iter().zip(&mono_banks_l).map(|(o, l)| o - l).collect();
            let h = std::f32::consts::FRAC_1_SQRT_2;
            let l: Vec<f32> = mono_banks_l
                .iter()
                .zip(&centre)
                .map(|(l, c)| (l + c * h) * g)
                .collect();
            let r: Vec<f32> = out_r
                .iter()
                .zip(&centre)
                .map(|(r, c)| (r + c * h) * g)
                .collect();
            return Ok(one(Buf::from_channels(vec![l, r])));
        }
        if g != 1.0 {
            out.iter_mut().for_each(|v| *v *= g);
        }
        Ok(one(Buf::Audio(out)))
    }
}

/// Most events one `cascade` may make.
const MAX_CASCADE: usize = 200_000;

/// A mass failing over seconds: a landslide, an avalanche, a rockfall down a
/// slope, a building giving way floor by floor. Where `collapse` times each
/// brick from its own fall, `cascade` gives the release an envelope: the
/// event rate and the speed both follow a surge that builds, peaks and runs
/// out over `duration`. Feed it to `debris` (with `scale`).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cascade {
    /// How many events (default 2000, 1..200000).
    #[serde(default)]
    pub count: Option<usize>,
    /// Seconds from the first movement to the last (default 6).
    #[serde(default)]
    pub duration: Option<f32>,
    /// Where the surge peaks, 0..1 of the duration (default 0.3: a fast
    /// start and a long run-out).
    #[serde(default)]
    pub surge: Option<f32>,
    /// Failures in sequence, 1..32 (default 1): above 1 the surge is that
    /// many overlapping pulses, each heavier than the last, one per floor.
    #[serde(default)]
    pub floors: Option<usize>,
    /// Event value at the peak of the surge, 0..1 (default 0.8); the tails
    /// run at a quarter of it, each hit scattered by `looseness`.
    #[serde(default)]
    pub peak: Option<f32>,
    /// Scatter of the values, 0..1 (default 0.5).
    #[serde(default)]
    pub looseness: Option<f32>,
    /// The N strongest events also go on the `hero` port (default 0).
    #[serde(default)]
    pub hero: Option<usize>,
    /// Time the mass starts to go, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Cascade {
    const BLURB: &'static str = "a large mass failing over seconds (avalanche, rockfall, a building floor by floor): event rate and value follow a surge envelope; ports hero; drives debris with `scale`";
    fn example() -> serde_json::Value {
        serde_json::json!({"count": 2000, "duration": 6, "surge": 0.3, "floors": 1})
    }
}

/// The surge shape over x in 0..1: a skewed bump peaking at `p`, 0 at the ends.
fn surge_bump(x: f64, p: f64) -> f64 {
    if !(0.0..1.0).contains(&x) {
        return 0.0;
    }
    let (a, b) = (4.0 * p.max(0.05), 4.0 * (1.0 - p).max(0.05));
    let peak = (a / (a + b)).powf(a) * (b / (a + b)).powf(b);
    x.powf(a) * (1.0 - x).powf(b) / peak
}

impl Eval for Cascade {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let count = self.count.unwrap_or(2000);
        if count == 0 || count > MAX_CASCADE {
            return Err(format!("count: {count}: give 1..{MAX_CASCADE}"));
        }
        let dur = f64::from(positive("duration", self.duration.unwrap_or(6.0))?);
        let p = f64::from(self.surge.unwrap_or(0.3));
        if !(0.0..=1.0).contains(&p) {
            return Err(format!("surge: {p}: give 0..1"));
        }
        let floors = self.floors.unwrap_or(1);
        if !(1..=32).contains(&floors) {
            return Err(format!("floors: {floors}: give 1..32"));
        }
        let peak = f64::from(self.peak.unwrap_or(0.8));
        let loose = f64::from(self.looseness.unwrap_or(0.5));
        if !(0.0..=1.0).contains(&peak) || !(0.0..=1.0).contains(&loose) {
            return Err("peak and looseness: give 0..1".into());
        }
        let at = f64::from(non_negative("at", self.at.unwrap_or(0.0))?);
        // The envelope: one surge, or `floors` of them, each a window of
        // width w that starts later and weighs more than the one before.
        let w = 1.0 / (1.0 + 0.5 * (floors as f64 - 1.0));
        let env = |x: f64| -> f64 {
            if floors == 1 {
                return surge_bump(x, p);
            }
            (0..floors)
                .map(|f| {
                    let start = f as f64 * (1.0 - w) / (floors as f64 - 1.0);
                    (0.5 + f as f64 / floors as f64) * surge_bump((x - start) / w, p)
                })
                .sum()
        };
        const N: usize = 2048;
        let table: Vec<f64> = (0..N).map(|i| env((i as f64 + 0.5) / N as f64)).collect();
        let top = table.iter().copied().fold(1e-9, f64::max);
        let mut cdf = Vec::with_capacity(N);
        let mut acc = 0.0;
        for v in &table {
            acc += v + 1e-4 * top;
            cdf.push(acc);
        }
        let mut rng = cx.rng(self.seed);
        let mut hits: Vec<(f64, f32)> = Vec::with_capacity(count);
        for _ in 0..count {
            let u = f64::from(rng.unit()) * acc;
            let i = cdf.partition_point(|c| *c < u).min(N - 1);
            let lo = if i == 0 { 0.0 } else { cdf[i - 1] };
            let x = (i as f64 + (u - lo) / (cdf[i] - lo).max(1e-12)) / N as f64;
            let level = (table[i] / top).clamp(0.0, 1.0);
            let base = peak * (0.25 + 0.75 * level);
            let v = base * (1.0 - loose * 0.8 * f64::from(rng.unit()));
            hits.push((at + x * dur, v as f32));
        }
        let len = cx.n as f64 / f64::from(cx.sr);
        hits.retain(|(t, _)| *t >= 0.0 && *t < len);
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let to_events = |v: &[(f64, f32)]| {
            Buf::Events(
                v.iter()
                    .enumerate()
                    .map(|(i, &(t, value))| Event {
                        t,
                        value,
                        id: i as u32,
                        ..Default::default()
                    })
                    .collect(),
            )
        };
        let mut heroes = hits.clone();
        heroes.sort_by(|a, b| b.1.total_cmp(&a.1));
        heroes.truncate(self.hero.unwrap_or(0));
        heroes.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(vec![
            ("out", to_events(&hits)),
            ("hero", to_events(&heroes)),
        ])
    }
}
