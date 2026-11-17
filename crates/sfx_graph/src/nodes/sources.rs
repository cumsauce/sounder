//! Sources: oscillators, noise, impulses, samples.

use serde::Deserialize;

use super::{OneOrMany, mono, one, positive};
use crate::buf::Buf;
use crate::dsp::{Osc as OscState, Wave};
use crate::node::{Cx, Describe, Eval, In, Outs};

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WaveSpec {
    #[default]
    Sine,
    Saw,
    Square,
    Tri,
    Pulse,
}

impl WaveSpec {
    pub fn wave(self) -> Wave {
        match self {
            WaveSpec::Sine => Wave::Sine,
            WaveSpec::Saw => Wave::Saw,
            WaveSpec::Square => Wave::Square,
            WaveSpec::Tri => Wave::Tri,
            WaveSpec::Pulse => Wave::Pulse,
        }
    }
}

/// The most unison voices one `osc` runs.
const MAX_UNISON: u32 = 16;
/// The widest `detune`, cents (one octave each way).
const MAX_DETUNE: f32 = 1200.0;

/// A band-limited oscillator.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Osc {
    /// sine (default), saw, square, tri, pulse.
    #[serde(default)]
    pub shape: WaveSpec,
    /// Hz; a node here sweeps or frequency-modulates.
    pub freq: In,
    /// Start phase, 0..1 (default 0). With `unison` > 1 only the first voice
    /// starts here; the others start at seeded random phases.
    #[serde(default)]
    pub phase: Option<f32>,
    /// Pulse width 0..1 (pulse only; default 0.5).
    #[serde(default)]
    pub width: Option<In>,
    /// Linear amplitude (default 1). Unison voices are summed, not averaged:
    /// `unison` 7 is about sqrt(7) times louder than one voice.
    #[serde(default)]
    pub amp: Option<In>,
    /// Hard sync: a mono signal (the master); every rising zero crossing
    /// resets this oscillator's phase, band-limited with polyBLEP at the
    /// reset. The master crossing is placed to a fraction of a sample.
    #[serde(default)]
    pub sync: Option<In>,
    /// Voices stacked on `freq`, 1..16 (default 1). More than 1 makes the
    /// node random (seeded start phases).
    #[serde(default)]
    pub unison: Option<u32>,
    /// Unison detune, cents 0..1200 (default 0): the voices are spread evenly
    /// from -detune to +detune around `freq`.
    #[serde(default)]
    pub detune: Option<f32>,
    /// Stereo width of the unison voices, 0..1 (default 0 = mono). Above 0
    /// the output is stereo: voices alternate left and right, the outermost
    /// hard-panned at 1 (a balance law: a centred voice is unchanged in both
    /// channels).
    #[serde(default)]
    pub spread: Option<f32>,
    /// Phase modulation, in cycles (1 = a full turn), a signal: a node here
    /// adds to the phase, so an `osc` into `pm` is FM.
    #[serde(default)]
    pub pm: Option<In>,
    /// Mixed with the document seed and the node name (default 0); picks the
    /// unison start phases.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Osc {
    const BLURB: &'static str = "band-limited oscillator (sine/saw/square/tri/pulse); freq may be a node (pitch sweeps, FM); sync (hard sync from a master signal), unison + detune + spread (supersaw), pm (phase modulation)";
    fn example() -> serde_json::Value {
        serde_json::json!({"shape": "saw", "freq": "sweep", "amp": 0.5})
    }
}

/// The unison voice `i` of `unison`: its detune offset in cents and its pan
/// position -1..1 (voices alternate sides, the outermost first).
fn voice_layout(i: u32, unison: u32, detune: f32, spread: f32) -> (f32, f32) {
    if unison < 2 {
        return (0.0, 0.0);
    }
    let last = (unison - 1) as f32;
    let cents = detune * (2.0 * i as f32 / last - 1.0);
    let j = if i.is_multiple_of(2) {
        i / 2
    } else {
        unison - 1 - i / 2
    };
    (cents, spread * (2.0 * j as f32 / last - 1.0))
}

