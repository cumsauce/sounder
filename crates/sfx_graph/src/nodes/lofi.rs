//! Loops and lo-fi: `crossfade_loop` (makes the wrap of a loop seamless),
//! `resample` (sample-rate reduction with hold), `tape` (wow, flutter,
//! saturation, hiss, a tone roll-off) and `stutter` (repeats the last slice
//! at each event).

use std::f32::consts::{FRAC_PI_2, TAU};

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::{Buf, Event};
use crate::db_to_gain;
use crate::dsp::{Biquad, DelayLine, FilterMode, Svf};
use crate::node::{Cx, Describe, Eval, In, Outs};

fn finite_in(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

// ----------------------------------------------------------- crossfade_loop

/// Makes a sound loop without a seam by crossfading the material on both
/// sides of the wrap.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrossfadeLoop {
    /// The sound to make loopable: mono or stereo, a loop's whole length.
    pub input: In,
    /// Length of each of the two crossfades, seconds: more than 0, and at
    /// most half the sound's length (default 0.25). The sound is unchanged
    /// except for this long at each end. Alias `time`.
    #[serde(default, alias = "time")]
    pub fade: Option<f32>,
}

impl Describe for CrossfadeLoop {
    const BLURB: &'static str = "crossfades the end of a sound into the start so it loops with no seam (equal power; the length is unchanged)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "fade": 0.1})
    }
}

impl Eval for CrossfadeLoop {
    // The end of the sound is read while writing its start.
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let fade = self.fade.unwrap_or(0.25);
        if !(fade.is_finite() && fade > 0.0) {
            return Err(format!("fade: {fade}: give seconds more than 0"));
        }
        let f = (fade * cx.sr).round() as usize;
        let longest = cx.n / 2;
        if f < 2 {
            return Err(format!(
                "fade: {fade}: that is under 2 samples: give at least {} s",
                2.0 / cx.sr
            ));
        }
        if f > longest {
            return Err(format!(
                "fade: {fade}: the sound is {:.3} s long and each crossfade takes `fade`: give at most {:.3} s (or make the sound longer with `duration`)",
                cx.n as f32 / cx.sr,
                longest as f32 / cx.sr
            ));
        }
        let n = cx.n;
        Ok(one(per_channel(&input, n, |x| {
            // y is x with the seam replaced by x read F samples earlier
            // (a copy of x rotated by F, which is smooth at the wrap), put
            // in by an equal-power ramp over the first F and the last F
            // samples. The ramps weight the earlier copy by cos and the
            // sound by sin: 0 at the wrap, 1 at the ends of the ramps.
            let mut y = x.to_vec();
            let r = (f - 1) as f32;
            for i in 0..f {
                let t = FRAC_PI_2 * i as f32 / r;
                let (s, c) = t.sin_cos();
                y[i] = s * x[i] + c * x[i + n - f];
                let j = n - 1 - i; // mirror: the tail ramp
                y[j] = s * x[j] + c * x[j - f];
            }
            y
        })))
    }
}

// ----------------------------------------------------------------- resample

/// Eighth-order Butterworth Q per section.
const BUTTER8: [f32; 4] = [0.5098, 0.6013, 0.9, 2.5629];

/// The 48 dB/oct low-pass `resample` puts before and after the hold: four
/// trapezoidal state-variable sections, which stay stable when the cutoff
/// follows a fast `rate`.
struct Anti([Svf; 4]);

impl Anti {
    fn new() -> Anti {
        Anti(Default::default())
    }
    fn tick(&mut self, x: f32, hz: f32, sr: f32) -> f32 {
        self.0
            .iter_mut()
            .zip(BUTTER8)
            .fold(x, |v, (f, q)| f.tick(v, FilterMode::Lp, hz, q, 0.0, sr))
    }
}

/// Sample-rate reduction: the input is sampled `rate` times a second and
/// each sample held until the next.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resample {
    /// Mono or stereo audio.
    pub input: In,
    /// Samples a second, Hz. A number from 1 to the sample rate, or a node
    /// for a sweep (a signal is clamped to that range per sample). The
    /// sample rate itself passes the input through unchanged.
    pub rate: In,
    /// Low-pass the input before sampling and the held signal after it
    /// (both 8th order Butterworth at 0.45 x `rate`), which removes the
    /// folded-down and mirrored-up copies (default false).
    #[serde(default)]
    pub antialias: Option<bool>,
}

