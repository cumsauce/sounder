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
            // RMS frames; from the end of the first loud moment (a run of
            // frames within 6 dB of the peak, dips under 30 ms bridged) to
            // the last frame within 40 dB of the peak. A one-hit sound has
            // one run, so this is the last loud frame, as it always was; a
            // hit train measures from its first loud hit, so its value no
            // longer jumps when a later hit crosses -6 dB. Frames are 10 ms,
            // shorter for a sound under 0.5 s (a fiftieth of its length, 2 ms
            // at the least), so a click's ring-out is not one or two frames.
            let sr = audio.sample_rate as usize;
            let secs = x.len() as f32 / sr.max(1) as f32;
            let fr = if secs >= 0.5 {
                (sr / 100).max(1)
            } else {
                ((secs / 50.0 * sr as f32) as usize).max(sr / 500).max(1)
            };
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
            let bridge = (0.03 * sr as f32 / fr as f32).ceil() as usize;
            let loud = |i: usize| lev[i] > top - 6.0;
            let mut end = lev.iter().position(|v| *v > top - 6.0).unwrap_or(0);
            let mut i = end + 1;
            while i < lev.len() && i <= end + bridge {
                if loud(i) {
                    end = i;
                }
                i += 1;
            }
            let last40 = lev.iter().rposition(|v| *v > top - 40.0).unwrap_or(0);
            (last40.saturating_sub(end)) as f32 * fr as f32 / sr as f32
        }
        Measure::Inharmonicity => inharmonicity_cents(audio).unwrap_or(0.0),
        Measure::Roughness => roughness_pct(audio),
        Measure::Density => density_per_s(audio),
        Measure::Attack => attack_ms(audio),
        Measure::Spacing => spacing_s(audio),
        Measure::Ring => ring_ms(audio),
        Measure::Swing => {
            // Loudness over 0.2 s windows every 0.05 s; a sound under 0.6 s
            // uses a third of its length (20 ms at the least), so a short
            // sound still has enough windows to take percentiles of.
            let secs = x.len() as f64 / f64::from(audio.sample_rate.max(1));
            let win = if secs >= 0.6 {
                0.2
            } else {
                (secs / 3.0).max(0.02)
            };
            let lc: Vec<f32> = loudness::curve(&audio.channels, audio.sample_rate, win, win / 4.0)
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
        Measure::Density => format!("{v:.1} events/s"),
        _ => format!("{v:.2} {}", m.unit()),
    }
}

// ---------------------------------------------------------------------------
// Senses for synthesized sources: aliasing, inharmonicity, modulation,
// roughness. Each is a function of the audio alone. `Extra` gathers them for
// the descriptor table; the checks and (once `Measure` has the variants) the
// macro claims call the single functions.
// ---------------------------------------------------------------------------

use realfft::RealFftPlanner;

use crate::pitch;

/// A rate found in an amplitude envelope or a pitch track.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ModPeak {
    pub rate_hz: f32,
    /// Peak-to-peak size of the component: dB for the amplitude envelope,
    /// cents for the pitch track.
    pub depth: f32,
}

/// All four, for the descriptor table.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Extra {
    /// Off-grid energy above 4 kHz over total, dB (None: no steady pitched
    /// segment to measure).
    pub aliasing_db: Option<f32>,
    /// Mean deviation of the strongest 8 partials from integer multiples of
    /// f0, cents (None: fewer than 3 clear partials).
    pub inharmonicity_cents: Option<f32>,
    /// Up to 3 strongest amplitude-envelope rates, 0.5 to 200 Hz.
    pub amp_mod: Vec<ModPeak>,
    /// Up to 3 strongest pitch-track rates, 0.5 Hz up to 0.4 of the pitch
    /// frame rate (about 40 Hz at most).
    pub pitch_mod: Vec<ModPeak>,
    /// Percent of the envelope's energy in 20 to 150 Hz.
    pub roughness_pct: f32,
}

/// Measures all four.
pub fn extra(audio: &Audio) -> Extra {
    Extra {
        aliasing_db: aliasing_db(audio),
        inharmonicity_cents: inharmonicity_cents(audio),
        amp_mod: amplitude_modulation(audio),
        pitch_mod: pitch_modulation(audio),
        roughness_pct: roughness_pct(audio),
    }
}

