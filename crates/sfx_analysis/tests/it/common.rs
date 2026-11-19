//! Helpers every node test module shares (`use crate::common::*;`), in
//! `crates/sfx_analysis/tests/`. Render a node list, pull one node's samples,
//! and measure them (spectrum, peaks, band levels, aliasing, level).
//!
//! Levels follow `sfx_analysis::stft`: powers are scaled so a sine of
//! amplitude A sums to A^2 / 2 over its bins, so a full-scale sine reads
//! -3 dB in `band_db` and a 0.1 amplitude sine -23 dB.

#![allow(dead_code)] // each test file uses some of these

use realfft::RealFftPlanner;
use serde_json::{Value, json};
use sfx_analysis::stft::{db, hann, stft};
use sfx_graph::{Audio, Buf, RenderOpts, SoundDoc};

/// The sample rate of every document these helpers build.
pub const SR: u32 = 48_000;

/// A document of `nodes` (the type / name / fields objects) lasting
/// `duration` seconds. When none is an `output`, one is added reading
/// `read` (the last node by default), named `_out`.
fn doc(nodes: &[Value], duration: f32, read: Option<&str>) -> SoundDoc {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes.to_vec();
    d.duration = Some(duration);
    if !d
        .nodes
        .iter()
        .any(|n| n.get("type").and_then(Value::as_str) == Some("output"))
    {
        let last = read.map(str::to_string).or_else(|| {
            nodes
                .last()
                .and_then(|n| n.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        d.nodes
            .push(json!({"type": "output", "name": "_out", "input": last.unwrap_or_default()}));
    }
    d
}

fn opts() -> RenderOpts {
    RenderOpts {
        cached: false,
        ..RenderOpts::default()
    }
}

/// Renders `nodes` for `duration` seconds and returns the output node's
/// audio (with its default 2 ms / 5 ms edge fades). Panics with the render
/// error, so a test reads as the sound it builds.
pub fn render(nodes: impl AsRef<[Value]>, duration: f32) -> Audio {
    let d = doc(nodes.as_ref(), duration, None);
    d.render(&opts()).unwrap_or_else(|e| panic!("{e}")).audio
}

/// The samples of node `name` (mono; the left channel of a stereo node),
/// `duration` seconds long, before any output stage. The node must be audio.
pub fn node_audio(nodes: impl AsRef<[Value]>, name: &str, duration: f32) -> Vec<f32> {
    let d = doc(nodes.as_ref(), duration, Some(name));
    let r = d.render(&opts()).unwrap_or_else(|e| panic!("{e}"));
    match &*r
        .eval
        .buf(name)
        .unwrap_or_else(|e| panic!("node `{name}`: {e}"))
    {
        Buf::Audio(x) => x.clone(),
        Buf::Stereo(x) => x.iter().map(|s| s[0]).collect(),
        other => panic!("node `{name}` is {}, not audio", other.kind()),
    }
}

/// Renders twice, once on the global thread pool and once on a single
/// thread, with no cache, and requires the same bits in every sample.
pub fn assert_bit_identical(nodes: impl AsRef<[Value]>, duration: f32) {
    let d = doc(nodes.as_ref(), duration, None);
    let wide = d.render(&opts()).unwrap_or_else(|e| panic!("{e}")).audio;
    let one = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| d.render(&opts()).unwrap_or_else(|e| panic!("{e}")).audio);
    assert_eq!(wide.channels.len(), one.channels.len(), "channel count");
    for (c, (a, b)) in wide.channels.iter().zip(&one.channels).enumerate() {
        assert_eq!(a.len(), b.len(), "channel {c} length");
        let bad = a
            .iter()
            .zip(b)
            .position(|(p, q)| p.to_bits() != q.to_bits());
        assert!(
            bad.is_none(),
            "channel {c}: samples differ at {} between the two renders",
            bad.unwrap_or(0)
        );
    }
}

/// The power spectrum of `x` as (Hz, dB) per bin from 0 to Nyquist: one
/// Hann window over the whole signal, zero-padded to the next power of two.
/// Levels as in the module comment.
pub fn spectrum(x: &[f32], sr: u32) -> Vec<(f32, f32)> {
    let (hz, p) = power_bins(x, sr);
    hz.into_iter().zip(p).map(|(f, v)| (f, db(v))).collect()
}

/// (bin Hz, bin power) with the stft scaling.
fn power_bins(x: &[f32], sr: u32) -> (Vec<f32>, Vec<f32>) {
    let n = x.len().max(2);
    let size = n.next_power_of_two().max(16);
    let w = hann(n);
    let energy: f32 = w.iter().map(|v| v * v).sum();
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let mut input = fft.make_input_vec();
    for (i, v) in input.iter_mut().enumerate() {
        *v = if i < x.len() { x[i] * w[i] } else { 0.0 };
    }
    let mut out = fft.make_output_vec();
    fft.process(&mut input, &mut out).unwrap();
    let norm = 1.0 / (size as f32 * energy.max(1e-20));
    let half = size / 2;
    let hz = (0..=half)
        .map(|k| k as f32 * sr as f32 / size as f32)
        .collect();
    let p = out
        .iter()
        .enumerate()
        .map(|(k, z)| z.norm_sqr() * norm * if k == 0 || k == half { 1.0 } else { 2.0 })
        .collect();
    (hz, p)
}

/// Frequency of the strongest spectral peak, parabolic-interpolated.
pub fn peak_hz(x: &[f32], sr: u32) -> f32 {
    peaks(x, sr, 1, 0.0).first().map_or(0.0, |p| p.0)
}

/// The `count` strongest local maxima as (Hz, dB), strongest first, each
/// within `within_db` of the strongest (pass a big number for all), at
/// least 5 bins apart. Hz is parabolic-interpolated on the dB curve.
pub fn peaks(x: &[f32], sr: u32, count: usize, within_db: f32) -> Vec<(f32, f32)> {
    let s = spectrum(x, sr);
    if s.len() < 3 {
        return Vec::new();
    }
    let bin = s[1].0 - s[0].0;
    let top = s.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let mut cand: Vec<usize> = (1..s.len() - 1)
        .filter(|&k| s[k].1 > s[k - 1].1 && s[k].1 >= s[k + 1].1)
        .collect();
    cand.sort_by(|&a, &b| s[b].1.total_cmp(&s[a].1));
    let mut out: Vec<(usize, f32, f32)> = Vec::new();
    for k in cand {
        if s[k].1 < top - within_db.max(0.0) && !out.is_empty() {
            break;
        }
        if out.iter().any(|(j, _, _)| k.abs_diff(*j) < 5) {
            continue;
        }
        let (a, b, c) = (s[k - 1].1, s[k].1, s[k + 1].1);
        let denom = a - 2.0 * b + c;
        let off = if denom.abs() > 1e-9 {
            (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        out.push((k, s[k].0 + off * bin, b));
        if out.len() == count {
            break;
        }
    }
    out.into_iter().map(|(_, f, l)| (f, l)).collect()
}

/// Total power (dB) in `lo..hi` Hz.
pub fn band_db(x: &[f32], sr: u32, lo: f32, hi: f32) -> f32 {
    let (hz, p) = power_bins(x, sr);
    db(hz
        .iter()
        .zip(&p)
        .filter(|(f, _)| **f >= lo && **f < hi)
        .map(|(_, v)| v)
        .sum())
}

/// Energy off the harmonic grid of `f0` relative to the energy on it, in dB:
/// the aliasing (or inharmonic junk) a nonlinearity left in a tone at `f0`.
/// A bin is on the grid when within 8 resolution cells (8 * sr / len Hz, which
/// leaves a pure tone at -63 dB) of
/// a multiple of `f0` below Nyquist; bins under that distance from DC are
/// ignored.
pub fn alias_db(x: &[f32], sr: u32, f0: f32) -> f32 {
    let (hz, p) = power_bins(x, sr);
    let tol = 8.0 * sr as f32 / x.len().max(1) as f32;
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (f, v) in hz.iter().zip(&p) {
        if *f < tol {
            continue;
        }
        let nearest = (f / f0).round().max(1.0) * f0;
        if (f - nearest).abs() <= tol {
            on += f64::from(*v);
        } else {
            off += f64::from(*v);
        }
    }
    (10.0 * (off.max(1e-30) / on.max(1e-30)).log10()) as f32
}

/// RMS level in dB (a full-scale sine reads -3.01; silence -120).
pub fn rms_db(x: &[f32]) -> f32 {
    if x.is_empty() {
        return -120.0;
    }
    let ms = x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64;
    ((10.0 * ms.max(1e-12).log10()) as f32).max(-120.0)
}

/// Every sample is finite; names the first that is not.
pub fn assert_finite(x: &[f32], what: &str) {
    if let Some(i) = x.iter().position(|v| !v.is_finite()) {
        panic!("{what}: sample {i} is {}", x[i]);
    }
}

/// The error of rendering `nodes` plus an `output` reading `input`, for
/// `duration` seconds. Panics when the render succeeds.
pub fn render_error(nodes: Vec<Value>, input: &str, duration: f32) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": input}));
    d.duration = Some(duration);
    d.render(&RenderOpts::default()).unwrap_err()
}

/// The error of rendering `nodes` exactly as given (no `output` added) for
/// `duration` seconds. Panics when the render succeeds.
pub fn render_error_raw(nodes: Vec<Value>, duration: f32) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.duration = Some(duration);
    match d.render(&RenderOpts::default()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    }
}

/// A document of `nodes` with an optional `duration` and nothing added.
pub fn bare_doc(nodes: Vec<Value>, duration: Option<f32>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = duration;
    d
}

/// `node` with every field of the object `extra` set on it.
pub fn with_fields(mut node: Value, extra: Value) -> Value {
    node.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    node
}

/// An `osc` node named `src` at `hz` with amplitude `amp`.
pub fn sine_src(hz: f32, amp: f32) -> Value {
    json!({"type": "osc", "name": "src", "freq": hz, "amp": amp})
}

/// `secs` seconds of a sine at `hz` with amplitude `amp`, at `SR`.
pub fn sine_samples(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(secs * SR as f32) as usize)
        .map(|i| {
            amp * (std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR)).sin() as f32
        })
        .collect()
}