impl Describe for Resample {
    const BLURB: &'static str = "sample-rate reduction: holds a sample every 1/rate s (antialias low-passes before and after)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "rate": 8000, "antialias": false})
    }
}

impl Eval for Resample {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let rate = cx.mono("rate", &self.rate)?;
        let anti = self.antialias.unwrap_or(false);
        if let Some(r) = rate.constant()
            && !(r.is_finite() && (1.0..=cx.sr).contains(&r))
        {
            return Err(format!(
                "rate: {r}: give a number from 1 to {} Hz (the sample rate)",
                cx.sr
            ));
        }
        let sr = cx.sr;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut pre = Anti::new();
            let mut post = Anti::new();
            let (mut phase, mut held) = (1.0f64, 0.0f32);
            let mut y = Vec::with_capacity(x.len());
            for (i, v) in x.iter().enumerate() {
                let r = finite_in(rate.at(i)).clamp(1.0, sr);
                let on = anti && r < sr;
                let hz = 0.45 * r;
                let v = finite_in(*v);
                let v = if on { pre.tick(v, hz, sr) } else { v };
                if phase >= 1.0 {
                    held = v;
                    phase -= 1.0;
                }
                phase += f64::from(r) / f64::from(sr);
                y.push(if on { post.tick(held, hz, sr) } else { held });
            }
            y
        })))
    }
}

// --------------------------------------------------------------------- tape

/// Largest wow or flutter depth, percent of speed.
const MAX_SPEED_PCT: f32 = 5.0;
/// Largest `drive`.
const MAX_DRIVE: f32 = 40.0;

/// A tape machine: speed wobble (wow and flutter), soft saturation, hiss and
/// a tone roll-off.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tape {
    /// Mono or stereo audio (both channels share one speed wobble; the hiss
    /// is separate per channel). The output is the input delayed by 1 sample
    /// plus the wobble's mean delay (about 0.5 ms at the defaults).
    pub input: In,
    /// Slow speed wobble (0.4 to 1.2 Hz, picked by the seed): peak speed
    /// deviation, percent, 0 ..= 5 (default 0.2). 1 percent moves a 1 kHz
    /// tone by +-10 Hz.
    #[serde(default)]
    pub wow: Option<f32>,
    /// Fast speed wobble (two partials at 5 to 15 Hz): peak speed deviation,
    /// percent, 0 ..= 5 (default 0.1).
    #[serde(default)]
    pub flutter: Option<f32>,
    /// Saturation, 0 ..= 40 (default 1): `tanh(drive x) / drive`, which is
    /// the input for small signals; 0 is off. Not oversampled.
    #[serde(default)]
    pub drive: Option<f32>,
    /// Hiss: white noise at this RMS level, dBFS, -120 ..= 0 (default -60;
    /// -120 is off). It goes through the tone filter.
    #[serde(default)]
    pub hiss_db: Option<f32>,
    /// Corner of a 12 dB/oct low-pass, Hz, 20 up to 0.45 x the sample rate
    /// (default 8000).
    #[serde(default)]
    pub tone: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): picks the
    /// wobble frequencies and phases and the hiss.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Tape {
    const BLURB: &'static str =
        "tape: wow and flutter (speed wobble), saturation, hiss, tone roll-off (seeded)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "wow": 0.2, "flutter": 0.1, "drive": 1, "hiss_db": -60, "tone": 8000, "seed": 1})
    }
}

/// One sinusoidal speed deviation: peak `depth` (a fraction), `hz`, `phase`.
struct Wobble {
    depth: f64,
    hz: f64,
    phase: f64,
}

impl Wobble {
    /// Delay in seconds at time `t` that makes the playback speed
    /// 1 + depth sin(2 pi hz t + phase) (the delay's slope is minus the
    /// deviation): amp (1 + cos), always >= 0.
    fn delay(&self, t: f64) -> f64 {
        let amp = self.depth / (std::f64::consts::TAU * self.hz);
        amp * (1.0 + (std::f64::consts::TAU * self.hz * t + self.phase).cos())
    }
    fn max_delay(&self) -> f64 {
        2.0 * self.depth / (std::f64::consts::TAU * self.hz)
    }
}

