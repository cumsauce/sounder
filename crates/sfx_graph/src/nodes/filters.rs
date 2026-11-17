//! Filters.

use serde::Deserialize;

use super::{one, per_channel};
use crate::dsp::{DelayLine, FilterMode, OnePole, Resonator, Svf};
use crate::node::{Cx, Describe, Eval, In, Outs};

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FilterModeSpec {
    #[default]
    Lp,
    Hp,
    Bp,
    Notch,
    Peak,
    Lowshelf,
    Highshelf,
}

impl FilterModeSpec {
    pub(crate) fn mode(self) -> FilterMode {
        match self {
            FilterModeSpec::Lp => FilterMode::Lp,
            FilterModeSpec::Hp => FilterMode::Hp,
            FilterModeSpec::Bp => FilterMode::Bp,
            FilterModeSpec::Notch => FilterMode::Notch,
            FilterModeSpec::Peak => FilterMode::Peak,
            FilterModeSpec::Lowshelf => FilterMode::LowShelf,
            FilterModeSpec::Highshelf => FilterMode::HighShelf,
        }
    }
}

/// A state-variable filter (12 dB an octave).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub input: In,
    /// lp (default), hp, bp, notch, peak, lowshelf, highshelf.
    #[serde(default)]
    pub mode: FilterModeSpec,
    /// Cutoff / centre, Hz (a node sweeps it).
    pub freq: In,
    /// Resonance (default 0.707: no peak).
    #[serde(default)]
    pub q: Option<In>,
    /// peak and shelves: boost (+) or cut (-), dB.
    #[serde(default)]
    pub gain_db: Option<In>,
}

impl Describe for Filter {
    const BLURB: &'static str =
        "state-variable filter lp/hp/bp/notch/peak/lowshelf/highshelf; freq and q take signals";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "mode": "lp", "freq": "cutoff_env", "q": 2.0})
    }
}

impl Eval for Filter {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let freq = cx.mono("freq", &self.freq)?;
        let q = cx.mono_or("q", &self.q, std::f32::consts::FRAC_1_SQRT_2)?;
        let g = cx.mono_or("gain_db", &self.gain_db, 0.0)?;
        let mode = self.mode.mode();
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = Svf::default();
            x.iter()
                .enumerate()
                .map(|(i, v)| f.tick(*v, mode, freq.at(i), q.at(i), g.at(i), cx.sr))
                .collect()
        })))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PoleMode {
    #[default]
    Lp,
    Hp,
}

/// A one-pole filter (6 dB an octave).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Onepole {
    pub input: In,
    /// lp (default) or hp.
    #[serde(default)]
    pub mode: PoleMode,
    /// Cutoff, Hz.
    pub freq: In,
}

impl Describe for Onepole {
    const BLURB: &'static str = "gentle 6 dB/oct low- or high-pass";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "mode": "hp", "freq": 120})
    }
}

impl Eval for Onepole {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let freq = cx.mono("freq", &self.freq)?;
        let hp = matches!(self.mode, PoleMode::Hp);
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = OnePole::default();
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let (l, h) = f.tick(*v, freq.at(i), cx.sr);
                    if hp { h } else { l }
                })
                .collect()
        })))
    }
}

/// Delay in samples from `freq` (Hz) or `time` (s), the signal at sample i.
fn period(
    cx: &Cx,
    freq: &Option<In>,
    time: &Option<In>,
) -> Result<(crate::buf::Sig, bool), String> {
    match (freq, time) {
        (Some(f), None) => Ok((cx.mono("freq", f)?, true)),
        (None, Some(t)) => Ok((cx.mono("time", t)?, false)),
        _ => Err(
            "give exactly one of freq (Hz: a pitched resonance) or time (seconds: an echo)".into(),
        ),
    }
}

