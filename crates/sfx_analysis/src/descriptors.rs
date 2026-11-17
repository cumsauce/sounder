//! What a sound measures as: the agent's main readout.

use serde::Serialize;
use sfx_graph::{Audio, gain_to_db, loudness};

use crate::measure::{FineEvent, RateMethod, event_rate, fine_events};
use crate::pitch;
use crate::stft::{self, Spectra, db};

/// Octave band centres of the band table, Hz (the last runs to Nyquist).
pub const BANDS: [f32; 8] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
/// Labels of [`BANDS`].
pub const BAND_LABELS: [&str; 8] = ["63", "125", "250", "500", "1k", "2k", "4k", "8k+"];
/// Equal time segments of the band table.
pub const SEGMENTS: usize = 5;
/// Rows of the per-onset table.
pub const MAX_ONSET_ROWS: usize = 24;

/// A spectral measure at the start, middle and end thirds of the sound.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Sme {
    pub start: f32,
    pub middle: f32,
    pub end: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Onset {
    /// Seconds.
    pub t: f32,
    /// Spectral flux at the onset, relative to the strongest (0..1).
    pub strength: f32,
}

/// One row of the per-onset table: what the sound does from one onset until
/// the next (or the end).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct OnsetRow {
    /// Seconds.
    pub t: f32,
    /// Spectral flux relative to the strongest onset (0..1).
    pub strength: f32,
    /// Milliseconds from the last point under 10% of the hit's peak up to
    /// the peak (0.25 ms resolution).
    pub attack_ms: f32,
    /// Milliseconds after the hit's peak until the level (smoothed over
    /// 6 ms) stays 20 dB under it; None: it does not fall that far before the
    /// next onset (or the end).
    pub decay20_ms: Option<f32>,
    /// Spectral centroid of the first 100 ms of the hit (or until the next
    /// onset), Hz.
    pub centroid_hz: f32,
    /// Median f0 of the voiced frames of the hit, Hz.
    pub pitch_hz: Option<f32>,
    /// Median f0 of the first / last two voiced frames, when the hit has at
    /// least three voiced frames (a chirp or a drip).
    pub pitch_start_hz: Option<f32>,
    pub pitch_end_hz: Option<f32>,
    /// Milliseconds after the hit's peak until the level (smoothed over 6 ms)
    /// stays 60 dB under it. When `decay60_open` it did not fall that far
    /// before the next onset (or the end): the value is the time available,
    /// printed as `>N`.
    pub decay60_ms: f32,
    pub decay60_open: bool,
    /// The table prints `n/a` in the pitch column: the hit's strongest
    /// partials are inharmonic (over 100 cents), or under 30% of its audible
    /// pitch frames are voiced, so any pitch would be a guess.
    pub pitch_na: bool,
    /// Inharmonicity of the hit's strongest partials, cents (None: under 3
    /// clear partials, or a noisy spectrum).
    pub inharmonicity_cents: Option<f32>,
    /// A burst under 60 ms whose spectral peak clearly rises or falls: the
    /// peak's start and end frequency, Hz (a short-time spectrum every 2 ms).
    pub glide_hz: Option<(f32, f32)>,
}

/// Events per second at a fine time resolution, apart from the merged
/// onset list.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Density {
    /// Events over the audible span, per second.
    pub per_s: f32,
    pub count: usize,
    /// Seconds from the first to the last sample above -60 dBFS.
    pub span_s: f32,
    /// Events closer than this count as one, milliseconds.
    pub min_spacing_ms: f32,
    /// How `per_s` was reached (see [`crate::measure::event_rate`]): the
    /// counter's rate when events are far apart, corrected when they overlap.
    pub method: RateMethod,
    /// The counter's rate, as is (`count` / `span_s`).
    pub counted_per_s: f32,
}

/// The fine events by strength, and where the last one is.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventSummary {
    /// Events whose level is at least half (-6 dB) of the loudest event's.
    pub strong: usize,
    pub weak: usize,
    /// Seconds from the start to the last fine event (None: no events).
    pub last_s: Option<f32>,
    /// Seconds from the last event to the last sample above -60 dBFS.
    pub tail_s: f32,
}

/// Loudness spread over 400 ms windows (what a loop should keep small).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Steadiness {
    pub min_lufs: f32,
    pub max_lufs: f32,
    pub spread_lu: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Mode {
    pub hz: f32,
    /// Seconds to fall 60 dB (extrapolated from the measured slope; 600 =
    /// sustained).
    pub decay_s: f32,
    /// Peak level of the partial, dB.
    pub level_db: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Stereo {
    /// -1 (opposite) .. 1 (identical channels).
    pub correlation: f32,
    /// Side RMS over mid RMS (0 = mono).
    pub width: f32,
}

/// Every descriptor of one sound.
#[derive(Debug, Clone, Serialize)]
pub struct Descriptors {
    pub seconds: f32,
    pub sample_rate: u32,
    pub channels: usize,
    pub peak_db: f32,
    pub true_peak_db: f32,
    pub lufs: f32,
    pub lufs_momentary_max: f32,
    pub rms_db: f32,
    /// Peak over RMS, dB.
    pub crest_db: f32,
    /// Mean sample value, dB (-200 = none).
    pub dc_db: f32,
    /// Seconds from 10% to 90% of the peak envelope.
    pub attack_s: f32,
    /// Seconds of the peak.
    pub peak_t: f32,
    /// Seconds after the peak until the level stays 20 / 60 dB under it
    /// (None: it never falls that far).
    pub decay20_s: Option<f32>,
    pub decay60_s: Option<f32>,
    /// Seconds before the first / after the last sample above -60 dBFS.
    pub silence_start_s: f32,
    pub silence_end_s: f32,
    pub onsets: Vec<Onset>,
    /// Median f0 of voiced frames, Hz.
    pub pitch_hz: Option<f32>,
    /// Share of audible frames that are voiced (pitched), 0..1.
    pub voiced: f32,
    /// 1 = perfectly periodic, 0 = noise (audible frames).
    pub harmonicity: f32,
    pub centroid_hz: Sme,
    /// Frequency under which 85% of the energy lies.
    pub rolloff_hz: Sme,
    /// Spectral flatness of the averaged spectrum (1 = white noise, 0 = a
    /// pure tone).
    pub flatness: Sme,
    /// bands[band][segment], dB (mean power; a full-scale sine reads -3).
    pub bands: Vec<[f32; SEGMENTS]>,
    /// bands over the whole sound, dB.
    pub bands_total: Vec<f32>,
    pub modes: Vec<Mode>,
    pub stereo: Option<Stereo>,
    /// Centroid / 85% rolloff of the whole sound's spectrum (frames weighted
    /// by their energy), Hz.
    pub centroid_all_hz: f32,
    pub rolloff_all_hz: f32,
    /// Centroid in each tenth of the sound, Hz (None: nothing audible).
    pub centroid_tenths: Vec<Option<f32>>,
    /// Up to 24 rows, one per onset of `onsets`.
    pub onset_rows: Vec<OnsetRow>,
    pub density: Density,
    /// Strong / weak fine events and the last one's time (the `events:` line).
    pub events: EventSummary,
    /// Percent of the energy under 200 Hz, 200-2k, 2k-8k, over 8k.
    pub band_shares: [f32; 4],
    /// Median YIN f0 in each tenth of the sound (None: unvoiced / quiet).
    pub pitch_tenths: Vec<Option<f32>>,
    /// Strongest partials (Hz, ascending) when the sound is not harmonic.
    pub inharmonic_partials: Vec<f32>,
    /// Seconds from the first sample above -60 dBFS to the first 0.25 ms
    /// frame at 90% of the sample peak.
    pub attack_to90_s: f32,
    /// Seconds from the first sample above -60 dBFS to the sample peak.
    pub attack_to_peak_s: f32,
    /// Flatness of the whole sound's spectrum, as `flatness` but over all.
    pub flatness_all: f32,
    /// The band holding the middle 80% of the energy (10th to 90th
    /// percentile), Hz.
    pub occupied_hz: (f32, f32),
    /// Flatness inside `occupied_hz` (None when under 5 bins wide: tonal).
    pub flatness_occupied: Option<f32>,
    /// Loudness spread over 400 ms windows (None: under two windows).
    pub steadiness: Option<Steadiness>,
    /// Set with [`Descriptors::with_loop`]: the table drops attack, peak-at
    /// and decay.
    pub is_loop: bool,
    /// The headline attack: seconds from the first sample above -60 dBFS to
    /// 90% of the peak of the smoothed envelope (a 10 ms trailing RMS). A
    /// swell reads long, a click near zero; `attack_s`, `attack_to90_s` and
    /// `attack_to_peak_s` measure other things (see their docs).
    pub swell_attack_s: f32,
    /// Times (seconds) of the fine events behind `density` (1 ms envelope
    /// over 1 kHz, 6 dB rise): resolves hits 5 ms apart, which `onsets`
    /// (40 ms merge) cannot.
    pub fine_onsets: Vec<f32>,
    /// Fine events per second in each tenth of the sound.
    pub density_tenths: Vec<f32>,
    /// Mean deviation, cents, of the strongest partials' ratios to the
    /// lowest of them from whole numbers (None: under 3 partials, or noise).
    /// Not a pitch guess: see [`partial_inharmonicity_cents`].
    pub inharmonicity_cents: Option<f32>,
    /// Pairs of consecutive fine events under 20 ms apart: (t1, t2) seconds.
    pub close_pairs: Vec<(f32, f32)>,
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// The analysis window for a sound of `n` samples at `sr`.
pub fn window_for(n: usize, sr: u32) -> usize {
    let w = if n < sr as usize / 4 { 1024 } else { 2048 };
    (w as f32 * sr as f32 / 48_000.0).round().max(256.0) as usize
}

/// Band power (dB) of a mean power spectrum, per octave band.
pub fn band_db(s: &Spectra, power: &[f32]) -> Vec<f32> {
    let nyq = s.sr as f32 / 2.0;
    BANDS
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let lo = c / std::f32::consts::SQRT_2;
            let hi = if k == BANDS.len() - 1 {
                nyq
            } else {
                c * std::f32::consts::SQRT_2
            };
            let p: f32 = (0..power.len())
                .filter(|&b| {
                    let f = s.hz(b);
                    f >= lo && f < hi
                })
                .map(|b| power[b])
                .sum();
            db(p).max(-120.0)
        })
        .collect()
}

