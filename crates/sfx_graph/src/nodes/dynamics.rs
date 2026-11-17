//! Dynamics and enhancement: `transient` (attack / sustain shaper), `gate`
//! (downward expander), `sub` (octave-down divider) and `exciter` (high
//! band saturation). All causal; stereo is linked (`transient`, `gate`) or
//! run per channel (`exciter`); `sub` is mono.

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::Buf;
use crate::dsp::{Biquad, FilterMode, Follower, Svf, flush, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{db_to_gain, gain_to_db};

/// Largest time a dynamics field takes, seconds.
const MAX_SECONDS: f32 = 10.0;
/// Largest boost or cut of `transient`, dB.
const MAX_TRANSIENT_DB: f32 = 24.0;
/// How far apart (dB) the fast and slow followers must be for the full
/// attack or sustain gain to apply.
const TRANSIENT_RANGE_DB: f32 = 6.0;
/// The gate's detector lets go of a peak this fast, seconds.
const DETECTOR_RELEASE: f32 = 0.005;
/// Deepest reduction a finite-ratio gate applies, dB.
const MAX_REDUCTION_DB: f32 = 120.0;

fn finite(field: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give a finite number"))
    }
}

/// A time in seconds, 0 ..= [`MAX_SECONDS`].
fn seconds(field: &str, x: f32) -> Result<f32, String> {
    if x.is_finite() && (0.0..=MAX_SECONDS).contains(&x) {
        Ok(x)
    } else {
        Err(format!(
            "{field}: {x}: give seconds between 0 and {MAX_SECONDS}"
        ))
    }
}

// ---------------------------------------------------------------- transient

/// An attack / sustain shaper: boosts or cuts the start of each sound and
/// its body separately, whatever the level.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transient {
    /// Mono or stereo audio (stereo channels are linked: one gain).
    pub input: In,
    /// Gain in dB applied while the level is rising (the attack), -24 ..= 24
    /// (default 0).
    #[serde(default)]
    pub attack_db: Option<f32>,
    /// Gain in dB applied while the level is falling or held below its
    /// recent average (the sustain / tail), -24 ..= 24 (default 0).
    #[serde(default)]
    pub sustain_db: Option<f32>,
    /// Reaction time, seconds, 0.001 ..= 1 (default 0.02). The fast follower
    /// takes `speed / 4`, the slow one `speed * 2.5`.
    #[serde(default)]
    pub speed: Option<f32>,
}

impl Describe for Transient {
    const BLURB: &'static str = "transient shaper: separate gain for attack and sustain";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "attack_db": 6, "sustain_db": -3, "speed": 0.02})
    }
}

impl Eval for Transient {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let limit = |field: &str, x: f32| {
            if x.is_finite() && x.abs() <= MAX_TRANSIENT_DB {
                Ok(x)
            } else {
                Err(format!(
                    "{field}: {x}: give dB between -{MAX_TRANSIENT_DB} and {MAX_TRANSIENT_DB}"
                ))
            }
        };
        let attack_db = limit("attack_db", self.attack_db.unwrap_or(0.0))?;
        let sustain_db = limit("sustain_db", self.sustain_db.unwrap_or(0.0))?;
        let speed = self.speed.unwrap_or(0.02);
        if !(speed.is_finite() && (0.001..=1.0).contains(&speed)) {
            return Err(format!("speed: {speed}: give seconds between 0.001 and 1"));
        }
        let chans = input.channels(cx.n);
        let (fast_t, slow_t) = (speed * 0.25, speed * 2.5);
        let (mut fast, mut slow) = (Follower::new(), Follower::new());
        let gain: Vec<f32> = (0..cx.n)
            .map(|i| {
                let p = chans.iter().map(|c| c[i] * c[i]).fold(0.0, f32::max);
                let f = fast.tick(p, fast_t, fast_t, cx.sr);
                let s = slow.tick(p, slow_t, slow_t, cx.sr);
                let diff = 10.0 * ((f + 1e-14) / (s + 1e-14)).log10();
                let rising = (diff / TRANSIENT_RANGE_DB).clamp(0.0, 1.0);
                let falling = (-diff / TRANSIENT_RANGE_DB).clamp(0.0, 1.0);
                db_to_gain(attack_db * rising + sustain_db * falling)
            })
            .collect();
        Ok(one(Buf::from_channels(
            chans
                .into_iter()
                .map(|c| c.iter().zip(&gain).map(|(x, g)| x * g).collect())
                .collect(),
        )))
    }
}

// --------------------------------------------------------------------- gate

