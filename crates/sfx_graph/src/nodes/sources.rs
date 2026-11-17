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

/// A band-limited oscillator.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Osc {
    /// sine (default), saw, square, tri, pulse.
    #[serde(default)]
    pub shape: WaveSpec,
    /// Hz; a node here sweeps or frequency-modulates.
    pub freq: In,
    /// Start phase, 0..1 (default 0).
    #[serde(default)]
    pub phase: Option<f32>,
    /// Pulse width 0..1 (pulse only; default 0.5).
    #[serde(default)]
    pub width: Option<In>,
    /// Linear amplitude (default 1).
    #[serde(default)]
    pub amp: Option<In>,
}

impl Describe for Osc {
    const BLURB: &'static str = "band-limited oscillator (sine/saw/square/tri/pulse); freq may be a node (pitch sweeps, FM)";
    fn example() -> serde_json::Value {
        serde_json::json!({"shape": "saw", "freq": "sweep", "amp": 0.5})
    }
}

impl Eval for Osc {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let freq = cx.mono("freq", &self.freq)?;
        let width = cx.mono_or("width", &self.width, 0.5)?;
        let amp = cx.mono_or("amp", &self.amp, 1.0)?;
        let mut o = OscState::new(self.shape.wave(), self.phase.unwrap_or(0.0));
        Ok(one(mono(cx.n, |i| {
            o.tick(freq.at(i), width.at(i), cx.sr) * amp.at(i)
        })))
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
}

/// Seeded noise.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Noise {
    /// white (default), pink (-3 dB/oct), brown (-6 dB/oct), velvet (sparse
    /// random-sign impulses: smooth, cheap reverb-like texture).
    #[serde(default)]
    pub color: NoiseColor,
    /// Velvet only: impulses a second (default 2000).
    #[serde(default)]
    pub density: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Linear amplitude (default 1).
    #[serde(default)]
    pub amp: Option<In>,
}

impl Describe for Noise {
    const BLURB: &'static str = "white/pink/brown/velvet noise (seeded)";
    fn example() -> serde_json::Value {
        serde_json::json!({"color": "pink", "seed": 3})
    }
}

#[allow(clippy::excessive_precision)]
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
                // Paul Kellet's refined pink filter (his published constants).
                let mut b = [0.0f32; 7];
                mono(n, |i| {
                    let w = rng.signed();
                    b[0] = 0.99886 * b[0] + w * 0.0555179;
                    b[1] = 0.99332 * b[1] + w * 0.0750759;
                    b[2] = 0.96900 * b[2] + w * 0.1538520;
                    b[3] = 0.86650 * b[3] + w * 0.3104856;
                    b[4] = 0.55000 * b[4] + w * 0.5329522;
                    b[5] = -0.7616 * b[5] - w * 0.0168980;
                    let p = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
                    b[6] = w * 0.115926;
                    p * 0.11 * amp.at(i)
                })
            }
            NoiseColor::Brown => {
                let mut y = 0.0f32;
                mono(n, |i| {
                    y = (y + 0.02 * rng.signed()) / 1.02;
                    y * 3.5 * amp.at(i)
                })
            }
            NoiseColor::Velvet => {
                let density = positive("density", self.density.unwrap_or(2000.0))?;
                let period = (cx.sr / density).max(1.0);
                let mut v = vec![0.0f32; n];
                let mut k = 0.0f32;
                while (k as usize) < n {
                    let at = (k + rng.unit() * period) as usize;
                    let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                    if at < n {
                        v[at] = sign * amp.at(at);
                    }
                    k += period;
                }
                Buf::Audio(v)
            }
        };
        Ok(one(buf))
    }
}

/// Single-sample clicks.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Impulse {
    /// Seconds: one time or a list (default 0).
    #[serde(default)]
    pub at: Option<OneOrMany>,
    /// Height of each click (default 1).
    #[serde(default)]
    pub amp: Option<f32>,
}

impl Describe for Impulse {
    const BLURB: &'static str =
        "single-sample clicks at the given times (excites resonators, comb, reverb)";
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
