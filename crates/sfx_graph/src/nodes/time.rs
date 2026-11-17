//! Time: delay, reverb, convolution.

use serde::Deserialize;

use super::{one, per_channel, space};
use crate::buf::Buf;
use crate::dsp::{DelayLine, flush};
use crate::node::{Cx, Describe, Eval, In, Outs, is_file};

/// An interpolated delay.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Delay {
    pub input: In,
    /// Seconds (a node modulates it: chorus, flanger, doppler).
    pub time: In,
    /// Longest delay the line holds, seconds (default: the longest `time`).
    #[serde(default)]
    pub max: Option<f32>,
    /// Echo feedback inside the line, -0.99..0.99 (default 0: one echo).
    #[serde(default)]
    pub feedback: Option<In>,
}

impl Describe for Delay {
    const BLURB: &'static str =
        "interpolated delay with optional internal feedback; time may be a signal";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "time": 0.25, "feedback": 0.4})
    }
}

impl Eval for Delay {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let time = cx.mono("time", &self.time)?;
        let fb = cx.mono_or("feedback", &self.feedback, 0.0)?;
        let longest = match self.max {
            Some(m) => m.max(0.0),
            None => (0..cx.n.max(1)).map(|i| time.at(i)).fold(0.0f32, f32::max),
        };
        let max = ((longest * cx.sr).ceil() as usize + 2).min(60 * cx.sr as usize);
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut line = DelayLine::new(max);
            x.iter()
                .enumerate()
                .map(|(i, v)| {
                    let d = (time.at(i) * cx.sr).max(1.0);
                    let back = line.read(d - 1.0);
                    line.write(v + fb.at(i).clamp(-0.99, 0.99) * back);
                    back
                })
                .collect()
        })))
    }
}

/// Which reverb algorithm renders the tail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReverbModel {
    /// The 8-line feedback delay network (default).
    #[default]
    Fdn,
    /// Early reflections from a shoebox room (image sources, order 3),
    /// then the feedback delay network.
    Room,
    /// Four input diffusers into short feedback lines: dense, bright, a
    /// fast build.
    Plate,
    /// A feedback loop through a chain of dispersive allpass sections: the
    /// spring's chirp, repeated every round trip.
    Spring,
}

/// A reverb: an 8-line feedback delay network, or one of three other models.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reverb {
    pub input: In,
    /// Room size 0 (closet) .. 1 (hall), default 0.5. `fdn`: scales the
    /// line lengths; `room`: the room's longest side, 3 m (0) to 20 m (1);
    /// `plate`: scales the line lengths (0.6x .. 1.4x); `spring`: the loop
    /// time, 20 ms (0) to 80 ms (1).
    #[serde(default)]
    pub size: Option<f32>,
    /// Seconds for the tail to fall 60 dB (default 1.5).
    #[serde(default)]
    pub decay: Option<f32>,
    /// High-frequency damping 0..1 (default 0.3; `plate` 0.1).
    #[serde(default)]
    pub damp: Option<f32>,
    /// Wet share 0..1 (default 0.3); port `wet` is the tail alone.
    #[serde(default)]
    pub mix: Option<f32>,
    /// Seconds before the tail starts (default 0.01).
    #[serde(default)]
    pub predelay: Option<f32>,
    /// Algorithm: fdn (default; identical to the reverb before models
    /// existed), room, plate, spring.
    #[serde(default)]
    pub model: ReverbModel,
    /// `room` only: the early reflections' share of the wet signal, 0 (the
    /// tail alone) .. 1 (the reflections alone); default 0.3. An error for
    /// other models.
    #[serde(default)]
    pub er_mix: Option<f32>,
    /// Stereo width of the wet signal, 0 (mono) .. 2 (side doubled); 1 is
    /// the model's own width. Default: the model's own width.
    #[serde(default)]
    pub width: Option<f32>,
}

impl Describe for Reverb {
    const BLURB: &'static str = "reverb, stereo out: model fdn (8-line feedback delay network), room (shoebox early reflections + fdn), plate (dense, bright), spring (dispersive chirp)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "size": 0.6, "decay": 1.8, "damp": 0.4, "mix": 0.3})
    }
}

