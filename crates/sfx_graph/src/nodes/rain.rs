//! `rain`: physically sized drops, after the `Rain` bed of the foley
//! project (github.com/cumsauce/foley, `foley-dsp/src/rain.rs`, MIT /
//! Apache-2.0). Near drops are heard one by one, sized by Marshall-Palmer
//! for the rain rate, each a struck surface (two damped modes and a click)
//! or, on water, a small ringing bubble; the far ones blur into a hiss
//! coloured by the surface. (A port of Farnell's RAIN patches rendered a
//! few drops a second and was heard as static and popcorn; it is gone.)

use std::f32::consts::{PI, TAU};

use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// What the rain lands on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RainSurface {
    /// Ponds, the sea: small drops trap bubbles ringing near 14 kHz.
    Water,
    /// Stone, concrete, pavement: a sharp tick (3-7 kHz modes).
    #[default]
    Stone,
    /// Leaves: a soft, damped patter (0.5-1.8 kHz).
    #[serde(alias = "leaves")]
    Leaf,
    /// Planks, logs, a wooden roof: a dull knock.
    Wood,
    /// Grass, dirt, sand, snow: little more than a thud (`ground` too).
    #[serde(alias = "ground", alias = "grass", alias = "dirt")]
    Soft,
    /// A window: a bright ring (`window` too).
    #[serde(alias = "window")]
    Glass,
    /// A tin roof, a car: a tinny ring.
    Metal,
}

/// Rain falling round the listener.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rain {
    /// What it lands on (default `stone`): water, stone, leaf, wood, soft
    /// (grass, dirt; `ground`), glass (`window`), metal.
    #[serde(default)]
    pub surface: RainSurface,
    /// Rain rate, mm/h, 0.1 to 200 (default 10): 2 light, 10 moderate, 50 a
    /// downpour. Sets the drops a second (Marshall-Palmer: about 2 250 heard
    /// one by one at 10) and their sizes, and the far hiss.
    #[serde(default)]
    pub rate: Option<f32>,
    /// Sheltering under a roof of this surface: 85 % of the near drops land
    /// on it, the rest of the rain comes in muffled (the hiss low-passed at
    /// 600 Hz).
    #[serde(default)]
    pub roof: Option<RainSurface>,
    /// The roof's height over the ear, m (default 3): lower is louder.
    #[serde(default)]
    pub roof_height: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Rain {
    const BLURB: &'static str = "rain (after the foley project's physical rain): Marshall-Palmer drops, each a struck surface or a bubble on water, over a hiss coloured by the surface; surface water / stone / leaf / wood / soft / glass / metal, rate mm/h, roof; loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["surface", "roof"];
    fn example() -> serde_json::Value {
        serde_json::json!({"surface": "stone", "rate": 10, "seed": 1})
    }
}

impl Eval for Rain {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = self.rate.unwrap_or(10.0);
        if !(rate.is_finite() && (0.1..=200.0).contains(&rate)) {
            return Err(format!(
                "rate: {rate}: give mm/h from 0.1 to 200 (2 light, 10 moderate, 50 a downpour)"
            ));
        }
        let h = self.roof_height.unwrap_or(3.0);
        if !(h.is_finite() && (0.5..=50.0).contains(&h)) {
            return Err(format!("roof_height: {h}: give metres from 0.5 to 50"));
        }
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let out = render(
            &mut rng,
            cx.n,
            cx.sr,
            self.surface,
            rate,
            self.roof.map(|r| (r, h)),
            wrap,
        );
        Ok(one(Buf::Audio(out)))
    }
}

/// Marshall-Palmer: N(D) = N0 e^(-lambda D), lambda = 4.1 R^-0.21 per mm.
fn lambda(rate: f32) -> f32 {
    4.1 * rate.max(0.1).powf(-0.21)
}

/// Terminal speed, m/s, for a diameter in mm (Gunn-Kinzer, Atlas et al.).
fn speed(d: f32) -> f32 {
    (9.65 - 10.3 * (-0.6 * d).exp()).max(0.1)
}

const MAX_DROPS: f32 = 2500.0;
/// Drops on this much area (m2) round the listener stand out one by one.
const NEAR_AREA: f32 = 0.5;
const MP_N0: f32 = 8000.0;
const MAX_VOICES: usize = 384;

/// The drops' surface as the model sees it.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Water,
    Stone,
    Leaves,
    Wood,
    Soft,
    Glass,
    Metal,
}

fn kind(s: RainSurface) -> Kind {
    match s {
        RainSurface::Water => Kind::Water,
        RainSurface::Stone => Kind::Stone,
        RainSurface::Leaf => Kind::Leaves,
        RainSurface::Wood => Kind::Wood,
        RainSurface::Soft => Kind::Soft,
        RainSurface::Glass => Kind::Glass,
        RainSurface::Metal => Kind::Metal,
    }
}