impl Eval for Tape {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let wow = self.wow.unwrap_or(0.2);
        let flutter = self.flutter.unwrap_or(0.1);
        let drive = self.drive.unwrap_or(1.0);
        let hiss_db = self.hiss_db.unwrap_or(-60.0);
        let tone = self.tone.unwrap_or(8000.0);
        for (name, v) in [("wow", wow), ("flutter", flutter)] {
            if !(v.is_finite() && (0.0..=MAX_SPEED_PCT).contains(&v)) {
                return Err(format!(
                    "{name}: {v}: give percent of speed from 0 to {MAX_SPEED_PCT}"
                ));
            }
        }
        if !(drive.is_finite() && (0.0..=MAX_DRIVE).contains(&drive)) {
            return Err(format!(
                "drive: {drive}: give a number from 0 to {MAX_DRIVE}"
            ));
        }
        if !(hiss_db.is_finite() && (-120.0..=0.0).contains(&hiss_db)) {
            return Err(format!(
                "hiss_db: {hiss_db}: give dBFS from -120 (off) to 0"
            ));
        }
        let top = 0.45 * cx.sr;
        if !(tone.is_finite() && (20.0..=top).contains(&tone)) {
            return Err(format!("tone: {tone}: give Hz from 20 to {top}"));
        }
        // The wobbles: seeded frequencies and phases, drawn in a fixed
        // order so the seed alone decides them.
        let mut rng = cx.rng(self.seed);
        let mut draw = |lo: f32, hi: f32| f64::from(lo + (hi - lo) * rng.unit());
        let wows = Wobble {
            depth: f64::from(wow) / 100.0,
            hz: draw(0.4, 1.2),
            phase: draw(0.0, TAU),
        };
        let fl = f64::from(flutter) / 100.0;
        let flutters = [
            Wobble {
                depth: 0.65 * fl,
                hz: draw(5.0, 9.0),
                phase: draw(0.0, TAU),
            },
            Wobble {
                depth: 0.35 * fl,
                hz: draw(9.0, 15.0),
                phase: draw(0.0, TAU),
            },
        ];
        let sr = f64::from(cx.sr);
        // The delay, in samples, at each sample: one sample plus the
        // wobbles. Every channel shares it.
        let max_delay =
            1.0 + sr * (wows.max_delay() + flutters.iter().map(Wobble::max_delay).sum::<f64>());
        let delays: Vec<f32> = (0..cx.n)
            .map(|i| {
                let t = i as f64 / sr;
                (1.0 + sr * (wows.delay(t) + flutters.iter().map(|w| w.delay(t)).sum::<f64>()))
                    as f32
            })
            .collect();
        let hiss = db_to_gain(hiss_db) * 3.0f32.sqrt(); // uniform noise has RMS 1/sqrt 3
        let hiss_on = hiss_db > -120.0;
        let mut channels = Vec::new();
        for (c, x) in input.channels(cx.n).iter().enumerate() {
            let mut line = DelayLine::new(max_delay.ceil() as usize + 4);
            let mut lp = Biquad::new();
            lp.set(FilterMode::Lp, tone, 0.707, 0.0, cx.sr);
            let mut noise = cx.rng(self.seed ^ ((c as u64 + 1) << 40));
            channels.push(
                x.iter()
                    .zip(&delays)
                    .map(|(v, d)| {
                        let v = finite_in(*v);
                        let v = if drive > 1e-3 {
                            (drive * v).tanh() / drive
                        } else {
                            v
                        };
                        line.write(v);
                        let mut y = line.read_lagrange3(*d);
                        if hiss_on {
                            y += hiss * noise.signed();
                        }
                        lp.tick(y)
                    })
                    .collect::<Vec<f32>>(),
            );
        }
        Ok(one(Buf::from_channels(channels)))
    }
}

// ------------------------------------------------------------------ stutter

/// Largest `repeats`.
const MAX_REPEATS: u32 = 256;
/// Largest `slice`, seconds.
const MAX_SLICE: f32 = 10.0;
/// Most samples the node may write (events x repeated length).
const MAX_WORK: u128 = 400_000_000;
/// Crossfade (linear: the two sides are usually the same material) at each
/// repeat boundary, seconds.
const STUTTER_FADE: f32 = 0.002;