/// Spectral flux onsets: half-wave rectified rise of the log spectrum,
/// peaks above an adaptive (median + margin) threshold, at least 30 ms
/// apart.
fn onsets(s: &Spectra) -> Vec<Onset> {
    let n = s.frames.len();
    let mut flux = vec![0.0f32; n];
    let floor = -80.0;
    for k in 0..n {
        let cur = &s.frames[k];
        let mut sum = 0.0;
        for (b, p) in cur.iter().enumerate() {
            let now = db(*p).max(floor);
            let before = if k == 0 {
                floor
            } else {
                db(s.frames[k - 1][b]).max(floor)
            };
            sum += (now - before).max(0.0);
        }
        flux[k] = sum / cur.len() as f32;
    }
    let peak = flux.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return Vec::new();
    }
    let gap = ((0.03 * s.sr as f32) / s.hop as f32).ceil() as usize;
    let mut out: Vec<Onset> = Vec::new();
    let mut last: Option<usize> = None;
    for k in 0..n {
        let lo = k.saturating_sub(8);
        let hi = (k + 8).min(n - 1);
        let mut win: Vec<f32> = flux[lo..=hi].to_vec();
        win.sort_by(f32::total_cmp);
        let med = win[win.len() / 2];
        let is_peak = (k == 0 || flux[k] >= flux[k - 1]) && (k + 1 >= n || flux[k] > flux[k + 1]);
        if is_peak && flux[k] > med + 0.1 * peak && flux[k] > 0.5 {
            if let Some(l) = last
                && k - l < gap
            {
                if flux[k] > flux[l]
                    && let Some(o) = out.last_mut()
                {
                    *o = Onset {
                        t: s.time(k),
                        strength: flux[k] / peak,
                    };
                    last = Some(k);
                }
                continue;
            }
            out.push(Onset {
                t: s.time(k),
                strength: flux[k] / peak,
            });
            last = Some(k);
        }
    }
    out
}

/// The strongest spectral peaks after the loudest moment, with per-peak
/// decay (a line fitted to the bin's dB track).
fn modes(s: &Spectra, from_frame: usize) -> Vec<Mode> {
    if s.frames.len() <= from_frame {
        return Vec::new();
    }
    let tail = from_frame..s.frames.len();
    let avg = s.mean(tail.clone());
    let maxp: Vec<f32> = (0..s.bins())
        .map(|b| {
            s.frames[tail.clone()]
                .iter()
                .map(|f| f[b])
                .fold(0.0, f32::max)
        })
        .collect();
    let top = maxp.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return Vec::new();
    }
    let lo = s.bin(30.0).max(2);
    let mut peaks: Vec<usize> = (lo..s.bins() - 2)
        .filter(|&b| {
            avg[b] > avg[b - 1]
                && avg[b] >= avg[b + 1]
                && avg[b] > avg[b - 2]
                && avg[b] >= avg[b + 2]
        })
        .filter(|&b| db(maxp[b]) > db(top) - 50.0)
        .collect();
    peaks.sort_by(|a, b| avg[*b].total_cmp(&avg[*a]));
    let mut chosen: Vec<usize> = Vec::new();
    for b in peaks {
        if chosen.iter().all(|c| c.abs_diff(b) > 3) {
            chosen.push(b);
        }
        if chosen.len() == 12 {
            break;
        }
    }
    let dt = s.hop as f32 / s.sr as f32;
    let mut out: Vec<Mode> =
        chosen
            .into_iter()
            .map(|b| {
                // Sum the peak's three bins (the Hann main lobe).
                let track: Vec<f32> = s.frames[tail.clone()]
                    .iter()
                    .map(|f| db(f[b - 1] + f[b] + f[b + 1]))
                    .collect();
                let (kmax, vmax) = track.iter().enumerate().fold((0, f32::MIN), |m, (k, v)| {
                    if *v > m.1 { (k, *v) } else { m }
                });
                // Fit from the peak down to 40 dB under it.
                let pts: Vec<(f32, f32)> = track[kmax..]
                    .iter()
                    .enumerate()
                    .take_while(|(_, v)| **v > vmax - 40.0)
                    .map(|(k, v)| (k as f32 * dt, *v))
                    .collect();
                let slope = if pts.len() >= 3 {
                    let n = pts.len() as f32;
                    let mx = pts.iter().map(|p| p.0).sum::<f32>() / n;
                    let my = pts.iter().map(|p| p.1).sum::<f32>() / n;
                    let sxx: f32 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
                    let sxy: f32 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
                    if sxx > 0.0 { sxy / sxx } else { 0.0 }
                } else {
                    0.0
                };
                // Parabolic peak frequency on log power.
                let (a, c0, c) = (db(avg[b - 1]), db(avg[b]), db(avg[b + 1]));
                let den = a - 2.0 * c0 + c;
                let off = if den.abs() > 1e-9 {
                    0.5 * (a - c) / den
                } else {
                    0.0
                };
                Mode {
                    hz: s.hz(b) + off.clamp(-0.5, 0.5) * s.sr as f32 / s.win as f32,
                    decay_s: if slope < -1e-3 {
                        (60.0 / -slope).min(600.0)
                    } else {
                        600.0
                    },
                    level_db: vmax,
                }
            })
            .collect();
    out.sort_by(|a, b| a.hz.total_cmp(&b.hz));
    out
}

/// Sliding maximum of `v` over `radius` samples each side.
fn sliding_max(v: &[f32], radius: usize) -> Vec<f32> {
    use std::collections::VecDeque;
    let n = v.len();
    let mut out = vec![0.0f32; n];
    let mut dq: VecDeque<usize> = VecDeque::new();
    let mut next = 0;
    for k in 0..n {
        let hi = (k + radius).min(n - 1);
        while next <= hi {
            while dq.back().is_some_and(|b| v[*b] <= v[next]) {
                dq.pop_back();
            }
            dq.push_back(next);
            next += 1;
        }
        while dq.front().is_some_and(|f| f + radius < k) {
            dq.pop_front();
        }
        out[k] = dq.front().map_or(0.0, |f| v[*f]);
    }
    out
}