impl Kind {
    fn brightness(self) -> f32 {
        match self {
            Kind::Water => 0.8,
            Kind::Stone | Kind::Glass | Kind::Metal => 0.7,
            Kind::Wood => 0.45,
            Kind::Leaves => 0.35,
            Kind::Soft => 0.25,
        }
    }
    fn loudness(self) -> f32 {
        match self {
            Kind::Water | Kind::Stone => 1.0,
            Kind::Glass | Kind::Metal => 1.2,
            Kind::Wood => 0.8,
            Kind::Leaves => 0.6,
            Kind::Soft => 0.15,
        }
    }
}

fn unit(rng: &mut Rng) -> f32 {
    rng.unit()
}

fn exponential(rng: &mut Rng, rate: f32) -> f32 {
    -(1.0 - unit(rng)).max(1e-9).ln() / rate.max(1e-6)
}

/// A struck surface: two damped modes as rotating phasors, and a click.
struct Struck {
    modes: [[f32; 4]; 2],
    click: f32,
    click_step: f32,
    age: f32,
    decay: f32,
}

impl Struck {
    fn new(rng: &mut Rng, k: Kind, amp: f32, sr: f32) -> Struck {
        let (lo, hi, decay, click) = match k {
            Kind::Stone => (4500.0, 10000.0, 1100.0, 0.9), // foley: 3-7 kHz, heard as "too low, oil fry"
            Kind::Leaves => (500.0, 1800.0, 220.0, 0.25),
            Kind::Wood => (280.0, 900.0, 260.0, 0.35),
            Kind::Glass => (2500.0, 7000.0, 450.0, 0.6), // foley: decay 90, heard as chimes
            Kind::Metal => (900.0, 4000.0, 220.0, 0.5),  // foley: decay 35, heard as chimes
            Kind::Soft | Kind::Water => (250.0, 1200.0, 700.0, 0.35),
        };
        let f32_hi: f32 = hi;
        let mut mode = |gain: f32| {
            let f = lo * (f32_hi / lo).powf(unit(rng));
            let shrink = (-decay * (0.8 + 0.4 * unit(rng)) / sr).exp();
            let step = TAU * f.min(0.45 * sr) / sr;
            let a = amp * gain * (0.6 + 0.4 * unit(rng));
            [a, 0.0, shrink * step.cos(), shrink * step.sin()]
        };
        let modes = [mode(1.0), mode(0.5)];
        Struck {
            modes,
            click: amp * click,
            click_step: (-4000.0 / sr).exp(),
            age: 0.0,
            decay,
        }
    }
    fn sample(&mut self, noise: f32, dt: f32) -> f32 {
        self.click *= self.click_step;
        let mut out = self.click * noise;
        for [re, im, c, s] in &mut self.modes {
            let (r, i) = (*re * *c - *im * *s, *re * *s + *im * *c);
            *re = r;
            *im = i;
            out += i;
        }
        self.age += dt;
        out
    }
    fn done(&self) -> bool {
        self.decay * self.age > 7.0
    }
}

/// A Minnaert bubble whose pitch rises a little as it rings.
struct Bubble {
    freq: f32,
    damping: f32,
    rise: f32,
    amp: f32,
    step: f32,
    phase: f32,
    age: f32,
}

impl Bubble {
    fn new(radius: f32, loud: f32, rise: f32, sr: f32) -> Bubble {
        let freq = 20.65 / (TAU * radius);
        let damping = PI * freq * 0.045;
        Bubble {
            freq,
            damping,
            rise: rise * damping,
            amp: loud * (radius / 0.001).powf(1.5),
            step: (-damping / sr).exp(),
            phase: 0.0,
            age: 0.0,
        }
    }
    fn sample(&mut self, dt: f32) -> f32 {
        let f = (self.freq * (1.0 + self.rise * self.age)).min(0.45 / dt);
        self.phase = (self.phase + f * dt).fract();
        self.age += dt;
        self.amp *= self.step;
        self.amp * (TAU * self.phase).sin()
    }
    fn done(&self) -> bool {
        self.damping * self.age > 6.9
    }
}

enum Voice {
    Struck(Struck),
    Bubble(Bubble),
}

/// A one-pole band-pass made of a high-pass and a low-pass (for the far
/// water's sizzle near 13.5 kHz).
struct Band {
    lp: f32,
    hp_in: f32,
    hp: f32,
    a_lo: f32,
    a_hi: f32,
}