impl Eval for Osc {
    fn random(&self) -> bool {
        self.unison.unwrap_or(1) > 1
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let unison = self.unison.unwrap_or(1);
        if !(1..=MAX_UNISON).contains(&unison) {
            return Err(format!(
                "unison: {unison}: give a whole number from 1 to {MAX_UNISON}"
            ));
        }
        let detune = self.detune.unwrap_or(0.0);
        if !(detune.is_finite() && (0.0..=MAX_DETUNE).contains(&detune)) {
            return Err(format!(
                "detune: {detune}: give cents from 0 to {MAX_DETUNE}"
            ));
        }
        let spread = self.spread.unwrap_or(0.0);
        if !(spread.is_finite() && (0.0..=1.0).contains(&spread)) {
            return Err(format!("spread: {spread}: give a number from 0 to 1"));
        }
        let freq = cx.mono("freq", &self.freq)?;
        let width = cx.mono_or("width", &self.width, 0.5)?;
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let pm = self.pm.as_ref().map(|p| cx.mono("pm", p)).transpose()?;
        let sync = self.sync.as_ref().map(|s| cx.mono("sync", s)).transpose()?;
        let crossings: Option<Vec<Option<f32>>> = sync.map(|s| {
            let mut prev = s.at(0);
            (0..cx.n)
                .map(|i| {
                    let cur = s.at(i);
                    let hit = i > 0 && prev <= 0.0 && cur > 0.0;
                    let f = hit.then(|| (-prev / (cur - prev)).clamp(0.0, 1.0));
                    prev = cur;
                    f
                })
                .collect()
        });
        let start = self.phase.unwrap_or(0.0);
        let mut rng = cx.rng(self.seed);
        let stereo = spread > 0.0;
        let mut left = vec![0.0f32; cx.n];
        let mut right = if stereo {
            vec![0.0f32; cx.n]
        } else {
            Vec::new()
        };
        let plain = pm.is_none() && crossings.is_none();
        let mut voice = vec![0.0f32; cx.n];
        for v in 0..unison {
            let (cents, pos) = voice_layout(v, unison, detune, spread);
            let ratio = if cents == 0.0 {
                1.0
            } else {
                (cents / 1200.0).exp2()
            };
            let ph = if v == 0 { start } else { start + rng.unit() };
            let mut o = OscState::new(self.shape.wave(), ph);
            for i in 0..cx.n {
                let hz = freq.at(i) * ratio;
                if plain {
                    voice[i] = o.tick(hz, width.at(i), cx.sr);
                } else {
                    let p = pm.as_ref().map_or(0.0, |p| p.at(i));
                    let sy = crossings.as_ref().and_then(|c| c[i]);
                    let (x, fix) = o.tick_ex(hz, width.at(i), cx.sr, p, sy);
                    voice[i] = x;
                    if fix != 0.0 && i > 0 {
                        voice[i - 1] += fix;
                    }
                }
            }
            let (gl, gr) = if stereo {
                ((1.0 - pos).min(1.0), (1.0 + pos).min(1.0))
            } else {
                (1.0, 0.0)
            };
            for (i, x) in voice.iter().enumerate() {
                left[i] += x * gl;
            }
            if stereo {
                for (i, x) in voice.iter().enumerate() {
                    right[i] += x * gr;
                }
            }
        }
        for (i, l) in left.iter_mut().enumerate() {
            *l *= amp.at(i);
        }
        if !stereo {
            return Ok(one(Buf::Audio(left)));
        }
        for (i, r) in right.iter_mut().enumerate() {
            *r *= amp.at(i);
        }
        Ok(one(Buf::from_channels(vec![left, right])))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NoiseColor {
    #[default]
    White,
    Pink,
    Brown,
    Velvet,
    Blue,
    Violet,
    Crackle,
}

/// Scale of the differentiated pink stream so blue noise has about the
/// level of violet (measured: tests/s1.rs).
const BLUE_GAIN: f32 = 3.5;

/// Seeded noise.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Noise {
    /// white (default), pink (-3 dB/oct), brown (-6 dB/oct), blue (+3 dB/oct),
    /// violet (+6 dB/oct), velvet (one random-sign impulse per cell: smooth,
    /// cheap reverb-like texture), crackle (random-sign, random-height
    /// impulses at random, irregular times).
    #[serde(default)]
    pub color: NoiseColor,
    /// Velvet and crackle: impulses a second (default 2000 for velvet, 50 for
    /// crackle); a number or a signal. Velvet above the sample rate is
    /// capped at one a sample.
    #[serde(default)]
    pub density: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Linear amplitude (default 1).
    #[serde(default)]
    pub amp: Option<In>,
}

impl Describe for Noise {
    const BLURB: &'static str = "white/pink/brown/blue/violet noise, velvet and crackle impulses (seeded); density (a signal) sets the impulse rate";
    fn example() -> serde_json::Value {
        serde_json::json!({"color": "pink", "seed": 3})
    }
}

/// Paul Kellet's refined pink filter (his published constants), one sample.
#[allow(clippy::excessive_precision)]
fn pink_tick(b: &mut [f32; 7], w: f32) -> f32 {
    b[0] = 0.99886 * b[0] + w * 0.0555179;
    b[1] = 0.99332 * b[1] + w * 0.0750759;
    b[2] = 0.96900 * b[2] + w * 0.1538520;
    b[3] = 0.86650 * b[3] + w * 0.3104856;
    b[4] = 0.55000 * b[4] + w * 0.5329522;
    b[5] = -0.7616 * b[5] - w * 0.0168980;
    let p = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
    b[6] = w * 0.115926;
    p * 0.11
}

impl Eval for Noise {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let mut rng = cx.rng(self.seed);
        let n = cx.n;
        let buf = match self.color {
            NoiseColor::White => mono(n, |i| rng.signed() * amp.at(i)),
            NoiseColor::Pink => {
                let mut b = [0.0f32; 7];
                mono(n, |i| {
                    let w = rng.signed();
                    pink_tick(&mut b, w) * amp.at(i)
                })
            }
            NoiseColor::Brown => {
                let mut y = 0.0f32;
                mono(n, |i| {
                    y = (y + 0.02 * rng.signed()) / 1.02;
                    y * 3.5 * amp.at(i)
                })
            }
            // The first difference adds +6 dB/oct (|1 - z^-1| = 2 sin(pi f / sr)).
            NoiseColor::Violet => {
                let mut prev = 0.0f32;
                mono(n, |i| {
                    let w = rng.signed();
                    let y = 0.5 * (w - prev);
                    prev = w;
                    y * amp.at(i)
                })
            }
            NoiseColor::Blue => {
                let mut b = [0.0f32; 7];
                let mut prev = 0.0f32;
                mono(n, |i| {
                    let p = pink_tick(&mut b, rng.signed());
                    let y = BLUE_GAIN * (p - prev);
                    prev = p;
                    y * amp.at(i)
                })
            }
            NoiseColor::Velvet => {
                let density = self.rate(cx, 2000.0)?;
                let mut v = vec![0.0f32; n];
                let mut k = 0.0f32;
                while (k as usize) < n {
                    let d = density.at(k as usize);
                    if d <= 0.0 || !d.is_finite() {
                        // No impulses while the rate is zero: look again next sample.
                        k += 1.0;
                        continue;
                    }
                    let period = (cx.sr / d).max(1.0);
                    let at = (k + rng.unit() * period) as usize;
                    let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                    if at < n {
                        v[at] = sign * amp.at(at);
                    }
                    k += period;
                }
                Buf::Audio(v)
            }
            NoiseColor::Crackle => {
                // A Poisson process: each sample holds an impulse with
                // probability density / sr (so the rate can vary), of random
                // sign and a height between 0.1 and 1.
                let density = self.rate(cx, 50.0)?;
                let mut v = vec![0.0f32; n];
                for (i, o) in v.iter_mut().enumerate() {
                    let p = (density.at(i) / cx.sr).clamp(0.0, 1.0);
                    if rng.unit() < p {
                        let h = 0.1 + 0.9 * rng.unit();
                        let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                        *o = sign * h * amp.at(i);
                    }
                }
                Buf::Audio(v)
            }
        };
        Ok(one(buf))
    }
}

