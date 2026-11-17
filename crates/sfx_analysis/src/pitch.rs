//! Fundamental frequency (YIN, de Cheveigné & Kawahara 2002).

/// One analysed frame.
#[derive(Debug, Clone, Copy)]
pub struct PitchFrame {
    /// Seconds (frame centre).
    pub t: f32,
    /// Hz, when voiced.
    pub hz: Option<f32>,
    /// The cumulative-mean-normalized difference at the chosen lag: 0 =
    /// perfectly periodic, 1 = noise.
    pub aperiodicity: f32,
    /// Frame RMS, dBFS.
    pub level_db: f32,
}

/// YIN over `x`: 1024-sample windows (at 48 kHz; scaled with the rate),
/// lags for 40 Hz .. 4 kHz, threshold 0.15; frames under -50 dBFS are
/// unvoiced and skipped. Hops 512 samples, or longer so a long sound gets
/// at most ~400 frames.
pub fn yin(x: &[f32], sr: u32) -> Vec<PitchFrame> {
    let scale = sr as f32 / 48_000.0;
    let w = ((1024.0 * scale) as usize).max(64);
    let max_lag = ((sr as f32 / 40.0) as usize).min(2 * w);
    let min_lag = ((sr as f32 / 4000.0) as usize).max(2);
    let hop = ((512.0 * scale) as usize).max(x.len() / 400).max(1);
    let need = w + max_lag + 2;
    let mut out = Vec::new();
    if x.len() < need {
        return out;
    }
    let mut d = vec![0.0f32; max_lag + 2];
    let mut start = 0;
    while start + need <= x.len() {
        let frame = &x[start..start + need];
        let level = (frame[..w].iter().map(|v| v * v).sum::<f32>() / w as f32).sqrt();
        let level_db = 20.0 * level.max(1e-10).log10();
        if level_db <= -50.0 {
            out.push(PitchFrame {
                t: (start + w / 2) as f32 / sr as f32,
                hz: None,
                aperiodicity: 1.0,
                level_db,
            });
            start += hop;
            continue;
        }
        // Difference function.
        for (tau, dv) in d.iter_mut().enumerate().skip(1) {
            let mut s = 0.0f32;
            for j in 0..w {
                let e = frame[j] - frame[j + tau];
                s += e * e;
            }
            *dv = s;
        }
        // Cumulative mean normalized difference.
        let mut cm = vec![1.0f32; max_lag + 2];
        let mut run = 0.0f32;
        for tau in 1..=max_lag + 1 {
            run += d[tau];
            cm[tau] = if run > 0.0 {
                d[tau] * tau as f32 / run
            } else {
                1.0
            };
        }
        let mut pick = None;
        let mut tau = min_lag;
        while tau <= max_lag {
            if cm[tau] < 0.15 {
                while tau < max_lag && cm[tau + 1] < cm[tau] {
                    tau += 1;
                }
                pick = Some(tau);
                break;
            }
            tau += 1;
        }
        let best = pick.unwrap_or_else(|| {
            (min_lag..=max_lag)
                .min_by(|a, b| cm[*a].total_cmp(&cm[*b]))
                .unwrap_or(min_lag)
        });
        let ap = cm[best];
        let voiced = pick.is_some() && level_db > -50.0;
        let hz = if voiced {
            // Parabolic interpolation on the raw difference.
            let (a, b, c) = (d[best - 1], d[best], d[best + 1]);
            let den = a - 2.0 * b + c;
            let off = if den.abs() > 1e-12 {
                0.5 * (a - c) / den
            } else {
                0.0
            };
            Some(sr as f32 / (best as f32 + off.clamp(-1.0, 1.0)))
        } else {
            None
        };
        out.push(PitchFrame {
            t: (start + w / 2) as f32 / sr as f32,
            hz,
            aperiodicity: ap,
            level_db,
        });
        start += hop;
    }
    out
}

/// Cents from `a` to `b`.
pub fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (b / a).log2()
}