/// Peak of each `c`-sample chunk.
fn chunk_peaks(x: &[f32], c: usize) -> Vec<f32> {
    x.chunks(c)
        .map(|k| k.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect()
}

/// The per-onset table, one row per onset (at most [`MAX_ONSET_ROWS`]).
fn onset_rows(
    x: &[f32],
    spec: &Spectra,
    ons: &[Onset],
    pitch_frames: &[pitch::PitchFrame],
    centroid: &dyn Fn(&Vec<f32>) -> f32,
) -> Vec<OnsetRow> {
    let sr = spec.sr as f32;
    let c = (spec.sr as usize / 4000).max(1);
    let env = chunk_peaks(x, c);
    if env.is_empty() {
        return Vec::new();
    }
    let sm = sliding_max(&env, 12);
    let half = spec.win as f32 / 2.0 / sr;
    let total = x.len() as f32 / sr;
    let ms_per_chunk = c as f32 / sr * 1000.0;
    ons.iter()
        .take(MAX_ONSET_ROWS)
        .enumerate()
        .map(|(i, o)| {
            let t = o.t;
            let next = ons.get(i + 1).map(|n| n.t);
            let end_t = next.map_or(total, |n| (n - half).max(t));
            let a = (((t - half).max(0.0) * sr) as usize / c).min(env.len() - 1);
            let b = ((end_t * sr).ceil() as usize)
                .div_ceil(c)
                .clamp(a + 1, env.len());
            let p = (a..b).fold(a, |m, k| if env[k] > env[m] { k } else { m });
            let pv = env[p];
            let start = (a..p)
                .rev()
                .find(|k| env[*k] < 0.1 * pv)
                .map_or(a, |k| k + 1);
            let attack_ms = (p - start) as f32 * ms_per_chunk;
            let top = sm[p..b].iter().copied().fold(0.0f32, f32::max);
            let decay20_ms = (p..b)
                .rev()
                .find(|k| sm[*k] >= 0.1 * top)
                .filter(|last| last + 1 < b && top > 0.0)
                .map(|last| (last + 1 - p) as f32 * ms_per_chunk);
            // -60 dB: the same, 60 dB under the hit's loudest smoothed level;
            // open ("at least") when it is still above that at the next
            // onset or the end.
            let last60 = (p..b).rev().find(|k| sm[*k] >= 0.001 * top);
            let (decay60_ms, decay60_open) = match last60 {
                Some(last) if last + 1 < b && top > 0.0 => {
                    ((last + 1 - p) as f32 * ms_per_chunk, false)
                }
                _ => ((b - p) as f32 * ms_per_chunk, true),
            };
            // Spectrum of the first 100 ms of the hit.
            let f0 = ((t * sr / spec.hop as f32).round() as usize).min(spec.frames.len() - 1);
            let f1 = (((t + 0.1).min(next.unwrap_or(f32::MAX)) * sr / spec.hop as f32).ceil()
                as usize)
                .clamp(f0 + 1, spec.frames.len());
            let onset_spec = spec.mean(f0..f1);
            let centroid_hz = centroid(&onset_spec);
            let inharmonicity_cents = {
                let lo = spec.bin(20.0).max(1);
                let flat = flatness_of(&onset_spec[lo..onset_spec.len() - 1]);
                let hz = spectrum_partials(spec, &onset_spec);
                if flat > 0.3 {
                    None
                } else {
                    partial_inharmonicity_cents(&hz)
                }
            };
            // f0 of the voiced frames of the hit.
            let hi = next.unwrap_or(f32::MAX);
            let in_hit = |f: &&pitch::PitchFrame| f.t >= t - 0.01 && f.t < hi;
            let audible: Vec<&pitch::PitchFrame> = pitch_frames
                .iter()
                .filter(in_hit)
                .filter(|f| f.level_db > -50.0)
                .collect();
            let audible_frames = audible.len();
            let voiced_audible = audible.iter().filter(|f| f.hz.is_some()).count();
            let voiced: Vec<f32> = pitch_frames
                .iter()
                .filter(|f| f.t >= t - 0.01 && f.t < hi)
                .filter_map(|f| f.hz)
                .collect();
            let (mut ps, mut pe) = if voiced.len() >= 3 {
                (
                    median(voiced[..2].to_vec()),
                    median(voiced[voiced.len() - 2..].to_vec()),
                )
            } else {
                (None, None)
            };
            // A blip of 15..60 ms (a drip) is too short for the frames
            // above (two or three frames): read the two halves of its first
            // 40 ms directly, in place of them. Its end is where the level
            // first falls 30 dB under the peak (looking past the next onset:
            // a blip's own tail can raise one).
            let mut blip = false;
            let mut glide_hz = None;
            {
                let thr = 0.0316 * env[p];
                let end30 = (p..env.len())
                    .find(|k| env[*k..(*k + 4).min(env.len())].iter().all(|v| *v < thr))
                    .unwrap_or(env.len());
                let (s0, s1) = (start * c, end30 * c);
                let len_ms = (s1.saturating_sub(s0)) as f32 / sr * 1000.0;
                if env[p] > 0.0
                    && (15.0..=60.0).contains(&len_ms)
                    && let Some((a, z)) = blip_glide(x, spec.sr, s0, s1)
                {
                    (ps, pe) = (Some(a), Some(z));
                    blip = true;
                }
                if env[p] > 0.0 && (8.0..=60.0).contains(&len_ms) {
                    glide_hz = spectral_glide(x, spec.sr, s0, s1);
                }
            }
            let pitch_na = !blip
                && glide_hz.is_none()
                && (inharmonicity_cents.is_some_and(|c| c > INHARMONIC_CENTS)
                    || (!voiced.is_empty()
                        && (voiced_audible as f32) < 0.3 * audible_frames as f32));
            OnsetRow {
                t,
                strength: o.strength,
                attack_ms,
                decay20_ms,
                centroid_hz,
                pitch_hz: median(voiced),
                pitch_start_hz: ps,
                pitch_end_hz: pe,
                decay60_ms,
                decay60_open,
                pitch_na,
                inharmonicity_cents,
                glide_hz,
            }
        })
        .collect()
}

/// Inharmonicity (cents) over which the table calls partials inharmonic: the
/// `inharmonic: partials at` line is printed and a hit's pitch reads `n/a`.
pub const INHARMONIC_CENTS: f32 = 100.0;

/// Mean distance, in cents, of the partials' frequency ratios to the lowest
/// partial from the nearest whole number (the lowest itself is the reference
/// and is not counted). Harmonic partials of a series that starts at its
/// lowest member read near 0; 1 : 1.29 : 2.6 : 3.5 reads over 200. None:
/// under 3 partials. It uses no pitch guess.
pub fn partial_inharmonicity_cents(hz: &[f32]) -> Option<f32> {
    if hz.len() < 3 {
        return None;
    }
    let lo = hz.iter().copied().fold(f32::MAX, f32::min);
    if lo <= 0.0 {
        return None;
    }
    let dev: Vec<f32> = hz
        .iter()
        .filter(|h| **h > lo)
        .map(|h| {
            let r = h / lo;
            (1200.0 * (r / r.round().max(1.0)).log2()).abs()
        })
        .collect();
    (!dev.is_empty()).then(|| dev.iter().sum::<f32>() / dev.len() as f32)
}

/// The strongest (up to 6) peaks of a mean power spectrum within 30 dB of its
/// loudest, at least 4 bins apart, refined on log power (Hz, ascending).
fn spectrum_partials(s: &Spectra, avg: &[f32]) -> Vec<f32> {
    let lo = s.bin(30.0).max(2);
    let hi = s.bins().saturating_sub(2);
    if hi <= lo {
        return Vec::new();
    }
    let top = avg[lo..hi].iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return Vec::new();
    }
    let mut peaks: Vec<usize> = (lo..hi)
        .filter(|&b| {
            avg[b] > avg[b - 1]
                && avg[b] >= avg[b + 1]
                && avg[b] > avg[b - 2]
                && avg[b] >= avg[b + 2]
                && db(avg[b]) > db(top) - 30.0
        })
        .collect();
    peaks.sort_by(|a, b| avg[*b].total_cmp(&avg[*a]));
    let mut chosen: Vec<usize> = Vec::new();
    for b in peaks {
        if chosen.iter().all(|c| c.abs_diff(b) > 3) {
            chosen.push(b);
        }
        if chosen.len() == 6 {
            break;
        }
    }
    let mut out: Vec<f32> = chosen
        .into_iter()
        .map(|b| {
            let (a, c0, c) = (db(avg[b - 1]), db(avg[b]), db(avg[b + 1]));
            let den = a - 2.0 * c0 + c;
            let off = if den.abs() > 1e-9 {
                0.5 * (a - c) / den
            } else {
                0.0
            };
            s.hz(b) + off.clamp(-0.5, 0.5) * s.sr as f32 / s.win as f32
        })
        .collect();
    out.sort_by(f32::total_cmp);
    out
}