/// A stutter: at each event, the last slice of the sound is played again,
/// several times, in place of what would have played.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stutter {
    /// Mono or stereo audio.
    pub input: In,
    /// Event stream (`pattern`, `poisson`, `burst`, ...): each event starts
    /// one stutter. Events after the end of the sound are ignored; a
    /// later event takes over from an earlier one still repeating.
    pub events: In,
    /// Length of the repeated slice, seconds, 0.0001 ..= 10 (default 0.05).
    /// At an event nearer the start than this, the slice is as long as the
    /// sound so far (an event at 0 does nothing).
    #[serde(default)]
    pub slice: Option<f32>,
    /// How many times the slice plays after the event, 1 ..= 256 (default
    /// 4). The sound resumes after `repeats` x `slice` seconds.
    #[serde(default)]
    pub repeats: Option<u32>,
    /// Play the repeats backwards (default false). Then the slice is the
    /// one that follows the event (reversed), so the node reads ahead and
    /// is not causal.
    #[serde(default)]
    pub reverse: Option<bool>,
}

impl Describe for Stutter {
    const BLURB: &'static str = "at each event repeats the last slice (or, reverse, the next one backwards) a number of times";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "events": "ev", "slice": 0.05, "repeats": 4, "reverse": false})
    }
}

impl Stutter {
    fn run(&self, x: &[f32], events: &[Event], s: usize, repeats: usize, sr: f32) -> Vec<f32> {
        let n = x.len();
        let reverse = self.reverse.unwrap_or(false);
        let mut out = x.to_vec();
        for ev in events {
            let te = (ev.t * f64::from(sr)).round();
            if !(te >= 0.0 && te < n as f64) {
                continue;
            }
            let te = te as usize;
            let slice: Vec<f32> = if reverse {
                x[te..(te + s).min(n)].iter().rev().copied().collect()
            } else {
                out[te - s.min(te)..te].to_vec()
            };
            let l = slice.len();
            if l == 0 {
                continue;
            }
            let xf = ((STUTTER_FADE * sr) as usize).min(l / 2);
            let end = te + repeats * l;
            // Each copy starts by fading in from the sound's own continuation
            // after te (so the join with the copy before it is smooth).
            for r in 0..repeats {
                let a = te + r * l;
                for (k, v) in slice.iter().enumerate() {
                    if a + k >= n {
                        break;
                    }
                    out[a + k] = if k < xf {
                        let w = k as f32 / xf as f32;
                        w * v + (1.0 - w) * x[te + k]
                    } else {
                        *v
                    };
                }
            }
            // And the end fades the continuation back into what was there.
            for k in 0..xf {
                if end + k < n {
                    let w = k as f32 / xf as f32;
                    out[end + k] = w * out[end + k] + (1.0 - w) * x[te + k];
                }
            }
        }
        out
    }
}

impl Eval for Stutter {
    fn causal(&self) -> bool {
        !self.reverse.unwrap_or(false)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let events = cx.events("events", &self.events)?;
        let slice = self.slice.unwrap_or(0.05);
        let repeats = self.repeats.unwrap_or(4);
        if !(slice.is_finite() && (0.0001..=MAX_SLICE).contains(&slice)) {
            return Err(format!(
                "slice: {slice}: give seconds from 0.0001 to {MAX_SLICE}"
            ));
        }
        if !(1..=MAX_REPEATS).contains(&repeats) {
            return Err(format!(
                "repeats: {repeats}: give a whole number from 1 to {MAX_REPEATS}"
            ));
        }
        let s = ((slice * cx.sr).round() as usize).max(1);
        let each = (s as u128 * u128::from(repeats)).min(cx.n as u128);
        if events.len() as u128 * each > MAX_WORK {
            return Err(format!(
                "events: {} events of {each} samples is over the limit of {MAX_WORK} samples: use fewer events, a shorter slice or fewer repeats",
                events.len()
            ));
        }
        Ok(one(per_channel(&input, cx.n, |x| {
            self.run(x, &events, s, repeats as usize, cx.sr)
        })))
    }
}