fn max_delay(sig: &crate::buf::Sig, is_freq: bool, n: usize, sr: f32) -> usize {
    let d = |v: f32| {
        if is_freq {
            sr / v.max(1.0)
        } else {
            v.max(0.0) * sr
        }
    };
    let m = (0..n.max(1)).map(|i| d(sig.at(i))).fold(1.0f32, f32::max);
    (m.ceil() as usize + 2).min(60 * sr as usize)
}

/// A feedback comb.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comb {
    pub input: In,
    /// Resonant pitch, Hz (or give time).
    #[serde(default)]
    pub freq: Option<In>,
    /// Loop time, seconds (or give freq).
    #[serde(default)]
    pub time: Option<In>,
    /// Loop gain, -0.999..0.999 (higher rings longer).
    pub feedback: In,
    /// Low-pass in the loop, 0 (bright) .. 1 (dark), default 0.
    #[serde(default)]
    pub damp: Option<f32>,
}

impl Describe for Comb {
    const BLURB: &'static str = "feedback comb (resonant pitch at freq, or echo at time)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "burst", "freq": 220, "feedback": 0.9, "damp": 0.3})
    }
}

impl Eval for Comb {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let (p, is_freq) = period(cx, &self.freq, &self.time)?;
        let fb = cx.mono("feedback", &self.feedback)?;
        let damp = self.damp.unwrap_or(0.0).clamp(0.0, 0.99);
        let max = max_delay(&p, is_freq, cx.n, cx.sr);
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut line = DelayLine::new(max);
            let mut lp = 0.0f32;
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let d = if is_freq {
                        cx.sr / p.at(i).max(1.0)
                    } else {
                        p.at(i) * cx.sr
                    };
                    let back = line.read((d - 1.0).max(0.0));
                    lp = back + damp * (lp - back);
                    let y = v + fb.at(i).clamp(-0.999, 0.999) * lp;
                    line.write(y);
                    y
                })
                .collect()
        })))
    }
}

/// A Schroeder allpass: passes every frequency at the same level but smears
/// them in time.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Allpass {
    pub input: In,
    /// Loop time, seconds.
    pub time: In,
    /// -0.99..0.99 (default 0.5).
    #[serde(default)]
    pub feedback: Option<f32>,
}

impl Describe for Allpass {
    const BLURB: &'static str = "Schroeder allpass (diffusion, smearing transients)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "time": 0.005, "feedback": 0.6})
    }
}

impl Eval for Allpass {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let time = cx.mono("time", &self.time)?;
        let g = self.feedback.unwrap_or(0.5).clamp(-0.99, 0.99);
        let max = max_delay(&time, false, cx.n, cx.sr);
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut line = DelayLine::new(max);
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let d = (time.at(i) * cx.sr).max(1.0);
                    let back = line.read(d - 1.0);
                    let w = v + g * back;
                    line.write(w);
                    back - g * w
                })
                .collect()
        })))
    }
}

/// A bank of ringing modes.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resonators {
    /// The excitation (an impulse, a noise burst, a contact pulse).
    pub input: In,
    /// [[hz, decay_s (to -60 dB), gain], ...].
    pub modes: Vec<[f32; 3]>,
}

impl Describe for Resonators {
    const BLURB: &'static str = "bank of ringing modes [[hz, decay_s, gain], ...] (struck objects)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "hit", "modes": [[523, 1.2, 1.0], [1410, 0.6, 0.5], [2730, 0.3, 0.3]]})
    }
}

impl Eval for Resonators {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if self.modes.is_empty() {
            return Err("modes: give at least one [hz, decay_s, gain]".into());
        }
        let input = cx.audio("input", &self.input)?;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut out = vec![0.0f32; x.len()];
            for m in &self.modes {
                if m[0] >= crate::dsp::max_hz(cx.sr) {
                    continue; // above Nyquist: inaudible (summary warns)
                }
                let mut r = Resonator::new(m[0], m[1], m[2], cx.sr);
                for (o, v) in out.iter_mut().zip(x) {
                    *o += r.tick(*v);
                }
            }
            out
        })))
    }
}
