//! Modulation effects. `chorus`, `flanger` and `phaser` modulate a delay
//! or a chain of allpass filters with a sine LFO and add the result to the
//! dry signal; `freq_shift` moves every partial by a fixed number of Hz
//! (single sideband, through [`Hilbert`]). Delay reads use
//! [`DelayLine::read_lagrange3`]. All four are sample by sample with no
//! lookahead (causal) and read no seed.

use std::f32::consts::{FRAC_PI_4, SQRT_2, TAU};

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::{Buf, Sig};
use crate::dsp::{DelayLine, Hilbert, flush};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Most chorus voices.
const MAX_VOICES: usize = 8;
/// Fastest LFO, Hz.
const MAX_RATE: f32 = 20.0;
/// Longest flanger delay, seconds.
const MAX_FLANGE: f32 = 0.02;
/// Longest chorus delay, seconds.
const MAX_CHORUS: f32 = 0.1;
/// Widest phaser sweep either side of the centre, octaves.
const MAX_OCTAVES: f32 = 4.0;

/// Errors when a constant signal is outside `lo..=hi`.
fn check(field: &str, s: &Sig, lo: f32, hi: f32, unit: &str) -> Result<(), String> {
    if let Some(v) = s.constant()
        && !(v.is_finite() && (lo..=hi).contains(&v))
    {
        return Err(format!(
            "{field}: {v}: give a number from {lo} to {hi}{unit}"
        ));
    }
    Ok(())
}

/// Errors when a plain number is outside `lo..=hi`.
fn check_num(field: &str, v: f32, lo: f32, hi: f32, unit: &str) -> Result<f32, String> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(format!(
            "{field}: {v}: give a number from {lo} to {hi}{unit}"
        ))
    }
}

/// A signal sample clamped to `lo..=hi`; a non-finite one becomes `fallback`.
#[inline]
fn sane(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

/// A sine LFO whose frequency may change every sample (`f64` phase).
struct Sine {
    phase: f64,
}

impl Sine {
    /// The sine at `offset` (cycles) from the current phase; then the phase
    /// moves on by `rate` Hz.
    #[inline]
    fn at(&self, offset: f64) -> f32 {
        ((self.phase + offset).rem_euclid(1.0) as f32 * TAU).sin()
    }

    #[inline]
    fn advance(&mut self, rate: f32, sr: f32) {
        self.phase = (self.phase + f64::from(rate) / f64::from(sr)).rem_euclid(1.0);
    }
}

/// The name of a node input, for messages.
fn input_name(i: &In) -> String {
    match i {
        In::Name(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

// ----------------------------------------------------------------- chorus

/// A chorus: several copies of the input, each read from a delay line whose
/// length is swung by a sine LFO (the copies are a fraction of a cycle
/// apart), mixed with the dry input. A stereo result spreads the copies from
/// left to right.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Chorus {
    /// The sound to thicken. Mono when `spread` is above 0 (the output is
    /// then stereo); mono or stereo (each channel on its own) when `spread`
    /// is 0.
    pub input: In,
    /// LFO rate, Hz, 0..20 (default 0.8). A number outside the range is an
    /// error; a signal is clamped per sample.
    #[serde(default)]
    pub rate: Option<In>,
    /// Peak swing of the delay either side of `delay`, seconds, 0 up to
    /// `delay` (default 0.003). A number above `delay` is an error; a
    /// signal is clamped per sample. A tone at `f` Hz is spread by about
    /// `f * depth * 2 pi * rate` Hz.
    #[serde(default)]
    pub depth: Option<In>,
    /// Centre delay, seconds, 0.001..0.1 (default 0.02). A number.
    #[serde(default)]
    pub delay: Option<f32>,
    /// Copies of the input, a whole number from 1 to 8 (default 3). Their
    /// LFO phases are evenly spread over one cycle.
    #[serde(default)]
    pub voices: Option<usize>,
    /// Stereo width, 0..1 (default 1): the voices are panned evenly from
    /// `-spread` (left) to `+spread` (right), equal power. 0 gives a mono
    /// result. One voice has nothing to spread: both channels are equal.
    #[serde(default)]
    pub spread: Option<f32>,
    /// Wet amount, 0..1 (default 0.5): `dry * (1 - mix) + chorus * mix`. 0 is
    /// the input bit for bit. A number outside 0..1 is an error; a signal is
    /// clamped per sample.
    #[serde(default)]
    pub mix: Option<In>,
}

impl Describe for Chorus {
    const BLURB: &'static str =
        "chorus: several LFO-swung delay taps mixed with the dry input; stereo when spread > 0";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "rate": 0.8, "depth": 0.003, "delay": 0.02, "voices": 3, "spread": 1.0, "mix": 0.5})
    }
}

