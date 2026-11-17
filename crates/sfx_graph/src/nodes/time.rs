//! Time: delay, reverb, convolution.

use serde::Deserialize;

use super::{one, per_channel};
use crate::buf::Buf;
use crate::dsp::{DelayLine, flush};
use crate::node::{Cx, Eval, In, Outs, is_file};

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

/// An 8-line feedback delay network.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reverb {
    pub input: In,
    /// Room size 0 (closet) .. 1 (hall), default 0.5.
    #[serde(default)]
    pub size: Option<f32>,
    /// Seconds for the tail to fall 60 dB (default 1.5).
    #[serde(default)]
    pub decay: Option<f32>,
    /// High-frequency damping 0..1 (default 0.3).
    #[serde(default)]
    pub damp: Option<f32>,
    /// Wet share 0..1 (default 0.3); port `wet` is the tail alone.
    #[serde(default)]
    pub mix: Option<f32>,
    /// Seconds before the tail starts (default 0.01).
    #[serde(default)]
    pub predelay: Option<f32>,
}

impl Eval for Reverb {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let size = self.size.unwrap_or(0.5).clamp(0.0, 1.0);
        let decay = self.decay.unwrap_or(1.5).max(0.05);
        let damp = self.damp.unwrap_or(0.3).clamp(0.0, 0.99);
        let mix = self.mix.unwrap_or(0.3).clamp(0.0, 1.0);
        let pre = (self.predelay.unwrap_or(0.01).max(0.0) * cx.sr) as usize;
        // Mutually prime-ish line lengths (ms) scaled by size.
        const BASE: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 53.9, 59.3, 67.1, 73.3];
        let scale = 0.35 + 1.65 * size;
        let lens: Vec<usize> = BASE
            .iter()
            .map(|ms| ((ms * scale * 0.001 * cx.sr) as usize).max(8))
            .collect();
        let gains: Vec<f32> = lens
            .iter()
            .map(|l| 10f32.powf(-3.0 * *l as f32 / (decay * cx.sr)))
            .collect();
        let mut lines: Vec<DelayLine> = lens.iter().map(|l| DelayLine::new(*l + 2)).collect();
        let mut lp = [0.0f32; 8];
        let n = cx.n;
        let mut wet = vec![[0.0f32; 2]; n];
        let mono_in: Vec<f32> = (0..n).map(|i| input.at(i)).collect();
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
