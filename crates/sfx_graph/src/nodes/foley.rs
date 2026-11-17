//! Physical models wrapped from `foley` (the user's DSP repo): a water
//! bubble, air absorption, a glottis and a vocal tract, a bird's syrinx and
//! thunder.
//!
//! Where foley renders a finished one-shot (`birdsong2`, `strike`), the node
//! returns that buffer placed at `at` and fitted to the document's length.
//! Foley's waveguide `Tract` is private, so `tract` here is a formant
//! cascade (Klatt resonators) driven by foley's public LF glottal pulse.

use std::f32::consts::TAU;
use std::sync::OnceLock;

use foley_synth::{Birds, Bubble, LowPass, Pulse, absorption_cutoff, birdsong2, lf_pulse, strike};
use serde::Deserialize;

use super::{mono, non_negative, one, per_channel, positive};
use crate::buf::Buf;
use crate::dsp::{FilterMode, Svf};
use crate::node::{Cx, Eval, In, Outs};

/// foley takes whole-number sample rates.
fn rate(cx: &Cx) -> u32 {
    (cx.sr.round() as u32).max(1)
}

/// `src` placed at `at` seconds, scaled by `gain`, cut or zero-padded to the
/// document's length. A cut gets a 20 ms fade so it does not click.
fn fit(src: &[f32], at: f32, gain: f32, cx: &Cx) -> Buf {
    let start = cx.at(at);
    let mut v = vec![0.0f32; cx.n];
    let room = cx.n - start;
    let take = src.len().min(room);
    for (o, s) in v[start..start + take].iter_mut().zip(src) {
        *o = s * gain;
    }
    if src.len() > room {
        let fade = ((0.02 * cx.sr) as usize).min(take);
        for k in 0..fade {
            v[start + take - 1 - k] *= k as f32 / fade as f32;
        }
    }
    Buf::Audio(v)
}

// ---------------------------------------------------------------- bubble

/// A water bubble ringing as it settles (foley's `Bubble`, van den Doel's
/// model): a sine at the Minnaert frequency `f = 3.29 kHz / radius_mm`,
/// decaying at pi f 0.045 a second, rising in pitch as it nears the surface,
/// louder the bigger it is (amplitude ~ r^1.5). Radii under about 0.7 mm
/// ring above Nyquist at 48 kHz and alias. The radius is a number (foley
/// cannot retune a ringing bubble): in `voices`, give it a `$rand` expression.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BubbleNode {
    /// Bubble radius, millimetres (> 0). 1 mm rings at 3.3 kHz, 4 mm at 820 Hz.
    pub radius_mm: f32,
    /// Level at 1 mm (default 0.3); bigger bubbles scale by r^1.5.
    #[serde(default)]
    pub loudness: Option<f32>,
    /// Pitch rise as it nears the surface, in units of its decay rate over
    /// a second (default 0.1; foley uses less for a raindrop's bubble).
    #[serde(default)]
    pub rise: Option<f32>,
    /// Seconds into the sound the bubble is born (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Random +- fraction on the radius, 0..0.9 (default 0: exact).
    #[serde(default)]
    pub jitter: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Eval for BubbleNode {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let radius_mm = positive("radius_mm", self.radius_mm)?;
        let jitter = self.jitter.unwrap_or(0.0).clamp(0.0, 0.9);
        let loudness = non_negative("loudness", self.loudness.unwrap_or(0.3))?;
        let rise = self.rise.unwrap_or(0.1);
        if !rise.is_finite() {
            return Err(format!("rise: {rise}: give a number"));
        }
        let mut radius = radius_mm * 1e-3;
        if jitter > 0.0 {
            radius *= 1.0 + jitter * cx.rng(self.seed).signed();
        }
        let mut bubble = Bubble::new_at(radius, loudness, rate(cx)).with_rise(rise);
        let start = cx.at(self.at.unwrap_or(0.0));
        let mut v = vec![0.0f32; cx.n];
        for o in v.iter_mut().skip(start) {
            if bubble.done() {
                break;
            }
            *o = bubble.sample();
        }
        Ok(one(Buf::Audio(v)))
    }
}

