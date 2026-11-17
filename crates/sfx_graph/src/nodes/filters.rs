//! Filters.

use serde::Deserialize;

use super::{one, per_channel};
use crate::dsp::{Biquad, DelayLine, FilterMode, OnePole, Resonator, Svf};
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

/// A state-variable filter (12 dB an octave; lp and hp can be steeper).
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
    /// Steepness in dB an octave: 12 (default), 24 or 48. lp and hp only
    /// (cascaded 12 dB sections with Butterworth Q spread, flat in the
    /// passband); `q` scales the last section, so the default q is a pure
    /// Butterworth. Not available inside a feedback loop.
    #[serde(default)]
    pub slope: Option<f32>,
}

impl Describe for Filter {
    const BLURB: &'static str = "state-variable filter lp/hp/bp/notch/peak/lowshelf/highshelf; freq and q take signals; slope 12/24/48 for lp and hp";
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
        let sections = slope_sections(self.slope, self.mode)?;
        if sections == 1 {
            return Ok(one(per_channel(&input, cx.n, |x| {
                let mut f = Svf::default();
                x.iter()
                    .enumerate()
                    .map(|(i, v)| f.tick(*v, mode, freq.at(i), q.at(i), g.at(i), cx.sr))
                    .collect()
            })));
        }
        // The Butterworth Q of each section, lowest first; `q` rescales the
        // last (highest) one, so q = 1/sqrt(2) keeps the maximally flat shape.
        let qs = butterworth_qs(sections);
        let last = sections - 1;
        let k = qs[last] / std::f32::consts::FRAC_1_SQRT_2;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = vec![Svf::default(); sections];
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut y = *v;
                    for (j, s) in f.iter_mut().enumerate() {
                        let qj = if j == last { q.at(i) * k } else { qs[j] };
                        y = s.tick(y, mode, freq.at(i), qj, 0.0, cx.sr);
                    }
                    y
                })
                .collect()
        })))
    }
}

/// How many 12 dB sections `slope` needs (1, 2 or 4), or the error.
fn slope_sections(slope: Option<f32>, mode: FilterModeSpec) -> Result<usize, String> {
    let Some(s) = slope else { return Ok(1) };
    let n = if s == 12.0 {
        1
    } else if s == 24.0 {
        2
    } else if s == 48.0 {
        4
    } else {
        return Err(format!("slope: {s}: give 12, 24 or 48"));
    };
    if n > 1 && !matches!(mode, FilterModeSpec::Lp | FilterModeSpec::Hp) {
        return Err(format!(
            "slope: {s}: only lp and hp can be steeper than 12; this mode is 12 dB an octave (cascade two filter nodes for more)"
        ));
    }
    Ok(n)
}

/// The Q of each second-order section of a Butterworth filter of order
/// 2 * `sections`, lowest Q first: Q_k = 1 / (2 sin((2k - 1) pi / (4 sections))).
fn butterworth_qs(sections: usize) -> Vec<f32> {
    let order = 2.0 * sections as f64;
    let mut qs: Vec<f32> = (1..=sections)
        .map(|k| {
            let theta = (2.0 * k as f64 - 1.0) * std::f64::consts::PI / (2.0 * order);
            (1.0 / (2.0 * theta.sin())) as f32
        })
        .collect();
    qs.sort_by(f32::total_cmp);
    qs
}

/// Moog-style transistor ladder: four one-pole stages with a tanh at each
/// stage input and a global resonance feedback.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ladder {
    pub input: In,
    /// Cutoff, Hz (a signal; clamped to 1 Hz .. 0.49 x sample rate).
    pub freq: In,
    /// Feedback, 0..1.2 (a signal, default 0; clamped). Around 1 the filter
    /// self-oscillates at `freq` once anything excites it. The passband gain
    /// falls as 1 / (1 + 4 x resonance).
    #[serde(default)]
    pub resonance: Option<In>,
    /// Gain into the first stage (default 1, 0.01..100). The tanh stages clip
    /// the sum; the output is not compensated for it.
    #[serde(default)]
    pub drive: Option<f32>,
    /// lp (default, 24 dB an octave), bp (12 dB an octave each side) or hp
    /// (24 dB an octave).
    #[serde(default)]
    pub mode: LadderMode,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LadderMode {
    #[default]
    Lp,
    Bp,
    Hp,
}

impl Describe for Ladder {
    const BLURB: &'static str =
        "four-pole nonlinear ladder filter lp/bp/hp; resonance near 1 self-oscillates at freq";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "mode": "lp", "freq": "cutoff_env", "resonance": 0.7, "drive": 2.0})
    }
}

/// Four zero-delay (trapezoidal) one-pole stages, a tanh in front of each,
/// and the resonance loop solved from the linear chain.
#[derive(Default)]
struct LadderCore {
    s: [f64; 4],
}