/// The feedback delay network's wet signal for `mono_in`, started `pre`
/// samples late. This is the reverb as it was before `model` existed; the
/// arithmetic must not change (a test pins the default's output bit for bit).
pub(crate) fn fdn_tail(
    mono_in: &[f32],
    pre: usize,
    size: f32,
    decay: f32,
    damp: f32,
    sr: f32,
) -> Vec<[f32; 2]> {
    // Mutually prime-ish line lengths (ms) scaled by size.
    const BASE: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.9, 59.3, 67.1, 73.3];
    let scale = 0.35 + 1.65 * size;
    let lens: Vec<usize> = BASE
        .iter()
        .map(|ms| ((ms * scale * 0.001 * sr) as usize).max(8))
        .collect();
    let gains: Vec<f32> = lens
        .iter()
        .map(|l| 10f32.powf(-3.0 * *l as f32 / (decay * sr)))
        .collect();
    let mut lines: Vec<DelayLine> = lens.iter().map(|l| DelayLine::new(*l + 2)).collect();
    let mut lp = [0.0f32; 8];
    let n = mono_in.len();
    let mut wet = vec![[0.0f32; 2]; n];
    for i in 0..n {
        let x = if i >= pre { mono_in[i - pre] } else { 0.0 };
        let mut v = [0.0f32; 8];
        for k in 0..8 {
            let back = lines[k].read((lens[k] - 1) as f32);
            lp[k] = flush(back + damp * (lp[k] - back));
            v[k] = lp[k] * gains[k];
        }
        // Fast Hadamard transform, normalized: an energy-preserving mix.
        let mut h = 1;
        while h < 8 {
            for j in (0..8).step_by(h * 2) {
                for k in j..j + h {
                    let (a, b) = (v[k], v[k + h]);
                    v[k] = a + b;
                    v[k + h] = a - b;
                }
            }
            h *= 2;
        }
        let norm = 1.0 / 8f32.sqrt();
        for k in 0..8 {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            lines[k].write(v[k] * norm + sign * x * 0.35);
        }
        let l = lp[0] + lp[2] + lp[4] + lp[6];
        let r = lp[1] + lp[3] + lp[5] + lp[7];
        wet[i] = [l * 0.5, r * 0.5];
    }
    wet
}

impl Eval for Reverb {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        if let Some(e) = self.er_mix
            && self.model != ReverbModel::Room
        {
            return Err(format!(
                "er_mix: {e}: only model room has early reflections: remove er_mix or set \"model\": \"room\""
            ));
        }
        let er_mix = self.er_mix.unwrap_or(0.3);
        if !(0.0..=1.0).contains(&er_mix) {
            return Err(format!("er_mix: {er_mix}: give a number from 0 to 1"));
        }
        if let Some(w) = self.width
            && !(0.0..=2.0).contains(&w)
        {
            return Err(format!("width: {w}: give a number from 0 (mono) to 2"));
        }
        let size = self.size.unwrap_or(0.5).clamp(0.0, 1.0);
        let decay = self.decay.unwrap_or(1.5).max(0.05);
        let default_damp = if self.model == ReverbModel::Plate {
            0.1
        } else {
            0.3
        };
        let damp = self.damp.unwrap_or(default_damp).clamp(0.0, 0.99);
        let mix = self.mix.unwrap_or(0.3).clamp(0.0, 1.0);
        let pre = (self.predelay.unwrap_or(0.01).max(0.0) * cx.sr) as usize;
        let n = cx.n;
        let mono_in: Vec<f32> = (0..n).map(|i| input.at(i)).collect();
        let mut wet = match self.model {
            ReverbModel::Fdn => fdn_tail(&mono_in, pre, size, decay, damp, cx.sr),
            ReverbModel::Room => space::room_wet(&mono_in, pre, size, decay, damp, er_mix, cx.sr),
            ReverbModel::Plate => space::plate_wet(&mono_in, pre, size, decay, damp, cx.sr),
            ReverbModel::Spring => space::spring_wet(&mono_in, pre, size, decay, damp, cx.sr),
        };
        if let Some(w) = self.width {
            for s in &mut wet {
                let (m, d) = ((s[0] + s[1]) * 0.5, (s[0] - s[1]) * 0.5 * w);
                *s = [m + d, m - d];
            }
        }
        let out: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let d = [input.ch(0, i), input.ch(1, i)];
                [
                    d[0] * (1.0 - mix) + wet[i][0] * mix,
                    d[1] * (1.0 - mix) + wet[i][1] * mix,
                ]
            })
            .collect();
        Ok(vec![("out", Buf::Stereo(out)), ("wet", Buf::Stereo(wet))])
    }
}