/// A noise gate / downward expander: quiet passages are turned down, loud
/// ones pass untouched.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    /// Mono or stereo audio (stereo channels are linked: one gain).
    pub input: In,
    /// Level (peak, dBFS) under which the gate closes.
    pub threshold_db: f32,
    /// Expansion ratio, >= 1: each dB under the threshold becomes `ratio` dB
    /// (reduction 120 dB at most). Left out, the gate closes fully.
    #[serde(default)]
    pub ratio: Option<f32>,
    /// Seconds (time constant of the gain) to open, 0 ..= 10 (default
    /// 0.001).
    #[serde(default)]
    pub attack: Option<f32>,
    /// Seconds (time constant of the gain) to close, 0 ..= 10 (default
    /// 0.05). Closing by 60 dB takes about 6.9 times this.
    #[serde(default)]
    pub release: Option<f32>,
    /// Seconds the gate stays open after the level last reached the
    /// threshold, 0 ..= 10 (default 0.01).
    #[serde(default)]
    pub hold: Option<f32>,
    /// A signal that drives the detector instead of the input (a mono or
    /// stereo node; the gain still applies to `input`).
    #[serde(default)]
    pub key: Option<In>,
}

impl Describe for Gate {
    const BLURB: &'static str = "gate / downward expander with hold and sidechain key";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "threshold_db": -40, "attack": 0.001, "release": 0.05, "hold": 0.01})
    }
}

impl Eval for Gate {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let threshold_db = finite("threshold_db", self.threshold_db)?;
        let ratio = match self.ratio {
            None => None,
            Some(r) if r.is_finite() && r >= 1.0 => Some(r),
            Some(r) => {
                return Err(format!(
                    "ratio: {r}: give a number >= 1, or leave it out for a gate that closes fully"
                ));
            }
        };
        let attack = seconds("attack", self.attack.unwrap_or(0.001))?;
        let release = seconds("release", self.release.unwrap_or(0.05))?;
        let hold = seconds("hold", self.hold.unwrap_or(0.01))?;
        let key = match &self.key {
            Some(k) => cx.audio("key", k)?,
            None => input.clone(),
        };
        let chans = input.channels(cx.n);
        let key = key.channels(cx.n);
        let thr = db_to_gain(threshold_db);
        let hold_n = (hold * cx.sr).round() as usize;
        let mut det = Follower::new();
        let mut env = Follower::new();
        let mut held = 0usize;
        let gain: Vec<f32> = (0..cx.n)
            .map(|i| {
                let peak = key.iter().map(|c| c[i].abs()).fold(0.0, f32::max);
                let level = det.tick(peak, 0.0, DETECTOR_RELEASE, cx.sr);
                let target = if level >= thr {
                    held = hold_n;
                    1.0
                } else if held > 0 {
                    held -= 1;
                    1.0
                } else {
                    match ratio {
                        None => 0.0,
                        Some(r) => db_to_gain(
                            ((gain_to_db(level) - threshold_db) * (r - 1.0)).max(-MAX_REDUCTION_DB),
                        ),
                    }
                };
                if i == 0 {
                    env.env = target;
                }
                env.tick(target, attack, release, cx.sr)
            })
            .collect();
        Ok(one(Buf::from_channels(
            chans
                .into_iter()
                .map(|c| c.iter().zip(&gain).map(|(x, g)| x * g).collect())
                .collect(),
        )))
    }
}

// ---------------------------------------------------------------------- sub

/// A sub-octave generator: a flip-flop divides the low-passed input's zero
/// crossings, a filter turns the square into a sine, and the input's low-band
/// level sets its amplitude. The result is added to the input.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sub {
    /// Mono audio (a stereo input errors: add a `mix` with `"to": "mono"`).
    pub input: In,
    /// Octaves down: 1 or 2 (default 1).
    #[serde(default)]
    pub octaves: Option<f32>,
    /// Level of the sub added to the input, >= 0 and <= 4 (default 0.5; 1 =
    /// as loud as the input's low band).
    #[serde(default)]
    pub mix: Option<f32>,
    /// Hz, 20 ..= 1000 (default 120): a 4th-order low-pass that picks the
    /// band whose zero crossings are divided. Only content under this
    /// frequency gets a sub; a note must sit near or under it.
    #[serde(default)]
    pub lowpass: Option<f32>,
}

impl Describe for Sub {
    const BLURB: &'static str = "sub-octave generator (zero-crossing divider, sine-shaped)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "tone", "octaves": 1, "mix": 0.5, "lowpass": 250})
    }
}

/// Butterworth Q of each section of a 4th-order low-pass.
const BUTTER4_Q: [f32; 2] = [0.541_196_1, 1.306_563];

impl Eval for Sub {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.mono("input", &self.input)?;
        let octaves = match self.octaves.unwrap_or(1.0) {
            1.0 => 1u32,
            2.0 => 2,
            x => return Err(format!("octaves: {x}: give 1 or 2")),
        };
        let mix = self.mix.unwrap_or(0.5);
        if !(mix.is_finite() && (0.0..=4.0).contains(&mix)) {
            return Err(format!("mix: {mix}: give a number between 0 and 4"));
        }
        let lowpass = self.lowpass.unwrap_or(120.0);
        if !(lowpass.is_finite() && (20.0..=1000.0).contains(&lowpass)) {
            return Err(format!("lowpass: {lowpass}: give Hz between 20 and 1000"));
        }
        let lowpass = lowpass.min(max_hz(cx.sr));
        let sr = cx.sr;