impl Extra {
    /// The descriptor lines (empty-handed ones say why).
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(match self.aliasing_db {
            Some(v) => {
                format!("aliasing: {v:.1} dB (off the harmonic grid above 4 kHz, over total)")
            }
            None => "aliasing: n/a (no steady pitched segment)".into(),
        });
        out.push(match self.inharmonicity_cents {
            Some(v) => format!("inharmonicity: {v:.1} cents (mean over the strongest 8 partials)"),
            None => "inharmonicity: n/a (under 3 clear partials)".into(),
        });
        let list = |m: &[ModPeak], unit: &str| {
            if m.is_empty() {
                "none".to_string()
            } else {
                m.iter()
                    .map(|p| format!("{:.1} Hz {:.1} {unit}", p.rate_hz, p.depth))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        out.push(format!(
            "amplitude modulation (peak-to-peak): {}",
            list(&self.amp_mod, "dB")
        ));
        out.push(format!(
            "pitch modulation (peak-to-peak): {}",
            list(&self.pitch_mod, "cents")
        ));
        out.push(format!(
            "roughness: {:.1} % of the envelope energy in 20-150 Hz",
            self.roughness_pct
        ));
        out
    }
}

fn median_of(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Parabolic peak of three neighbouring values: (offset -0.5..0.5, value).
fn parabola(a: f32, b: f32, c: f32) -> (f32, f32) {
    let den = a - 2.0 * b + c;
    if den.abs() < 1e-20 {
        return (0.0, b);
    }
    let off = (0.5 * (a - c) / den).clamp(-0.5, 0.5);
    (off, b - 0.25 * (a - c) * off)
}

/// RMS envelope (Hann-weighted, 6 ms) every ~1 ms: (samples, rate Hz).
fn envelope(x: &[f32], sr: u32) -> (Vec<f32>, f32) {
    let hop = (sr as usize / 1000).max(1);
    let rate = sr as f32 / hop as f32;
    let w = stft::hann(((0.006 * sr as f32).round() as usize).max(8));
    let wsum: f32 = w.iter().sum();
    let half = (w.len() / 2) as isize;
    let env = (0..x.len().div_ceil(hop))
        .map(|k| {
            let start = (k * hop) as isize - half;
            let mut acc = 0.0f32;
            for (j, wj) in w.iter().enumerate() {
                let i = start + j as isize;
                if i >= 0 && (i as usize) < x.len() {
                    let v = x[i as usize];
                    acc += wj * v * v;
                }
            }
            (acc / wsum).sqrt()
        })
        .collect();
    (env, rate)
}

/// The part of `env` from the first to the last sample within 40 dB of its
/// peak (empty for silence).
fn active(env: &[f32]) -> &[f32] {
    let peak = env.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return &[];
    }
    let th = peak * 0.01;
    let a = env.iter().position(|v| *v > th).unwrap_or(0);
    let b = env.iter().rposition(|v| *v > th).unwrap_or(0);
    &env[a..=b.max(a)]
}

/// The strongest sinusoidal components of `track` between `lo` and `hi` Hz:
/// a linear trend is removed, a Hann window applied, the spectrum zero-padded
/// 8x; depth is twice the interpolated amplitude. At most 3 peaks, at least
/// `min_depth`, within 24 dB of the strongest, and not under two cycles of
/// the track's length.
fn mod_peaks(track: &[f32], rate: f32, lo: f32, hi: f32, min_depth: f32) -> Vec<ModPeak> {
    let n = track.len();
    if n < 16 || rate <= 0.0 {
        return Vec::new();
    }
    let secs = n as f32 / rate;
    let lo = lo.max(2.0 / secs);
    if hi <= lo {
        return Vec::new();
    }
    let mean = track.iter().sum::<f32>() / n as f32;
    let (mut sxy, mut sxx) = (0.0f64, 0.0f64);
    let xm = (n as f64 - 1.0) / 2.0;
    for (i, v) in track.iter().enumerate() {
        let dx = i as f64 - xm;
        sxy += dx * f64::from(*v - mean);
        sxx += dx * dx;
    }
    let slope = (sxy / sxx.max(1e-12)) as f32;
    let w = stft::hann(n);
    let sw: f32 = w.iter().sum();
    let size = (n * 8).next_power_of_two().clamp(64, 1 << 21);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut input = fft.make_input_vec();
    for (i, v) in track.iter().enumerate() {
        input[i] = (v - mean - slope * (i as f32 - xm as f32)) * w[i];
    }
    let mut out = fft.make_output_vec();
    if fft.process(&mut input, &mut out).is_err() {
        return Vec::new();
    }
    let amp: Vec<f32> = out.iter().map(|z| 2.0 * z.norm() / sw).collect();
    let df = rate / size as f32;
    let (k0, k1) = (
        ((lo / df).ceil() as usize).max(1),
        ((hi / df) as usize).min(amp.len() - 2),
    );
    let mut cand: Vec<usize> = (k0..=k1)
        .filter(|&k| amp[k] > amp[k - 1] && amp[k] >= amp[k + 1])
        .collect();
    cand.sort_by(|&a, &b| amp[b].total_cmp(&amp[a]));
    let guard = 2.5 / secs;
    let mut found: Vec<ModPeak> = Vec::new();
    for k in cand {
        let (off, a) = parabola(amp[k - 1], amp[k], amp[k + 1]);
        let f = (k as f32 + off) * df;
        if found.iter().any(|p| (p.rate_hz - f).abs() < guard) {
            continue;
        }
        let depth = 2.0 * a;
        if depth < min_depth || found.first().is_some_and(|p| depth < p.depth * 0.063) {
            break;
        }
        found.push(ModPeak { rate_hz: f, depth });
        if found.len() == 3 {
            break;
        }
    }
    found
}

/// The 3 strongest rates (0.5 to 200 Hz) of the loudness envelope in dB,
/// between the first and last moment within 40 dB of the peak, with their
/// peak-to-peak depth in dB (0.3 dB and up). The envelope is smoothed over
/// 6 ms, so depth reads low above about 100 Hz.
pub fn amplitude_modulation(audio: &Audio) -> Vec<ModPeak> {
    let x = audio.mix_mono();
    let (env, rate) = envelope(&x, audio.sample_rate);
    let span = active(&env);
    let peak = span.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return Vec::new();
    }
    let db: Vec<f32> = span
        .iter()
        .map(|v| 20.0 * (v.max(peak * 1e-3)).log10())
        .collect();
    mod_peaks(&db, rate, 0.5, 200.0, 0.3)
}

