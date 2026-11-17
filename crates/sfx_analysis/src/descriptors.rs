//! What a sound measures as: the agent's main readout.

use serde::Serialize;
use sfx_graph::{Audio, gain_to_db, loudness};

use crate::pitch;
use crate::stft::{self, Spectra, db};

/// Octave band centres of the band table, Hz (the last runs to Nyquist).
pub const BANDS: [f32; 8] = [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
/// Labels of [`BANDS`].
pub const BAND_LABELS: [&str; 8] = ["63", "125", "250", "500", "1k", "2k", "4k", "8k+"];
/// Equal time segments of the band table.
pub const SEGMENTS: usize = 5;

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
        onsets: onsets(&spec),
        pitch_hz: median(voiced_hz),
        voiced,
        harmonicity,
        centroid_hz: sme(&centroid),
        rolloff_hz: sme(&rolloff),
        flatness: sme(&flatness),
        bands,
        bands_total,
        modes: modes(&spec, peak_frame),
        stereo,
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
            "time: attack {:.3} s, peak at {:.3} s, decay -20 dB {}, -60 dB {}, silence start {:.3} s / end {:.3} s\n",
            self.attack_s,
            self.peak_t,
            opt_s(self.decay20_s),
            opt_s(self.decay60_s),
            self.silence_start_s,
            self.silence_end_s
        );
        let ons: Vec<String> = self
            .onsets
            .iter()
            .take(16)
            .map(|x| format!("{:.3}({:.1})", x.t, x.strength))
            .collect();
        o += &format!(
            "onsets {}: {}{}\n",
            self.onsets.len(),
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
            let m: Vec<String> = self
                .modes
                .iter()
                .map(|m| {
                    let d = if m.decay_s >= 600.0 {
                        "sustained".to_string()
                    } else {
                        format!("{:.2} s", m.decay_s)
                    };
                    format!("{} Hz {d} {:.0} dB", hz(m.hz), m.level_db)
                })
                .collect();
            o += &format!("modes: {}\n", m.join("; "));
        }
        if let Some(s) = self.stereo {
            o += &format!(
                "stereo: correlation {:.2}, width {:.2}\n",
                s.correlation, s.width
            );
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
