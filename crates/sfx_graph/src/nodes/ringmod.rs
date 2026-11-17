//! `ringmod`: the reference node. Every later node copies this file's shape:
//! a strict serde struct with a `///` comment per field (unit, default,
//! range), a [`Describe`] impl, an [`Eval`] impl whose errors follow rule 6
//! of the swarm plan (field, value, what to do), and tests in
//! `crates/sfx_analysis/tests/ringmod.rs`.

use serde::Deserialize;

use super::{one, per_channel};
use crate::dsp::{Osc, Wave};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// The carrier waveform. The field that holds it is called `shape`, one of
/// the names every node may use for a word (rule 5), so its value is never
/// read as a reference to another node.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RingShape {
    #[default]
    Sine,
    Tri,
    Square,
    Saw,
}

impl RingShape {
    fn wave(self) -> Wave {
        match self {
            RingShape::Sine => Wave::Sine,
            RingShape::Tri => Wave::Tri,
            RingShape::Square => Wave::Square,
            RingShape::Saw => Wave::Saw,
        }
    }
}

/// A ring modulator: multiplies the input by an internal oscillator.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RingMod {
    /// The sound to modulate: mono or stereo (each channel is multiplied by
    /// the same carrier).
    pub input: In,
    /// Carrier frequency, Hz. A number, or a node for a sweep or vibrato.
    /// A number must be within +-sample rate / 2 (24000 at 48 kHz); a
    /// signal is clamped to that range per sample. Negative runs the
    /// carrier backwards, which changes nothing audible for a sine and
    /// reverses the phase of the others. 0 holds the carrier at its start
    /// value, so a sine carrier then outputs silence.
    pub freq: In,
    /// Carrier waveform: sine (default), tri, square, saw. Square and saw
    /// are band-limited (polyBLEP) but their upper sidebands can still
    /// fold past Nyquist; keep `freq` low or filter after.
    #[serde(default)]
    pub shape: RingShape,
    /// Wet amount, 0..1 (default 1): 0 is the dry input, 1 the product, in
    /// between a linear crossfade. A number outside 0..1 is an error; a
    /// signal is clamped to 0..1 per sample.
    #[serde(default)]
    pub mix: Option<In>,
}

impl Describe for RingMod {
    const BLURB: &'static str = "ring modulator: input times a sine/tri/square/saw carrier at freq (sum and difference tones)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "freq": 300, "shape": "sine", "mix": 1.0})
    }
}

impl Eval for RingMod {
    // Sample-by-sample with no lookahead, so the defaults hold: causal, and
    // not random (no seed is read).
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let freq = cx.mono("freq", &self.freq)?;
        let mix = cx.mono_or("mix", &self.mix, 1.0)?;
        // Work cap: nothing here scales with a field; the one buffer is
        // `cx.n` samples.
        let limit = cx.sr / 2.0;
        if let Some(f) = freq.constant()
            && !(f.is_finite() && f.abs() <= limit)
        {
            return Err(format!(
                "freq: {f}: give a number between -{limit} and {limit} Hz (half the sample rate)"
            ));
        }
        if let Some(m) = mix.constant()
            && !(0.0..=1.0).contains(&m)
        {
            return Err(format!("mix: {m}: give a number from 0 to 1"));
        }
        // One carrier for every channel. A non-finite frequency from a
        // signal becomes 0 so the oscillator phase can never turn NaN.
        let mut osc = Osc::new(self.shape.wave(), 0.0);
        let carrier: Vec<f32> = (0..cx.n)
            .map(|i| {
                let f = freq.at(i);
                let f = if f.is_finite() {
                    f.clamp(-limit, limit)
                } else {
                    0.0
                };
                osc.tick(f, 0.5, cx.sr)
            })
            .collect();
        // The wet/dry blend is v * (1 - m) + v * carrier * m, so m = 0 passes
        // the input bit for bit.
        Ok(one(per_channel(&input, cx.n, |x| {
            x.iter()
                .zip(&carrier)
                .enumerate()
                .map(|(i, (v, c))| {
                    let m = mix.at(i);
                    let m = if m.is_finite() {
                        m.clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    v * (1.0 - m) + v * c * m
                })
                .collect()
        })))
    }
}
