//! S6: `chorus`, `flanger`, `phaser`, `freq_shift`. Each test measures
//! something and prints it (these lines go in docs/nodes/s6.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::stft::db;
use sfx_graph::node::parse_body;
use sfx_graph::{RenderOpts, SoundDoc};

fn noise() -> Value {
    json!({"type": "noise", "name": "src", "seed": 5, "amp": 0.3})
}

fn sine(hz: f32) -> Value {
    json!({"type": "osc", "name": "src", "freq": hz, "amp": 0.5})
}

/// `src` then a node of `ty` named `fx` with extra fields.
fn fx(src: Value, ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "fx", "input": "src"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![src, r]
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "fx"}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
}

/// Mean power spectrum (dB per bin) over consecutive blocks of `block`
/// samples: a smooth estimate of a noise-driven magnitude response.
fn averaged(x: &[f32], block: usize) -> Vec<(f32, f32)> {
    let mut acc: Vec<(f32, f64)> = Vec::new();
    let mut count = 0.0f64;
    for c in x.chunks_exact(block) {
        let s = spectrum(c, SR);
        if acc.is_empty() {
            acc = s.iter().map(|p| (p.0, 0.0)).collect();
        }
        for (a, p) in acc.iter_mut().zip(&s) {
            a.1 += 10f64.powf(f64::from(p.1) / 10.0);
        }
        count += 1.0;
    }
    acc.into_iter()
        .map(|(f, p)| (f, db((p / count) as f32)))
        .collect()
}