impl LadderCore {
    #[inline]
    fn tick(&mut self, x: f32, mode: LadderMode, hz: f32, res: f32, drive: f32, sr: f32) -> f32 {
        let x = if x.is_finite() { f64::from(x) } else { 0.0 };
        let hz = if hz.is_finite() { hz } else { 1000.0 };
        let res = if res.is_finite() { res } else { 0.0 };
        let g = (std::f64::consts::PI * f64::from(hz.clamp(1.0, crate::dsp::max_hz(sr)))
            / f64::from(sr))
        .tan();
        let gg = g / (1.0 + g);
        let c = 1.0 - gg;
        let k = 4.0 * f64::from(res.clamp(0.0, 1.2));
        // The part of the linear chain's output that the states already fix.
        let sg = gg * gg * gg * c * self.s[0]
            + gg * gg * c * self.s[1]
            + gg * c * self.s[2]
            + c * self.s[3];
        let u = f64::from(drive) * x;
        let fed = (u - k * sg) / (1.0 + k * gg * gg * gg * gg);
        let mut y = [0.0f64; 5];
        y[0] = fed.tanh();
        for i in 0..4 {
            let v = (if i == 0 { y[0] } else { y[i].tanh() } - self.s[i]) * gg;
            y[i + 1] = v + self.s[i];
            self.s[i] = flush64(y[i + 1] + v);
        }
        let out = match mode {
            LadderMode::Lp => y[4],
            LadderMode::Hp => y[0] - 4.0 * y[1] + 6.0 * y[2] - 4.0 * y[3] + y[4],
            LadderMode::Bp => 4.0 * (y[2] - 2.0 * y[3] + y[4]),
        };
        if out.is_finite() { out as f32 } else { 0.0 }
    }
}

fn flush64(y: f64) -> f64 {
    if y.abs() < 1e-30 { 0.0 } else { y }
}

impl Eval for Ladder {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let freq = cx.mono("freq", &self.freq)?;
        let res = cx.mono_or("resonance", &self.resonance, 0.0)?;
        let drive = self.drive.unwrap_or(1.0);
        if !(0.01..=100.0).contains(&drive) {
            return Err(format!("drive: {drive}: give a number from 0.01 to 100"));
        }
        let mode = self.mode;
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut f = LadderCore::default();
            x.iter()
                .enumerate()
                .map(|(i, v)| f.tick(*v, mode, freq.at(i), res.at(i), drive, cx.sr))
                .collect()
        })))
    }
}

/// The most bands an `eq` takes.
pub const EQ_MAX_BANDS: usize = 16;

/// One band of an `eq`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EqBand {
    /// lp, hp, bp, notch, peak, lowshelf or highshelf (as `filter`).
    pub mode: FilterModeSpec,
    /// Cutoff / centre, Hz (1 .. 0.49 x sample rate).
    pub freq: f32,
    /// peak and shelves: boost (+) or cut (-), dB (-60..60, default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Resonance / bandwidth (0.025..1000, default 0.707; higher is
    /// narrower for peak, notch and bp).
    #[serde(default)]
    pub q: Option<f32>,
}

/// A chain of up to 16 filter bands (parametric equalizer).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Equalizer {
    pub input: In,
    /// Up to 16 bands, applied in order. An empty list is the input.
    pub bands: Vec<EqBand>,
}

impl Describe for Equalizer {
    const BLURB: &'static str =
        "parametric EQ: up to 16 bands of lp/hp/bp/notch/peak/lowshelf/highshelf, fixed settings";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "bands": [
            {"mode": "highshelf", "freq": 6000, "gain_db": -4},
            {"mode": "peak", "freq": 1000, "gain_db": 6, "q": 2}]})
    }
}

impl Eval for Equalizer {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        if self.bands.len() > EQ_MAX_BANDS {
            return Err(format!(
                "bands: {} bands: give at most {EQ_MAX_BANDS} (merge or drop some)",
                self.bands.len()
            ));
        }
        let top = crate::dsp::max_hz(cx.sr);
        let mut tuned = Vec::new();
        for (i, b) in self.bands.iter().enumerate() {
            if !(b.freq.is_finite() && (1.0..=top).contains(&b.freq)) {
                return Err(format!(
                    "bands[{i}].freq: {}: give 1 to {top:.0} Hz (0.49 x the sample rate)",
                    b.freq
                ));
            }
            let g = b.gain_db.unwrap_or(0.0);
            if !(g.is_finite() && (-60.0..=60.0).contains(&g)) {
                return Err(format!("bands[{i}].gain_db: {g}: give -60 to 60 dB"));
            }
            let q = b.q.unwrap_or(std::f32::consts::FRAC_1_SQRT_2);
            if !(q.is_finite() && (0.025..=1000.0).contains(&q)) {
                return Err(format!("bands[{i}].q: {q}: give 0.025 to 1000"));
            }
            let mut bq = Biquad::new();
            bq.set(b.mode.mode(), b.freq, q, g, cx.sr);
            tuned.push(bq);
        }
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut bands = tuned.clone();
            x.iter()
                .map(|v| bands.iter_mut().fold(*v, |y, b| b.tick(y)))
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
