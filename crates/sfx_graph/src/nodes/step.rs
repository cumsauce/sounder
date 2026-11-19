//! `step`: one footstep or a walk, after foley's `contact(Contact::Step, ..)`
//! (`foley-synth/src/modal.rs` of the foley project, MIT / Apache-2.0),
//! called as it stands. A heel of about 8 kg strikes the ground through a
//! Hertz contact (soft sole on the surface: the contact time sets how dull
//! it is); the surface's own plate modes ring under it; the sole scuffs as
//! the foot rolls; the ball of the foot lands 60-120 ms later, softer.
//! Gravel and sand crunch in grains, earth crumbles, grass swishes round the
//! shoe, snow crackles as its bonds break, a wooden floor now and then
//! creaks. Each step in `at` is its own seed. Added because the `footstep`
//! node on concrete, gravel and grass was heard as a snare, a pitter-patter
//! and a blow (labels batch 11).
//!
//! The contact code is copied here (same physics and constants) so that
//! what a listener heard can be changed: `grit` scales the grains and scuff
//! (gravel without its grit was called "perfect all-purpose footsteps"); the
//! ball of the foot rings 3 x shorter than the heel (on concrete it was
//! heard "too resonant, wood-y"); grass is soil (foley's earth, softer than
//! gravel: the gravel body was heard "too hard") with dull grains and the
//! blades crunching at the heel and toe (foley's thin plant plate was not
//! heard as grass).

use std::f32::consts::{PI, TAU};

use foley_dsp::{BandPass, LowPass};
use foley_synth::{Material, MaterialProps, hertz_time};
use serde::Deserialize;

use super::{OneOrMany, one};
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

const MAX_STEPS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepSurface {
    /// Concrete, stone, pavement (foley's stone: a 25 cm slab).
    #[serde(alias = "stone", alias = "pavement")]
    Concrete,
    /// Gravel and sand: grains crushed under the heel.
    #[serde(alias = "sand")]
    Gravel,
    /// Grass, leaves, plants: blades brushing the shoe.
    #[serde(alias = "leaves", alias = "plant")]
    Grass,
    /// Dirt, mud, clay: a dull thud that crumbles.
    #[serde(alias = "earth", alias = "mud")]
    Dirt,
    /// A floor of 2.5 cm boards.
    Wood,
    /// Steel plate.
    Metal,
    /// Ice.
    Ice,
    /// Settled snow: crunch and squeak.
    Snow,
    /// Carpet, wool, rubber matting.
    #[serde(alias = "soft")]
    Carpet,
}

