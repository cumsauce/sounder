//! `creature`: a made-up animal's call (roar, growl, snarl, whine), after the
//! foley project (github.com/cumsauce/foley, `foley-synth/src/voice/`:
//! `creature.rs`, `score.rs`, `tract.rs`, and the ravager / polar bear
//! roar and the wolf's growl and whine in `mod.rs` / `mobs2.rs`, MIT /
//! Apache-2.0).
//!
//! The physics is foley's: a Liljencrants-Fant glottal source (Rd from
//! pressed 0.3 to breathy 2.7) with nonlinear phenomena (subharmonics,
//! sidebands, chaos with pitch jumps, creak, ingressive voicing) through a
//! Kelly-Lochbaum waveguide tract with a nasal branch. Formant spacing
//! follows the tract (dF = c / 2L), and a bigger body has a longer tract and
//! a lower pitch (Fitch 1997). `tract` and `glottis` in `foley.rs` are a
//! formant cascade; this is the waveguide, which foley keeps private, so it
//! is ported whole here. Dropped: frication at a constriction and
//! biphonation (no call here uses them) and the Minecraft mob table.
//! Differences: the tract's section length follows the document's sample
//! rate (foley is fixed at 48 kHz); a tract is at most 180 sections (about
//! 0.64 m at 48 kHz).

use std::f32::consts::TAU;

use foley_synth::LowPass;
use serde::Deserialize;

use super::foley::{fit, lf_at, rate};
use super::one;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

/// Which call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CreatureCallKind {
    /// A long, rough, rising-and-falling bellow (the ravager's and polar
    /// bear's warning): subharmonics, sidebands at 40 Hz, creak at the ends.
    Roar,
    /// Low rumbling, vowel uh to ah, with sidebands at the roughness rates.
    #[default]
    Growl,
    /// The wolf's growl: a pressed rumble with the lips drawn, then the same
    /// again drawn in on the in-breath.
    Snarl,
    /// A nasal, rising, trembling cry through the nose.
    Whine,
}

/// A creature's call.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Creature {
    /// roar, growl (default), snarl or whine.
    #[serde(default)]
    pub call: CreatureCallKind,
    /// The body, 0 (a bird: 4 cm tract, 1.5 kHz) to 1 (a giant: 80 cm, 60
    /// Hz); default 0.7 (a bear: 28 cm, about 130 Hz). Sets the tract length
    /// and the usual pitch together, as a bigger animal has both.
    #[serde(default)]
    pub size: Option<f32>,
    /// How threatening, 0 to 1 (default 0.6): pressed rather than breathy
    /// voicing, stronger subharmonics.
    #[serde(default)]
    pub aggression: Option<f32>,
    /// How far it goes into chaos, subharmonics, creak and the 30-150 Hz
    /// roughness, 0 to 1 (default 0.6).
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Seconds the call lasts (0.1 to 10). Unset: the call's own, about 1 s
    /// (a snarl 1.3).
    #[serde(default)]
    pub length: Option<f32>,
    /// Start time, s.
    #[serde(default)]
    pub at: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Creature {
    const BLURB: &'static str = "creature (after the foley project's creature voices): an LF glottal source with subharmonics, sidebands, chaos and creak through a waveguide vocal tract whose length is the body; call roar / growl / snarl / whine, size 0..1 (tract + pitch), aggression, roughness, length s";
    const WORDS: &'static [&'static str] = &["call"];
    fn example() -> serde_json::Value {
        serde_json::json!({"call": "growl", "size": 0.7, "aggression": 0.6, "roughness": 0.6, "seed": 1})
    }
}

fn unit_field(name: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() && (0.0..=1.0).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{name}: {x}: give a number from 0 to 1"))
    }
}

impl Eval for Creature {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = unit_field("size", self.size.unwrap_or(0.7))?;
        let aggression = unit_field("aggression", self.aggression.unwrap_or(0.6))?;
        let roughness = unit_field("roughness", self.roughness.unwrap_or(0.6))?;
        if let Some(l) = self.length
            && !(l.is_finite() && (0.1..=10.0).contains(&l))
        {
            return Err(format!("length: {l}: give seconds from 0.1 to 10"));
        }
        let mut rng = cx.rng(self.seed);
        let out = render(
            &mut rng,
            self.call,
            size,
            aggression,
            roughness,
            self.length,
            rate(cx),
        );
        Ok(one(fit(&out, self.at.unwrap_or(0.0), 1.0, cx)))
    }
}