/// Convolution with an impulse response.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Convolve {
    pub input: In,
    /// A WAV file (".wav") or a node whose output is the response.
    pub ir: String,
    /// Wet share 0..1 (default 1: only the convolved signal).
    #[serde(default)]
    pub mix: Option<f32>,
    /// Linear gain on the wet signal (default 1).
    #[serde(default)]
    pub gain: Option<f32>,
}

impl Describe for Convolve {
    const BLURB: &'static str = "convolution with an impulse response (a WAV file or a node)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "ir": "refs/room_ir.wav", "mix": 0.5})
    }
}

impl Eval for Convolve {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let ir: Vec<Vec<f32>> = if is_file(&self.ir) {
            let a = crate::wav::read(&cx.path(&self.ir)).map_err(|e| format!("ir: {e}"))?;
            if a.sample_rate as f32 != cx.sr {
                return Err(format!(
                    "ir: {} is {} Hz but the sound is {} Hz: resample the file or set the sound's sample_rate",
                    self.ir, a.sample_rate, cx.sr
                ));
            }
            a.channels
        } else {
            let s = cx.audio("ir", &In::Name(self.ir.clone()))?;
            s.channels(cx.n)
        };
        let mix = self.mix.unwrap_or(1.0).clamp(0.0, 1.0);
        let gain = self.gain.unwrap_or(1.0);
        let stereo = input.is_stereo() || ir.len() > 1;
        let k = if stereo { 2 } else { 1 };
        let mut chans = Vec::with_capacity(k);
        for c in 0..k {
            let x: Vec<f32> = (0..cx.n).map(|i| input.ch(c, i)).collect();
            let h = &ir[c.min(ir.len() - 1)];
            let wet = fft_convolve(&x, h, cx.n)?;
            chans.push(
                x.iter()
                    .zip(wet)
                    .map(|(d, w)| d * (1.0 - mix) + w * gain * mix)
                    .collect(),
            );
        }
        Ok(one(Buf::from_channels(chans)))
    }
}

/// The first `n` samples of `x` convolved with `h` (one FFT, zero-padded).
pub fn fft_convolve(x: &[f32], h: &[f32], n: usize) -> Result<Vec<f32>, String> {
    use realfft::RealFftPlanner;
    if h.is_empty() || x.is_empty() {
        return Ok(vec![0.0; n]);
    }
    let len = (x.len() + h.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(len);
    let inv = planner.plan_fft_inverse(len);
    let mut a = vec![0.0f32; len];
    a[..x.len()].copy_from_slice(x);
    let mut b = vec![0.0f32; len];
    b[..h.len()].copy_from_slice(h);
    let mut fa = fwd.make_output_vec();
    let mut fb = fwd.make_output_vec();
    fwd.process(&mut a, &mut fa)
        .map_err(|e| format!("fft: {e}"))?;
    fwd.process(&mut b, &mut fb)
        .map_err(|e| format!("fft: {e}"))?;
    for (p, q) in fa.iter_mut().zip(&fb) {
        *p *= *q;
    }
    let mut out = inv.make_output_vec();
    inv.process(&mut fa, &mut out)
        .map_err(|e| format!("fft: {e}"))?;
    let scale = 1.0 / len as f32;
    Ok((0..n)
        .map(|i| out.get(i).copied().unwrap_or(0.0) * scale)
        .collect())
}