impl Eval for Chorus {
    // No lookahead, no seed: the defaults hold.
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let rate = cx.mono_or("rate", &self.rate, 0.8)?;
        let depth = cx.mono_or("depth", &self.depth, 0.003)?;
        let mix = cx.mono_or("mix", &self.mix, 0.5)?;
        let delay = check_num(
            "delay",
            self.delay.unwrap_or(0.02),
            0.001,
            MAX_CHORUS,
            " seconds",
        )?;
        let spread = check_num("spread", self.spread.unwrap_or(1.0), 0.0, 1.0, "")?;
        let voices = self.voices.unwrap_or(3);
        if !(1..=MAX_VOICES).contains(&voices) {
            return Err(format!(
                "voices: {voices}: give a whole number from 1 to {MAX_VOICES}"
            ));
        }
        check("rate", &rate, 0.0, MAX_RATE, " Hz")?;
        check("depth", &depth, 0.0, delay, " seconds (up to delay)")?;
        check("mix", &mix, 0.0, 1.0, "")?;
        let stereo = spread > 0.0;
        if stereo && input.is_stereo() {
            return Err(format!(
                "input: `{}` is stereo but spread {spread} makes a stereo result from a mono input: add a mix node with \"to\": \"mono\" in between, or set spread to 0",
                input_name(&self.input)
            ));
        }
        // Work cap: one delay line of at most 2 * 0.1 s per channel; the
        // voices are taps on it.
        let max = (2.0 * delay * cx.sr).ceil() as usize + 8;
        let gain = 1.0 / (voices as f32).sqrt();
        // Per voice: the gain into (left, right).
        let gains: Vec<[f32; 2]> = (0..voices)
            .map(|v| {
                if stereo {
                    let pos = if voices == 1 {
                        0.0
                    } else {
                        spread * (2.0 * v as f32 / (voices - 1) as f32 - 1.0)
                    };
                    let angle = (pos + 1.0) * FRAC_PI_4;
                    [gain * SQRT_2 * angle.cos(), gain * SQRT_2 * angle.sin()]
                } else {
                    [gain, gain]
                }
            })
            .collect();
        let run = |x: &[f32]| -> [Vec<f32>; 2] {
            let mut line = DelayLine::new(max);
            let mut lfo = Sine { phase: 0.0 };
            let mut l = Vec::with_capacity(x.len());
            let mut r = Vec::with_capacity(x.len());
            for (i, &v) in x.iter().enumerate() {
                let swing = sane(depth.at(i), 0.0, delay, 0.0);
                let (mut wl, mut wr) = (0.0f32, 0.0f32);
                for (k, g) in gains.iter().enumerate() {
                    let s = lfo.at(k as f64 / voices as f64);
                    let d = ((delay + swing * s) * cx.sr).clamp(2.0, max as f32 - 4.0);
                    // Read before the write: a delay of `d` samples.
                    let tap = line.read_lagrange3(d - 1.0);
                    wl += tap * g[0];
                    wr += tap * g[1];
                }
                line.write(v);
                lfo.advance(sane(rate.at(i), 0.0, MAX_RATE, 0.0), cx.sr);
                let m = sane(mix.at(i), 0.0, 1.0, 0.5);
                l.push(v * (1.0 - m) + wl * m);
                r.push(v * (1.0 - m) + wr * m);
            }
            [l, r]
        };
        let chans = input.channels(cx.n);
        Ok(one(if stereo {
            let [l, r] = run(&chans[0]);
            Buf::from_channels(vec![l, r])
        } else {
            Buf::from_channels(
                chans
                    .iter()
                    .map(|c| {
                        let [l, _] = run(c);
                        l
                    })
                    .collect(),
            )
        }))
    }
}