/// `x` as a mono `Audio` at `SR`.
pub fn mono_audio(x: &[f32]) -> Audio {
    Audio {
        sample_rate: SR,
        channels: vec![x.to_vec()],
    }
}

/// Power-weighted mean frequency of the whole-signal `spectrum` of `x`.
pub fn spectrum_centroid(x: &[f32]) -> f32 {
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (f, d) in spectrum(x, SR) {
        let p = 10f64.powf(f64::from(d) / 10.0);
        num += f64::from(f) * p;
        den += p;
    }
    (num / den.max(1e-30)) as f32
}

/// Centroid (Hz) of the mean power over a 2048 / 1024 STFT of `x`.
pub fn stft_centroid(x: &[f32]) -> f32 {
    let s = stft(x, SR, 2048, 1024);
    let mean = s.mean(0..s.frames.len());
    let total: f32 = mean.iter().sum();
    mean.iter()
        .enumerate()
        .map(|(b, p)| s.hz(b) * p)
        .sum::<f32>()
        / total.max(1e-20)
}

/// FNV-1a over the little-endian bit patterns of every sample of `x`.
pub fn fnv(x: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Largest absolute sample.
pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// Sample peak in dBFS (silence reads -180).
pub fn peak_db(x: &[f32]) -> f32 {
    20.0 * peak(x).max(1e-9).log10()
}

/// Linear RMS of `x` (0 for an empty slice).
pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Like `render_error`, with the `output` reading the last of `nodes`.
pub fn render_error_last(nodes: Vec<Value>, duration: f32) -> String {
    let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    render_error(nodes, &last, duration)
}

/// FNV-1a over the bit patterns of every sample, channel by channel.
pub fn fnv_channels(channels: &[Vec<f32>]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in channels {
        for v in c {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}