impl Noise {
    /// The impulse rate: `density`, or `default`; a number must be > 0.
    fn rate(&self, cx: &Cx, default: f32) -> Result<crate::buf::Sig, String> {
        match &self.density {
            Some(d) => {
                let s = cx.mono("density", d)?;
                if let Some(c) = s.constant() {
                    positive("density", c)?;
                }
                Ok(s)
            }
            None => Ok(crate::buf::Sig::Const(default)),
        }
    }
}

/// Single-sample clicks.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Impulse {
    /// `at`: seconds into the sound, one number (`0.1`) or a list
    /// (`[0, 0.25, 0.5]`) for several clicks (default 0).
    #[serde(default)]
    pub at: Option<OneOrMany>,
    /// Height of each click (default 1).
    #[serde(default)]
    pub amp: Option<f32>,
}

impl Describe for Impulse {
    const BLURB: &'static str = "single-sample clicks: the field is `at`, one time or a list of times in seconds (excites resonators, comb, reverb)";
    fn example() -> serde_json::Value {
        serde_json::json!({"at": [0.0, 0.25]})
    }
}

impl Eval for Impulse {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let mut v = vec![0.0f32; cx.n];
        let times = self.at.as_ref().map_or(vec![0.0], OneOrMany::values);
        for t in times {
            let i = cx.at(t);
            if i < cx.n {
                v[i] += self.amp.unwrap_or(1.0);
            }
        }
        Ok(one(Buf::Audio(v)))
    }
}