/// The 3 strongest rates of the pitch track (YIN, in cents), over the
/// longest run of voiced frames, with their peak-to-peak depth in cents (3
/// and up). The track's frame rate (about 94 Hz, less for a long sound) caps
/// the rates at 0.4 of itself.
pub fn pitch_modulation(audio: &Audio) -> Vec<ModPeak> {
    let x = audio.mix_mono();
    let frames = pitch::yin(&x, audio.sample_rate);
    if frames.len() < 16 {
        return Vec::new();
    }
    let rate = 1.0 / (frames[1].t - frames[0].t).max(1e-6);
    let (mut best, mut run_start) = ((0usize, 0usize), 0usize);
    for i in 0..=frames.len() {
        let voiced = i < frames.len() && frames[i].hz.is_some();
        if !voiced {
            if i - run_start > best.1 - best.0 {
                best = (run_start, i);
            }
            run_start = i + 1;
        }
    }
    let run: Vec<f32> = frames[best.0..best.1].iter().filter_map(|f| f.hz).collect();
    if run.len() < 16 {
        return Vec::new();
    }
    let mean_hz = run.iter().sum::<f32>() / run.len() as f32;
    let cents: Vec<f32> = run.iter().map(|h| pitch::cents(mean_hz, *h)).collect();
    mod_peaks(&cents, rate, 0.5, (0.4 * rate).min(200.0), 3.0)
}

/// Percent of the loudness envelope's energy (DC included, Hann-windowed,
/// between the first and last moment within 40 dB of the peak) that lies in
/// 20 to 150 Hz: two tones 40 Hz apart beat there, two tones 400 Hz apart do
/// not. A tone under about 80 Hz counts its own waveform; a hit counts its
/// attack. 0 for under 0.2 s.
pub fn roughness_pct(audio: &Audio) -> f32 {
    let x = audio.mix_mono();
    let (env, rate) = envelope(&x, audio.sample_rate);
    let span = active(&env);
    let n = span.len();
    if (n as f32) < 0.2 * rate {
        return 0.0;
    }
    let w = stft::hann(n);
    let size = n.next_power_of_two().clamp(64, 1 << 21);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut input = fft.make_input_vec();
    for (i, v) in span.iter().enumerate() {
        input[i] = v * w[i];
    }
    let mut out = fft.make_output_vec();
    if fft.process(&mut input, &mut out).is_err() {
        return 0.0;
    }
    let df = rate / size as f32;
    let (mut band, mut total) = (0.0f64, 0.0f64);
    for (k, z) in out.iter().enumerate() {
        let p = f64::from(z.norm_sqr());
        total += p;
        let f = k as f32 * df;
        if (20.0..=150.0).contains(&f) {
            band += p;
        }
    }
    if total <= 0.0 {
        0.0
    } else {
        (100.0 * band / total) as f32
    }
}

/// The strongest (up to 8) spectral peaks of the sound, Hz ascending. Peaks must be 20 dB over the median bin and within 45 dB
/// of the strongest, and 8 bins apart; analysed over the first 2^20 samples
/// of the audible part.
fn partials(x: &[f32], sr: u32) -> Vec<f32> {
    let (env, _) = envelope(x, sr);
    let hop = (sr as usize / 1000).max(1);
    let peak = env.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return Vec::new();
    }
    let a = env.iter().position(|v| *v > peak * 0.01).unwrap_or(0) * hop;
    let b = ((env.iter().rposition(|v| *v > peak * 0.01).unwrap_or(0) + 1) * hop).min(x.len());
    let seg = &x[a.min(b)..b.min(a + (1 << 20))];
    if seg.len() < 256 {
        return Vec::new();
    }
    let w = stft::hann(seg.len());
    let size = seg.len().next_power_of_two().clamp(256, 1 << 21);
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut input = fft.make_input_vec();
    for (i, v) in seg.iter().enumerate() {
        input[i] = v * w[i];
    }
    let mut out = fft.make_output_vec();
    if fft.process(&mut input, &mut out).is_err() {
        return Vec::new();
    }
    let p: Vec<f32> = out.iter().map(|z| z.norm_sqr()).collect();
    let df = sr as f32 / size as f32;
    let (k0, k1) = (
        ((30.0 / df).ceil() as usize).max(1),
        ((0.45 * sr as f32 / df) as usize).min(p.len() - 2),
    );
    if k1 <= k0 + 2 {
        return Vec::new();
    }
    let med = median_of(p[k0..=k1].to_vec()).unwrap_or(0.0).max(1e-20);
    let top = p[k0..=k1].iter().copied().fold(0.0f32, f32::max);
    let floor = (med * 100.0).max(top * 10f32.powf(-4.5));
    let mut cand: Vec<usize> = (k0..=k1)
        .filter(|&k| p[k] > floor && p[k] > p[k - 1] && p[k] >= p[k + 1])
        .collect();
    cand.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
    let mut picked: Vec<usize> = Vec::new();
    for k in cand {
        if picked.iter().all(|j| k.abs_diff(*j) >= 8) {
            picked.push(k);
            if picked.len() == 8 {
                break;
            }
        }
    }
    let mut hz: Vec<f32> = picked
        .into_iter()
        .map(|k| {
            let (off, _) = parabola(
                p[k - 1].max(1e-30).ln(),
                p[k].max(1e-30).ln(),
                p[k + 1].max(1e-30).ln(),
            );
            (k as f32 + off) * df
        })
        .collect();
    hz.sort_by(f32::total_cmp);
    hz
}