// ---------------------------------------------------------------- flanger

/// A flanger: the input plus a copy delayed by a few milliseconds, the delay
/// swung by a sine LFO (or driven by `manual`), with feedback. Comb-filter
/// notches sweep the spectrum.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Flanger {
    /// The sound to flange: mono or stereo (each channel on its own, with
    /// the same LFO).
    pub input: In,
    /// LFO rate, Hz, 0..20 (default 0.25). A number outside the range is an
    /// error; a signal is clamped per sample.
    #[serde(default)]
    pub rate: Option<In>,
    /// Peak swing of the delay either side of `delay`, seconds, 0 up to
    /// `delay` (default 0.002). A number above `delay` is an error; a signal
    /// is clamped per sample. Ignored when `manual` is set.
    #[serde(default)]
    pub depth: Option<In>,
    /// Centre delay, seconds, 0.0001..0.02 (default 0.003). A number.
    /// Ignored when `manual` is set.
    #[serde(default)]
    pub delay: Option<f32>,
    /// Feedback of the delayed copy into the line, -0.95..0.95 (default 0,
    /// no resonance). Positive deepens the peaks between the notches,
    /// negative moves the notches to multiples of 1 / delay. A number
    /// outside the range is an error; a signal is clamped per sample.
    #[serde(default)]
    pub feedback: Option<In>,
    /// Wet amount, 0..1 (default 0.5): `dry * (1 - mix) + flanged * mix`. 0
    /// is the input bit for bit. A number outside 0..1 is an error; a signal
    /// is clamped per sample.
    #[serde(default)]
    pub mix: Option<In>,
    /// Replaces the LFO: the delay itself in seconds, 2 samples up to 0.02
    /// (default none). A number is a fixed comb filter (1 ms puts notches at
    /// 500, 1500, 2500 Hz ...); a node (a `ramp`, a `wander`) steers the
    /// flange by hand. A signal is clamped per sample.
    #[serde(default)]
    pub manual: Option<In>,
}

impl Describe for Flanger {
    const BLURB: &'static str = "flanger: dry plus a swept short delay with feedback (comb notches); manual replaces the LFO";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "rate": 0.25, "depth": 0.002, "delay": 0.003, "feedback": 0.5, "mix": 0.5})
    }
}

impl Eval for Flanger {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let rate = cx.mono_or("rate", &self.rate, 0.25)?;
        let depth = cx.mono_or("depth", &self.depth, 0.002)?;
        let fb = cx.mono_or("feedback", &self.feedback, 0.0)?;
        let mix = cx.mono_or("mix", &self.mix, 0.5)?;
        let manual = match &self.manual {
            Some(m) => Some(cx.mono("manual", m)?),
            None => None,
        };
        let delay = check_num(
            "delay",
            self.delay.unwrap_or(0.003),
            0.0001,
            MAX_FLANGE,
            " seconds",
        )?;
        check("rate", &rate, 0.0, MAX_RATE, " Hz")?;
        check("feedback", &fb, -0.95, 0.95, "")?;
        check("mix", &mix, 0.0, 1.0, "")?;
        let floor = 2.0 / cx.sr;
        if let Some(m) = &manual {
            check("manual", m, floor, MAX_FLANGE, " seconds")?;
        } else {
            check("depth", &depth, 0.0, delay, " seconds (up to delay)")?;
        }
        // Work cap: one line of at most 0.02 s per channel.
        let max = (MAX_FLANGE * cx.sr).ceil() as usize + 8;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut line = DelayLine::new(max);
            let mut lfo = Sine { phase: 0.0 };
            x.iter()
                .enumerate()
                .map(|(i, &v)| {
                    let secs = match &manual {
                        Some(m) => sane(m.at(i), floor, MAX_FLANGE, 0.001),
                        None => {
                            let swing = sane(depth.at(i), 0.0, delay, 0.0);
                            delay + swing * lfo.at(0.0)
                        }
                    };
                    lfo.advance(sane(rate.at(i), 0.0, MAX_RATE, 0.0), cx.sr);
                    let d = (secs * cx.sr).clamp(2.0, max as f32 - 4.0);
                    let wet = flush(line.read_lagrange3(d - 1.0));
                    let f = sane(fb.at(i), -0.95, 0.95, 0.0);
                    line.write(v + f * wet);
                    let m = sane(mix.at(i), 0.0, 1.0, 0.5);
                    v * (1.0 - m) + wet * m
                })
                .collect()
        })))
    }
}

