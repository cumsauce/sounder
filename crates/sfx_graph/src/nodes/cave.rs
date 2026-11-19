//! `lava` and `cave`, after the `Lava` and `Cave` beds of the foley project
//! (the foley repository, `foley-dsp/src/lava.rs` and `cave.rs`, MIT /
//! Apache-2.0). Lava is a low convection rumble under bubbles of gas
//! bursting at the surface (a Poisson process), each ringing as the
//! Helmholtz resonator its cavity makes, heavily damped by the melt (after
//! Vergniolle and Brandeis's Strombolian bubbles). The cave bed is a
//! handful of ceiling voices dripping (800 Hz to 1.4 kHz, 30 ms rings), a
//! sub-bass rumble that tracks the depth and the stone overhead, and the
//! rare settling stone, fading as the cave opens to the sky. The filters,
//! the Helmholtz formula and the wobble are foley-dsp's own.

use std::f32::consts::{PI, TAU};

use foley_dsp::{BandPass, Bubble, LowPass, Wobble, helmholtz};
use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// Bubbles bursting a second from one square metre of lava.
const BURSTS: f32 = 0.35;
const MAX_BURSTS: f32 = 30.0;
const MAX_VOICES: usize = 32;
/// The ringing folded back onto the start of a loop, s.
const WRAP_TAIL: f32 = 0.5;

fn check(name: &str, v: f32, lo: f32, hi: f32, what: &str) -> Result<(), String> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(format!("{name}: {v}: give {what} from {lo} to {hi}"))
    }
}

/// Fold the last samples of `out` (past `n`) onto its start (an equal-power
/// crossfade) and drop them, so the buffer wraps with no seam.
fn wrap(out: &mut Vec<f32>, n: usize) {
    let tail = out.len() - n;
    for j in 0..tail.min(n) {
        let w = j as f32 / tail as f32;
        let t = out[n + j];
        out[j] = out[j] * w.sqrt() + t * (1.0 - w).sqrt();
    }
    out.truncate(n);
}

/// Molten rock: a rumble and bursting, ringing bubbles.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lava {
    /// The open surface, m2, 0.1 to 1000 (default 20). Sets the rumble
    /// (2.5 x sqrt(size), up to 10 x) and the bursts (0.35 a second a m2,
    /// at most 30).
    #[serde(default)]
    pub size: Option<f32>,
    /// A scale on the burst rate, 0 to 4 (default 1): 0 is a rumble alone,
    /// 3 a boiling pool.
    #[serde(default)]
    pub activity: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Lava {
    const BLURB: &'static str = "lava (after the foley project): a 60 Hz convection rumble and Poisson bubbles bursting, each ringing as a Helmholtz cavity damped by the melt; size m2, activity (burst rate scale); loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["lava", "magma", "bubbles"];
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 20, "activity": 1, "seed": 1})
    }
}

impl Eval for Lava {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = self.size.unwrap_or(20.0);
        check("size", size, 0.1, 1000.0, "square metres")?;
        let activity = self.activity.unwrap_or(1.0);
        check("activity", activity, 0.0, 4.0, "a burst-rate scale")?;
        let wrapped = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        Ok(one(Buf::Audio(render_lava(
            &mut rng, cx.n, cx.sr, size, activity, wrapped,
        ))))
    }
}

/// One bursting bubble: the cavity rings as a Helmholtz resonator (a mode)
/// under a low thump, with a sizzle of spatter.
struct Burst {
    /// Each mode as a phasor (re, im, cos, sin) shrunk by its decay.
    modes: [[f32; 4]; 2],
    sizzle: f32,
    sizzle_step: f32,
}

impl Burst {
    fn new(rng: &mut Rng, sr: f32) -> Burst {
        // Bubble radius, m: 1 to 6 cm, mostly small.
        let radius = 0.01 * 6f32.powf(rng.unit().powf(1.5));
        let opening = radius * (0.3 + 0.4 * rng.unit());
        let (frequency, q) = helmholtz(radius, opening);
        // Lava's viscosity damps it further than radiation alone.
        let decay = PI * frequency / (0.5 * q).clamp(1.5, 10.0);
        let amplitude = 0.25 * radius / 0.03;
        let mode = |f: f32, decay: f32, a: f32| {
            let shrink = (-decay / sr).exp();
            let step = TAU * f.min(0.45 * sr) / sr;
            [a, 0.0, shrink * step.cos(), shrink * step.sin()]
        };
        Burst {
            modes: [
                mode(frequency, decay, amplitude),
                mode(0.3 * frequency, 2.0 * decay, 1.5 * amplitude),
            ],
            sizzle: 0.15 * amplitude,
            sizzle_step: (-1.0 / (0.04 * sr)).exp(),
        }
    }