// ------------------------------------------------------------------- air

/// Distance through air: the highs go (ISO 9613-1 absorption, via foley's
/// `absorption_cutoff`: the frequency where `distance` metres cost 6 dB,
/// used as a 12 dB/octave low-pass; none when the loss at 20 kHz is under
/// 6 dB) and the level falls as 1 / distance (spherical spreading from 1 m;
/// nothing is added inside 1 m). Both only ever fall as the distance grows.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Air {
    pub input: In,
    /// Metres from the listener (a node animates it: a fly-by). 0 = no loss.
    pub distance: In,
    /// Relative humidity, percent (default 50). Dry air absorbs more treble.
    #[serde(default)]
    pub humidity: Option<f32>,
    /// Air temperature, Celsius (default 15).
    #[serde(default)]
    pub celsius: Option<f32>,
    /// Apply the 1 / distance level drop (default true); false = filter only.
    #[serde(default)]
    pub level: Option<bool>,
}

impl Eval for Air {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let distance = cx.mono("distance", &self.distance)?;
        let humidity = self.humidity.unwrap_or(50.0);
        let celsius = self.celsius.unwrap_or(15.0);
        if !(0.0..=100.0).contains(&humidity) {
            return Err(format!("humidity: {humidity}: give a percentage 0..100"));
        }
        if !celsius.is_finite() || !(-60.0..=60.0).contains(&celsius) {
            return Err(format!("celsius: {celsius}: give a temperature -60..60"));
        }
        let level = self.level.unwrap_or(true);
        // Cutoffs and gains once per block of samples (a bisection each).
        const BLOCK: usize = 32;
        let blocks = cx.n / BLOCK + 1;
        let mut cutoff = Vec::with_capacity(blocks);
        let mut gain = Vec::with_capacity(blocks);
        let mut last: Option<(f32, f32, f32)> = None;
        for b in 0..blocks {
            let d = distance
                .at((b * BLOCK).min(cx.n.saturating_sub(1)))
                .max(0.0);
            let d = if d.is_finite() { d } else { 0.0 };
            let (c, g) = match last {
                Some((ld, c, g)) if ld == d => (c, g),
                _ => {
                    let c = absorption_cutoff(d, celsius, humidity).unwrap_or(f32::MAX);
                    let g = if level { 1.0 / d.max(1.0) } else { 1.0 };
                    last = Some((d, c, g));
                    (c, g)
                }
            };
            cutoff.push(c);
            gain.push(g);
        }
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = Svf::default();
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let c = cutoff[i / BLOCK];
                    // Keep the filter's state warm even when it is bypassed.
                    let y = f.tick(
                        *v,
                        FilterMode::Lp,
                        c.min(0.45 * cx.sr),
                        std::f32::consts::FRAC_1_SQRT_2,
                        0.0,
                        cx.sr,
                    );
                    let y = if c == f32::MAX { *v } else { y };
                    y * gain[i / BLOCK]
                })
                .collect()
        })))
    }
}

// -------------------------------------------------------------- glottis

const TABLES: usize = 25;
const RD_MIN: f32 = 0.3;
const RD_MAX: f32 = 2.7;

/// foley's LF pulses for Rd 0.3 (pressed) .. 2.7 (breathy), built once.
fn pulses() -> &'static [Pulse] {
    static T: OnceLock<Vec<Pulse>> = OnceLock::new();
    T.get_or_init(|| {
        (0..TABLES)
            .map(|i| lf_pulse(RD_MIN + (RD_MAX - RD_MIN) * i as f32 / (TABLES - 1) as f32))
            .collect()
    })
}