// ----------------------------------------------------------------- phaser

/// A phaser: the input plus a copy run through a chain of first-order
/// allpass filters whose corner frequency is swung by a sine LFO. Each pair
/// of stages puts one notch in the spectrum.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Phaser {
    /// The sound to phase: mono or stereo (each channel on its own, with
    /// the same LFO).
    pub input: In,
    /// LFO rate, Hz, 0..20 (default 0.5). A number outside the range is an
    /// error; a signal is clamped per sample.
    #[serde(default)]
    pub rate: Option<In>,
    /// Allpass stages: 4, 6, 8 or 12 (default 4). Any other number is an
    /// error. `stages / 2` notches.
    #[serde(default)]
    pub stages: Option<usize>,
    /// Centre of the sweep, Hz, 20..0.45 x sample rate (default 800). A
    /// number: this is the allpass corner, and the lowest notch sits below
    /// it.
    #[serde(default)]
    pub center: Option<f32>,
    /// Sweep either side of `center`, octaves, 0..4 (default 1). 0 holds
    /// the notches still. A number outside the range is an error; a signal
    /// is clamped per sample.
    #[serde(default)]
    pub depth: Option<In>,
    /// Feedback from the chain's output to its input, -0.95..0.95 (default
    /// 0). A number outside the range is an error; a signal is clamped per
    /// sample.
    #[serde(default)]
    pub feedback: Option<In>,
    /// Wet amount, 0..1 (default 0.5): `dry * (1 - mix) + phased * mix`. 0 is
    /// the input bit for bit. A number outside 0..1 is an error; a signal is
    /// clamped per sample.
    #[serde(default)]
    pub mix: Option<In>,
}

impl Describe for Phaser {
    const BLURB: &'static str =
        "phaser: dry plus an LFO-swept allpass chain (4/6/8/12 stages) with feedback";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "rate": 0.5, "stages": 4, "center": 800, "depth": 1.0, "feedback": 0.3, "mix": 0.5})
    }
}

