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

use foley_dsp::{LowPass, Wobble, helmholtz};
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
    const BLURB: &'static str = "cave bed (after the foley project): five ceiling voices dripping 0.8-1.4 kHz on a Poisson clock, a sub-bass rumble from depth x mass, rare settling stones; depth, mass, drips, openness (fades the bed); loop: true wraps without a seam";
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

/// Drip band, Hz: the radial mode of an in-falling drop's cavity.
const DRIP_HZ_LO: f32 = 800.0;
const DRIP_HZ_HI: f32 = 1400.0;
const DRIP_MS: f32 = 30.0;
const VOICES: usize = 5;
const RUMBLE_HZ: f32 = 55.0;
/// Settling stones a second when the cave is wet and deep.
const STONE_RATE: f32 = 0.04;
const STONE_GAIN: f32 = 0.05;
const STONE_MS: f32 = 80.0;

struct Drip {
    hz: f32,
    pan: f32,
    until: f32,
    envelope: f32,
    phase: f32,
    firing: bool,
}

struct Stone {
    envelope: f32,
    phase: f32,
    hz: f32,
}

fn render_cave(rng: &mut Rng, n: usize, sr: f32, scene: [f32; 4], looped: bool) -> Vec<f32> {
    let [depth, mass, drips, openness] = scene;
    let total = n + if looped { (WRAP_TAIL * sr) as usize } else { 0 };
    let dt = 1.0 / sr;
    let attack = 1.0 - (-1.0 / (0.001 * sr)).exp();
    let decay = (-1.0 / (DRIP_MS * 0.001 * sr * 0.3)).exp();
    let mut voices: [Drip; VOICES] = [(); VOICES].map(|()| Drip {
        hz: DRIP_HZ_LO + (DRIP_HZ_HI - DRIP_HZ_LO) * rng.unit(),
        pan: 2.0 * rng.unit() - 1.0,
        until: 0.2 * rng.unit(),
        envelope: 0.0,
        phase: 0.0,
        firing: false,
    });
    let mut rumble = LowPass::new_at(RUMBLE_HZ, sr as u32);
    let mut lfo = Wobble::new_at(0.3, 0.6, 0.5, 1.5, sr as u32);
    let rumble_level = 0.15 * depth * mass;
    let drip_rate = 5.0 * drips * VOICES as f32;
    let stones_active = drips > 0.0 && drips + depth > 0.5;
    let fade = (1.0 - openness).clamp(0.0, 1.0);
    let stone_attack = 1.0 - (-1.0 / (0.005 * sr)).exp();
    let stone_decay = (-1.0 / (STONE_MS * 0.001 * sr * 0.3)).exp();
    let mut stone: Option<Stone> = None;
    let mut stone_wait = 0.0f32;
    let mut out = vec![0.0f32; total];
    // Foley warms the filters and the slow wobble for 4096 samples at 48 kHz.
    let warm = (4096.0 * sr / 48000.0) as usize;
    for i in 0..warm + total {
        let noise = rng.signed();
        let w = lfo.next(rng.signed());
        let rum = rumble_level * w * rumble.process(noise);
        let mut sum = 0.0f32;
        for v in &mut voices {
            v.until -= dt;
            if v.until <= 0.0 {
                if drip_rate > 0.0 {
                    v.until = rng.exponential(drip_rate);
                    v.envelope = 0.0;
                    v.phase = 0.0;
                    v.firing = true;
                } else {
                    v.until = 1.0;
                    v.firing = false;
                }
            }
            if v.firing {
                v.envelope += attack * (1.0 - v.envelope);
                v.phase = (v.phase + TAU * v.hz / sr) % TAU;
                sum += 0.4 * v.envelope * v.phase.sin() * (0.5 + 0.5 * v.pan);
                v.envelope *= decay;
                if v.envelope < 1e-3 {
                    v.firing = false;
                }
            }
        }
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
                // A half-rectified low sine: a rock click.
                stone_mix = STONE_GAIN * s.envelope * s.phase.sin().abs();
                s.envelope *= stone_decay;
                if s.envelope < 1e-4 {
                    stone = None;
                }
            }
        }
        if i >= warm {
            out[i - warm] = ((rum + sum + stone_mix) * fade).tanh();
        }
    }
    if looped {
        wrap(&mut out, n);
    }
    out
}