/// Tract length (m): 4 cm for a bird to 80 cm for a giant.
fn tract_length(size: f32) -> f32 {
    0.04 * 20f32.powf(size)
}

/// The usual pitch (Hz): 1.5 kHz down to 60 Hz.
fn usual_pitch(size: f32) -> f32 {
    1500.0 * 0.04f32.powf(size)
}

fn render(
    rng: &mut Rng,
    call: CreatureCallKind,
    size: f32,
    aggression: f32,
    roughness: f32,
    length: Option<f32>,
    sr: u32,
) -> Vec<f32> {
    let (mut segments, peak, natural) = match call {
        CreatureCallKind::Roar => (roar(rng, aggression, roughness), 0.7, 1.0),
        CreatureCallKind::Growl => (growl(rng, aggression, roughness), 0.6, 1.2),
        CreatureCallKind::Snarl => (snarl(rng, aggression, roughness), 0.7, 1.3),
        CreatureCallKind::Whine => (whine(rng, roughness), 0.5, 0.8),
    };
    if let Some(l) = length {
        let scale = l / natural;
        for s in &mut segments {
            s.secs *= scale;
            s.gap *= scale;
        }
    }
    let voice = Voice {
        length: tract_length(size),
        f0: usual_pitch(size) * (1.0 + 0.06 * rng.signed()),
    };
    finish_at(perform_at(rng, voice, &segments, sr), peak, sr)
}

/// A usual value wavered by the usual amount.
fn vary(rng: &mut Rng, base: f32, frac: f32) -> f32 {
    let gauss = rng.signed() + rng.signed() + rng.signed();
    base * (1.0 + frac * gauss / 3.0)
}

/// The ravager's and polar bear's roar: out of a grunt, up, and down.
fn roar(rng: &mut Rng, aggression: f32, rough: f32) -> Vec<Seg> {
    let mut g = Seg::new(vary(rng, 1.0, 0.2)).vowels(&[(0.0, UH), (0.3, AA), (1.0, OH)]);
    g.f0 = k(&[(0.0, 1.0), (0.3, 1.3), (1.0, 0.9)]);
    g.rd = c(0.9 - 0.6 * aggression);
    g.sub = c(0.2 + 0.6 * aggression * rough.max(0.3));
    g.chaos = c(rough);
    g.creak = k(&[(0.0, 0.3), (0.2, 0.0), (0.8, 0.0), (1.0, 0.5)]);
    g.sidebands = c(0.2);
    g.sideband_hz = 40.0;
    g.breath = c(0.35);
    vec![g]
}

/// foley's `creature` growl: low and rumbling, out and in, sidebands at the
/// roughness rates.
fn growl(rng: &mut Rng, aggression: f32, rough: f32) -> Vec<Seg> {
    let breathy = 0.8 * (1.0 - aggression);
    let mut g = Seg::new(vary(rng, 1.2, 0.2)).vowels(&[(0.0, UH), (1.0, AA)]);
    g.f0 = k(&[(0.0, 0.8), (0.5, 0.75), (1.0, 0.7)]);
    g.rd = c(0.5 + 0.4 * breathy);
    g.loud = k(&[(0.0, 0.0), (0.2, 0.9), (0.7, 0.8), (1.0, 0.0)]);
    g.sub = c(0.45 + 0.25 * rough);
    g.sidebands = c(0.2 + 0.3 * rough);
    g.sideband_hz = 30.0 + 60.0 * rough;
    g.chaos = k(&[(0.0, 0.15 * rough), (0.5, 0.3 * rough), (1.0, 0.2 * rough)]);
    g.breath = c(0.25 + 0.25 * breathy);
    g.jitter = 0.03 * (1.0 + rough);
    vec![g]
}

/// The wolf's growl: rumbling out and in, about a third of the usual pitch.
fn snarl(rng: &mut Rng, aggression: f32, rough: f32) -> Vec<Seg> {
    let mut out = Seg::new(vary(rng, 0.8, 0.2)).vowels(&[(0.0, UH)]);
    out.f0 = c(0.4 - 0.1 * aggression);
    out.rd = c(0.9 - 0.4 * aggression);
    out.sub = c(0.5);
    out.chaos = c(0.5 * rough / 0.6);
    out.creak = c(0.3);
    out.lips = c(0.5);
    out.sidebands = c(0.2);
    out.sideband_hz = 40.0;
    out.breath = c(0.3);
    let mut inward = out.clone();
    inward.secs = vary(rng, 0.5, 0.2);
    inward.ingressive = true;
    vec![out, inward]
}