/// A WAV file, played from `start` at `rate`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// A WAV in the project (relative paths resolve against the project).
    pub file: String,
    /// Seconds into the file to start from (default 0).
    #[serde(default)]
    pub start: Option<f32>,
    /// Playback rate: 2 = an octave up and twice as fast (default 1; may be a
    /// signal).
    #[serde(default)]
    pub rate: Option<In>,
    /// Seconds into the sound the file starts playing (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Linear amplitude (default 1).
    #[serde(default)]
    pub amp: Option<f32>,
    /// Wrap around at the end of the file instead of falling silent.
    #[serde(default, rename = "loop")]
    pub looping: bool,
}

impl Describe for Sample {
    const BLURB: &'static str = "a WAV file from the project, at a playback rate";
    fn example() -> serde_json::Value {
        serde_json::json!({"file": "refs/knock.wav", "rate": 1.2, "start": 0.0})
    }
}

impl Eval for Sample {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let path = cx.path(&self.file);
        let audio = crate::wav::read(&path)
            .map_err(|e| format!("file: {e} (put the WAV in the project and give its path)"))?;
        let rate = cx.mono_or("rate", &self.rate, 1.0)?;
        let step = audio.sample_rate as f64 / f64::from(cx.sr);
        let len = audio.len();
        let begin = cx.at(self.at.unwrap_or(0.0));
        let amp = self.amp.unwrap_or(1.0);
        let start = f64::from(self.start.unwrap_or(0.0).max(0.0)) * audio.sample_rate as f64;
        let chans: Vec<Vec<f32>> = audio
            .channels
            .iter()
            .map(|src| {
                let mut out = vec![0.0f32; cx.n];
                let mut pos = start;
                for (i, o) in out.iter_mut().enumerate().skip(begin) {
                    let mut p = pos;
                    if self.looping && len > 0 {
                        p = p.rem_euclid(len as f64);
                    }
                    if p < 0.0 || p >= (len.max(1) - 1) as f64 {
                        if !self.looping {
                            break;
                        }
                    } else {
                        let k = p.floor() as usize;
                        let f = (p - k as f64) as f32;
                        let a = src[k];
                        let b = src[(k + 1).min(len - 1)];
                        *o = (a + (b - a) * f) * amp;
                    }
                    pos += step * f64::from(rate.at(i));
                }
                out
            })
            .collect();
        Ok(one(Buf::from_channels(chans)))
    }
}
