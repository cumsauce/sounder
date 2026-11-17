//! Loudness (ITU-R BS.1770-4) and true peak.
//!
//! Ported from foley's `foley-sim/src/analysis.rs` `lufs`, which hard-codes
//! the 48 kHz K-weighting coefficients; here they are derived for any sample
//! rate (the formulas libebur128 uses), and stereo channels are summed by
//! power as the standard says.

#[allow(clippy::excessive_precision, clippy::inconsistent_digit_grouping)]
/// The two K-weighting biquads (head shelf, then high-pass) at `sr`, as
/// (b, a) with a[0] = 1 left out.
fn k_weighting(sr: f64) -> [([f64; 3], [f64; 2]); 2] {
    use std::f64::consts::PI;
    let (f0, g, q) = (
        1681.974_450_955_533,
        3.999_843_853_973_347,
        0.707_175_236_955_419_6,
    );
    let k = (PI * f0 / sr).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = (
        [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    );
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (PI * f0 / sr).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = (
        [1.0, -2.0, 1.0],
        [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    );
    [shelf, hp]
}

/// `x` through the K-weighting filter, as f64.
fn weighted(x: &[f32], sr: u32) -> Vec<f64> {
    let mut y: Vec<f64> = x.iter().map(|v| f64::from(*v)).collect();
    for (b, a) in k_weighting(f64::from(sr)) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        for v in &mut y {
            let out = b[0] * *v + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, out);
            *v = out;
        }
    }
    y
}

fn loud(p: f64) -> f64 {
    -0.691 + 10.0 * p.max(1e-20).log10()
}

/// Mean K-weighted power (summed over channels) of each 400 ms block, hop
/// `hop_s` seconds. A sound shorter than one block is measured as one
/// zero-padded block.
fn block_powers(channels: &[Vec<f32>], sr: u32, hop_s: f64) -> Vec<f64> {
    let block = (f64::from(sr) * 0.4) as usize;
    let hop = ((f64::from(sr) * hop_s) as usize).max(1);
    let w: Vec<Vec<f64>> = channels.iter().map(|c| weighted(c, sr)).collect();
    let n = w.first().map_or(0, Vec::len);
    let count = if n <= block { 1 } else { (n - block) / hop + 1 };
    (0..count)
        .map(|k| {
            let s = k * hop;
            w.iter()
                .map(|c| {
                    c[s.min(c.len())..(s + block).min(c.len())]
                        .iter()
                        .map(|v| v * v)
                        .sum::<f64>()
                        / block as f64
                })
                .sum()
        })
        .collect()
}

/// Integrated loudness, LUFS: 400 ms blocks overlapping by 75 %, gated at
/// -70 LUFS and then 10 LU under the gated mean. -70 for silence.
pub fn integrated(channels: &[Vec<f32>], sr: u32) -> f32 {
    let powers = block_powers(channels, sr, 0.1);
    let gated: Vec<f64> = powers
        .iter()
        .copied()
        .filter(|p| loud(*p) > -70.0)
        .collect();
    if gated.is_empty() {
        return -70.0;
    }
    let mean = gated.iter().sum::<f64>() / gated.len() as f64;
    let rel: Vec<f64> = gated
        .iter()
        .copied()
        .filter(|p| loud(*p) > loud(mean) - 10.0)
        .collect();
    loud(rel.iter().sum::<f64>() / rel.len().max(1) as f64) as f32
}

/// Momentary loudness (400 ms window) every `hop_s` seconds, LUFS (floored
/// at -90).
pub fn momentary(channels: &[Vec<f32>], sr: u32, hop_s: f64) -> Vec<f32> {
    block_powers(channels, sr, hop_s)
        .into_iter()
        .map(|p| (loud(p) as f32).max(-90.0))
        .collect()
}

/// A loudness curve for display: K-weighted level in a `window_s` window
/// centred every `hop_s` seconds (zero-padded at the edges), LUFS floored at
/// -90. With a 0.4 s window this is momentary loudness centred on each time.
pub fn curve(channels: &[Vec<f32>], sr: u32, window_s: f64, hop_s: f64) -> Vec<f32> {
    let win = ((f64::from(sr) * window_s) as usize).max(1);
    let hop = ((f64::from(sr) * hop_s) as usize).max(1);
    let w: Vec<Vec<f64>> = channels.iter().map(|c| weighted(c, sr)).collect();
    let n = w.first().map_or(0, Vec::len);
    // Prefix sums of power per channel.
    let pre: Vec<Vec<f64>> = w
        .iter()
        .map(|c| {
            let mut acc = vec![0.0f64; c.len() + 1];
            for (i, v) in c.iter().enumerate() {
                acc[i + 1] = acc[i] + v * v;
            }
            acc
        })
        .collect();
    (0..n.div_ceil(hop).max(1))
        .map(|k| {
            let c = (k * hop) as isize;
            let a = (c - win as isize / 2).clamp(0, n as isize) as usize;
            let b = (c + win as isize / 2).clamp(0, n as isize) as usize;
            let p: f64 = pre.iter().map(|s| (s[b] - s[a]) / win as f64).sum();
            (loud(p) as f32).max(-90.0)
        })
        .collect()
}

/// True peak (4x oversampled with a windowed-sinc interpolator), linear.
pub fn true_peak(channels: &[Vec<f32>]) -> f32 {
    const TAPS: isize = 12; // each side
    const UP: usize = 4;
    // Polyphase kernels for the 3 in-between phases (phase 0 is the sample).
    let kernels: Vec<Vec<f32>> = (1..UP)
        .map(|p| {
            let frac = p as f64 / UP as f64;
            (-TAPS + 1..=TAPS)
                .map(|k| {
                    let t = k as f64 - frac;
                    let sinc = if t.abs() < 1e-12 {
                        1.0
                    } else {
                        (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
                    };
                    let w = 0.5 + 0.5 * (std::f64::consts::PI * t / (TAPS as f64 + 1.0)).cos();
                    (sinc * w) as f32
                })
                .collect()
        })
        .collect();
    let mut peak = 0.0f32;
    for c in channels {
        let n = c.len() as isize;
        let at = |i: isize| if i < 0 || i >= n { 0.0 } else { c[i as usize] };
        for i in 0..n {
            peak = peak.max(c[i as usize].abs());
            for kern in &kernels {
                let mut acc = 0.0f32;
                for (j, k) in (-TAPS + 1..=TAPS).enumerate() {
                    acc += kern[j] * at(i + k);
                }
                peak = peak.max(acc.abs());
            }
        }
    }
    peak
}
