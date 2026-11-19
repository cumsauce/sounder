//! `surf`: waves on a shore, after the foley project's shore bed
//! (`foley-synth/src/surf.rs` and `outdoors::surf`, MIT / Apache-2.0). Each
//! wave steepens in a low roar, plunges (its trapped air torn into bubbles
//! of every size, Deane and Stokes' spectrum), leaves foam that hisses as it
//! runs up the beach, and drains back, drawing pebbles on a shingle shore.
//! Waves start every `period` seconds over a continuous foam wash.

use std::f32::consts::TAU;

use foley_dsp::{BandPass, Bubble, LowPass, Wobble, plunge_radius};
use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// What the water runs up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Shore {
    /// Fine sand: the backwash drains quietly.
    #[default]
    Sand,
    /// Pebbles: the backwash draws them down, knocking at 2.5-5.5 kHz.
    Shingle,
}

/// Waves breaking on a shore.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Surf {
    /// `sand` (default) or `shingle`.
    #[serde(default)]
    pub shore: Shore,
    /// Wave height, m, 0.2 to 3 (default 1): taller waves entrain more
    /// bubbles and are louder.
    #[serde(default)]
    pub wave_height: Option<f32>,
    /// Seconds from one wave to the next, 1 to 60 (default 8). A wave lasts
    /// 7 s from its rise to the end of its backwash; a shorter period
    /// overlaps them (a storm sea is about 2 s).
    #[serde(default)]
    pub period: Option<f32>,
    /// 0 at the water's edge (default) to 1 far from it: waves fall to 40 %
    /// of their level, and the foam wash between them is gone.
    #[serde(default)]
    pub distance: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Surf {
    const BLURB: &'static str = "surf (after the foley project's shore): waves that rise in a roar, plunge into a spectrum of bubbles, hiss as foam and drain back (pebbles on shingle) over a foam wash; shore sand / shingle, wave_height m, period s, distance 0..1; loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["shore"];
    fn example() -> serde_json::Value {
        serde_json::json!({"shore": "shingle", "wave_height": 1.0, "period": 8, "seed": 1})
    }
}

impl Eval for Surf {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let height = self.wave_height.unwrap_or(1.0);
        if !(height.is_finite() && (0.2..=3.0).contains(&height)) {
            return Err(format!(
                "wave_height: {height}: give metres from 0.2 to 3 (0.3 a lap, 1 a swell, 3 a storm sea)"
            ));
        }
        let period = self.period.unwrap_or(8.0);
        if !(period.is_finite() && (1.0..=60.0).contains(&period)) {
            return Err(format!(
                "period: {period}: give seconds between waves from 1 to 60 (2 a storm, 8 a swell, 12 a calm)"
            ));
        }
        let distance = self.distance.unwrap_or(0.0);
        if !(distance.is_finite() && (0.0..=1.0).contains(&distance)) {
            return Err(format!(
                "distance: {distance}: give 0 (at the water's edge) to 1 (far from it)"
            ));
        }
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let out = render(
            &mut rng,
            cx.n,
            cx.sr,
            self.shore == Shore::Shingle,
            height,
            period,
            1.0 - distance,
            wrap,
        );
        Ok(one(Buf::Audio(out)))
    }
}