    fn sample(&mut self, noise: f32) -> f32 {
        let mut out = self.sizzle * noise;
        self.sizzle *= self.sizzle_step;
        for [re, im, c, s] in &mut self.modes {
            let (r, i) = (*re * *c - *im * *s, *re * *s + *im * *c);
            *re = r;
            *im = i;
            out += i;
        }
        out
    }

    fn done(&self) -> bool {
        self.sizzle < 1e-4 && self.modes.iter().all(|m| m[0].abs() + m[1].abs() < 1e-4)
    }
}

fn render_lava(
    rng: &mut Rng,
    n: usize,
    sr: f32,
    size: f32,
    activity: f32,
    looped: bool,
) -> Vec<f32> {
    let total = n + if looped { (WRAP_TAIL * sr) as usize } else { 0 };
    let dt = 1.0 / sr;
    let rate = (BURSTS * size * activity).min(MAX_BURSTS);
    let rumble_level = 2.5 * size.sqrt().min(10.0);
    let mut rumble = [
        LowPass::new_at(60.0, sr as u32),
        LowPass::new_at(60.0, sr as u32),
    ];
    // Let the rumble filters settle before sample 0.
    for _ in 0..(0.1 * sr) as usize {
        let first = rumble[0].process(rng.signed());
        rumble[1].process(first);
    }
    let mut until = 0.0f32;
    let mut bursts: Vec<Burst> = Vec::with_capacity(MAX_VOICES);
    let mut out = vec![0.0f32; total];
    for (i, o) in out.iter_mut().enumerate() {
        if rate > 0.0 {
            until -= dt;
            while until <= 0.0 {
                until += rng.exponential(rate);
                if bursts.len() < MAX_VOICES {
                    bursts.push(Burst::new(rng, sr));
                }
            }
        }
        let noise = rng.signed();
        let first = rumble[0].process(noise);
        let mut mix = rumble_level * rumble[1].process(first);
        for b in &mut bursts {
            mix += b.sample(noise);
        }
        if i % 64 == 63 {
            bursts.retain(|b| !b.done());
        }
        *o = mix;
    }
    if looped {
        wrap(&mut out, n);
    }
    out
}

/// A cave: drips from a stone roof and a sub-bass rumble.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cave {
    /// How deep the ear is, 0..1 (default 1; 1 = 60 m down). The rumble is
    /// 0.15 x depth x mass; settling stones need drips + depth over 0.5.
    #[serde(default)]
    pub depth: Option<f32>,
    /// How much stone lies overhead, 0..1 (default 1).
    #[serde(default)]
    pub mass: Option<f32>,
    /// How wet the roof is, 0..1 (default 0.5): 0 none, 1 five drips a
    /// second shared by five voices.
    #[serde(default)]
    pub drips: Option<f32>,
    /// How open to the sky, 0..1 (default 0 = shut): the whole bed is
    /// multiplied by 1 - openness, so 1 is silent.
    #[serde(default)]
    pub openness: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Cave {
    const BLURB: &'static str = "cave bed: drips (0.4-4 a second) into pools (Minnaert bubbles 0.7-2.2 kHz, rising) and on wet rock, in a long dark echo sized by depth (1.5-4 s), a sub-bass rumble from depth x mass, rare settling stones; depth, mass, drips, openness (fades the bed); loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["cave", "drip", "drips"];
    fn example() -> serde_json::Value {
        serde_json::json!({"depth": 1, "mass": 1, "drips": 0.5, "seed": 1})
    }
}

impl Eval for Cave {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let depth = self.depth.unwrap_or(1.0);
        let mass = self.mass.unwrap_or(1.0);
        let drips = self.drips.unwrap_or(0.5);
        let open = self.openness.unwrap_or(0.0);
        check("depth", depth, 0.0, 1.0, "a fraction")?;
        check("mass", mass, 0.0, 1.0, "a fraction")?;
        check("drips", drips, 0.0, 1.0, "a fraction")?;
        check("openness", open, 0.0, 1.0, "a fraction")?;
        let wrapped = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        Ok(one(Buf::Audio(render_cave(
            &mut rng,
            cx.n,
            cx.sr,
            [depth, mass, drips, open],
            wrapped,
        ))))
    }
}

/// Drips a second at `drips` 0 and 1 (a Poisson process): a few, not a buzz.
const DRIP_RATE: [f32; 2] = [0.4, 4.0];
/// Pool drops ring as Minnaert bubbles 1.5 to 5 mm across (2.2 kHz to 660 Hz).
const DROP_MM: [f32; 2] = [1.5, 5.0];
/// Share of drips that land in a pool; the rest tick on wet rock.
const POOL: f32 = 0.7;
const RUMBLE_HZ: f32 = 55.0;
/// Settling stones, a second, in a deep wet cave.
const STONE_RATE: f32 = 0.04;
const STONE_GAIN: f32 = 0.05;
const STONE_MS: f32 = 80.0;
/// Seconds rendered before sample 0, so the cave's echo is already full.
const PRE_ROLL: f32 = 2.0;