/// (flow derivative, flow) at `phase` 0..1 of a period with `rd`.
fn lf_at(rd: f32, phase: f32) -> (f32, f32) {
    let t = pulses();
    let x = ((rd.clamp(RD_MIN, RD_MAX) - RD_MIN) / (RD_MAX - RD_MIN)) * (TABLES - 1) as f32;
    let i = (x as usize).min(TABLES - 2);
    let w = x - i as f32;
    let len = t[0].derivative.len();
    let p = phase.rem_euclid(1.0) * len as f32;
    let k = (p as usize).min(len - 1);
    let f = p - k as f32;
    let k2 = (k + 1) % len;
    let read = |tab: &[f32]| tab[k] + (tab[k2] - tab[k]) * f;
    let d = read(&t[i].derivative) * (1.0 - w) + read(&t[i + 1].derivative) * w;
    let fl = read(&t[i].flow) * (1.0 - w) + read(&t[i + 1].flow) * w;
    (d, fl)
}

/// A voice source: foley's Liljencrants-Fant glottal flow derivative (what
/// excites a vocal tract) at `f0`, with breath noise and vibrato. Feed it to
/// `tract` for vowels. Mono. Above ~1.5 kHz the sharp return phase aliases.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Glottis {
    /// Pitch, Hz (a node sweeps it: intonation, a creature's call).
    pub f0: In,
    /// 0..1: 0 breathy and soft (Rd 2.7), 1 pressed and bright (Rd 0.3).
    /// Default 0.65 (modal). A signal.
    #[serde(default)]
    pub tension: Option<In>,
    /// Aspiration noise level 0..1 (default 0.05), seeded. A signal.
    #[serde(default)]
    pub breath: Option<In>,
    /// Vibrato depth, cents peak (default 0).
    #[serde(default)]
    pub vibrato_cents: Option<f32>,
    /// Vibrato rate, Hz (default 5.5).
    #[serde(default)]
    pub vibrato_hz: Option<f32>,
    /// Linear amplitude (default 1; a signal gates and shapes the voice).
    #[serde(default)]
    pub amp: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Eval for Glottis {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let f0 = cx.mono("f0", &self.f0)?;
        let tension = cx.mono_or("tension", &self.tension, 0.65)?;
        let breath = cx.mono_or("breath", &self.breath, 0.05)?;
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let cents = self.vibrato_cents.unwrap_or(0.0);
        let vib_hz = non_negative("vibrato_hz", self.vibrato_hz.unwrap_or(5.5))?;
        if !cents.is_finite() {
            return Err(format!("vibrato_cents: {cents}: give a number"));
        }
        let mut rng = cx.rng(self.seed);
        let mut phase = 0.0f32;
        let mut vib = 0.0f32;
        Ok(one(mono(cx.n, |i| {
            let t = tension.at(i).clamp(0.0, 1.0);
            let rd = RD_MAX - (RD_MAX - RD_MIN) * t;
            let bend = if cents == 0.0 {
                1.0
            } else {
                vib = (vib + vib_hz / cx.sr).fract();
                (cents / 1200.0 * (TAU * vib).sin()).exp2()
            };
            let hz = (f0.at(i) * bend).clamp(1.0, 0.25 * cx.sr);
            phase = (phase + hz / cx.sr).fract();
            let (d, flow) = lf_at(rd, phase);
            let noise = rng.signed() * breath.at(i).clamp(0.0, 1.0) * (0.3 + 0.7 * flow);
            (0.5 * d + 0.5 * noise) * amp.at(i)
        })))
    }
}

// ---------------------------------------------------------------- tract

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VowelSpec {
    /// "father" (730 / 1090 / 2440 Hz).
    #[default]
    A,
    /// "bed" (530 / 1840 / 2480).
    E,
    /// "beet" (270 / 2290 / 3010).
    I,
    /// "bought" (570 / 840 / 2410).
    O,
    /// "boot" (300 / 870 / 2240).
    U,
    /// "bat" (660 / 1720 / 2410).
    Ae,
    /// "but" (640 / 1190 / 2390).
    Uh,
    /// "about" (500 / 1500 / 2500).
    Schwa,
}