impl Band {
    fn new(lo: f32, hi: f32, sr: f32) -> Band {
        Band {
            lp: 0.0,
            hp_in: 0.0,
            hp: 0.0,
            a_lo: (-TAU * lo / sr).exp(),
            a_hi: 1.0 - (-TAU * hi.min(0.45 * sr) / sr).exp(),
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.hp = self.a_lo * (self.hp + x - self.hp_in);
        self.hp_in = x;
        self.lp += self.a_hi * (self.hp - self.lp);
        self.lp
    }
}

/// The rain: `rate` mm/h on `surface`, under an optional `roof` (its
/// surface and its height over the ear, blocks / m). With `wrap` the last
/// 0.5 s of ringing is folded onto the start, so the buffer loops.
fn render(
    rng: &mut Rng,
    n: usize,
    sr: f32,
    surface: RainSurface,
    rate: f32,
    roof: Option<(RainSurface, f32)>,
    wrap: bool,
) -> Vec<f32> {
    let k = kind(surface);
    let roof = roof.map(|(s, h)| (kind(s), h));
    let lam = lambda(rate);
    let flux: f32 = (1..=60)
        .map(|i| {
            let d = i as f32 * 0.1;
            MP_N0 * (-lam * d).exp() * speed(d) * 0.1
        })
        .sum();
    let near = if rate > 0.0 {
        (flux * NEAR_AREA).min(MAX_DROPS)
    } else {
        0.0
    };
    let tail = if wrap { (0.5 * sr) as usize } else { 0 };
    let total = n + tail;
    let dt = 1.0 / sr;
    let brightness = k.brightness();
    let open_cut = 1000.0 + 7000.0 * brightness;
    let cut = if roof.is_some() { 600.0 } else { open_cut };
    let smooth = 1.0 - (-TAU * cut / sr).exp();
    let level = 0.012 * (rate / 10.0).sqrt() * (0.3 + brightness);
    let sizzle_lvl = if k == Kind::Water && roof.is_none() {
        0.025 * (rate / 10.0).sqrt() // foley 0.07: heard as white noise
    } else {
        0.0
    };
    let mut sizzle = Band::new(9000.0, 18000.0, sr);
    let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES);
    let (mut hiss, mut hiss_low) = (0.0f32, 0.0f32);
    let mut until = 0.0f32;
    let mut out = vec![0.0f32; total];
    // A second of hiss first, so the filters have settled at sample 0.
    for _ in 0..(0.05 * sr) as usize {
        hiss += smooth * (rng.signed() - hiss);
        hiss_low += smooth * (hiss - hiss_low);
    }
    for (i, o) in out.iter_mut().enumerate() {
        if near > 0.0 {
            until -= dt;
            while until <= 0.0 {
                if voices.len() < MAX_VOICES {
                    let d = (0.3 + exponential(rng, lam)).min(5.0);
                    let energy = (d / 2.0).powi(3) * (speed(d) / speed(2.0)).powi(2);
                    let dist = 0.3 + 1.2 * unit(rng).sqrt();
                    let mut amp = 0.05 * energy.sqrt() / dist;
                    let s = match roof {
                        Some((r, h)) if unit(rng) < 0.85 => {
                            amp *= 1.5 / h.max(1.0);
                            r
                        }
                        Some(_) => {
                            amp *= 0.25;
                            k
                        }
                        None => k,
                    };
                    amp *= s.loudness();
                    if s == Kind::Water {
                        // Small drops trap a bubble near 14 kHz (the sizzle);
                        // heavy ones now and then a lower plink.
                        if (0.8..=1.1).contains(&d) {
                            let r = 0.0002 + 0.00005 * unit(rng);
                            voices.push(Voice::Bubble(Bubble::new(r, amp * 6.0, 0.02, sr)));
                        } else if d > 2.2 && unit(rng) < 0.15 {
                            let r = 0.0006 + 0.0009 * unit(rng);
                            voices.push(Voice::Bubble(Bubble::new(r, amp * 0.1, 0.02, sr)));
                        }
                        voices.push(Voice::Struck(Struck::new(rng, Kind::Soft, amp * 0.25, sr)));
                    } else {
                        voices.push(Voice::Struck(Struck::new(rng, s, amp, sr)));
                    }
                }
                until += exponential(rng, near);
            }
        }
        let noise = rng.signed();
        let mut mix = 0.0;
        for v in &mut voices {
            mix += match v {
                Voice::Struck(s) => s.sample(noise, dt),
                Voice::Bubble(b) => b.sample(dt),
            };
        }
        if i % 64 == 63 {
            voices.retain(|v| match v {
                Voice::Struck(s) => !s.done(),
                Voice::Bubble(b) => !b.done(),
            });
        }
        hiss += smooth * (rng.signed() - hiss);
        hiss_low += smooth * (hiss - hiss_low);
        let far = sizzle_lvl * sizzle.tick(rng.signed());
        *o = mix + level * hiss_low * 3.0 + far;
    }
    if wrap {
        // The tail's ringing goes onto the start; the hiss crossfades.
        let fade = tail;
        for j in 0..fade {
            let w = j as f32 / fade as f32;
            let tail_v = out[n + j];
            out[j] = out[j] * w.sqrt() + tail_v * (1.0 - w).sqrt();
        }
        out.truncate(n);
    }
    out
}