struct Stone {
    envelope: f32,
    phase: f32,
    hz: f32,
}

/// The dry cave: drips into pools and on rock, the rumble and the stones.
fn cave_dry(rng: &mut Rng, total: usize, sr: f32, depth: f32, mass: f32, drips: f32) -> Vec<f32> {
    let dt = 1.0 / sr;
    let sri = sr as u32;
    let mut rumble = LowPass::new_at(RUMBLE_HZ, sri);
    let mut lfo = Wobble::new_at(0.3, 0.6, 0.5, 1.5, sri);
    let rumble_level = 0.15 * depth * mass;
    let drip_rate = if drips > 0.0 {
        DRIP_RATE[0] + (DRIP_RATE[1] - DRIP_RATE[0]) * drips
    } else {
        0.0
    };
    let stones_active = drips > 0.0 && drips + depth > 0.5;
    let stone_attack = 1.0 - (-1.0 / (0.005 * sr)).exp();
    let stone_decay = (-1.0 / (STONE_MS * 0.001 * sr * 0.3)).exp();
    let tick_decay = (-1.0 / (0.0015 * sr)).exp();
    let mut tick_bp = BandPass::new_at(3200.0, 1.2, sri);
    let mut tick = 0.0f32;
    let mut pools: Vec<(Bubble, f32)> = Vec::new();
    let mut until = if drip_rate > 0.0 {
        rng.exponential(drip_rate)
    } else {
        f32::MAX
    };
    let mut stone: Option<Stone> = None;
    let mut stone_wait = rng.exponential(STONE_RATE);
    let mut out = vec![0.0f32; total];
    for o in out.iter_mut() {
        until -= dt;
        while until <= 0.0 {
            until += rng.exponential(drip_rate);
            // Near drops are loud, far ones soft: the cave's depth of field.
            let level = 0.25 + 0.75 * rng.unit() * rng.unit();
            if rng.unit() < POOL {
                let mm = DROP_MM[0] * (DROP_MM[1] / DROP_MM[0]).powf(rng.unit());
                let rise = 0.05 + 0.25 * rng.unit();
                pools.push((Bubble::new_at(mm * 1e-3, 0.3, sri).with_rise(rise), level));
                tick += 0.3 * level;
            } else {
                tick += level;
            }
        }
        let noise = rng.signed();
        let rum = rumble_level * lfo.next(rng.signed()) * rumble.process(noise);
        let mut sum = 0.0f32;
        for (b, g) in &mut pools {
            sum += *g * b.sample();
        }
        pools.retain(|(b, _)| !b.done());
        sum += 0.5 * tick_bp.process(tick * noise);
        tick *= tick_decay;
        let mut stone_mix = 0.0;
        if stones_active {
            stone_wait -= dt;
            if stone_wait <= 0.0 {
                stone_wait = rng.exponential(STONE_RATE);
                stone = Some(Stone {
                    envelope: 0.0,
                    phase: 0.0,
                    hz: 120.0 + 80.0 * rng.unit(),
                });
            }
            if let Some(s) = &mut stone {
                s.envelope += stone_attack * (1.0 - s.envelope);
                s.phase = (s.phase + TAU * s.hz / sr) % TAU;
                stone_mix = STONE_GAIN * s.envelope * s.phase.sin().abs();
                s.envelope *= stone_decay;
                if s.envelope < 1e-4 {
                    stone = None;
                }
            }
        }
        *o = rum + sum + stone_mix;
    }
    out
}

/// A cave is its echo: the drips go through a long, dark feedback delay
/// network sized by the depth (1.5 s of ring near the surface, 4 s deep).
fn render_cave(rng: &mut Rng, n: usize, sr: f32, scene: [f32; 4], looped: bool) -> Vec<f32> {
    let [depth, mass, drips, openness] = scene;
    let pre = (PRE_ROLL * sr) as usize;
    let total = pre + n + if looped { (WRAP_TAIL * sr) as usize } else { 0 };
    let dry = cave_dry(rng, total, sr, depth, mass, drips);
    let size = 0.5 + 0.5 * depth;
    let rt = 1.5 + 2.5 * depth;
    let wet = crate::nodes::time::fdn_tail(&dry, 0, size, rt, 0.55, sr);
    let fade = (1.0 - openness).clamp(0.0, 1.0);
    // An opening lets the echo out first.
    let wet_gain = 0.9 * fade;
    let mut out: Vec<f32> = dry
        .iter()
        .zip(&wet)
        .skip(pre)
        .map(|(d, w)| ((d * fade + wet_gain * 0.5 * (w[0] + w[1])) * fade).tanh())
        .collect();
    if looped {
        wrap(&mut out, n);
    }
    out
}