/// Mean absolute deviation, in cents, of the strongest 8 spectral partials
/// from integer multiples of f0, with f0 free within 2 % of its first guess
/// (the YIN median when most of the sound is pitched, else the lowest
/// partial) and set where the deviation is smallest. A harmonic tone reads
/// under 2; partials at 1, 2.1 and 3.3 times f0 read over 50. None: under 3
/// clear partials.
pub fn inharmonicity_cents(audio: &Audio) -> Option<f32> {
    let x = audio.mix_mono();
    let sr = audio.sample_rate;
    let hz = partials(&x, sr);
    if hz.len() < 3 {
        return None;
    }
    let frames: Vec<_> = pitch::yin(&x, sr)
        .into_iter()
        .filter(|f| f.level_db > -50.0)
        .collect();
    let voiced: Vec<f32> = frames.iter().filter_map(|f| f.hz).collect();
    let guess = if !frames.is_empty() && voiced.len() * 2 >= frames.len() {
        median_of(voiced)?
    } else {
        hz[0]
    };
    let dev = |f0: f32| {
        hz.iter()
            .map(|p| {
                let r = p / f0;
                (1200.0 * (r / r.round().max(1.0)).log2()).abs()
            })
            .sum::<f32>()
            / hz.len() as f32
    };
    Some(
        (-100..=100)
            .map(|i| dev(guess * (1.0 + i as f32 * 0.0002)))
            .fold(f32::MAX, f32::min),
    )
}

/// Energy off the harmonic grid above 4 kHz over the total energy of the
/// pitched frames, dB. For each analysis frame the sound's f0 (YIN, then
/// refined from the harmonics below 4 kHz) gives a grid; a bin within 2.5 bins
/// of a multiple is on it. Frames whose pitch moves more than 1.5 %, or where
/// under half of the energy below 4 kHz is on the grid, are skipped. The
/// window is 12 periods of the median f0 long (a power of two, 2048 to
/// 32768 at 48 kHz), so a pitch under about 60 Hz, or a sweeping or noisy
/// sound, gives None.
pub fn aliasing_db(audio: &Audio) -> Option<f32> {
    let x = audio.mix_mono();
    let sr = audio.sample_rate;
    let yin = pitch::yin(&x, sr);
    let f0m = median_of(yin.iter().filter_map(|f| f.hz).collect())?;
    let scale = sr as f32 / 48_000.0;
    let win = ((12.0 * sr as f32 / f0m).ceil() as usize)
        .clamp((2048.0 * scale) as usize, (32_768.0 * scale) as usize)
        .next_power_of_two()
        .max(256);
    let s = stft::stft(&x, sr, win, win / 2);
    let bin_hz = sr as f32 / win as f32;
    let loudest = s
        .frames
        .iter()
        .map(|f| f.iter().sum::<f32>())
        .fold(0.0f32, f32::max);
    let (mut off, mut total) = (0.0f64, 0.0f64);
    let mut used = 0;
    for (k, p) in s.frames.iter().enumerate() {
        let t = s.time(k);
        let half = 0.5 * win as f32 / sr as f32;
        let hzs: Vec<f32> = yin
            .iter()
            .filter(|f| (f.t - t).abs() <= half)
            .map(|f| f.hz.unwrap_or(0.0))
            .collect();
        let voiced: Vec<f32> = hzs.iter().copied().filter(|h| *h > 0.0).collect();
        if hzs.is_empty() || voiced.len() * 2 < hzs.len() {
            continue;
        }
        let (lo, hi) = voiced
            .iter()
            .fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        if hi > lo * 1.015 {
            continue;
        }
        let frame_total: f32 = p.iter().sum();
        if frame_total < loudest * 1e-5 {
            continue;
        }
        // Refine f0 from the harmonics below 4 kHz.
        let mut f = median_of(voiced)?;
        let mut hits = 0;
        for h in 1..=96usize {
            let e = h as f32 * f;
            if e > 4000.0 || e > 0.45 * sr as f32 {
                break;
            }
            let c = (e / bin_hz).round() as usize;
            let (a, b) = (c.saturating_sub(2).max(1), (c + 2).min(p.len() - 2));
            if a >= b {
                break;
            }
            let k = (a..=b).max_by(|i, j| p[*i].total_cmp(&p[*j]))?;
            if k == a || k == b || p[k] < frame_total * 1e-4 {
                continue;
            }
            let (off_b, _) = parabola(
                p[k - 1].max(1e-30).ln(),
                p[k].max(1e-30).ln(),
                p[k + 1].max(1e-30).ln(),
            );
            f = (k as f32 + off_b) * bin_hz / h as f32;
            hits += 1;
        }
        if hits == 0 || f < 5.0 * bin_hz {
            continue;
        }
        let (mut on_low, mut all_low, mut off_high) = (0.0f64, 0.0f64, 0.0f64);
        for (b, v) in p.iter().enumerate().skip(1) {
            let fb = b as f32 * bin_hz;
            let h = (fb / f).round();
            let on = h >= 1.0 && (fb - h * f).abs() <= 2.5 * bin_hz;
            if fb <= 4000.0 {
                all_low += f64::from(*v);
                if on {
                    on_low += f64::from(*v);
                }
            } else if !on {
                off_high += f64::from(*v);
            }
        }
        if on_low < 0.5 * all_low {
            continue;
        }
        off += off_high;
        total += f64::from(frame_total);
        used += 1;
    }
    (used > 0 && total > 0.0).then(|| (10.0 * (off.max(1e-30) / total).log10()).max(-120.0) as f32)
}

