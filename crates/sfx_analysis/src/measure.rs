//! The measures a macro's claim names (`sfx_graph::Measure`), each one a
//! single number over the whole sound. The macros check sweeps a knob and
//! reads one of these at five positions.

use sfx_graph::{Audio, Measure, gain_to_db, loudness};

use crate::stft;

/// Measures `audio`.
pub fn measure(audio: &Audio, m: Measure) -> f32 {
    let x = audio.mix_mono();
    match m {
        Measure::Lufs => loudness::integrated(&audio.channels, audio.sample_rate),
        Measure::Crest => {
            let peak = audio.peak();
            let n = audio.channels.iter().map(Vec::len).sum::<usize>().max(1);
            let ss: f64 = audio
                .channels
                .iter()
                .flatten()
                .map(|v| f64::from(*v).powi(2))
                .sum();
            gain_to_db(peak) - gain_to_db((ss / n as f64).sqrt() as f32)
        }
        Measure::Centroid | Measure::Flatness => {
            let sr = audio.sample_rate;
            let win = crate::descriptors::window_for(x.len(), sr);
            let s = stft::stft(&x, sr, win, win / 4);
            let mean = s.mean(0..s.frames.len());
            if m == Measure::Centroid {
                let (mut num, mut den) = (0.0f64, 0.0f64);
                for (b, p) in mean.iter().enumerate().skip(1) {
                    num += f64::from(s.hz(b)) * f64::from(*p);
                    den += f64::from(*p);
                }
                if den > 0.0 { (num / den) as f32 } else { 0.0 }
            } else {
                let v: Vec<f64> = mean[2..mean.len() - 1]
                    .iter()
                    .map(|p| f64::from(*p).max(1e-30))
                    .collect();
                let geo = (v.iter().map(|p| p.ln()).sum::<f64>() / v.len() as f64).exp();
                let ari = v.iter().sum::<f64>() / v.len() as f64;
                (geo / ari) as f32
            }
        }
        Measure::Decay => {
            // 10 ms RMS frames; from the last frame within 6 dB of the peak
            // to the last within 40 dB.
            let fr = (audio.sample_rate / 100).max(1) as usize;
            let lev: Vec<f32> = x
                .chunks(fr)
                .map(|c| {
                    10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
                        .max(1e-20)
                        .log10()
                })
                .collect();
            let Some(top) = lev.iter().copied().reduce(f32::max) else {
                return 0.0;
            };
            let last = |drop: f32| lev.iter().rposition(|v| *v > top - drop).unwrap_or(0);
            (last(40.0).saturating_sub(last(6.0))) as f32 / 100.0
        }
        Measure::Swing => {
            let lc: Vec<f32> = loudness::curve(&audio.channels, audio.sample_rate, 0.2, 0.05)
                .into_iter()
                .filter(|v| *v > -70.0)
                .collect();
            if lc.len() < 3 {
                return 0.0;
            }
            let mut s = lc;
            s.sort_by(f32::total_cmp);
            s[s.len() * 9 / 10] - s[s.len() / 10]
        }
    }
}

/// `v` formatted with the measure's unit.
pub fn fmt(m: Measure, v: f32) -> String {
    match m {
        Measure::Centroid if v >= 1000.0 => format!("{:.1}k Hz", v / 1000.0),
        Measure::Centroid => format!("{v:.0} Hz"),
        Measure::Flatness => format!("{v:.3}"),
        _ => format!("{v:.2} {}", m.unit()),
    }
}