/// The wolf's whine: a nasal rising cry, trembling at 6 Hz, at twice the
/// usual pitch.
fn whine(rng: &mut Rng, rough: f32) -> Vec<Seg> {
    let mut w = Seg::new(vary(rng, 0.8, 0.25)).vowels(&[(0.0, OO)]);
    w.f0 = k(&[(0.0, 2.0), (0.4, 2.4), (1.0, 1.9)]);
    w.velum = c(1.0);
    w.lips = c(0.2);
    w.tremolo = (6.0, 0.4);
    w.chaos = c(0.1 * rough);
    vec![w]
}

// ------------------------------------------------------------------ score

/// A value over a segment, by keyframes at fractions 0 to 1 of it.
#[derive(Debug, Clone)]
struct K(Vec<(f32, f32)>);

impl K {
    fn at(&self, t: f32) -> f32 {
        let keys = &self.0;
        let i = keys.iter().position(|k| k.0 > t).unwrap_or(keys.len());
        if i == 0 {
            return keys[0].1;
        }
        if i == keys.len() {
            return keys[keys.len() - 1].1;
        }
        let (t0, a) = keys[i - 1];
        let (t1, b) = keys[i];
        a + (b - a) * (t - t0) / (t1 - t0).max(1e-6)
    }
}

fn k(points: &[(f32, f32)]) -> K {
    K(points.to_vec())
}

fn c(value: f32) -> K {
    K(vec![(0.0, value)])
}

/// Mouth shapes, measured on a 17 cm tract against Peterson and Barney's
/// vowels: where the tongue is (0 glottis to 1 lips), its height (Pink
/// Trombone's diameter, 2.05 close to 3.5 open), and the lips' opening.
#[derive(Debug, Clone, Copy)]
struct Vowel(f32, f32, f32);

const AA: Vowel = Vowel(0.27, 2.7, 1.0);
const UH: Vowel = Vowel(0.35, 2.8, 0.85);
const OH: Vowel = Vowel(0.34, 2.4, 0.5);
const OO: Vowel = Vowel(0.48, 2.1, 0.3);

/// One stretch of a call.
#[derive(Debug, Clone)]
struct Seg {
    secs: f32,
    /// Silence after it (the tract rings on).
    gap: f32,
    /// Pitch against the voice's usual.
    f0: K,
    /// Voice quality, Rd: 0.3 pressed, 1 modal, 2.7 breathy.
    rd: K,
    loud: K,
    voicing: K,
    breath: K,
    mouth: Vec<(f32, Vowel)>,
    lips: K,
    velum: K,
    /// Subharmonics: every second pulse full, the others weaker by this.
    sub: K,
    /// Amplitude modulation at `sideband_hz` (30-150 Hz is roughness).
    sidebands: K,
    sideband_hz: f32,
    /// Bursts of jitter and shimmer, and pitch jumps.
    chaos: K,
    /// Vocal fry: irregular pulses at 25-65 Hz, blended in.
    creak: K,
    jitter: f32,
    ingressive: bool,
    /// Vibrato: rate (Hz) and depth.
    tremolo: (f32, f32),
}

impl Seg {
    fn new(secs: f32) -> Seg {
        Seg {
            secs,
            gap: 0.0,
            f0: c(1.0),
            rd: c(1.0),
            loud: k(&[(0.0, 0.0), (0.12, 1.0), (0.8, 0.9), (1.0, 0.0)]),
            voicing: c(1.0),
            breath: c(0.05),
            mouth: vec![(0.0, UH)],
            lips: c(1.0),
            velum: c(0.0),
            sub: c(0.0),
            sidebands: c(0.0),
            sideband_hz: 70.0,
            chaos: c(0.0),
            creak: c(0.0),
            jitter: 0.01,
            ingressive: false,
            tremolo: (0.0, 0.0),
        }
    }

    fn vowels(mut self, keys: &[(f32, Vowel)]) -> Seg {
        self.mouth = keys.to_vec();
        self
    }