impl Eval for Phaser {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let rate = cx.mono_or("rate", &self.rate, 0.5)?;
        let depth = cx.mono_or("depth", &self.depth, 1.0)?;
        let fb = cx.mono_or("feedback", &self.feedback, 0.0)?;
        let mix = cx.mono_or("mix", &self.mix, 0.5)?;
        let stages = self.stages.unwrap_or(4);
        if ![4, 6, 8, 12].contains(&stages) {
            return Err(format!("stages: {stages}: give 4, 6, 8 or 12"));
        }
        let top = 0.45 * cx.sr;
        let center = check_num("center", self.center.unwrap_or(800.0), 20.0, top, " Hz")?;
        check("rate", &rate, 0.0, MAX_RATE, " Hz")?;
        check("depth", &depth, 0.0, MAX_OCTAVES, " octaves")?;
        check("feedback", &fb, -0.95, 0.95, "")?;
        check("mix", &mix, 0.0, 1.0, "")?;
        // Work cap: at most 12 one-pole states per channel.
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut state = [0.0f64; 12];
            let mut lfo = Sine { phase: 0.0 };
            let mut last = 0.0f32;
            x.iter()
                .enumerate()
                .map(|(i, &v)| {
                    let octaves = sane(depth.at(i), 0.0, MAX_OCTAVES, 0.0);
                    let hz = (center * (octaves * lfo.at(0.0)).exp2()).clamp(20.0, top);
                    lfo.advance(sane(rate.at(i), 0.0, MAX_RATE, 0.0), cx.sr);
                    let t = (std::f64::consts::PI * f64::from(hz) / f64::from(cx.sr)).tan();
                    let a = (t - 1.0) / (t + 1.0);
                    let f = sane(fb.at(i), -0.95, 0.95, 0.0);
                    // y = a x + s; s' = x - a y: (a + z^-1) / (1 + a z^-1).
                    let mut y = f64::from(v + f * last);
                    for s in state.iter_mut().take(stages) {
                        let out = a * y + *s;
                        *s = flush64(y - a * out);
                        y = out;
                    }
                    last = flush(y as f32);
                    let m = sane(mix.at(i), 0.0, 1.0, 0.5);
                    v * (1.0 - m) + last * m
                })
                .collect()
        })))
    }
}

fn flush64(y: f64) -> f64 {
    if y.abs() < 1e-30 { 0.0 } else { y }
}

// ------------------------------------------------------------- freq_shift

/// A frequency shifter: every partial moves by `shift` Hz (single
/// sideband modulation of the analytic signal). Unlike `pitch_shift` the
/// partials keep their spacing, so harmonic sounds turn inharmonic.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FreqShift {
    /// The sound to shift: mono or stereo (each channel on its own, with
    /// the same shift).
    pub input: In,
    /// Shift, Hz: positive moves partials up, negative down. A number must
    /// be within +-sample rate / 2 (24000 at 48 kHz); a signal is clamped to
    /// that range per sample. Partials shifted below 0 Hz or past Nyquist
    /// wrap (mirror).
    pub shift: In,
    /// Wet amount, 0..1 (default 1): `dry * (1 - mix) + shifted * mix`. 0 is
    /// the input bit for bit. A number outside 0..1 is an error; a signal is
    /// clamped per sample. The dry input is not delay-matched to the shifted
    /// path (see the doc's limits).
    #[serde(default)]
    pub mix: Option<In>,
}

impl Describe for FreqShift {
    const BLURB: &'static str =
        "frequency shifter: moves every partial by shift Hz (single sideband), spacing kept";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "shift": 100, "mix": 1.0})
    }
}

impl Eval for FreqShift {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let shift = cx.mono("shift", &self.shift)?;
        let mix = cx.mono_or("mix", &self.mix, 1.0)?;
        let limit = cx.sr / 2.0;
        if let Some(s) = shift.constant()
            && !(s.is_finite() && s.abs() <= limit)
        {
            return Err(format!(
                "shift: {s}: give a number between -{limit} and {limit} Hz (half the sample rate)"
            ));
        }
        check("mix", &mix, 0.0, 1.0, "")?;
        // One carrier phase (cycles, f64) for every channel.
        let mut phase = 0.0f64;
        let carrier: Vec<(f32, f32)> = (0..cx.n)
            .map(|i| {
                let (s, c) = (phase * std::f64::consts::TAU).sin_cos();
                phase = (phase
                    + f64::from(sane(shift.at(i), -limit, limit, 0.0)) / f64::from(cx.sr))
                .rem_euclid(1.0);
                (s as f32, c as f32)
            })
            .collect();
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut h = Hilbert::new();
            x.iter()
                .zip(&carrier)
                .enumerate()
                .map(|(i, (&v, &(s, c)))| {
                    // z = i + j q is the analytic signal; Re(z e^{j theta}).
                    let (re, im) = h.tick(v);
                    let wet = re * c - im * s;
                    let m = sane(mix.at(i), 0.0, 1.0, 1.0);
                    v * (1.0 - m) + wet * m
                })
                .collect()
        })))
    }
}