// ---------------------------------------------------------------------------
// Fine-grained events: density, attack, spacing, and the `onsets` check with
// a `spacing_ms` under the spectral detector's 30 ms.
// ---------------------------------------------------------------------------

/// RBJ high-pass (Q 0.707) of `x` at `hp_hz` (at most 0.2 of the rate).
fn highpass(x: &[f32], sr: u32, hp_hz: f32) -> Vec<f32> {
    let mut y = x.to_vec();
    let w0 = std::f32::consts::TAU * hp_hz.min(sr as f32 * 0.2) / sr as f32;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = (
        (1.0 + cos) / 2.0 / a0,
        -(1.0 + cos) / a0,
        (1.0 + cos) / 2.0 / a0,
    );
    let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for v in y.iter_mut() {
        let xn = f64::from(*v);
        let yn = f64::from(b0) * xn + f64::from(b1) * x1 + f64::from(b2) * x2
            - f64::from(a1) * y1
            - f64::from(a2) * y2;
        (x2, x1, y2, y1) = (x1, xn, y1, yn);
        *v = yn as f32;
    }
    y
}

/// One event of [`fine_events`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FineEvent {
    /// Seconds from the start.
    pub t: f32,
    /// The loudest 1 ms RMS chunk from the event to the end of its
    /// refractory time.
    pub level: f32,
}

/// Events from a 1 ms RMS envelope of `x` high-passed at `hp_hz` (RBJ, Q
/// 0.707; lower when the rate is low): a chunk at least 6 dB over the loudest
/// of the 3 chunks before it and within 45 dB of the loudest chunk, then
/// `refractory` chunks (ms) of silence for the detector. This is the
/// algorithm of the descriptors' `density` line (`hp_hz` 1000, `refractory`
/// 4), so `Measure::Density` agrees with it. A tone's ripple does not count
/// (nothing rises over a recent crest).
pub fn fine_events(x: &[f32], sr: u32, hp_hz: f32, refractory: usize) -> Vec<FineEvent> {
    let y = highpass(x, sr, hp_hz);
    let c = (sr as usize / 1000).max(1);
    let rms = |ch: &[f32]| (ch.iter().map(|v| v * v).sum::<f32>() / ch.len().max(1) as f32).sqrt();
    let e: Vec<f32> = y.chunks(c).map(rms).collect();
    let peak = e.iter().copied().fold(0.0f32, f32::max);
    let floor = peak * 0.0056;
    let refractory = refractory.max(1);
    let mut out = Vec::new();
    let mut k = 0;
    while k < e.len() && peak > 0.0 {
        let prev = (k.saturating_sub(3)..k)
            .map(|j| e[j])
            .fold(0.0f32, f32::max);
        if e[k] >= floor && e[k] >= 2.0 * prev {
            let level = e[k..(k + refractory).min(e.len())]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
            out.push(FineEvent {
                t: (k * c) as f32 / sr as f32,
                level,
            });
            k += refractory;
        } else {
            k += 1;
        }
    }
    out
}

/// Times (seconds) of the events a `spacing_ms` under the spectral
/// detector's 30 ms asks for: [`fine_events`] above 1 kHz, closer than
/// `spacing_ms` merged (the refractory time is `ceil(spacing_ms) - 1` ms, so
/// a train exactly `spacing_ms` apart is not lost to chunk rounding).
pub fn fine_onsets(audio: &Audio, spacing_ms: f32) -> Vec<f32> {
    let refractory = (spacing_ms.max(1.0).ceil() as usize)
        .saturating_sub(1)
        .max(1);
    fine_events(&audio.mix_mono(), audio.sample_rate, 1000.0, refractory)
        .into_iter()
        .map(|e| e.t)
        .collect()
}

/// How [`event_rate`] reached its number.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum RateMethod {
    /// The refractory counter's events over the span, uncorrected (regular
    /// or sparse: nothing is hidden).
    Count,
    /// The counter's rate corrected for events hidden by the tail of the
    /// one before.
    CountCorrected,
    /// Envelope peaks, corrected for overlap (ticks closer than the counter
    /// can separate).
    Peaks,
    /// The repetition rate of a periodic train, from the envelope's
    /// autocorrelation.
    Period,
}

/// What [`event_rate`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EventRate {
    /// Events per second.
    pub per_s: f32,
    pub method: RateMethod,
    /// [`fine_events`]' count over the span, per second, as is.
    pub counted_per_s: f32,
    /// Prominent envelope peaks per second, as is.
    pub peaks_per_s: f32,
    /// Repetition rate (Hz) from the envelope's autocorrelation, when the
    /// envelope repeats (ACF peak over 0.5 and envelope depth over 10 %).
    pub period_hz: Option<f32>,
}