    fn mouth_at(&self, t: f32) -> Vowel {
        let keys = &self.mouth;
        let i = keys.iter().position(|k| k.0 > t).unwrap_or(keys.len());
        if i == 0 {
            return keys[0].1;
        }
        if i == keys.len() {
            return keys[keys.len() - 1].1;
        }
        let (t0, a) = keys[i - 1];
        let (t1, b) = keys[i];
        let w = (t - t0) / (t1 - t0).max(1e-6);
        Vowel(
            a.0 + w * (b.0 - a.0),
            a.1 + w * (b.1 - a.1),
            a.2 + w * (b.2 - a.2),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Voice {
    length: f32,
    f0: f32,
}

/// A rough normal deviate (the sum of three uniforms).
fn gauss(rng: &mut Rng) -> f32 {
    rng.signed() + rng.signed() + rng.signed()
}

/// Performs `segments`: the LF model at each segment's Rd, its period set by
/// the pitch with jitter and its pulses by shimmer; subharmonics weaken (and
/// lengthen) every other pulse; sidebands modulate the amplitude; chaos adds
/// bursts of jitter and shimmer and occasional pitch jumps; creak pulls the
/// rate down to 25-65 Hz, irregularly; ingressive voicing plays each pulse
/// backwards, breathier and quieter. Breath noise follows the glottal flow.
/// The tract moves towards each instant's mouth shape in a few milliseconds.
fn perform_at(rng: &mut Rng, voice: Voice, segments: &[Seg], sample_rate: u32) -> Vec<f32> {
    let rate = sample_rate as f32;
    let mut tract = Tract::new(voice.length, sample_rate);
    let mut out = Vec::new();
    let mut phase = 0.0f32;
    let mut scale = 1.0f32;
    let mut amp = 1.0f32;
    let mut pulse = 0u32;
    let mut jump = 1.0f32;
    let creak_hz = 25.0 + 40.0 * rng.unit();
    let mut breath_lp = [
        LowPass::new_at(4000.0, sample_rate),
        LowPass::new_at(6000.0, sample_rate),
    ];
    let mut time = 0.0f32;
    for seg in segments {
        let samples = (seg.secs * rate) as usize;
        for i in 0..samples {
            let t = i as f32 / samples.max(1) as f32;
            time += 1.0 / rate;
            if i % 32 == 0 {
                let m = seg.mouth_at(t);
                tract.shape(m.0, m.1, m.2 * seg.lips.at(t), seg.velum.at(t));
            }
            tract.settle(0.01);
            let chaos = seg.chaos.at(t);
            let creak = seg.creak.at(t);
            let mut f0 = voice.f0 * seg.f0.at(t) * scale * jump;
            if seg.tremolo.0 > 0.0 {
                f0 *= 1.0 + 0.05 * seg.tremolo.1 * (TAU * seg.tremolo.0 * time).sin();
            }
            f0 = f0 * (1.0 - creak) + creak_hz * creak;
            phase += f0 / rate;
            if phase >= 1.0 {
                phase -= 1.0;
                pulse += 1;
                let jitter = seg.jitter + 0.08 * chaos + 0.2 * creak;
                scale = (1.0 + jitter * gauss(rng)).clamp(0.5, 1.6);
                amp = (1.0 + (0.05 + 0.4 * chaos + 0.3 * creak) * gauss(rng)).max(0.0);
                let sub = seg.sub.at(t);
                if sub > 0.0 && !pulse.is_multiple_of(2) {
                    amp *= 1.0 - sub;
                    scale *= 1.0 + 0.15 * sub;
                }
                if chaos > 0.3 && rng.unit() < 0.015 * chaos {
                    jump = if jump != 1.0 {
                        1.0
                    } else if rng.unit() < 0.5 {
                        1.35
                    } else {
                        0.78
                    };
                }
            }
            let mut rd = seg.rd.at(t);
            let mut gain = 1.0;
            let mut cycle = phase;
            if seg.ingressive {
                cycle = 1.0 - phase;
                rd += 0.5;
                gain = 0.6;
            }
            if creak > 0.0 {
                rd = rd * (1.0 - creak) + 0.4 * creak;
            }
            let (dg, flow) = lf_at(rd, cycle);
            let voicing = seg.voicing.at(t);
            let mut voiced = dg * amp * voicing * gain;
            let sidebands = seg.sidebands.at(t);
            if sidebands > 0.0 {
                voiced *= 1.0 + sidebands * (TAU * seg.sideband_hz * time).sin();
            }
            let noise = rng.signed();
            let opening = if voicing > 0.1 { 0.2 + flow } else { 0.7 };
            // Breath noise sits well below the voice (about -10 dB at full breath).
            let first = breath_lp[0].process(noise);
            let coloured = breath_lp[1].process(first);
            let ingress = if seg.ingressive { 1.5 } else { 1.0 };
            let aspiration = 0.3 * coloured * seg.breath.at(t) * opening * ingress;
            let glottis = seg.loud.at(t) * (0.5 * voiced + aspiration);
            out.push(tract.run(glottis));
        }
        for _ in 0..(seg.gap * rate) as usize {
            tract.settle(0.01);
            out.push(tract.run(0.0));
        }
    }
    out
}

/// Takes off the lowest rumble (below about 40 Hz), scales to `peak`, and
/// fades the ends so it never clicks.
fn finish_at(mut out: Vec<f32>, peak: f32, sample_rate: u32) -> Vec<f32> {
    let mut low = LowPass::new_at(40.0, sample_rate);
    for x in &mut out {
        *x -= low.process(*x);
    }
    out.extend(std::iter::repeat_n(0.0, sample_rate as usize / 50));
    let loudest = out.iter().fold(0f32, |m, x| m.max(x.abs()));
    if loudest > 0.0 {
        out.iter_mut().for_each(|x| *x *= peak / loudest);
    }
    let fade = (sample_rate as usize / 500).min(out.len() / 2);
    let len = out.len();
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        out[i] *= g;
        out[len - 1 - i] *= g;
    }
    out
}

// ------------------------------------------------------------------ tract

const SPEED_OF_SOUND: f32 = 343.0;
const GLOTTIS: f32 = 0.75;
const LIPS: f32 = -0.85;
const NOSTRILS: f32 = -0.85;
const FADE: f32 = 0.999;

#[derive(Debug, Clone)]
struct Tube {
    right: Vec<f32>,
    left: Vec<f32>,
    out_right: Vec<f32>,
    out_left: Vec<f32>,
    area: Vec<f32>,
}

impl Tube {
    fn new(n: usize) -> Tube {
        Tube {
            right: vec![0.0; n],
            left: vec![0.0; n],
            out_right: vec![0.0; n + 1],
            out_left: vec![0.0; n + 1],
            area: vec![1.0; n],
        }
    }

    fn len(&self) -> usize {
        self.right.len()
    }

    /// Two-port junctions between every pair of sections from `from`, as
    /// pressure: p = 2(A1 p1+ + A2 p2-)/(A1 + A2), each outgoing wave p minus
    /// what came in on that side.
    fn scatter(&mut self, from: usize, skip: Option<usize>) {
        for i in from.max(1)..self.len() {
            if Some(i) == skip {
                continue;
            }
            let (a1, a2) = (self.area[i - 1], self.area[i]);
            let p = 2.0 * (a1 * self.right[i - 1] + a2 * self.left[i]) / (a1 + a2);
            self.out_right[i] = p - self.left[i];
            self.out_left[i] = p - self.right[i - 1];
        }
    }

    fn propagate(&mut self) {
        let n = self.len();
        for i in 0..n {
            self.right[i] = self.out_right[i] * FADE;
            self.left[i] = self.out_left[i + 1] * FADE;
        }
    }
}

/// A Kelly-Lochbaum waveguide vocal tract with a nasal branch, in pressure
/// waves: sections of the length sound travels in half a sample, two steps a
/// sample, a three-way junction at the velum, losses along the walls, and
/// reflections at the glottis (0.75) and at the lips and nostrils (-0.85).
#[derive(Debug, Clone)]
struct Tract {
    oral: Tube,
    nose: Tube,
    velum_at: usize,
    target: Vec<f32>,
    diameter: Vec<f32>,
    velum_target: f32,
    velum: f32,
    rest: Vec<f32>,
}

impl Tract {
    fn new(length: f32, sample_rate: u32) -> Tract {
        let section = SPEED_OF_SOUND / (2.0 * sample_rate as f32);
        let n = ((length / section).round() as usize).clamp(8, 180);
        let rest: Vec<f32> = (0..n)
            .map(|i| {
                let x = i as f32 / n as f32;
                if x < 7.0 / 44.0 {
                    0.6
                } else if x < 12.0 / 44.0 {
                    1.1
                } else {
                    1.5
                }
            })
            .collect();
        let nose_n = ((n as f32) * 28.0 / 44.0).round().max(4.0) as usize;
        let mut nose = Tube::new(nose_n);
        for i in 0..nose_n {
            let d = 2.0 * i as f32 / nose_n as f32;
            let d = if d < 1.0 {
                0.4 + 1.6 * d
            } else {
                0.5 + 1.5 * (2.0 - d)
            };
            nose.area[i] = d.min(1.9).powi(2);
        }
        let mut tract = Tract {
            oral: Tube::new(n),
            nose,
            velum_at: n - nose_n + 1,
            target: rest.clone(),
            diameter: rest.clone(),
            velum_target: 0.01,
            velum: 0.01,
            rest,
        };
        tract.update_areas();
        tract
    }

    /// Sets the shape to aim for (Pink Trombone's tongue): `tongue` along
    /// the tract (0 glottis to 1 lips), its `height` (2.05 close to 3.5
    /// open), `lips` open 0 to 1, the `velum` 0 shut to 1 open.
    fn shape(&mut self, tongue: f32, height: f32, lips: f32, velum: f32) {
        let n = self.oral.len();
        let at = |x: f32| (x * n as f32).round() as isize;
        let blade = at(10.0 / 44.0);
        let tip = at(32.0 / 44.0);
        let lip = at(39.0 / 44.0);
        let index = tongue.clamp(0.0, 1.0) * n as f32;
        let fixed = 2.0 + (height.clamp(2.0, 3.6) - 2.0) / 1.5;
        for i in 0..n {
            let ii = i as isize;
            let mut d = self.rest[i];
            if ii >= blade && ii < lip {
                let t = 1.1 * std::f32::consts::PI * (index - i as f32) / (tip - blade) as f32;
                let mut curve = (1.5 - fixed + 1.7) * t.cos();
                if ii == blade - 2 || ii == lip - 1 {
                    curve *= 0.8;
                }
                if ii == blade || ii == lip - 2 {
                    curve *= 0.94;
                }
                d = self.rest[i] - curve;
            }
            if ii >= lip {
                d = 1.5 * lips.clamp(0.0, 1.0);
            }
            self.target[i] = d.max(0.0);
        }
        self.velum_target = 0.01 + 0.39 * velum.clamp(0.0, 1.0);
    }

    /// Moves a step towards the target shape.
    fn settle(&mut self, rate: f32) {
        for (d, t) in self.diameter.iter_mut().zip(&self.target) {
            *d += (t - *d) * rate;
        }
        self.velum += (self.velum_target - self.velum) * rate;
        self.update_areas();
    }

    fn update_areas(&mut self) {
        for (a, d) in self.oral.area.iter_mut().zip(&self.diameter) {
            *a = (d * d).max(1e-4);
        }
        self.nose.area[0] = self.velum.max(1e-4);
    }

    /// One sample: `glottis` drives the lower end. Returns what radiates
    /// from the lips and nostrils.
    fn run(&mut self, glottis: f32) -> f32 {
        let mut out = 0.0;
        for _ in 0..2 {
            out += self.step(glottis);
        }
        0.5 * out
    }

    fn step(&mut self, glottis: f32) -> f32 {
        let n = self.oral.len();
        let v = self.velum_at;
        self.oral.out_right[0] = self.oral.left[0] * GLOTTIS + glottis;
        self.oral.out_left[n] = self.oral.right[n - 1] * LIPS;
        self.oral.scatter(1, Some(v));
        // The velum: three tubes meet.
        let (al, ar, an) = (self.oral.area[v - 1], self.oral.area[v], self.nose.area[0]);
        let p = 2.0
            * (al * self.oral.right[v - 1] + ar * self.oral.left[v] + an * self.nose.left[0])
            / (al + ar + an);
        self.oral.out_right[v] = p - self.oral.left[v];
        self.oral.out_left[v] = p - self.oral.right[v - 1];
        self.nose.out_right[0] = p - self.nose.left[0];
        self.oral.propagate();
        let m = self.nose.len();
        self.nose.out_left[m] = self.nose.right[m - 1] * NOSTRILS;
        self.nose.scatter(1, None);
        self.nose.propagate();
        self.oral.right[n - 1] * (1.0 + LIPS) + self.nose.right[m - 1] * (1.0 + NOSTRILS)
    }
}