impl VowelSpec {
    /// F1..F4 and bandwidths, Hz (Peterson & Barney 1952 male means; F4 and
    /// the bandwidths are typical values).
    fn formants(self) -> [[f32; 2]; 4] {
        let f = match self {
            VowelSpec::A => [730.0, 1090.0, 2440.0],
            VowelSpec::E => [530.0, 1840.0, 2480.0],
            VowelSpec::I => [270.0, 2290.0, 3010.0],
            VowelSpec::O => [570.0, 840.0, 2410.0],
            VowelSpec::U => [300.0, 870.0, 2240.0],
            VowelSpec::Ae => [660.0, 1720.0, 2410.0],
            VowelSpec::Uh => [640.0, 1190.0, 2390.0],
            VowelSpec::Schwa => [500.0, 1500.0, 2500.0],
        };
        [[f[0], 70.0], [f[1], 90.0], [f[2], 130.0], [3600.0, 200.0]]
    }
}

/// Klatt's resonator coefficients (unity gain at DC).
fn reson(f: f32, bw: f32, sr: f32) -> (f32, f32, f32) {
    let c = -(-TAU * bw / sr).exp();
    let b = 2.0 * (-std::f32::consts::PI * bw / sr).exp() * (TAU * f / sr).cos();
    (1.0 - b - c, b, c)
}

/// Overall level so a vowel from `glottis` peaks around 0.5.
const TRACT_MAKEUP: f32 = 0.8;

/// A vocal tract: the input (a `glottis`, or any buzz or noise) through four
/// formant resonators. Foley's waveguide tract is private, so this is a
/// Klatt cascade at the Peterson-Barney vowel formants; `vowel_to` / `morph` glide
/// between two vowels (diphthongs, a creature's "wah"), `scale` shrinks the
/// tract (> 1 = shorter: a child, a small creature) or lengthens it (< 1: a
/// giant). Mono.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tract {
    pub input: In,
    /// a (default), e, i, o, u, ae, uh, schwa.
    #[serde(default)]
    pub vowel: VowelSpec,
    /// Glide target vowel (with `morph`).
    #[serde(default)]
    pub vowel_to: Option<VowelSpec>,
    /// 0 = `vowel`, 1 = `vowel_to` (default 0; a signal).
    #[serde(default)]
    pub morph: Option<In>,
    /// Explicit formants `[[hz, bandwidth_hz], ...]` (1..6) instead of a
    /// vowel.
    #[serde(default)]
    pub formants: Option<Vec<[f32; 2]>>,
    /// Formant frequency multiplier (default 1; a signal). 1.2 = a 20 % shorter tract.
    #[serde(default)]
    pub scale: Option<In>,
}

impl Eval for Tract {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let morph = cx.mono_or("morph", &self.morph, 0.0)?;
        let scale = cx.mono_or("scale", &self.scale, 1.0)?;
        let from: Vec<[f32; 2]> = match &self.formants {
            Some(f) => {
                if f.is_empty() || f.len() > 6 {
                    return Err("formants: give 1..6 pairs [hz, bandwidth_hz]".into());
                }
                for [hz, bw] in f {
                    positive("formants hz", *hz)?;
                    positive("formants bandwidth", *bw)?;
                }
                if self.vowel_to.is_some() {
                    return Err("formants and vowel_to together: morph between presets or give a list, not both".into());
                }
                f.clone()
            }
            None => self.vowel.formants().to_vec(),
        };
        let to: Vec<[f32; 2]> = match self.vowel_to {
            Some(v) => v.formants().to_vec(),
            None => from.clone(),
        };
        let k = from.len();
        let mut state = vec![[0.0f32; 2]; k];
        let mut coef = vec![(0.0f32, 0.0f32, 0.0f32); k];
        const BLOCK: usize = 16;
        Ok(one(mono(cx.n, |i| {
            if i % BLOCK == 0 || i == 0 {
                let m = morph.at(i).clamp(0.0, 1.0);
                let s = scale.at(i).clamp(0.1, 10.0);
                for j in 0..k {
                    let f = from[j][0] * (to[j][0] / from[j][0]).powf(m) * s;
                    let bw = from[j][1] + (to[j][1] - from[j][1]) * m;
                    // Formants past Nyquist go flat instead of aliasing.
                    coef[j] = if f < 0.45 * cx.sr {
                        reson(f, bw * s.sqrt(), cx.sr)
                    } else {
                        (1.0, 0.0, 0.0)
                    };
                }
            }
            let mut x = input.at(i);
            for (st, (a, b, c)) in state.iter_mut().zip(&coef) {
                let y = a * x + b * st[0] + c * st[1];
                st[1] = st[0];
                st[0] = if y.is_finite() { y } else { 0.0 };
                x = st[0];
            }
            x * TRACT_MAKEUP
        })))
    }
}

