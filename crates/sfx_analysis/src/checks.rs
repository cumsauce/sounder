//! Deterministic checks on a rendered sound (`verify` and `sound_ops`
//! `expect`): each prints PASS / FAIL with the measured value, and a FAIL
//! says what to change. Modeled on modeler's `MatExpect` + `MatDoc::check`.

use serde::{Deserialize, Serialize};
use sfx_graph::{Kind, RenderOpts, Rendered, SoundDoc, db_to_gain, gain_to_db};

use crate::descriptors::{self, BAND_LABELS, BANDS, Descriptors, SEGMENTS};
use crate::stft;

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Max {
    pub max: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
}

/// `true`, or the check's settings.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Toggle<T> {
    On(bool),
    With(T),
}

impl<T: Default + Clone> Toggle<T> {
    fn get(&self) -> Option<T> {
        match self {
            Toggle::On(true) => Some(T::default()),
            Toggle::On(false) => None,
            Toggle::With(t) => Some(t.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DcCheck {
    /// Largest DC offset, dB (default -40).
    #[serde(default)]
    pub max: Option<f32>,
}

/// One cell (or a whole row) of the band table.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BandCheck {
    /// "63", "125", "250", "500", "1k", "2k", "4k", "8k" (or a number of Hz:
    /// the band holding it).
    pub band: serde_json::Value,
    /// Time segment 0..4 (fifths of the sound); omitted = the whole sound.
    #[serde(default)]
    pub segment: Option<usize>,
    #[serde(default)]
    pub min_db: Option<f32>,
    #[serde(default)]
    pub max_db: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PitchCheck {
    pub hz: f32,
    /// Tolerance, cents (default 25).
    #[serde(default)]
    pub tol_cents: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variations {
    /// Seeds 1..=N rendered (default 6).
    #[serde(default)]
    pub seeds: Option<u64>,
    /// Largest loudness spread across them, LU (default 1).
    #[serde(default)]
    pub lufs_spread: Option<f32>,
    /// Smallest STFT distance between any two, dB (default 1).
    #[serde(default)]
    pub min_distance: Option<f32>,
}

/// The checks. With none given, `verify` runs [`Expect::defaults`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Sample peak at most `max` dBFS.
    #[serde(default)]
    pub peak_db: Option<Max>,
    /// True peak (4x oversampled) at most `max` dBTP.
    #[serde(default)]
    pub true_peak_db: Option<Max>,
    /// Integrated loudness in [min, max] LUFS.
    #[serde(default)]
    pub lufs: Option<Range>,
    /// Length in [min, max] seconds.
    #[serde(default)]
    pub duration: Option<Range>,
    /// First and last samples under -60 dBFS (no click at the edges).
    #[serde(default)]
    pub silent_edges: Option<bool>,
    /// No sample steps far above the local level (foley's click detector).
    #[serde(default)]
    pub clicks: Option<bool>,
    /// DC offset under `max` dB (default -40).
    #[serde(default)]
    pub dc: Option<Toggle<DcCheck>>,
    /// No NaN or infinite samples.
    #[serde(default)]
    pub finite: Option<bool>,
    /// Loop kind: no jump and no spectral change across the wrap.
    #[serde(default)]
    pub loop_seam: Option<bool>,
    /// Band-table limits.
    #[serde(default)]
    pub bands: Option<Vec<BandCheck>>,
    /// Median pitch within tol_cents of hz.
    #[serde(default)]
    pub pitch: Option<PitchCheck>,
    /// Onset count in [min, max].
    #[serde(default)]
    pub onsets: Option<Range>,
    /// Seeds differ audibly yet sit within lufs_spread LU.
    #[serde(default)]
    pub variations: Option<Toggle<Variations>>,
    /// An uncached re-render is bit-identical.
    #[serde(default)]
    pub rebuild: Option<bool>,
    /// Every macro keeps its claim: swept over 0, .25, .5, .75, 1 (the others
    /// where they are), its measure moves the claimed way at every step and
    /// no position peaks over -0.3 dBFS.
    #[serde(default)]
    pub macros: Option<bool>,
}

impl Expect {
    /// No check chosen (every field absent): `verify` then runs the defaults.
    pub fn is_empty(&self) -> bool {
        serde_json::to_value(self)
            .map(|v| {
                v.as_object()
                    .is_none_or(|m| m.values().all(serde_json::Value::is_null))
            })
            .unwrap_or(true)
    }

    /// `defaults`, plus `macros` when the sound has any.
    ///
    /// A loop checks `loop_seam` instead of `silent_edges` (a loop's first
    /// and last samples are not meant to be silent).
    pub fn defaults_for(doc: &SoundDoc) -> Expect {
        let looped = doc.kind == Kind::Loop;
        Expect {
            macros: (!doc.macros.is_empty()).then_some(true),
            silent_edges: (!looped).then_some(true),
            loop_seam: looped.then_some(true),
            ..Expect::defaults()
        }
    }

    /// `finite`, `peak_db` max -0.3, `silent_edges`, `dc`, `rebuild`.
    pub fn defaults() -> Expect {
        Expect {
            finite: Some(true),
            peak_db: Some(Max { max: -0.3 }),
            silent_edges: Some(true),
            dc: Some(Toggle::On(true)),
            rebuild: Some(true),
            ..Expect::default()
        }
    }
}

/// One check's verdict.
#[derive(Clone, Debug, serde::Serialize)]
pub struct CheckResult {
    pub name: String,
    pub pass: bool,
    pub detail: String,
}

/// PASS / FAIL lines.
pub fn report(results: &[CheckResult]) -> String {
    results
        .iter()
        .map(|r| {
            format!(
                "{} {}: {}\n",
                if r.pass { "PASS" } else { "FAIL" },
                r.name,
                r.detail
            )
        })
        .collect()
}

fn range_ok(r: &Range, v: f32) -> bool {
    r.min.is_none_or(|m| v >= m - 1e-4) && r.max.is_none_or(|m| v <= m + 1e-4)
}

fn range_text(r: &Range) -> String {
    match (r.min, r.max) {
        (Some(a), Some(b)) => format!("{a}..{b}"),
        (Some(a), None) => format!(">= {a}"),
        (None, Some(b)) => format!("<= {b}"),
        (None, None) => "any".into(),
    }
}

/// Which band `v` names.
fn band_index(v: &serde_json::Value) -> Result<usize, String> {
    let hz = match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
        serde_json::Value::String(s) => {
            let t = s.trim().trim_end_matches('+').to_ascii_lowercase();
            if let Some(i) = BAND_LABELS
                .iter()
                .position(|l| l.trim_end_matches('+') == t)
            {
                return Ok(i);
            }
            let (num, k) = match t.strip_suffix('k') {
                Some(n) => (n, 1000.0),
                None => (t.trim_end_matches("hz").trim(), 1.0),
            };
            num.parse::<f32>().map_err(|_| {
                format!(
                    "band `{s}`: use one of {} or a number of Hz",
                    BAND_LABELS.join(", ")
                )
            })? * k
        }
        other => {
            return Err(format!(
                "band {other}: use one of {} or a number of Hz",
                BAND_LABELS.join(", ")
            ));
        }
    };
    Ok(BANDS
        .iter()
        .enumerate()
        .min_by(|a, b| {
            (a.1.ln() - hz.max(1.0).ln())
                .abs()
                .total_cmp(&(b.1.ln() - hz.max(1.0).ln()).abs())
        })
        .map_or(0, |(i, _)| i))
}

/// Samples whose step from the last is far (12x) above what is usual in the
/// 10 ms round them (foley's `foley-sim` `clicks`). Their times, seconds.
pub fn clicks(x: &[f32], sr: u32) -> Vec<f32> {
    let frame = ((sr / 100) as usize).max(2);
    let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let mut out = Vec::new();
    for (k, f) in d.chunks(frame).enumerate() {
        let rms = (f.iter().map(|v| v * v).sum::<f32>() / f.len() as f32).sqrt();
        if rms < 1e-5 {
            continue;
        }
        for (j, v) in f.iter().enumerate() {
            if v.abs() > 12.0 * rms {
                out.push((k * frame + j + 1) as f32 / sr as f32);
            }
        }
    }
    out
}

/// How much the spectrum changes across a loop's wrap: the mean |dB|
/// difference between the end and the start in third-octave bands from 100 Hz
/// up (bands within 60 dB of the loudest). Bands, not bins: two windows of
/// noise differ by several dB in every bin and by well under 1 dB in a band.
fn seam_spectral(tail: &[f32], head: &[f32], sr: u32) -> f32 {
    let win = 2048.min(tail.len()).max(16).next_power_of_two() / 2;
    let inner = |x: &[f32]| {
        let s = stft::stft(x, sr, win, win / 4);
        // Frame k is centred on k * hop: keep those wholly inside.
        let first = (win / 2).div_ceil(win / 4);
        let last = (x.len().saturating_sub(win / 2)) / (win / 4);
        let mean = s.mean(first..last.max(first + 1).min(s.frames.len()));
        let nyq = sr as f32 / 2.0;
        let mut bands = Vec::new();
        let mut lo = 100.0f32;
        while lo < nyq {
            let hi = (lo * 2f32.powf(1.0 / 3.0)).min(nyq);
            let p: f32 = (0..mean.len())
                .filter(|&b| s.hz(b) >= lo && s.hz(b) < hi)
                .map(|b| mean[b])
                .sum();
            bands.push(p);
            lo = hi;
        }
        bands
    };
    let (a, b) = (inner(tail), inner(head));
    let top = a.iter().chain(&b).copied().fold(0.0f32, f32::max);
    let floor = stft::db(top) - 60.0;
    let (mut sum, mut k) = (0.0f32, 0usize);
    for (p, q) in a.iter().zip(&b) {
        let (x, y) = (stft::db(*p), stft::db(*q));
        if x > floor || y > floor {
            sum += (x.max(floor) - y.max(floor)).abs();
            k += 1;
        }
    }
    if k == 0 { 0.0 } else { sum / k as f32 }
}

/// Sweeps one macro; (pass, text).
fn macro_check(cx: &CheckCx, m: &sfx_graph::Macro) -> Result<(bool, String), String> {
    use crate::measure::{fmt, measure};
    let sfx_graph::Claim(measure_of, dir) = m.claim;
    let xs = [0.0, 0.25, 0.5, 0.75, 1.0];
    let (mut vals, mut peaks) = (Vec::new(), Vec::new());
    for x in xs {
        let r = cx.doc.render(&RenderOpts {
            macros: [(m.name.clone(), x)].into(),
            seed: Some(cx.rendered.seed),
            ..cx.opts.clone()
        })?;
        vals.push(measure(&r.audio, measure_of));
        peaks.push(r.peak_db);
    }
    let d = f32::from(dir);
    let tol = ((vals[4] - vals[0]).abs() * 0.01).max(1e-6);
    let steady = vals.windows(2).all(|w| (w[1] - w[0]) * d > -tol);
    let net = (vals[4] - vals[0]) * d > tol;
    let clip = peaks.iter().position(|p| *p > -0.3);
    let shown: Vec<String> = vals.iter().map(|v| fmt(measure_of, *v)).collect();
    let word = if dir > 0 { "rise" } else { "fall" };
    let list = shown.join(" -> ");
    Ok(match (steady && net, clip) {
        (true, None) => (
            true,
            format!("{} {}s over 0..1: {list}", measure_of.name(), word),
        ),
        (_, Some(i)) => (
            false,
            format!(
                "clips at {} (peak {:.1} dBFS): lower the gains it drives, or limit before the output",
                xs[i], peaks[i]
            ),
        ),
        (false, None) => (
            false,
            format!(
                "{} does not {word} over 0..1 ({list}): rename the macro to what it really does (claim), or fix what it moves",
                measure_of.name()
            ),
        ),
    })
}

/// What the checks read.
pub struct CheckCx<'a> {
    pub doc: &'a SoundDoc,
    pub rendered: &'a Rendered,
    pub opts: &'a RenderOpts,
}

/// Runs the checks in `e` against a render.
pub fn run(e: &Expect, cx: &CheckCx) -> Result<Vec<CheckResult>, String> {
    let a = &cx.rendered.audio;
    let sr = a.sample_rate;
    let mut out = Vec::new();
    let mut push = |name: &str, pass: bool, detail: String| {
        out.push(CheckResult {
            name: name.into(),
            pass,
            detail,
        })
    };
    let mut desc: Option<Descriptors> = None;
    let mut d = || -> Descriptors { desc.get_or_insert_with(|| descriptors::describe(a)).clone() };

    if e.finite == Some(true) {
        let bad = a
            .channels
            .iter()
            .flatten()
            .filter(|x| !x.is_finite())
            .count();
        push(
            "finite",
            bad == 0,
            if bad == 0 {
                "every sample is finite".into()
            } else {
                format!(
                    "{bad} NaN or infinite samples: look for a division by zero, a pow of a negative, or a filter with q near 0"
                )
            },
        );
    }
    if let Some(m) = &e.peak_db {
        let v = cx.rendered.peak_db;
        push(
            "peak_db",
            v <= m.max + 1e-4,
            if v <= m.max + 1e-4 {
                format!("{v:.2} dBFS (max {})", m.max)
            } else {
                format!(
                    "{v:.2} dBFS is over {}: lower output ceiling_db (or gain_db)",
                    m.max
                )
            },
        );
    }
    if let Some(m) = &e.true_peak_db {
        let v = gain_to_db(sfx_graph::loudness::true_peak(&a.channels));
        push(
            "true_peak_db",
            v <= m.max + 1e-4,
            if v <= m.max + 1e-4 {
                format!("{v:.2} dBTP (max {})", m.max)
            } else {
                format!(
                    "{v:.2} dBTP is over {}: lower output ceiling_db by {:.1} dB",
                    m.max,
                    v - m.max
                )
            },
        );
    }
    if let Some(r) = &e.lufs {
        let v = cx.rendered.lufs;
        let ok = range_ok(r, v);
        push(
            "lufs",
            ok,
            if ok {
                format!("{v:.2} LUFS ({})", range_text(r))
            } else {
                let target = r
                    .min
                    .zip(r.max)
                    .map_or(r.min.or(r.max).unwrap_or(v), |(a, b)| 0.5 * (a + b));
                format!(
                    "{v:.2} LUFS, want {}: set output lufs to {target:.1}",
                    range_text(r)
                )
            },
        );
    }
    if let Some(r) = &e.duration {
        let v = a.seconds();
        let ok = range_ok(r, v);
        push(
            "duration",
            ok,
            if ok {
                format!("{v:.3} s ({})", range_text(r))
            } else if cx.rendered.auto_length {
                format!(
                    "{v:.3} s, want {}: shorten or lengthen the decay envelope, or set duration",
                    range_text(r)
                )
            } else {
                format!("{v:.3} s, want {}: change duration", range_text(r))
            },
        );
    }
    if e.silent_edges == Some(true) {
        let th = db_to_gain(-60.0);
        let first = a
            .channels
            .iter()
            .map(|c| c.first().map_or(0.0, |v| v.abs()))
            .fold(0.0, f32::max);
        let last = a
            .channels
            .iter()
            .map(|c| c.last().map_or(0.0, |v| v.abs()))
            .fold(0.0, f32::max);
        let ok = first < th && last < th;
        let mut why = Vec::new();
        if first >= th {
            why.push(format!(
                "first sample at {:.1} dBFS (a click on start: add output fade_in)",
                gain_to_db(first)
            ));
        }
        if last >= th {
            why.push(format!(
                "last sample at {:.1} dBFS (cut off mid-wave: add output fade_out, or let it decay)",
                gain_to_db(last)
            ));
        }
        push(
            "silent_edges",
            ok,
            if ok {
                format!(
                    "edges at {:.0} / {:.0} dBFS",
                    gain_to_db(first),
                    gain_to_db(last)
                )
            } else {
                why.join("; ")
            },
        );
    }
    if e.clicks == Some(true) {
        let t = clicks(&a.mix_mono(), sr);
        push(
            "clicks",
            t.is_empty(),
            if t.is_empty() {
                "no clicks".into()
            } else {
                let at: Vec<String> = t.iter().take(5).map(|s| format!("{s:.3}")).collect();
                format!(
                    "{} clicks (at {} s): smooth hard edges (an envelope that jumps, a hard sync, sample_hold on audio) or low-pass them",
                    t.len(),
                    at.join(", ")
                )
            },
        );
    }
    if let Some(dc) = e.dc.as_ref().and_then(Toggle::get) {
        let max = dc.max.unwrap_or(-40.0);
        let worst = a
            .channels
            .iter()
            .map(|c| {
                gain_to_db(
                    (c.iter().map(|x| f64::from(*x)).sum::<f64>() / c.len().max(1) as f64) as f32,
                )
            })
            .fold(-200.0f32, f32::max);
        push(
            "dc",
            worst <= max,
            if worst <= max {
                format!("{worst:.0} dB (max {max})")
            } else {
                format!(
                    "DC offset {worst:.1} dB is over {max}: add an onepole hp at 20 Hz before the output"
                )
            },
        );
    }
    if e.loop_seam == Some(true) {
        if cx.doc.kind != Kind::Loop {
            push(
                "loop_seam",
                false,
                "the sound is a oneshot: set kind \"loop\" and a duration".into(),
            );
        } else {
            // The wrap sample against the loop's own statistics: the step
            // (a jump) and the error predicting x[0] from the last two
            // samples (a kink: a slope reversal clicks too).
            let mut worst_ratio = 0.0f32;
            for c in &a.channels {
                let n = c.len();
                if n < 4 {
                    continue;
                }
                let p99 = |mut v: Vec<f32>| {
                    v.sort_by(f32::total_cmp);
                    v[(v.len() * 99) / 100].max(1e-5)
                };
                let steps = p99(c.windows(2).map(|w| (w[1] - w[0]).abs()).collect());
                let curves = p99(c
                    .windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .collect());
                let step = (c[0] - c[n - 1]).abs() / steps;
                let kink = (c[0] - (2.0 * c[n - 1] - c[n - 2]))
                    .abs()
                    .max((c[1] - (2.0 * c[0] - c[n - 1])).abs())
                    / curves;
                worst_ratio = worst_ratio.max(step).max(kink);
            }
            let m = a.mix_mono();
            let w = (16_384.min(m.len() / 2)).max(16);
            let tail = &m[m.len() - w..];
            let head = &m[..w];
            let spec_d = seam_spectral(tail, head, sr);
            let ok = worst_ratio <= 2.0 && spec_d <= 6.0;
            push(
                "loop_seam",
                ok,
                format!(
                    "wrap discontinuity {worst_ratio:.2}x the loop's 99th-percentile step / curvature (max 2), spectral change {spec_d:.1} dB (max 6){}",
                    if ok {
                        ""
                    } else {
                        ": make every element periodic in the duration (lfo freq a multiple of 1/duration) or crossfade the end into the start"
                    }
                ),
            );
        }
    }
    if let Some(bands) = &e.bands {
        let dd = d();
        for b in bands {
            let i = band_index(&b.band)?;
            let (v, wh) = match b.segment {
                Some(s) if s < SEGMENTS => (dd.bands[i][s], format!("{}-{}%", s * 20, s * 20 + 20)),
                Some(s) => return Err(format!("bands: segment {s}: use 0..{}", SEGMENTS - 1)),
                None => (dd.bands_total[i], "whole sound".into()),
            };
            let ok = b.min_db.is_none_or(|m| v >= m) && b.max_db.is_none_or(|m| v <= m);
            let want = range_text(&Range {
                min: b.min_db,
                max: b.max_db,
            });
            push(
                &format!("bands {} Hz {wh}", BAND_LABELS[i]),
                ok,
                if ok {
                    format!("{v:.1} dB ({want})")
                } else {
                    format!(
                        "{v:.1} dB, want {want}: {} that band with a filter (peak / shelf) or change the source",
                        if b.min_db.is_some_and(|m| v < m) {
                            "boost"
                        } else {
                            "cut"
                        }
                    )
                },
            );
        }
    }
    if let Some(p) = &e.pitch {
        let dd = d();
        let tol = p.tol_cents.unwrap_or(25.0);
        match dd.pitch_hz {
            Some(hz) => {
                let c = crate::pitch::cents(p.hz, hz);
                push(
                    "pitch",
                    c.abs() <= tol,
                    if c.abs() <= tol {
                        format!("{hz:.2} Hz ({c:+.1} cents from {})", p.hz)
                    } else {
                        format!(
                            "{hz:.2} Hz is {c:+.0} cents from {} (tolerance {tol}): scale the frequencies by {:.4}",
                            p.hz,
                            p.hz / hz
                        )
                    },
                );
            }
            None => push(
                "pitch",
                false,
                format!(
                    "no pitch found (voiced {:.0}%): the sound is noisy or too short to hold a pitch",
                    dd.voiced * 100.0
                ),
            ),
        }
    }
    if let Some(r) = &e.onsets {
        let k = d().onsets.len() as f32;
        let ok = range_ok(r, k);
        push(
            "onsets",
            ok,
            if ok {
                format!("{k} onsets ({})", range_text(r))
            } else {
                format!("{k} onsets, want {}", range_text(r))
            },
        );
    }
    if let Some(v) = e.variations.as_ref().and_then(Toggle::get) {
        let n = v.seeds.unwrap_or(6).clamp(2, 32);
        let spread_max = v.lufs_spread.unwrap_or(1.0);
        let dmin = v.min_distance.unwrap_or(1.0);
        let mut renders = Vec::new();
        for s in 1..=n {
            let r = cx.doc.render(&RenderOpts {
                seed: Some(s),
                ..cx.opts.clone()
            })?;
            renders.push((r.lufs, r.audio.mix_mono()));
        }
        let lo = renders.iter().map(|r| r.0).fold(f32::MAX, f32::min);
        let hi = renders.iter().map(|r| r.0).fold(f32::MIN, f32::max);
        let mut closest = (f32::MAX, 0u64, 0u64);
        for i in 0..renders.len() {
            for j in i + 1..renders.len() {
                let dist = stft::distance(&renders[i].1, &renders[j].1, sr);
                if dist < closest.0 {
                    closest = (dist, i as u64 + 1, j as u64 + 1);
                }
            }
        }
        let ok_spread = hi - lo <= spread_max;
        let ok_dist = closest.0 >= dmin;
        let mut why = format!(
            "{n} seeds: loudness spread {:.2} LU (max {spread_max}), closest pair seeds {} and {} at {:.2} dB (min {dmin})",
            hi - lo,
            closest.1,
            closest.2,
            closest.0
        );
        if !ok_spread {
            why += ": the loudness varies too much; narrow the random ranges that drive level or set output lufs";
        } else if !ok_dist {
            why += ": the seeds sound alike; let the seed drive more (noise seeds, wander, random pitch)";
        }
        push("variations", ok_spread && ok_dist, why);
    }
    if e.macros == Some(true) {
        for m in &cx.doc.macros {
            let r = macro_check(cx, m)?;
            push(&format!("macro {}", m.name), r.0, r.1);
        }
    }
    if e.rebuild == Some(true) {
        let fresh = cx.doc.render(&RenderOpts {
            cached: false,
            seed: Some(cx.rendered.seed),
            ..cx.opts.clone()
        })?;
        let same = fresh.audio.channels.len() == a.channels.len()
            && fresh.audio.channels.iter().zip(&a.channels).all(|(p, q)| {
                p.len() == q.len() && p.iter().zip(q).all(|(x, y)| x.to_bits() == y.to_bits())
            });
        push(
            "rebuild",
            same,
            if same {
                "an uncached re-render is bit-identical".into()
            } else {
                "an uncached re-render differs: a node is not deterministic (report it as a bug)"
                    .into()
            },
        );
    }
    Ok(out)
}