        let mut lp = [Biquad::new(), Biquad::new()];
        for (b, q) in lp.iter_mut().zip(BUTTER4_Q) {
            b.set(FilterMode::Lp, lowpass, q, 0.0, sr);
        }
        // The level of the input around the band we divide, to set the
        // amplitude (4x the cutoff keeps the fundamental and its first
        // overtones, not the hiss).
        let mut band = Biquad::new();
        band.set(
            FilterMode::Lp,
            (lowpass * 4.0).min(0.45 * sr),
            0.707,
            0.0,
            sr,
        );
        let mut band_power = Follower::new();
        let mut lp_power = Follower::new();
        let mut shape = [Svf::default(), Svf::default()];
        let level_t = 0.01;

        let mut armed = false;
        let mut stage = [false; 2]; // flip-flops, true = high
        let mut since = 0usize;
        let mut sub_hz = lowpass / (1 << octaves) as f32;
        let mut out = Vec::with_capacity(cx.n);
        for i in 0..cx.n {
            let x = input.at(i);
            let y1 = lp[0].tick(x);
            let y = flush(lp[1].tick(y1));
            let amp = (2.0 * band_power.tick(band.tick(x).powi(2), level_t, level_t, sr)).sqrt();
            let own = (2.0 * lp_power.tick(y * y, level_t, level_t, sr)).sqrt();
            let hyst = (0.1 * own).max(1e-5);
            since = since.saturating_add(1);
            if y < -hyst {
                armed = true;
            }
            if armed && y >= 0.0 {
                armed = false;
                stage[0] = !stage[0];
                let mut toggled = true;
                if octaves == 2 {
                    // The second flip-flop clocks on the first one's rising edge.
                    toggled = stage[0];
                    if toggled {
                        stage[1] = !stage[1];
                    }
                }
                if toggled {
                    // A toggle ends half a period of the sub wave.
                    sub_hz = (sr / (2.0 * since as f32)).clamp(5.0, lowpass);
                    since = 0;
                }
            }
            let square = if stage[octaves as usize - 1] {
                1.0
            } else {
                -1.0
            };
            let hz = 1.5 * sub_hz;
            let s1 = shape[0].tick(square, FilterMode::Lp, hz, 0.707, 0.0, sr);
            let sine = shape[1].tick(s1, FilterMode::Lp, hz, 0.707, 0.0, sr);
            // The filtered square's fundamental is 1.1 times the sine amp.
            out.push(x + mix * sine * amp / 1.1);
        }
        Ok(one(Buf::Audio(out)))
    }
}

// ------------------------------------------------------------------ exciter

/// A harmonic exciter: the band above `freq` is saturated and the new high
/// harmonics are added back to the input.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Exciter {
    /// Mono or stereo audio (each channel on its own).
    pub input: In,
    /// Hz, 20 ..= 0.45 x sample rate (default 3000): the corner of both
    /// high-pass filters (2nd order).
    #[serde(default)]
    pub freq: Option<f32>,
    /// Gain into the saturator (tanh), > 0 and <= 100 (default 2).
    #[serde(default)]
    pub drive: Option<f32>,
    /// Level of the saturated band added to the input, 0 ..= 10 (default
    /// 0.3). 0 returns the input unchanged.
    #[serde(default)]
    pub mix: Option<f32>,
}

impl Describe for Exciter {
    const BLURB: &'static str = "harmonic exciter: high-pass, saturate, high-pass, add back";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "freq": 3000, "drive": 2, "mix": 0.3})
    }
}

impl Eval for Exciter {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let top = 0.45 * cx.sr;
        let freq = self.freq.unwrap_or(3000.0);
        if !(freq.is_finite() && (20.0..=top).contains(&freq)) {
            return Err(format!(
                "freq: {freq}: give Hz between 20 and {top} (0.45 x the sample rate)"
            ));
        }
        let drive = self.drive.unwrap_or(2.0);
        if !(drive.is_finite() && drive > 0.0 && drive <= 100.0) {
            return Err(format!("drive: {drive}: give a number > 0 and <= 100"));
        }
        let mix = self.mix.unwrap_or(0.3);
        if !(mix.is_finite() && (0.0..=10.0).contains(&mix)) {
            return Err(format!("mix: {mix}: give a number between 0 and 10"));
        }
        let sr = cx.sr;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut pre = Biquad::new();
            let mut post = Biquad::new();
            pre.set(FilterMode::Hp, freq, 0.707, 0.0, sr);
            post.set(FilterMode::Hp, freq, 0.707, 0.0, sr);
            x.iter()
                .map(|&v| {
                    let band = (drive * pre.tick(v)).tanh();
                    v + mix * post.tick(band)
                })
                .collect()
        })))
    }
}