/// Dead time, seconds, of the counter ([`fine_events`], 4 ms refractory,
/// 6 dB over the last 3 ms): on Poisson trains of ticks it reads
/// rate * exp(-rate * 5.5 ms) from 20 to 160/s (measured).
const COUNT_DEAD_S: f32 = 0.0055;

/// Dead time, seconds, of the envelope-peak counter on Poisson trains of
/// ticks with 3 ms decays: peaks / rate = exp(-rate * 1.7 ms) from 160 to
/// 500/s (measured; the three tick families tried spread +-25 % around it).
const PEAK_DEAD_S: f32 = 0.0017;

/// The peak counter takes over from the counter at this many peaks per
/// second (about 150 events/s of ticks), and only when the counter has
/// collapsed (reads under 1 / 1.6 of the peaks).
const DENSE_PEAKS_PER_S: f32 = 100.0;

/// 1 ms RMS envelope of `y`, one value every `sr / hop_per_s` samples.
fn rms_envelope_1ms(y: &[f32], sr: u32, hop_per_s: u32) -> (Vec<f32>, f32) {
    let hop = (sr / hop_per_s).max(1) as usize;
    let win = (sr as usize / 1000).max(hop);
    let mut cs = vec![0.0f64; y.len() + 1];
    for (i, v) in y.iter().enumerate() {
        cs[i + 1] = cs[i] + f64::from(*v) * f64::from(*v);
    }
    let mut e = Vec::new();
    let mut k = 0;
    while k + win <= y.len() {
        e.push(((cs[k + win] - cs[k]) / win as f64).sqrt() as f32);
        k += hop;
    }
    (e, hop as f32 / sr as f32)
}

/// Indices of the envelope's local maxima that stand 30 % of their height
/// above the lowest point since the previous such maximum, and within 45 dB
/// of the loudest. White noise has almost none of these (its envelope
/// wobbles by less: 8 per second at 48 kHz); ticks have about one each, also
/// when the next tick lands on the tail of the last, which the 6 dB-over-the-
/// recent-crest rule of [`fine_events`] misses.
fn envelope_peaks(e: &[f32]) -> Vec<usize> {
    const PROMINENCE: f32 = 0.3;
    let top = e.iter().copied().fold(0.0f32, f32::max);
    let floor = top * 0.0056;
    let mut out = Vec::new();
    if e.len() < 3 || top <= 0.0 {
        return out;
    }
    let mut trough = e[0];
    let mut rising = false;
    for i in 1..e.len() {
        trough = trough.min(e[i]);
        if e[i] > e[i - 1] {
            rising = true;
        } else if e[i] < e[i - 1] && rising {
            let pk = e[i - 1];
            if pk >= floor && pk - trough >= PROMINENCE * pk {
                out.push(i - 1);
                trough = e[i];
            }
            rising = false;
        }
    }
    out
}

/// The repetition rate of the envelope `e` (hop `hop_s`): the first local
/// maximum of its autocorrelation between 1.5 ms and 60 ms that reaches
/// 0.5 of lag 0, as 1 / lag, when the envelope swings by at least 10 % of
/// its mean (a steady tone's ripple is under 5 %).
fn envelope_period_hz(e: &[f32], hop_s: f32) -> Option<f32> {
    let n = e.len();
    let mean = e.iter().sum::<f32>() / n.max(1) as f32;
    if n < 64 || mean <= 0.0 {
        return None;
    }
    let d: Vec<f32> = e.iter().map(|v| v - mean).collect();
    let var = d.iter().map(|v| v * v).sum::<f32>() / n as f32;
    if var.sqrt() < 0.1 * mean {
        return None;
    }
    let max_lag = ((0.06 / hop_s) as usize).min(n / 4);
    let min_lag = ((0.0015 / hop_s).ceil() as usize).max(2);
    let acf: Vec<f32> = (0..=max_lag + 1)
        .map(|l| {
            let m = n - l;
            d[..m].iter().zip(&d[l..]).map(|(a, b)| a * b).sum::<f32>() / (m as f32 * var)
        })
        .collect();
    (min_lag..=max_lag).find_map(|l| {
        (acf[l] >= 0.3 && acf[l] >= acf[l - 1] && acf[l] > acf[l + 1]).then(|| {
            let (off, _) = parabola(acf[l - 1], acf[l], acf[l + 1]);
            1.0 / ((l as f32 + off) * hop_s)
        })
    })
}

/// Coefficient of variation of the gaps between `times` (seconds), mapped to
/// 0 (0.3 or less: a regular train, whose ticks do not hide each other at
/// random) .. 1 (0.7 or more: Poisson-like). 0 under 8 events.
fn poisson_weight(times: &[f32]) -> f32 {
    if times.len() < 8 {
        return 0.0;
    }
    let gaps: Vec<f32> = times.windows(2).map(|p| p[1] - p[0]).collect();
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    if mean <= 0.0 {
        return 0.0;
    }
    let var = gaps.iter().map(|g| (g - mean) * (g - mean)).sum::<f32>() / gaps.len() as f32;
    ((var.sqrt() / mean - 0.3) / 0.4).clamp(0.0, 1.0)
}