/// Foam and shingle wash level at full height at the water's edge, well
/// below the waves it runs under.
const WASH_LEVEL: f32 = 0.15;
/// Gain of one wave at full height before the distance taper.
const GRAIN_GAIN: f32 = 0.12;
const DISTANCE_FLOOR: f32 = 0.4;
const DISTANCE_RANGE: f32 = 0.6;
/// Seconds of linear fade at a wave's tail (an exact-zero end).
const TAIL_FADE: f32 = 0.05;
const WAVE_SECS: f32 = 7.0;

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// One wave breaking on a shore, `height` m, 7 s from its rise to the end
/// of its backwash (`outdoors::surf_into`, with the shore chosen).
fn wave(height: f32, shingle: bool, seed: u64, sr: f32) -> Vec<f32> {
    let rate = sr.round() as u32;
    let mut rng = Rng::new(seed ^ 0x5EA);
    let length = (WAVE_SECS * sr) as usize;
    let mut out = vec![0.0f32; length];
    let mut bubbles: Vec<Bubble> = Vec::with_capacity(256);
    let break_at = 1.2 + 0.6 * rng.unit();
    // foley draws the shore here; keep the draw so the rest follows its stream.
    let _ = rng.unit();
    let mut rumble = [LowPass::new_at(250.0, rate), LowPass::new_at(250.0, rate)];
    let mut crash = BandPass::new_at(900.0, 0.5, rate);
    let mut foam = BandPass::new_at(5000.0, 0.7, rate);
    let mut swell = Wobble::new_at(2.0, 3.0, 0.4, 1.6, rate);
    let (mut until_bubble, mut until_pop, mut pop) = (0.0f32, 0.0f32, 0.0f32);
    let dt = 1.0 / sr;
    for (i, sample) in out.iter_mut().enumerate() {
        let t = i as f32 * dt;
        let after = t - break_at;
        let noise = rng.signed();
        // The rising face: a low roar growing until it plunges.
        let rise = if after < 0.0 {
            (t / break_at).powi(2)
        } else {
            (-after / 1.5).exp()
        };
        let first = rumble[0].process(noise);
        let low = rumble[1].process(first);
        let mut s = 2.5 * rise * low;
        if after >= 0.0 {
            // The crash: broadband, falling away over a second.
            let hit = (after * 40.0).min(1.0) * (-after / 0.7).exp();
            s += 0.5 * hit * crash.process(noise) + 3.0 * hit * low;
            // The foam's hiss: bursting bubbles, brighter as the sheet runs
            // thin up the beach, gone as it soaks in.
            let sheet = (after / 0.4).min(1.0) * (-after / 2.2).exp();
            let wobble = swell.next(rng.signed());
            s += 0.25 * sheet * wobble * foam.process(rng.signed());
            // Tiny bubbles popping in the foam.
            until_pop -= dt;
            if until_pop <= 0.0 {
                pop = 0.2 + 0.8 * rng.unit();
                until_pop = rng.exponential(400.0 * sheet + 1.0);
            }
            pop *= 0.8;
            s += 0.05 * sheet * pop * rng.signed();
            // Trapped air, torn into bubbles that ring as they settle.
            let entrained = 4000.0 * (-after / 0.5).exp() * height.sqrt();
            until_bubble -= dt;
            while until_bubble <= 0.0 && entrained > 1.0 {
                until_bubble += rng.exponential(entrained);
                if bubbles.len() < 256 {
                    let radius = plunge_radius(&mut rng);
                    bubbles.push(Bubble::new_at(radius, 0.01 * (0.5 + rng.unit()), rate));
                }
            }
        }
        for b in &mut bubbles {
            s += b.sample();
        }
        if i % 256 == 0 {
            bubbles.retain(|b| !b.done());
        }
        *sample = s;
    }
    if shingle {
        // The backwash through shingle: pebbles knocking as the water draws
        // them down, thinning out as it drains.
        let start = break_at + 2.5;
        let mut at = start;
        while at < 6.8 {
            let left = 1.0 - (at - start) / (6.8 - start);
            let f = 2500.0 + 3000.0 * rng.unit();
            let knock_len = rate as usize / 100;
            let gain = 0.06 * left * rng.unit();
            let index = (at * sr) as usize;
            for (o, n) in out.iter_mut().skip(index).zip(0..knock_len) {
                let t = n as f32 * dt;
                *o += gain * (-t * 600.0).exp() * (TAU * f * t).sin();
            }
            at += rng.exponential(250.0 * left + 5.0);
        }
    }
    // Fade the end so it can overlap the next wave.
    let fade = (0.5 * sr) as usize;
    for i in 0..fade {
        out[length - 1 - i] *= i as f32 / fade as f32;
    }
    let loudest = peak(&out);
    if loudest > 0.0 {
        let scale = (0.25 + 0.2 * (height / 3.0)) / loudest;
        out.iter_mut().for_each(|x| *x *= scale);
    }
    // An exact-zero end, so a retiring wave never clicks.
    let tail = (TAIL_FADE * sr) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(length.saturating_sub(tail)) {
        *s *= (length - 1 - i) as f32 / tail as f32;
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn render(
    rng: &mut Rng,
    n: usize,
    sr: f32,
    shingle: bool,
    height: f32,
    period: f32,
    shore: f32,
    wrap: bool,
) -> Vec<f32> {
    let rate = sr.round() as u32;
    let tail = if wrap {
        ((0.5 * sr) as usize).min(n / 2)
    } else {
        0
    };
    let total = n + tail;
    // The continuous wash: noise through three 900 Hz band-pass stages.
    let wash_level = WASH_LEVEL * (height / 3.0) * shore;
    let mut stages = [
        BandPass::new_at(900.0, 0.5, rate),
        BandPass::new_at(900.0, 0.5, rate),
        BandPass::new_at(900.0, 0.5, rate),
    ];
    let mut out: Vec<f32> = (0..total)
        .map(|_| {
            let w = stages[0].process(rng.signed());
            let w = stages[1].process(w);
            wash_level * stages[2].process(w)
        })
        .collect();
    if wrap && tail > 0 {
        // The wash is stationary noise: crossfade its tail onto the start.
        for j in 0..tail {
            let w = j as f32 / tail as f32;
            let t = out[n + j];
            out[j] = out[j] * w.sqrt() + t * (1.0 - w).sqrt();
        }
    }
    out.truncate(n);
    // Waves from t = 0 every `period`; in a loop the last ones run on past
    // the end and wrap round to the start.
    let gain = GRAIN_GAIN * height * (DISTANCE_FLOOR + DISTANCE_RANGE * shore);
    let base = rng.next_u64();
    let mut k = 0u64;
    let mut start = 0usize;
    while start < n {
        let w = wave(
            height,
            shingle,
            base ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            sr,
        );
        for (j, v) in w.iter().enumerate() {
            let idx = start + j;
            if idx < n {
                out[idx] += gain * v;
            } else if wrap {
                out[idx % n] += gain * v;
            } else {
                break;
            }
        }
        k += 1;
        start = (k as f32 * period * sr) as usize;
    }
    // Soft-clip so any overlap stays in range.
    for s in &mut out {
        *s /= 1.0 + s.abs();
    }
    out
}
