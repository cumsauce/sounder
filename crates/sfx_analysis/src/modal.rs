//! Modal analysis of a struck object: the partials of a recording as
//! `[hz, decay_s, gain]` rows for a `resonators` node.
//!
//! The spectrum picks candidate frequencies (a long window from the onset, so
//! close pairs separate). Each is then fitted in the time domain by peeling:
//! subtract the other modes' current models from the recording, demodulate the
//! rest at the mode's frequency, and fit a line to the log envelope (decay and
//! the amplitude at the hit) and to the unwrapped phase (a frequency
//! correction). A few passes over all modes let strong modes clean up the weak
//! ones near them. The gain is the amplitude the mode has at the hit, in the
//! recording's own scale, so a unit `impulse` into the node reproduces the
//! recording's level.

use realfft::RealFftPlanner;
use sfx_graph::Audio;

/// One fitted mode.
#[derive(Debug, Clone, Copy)]
pub struct FitMode {
    pub hz: f32,
    /// Seconds to fall 60 dB.
    pub decay_s: f32,
    /// Amplitude at the hit (linear, the recording's scale).
    pub gain: f32,
}

/// Longest stretch analysed after the onset, seconds.
const MAX_SECONDS: f32 = 3.0;
/// Spectrum window used for finding candidates.
const MAX_WINDOW: usize = 16_384;

/// First sample within 30 dB of the peak.
fn onset(x: &[f32]) -> usize {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let th = peak * 10f32.powf(-30.0 / 20.0);
    x.iter().position(|v| v.abs() >= th).unwrap_or(0)
}

/// Candidate frequencies (Hz, strongest first): peaks of the spectrum of the
/// first window after the onset, with a window that starts at full height
/// and falls (a struck object is loudest at the hit).
fn candidates(seg: &[f32], sr: u32, max: usize) -> Vec<f32> {
    let mut win = MAX_WINDOW;
    while win > 512 && win > seg.len() {
        win /= 2;
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(win);
    let mut input = fft.make_input_vec();
    let mut spec = fft.make_output_vec();
    for (i, v) in input.iter_mut().enumerate() {
        let w = (std::f32::consts::FRAC_PI_2 * i as f32 / win as f32).cos().powi(2);
        *v = seg.get(i).copied().unwrap_or(0.0) * w;
    }
    if fft.process(&mut input, &mut spec).is_err() {
        return Vec::new();
    }
    let mag: Vec<f32> = spec.iter().map(|z| z.norm()).collect();
    let top = mag.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return Vec::new();
    }
    let lo = ((20.0 * win as f32 / sr as f32) as usize).max(2);
    let hi = mag.len().saturating_sub(2);
    let mut peaks: Vec<usize> = (lo..hi)
        .filter(|&b| {
            mag[b] > mag[b - 1]
                && mag[b] >= mag[b + 1]
                && mag[b] > mag[b - 2]
                && mag[b] >= mag[b + 2]
                && mag[b] > top * 10f32.powf(-55.0 / 20.0)
        })
        .collect();
    peaks.sort_by(|a, b| mag[*b].total_cmp(&mag[*a]));
    let mut chosen: Vec<usize> = Vec::new();
    for b in peaks {
        if chosen.iter().all(|c| c.abs_diff(b) >= 3) {
            chosen.push(b);
        }
        if chosen.len() == max {
            break;
        }
    }
    chosen
        .into_iter()
        .map(|b| {
            let l = |k: usize| mag[k].max(1e-20).ln();
            let (a, c0, c) = (l(b - 1), l(b), l(b + 1));
            let den = a - 2.0 * c0 + c;
            let off = if den.abs() > 1e-9 {
                (0.5 * (a - c) / den).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            (b as f32 + off) * sr as f32 / win as f32
        })
        .collect()
}

/// A mode's parameters while fitting (amplitude, decay rate in 1/s,
/// frequency in rad/sample, phase at the onset).
#[derive(Clone, Copy)]
struct Param {
    amp: f64,
    alpha: f64,
    w: f64,
    phi: f64,
}

fn model(p: &Param, sr: f64, out: &mut [f64]) {
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f64 / sr;
        *o = p.amp * (-p.alpha * t).exp() * (p.w * i as f64 + p.phi).cos();
    }
}

/// Weighted least squares line y = a + b t (weights `w`).
fn line(pts: &[(f64, f64, f64)]) -> Option<(f64, f64)> {
    let sw: f64 = pts.iter().map(|p| p.2).sum();
    if sw <= 0.0 || pts.len() < 3 {
        return None;
    }
    let mt = pts.iter().map(|p| p.2 * p.0).sum::<f64>() / sw;
    let my = pts.iter().map(|p| p.2 * p.1).sum::<f64>() / sw;
    let sxx: f64 = pts.iter().map(|p| p.2 * (p.0 - mt).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| p.2 * (p.0 - mt) * (p.1 - my)).sum();
    let b = if sxx > 1e-30 { sxy / sxx } else { 0.0 };
    Some((my - b * mt, b))
}