/// A short burst `x[s0..s1]` (at most its first 60 ms) whose spectral peak
/// clearly glides: the peak of a Hann spectrum of about 5.5 ms every 2 ms
/// (frames within 25 dB of the loudest), a line fitted to its log frequency;
/// returns the fitted start and end Hz when at least 5 frames fit with
/// R^2 over 0.75 and the change is over 250 cents. None for a steady tone,
/// a click or noise.
fn spectral_glide(x: &[f32], sr: u32, s0: usize, s1: usize) -> Option<(f32, f32)> {
    let s1 = s1.min(s0 + sr as usize * 60 / 1000).min(x.len());
    if s1 <= s0 + sr as usize / 125 {
        return None;
    }
    let win = 1usize << ((sr as f32 * 0.0055).log2().round().max(6.0) as u32);
    let hop = (sr as usize * 2 / 1000).max(1);
    let sp = stft::stft(&x[s0..s1], sr, win, hop);
    let energy: Vec<f32> = sp.frames.iter().map(|f| f.iter().sum()).collect();
    let top = energy.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return None;
    }
    let (lo, hi) = (
        sp.bin(300.0).max(2),
        (sp.bin(0.45 * sr as f32)).min(sp.bins() - 3),
    );
    if hi <= lo {
        return None;
    }
    let mut pts: Vec<(f32, f32)> = Vec::new();
    for (k, f) in sp.frames.iter().enumerate() {
        if energy[k] < top * 0.003 {
            continue;
        }
        // Only frames whose window lies inside the burst.
        let c = k * hop;
        if c < win / 2 || c + win / 2 > s1 - s0 {
            continue;
        }
        let b = (lo..=hi).fold(lo, |m, i| if f[i] > f[m] { i } else { m });
        let (a, c0, cc) = (db(f[b - 1]), db(f[b]), db(f[b + 1]));
        let den = a - 2.0 * c0 + cc;
        let off = if den.abs() > 1e-9 {
            0.5 * (a - cc) / den
        } else {
            0.0
        };
        let hz = sp.hz(b) + off.clamp(-0.5, 0.5) * sr as f32 / win as f32;
        if hz > 0.0 {
            pts.push((sp.time(k), hz.ln()));
        }
    }
    if pts.len() < 5 {
        return None;
    }
    let n = pts.len() as f32;
    let mx = pts.iter().map(|p| p.0).sum::<f32>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f32>() / n;
    let sxx: f32 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let syy: f32 = pts.iter().map(|p| (p.1 - my).powi(2)).sum();
    let sxy: f32 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    if sxx <= 0.0 || syy <= 0.0 {
        return None;
    }
    let r2 = sxy * sxy / (sxx * syy);
    let slope = sxy / sxx;
    let (t_a, t_b) = (pts[0].0, pts[pts.len() - 1].0);
    let (ln_a, ln_b) = (my + slope * (t_a - mx), my + slope * (t_b - mx));
    let cents = 1200.0 * (ln_b - ln_a) / std::f32::consts::LN_2;
    (r2 > 0.75 && cents.abs() > 250.0).then(|| (ln_a.exp(), ln_b.exp()))
}

/// Events per second from a 1 ms envelope of the sound above 1 kHz: an event
/// is a 1 ms chunk at least 6 dB over the loudest of the 3 chunks before it
/// (and within 45 dB of the loudest chunk), then 4 ms of refractory time
/// ([`fine_events`]). Unlike the merged onset list this resolves
/// clicks 5 ms apart. A tone's ripple does not count (nothing rises over a
/// recent crest), nor does a bass thump (high-passed away). `per_s` is
/// [`event_rate`] over the events: the count over the span while
/// events are far apart, corrected when they overlap (the counter misses an
/// event that lands on the last one's tail), so it keeps rising with the true
/// rate up to about 500 events/s where the plain count falls from 100.
fn density(x: &[f32], sr: u32, span: (usize, usize)) -> (Density, Vec<FineEvent>) {
    const SPACING_CHUNKS: usize = 4;
    let events = fine_events(x, sr, 1000.0, SPACING_CHUNKS);
    let span_s = (span.1.saturating_sub(span.0)) as f32 / sr as f32;
    let c = (sr as usize / 1000).max(1);
    let rate = event_rate(x, sr, span_s, &events);
    let d = Density {
        per_s: if span_s > 0.0 { rate.per_s } else { 0.0 },
        count: events.len(),
        span_s,
        min_spacing_ms: SPACING_CHUNKS as f32 * c as f32 / sr as f32 * 1000.0,
        method: rate.method,
        counted_per_s: if span_s > 0.0 {
            rate.counted_per_s
        } else {
            0.0
        },
    };
    (d, events)
}

/// Strong (level at least half of the loudest fine event's) and weak events,
/// the last event's time and the audible tail after it.
fn event_summary(events: &[FineEvent], audible_end_s: f32) -> EventSummary {
    let top = events.iter().map(|e| e.level).fold(0.0f32, f32::max);
    let strong = events.iter().filter(|e| e.level >= 0.5 * top).count();
    let last_s = events.last().map(|e| e.t);
    EventSummary {
        strong,
        weak: events.len() - strong,
        last_s,
        tail_s: last_s.map_or(0.0, |t| (audible_end_s - t).max(0.0)),
    }
}

/// The headline attack, seconds: from `first_loud` (the first sample above
/// -60 dBFS) to the first 0.25 ms step at which the trailing 10 ms RMS (only
/// samples from `first_loud` on count, so a step reads about zero) reaches
/// 90% of its own maximum. 0 for silence.
fn swell_attack(x: &[f32], sr: u32, first_loud: usize) -> f32 {
    let n = x.len();
    if first_loud >= n {
        return 0.0;
    }
    let c = (sr as usize / 4000).max(1);
    let win = (sr as usize / 100).max(c);
    let mut cs = vec![0.0f64; n + 1];
    for (i, v) in x.iter().enumerate() {
        cs[i + 1] = cs[i] + f64::from(*v) * f64::from(*v);
    }
    let mut env: Vec<(usize, f64)> = Vec::new();
    let mut e = first_loud + c;
    loop {
        let end = e.min(n);
        let from = end.saturating_sub(win).max(first_loud);
        let len = (end - from).max(1);
        env.push((end, ((cs[end] - cs[from]) / len as f64).sqrt()));
        if end >= n {
            break;
        }
        e += c;
    }
    let top = env.iter().map(|p| p.1).fold(0.0f64, f64::max);
    if top <= 0.0 {
        return 0.0;
    }
    let (end, _) = env
        .iter()
        .copied()
        .find(|p| p.1 >= 0.9 * top)
        .unwrap_or(env[0]);
    // The step's own width is not part of the attack.
    (end.saturating_sub(first_loud + c)) as f32 / sr as f32
}