fn median(s: &[(f32, f32)]) -> f32 {
    let mut v: Vec<f32> = s.iter().map(|p| p.1).collect();
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Frequencies of the notches of `s`: runs of bins (after a 7-bin moving
/// average) more than `depth_db` under the median, each reported at its
/// lowest bin.
fn notches(s: &[(f32, f32)], depth_db: f32) -> Vec<f32> {
    let floor = median(s) - depth_db;
    let smooth: Vec<f32> = (0..s.len())
        .map(|k| {
            let (lo, hi) = (k.saturating_sub(3), (k + 4).min(s.len()));
            s[lo..hi].iter().map(|p| p.1).sum::<f32>() / (hi - lo) as f32
        })
        .collect();
    let mut out = Vec::new();
    let mut k = 4;
    while k < s.len() - 4 {
        if smooth[k] < floor {
            let start = k;
            while k < s.len() - 4 && smooth[k] < floor {
                k += 1;
            }
            let best = (start..k)
                .min_by(|&a, &b| smooth[a].total_cmp(&smooth[b]))
                .unwrap();
            out.push(s[best].0);
        }
        k += 1;
    }
    out
}

/// Lowest level in `lo..hi` Hz of an averaged spectrum, with its frequency.
fn lowest(s: &[(f32, f32)], lo: f32, hi: f32) -> (f32, f32) {
    s.iter()
        .filter(|p| p.0 >= lo && p.0 <= hi)
        .fold((0.0, f32::MAX), |m, p| if p.1 < m.1 { *p } else { m })
}

// ----------------------------------------------------------------- flanger

#[test]
fn flanger_manual_1ms_notches_every_khz_from_500() {
    let x = node_audio(
        fx(
            noise(),
            "flanger",
            json!({"manual": 0.001, "mix": 0.5, "feedback": 0}),
        ),
        "fx",
        10.0,
    );
    assert_finite(&x, "fx");
    let s = averaged(&x[SR as usize / 10..], 8192);
    let med = median(&s);
    let mut worst = f32::MAX; // the shallowest notch, dB under the median
    let mut off = 0.0f32; // the largest position error, Hz
    for k in 0..24 {
        let want = 500.0 + 1000.0 * k as f32;
        let (at, level) = lowest(&s, want - 100.0, want + 100.0);
        worst = worst.min(med - level);
        off = off.max((at - want).abs());
    }
    // And the peaks between them are where the comb says (1 kHz multiples).
    let peak_mid = (1..23)
        .map(|k| lowest(&s, 1000.0 * k as f32 - 100.0, 1000.0 * k as f32 + 100.0).1)
        .fold(f32::MAX, f32::min);
    println!(
        "flanger manual 1 ms, mix 0.5, white noise: 24 notches 500..23500 Hz, shallowest {worst:.1} dB under the median level ({med:.1} dB), largest position error {off:.1} Hz, lowest level near the 1 kHz multiples {peak_mid:.1} dB"
    );
    assert!(worst >= 15.0, "shallowest notch {worst:.1} dB");
    assert!(off <= 12.0, "position error {off} Hz");
    assert!(peak_mid > med - 3.0, "peaks stay up: {peak_mid} vs {med}");
}

/// The sample and height of the strongest sample at least 10 samples after
/// `from` (the echo of an impulse at `from`).
fn echo(x: &[f32], from: usize) -> (usize, f32) {
    let (i, v) = x
        .iter()
        .enumerate()
        .skip(from + 10)
        .fold(
            (0, 0.0f32),
            |m, (i, v)| if v.abs() > m.1.abs() { (i, *v) } else { m },
        );
    (i - from, v)
}

#[test]
fn flanger_lfo_swings_the_delay_and_feedback_repeats_it() {
    // An impulse sees the delay at its own moment: rate 0.1 Hz, 3 +- 2 ms,
    // so 3 ms (144 samples) at t = 0 and 5 ms (240) a quarter cycle later.
    let at = |t: f32| {
        let nodes = fx(
            json!({"type": "impulse", "name": "src", "at": t}),
            "flanger",
            json!({"rate": 0.1, "depth": 0.002, "delay": 0.003, "mix": 1}),
        );
        let x = node_audio(nodes, "fx", 3.0);
        echo(&x, (t * SR as f32) as usize)
    };
    let (d0, h0) = at(0.0);
    let (d1, h1) = at(2.5);
    println!(
        "flanger lfo 0.1 Hz, 3 +- 2 ms, impulse echo: {d0} samples ({:.3} ms) high {h0:.3} at t = 0; {d1} samples ({:.3} ms) high {h1:.3} at t = 2.5 s",
        d0 as f32 / 48.0,
        d1 as f32 / 48.0
    );
    assert!(d0.abs_diff(144) <= 1 && d1.abs_diff(240) <= 1);
    // Feedback 0.5 at a fixed 1 ms: echoes at 48, 96, 144 samples of height
    // 1, 0.5, 0.25.
    let nodes = fx(
        json!({"type": "impulse", "name": "src", "at": 0.1}),
        "flanger",
        json!({"manual": 0.001, "mix": 1, "feedback": 0.5}),
    );
    let x = node_audio(nodes, "fx", 0.5);
    let base = (0.1 * SR as f32) as usize;
    let h: Vec<f32> = (1..=3).map(|k| x[base + 48 * k]).collect();
    println!(
        "flanger fixed 1 ms, feedback 0.5, mix 1: echoes at 48, 96, 144 samples read {:.4}, {:.4}, {:.4}",
        h[0], h[1], h[2]
    );
    assert!((h[0] - 1.0).abs() < 1e-3 && (h[1] - 0.5).abs() < 1e-3 && (h[2] - 0.25).abs() < 1e-3);
    // Negative feedback alternates the sign: 1, -0.5, 0.25.
    let nodes = fx(
        json!({"type": "impulse", "name": "src", "at": 0.1}),
        "flanger",
        json!({"manual": 0.001, "mix": 1, "feedback": -0.5}),
    );
    let x = node_audio(nodes, "fx", 0.5);
    assert!((x[base + 96] + 0.5).abs() < 1e-3 && (x[base + 144] - 0.25).abs() < 1e-3);
}

// ------------------------------------------------------------------ phaser

#[test]
fn phaser_n_stages_make_n_over_2_notches_where_the_allpass_says() {
    for (stages, want) in [(4usize, 2usize), (6, 3), (8, 4), (12, 6)] {
        // A fixed phaser is linear and time-invariant: an impulse at 0.5 s
        // gives its exact frequency response.
        let src = json!({"type": "impulse", "name": "src", "at": 0.5});
        let x = node_audio(
            fx(
                src,
                "phaser",
                json!({"stages": stages, "depth": 0, "center": 800, "mix": 0.5}),
            ),
            "fx",
            1.0,
        );
        assert_finite(&x, "fx");
        let s = spectrum(&x, SR);
        let found = notches(&s, 15.0);
        // The chain has equal first-order stages at 800 Hz (warped): total
        // phase -(2k+1) pi at tan(pi f / sr) = tan(800 pi / sr) *
        // tan((2k+1) pi / (2 stages)).
        let wc = (std::f64::consts::PI * 800.0 / f64::from(SR)).tan();
        let expect: Vec<f32> = (0..stages / 2)
            .map(|k| {
                let r = (std::f64::consts::PI * (2 * k + 1) as f64 / (2 * stages) as f64).tan();
                (f64::from(SR) / std::f64::consts::PI * (r * wc).atan()) as f32
            })
            .collect();
        println!(
            "phaser {stages} stages, fixed at 800 Hz, impulse response: {} notches at {found:.0?} Hz (allpass maths: {expect:.0?})",
            found.len()
        );
        assert_eq!(found.len(), want, "{found:?}");
        for (f, e) in found.iter().zip(&expect) {
            assert!((f - e).abs() < 0.06 * e + 15.0, "{f} vs {e}");
        }
    }
}

#[test]
fn phaser_lfo_sweeps_the_notches() {
    let nodes = fx(
        noise(),
        "phaser",
        json!({"rate": 0.5, "depth": 1.5, "center": 800, "mix": 0.5}),
    );
    let x = node_audio(nodes, "fx", 2.0);
    assert_finite(&x, "fx");
    // The LFO crests (sine peak) at 0.5 s and troughs at 1.5 s: the lowest
    // notch is at a different place at the two moments.
    let hi = averaged(&x[SR as usize * 9 / 20..SR as usize * 11 / 20], 2048);
    let lo = averaged(&x[SR as usize * 29 / 20..SR as usize * 31 / 20], 2048);
    let (nh, nl) = (notches(&hi, 6.0), notches(&lo, 6.0));
    println!(
        "phaser lfo 0.5 Hz +-1.5 oct at 800 Hz, 100 ms of white noise each: lowest notch {:.0} Hz at the crest, {:.0} Hz at the trough (all dips found: {nh:.0?} / {nl:.0?})",
        nh[0], nl[0]
    );
    assert!(!nh.is_empty() && !nl.is_empty());
    assert!(nh[0] > 4.0 * nl[0], "{} vs {}", nh[0], nl[0]);
}

// ------------------------------------------------------------------ chorus

#[test]
fn chorus_spreads_a_sine_by_depth_rate_and_frequency() {
    let (f0, depth, rate) = (440.0f32, 0.003f32, 0.8f32);
    let x = node_audio(
        fx(
            sine(f0),
            "chorus",
            json!({"voices": 1, "spread": 0, "mix": 1, "rate": rate, "depth": depth, "delay": 0.02}),
        ),
        "fx",
        10.0,
    );
    assert_finite(&x, "fx");
    let x = &x[SR as usize / 2..]; // skip the delay line filling
    let s = spectrum(x, SR);
    let total: f64 = s.iter().map(|p| 10f64.powf(f64::from(p.1) / 10.0)).sum();
    // Half-width: the smallest offset from f0 holding 98 % of the power.
    let mut by_offset: Vec<(f32, f64)> = s
        .iter()
        .map(|p| ((p.0 - f0).abs(), 10f64.powf(f64::from(p.1) / 10.0)))
        .collect();
    by_offset.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut acc = 0.0;
    let mut half = 0.0;
    for (o, p) in &by_offset {
        acc += p;
        if acc >= 0.98 * total {
            half = *o;
            break;
        }
    }
    let want = f0 * depth * std::f32::consts::TAU * rate;
    println!(
        "chorus 1 voice, 440 Hz sine, depth 3 ms, rate 0.8 Hz: 98 % of the power within +-{half:.2} Hz of 440; f * depth * 2 pi * rate = {want:.2} Hz (ratio {:.2})",
        half / want
    );
    assert!((half - want).abs() <= 0.3 * want, "{half} vs {want}");
}

#[test]
fn chorus_stereo_spread_decorrelates() {
    let a = render(
        fx(
            sine(440.0),
            "chorus",
            json!({"voices": 3, "spread": 1, "mix": 0.5}),
        ),
        3.0,
    );
    assert_eq!(a.channels.len(), 2);
    let (l, r) = (&a.channels[0][4800..], &a.channels[1][4800..]);
    let corr = |l: &[f32], r: &[f32]| {
        let (mut lr, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
        for (a, b) in l.iter().zip(r) {
            lr += f64::from(*a) * f64::from(*b);
            ll += f64::from(*a) * f64::from(*a);
            rr += f64::from(*b) * f64::from(*b);
        }
        lr / (ll * rr).sqrt()
    };
    let c = corr(l, r);
    // spread 0 is mono: one channel.
    let mono = render(
        fx(
            sine(440.0),
            "chorus",
            json!({"voices": 3, "spread": 0, "mix": 0.5}),
        ),
        0.5,
    );
    println!(
        "chorus 3 voices spread 1, 440 Hz sine, mix 0.5: L/R correlation {c:.3}, rms L {:.2} dB R {:.2} dB; spread 0: {} channel",
        rms_db(l),
        rms_db(r),
        mono.channels.len()
    );
    assert!(c < 0.9, "correlation {c}");
    assert_eq!(mono.channels.len(), 1);
    // Per-channel level stays near the mono chorus's.
    assert!((rms_db(l) - rms_db(r)).abs() < 3.0);
}

#[test]
fn chorus_mix_zero_is_the_input_and_stereo_input_runs_per_channel() {
    let dry = node_audio(vec![sine(300.0)], "src", 0.3);
    let zero = node_audio(
        fx(sine(300.0), "chorus", json!({"spread": 0, "mix": 0})),
        "fx",
        0.3,
    );
    assert!(
        dry.iter()
            .zip(&zero)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "mix 0 passes the input unchanged"
    );
    for (ty, extra) in [
        ("chorus", json!({"spread": 0})),
        ("flanger", json!({})),
        ("phaser", json!({})),
        ("freq_shift", json!({"shift": 50})),
    ] {
        let mut nodes = vec![
            sine(300.0),
            json!({"type": "pan", "name": "st", "input": "src", "pan": 0.5}),
        ];
        let mut n = json!({"type": ty, "name": "fx", "input": "st"});
        n.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        nodes.push(n);
        let a = render(nodes, 0.3);
        assert_eq!(a.channels.len(), 2, "{ty}: stereo in, stereo out");
        assert!(rms_db(&a.channels[0]) > -40.0 && rms_db(&a.channels[1]) > -40.0);
    }
    println!(
        "mix 0 is bit-identical; stereo input stays stereo for chorus (spread 0), flanger, phaser, freq_shift"
    );
}

// -------------------------------------------------------------- freq_shift

#[test]
fn freq_shift_moves_a_sine_and_rejects_the_image() {
    for (shift, want, image) in [(100.0f32, 1100.0f32, 900.0f32), (-100.0, 900.0, 1100.0)] {
        let x = node_audio(
            fx(sine(1000.0), "freq_shift", json!({"shift": shift})),
            "fx",
            1.0,
        );
        assert_finite(&x, "fx");
        let x = &x[SR as usize / 10..];
        let p = peaks(x, SR, 1, 0.0)[0];
        let top = band_db(x, SR, want - 5.0, want + 5.0);
        let img = band_db(x, SR, image - 5.0, image + 5.0);
        let orig = band_db(x, SR, 995.0, 1005.0);
        println!(
            "freq_shift {shift:+} Hz on a 1 kHz sine: peak {:.2} Hz ({:.1} dB band), image at {image} Hz {:.1} dB ({:.1} dB under the peak), 1 kHz {orig:.1} dB",
            p.0,
            top,
            img,
            top - img
        );
        assert!((p.0 - want).abs() < 1.0, "{}", p.0);
        assert!(img < top - 40.0, "image {:.1} dB under the peak", top - img);
        assert!(orig < top - 40.0);
    }
}

#[test]
fn freq_shift_zero_keeps_the_spectrum() {
    let nodes = fx(noise(), "freq_shift", json!({"shift": 0}));
    let y = node_audio(nodes, "fx", 4.0);
    let x = node_audio(vec![noise()], "src", 4.0);
    let (y, x) = (&y[SR as usize / 10..], &x[SR as usize / 10..]);
    let mut worst = 0.0f32;
    let mut at = 0.0;
    for (lo, hi) in [
        (100.0, 200.0),
        (200.0, 400.0),
        (400.0, 800.0),
        (800.0, 1600.0),
        (1600.0, 3200.0),
        (3200.0, 6400.0),
        (6400.0, 12800.0),
        (12800.0, 20000.0),
    ] {
        let d = (band_db(y, SR, lo, hi) - band_db(x, SR, lo, hi)).abs();
        if d > worst {
            worst = d;
            at = lo;
        }
    }
    // And a 1 kHz sine, bin by bin around the line.
    let ys = node_audio(
        fx(sine(1000.0), "freq_shift", json!({"shift": 0})),
        "fx",
        1.0,
    );
    let xs = node_audio(vec![sine(1000.0)], "src", 1.0);
    let dsine =
        (band_db(&ys[4800..], SR, 990.0, 1010.0) - band_db(&xs[4800..], SR, 990.0, 1010.0)).abs();
    println!(
        "freq_shift 0 on white noise: octave bands 100 Hz..20 kHz differ from the input by at most {worst:.3} dB (band from {at} Hz); 1 kHz sine {dsine:.3} dB"
    );
    assert!(worst < 0.1 && dsine < 0.1);
}

#[test]
fn freq_shift_takes_a_signal_and_mixes() {
    // A rising shift 0 -> 2000 Hz over 1 s on a 1 kHz sine: energy moves up.
    let nodes = vec![
        sine(1000.0),
        json!({"type": "ramp", "name": "sh", "from": 0, "to": 2000, "time": 1.0, "curve": "linear"}),
        json!({"type": "freq_shift", "name": "fx", "input": "src", "shift": "sh"}),
    ];
    let x = node_audio(nodes, "fx", 1.0);
    assert_finite(&x, "fx");
    let early = peak_hz(&x[2400..9600], SR);
    let late = peak_hz(&x[SR as usize * 9 / 10..SR as usize * 19 / 20], SR);
    println!(
        "freq_shift ramp 0 -> 2000 Hz: peak {early:.0} Hz at 0.05..0.2 s, {late:.0} Hz at 0.9..0.95 s"
    );
    assert!(early < 1300.0 && late > 2600.0);
    // mix 0.5 on a +100 Hz shift: both the line at 1000 and at 1100.
    let half = node_audio(
        fx(
            sine(1000.0),
            "freq_shift",
            json!({"shift": 100, "mix": 0.5}),
        ),
        "fx",
        1.0,
    );
    let (d, s) = (
        band_db(&half[4800..], SR, 995.0, 1005.0),
        band_db(&half[4800..], SR, 1095.0, 1105.0),
    );
    println!(
        "freq_shift +100 mix 0.5: dry line {d:.2} dB, shifted line {s:.2} dB (expected -15.05 each)"
    );
    assert!((d + 15.05).abs() < 0.3 && (s + 15.05).abs() < 0.3);
}

// ------------------------------------------------------------ determinism

fn all_nodes() -> Vec<(&'static str, Value)> {
    vec![
        ("chorus", json!({"voices": 5, "spread": 0.7, "rate": 1.3})),
        ("chorus", json!({"voices": 2, "spread": 0})),
        ("flanger", json!({"rate": 0.9, "feedback": 0.6})),
        ("phaser", json!({"stages": 6, "feedback": -0.4, "rate": 2})),
        ("freq_shift", json!({"shift": 137.5, "mix": 0.8})),
    ]
}

#[test]
fn modfx_are_bit_identical() {
    for (ty, extra) in all_nodes() {
        assert_bit_identical(fx(noise(), ty, extra), 0.3);
    }
}

#[test]
fn modfx_are_finite_at_the_extremes() {
    let loud = || json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6});
    let cases: Vec<(&str, Value)> = vec![
        (
            "chorus",
            json!({"rate": 20, "depth": 0.1, "delay": 0.1, "voices": 8, "mix": 1}),
        ),
        (
            "chorus",
            json!({"rate": 0, "depth": 0, "delay": 0.001, "voices": 1, "spread": 0}),
        ),
        (
            "flanger",
            json!({"rate": 20, "depth": 0.0001, "delay": 0.0001, "feedback": 0.95, "mix": 1}),
        ),
        (
            "flanger",
            json!({"delay": 0.02, "depth": 0.02, "feedback": -0.95, "rate": 20}),
        ),
        ("flanger", json!({"manual": 0.00004167, "feedback": 0.95})),
        ("flanger", json!({"manual": 0.02, "feedback": -0.95})),
        (
            "phaser",
            json!({"stages": 12, "rate": 20, "depth": 4, "center": 20, "feedback": 0.95, "mix": 1}),
        ),
        (
            "phaser",
            json!({"stages": 12, "rate": 20, "depth": 4, "center": 21600, "feedback": -0.95}),
        ),
        ("freq_shift", json!({"shift": -24000})),
        ("freq_shift", json!({"shift": 24000})),
        ("freq_shift", json!({"shift": 1e-6})),
    ];
    let mut peak_all = 0.0f32;
    for (ty, extra) in cases {
        let nodes = fx(loud(), ty, extra.clone());
        let a = render(&nodes, 0.5);
        for c in &a.channels {
            assert_finite(c, &format!("{ty} {extra}"));
            peak_all = peak_all.max(c.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        }
        let x = node_audio(&nodes, "fx", 0.5);
        assert_finite(&x, &format!("{ty} {extra}"));
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 1.0e9, "{ty} {extra}: peak {peak}");
    }
    // Signals far outside the legal range are clamped, never NaN.
    for (ty, fields) in [
        (
            "chorus",
            json!({"rate": "wild", "depth": "wild", "mix": "wild"}),
        ),
        (
            "flanger",
            json!({"rate": "wild", "depth": "wild", "feedback": "wild", "mix": "wild"}),
        ),
        ("flanger", json!({"manual": "wild", "feedback": "wild"})),
        (
            "phaser",
            json!({"rate": "wild", "depth": "wild", "feedback": "wild", "mix": "wild"}),
        ),
        ("freq_shift", json!({"shift": "wild", "mix": "wild"})),
    ] {
        let mut nodes = vec![
            sine(1000.0),
            json!({"type": "ramp", "name": "wild", "from": -1.0e9, "to": 1.0e9, "time": 0.3, "curve": "linear"}),
        ];
        let mut n = json!({"type": ty, "name": "fx", "input": "src"});
        n.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        nodes.push(n);
        let x = node_audio(&nodes, "fx", 0.3);
        assert_finite(&x, &format!("{ty} wild signals"));
    }
    println!(
        "extremes: all four nodes at their field limits with a 1e6 input, and +-1e9 signals on every signal field: all finite, peak at most 1e9"
    );
}

// ------------------------------------------------------------------ errors

#[test]
fn modfx_errors_say_the_field_value_and_fix() {
    let cases: Vec<(Vec<Value>, &str)> = vec![
        (
            fx(sine(440.0), "chorus", json!({"voices": 9})),
            "voices: 9: give a whole number from 1 to 8",
        ),
        (
            fx(sine(440.0), "chorus", json!({"delay": 0.5})),
            "delay: 0.5: give a number from 0.001 to 0.1 seconds",
        ),
        (
            fx(sine(440.0), "chorus", json!({"depth": 0.05, "delay": 0.02})),
            "depth: 0.05: give a number from 0 to 0.02",
        ),
        (
            fx(sine(440.0), "chorus", json!({"spread": 2})),
            "spread: 2: give a number from 0 to 1",
        ),
        (
            fx(sine(440.0), "chorus", json!({"rate": 50})),
            "rate: 50: give a number from 0 to 20 Hz",
        ),
        (
            fx(sine(440.0), "flanger", json!({"feedback": 1.5})),
            "feedback: 1.5: give a number from -0.95 to 0.95",
        ),
        (
            fx(sine(440.0), "flanger", json!({"manual": 0.5})),
            "manual: 0.5: give a number from",
        ),
        (
            fx(sine(440.0), "flanger", json!({"mix": -1})),
            "mix: -1: give a number from 0 to 1",
        ),
        (
            fx(sine(440.0), "phaser", json!({"stages": 5})),
            "stages: 5: give 4, 6, 8 or 12",
        ),
        (
            fx(sine(440.0), "phaser", json!({"center": 5})),
            "center: 5: give a number from 20 to 21600 Hz",
        ),
        (
            fx(sine(440.0), "phaser", json!({"depth": 9})),
            "depth: 9: give a number from 0 to 4 octaves",
        ),
        (
            fx(sine(440.0), "freq_shift", json!({"shift": 30000})),
            "shift: 30000: give a number between -24000 and 24000 Hz",
        ),
    ];
    for (nodes, want) in cases {
        let e = render_error(nodes);
        println!("{e}");
        assert!(e.contains(want), "wanted `{want}` in `{e}`");
    }
    let e = render_error(fx(sine(440.0), "phaser", json!({"stage": 4})));
    assert!(e.contains("stage") && e.contains("stages"), "{e}");
    // spread above 0 refuses a stereo input and names the fixes.
    let e = render_error(vec![
        sine(440.0),
        json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
        json!({"type": "chorus", "name": "fx", "input": "st", "spread": 1}),
    ]);
    println!("{e}");
    assert!(
        e.contains("input") && e.contains("stereo") && e.contains("spread to 0"),
        "{e}"
    );
}

#[test]
fn modfx_are_registered_causal_and_not_random() {
    for ty in ["chorus", "flanger", "phaser", "freq_shift"] {
        let ex = sfx_graph::example(ty).expect("registered");
        let mut body = ex.clone();
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, "x").unwrap();
        assert!(node.causal() && !node.random(), "{ty}");
        let text = sfx_graph::describe(Some(ty)).unwrap();
        assert!(text.contains("input"), "{ty}");
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
}

/// The recipe in docs/nodes/s6.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "wash", "color": "pink", "seed": 3, "amp": 0.4}),
        json!({"type": "phaser", "name": "swirl", "input": "wash", "rate": 0.3, "stages": 8, "center": 600, "depth": 1.5, "feedback": 0.5, "mix": 0.6}),
        json!({"type": "chorus", "name": "wide", "input": "swirl", "spread": 0, "rate": 0.5, "mix": 0.4}),
        json!({"type": "mix", "name": "summed", "inputs": ["wide"], "to": "mono"}),
        json!({"type": "chorus", "name": "wide2", "input": "summed", "spread": 1, "voices": 4, "mix": 0.5}),
    ]
}

#[test]
fn modfx_recipe_renders() {
    let a = render(recipe(), 2.0);
    println!(
        "recipe: {} channels, {} samples, rms L {:.2} dB R {:.2} dB",
        a.channels.len(),
        a.len(),
        rms_db(&a.channels[0]),
        rms_db(&a.channels[1])
    );
    assert_eq!(a.channels.len(), 2);
    assert_eq!(a.len(), 2 * SR as usize);
    assert!(rms_db(&a.channels[0]) > -50.0);
}