/// Demodulates `res` at `p.w`, fits the envelope and phase, and returns the
/// updated parameters (None when the mode is not there).
fn fit(res: &[f64], p: &Param, sr: f64) -> Option<Param> {
    let f = p.w * sr / std::f64::consts::TAU;
    // Smoothing kernel: 4 periods, between 4 ms and 40 ms.
    let l = ((4.0 / f.max(1.0)).clamp(0.004, 0.04) * sr) as usize;
    let l = l.max(8) | 1;
    let h: Vec<f64> = (0..l)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * (i as f64 + 0.5) / l as f64).cos())
        .collect();
    let hs: f64 = h.iter().sum();
    let step = (l / 8).max(1);
    let half = l / 2;
    let mut z: Vec<(f64, f64, f64)> = Vec::new(); // (t, re, im)
    let mut n = half;
    while n + half < res.len() {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, hv) in h.iter().enumerate() {
            let k = n + i - half;
            let ph = p.w * k as f64;
            re += hv * res[k] * ph.cos();
            im -= hv * res[k] * ph.sin();
        }
        z.push((n as f64 / sr, 2.0 * re / hs, 2.0 * im / hs));
        n += step;
    }
    let mags: Vec<f64> = z.iter().map(|v| v.1.hypot(v.2)).collect();
    let top = mags.iter().copied().fold(0.0f64, f64::max);
    if top < 1e-9 {
        return None;
    }
    // The contiguous run from the start that stays within 40 dB of the top.
    let end = mags
        .iter()
        .position(|m| *m < top * 0.01)
        .unwrap_or(mags.len());
    if end < 3 {
        return None;
    }
    let env: Vec<(f64, f64, f64)> = (0..end)
        .map(|i| (z[i].0, mags[i].ln(), mags[i] * mags[i]))
        .collect();
    let (ln_a, slope) = line(&env)?;
    let alpha = (-slope).max(0.0);
    // Phase: unwrap, then a weighted line for the phase at t = 0 and the
    // frequency correction.
    let mut ph = Vec::with_capacity(end);
    let mut prev = 0.0f64;
    let mut off = 0.0f64;
    for i in 0..end {
        let a = z[i].2.atan2(z[i].1);
        if i > 0 {
            let d = a + off - prev;
            off -= (d / std::f64::consts::TAU).round() * std::f64::consts::TAU;
        }
        let v = a + off;
        prev = v;
        ph.push((z[i].0, v, mags[i] * mags[i]));
    }
    let (phi_t0, dphi) = line(&ph)?;
    // Frequency correction (phase slope, rad/s), kept small: a fit to a
    // handful of points must not run away.
    let dw = (dphi / sr).clamp(-0.5 * p.w.max(1e-3) * 0.02, 0.5 * p.w.max(1e-3) * 0.02);
    // The smoothing kernel scales a decaying exponential by a constant.
    let kern: f64 = h
        .iter()
        .enumerate()
        .map(|(i, hv)| hv * (-alpha * (i as f64 - half as f64) / sr).exp())
        .sum::<f64>()
        / hs;
    let amp = ln_a.exp() / kern.max(1e-9);
    Some(Param {
        amp,
        alpha,
        w: p.w + dw,
        phi: phi_t0,
    })
}

/// The strongest `count` modes of `audio` (mono mix), with decays and the
/// amplitude each has at the hit.
pub fn extract(audio: &Audio, count: usize) -> Vec<FitMode> {
    let sr = f64::from(audio.sample_rate);
    let x = audio.mix_mono();
    let t0 = onset(&x);
    let end = (t0 + (MAX_SECONDS * audio.sample_rate as f32) as usize).min(x.len());
    if end <= t0 + 64 {
        return Vec::new();
    }
    let seg = &x[t0..end];
    let cands = candidates(seg, audio.sample_rate, (count * 2).clamp(2, 32));
    if cands.is_empty() {
        return Vec::new();
    }
    let sig: Vec<f64> = seg.iter().map(|v| f64::from(*v)).collect();
    let n = sig.len();
    // Initial parameters: the candidate frequency, a medium decay, no amp.
    let mut params: Vec<Option<Param>> = cands
        .iter()
        .map(|hz| {
            Some(Param {
                amp: 0.0,
                alpha: 20.0,
                w: std::f64::consts::TAU * f64::from(*hz) / sr,
                phi: 0.0,
            })
        })
        .collect();
    let mut models: Vec<Vec<f64>> = vec![vec![0.0; n]; cands.len()];
    let mut total = vec![0.0f64; n];
    for _pass in 0..3 {
        for k in 0..cands.len() {
            let Some(p) = params[k] else { continue };
            let res: Vec<f64> = (0..n).map(|i| sig[i] - total[i] + models[k][i]).collect();
            let next = fit(&res, &p, sr);
            for i in 0..n {
                total[i] -= models[k][i];
            }
            match next {
                Some(q) if q.amp.is_finite() && q.amp > 0.0 => {
                    model(&q, sr, &mut models[k]);
                    params[k] = Some(q);
                }
                _ => {
                    models[k].iter_mut().for_each(|v| *v = 0.0);
                    params[k] = None;
                }
            }
            for i in 0..n {
                total[i] += models[k][i];
            }
        }
    }
    let mut out: Vec<FitMode> = params
        .into_iter()
        .flatten()
        .map(|p| FitMode {
            hz: (p.w * sr / std::f64::consts::TAU) as f32,
            decay_s: if p.alpha > 1e-3 {
                (1000f64.ln() / p.alpha).min(600.0) as f32
            } else {
                600.0
            },
            gain: p.amp as f32,
        })
        .collect();
    out.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    let top = out.first().map_or(0.0, |m| m.gain);
    out.retain(|m| m.gain >= top * 10f32.powf(-50.0 / 20.0));
    out.truncate(count);
    out.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    out
}