impl StepSurface {
    fn material(self) -> Material {
        match self {
            StepSurface::Concrete => Material::Stone,
            StepSurface::Gravel => Material::Granular,
            StepSurface::Grass => Material::Earth,
            StepSurface::Dirt => Material::Earth,
            StepSurface::Wood => Material::Wood,
            StepSurface::Metal => Material::Metal,
            StepSurface::Ice => Material::Ice,
            StepSurface::Snow => Material::Snow,
            StepSurface::Carpet => Material::Soft,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepLayer {
    #[default]
    All,
    Body,
    Texture,
}

#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// concrete (default), gravel, grass, dirt, wood, metal, ice, snow, carpet.
    #[serde(default)]
    pub surface: Option<StepSurface>,
    /// Heel speed, m/s, 0.2..3 (default 1, a walk; 0.5 a careful step, 2 a
    /// run): faster is a shorter, brighter contact and more crunch.
    #[serde(default)]
    pub speed: Option<f32>,
    /// Seconds each step lands: one number or a list (default 0.005).
    #[serde(default)]
    pub at: Option<OneOrMany>,
    /// Grains and scuff, 0..2 (default 1): 0 leaves the bare heel and toe
    /// (an all-purpose step), 2 a crunchier one.
    #[serde(default)]
    pub grit: Option<f32>,
    /// Which part of the step: `all` (default), `body` (heel and toe, the
    /// surface's own ring, a board's creak) or `texture` (grains, scuff,
    /// blades, snow crackle). Same seed, so `body` + `texture` = `all`:
    /// a surface's texture can go on another step.
    #[serde(default)]
    pub layer: Option<StepLayer>,
    /// Level, dB (default 0; each step is foley's level: 0.4 peak on stone).
    #[serde(default)]
    pub gain_db: Option<f32>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Step {
    const BLURB: &'static str = "footsteps (after the foley project's contact model): a Hertz heel strike on the surface's own modes, the sole's scuff, the ball of the foot 60-120 ms later; gravel crunches, grass swishes, snow squeaks; surface, speed m/s, grit 0..2, layer all / body / texture (they add back), at (one time or a list: a walk), gain_db";
    const WORDS: &'static [&'static str] = &["surface", "layer"];
    fn example() -> serde_json::Value {
        serde_json::json!({"surface": "gravel", "speed": 1, "at": [0.005, 0.55], "seed": 1})
    }
}

impl Eval for Step {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = self.speed.unwrap_or(1.0);
        if !(speed.is_finite() && (0.2..=3.0).contains(&speed)) {
            return Err(format!(
                "speed: {speed}: give the heel speed in m/s from 0.2 to 3 (1 a walk, 2 a run)"
            ));
        }
        let times = self.at.as_ref().map_or(vec![0.005], |a| a.values());
        if times.len() > MAX_STEPS || times.iter().any(|t| !t.is_finite() || *t < 0.0) {
            return Err(format!(
                "at: give up to {MAX_STEPS} times in seconds, each 0 or more"
            ));
        }
        let grit = self.grit.unwrap_or(1.0);
        if !(grit.is_finite() && (0.0..=2.0).contains(&grit)) {
            return Err(format!(
                "grit: {grit}: give the grain and scuff level from 0 to 2"
            ));
        }
        let gain = 10f32.powf(self.gain_db.unwrap_or(0.0) / 20.0);
        let surface = self.surface.unwrap_or(StepSurface::Concrete);
        let sr = (cx.sr.round() as u32).max(1);
        let mut rng = cx.rng(self.seed);
        let mut out = vec![0.0f32; cx.n];
        for t in times {
            let mut srng = Rng::new(rng.next_u64());
            let s = footstep(
                &mut srng,
                surface,
                speed,
                grit,
                self.layer.unwrap_or_default(),
                sr,
            );
            let start = (t * cx.sr) as usize;
            for (o, x) in out.iter_mut().skip(start).zip(&s) {
                *o += gain * x;
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}

const SPEED_OF_SOUND: f32 = 343.0;
/// The ball of the foot's modes die this much faster than the heel's: the
/// foot is still on the surface, damping it.
const TOE_DAMP: f32 = 3.0;

fn wave_speed(p: &MaterialProps) -> f32 {
    (p.young / (p.density * (1.0 - p.poisson * p.poisson))).sqrt()
}

fn plate_mode(p: &MaterialProps, m: f32, n: f32) -> f32 {
    PI / 2.0 * p.thickness * wave_speed(p) / 12f32.sqrt() * (m * m + n * n)
}

fn coincidence(p: &MaterialProps) -> f32 {
    SPEED_OF_SOUND * SPEED_OF_SOUND / (1.8 * p.thickness * wave_speed(p))
}

fn effective_modulus(a: (f32, f32), b: (f32, f32)) -> f32 {
    1.0 / ((1.0 - a.1 * a.1) / a.0 + (1.0 - b.1 * b.1) / b.0)
}

fn half_sine_spectrum(f: f32, tau: f32) -> f32 {
    let x = 2.0 * f * tau;
    let d = 1.0 - x * x;
    if d.abs() < 1e-3 {
        PI / 4.0
    } else {
        ((PI * f * tau).cos() / d).abs()
    }
}

#[derive(Default)]
struct Modes(Vec<[f32; 4]>);

impl Modes {
    fn add(&mut self, f: f32, decay: f32, a: f32, sr: f32) {
        if f >= 0.45 * sr || a == 0.0 {
            return;
        }
        let shrink = (-decay / sr).exp();
        let step = TAU * f / sr;
        self.0
            .push([a, 0.0, shrink * step.cos(), shrink * step.sin()]);
    }

    fn render_into(&mut self, out: &mut Vec<f32>, start: usize, sr: f32) {
        let mut i = start;
        loop {
            let mut sample = 0.0;
            let mut alive = false;
            for [re, im, c, s] in &mut self.0 {
                let (r, j) = (*re * *c - *im * *s, *re * *s + *im * *c);
                *re = r;
                *im = j;
                sample += j;
                alive |= r.abs() + j.abs() > 1e-5;
            }
            if i >= out.len() {
                out.resize(i + 1, 0.0);
            }
            out[i] += sample;
            i += 1;
            if !alive || i - start > 4 * sr as usize {
                break;
            }
        }
    }
}

fn struck_modes(rng: &mut Rng, p: &MaterialProps, tau: f32, damp: f32, sr: f32) -> Modes {
    let (x0, y0) = (0.1 + 0.8 * rng.unit(), 0.1 + 0.8 * rng.unit());
    let fc = coincidence(p);
    let mut modes = Vec::new();
    for m in 1..=40 {
        for n in 1..=40 {
            let f = plate_mode(p, m as f32, n as f32);
            if f > 16_000.0 {
                break;
            }
            if f < 20.0 {
                continue;
            }
            let shape = (m as f32 * PI * x0).sin() * (n as f32 * PI * y0).sin();
            let w = shape.abs() * f * half_sine_spectrum(f, tau) * (f / (f + fc)).sqrt();
            modes.push((f * (1.0 + 0.02 * rng.signed()), w));
        }
    }
    modes.sort_by(|x, y| y.1.total_cmp(&x.1));
    modes.truncate(48);
    let strongest = modes.first().map_or(1.0, |m| m.1).max(1e-12);
    let mut bank = Modes::default();
    for (f, w) in modes {
        bank.add(f, damp * PI * f * p.loss, w / strongest, sr);
    }
    bank
}

#[allow(clippy::too_many_arguments)]
fn grains(
    rng: &mut Rng,
    out: &mut Vec<f32>,
    count: usize,
    seconds: f32,
    amp: f32,
    bright: f32,
    sr: f32,
) {
    for _ in 0..count {
        let at = (seconds * rng.unit().powi(2) * sr) as usize;
        let mut bank = Modes::default();
        let f = 800.0 + 6000.0 * bright * rng.unit();
        bank.add(f, 2000.0, amp * (0.3 + rng.unit()), sr);
        bank.render_into(out, at, sr);
    }
}

fn noise_burst(
    rng: &mut Rng,
    out: &mut Vec<f32>,
    start: usize,
    seconds: f32,
    centre: f32,
    amp: f32,
    sr: f32,
) {
    let len = (seconds * sr) as usize;
    let mut band = BandPass::new_at(centre, 1.2, sr as u32);
    if out.len() < start + len {
        out.resize(start + len, 0.0);
    }
    let decay = (-5.0 / len.max(1) as f32).exp();
    let mut env = amp;
    for x in &mut out[start..start + len] {
        *x += env * band.process(rng.signed());
        env *= decay;
    }
}

fn loudness(p: &MaterialProps) -> f32 {
    let stone = Material::Stone.props();
    ((p.density * wave_speed(p)) / (stone.density * wave_speed(&stone)))
        .powf(0.25)
        .clamp(0.2, 1.2)
}

/// One heel or toe strike: the surface's modes through the sole's Hertz
/// contact, the contact's click, then the surface's grains or crackle.
fn strike(
    rng: &mut Rng,
    surface: StepSurface,
    p: &MaterialProps,
    speed: f32,
    damp: f32,
    grit: f32,
    sr: f32,
) -> (Vec<f32>, Vec<f32>) {
    let (mass, radius, sole) = (8.0, 0.05, (10e6, 0.45));
    let crush = (mass * speed / 350.0).sqrt().clamp(0.1, 1.5);
    let tau = hertz_time(
        mass,
        radius,
        effective_modulus(sole, (p.young, p.poisson)),
        speed,
    );
    let mut out = Vec::new();
    struck_modes(rng, p, tau, damp, sr).render_into(&mut out, 0, sr);
    let pulse = ((tau * sr) as usize).max(2);
    if out.len() < pulse {
        out.resize(pulse, 0.0);
    }
    for (i, x) in out[..pulse].iter_mut().enumerate() {
        *x += 0.6 * (PI * i as f32 / pulse as f32).cos();
    }
    let mut smooth = LowPass::new_at((1.0 / tau).clamp(80.0, 20_000.0) * 2.0, sr as u32);
    for x in &mut out {
        *x = smooth.process(*x);
    }
    let mut tex = Vec::new();
    match surface {
        StepSurface::Gravel => grains(
            rng,
            &mut tex,
            (60.0 * crush * grit) as usize,
            0.06,
            0.3,
            0.7,
            sr,
        ),
        // Soil under the turf: a few dull crumbs, not gravel's crunch.
        StepSurface::Grass => grains(
            rng,
            &mut tex,
            (12.0 * crush * grit) as usize,
            0.05,
            0.15,
            0.15,
            sr,
        ),
        StepSurface::Dirt => grains(
            rng,
            &mut tex,
            (16.0 * crush * grit) as usize,
            0.04,
            0.2,
            0.2,
            sr,
        ),
        StepSurface::Snow => {
            for _ in 0..((120.0 * crush * grit) as usize).max(12) {
                let at = (0.1 * rng.unit().sqrt() * sr) as usize;
                let e = (1.0 + rng.unit() * (30f32.powf(-0.7) - 1.0)).powf(-1.0 / 0.7);
                let centre = 1500.0 + 2500.0 * rng.unit();
                noise_burst(rng, &mut tex, at, 0.003, centre, 0.8 * e.sqrt(), sr);
            }
        }
        _ => {}
    }
    (out, tex)
}

fn add_at(dst: &mut Vec<f32>, src: &[f32], at: usize, g: f32) {
    if dst.len() < at + src.len() {
        dst.resize(at + src.len(), 0.0);
    }
    for (o, x) in dst[at..].iter_mut().zip(src) {
        *o += g * x;
    }
}

/// A whole step: heel, scuff, the ball of the foot, and what the surface
/// adds (grass blades round the shoe, a board's creak).
fn footstep(
    rng: &mut Rng,
    surface: StepSurface,
    speed: f32,
    grit: f32,
    layer: StepLayer,
    sr: u32,
) -> Vec<f32> {
    let p = surface.material().props();
    let sr = sr as f32;
    let (mut out, mut tex) = strike(rng, surface, &p, speed, 1.0, grit, sr);
    let roll = 0.06 + 0.06 * rng.unit();
    let (scuff, centre) = match surface {
        StepSurface::Concrete | StepSurface::Gravel => (0.12, 3500.0),
        StepSurface::Wood | StepSurface::Metal | StepSurface::Ice => (0.06, 2500.0),
        StepSurface::Dirt => (0.05, 1500.0),
        StepSurface::Grass => (0.0, 0.0),
        StepSurface::Carpet | StepSurface::Snow => (0.02, 1200.0),
    };
    if scuff * grit > 0.0 {
        let c = centre * (0.8 + 0.4 * rng.unit());
        noise_burst(
            rng,
            &mut tex,
            (0.01 * sr) as usize,
            roll,
            c,
            scuff * grit,
            sr,
        );
    }
    let (toe, toe_tex) = strike(rng, surface, &p, 0.6 * speed, TOE_DAMP, grit, sr);
    let g = 0.45 + 0.25 * rng.unit();
    let at = (roll * sr) as usize;
    add_at(&mut out, &toe, at, g);
    add_at(&mut tex, &toe_tex, at, g);
    match surface {
        // Blades brushing the shoe going in and coming out.
        StepSurface::Grass => {
            // Most of the crunch is the heel pressing the blades down; a
            // little more as the ball of the foot lands.
            for k in 0..4 + (rng.unit() * 4.0) as usize {
                let base = if k % 3 == 2 { roll } else { 0.0 };
                let at = ((base + 0.035 * rng.unit()) * sr) as usize;
                let seconds = 0.03 + 0.06 * rng.unit();
                let c = 2500.0 + 3500.0 * rng.unit();
                noise_burst(rng, &mut tex, at, seconds, c, 0.35, sr);
            }
        }
        StepSurface::Wood if rng.unit() < 0.15 => {
            let seconds = 0.12 + 0.15 * rng.unit();
            let n = (seconds * sr) as usize;
            let mut body = BandPass::new_at(700.0 + 900.0 * rng.unit(), 5.0, sr as u32);
            let mut hz = 90.0 + 80.0 * rng.unit();
            let mut phase = 0.0f32;
            let start = (0.03 * sr) as usize;
            if out.len() < start + n {
                out.resize(start + n, 0.0);
            }
            for i in 0..n {
                let t = i as f32 / n as f32;
                hz *= 1.0 + 0.003 * rng.signed();
                phase += hz / sr;
                let slip = if phase >= 1.0 {
                    phase -= 1.0;
                    1.0 + 0.5 * rng.signed()
                } else {
                    0.0
                };
                out[start + i] += 0.5 * (t * 6.0).min(1.0) * (1.0 - t) * body.process(slip);
            }
        }
        _ => {}
    }
    // Levelled as a whole step, so the two layers add back to it exactly.
    let n = out.len().max(tex.len());
    out.resize(n, 0.0);
    tex.resize(n, 0.0);
    let peak = out
        .iter()
        .zip(&tex)
        .fold(0f32, |m, (a, b)| m.max((a + b).abs()));
    let k = if peak > 0.0 {
        0.4 * loudness(&p) / peak
    } else {
        0.0
    };
    for (a, b) in out.iter_mut().zip(&tex) {
        *a = k * match layer {
            StepLayer::All => *a + b,
            StepLayer::Body => *a,
            StepLayer::Texture => *b,
        };
    }
    let fade = out.len().min((0.005 * sr) as usize);
    let len = out.len();
    for (i, x) in out[len - fade..].iter_mut().enumerate() {
        *x *= 1.0 - i as f32 / fade as f32;
    }
    out
}