// --------------------------------------------------------------- syrinx

/// `Birds` variant names in snake_case, as written in `species`.
fn species_name(b: Birds) -> String {
    let dbg = format!("{b:?}");
    let mut out = String::new();
    for (i, ch) in dbg.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

fn species_of(name: &str) -> Result<Birds, String> {
    let want = name.to_ascii_lowercase().replace(['-', ' '], "_");
    Birds::ALL
        .iter()
        .copied()
        .find(|b| species_name(*b) == want)
        .ok_or_else(|| {
            let all: Vec<String> = Birds::ALL.iter().map(|b| species_name(*b)).collect();
            format!(
                "species: unknown `{name}`: one of {} (or give `pitch` for a whistle)",
                all.join(", ")
            )
        })
}

/// A bird. Either `species`: foley's finished call or song for one of 57
/// species (a phrase of its own length, placed at `at` and cut or padded to
/// the document; each `seed` is another individual), or `pitch`: a whistle
/// from a pitch contour you draw (a signal: `ramp`, `env`, `lfo`...), a
/// sine with a little second harmonic and a slow waver, as foley's songbirds
/// are made. Shape a whistle's loudness with `amp` (a `decay` or `adsr`).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Syrinx {
    /// A foley species, snake_case: crow, dove, robin, cardinal, chickadee,
    /// herring_gull, common_loon, jay, warbler, wood_thrush, common_raven,
    /// boreal_owl, ... (the error lists all 57).
    #[serde(default)]
    pub species: Option<String>,
    /// Whistle pitch, Hz (a signal; 0 rests). Instead of `species`.
    #[serde(default)]
    pub pitch: Option<In>,
    /// Whistle: level of the second harmonic (default 0.15).
    #[serde(default)]
    pub harmonic: Option<f32>,
    /// Whistle: random pitch waver, fraction of the pitch (default 0.04).
    #[serde(default)]
    pub waver: Option<f32>,
    /// Species: playback rate, 1 = as made; 2 = an octave up and twice as
    /// fast (a smaller bird). Default 1.
    #[serde(default)]
    pub rate: Option<f32>,
    /// Species: seconds into the sound the call starts (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Linear amplitude (default 1; a signal for a whistle).
    #[serde(default)]
    pub amp: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Eval for Syrinx {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        self.pitch.is_some()
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let mut rng = cx.rng(self.seed);
        match (&self.species, &self.pitch) {
            (Some(name), None) => {
                let bird = species_of(name)?;
                let play = positive("rate", self.rate.unwrap_or(1.0))?;
                let amp = match &self.amp {
                    None => 1.0,
                    Some(a) => cx
                        .mono("amp", a)?
                        .constant()
                        .ok_or("amp: a species call takes a number, not a signal")?,
                };
                let call = birdsong2(bird, rng.next_u64(), rate(cx));
                let call = if (play - 1.0).abs() < 1e-6 {
                    call
                } else {
                    let len = (call.len() as f32 / play) as usize;
                    (0..len)
                        .map(|i| {
                            let p = i as f32 * play;
                            let k = p as usize;
                            let f = p - k as f32;
                            let a = call.get(k).copied().unwrap_or(0.0);
                            let b = call.get(k + 1).copied().unwrap_or(0.0);
                            a + (b - a) * f
                        })
                        .collect()
                };
                Ok(one(fit(&call, self.at.unwrap_or(0.0), amp, cx)))
            }
            (None, Some(p)) => {
                let pitch = cx.mono("pitch", p)?;
                let amp = cx.mono_or("amp", &self.amp, 1.0)?;
                let harmonic = self.harmonic.unwrap_or(0.15);
                let waver = self.waver.unwrap_or(0.04);
                let mut slow = LowPass::new_at(25.0, rate(cx));
                let mut phase = 0.0f32;
                Ok(one(mono(cx.n, |i| {
                    let hz = pitch.at(i).max(0.0);
                    let f = hz * (1.0 + waver * slow.process(rng.signed()));
                    phase = (phase + TAU * f.clamp(0.0, 0.45 * cx.sr) / cx.sr) % TAU;
                    if hz <= 0.0 {
                        0.0
                    } else {
                        (phase.sin() + harmonic * (2.0 * phase).sin()) * amp.at(i)
                    }
                })))
            }
            _ => Err(format!(
                "give exactly one of species (a foley call, e.g. \"robin\") or pitch (a whistle's contour){}",
                if self.species.is_some() {
                    ": both were given"
                } else {
                    ""
                }
            )),
        }
    }
}