/// The rate whose paralyzable-dead-time reading `rate * exp(-wt * rate)` is
/// `observed` (rising branch); saturates at 1 / wt when `observed` is past
/// the curve's maximum, and is `observed` when `wt` is 0.
fn undo_dead_time(observed: f32, wt: f32) -> f32 {
    if wt <= 0.0 || observed <= 0.0 {
        return observed;
    }
    if observed * wt >= (-1.0f32).exp() {
        return 1.0 / wt;
    }
    let (mut lo, mut hi) = (observed, 1.0 / wt);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if mid * (-wt * mid).exp() < observed {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Events per second of `x` over `span_s` seconds, for `Measure::Density` and
/// the descriptors' `density` line. `events` are [`fine_events`]' (1 kHz,
/// 4 ms refractory) over the same signal.
///
/// Method. The counter ([`fine_events`]: 1 ms envelope over 1 kHz, a chunk
/// 6 dB over the loudest of the three before it, 4 ms apart) is exact while
/// events are about 6 ms apart; closer, an event lands on the last one's tail
/// and is missed, so on Poisson trains it reads rate * exp(-rate * 5.5 ms):
/// 0.64x at 80/s, and it falls from 160/s up. Three corrections, in order:
///
/// 1. **Counter, corrected.** The counter's rate with that dead time undone
///    (the rising branch of `r = rate * exp(-rate * T)`), weighted by how
///    Poisson-like the gaps between the counted events are (coefficient of
///    variation 0.3 or less, a regular train: no correction; 0.7 or more:
///    all of it), because evenly spaced ticks do not hide each other.
///    Valid while the counter's rate is under 1 / (e T), 67/s (about 180/s
///    of events).
/// 2. **Envelope peaks.** Past that the counter's reading falls, so it is
///    replaced by the rate of prominent peaks of a 0.5 ms hop, 1 ms RMS
///    envelope ([`envelope_peaks`], 30 % prominence), which still separate
///    ticks 2 ms apart, corrected the same way with T = 1.7 ms (peaks lost
///    to overlap, measured on Poisson trains of ticks with 3 ms decays:
///    peaks / rate = exp(-rate * 1.7 ms) from 160 to 500/s). It takes over
///    when there are more than 100 peaks per second. Peaks are not used
///    below that: a noise burst has several envelope wobbles and would be
///    over-counted.
/// 3. **Repetition rate.** A regular train too dense to separate (2 to 3 ms
///    apart) is a periodic envelope: the first autocorrelation peak of the
///    envelope gives 1 / period ([`envelope_period_hz`]). It is used when
///    it exceeds the result so far by 15 %.
///
/// Limits (measured in `tests/it/onsets.rs`): the dead times are calibrated on
/// ticks with 3 ms decays; the peaks reading differs by about +-25 % between
/// a fixed-carrier tick, a random-carrier tick and a band-passed noise burst
/// at 500/s; longer ticks hide each other more, so read low; above ~600/s
/// the reading saturates (the peaks reading cannot grow any more).
pub fn event_rate(x: &[f32], sr: u32, span_s: f32, events: &[FineEvent]) -> EventRate {
    let span = span_s.max(1e-3);
    let counted_per_s = events.len() as f32 / span;
    let times: Vec<f32> = events.iter().map(|e| e.t).collect();
    let wc = poisson_weight(&times);
    let y = highpass(x, sr, 1000.0);
    let (e, _) = rms_envelope_1ms(&y, sr, 2000);
    let pk = envelope_peaks(&e);
    let peaks_per_s = pk.len() as f32 / span;
    let dense = peaks_per_s > DENSE_PEAKS_PER_S && peaks_per_s > 1.6 * counted_per_s;
    // The autocorrelation is the costly part: only where the counter has
    // collapsed, on a 0.125 ms hop (a period of 3.125 ms is 25 hops).
    let period_hz = if dense {
        let (ef, hop_f) = rms_envelope_1ms(&y, sr, 8000);
        envelope_period_hz(&ef, hop_f)
    } else {
        None
    };
    let mut out = EventRate {
        per_s: counted_per_s,
        method: RateMethod::Count,
        counted_per_s,
        peaks_per_s,
        period_hz,
    };
    if wc > 0.0 {
        out.per_s = undo_dead_time(counted_per_s, wc * COUNT_DEAD_S);
        out.method = RateMethod::CountCorrected;
    }
    if dense {
        let p = undo_dead_time(peaks_per_s, PEAK_DEAD_S);
        if p > out.per_s {
            out.per_s = p;
            out.method = RateMethod::Peaks;
        }
    }
    // A periodic envelope whose repetition rate is no lower than the events
    // the counter saw, and not under half the peaks, is the tick rate itself
    // (a regular train).
    if let Some(p) = period_hz
        && p >= 0.9 * counted_per_s
        && p >= 0.5 * peaks_per_s
    {
        out.per_s = p;
        out.method = RateMethod::Period;
    }
    out
}

/// The span `density_per_s` and the descriptors count events over: first
/// sample over -60 dBFS to the last, at least 20 ms (so one click is not
/// 50 000 per second), in seconds. None for silence.
pub fn audible_span_s(audio: &Audio) -> Option<f32> {
    let th = sfx_graph::db_to_gain(-60.0);
    let first = audio
        .channels
        .iter()
        .filter_map(|c| c.iter().position(|v| v.abs() > th))
        .min()?;
    let last = audio
        .channels
        .iter()
        .filter_map(|c| c.iter().rposition(|v| v.abs() > th))
        .max()?;
    Some(((last + 1 - first) as f32 / audio.sample_rate as f32).max(0.02))
}

/// Events per second (`Measure::Density`): [`event_rate`] over the span from
/// the first sample over -60 dBFS to the last (at least 20 ms, so one click
/// is not 50 000 per second). Rises with the true rate of tick trains from 10
/// to 500/s; see [`event_rate`] for the method and its limits.
pub fn density_per_s(audio: &Audio) -> f32 {
    let Some(span_s) = audible_span_s(audio) else {
        return 0.0;
    };
    let x = audio.mix_mono();
    let sr = audio.sample_rate;
    let events = fine_events(&x, sr, 1000.0, 4);
    event_rate(&x, sr, span_s, &events).per_s
}

/// Milliseconds from the first sample over -60 dBFS to the first 0.25 ms
/// frame holding 90 % of the sample peak (`Measure::Attack`).
pub fn attack_ms(audio: &Audio) -> f32 {
    let sr = audio.sample_rate;
    let x = audio.mix_mono();
    let th = sfx_graph::db_to_gain(-60.0);
    let Some(first) = x.iter().position(|v| v.abs() > th) else {
        return 0.0;
    };
    let c = (sr as usize / 4000).max(1);
    let env: Vec<f32> = x
        .chunks(c)
        .map(|ch| ch.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect();
    let top = env.iter().copied().fold(0.0f32, f32::max);
    let Some(k90) = env.iter().position(|v| *v >= 0.9 * top) else {
        return 0.0;
    };
    (k90 * c).saturating_sub(first) as f32 / sr as f32 * 1000.0
}

/// Median gap, seconds, between the strongest onsets (`Measure::Spacing`):
/// [`fine_events`] above 150 Hz and 30 ms apart, keeping those whose level is
/// within 20 dB of the loudest. 0 under two onsets.
pub fn spacing_s(audio: &Audio) -> f32 {
    let ev = fine_events(&audio.mix_mono(), audio.sample_rate, 150.0, 30);
    let top = ev.iter().map(|e| e.level).fold(0.0f32, f32::max);
    let t: Vec<f32> = ev
        .iter()
        .filter(|e| e.level >= 0.1 * top)
        .map(|e| e.t)
        .collect();
    let gaps: Vec<f32> = t.windows(2).map(|w| w[1] - w[0]).collect();
    median_of(gaps).unwrap_or(0.0)
}

/// Per-hit ring-out in milliseconds (`Measure::Ring`): the median over the
/// strong onsets of each one's own -20 dB fall time.
///
/// Onsets are [`fine_events`] above 100 Hz (10 ms refractory) whose level is
/// at least 0.25 of the loudest. For each, the peak is the largest 2.5 ms
/// peak-hold envelope value between the onset and 30 ms later (or the next
/// onset), and the fall is the time from that peak to the first envelope
/// frame under peak - 20 dB. The hit's window ends at the next onset: a hit
/// the next one arrives over before it fell is skipped. A last hit that
/// reaches the end of the sound counts up to the end. No usable hit: the
/// whole-sound `decay`, in ms.
pub fn ring_ms(audio: &Audio) -> f32 {
    let sr = audio.sample_rate;
    let x = audio.mix_mono();
    let fallback = || measure(audio, Measure::Decay) * 1000.0;
    let ev = fine_events(&x, sr, 100.0, 10);
    let top = ev.iter().map(|e| e.level).fold(0.0f32, f32::max);
    if ev.is_empty() || top <= 0.0 {
        return fallback();
    }
    // Peak-hold envelope: max |x| over 2.5 ms, every 0.25 ms.
    let hop = (sr as usize / 4000).max(1);
    let hold = hop * 10;
    let abs: Vec<f32> = x.iter().map(|v| v.abs()).collect();
    let env: Vec<f32> = (0..abs.len().div_ceil(hop))
        .map(|k| {
            let a = k * hop;
            abs[a..(a + hold).min(abs.len())]
                .iter()
                .copied()
                .fold(0.0f32, f32::max)
        })
        .collect();
    let to_frame = |t: f32| ((t * sr as f32) as usize) / hop;
    let mut falls = Vec::new();
    for (i, e) in ev.iter().enumerate() {
        if e.level < 0.25 * top {
            continue;
        }
        let a = to_frame(e.t).min(env.len().saturating_sub(1));
        let next = ev
            .get(i + 1)
            .map_or(env.len(), |n| to_frame(n.t).min(env.len()));
        let search_end = (a + 120).min(next).max(a + 1);
        let Some((pk, pv)) = (a..search_end)
            .map(|k| (k, env[k]))
            .max_by(|p, q| p.1.total_cmp(&q.1))
        else {
            continue;
        };
        if pv <= 0.0 {
            continue;
        }
        let th = pv * 0.1;
        match (pk + 1..next).find(|k| env[*k] < th) {
            Some(k) => falls.push((k - pk) as f32),
            // Reached the end of the sound before another onset or the fall.
            None if next >= env.len() && i + 1 >= ev.len() => falls.push((next - pk) as f32),
            None => {}
        }
    }
    match median_of(falls) {
        Some(f) => f * hop as f32 * 1000.0 / sr as f32,
        None => fallback(),
    }
}