/// The pitch (Hz) of `seg` by a YIN-style difference function over lags that
/// fit inside it (100 Hz .. 8 kHz, at most half the segment): the first dip
/// of the cumulative-mean-normalised difference under 0.25. None when the
/// segment is too short (under 6 ms), silent or not periodic.
fn segment_pitch(seg: &[f32], sr: u32) -> Option<f32> {
    let w = seg.len();
    if w < sr as usize * 6 / 1000 {
        return None;
    }
    let mean = seg.iter().map(|v| f64::from(*v)).sum::<f64>() / w as f64;
    let y: Vec<f32> = seg.iter().map(|v| (f64::from(*v) - mean) as f32).collect();
    if rms(&y) < 1e-5 {
        return None;
    }
    let min_lag = (sr as usize / 8000).max(2);
    let max_lag = (sr as usize / 100).min(w / 2);
    if max_lag <= min_lag + 2 {
        return None;
    }
    let mut d = vec![0.0f32; max_lag + 2];
    for (tau, dv) in d.iter_mut().enumerate().skip(1) {
        let m = w - tau;
        let s: f32 = (0..m)
            .map(|i| {
                let e = y[i] - y[i + tau];
                e * e
            })
            .sum();
        *dv = s / m as f32;
    }
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
    // The first local minimum within 0.1 of the deepest one (and under 0.25),
    // so a faint early dip does not beat the true period.
    let deepest = (min_lag..=max_lag).map(|t| cm[t]).fold(f32::MAX, f32::min);
    if deepest >= 0.25 {
        return None;
    }
    let tau = (min_lag..max_lag)
        .find(|&t| cm[t] <= deepest + 0.1 && cm[t] <= cm[t - 1] && cm[t] <= cm[t + 1])?;
    let (a, b, cc) = (d[tau - 1], d[tau], d[tau + 1]);
    let den = a - 2.0 * b + cc;
    let off = if den.abs() > 1e-9 {
        (0.5 * (a - cc) / den).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    Some(sr as f32 / (tau as f32 + off))
}

/// Start and end pitch of a blip `x[s0..s1]` (the first 40 ms at most): the
/// pitch of each half, both required voiced and more than 60 cents apart,
/// then extended along the same slope in log frequency to the blip's two
/// ends (a half's pitch belongs to its centre; for a linear chirp that puts
/// the start and end back on the ends). None when not a clear glide.
fn blip_glide(x: &[f32], sr: u32, s0: usize, s1: usize) -> Option<(f32, f32)> {
    let s1 = s1.min(s0 + sr as usize * 40 / 1000).min(x.len());
    if s1 <= s0 {
        return None;
    }
    let half = (s1 - s0) / 2;
    let f1 = segment_pitch(&x[s0..s0 + half], sr)?;
    let f2 = segment_pitch(&x[s0 + half..s0 + 2 * half], sr)?;
    if pitch::cents(f1, f2).abs() <= 60.0 {
        return None;
    }
    // Linear in Hz (exact for a linear chirp); a glide too steep for that
    // to stay positive is extended in log frequency instead.
    let top = sr as f32 * 0.45;
    let extend = |from: f32, to: f32| {
        let lin = from - 0.5 * (to - from);
        let log = (from.ln() - 0.5 * (to.ln() - from.ln())).exp();
        (if lin > 0.3 * from.min(to) { lin } else { log }).clamp(20.0, top)
    };
    let (start, end) = (extend(f1, f2), extend(f2, f1));
    Some((start, end))
}

/// Descriptors of the part of `audio` between `t0` and `t1` seconds (clamped
/// to the sound; an empty window gives the descriptors of nothing). Every
/// time in the result (onsets, fine events, peak) counts from `t0`. This is
/// what a zoomed `listen` window reads.
pub fn descriptors_window(audio: &Audio, t0: f32, t1: f32) -> Descriptors {
    let sr = audio.sample_rate as f32;
    let cut = |t: f32, len: usize| ((t.max(0.0) * sr).round() as usize).min(len);
    let channels = audio
        .channels
        .iter()
        .map(|c| {
            let a = cut(t0, c.len());
            let b = cut(t1, c.len()).max(a);
            c[a..b].to_vec()
        })
        .collect();
    describe(&Audio {
        sample_rate: audio.sample_rate,
        channels,
    })
}

/// Partials (Hz, ascending) when `modes` (the strongest six) are not
/// harmonics of the sound's pitch: under half of them within 3% of a
/// multiple of it up to the 8th (a pitch far under the partials is a
/// subharmonic guess, and everything is a high multiple of it). Without a
/// pitch, the `harmonicity` alone decides. Noise (`flatness` over 0.3) has no
/// partials to name.
fn inharmonic(
    modes: &[Mode],
    pitch_hz: Option<f32>,
    harmonicity: f32,
    flatness: f32,
) -> (Vec<f32>, Option<f32>) {
    let mut top: Vec<Mode> = modes.to_vec();
    top.sort_by(|a, b| b.level_db.total_cmp(&a.level_db));
    top.truncate(6);
    if top.len() < 3 || flatness > 0.3 {
        return (Vec::new(), None);
    }
    let cents = partial_inharmonicity_cents(&top.iter().map(|m| m.hz).collect::<Vec<_>>());
    let harmonic = |f0: f32| {
        top.iter()
            .filter(|m| {
                let r = m.hz / f0;
                let k = r.round();
                r >= 0.9 && (1.0..=8.0).contains(&k) && (r - k).abs() / k < 0.03
            })
            .count()
    };
    let bad = match pitch_hz {
        Some(f0) => harmonic(f0) < top.len().div_ceil(2),
        None => harmonicity < 0.5,
    } || harmonicity < 0.5
        || cents.is_some_and(|c| c > INHARMONIC_CENTS);
    if !bad {
        return (Vec::new(), cents);
    }
    let mut hz: Vec<f32> = top.iter().map(|m| m.hz).collect();
    hz.sort_by(f32::total_cmp);
    (hz, cents)
}

fn flatness_of(p: &[f32]) -> f32 {
    let v: Vec<f64> = p.iter().map(|x| f64::from(*x).max(1e-20)).collect();
    if v.is_empty() || v.iter().sum::<f64>() < 1e-15 {
        return 0.0;
    }
    let geo = (v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp();
    let ari = v.iter().sum::<f64>() / v.len() as f64;
    (geo / ari) as f32
}

/// Measures `audio`.
pub fn describe(audio: &Audio) -> Descriptors {
    let sr = audio.sample_rate;
    let x = audio.mix_mono();
    let n = x.len();
    let seconds = audio.seconds();
    let peak = audio.peak();
    let r = rms(&audio.channels.iter().flatten().copied().collect::<Vec<_>>());
    let dc = x.iter().map(|v| f64::from(*v)).sum::<f64>() / n.max(1) as f64;
    // Peak envelope in 1 ms frames: attack.
    let ms = ((sr / 1000) as usize).max(1);
    let env: Vec<f32> = x
        .chunks(ms)
        .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
        .collect();
    let epk = env.iter().copied().fold(0.0f32, f32::max);
    let first = |th: f32| env.iter().position(|v| *v >= th * epk);
    let attack_s = match (first(0.1), first(0.9)) {
        (Some(a), Some(b)) if epk > 0.0 => (b.saturating_sub(a)) as f32 * ms as f32 / sr as f32,
        _ => 0.0,
    };
    // RMS in 10 ms frames: peak time and decays.
    let fr = ((sr / 100) as usize).max(1);
    let lev: Vec<f32> = x.chunks(fr).map(|c| gain_to_db(rms(c))).collect();
    let (pk, pv) = lev.iter().enumerate().fold(
        (0, f32::MIN),
        |m, (k, v)| if *v > m.1 { (k, *v) } else { m },
    );
    let decay = |drop: f32| -> Option<f32> {
        let th = pv - drop;
        let last_above = lev.iter().rposition(|v| *v > th)?;
        if last_above + 1 >= lev.len() {
            None
        } else {
            Some((last_above + 1 - pk) as f32 * fr as f32 / sr as f32)
        }
    };
    let th60 = sfx_graph::db_to_gain(-60.0);
    let loud = |c: &Vec<f32>| c.iter().position(|v| v.abs() > th60);
    let first_loud = audio.channels.iter().filter_map(loud).min().unwrap_or(n);
    let last_loud = audio
        .channels
        .iter()
        .filter_map(|c| c.iter().rposition(|v| v.abs() > th60))
        .max();
    let silence_end = match last_loud {
        Some(l) => (n - 1 - l) as f32 / sr as f32,
        None => seconds,
    };

    let win = window_for(n, sr);
    let spec = stft::stft(&x, sr, win, win / 4);
    // Spectral shape per third (audible frames only).
    let audible: Vec<bool> = spec
        .frames
        .iter()
        .map(|f| db(f.iter().sum()) > -60.0)
        .collect();
    let third = |k: usize| {
        let len = spec.frames.len();
        let r = (k * len / 3)..((k + 1) * len / 3).max(k * len / 3 + 1).min(len);
        let idx: Vec<usize> = r.filter(|i| audible[*i]).collect();
        let mut avg = vec![0.0f32; spec.bins()];
        for i in &idx {
            for (a, p) in avg.iter_mut().zip(&spec.frames[*i]) {
                *a += p / idx.len() as f32;
            }
        }
        avg
    };
    let thirds: Vec<Vec<f32>> = (0..3).map(third).collect();
    let lo_bin = spec.bin(20.0).max(1);
    let centroid = |p: &Vec<f32>| {
        let tot: f32 = p[lo_bin..].iter().sum();
        if tot <= 0.0 {
            0.0
        } else {
            (lo_bin..p.len()).map(|b| spec.hz(b) * p[b]).sum::<f32>() / tot
        }
    };
    let rolloff = |p: &Vec<f32>| {
        let tot: f32 = p[lo_bin..].iter().sum();
        let mut acc = 0.0;
        for b in lo_bin..p.len() {
            acc += p[b];
            if acc >= 0.85 * tot {
                return spec.hz(b);
            }
        }
        0.0
    };
    let flatness = |p: &Vec<f32>| {
        let v: Vec<f64> = p[lo_bin..p.len() - 1]
            .iter()
            .map(|x| f64::from(*x).max(1e-20))
            .collect();
        if v.iter().sum::<f64>() < 1e-15 {
            return 0.0;
        }
        let geo = (v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp();
        let ari = v.iter().sum::<f64>() / v.len() as f64;
        (geo / ari) as f32
    };
    let sme = |f: &dyn Fn(&Vec<f32>) -> f32| Sme {
        start: f(&thirds[0]),
        middle: f(&thirds[1]),
        end: f(&thirds[2]),
    };
    // Band table.
    let seg = |k: usize| {
        let t0 = seconds * k as f32 / SEGMENTS as f32;
        let t1 = seconds * (k + 1) as f32 / SEGMENTS as f32;
        band_db(&spec, &spec.mean(spec.frames_in(t0, t1)))
    };
    let segs: Vec<Vec<f32>> = (0..SEGMENTS).map(seg).collect();
    let bands: Vec<[f32; SEGMENTS]> = (0..BANDS.len())
        .map(|b| std::array::from_fn(|s| segs[s][b]))
        .collect();
    let bands_total = band_db(&spec, &spec.mean(0..spec.frames.len()));

    // Pitch.
    let mut px = x.clone();
    let need = (sr as usize / 40) + 2 * (1024.0 * sr as f32 / 48_000.0) as usize + 4;
    if px.len() < need {
        px.resize(need, 0.0);
    }
    let frames = pitch::yin(&px, sr);
    let aud: Vec<&pitch::PitchFrame> = frames.iter().filter(|f| f.level_db > -50.0).collect();
    let voiced_hz: Vec<f32> = aud.iter().filter_map(|f| f.hz).collect();
    let voiced = if aud.is_empty() {
        0.0
    } else {
        voiced_hz.len() as f32 / aud.len() as f32
    };
    let harmonicity = if aud.is_empty() {
        0.0
    } else {
        (1.0 - aud.iter().map(|f| f.aperiodicity.min(1.0)).sum::<f32>() / aud.len() as f32).max(0.0)
    };

    let peak_frame = spec
        .frames_in(pk as f32 * fr as f32 / sr as f32, seconds)
        .start;
    let stereo = if audio.is_stereo() {
        let (l, rr) = (&audio.channels[0], &audio.channels[1]);
        let (mut slr, mut sll, mut srr, mut mid, mut side) =
            (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (a, b) in l.iter().zip(rr) {
            let (a, b) = (f64::from(*a), f64::from(*b));
            slr += a * b;
            sll += a * a;
            srr += b * b;
            mid += (0.5 * (a + b)).powi(2);
            side += (0.5 * (a - b)).powi(2);
        }
        Some(Stereo {
            correlation: if sll > 0.0 && srr > 0.0 {
                (slr / (sll * srr).sqrt()) as f32
            } else {
                1.0
            },
            width: if mid > 0.0 {
                (side / mid).sqrt() as f32
            } else {
                0.0
            },
        })
    } else {
        None
    };
    let momentary = loudness::momentary(&audio.channels, sr, 0.1);
    let ons = onsets(&spec);
    let mode_list = modes(&spec, peak_frame);
    // Whole-sound spectrum (frames weigh by their energy).
    let mean_all = spec.mean(0..spec.frames.len());
    let tenth =
        |i: usize| spec.frames_in(seconds * i as f32 / 10.0, seconds * (i + 1) as f32 / 10.0);
    let centroid_tenths: Vec<Option<f32>> = (0..10)
        .map(|i| {
            let idx: Vec<usize> = tenth(i).filter(|k| audible[*k]).collect();
            if idx.is_empty() {
                return None;
            }
            let mut avg = vec![0.0f32; spec.bins()];
            for k in &idx {
                for (a, p) in avg.iter_mut().zip(&spec.frames[*k]) {
                    *a += p / idx.len() as f32;
                }
            }
            Some(centroid(&avg))
        })
        .collect();
    let pitch_tenths: Vec<Option<f32>> = (0..10)
        .map(|i| {
            let (t0, t1) = (seconds * i as f32 / 10.0, seconds * (i + 1) as f32 / 10.0);
            median(
                frames
                    .iter()
                    .filter(|f| f.t >= t0 && f.t < t1)
                    .filter_map(|f| f.hz)
                    .collect(),
            )
        })
        .collect();
    let onset_rows = onset_rows(&x, &spec, &ons, &frames, &centroid);
    let band_shares = {
        let tot: f32 = mean_all[1..].iter().sum();
        let mut sh = [0.0f32; 4];
        if tot > 0.0 {
            for b in 1..mean_all.len() {
                let f = spec.hz(b);
                let k =
                    usize::from(f >= 200.0) + usize::from(f >= 2000.0) + usize::from(f >= 8000.0);
                sh[k] += 100.0 * mean_all[b] / tot;
            }
        }
        sh
    };
    // The band holding the middle 80% of the energy and its flatness.
    let (occupied_hz, flatness_occupied) = {
        let tot: f32 = mean_all[lo_bin..].iter().sum();
        let (mut acc, mut b10, mut b90) = (0.0f32, lo_bin, mean_all.len() - 1);
        let (mut got10, mut got90) = (false, false);
        for b in lo_bin..mean_all.len() {
            acc += mean_all[b];
            if !got10 && acc >= 0.1 * tot {
                (b10, got10) = (b, true);
            }
            if !got90 && acc >= 0.9 * tot {
                (b90, got90) = (b, true);
            }
        }
        let flat = (tot > 0.0 && b90 + 1 >= b10 + 5).then(|| flatness_of(&mean_all[b10..=b90]));
        ((spec.hz(b10), spec.hz(b90)), flat)
    };
    // Attack: from the first sample over -60 dBFS to 90% of / the sample peak.
    let (attack_to90_s, attack_to_peak_s) = {
        let c = (sr as usize / 4000).max(1);
        let env_c = chunk_peaks(&x, c);
        let top = env_c.iter().copied().fold(0.0f32, f32::max);
        if top <= 0.0 || first_loud >= n {
            (0.0, 0.0)
        } else {
            let at = |k: usize| ((k * c).saturating_sub(first_loud)) as f32 / sr as f32;
            let kp = env_c.iter().position(|v| *v >= top).unwrap_or(0);
            let k90 = env_c.iter().position(|v| *v >= 0.9 * top).unwrap_or(kp);
            (at(k90), at(kp))
        }
    };
    let span = (first_loud, last_loud.map_or(first_loud, |l| l + 1));
    let (density, fev) = density(&x, sr, span);
    let fine_onsets: Vec<f32> = fev.iter().map(|e| e.t).collect();
    let events = event_summary(&fev, last_loud.map_or(0.0, |l| (l + 1) as f32 / sr as f32));
    // The tenths count the same events, scaled by what the overlap correction
    // added to the count (1 when events are far apart).
    let tenth_scale = if density.counted_per_s > 0.0 {
        (density.per_s / density.counted_per_s).clamp(1.0, 20.0)
    } else {
        1.0
    };
    let density_tenths: Vec<f32> = (0..10)
        .map(|i| {
            let (t0, t1) = (seconds * i as f32 / 10.0, seconds * (i + 1) as f32 / 10.0);
            let k = fine_onsets.iter().filter(|t| **t >= t0 && **t < t1).count();
            if t1 > t0 {
                k as f32 / (t1 - t0) * tenth_scale
            } else {
                0.0
            }
        })
        .collect();
    let swell_attack_s = swell_attack(&x, sr, first_loud);
    let steadiness = {
        let v: Vec<f32> = momentary.iter().copied().filter(|v| *v > -70.0).collect();
        if v.len() < 2 {
            None
        } else {
            let lo = v.iter().copied().fold(f32::MAX, f32::min);
            let hi = v.iter().copied().fold(f32::MIN, f32::max);
            Some(Steadiness {
                min_lufs: lo,
                max_lufs: hi,
                spread_lu: hi - lo,
            })
        }
    };
    let (inharmonic_partials, inharmonicity_cents) = inharmonic(
        &mode_list,
        median(frames.iter().filter_map(|f| f.hz).collect()),
        harmonicity,
        flatness_of(&mean_all[lo_bin..mean_all.len() - 1]),
    );
    Descriptors {
        seconds,
        sample_rate: sr,
        channels: audio.channels.len(),
        peak_db: gain_to_db(peak),
        true_peak_db: gain_to_db(loudness::true_peak(&audio.channels)),
        lufs: loudness::integrated(&audio.channels, sr),
        lufs_momentary_max: momentary.iter().copied().fold(-90.0, f32::max),
        rms_db: gain_to_db(r),
        crest_db: gain_to_db(peak) - gain_to_db(r),
        dc_db: gain_to_db(dc as f32),
        attack_s,
        peak_t: pk as f32 * fr as f32 / sr as f32,
        decay20_s: decay(20.0),
        decay60_s: decay(60.0),
        silence_start_s: first_loud as f32 / sr as f32,
        silence_end_s: silence_end,
        onsets: ons,
        pitch_hz: median(voiced_hz),
        voiced,
        harmonicity,
        centroid_hz: sme(&centroid),
        rolloff_hz: sme(&rolloff),
        flatness: sme(&flatness),
        bands,
        bands_total,
        modes: mode_list,
        stereo,
        centroid_all_hz: centroid(&mean_all),
        rolloff_all_hz: rolloff(&mean_all),
        centroid_tenths,
        onset_rows,
        density,
        events,
        band_shares,
        pitch_tenths,
        inharmonic_partials,
        attack_to90_s,
        attack_to_peak_s,
        flatness_all: flatness_of(&mean_all[lo_bin..mean_all.len() - 1]),
        occupied_hz,
        flatness_occupied,
        steadiness,
        is_loop: false,
        swell_attack_s,
        close_pairs: fine_onsets
            .windows(2)
            .filter(|w| w[1] - w[0] < 0.02)
            .map(|w| (w[0], w[1]))
            .collect(),
        fine_onsets,
        density_tenths,
        inharmonicity_cents,
    }
}

fn hz(v: f32) -> String {
    if v >= 1000.0 {
        format!("{:.2}k", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}

fn opt_s(v: Option<f32>) -> String {
    v.map_or("never".into(), |s| format!("{s:.3} s"))
}

impl Descriptors {
    /// The text table.
    pub fn table(&self) -> String {
        let mut o = String::new();
        o += &format!(
            "length {:.3} s, {} Hz, {}\n",
            self.seconds,
            self.sample_rate,
            if self.channels > 1 { "stereo" } else { "mono" }
        );
        o += &format!(
            "level: peak {:.1} dBFS, true peak {:.1} dBTP, {:.1} LUFS (momentary max {:.1}), RMS {:.1} dBFS, crest {:.1} dB, DC {:.0} dB\n",
            self.peak_db,
            self.true_peak_db,
            self.lufs,
            self.lufs_momentary_max,
            self.rms_db,
            self.crest_db,
            self.dc_db
        );
        o += &format!(
            "time: {}, {}\n",
            if self.is_loop {
                "loop: n/a (see steadiness)".to_string()
            } else {
                format!(
                    "attack (10-90% rise) {:.3} s, peak at {:.3} s, decay -20 dB {}, -60 dB {}",
                    self.attack_s,
                    self.peak_t,
                    opt_s(self.decay20_s),
                    opt_s(self.decay60_s)
                )
            },
            self.silence_words()
        );
        if !self.is_loop {
            o += &format!(
                "swell/attack (the headline): {:.1} ms, first sound to 90% of the smoothed-envelope peak (10 ms RMS)\n",
                self.swell_attack_s * 1000.0
            );
        }
        let ons: Vec<String> = self
            .onsets
            .iter()
            .take(16)
            .map(|x| format!("{:.3}({:.1})", x.t, x.strength))
            .collect();
        o += &format!(
            "onsets {} (40 ms merge); fine {} (5 ms): {}{}\n",
            self.onsets.len(),
            self.density.count,
            ons.join(" "),
            if self.onsets.len() > 16 { " ..." } else { "" }
        );
        o += &format!(
            "pitch: {} (voiced {:.0}%), harmonicity {:.2}\n",
            self.pitch_hz
                .map_or("none".into(), |p| format!("{p:.1} Hz")),
            self.voiced * 100.0,
            self.harmonicity
        );
        o += &format!(
            "spectrum (start/middle/end): centroid {}/{}/{} Hz, rolloff {}/{}/{} Hz, flatness {:.2}/{:.2}/{:.2}\n",
            hz(self.centroid_hz.start),
            hz(self.centroid_hz.middle),
            hz(self.centroid_hz.end),
            hz(self.rolloff_hz.start),
            hz(self.rolloff_hz.middle),
            hz(self.rolloff_hz.end),
            self.flatness.start,
            self.flatness.middle,
            self.flatness.end
        );
        o += "bands dB  | 0-20%  20-40%  40-60%  60-80%  80-100% | all\n";
        for (b, row) in self.bands.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .map(|v| format!("{:7.1}", v.max(-99.9)))
                .collect();
            o += &format!(
                "  {:>5} Hz|{} | {:6.1}\n",
                BAND_LABELS[b],
                cells.join(""),
                self.bands_total[b].max(-99.9)
            );
        }
        if !self.modes.is_empty() {
            // A partial whose fitted decay is over 5 times the sound's length
            // says nothing about the sound (a noise bin's flat track).
            let limit = self.seconds * 5.0;
            let ignored = self.modes.iter().filter(|m| m.decay_s > limit).count();
            let m: Vec<String> = self
                .modes
                .iter()
                .filter(|m| m.decay_s <= limit)
                .map(|m| {
                    let d = if m.decay_s >= 600.0 {
                        "sustained".to_string()
                    } else {
                        format!("{:.2} s", m.decay_s)
                    };
                    format!("{} Hz {d} {:.0} dB", hz(m.hz), m.level_db)
                })
                .collect();
            let note = if ignored > 0 {
                format!(" ({ignored} ignored: slower than the sound)")
            } else {
                String::new()
            };
            if m.is_empty() {
                o += &format!("modes: none left{note}\n");
            } else {
                o += &format!("modes: {}{note}\n", m.join("; "));
            }
        }
        if let Some(s) = self.stereo {
            o += &format!(
                "stereo: correlation {:.2}, width {:.2}\n",
                s.correlation, s.width
            );
        }
        o += &self.extra_lines();
        o
    }

    /// Marks the sound as a loop: the table says "loop: n/a" where attack,
    /// peak-at and decay would be.
    pub fn with_loop(mut self, is_loop: bool) -> Self {
        self.is_loop = is_loop;
        self
    }

    /// The `events:` line: how many fine events are strong and how many
    /// weak, and where the last one is. None for a loop (no ends to speak of)
    /// and when there are no events.
    fn events_line(&self) -> Option<String> {
        let last = self.events.last_s?;
        if self.is_loop {
            return None;
        }
        let tail = if self.events.tail_s < 0.1 {
            "silence follows it".to_string()
        } else {
            format!(
                "then {:.2} s of sound with no new event: a decaying or continuous tail",
                self.events.tail_s
            )
        };
        Some(format!(
            "events: {} strong (>= 0.5 of the loudest onset), {} weak; last event at {:.3} s ({})\n",
            self.events.strong, self.events.weak, last, tail
        ))
    }

    /// The silence wording of the `time:` line.
    fn silence_words(&self) -> String {
        if self.silence_start_s >= self.seconds - 1e-6 {
            return "all silent (under -60 dBFS)".into();
        }
        let lead = if self.silence_start_s < 0.005 {
            "no leading silence".to_string()
        } else {
            format!("leading silence {:.3} s", self.silence_start_s)
        };
        let trail = if self.silence_end_s < 0.01 {
            "no trailing silence".to_string()
        } else {
            format!(
                "trailing silence from {:.3} s ({:.3} s long)",
                self.seconds - self.silence_end_s,
                self.silence_end_s
            )
        };
        format!("{lead}, {trail}")
    }

    /// The later lines of the table (whole-sound spectrum, tenths, shares,
    /// pitch contour, attack detail, density, steadiness, onset rows), each
    /// short; the onset table is capped at [`MAX_ONSET_ROWS`] rows.
    fn extra_lines(&self) -> String {
        let mut o = String::new();
        let flat_occ = match self.flatness_occupied {
            Some(f) => format!(
                "{f:.2} in {}-{} Hz (middle 80% of the energy)",
                hz(self.occupied_hz.0),
                hz(self.occupied_hz.1)
            ),
            None => format!(
                "none: the middle 80% of the energy is {}-{} Hz, too narrow (tonal)",
                hz(self.occupied_hz.0),
                hz(self.occupied_hz.1)
            ),
        };
        o += &format!(
            "whole sound: centroid {} Hz, rolloff {} Hz, flatness {:.2} full band, {flat_occ}\n",
            hz(self.centroid_all_hz),
            hz(self.rolloff_all_hz),
            self.flatness_all
        );
        o += &format!(
            "energy shares: <200 Hz {:.0}%, 200-2k {:.0}%, 2k-8k {:.0}%, >8k {:.0}%\n",
            self.band_shares[0], self.band_shares[1], self.band_shares[2], self.band_shares[3]
        );
        let tenths = |v: &[Option<f32>]| {
            v.iter()
                .map(|c| c.map_or("-".to_string(), hz))
                .collect::<Vec<_>>()
                .join(" ")
        };
        o += &format!(
            "centroid by tenth (Hz): {}\n",
            tenths(&self.centroid_tenths)
        );
        if self.pitch_tenths.iter().any(Option::is_some) {
            o += &format!("pitch by tenth (Hz): {}\n", tenths(&self.pitch_tenths));
        }
        if !self.inharmonic_partials.is_empty() {
            let p: Vec<String> = self.inharmonic_partials.iter().map(|f| hz(*f)).collect();
            o += &format!(
                "inharmonic: partials at {} Hz{} (the pitch line is one of them or a guess)\n",
                p.join(" "),
                self.inharmonicity_cents.map_or(String::new(), |c| format!(
                    ", {c:.0} cents off whole ratios to the lowest"
                ))
            );
        }
        if !self.is_loop {
            o += &format!(
                "attack detail: first sound to 90% of peak {:.2} ms (sample-peak envelope), to the sample peak {:.2} ms (attack above is the 10-90% rise; swell/attack is the headline)\n",
                self.attack_to90_s * 1000.0,
                self.attack_to_peak_s * 1000.0
            );
        }
        let d = &self.density;
        o += &format!(
            "density: {:.0} events/s ({} in {:.2} s; 1 ms envelope over 1 kHz, 6 dB rise, {:.0} ms apart; onsets above merge under 40 ms)\n",
            d.per_s, d.count, d.span_s, d.min_spacing_ms
        );
        if d.count > 0 && (d.per_s - d.counted_per_s).abs() > 0.1 * d.counted_per_s.max(1.0) {
            o += &format!(
                "density estimate: the counter reads {:.0}/s but events overlap ({}); {:.0}/s is the corrected rate (tenths scaled the same)\n",
                d.counted_per_s,
                match d.method {
                    RateMethod::Period => "a regular train, rate from the envelope's repetition",
                    RateMethod::Peaks => "rate from envelope peaks",
                    _ => "rate corrected for events hidden by the one before",
                },
                d.per_s
            );
        }
        if self.density.count > 0 {
            o += &format!(
                "density by tenth (events/s): {}\n",
                self.density_tenths
                    .iter()
                    .map(|v| format!("{v:.0}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        if let Some(line) = self.events_line() {
            o += &line;
        }
        if !self.is_loop && !self.close_pairs.is_empty() {
            let at: Vec<String> = self
                .close_pairs
                .iter()
                .take(4)
                .map(|(a, b)| format!("{:.1}, {:.1}", a * 1000.0, b * 1000.0))
                .collect();
            o += &format!(
                "closely spaced events: {} pairs within 20 ms (at {} ms{})\n",
                self.close_pairs.len(),
                at.join("; "),
                if self.close_pairs.len() > 4 {
                    "; ..."
                } else {
                    ""
                }
            );
        }
        if !self.is_loop && !self.fine_onsets.is_empty() {
            // Compact, 20 to a line; the last row says the unit.
            let ms = |t: f32| {
                let v = t * 1000.0;
                if v < 10.0 {
                    format!("{v:.1}")
                } else {
                    format!("{v:.0}")
                }
            };
            let shown: Vec<String> = self.fine_onsets.iter().take(40).map(|t| ms(*t)).collect();
            let rows = shown.chunks(20).count();
            for (i, row) in shown.chunks(20).enumerate() {
                o += &format!(
                    "{} {}{}\n",
                    if i == 0 {
                        "fine onsets:"
                    } else {
                        "            "
                    },
                    row.join(" "),
                    if i + 1 == rows { " (ms)" } else { "" }
                );
            }
            if self.fine_onsets.len() > 40 {
                o += &format!(
                    "            ... {} more fine onsets not shown\n",
                    self.fine_onsets.len() - 40
                );
            }
        }
        if let Some(s) = self.steadiness.filter(|_| self.is_loop) {
            o += &format!(
                "steadiness: loudness over 400 ms windows {:.1} to {:.1} LUFS, spread {:.1} LU\n",
                s.min_lufs, s.max_lufs, s.spread_lu
            );
        }
        if self.is_loop {
            o += "onset table: left out for a loop (read density)\n";
        } else if !self.onset_rows.is_empty() {
            o += "onset table: t s, strength, attack ms (0.25 res), -20 dB ms, -60 dB ms (>N: still sounding at the next onset), centroid Hz, pitch Hz (start>end; n/a: inharmonic or unvoiced)\n";
            for r in &self.onset_rows {
                let pitch = if let Some((a, b)) = r.glide_hz {
                    format!(
                        "glide {} {}>{}",
                        if b > a { "up" } else { "down" },
                        hz(a),
                        hz(b)
                    )
                } else if r.pitch_na {
                    "n/a".into()
                } else {
                    match (r.pitch_hz, r.pitch_start_hz, r.pitch_end_hz) {
                        (_, Some(a), Some(b)) if crate::pitch::cents(a, b).abs() > 60.0 => {
                            format!("{}>{}", hz(a), hz(b))
                        }
                        (Some(p), _, _) => hz(p),
                        _ => "-".into(),
                    }
                };
                let d60 = if r.decay60_open {
                    format!(">{:.0}", r.decay60_ms)
                } else {
                    format!("{:.0}", r.decay60_ms)
                };
                o += &format!(
                    "  {:7.3} {:4.1} {:6.2} {:>6} {:>7} {:>6} {:>20}\n",
                    r.t,
                    r.strength,
                    r.attack_ms,
                    r.decay20_ms.map_or("-".into(), |v| format!("{v:.0}")),
                    d60,
                    hz(r.centroid_hz),
                    pitch
                );
            }
            if self.onsets.len() > self.onset_rows.len() {
                o += &format!(
                    "  ... {} more onsets not shown\n",
                    self.onsets.len() - self.onset_rows.len()
                );
            }
        }
        o
    }
}

/// A ready `resonators` node from a recording: the strongest `count` modes
/// as [hz, decay_s, gain], fed by `input`. Each gain is the mode's amplitude
/// at the hit in the recording's own scale, so a unit `impulse` into the node
/// reproduces the recording's level (see [`crate::modal`]).
pub fn modes_node(audio: &Audio, count: usize, name: &str, input: &str) -> serde_json::Value {
    let modes = crate::modal::extract(audio, count.clamp(1, 32));
    // Four significant digits.
    let sig = |v: f32| {
        if v <= 0.0 {
            return 0.0;
        }
        let k = 10f32.powf(3.0 - v.log10().floor());
        (v * k).round() / k
    };
    let round = |v: f32, k: f32| (v * k).round() / k;
    serde_json::json!({
        "type": "resonators",
        "name": name,
        "input": input,
        "modes": modes
            .iter()
            .map(|m| serde_json::json!([
                round(m.hz, 100.0),
                round(m.decay_s.min(10.0), 1000.0),
                sig(m.gain),
            ]))
            .collect::<Vec<_>>(),
    })
}