// -------------------------------------------------------------- thunder

/// One flash of thunder (foley's `strike`, the Ribner-Roy N-wave model: a
/// jagged lightning channel 2.5-4 km tall, each segment a clap or a rumble
/// depending on how it faces you, air absorption over each part's distance).
/// The first sound arrives after `distance / 343` seconds, so a distant flash
/// is silent for a while: set the document `duration` to fit (a 1.5 km flash
/// is about 4 s of quiet then 10+ s of rumble). Output is cut at the
/// document's length with a 20 ms fade. Foley levels every flash to a
/// similar peak (about -8 dBFS): distance changes the delay, the brightness
/// and the length of the rumble, not the loudness; set that with `amp`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Thunder {
    /// Metres from the base of the channel to the listener (default 1500).
    #[serde(default)]
    pub distance: Option<f32>,
    /// Air temperature, Celsius (default 15).
    #[serde(default)]
    pub celsius: Option<f32>,
    /// Relative humidity, percent (default 90).
    #[serde(default)]
    pub humidity: Option<f32>,
    /// 1 = in the open, 0 = shut in, where only the lows come through
    /// (a low-pass from 20 kHz down to 200 Hz). Default 1.
    #[serde(default)]
    pub exposure: Option<f32>,
    /// Seconds into the sound the flash happens (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Linear amplitude (default 1).
    #[serde(default)]
    pub amp: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Eval for Thunder {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let distance = non_negative("distance", self.distance.unwrap_or(1500.0))?;
        if distance > 30_000.0 {
            return Err(format!(
                "distance: {distance}: thunder is not heard past ~30 km; give metres up to 30000"
            ));
        }
        let celsius = self.celsius.unwrap_or(15.0);
        let humidity = self.humidity.unwrap_or(90.0);
        if !celsius.is_finite() || !(-60.0..=60.0).contains(&celsius) {
            return Err(format!("celsius: {celsius}: give a temperature -60..60"));
        }
        if !(0.0..=100.0).contains(&humidity) {
            return Err(format!("humidity: {humidity}: give a percentage 0..100"));
        }
        let exposure = self.exposure.unwrap_or(1.0).clamp(0.0, 1.0);
        let seed = cx.rng(self.seed).next_u64();
        let mut x = strike([distance, 0.0, 0.0], seed, celsius, humidity, rate(cx));
        for v in &mut x {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        if exposure < 1.0 {
            let cutoff = 20_000.0 * (200.0f32 / 20_000.0).powf(1.0 - exposure);
            let mut lp = LowPass::new_at(cutoff, rate(cx));
            for v in &mut x {
                *v = lp.process(*v);
            }
        }
        Ok(one(fit(
            &x,
            self.at.unwrap_or(0.0),
            self.amp.unwrap_or(1.0),
            cx,
        )))
    }
}
