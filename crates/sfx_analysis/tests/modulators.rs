//! S9: `chaos`, `fbm`, `curve`, `switch`, `pitch_follow`, `centroid_follow`.
//! Each test measures and prints (these lines go in docs/nodes/S9.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::checks::clicks;
use sfx_graph::node::parse_body;
use sfx_graph::{RenderOpts, SoundDoc};

fn render_error(nodes: Vec<Value>, read: &str) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": read}));
    d.duration = Some(0.2);
    d.render(&RenderOpts::default()).unwrap_err()
}

fn minmax(x: &[f32]) -> (f32, f32) {
    x.iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)))
}

fn std_dev(x: &[f32]) -> f32 {
    let m = x.iter().map(|v| f64::from(*v)).sum::<f64>() / x.len() as f64;
    (x.iter().map(|v| (f64::from(*v) - m).powi(2)).sum::<f64>() / x.len() as f64).sqrt() as f32
}

/// Spectral density (dB per Hz) of a control signal between `lo` and `hi`.
fn density(x: &[f32], lo: f32, hi: f32) -> f32 {
    band_db(x, SR, lo, hi) - 10.0 * (hi - lo).log10()
}

// -------------------------------------------------------------------- fbm

fn fbm(extra: Value) -> Vec<Value> {
    let mut n = json!({"type": "fbm", "name": "f", "rate": 2, "octaves": 6, "range": [-1, 1]});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![n]
}

#[test]
fn fbm_spectrum_falls_and_stays_in_range() {
    let x = node_audio(fbm(json!({"range": [100, 300], "seed": 1})), "f", 20.0);
    assert_finite(&x, "fbm");
    let (lo, hi) = minmax(&x);
    // Remove the mean (the range centre) before the spectrum.
    let m = x.iter().sum::<f32>() / x.len() as f32;
    let c: Vec<f32> = x.iter().map(|v| v - m).collect();
    let bands = [
        (0.2, 1.0),
        (1.0, 4.0),
        (4.0, 16.0),
        (16.0, 64.0),
        (64.0, 256.0),
    ];
    let d: Vec<f32> = bands.iter().map(|(a, b)| density(&c, *a, *b)).collect();
    println!(
        "fbm rate 2, 6 octaves, range 100..300: min {lo:.2} max {hi:.2}; density dB/Hz per band {bands:?}: {d:.1?}"
    );
    assert!(lo >= 100.0 && hi <= 300.0, "{lo} {hi}");
    assert!(hi - lo > 50.0, "it moves: {lo}..{hi}");
    for w in d.windows(2) {
        assert!(w[0] > w[1] + 3.0, "spectrum falls: {d:?}");
    }
}

#[test]
fn fbm_seeds() {
    let a = node_audio(fbm(json!({"seed": 1})), "f", 2.0);
    let b = node_audio(fbm(json!({"seed": 2})), "f", 2.0);
    let a2 = node_audio(fbm(json!({"seed": 1})), "f", 2.0);
    let diff = a
        .iter()
        .zip(&b)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0f32, f32::max);
    println!(
        "fbm seeds 1 vs 2: max difference {diff:.3}; seed 1 twice identical: {}",
        a == a2
    );
    assert!(diff > 0.2);
    assert!(a.iter().zip(&a2).all(|(p, q)| p.to_bits() == q.to_bits()));
}

#[test]
fn fbm_octaves_and_gain_shape_it() {
    // Octave 1 only is smoother than 6 octaves at gain 1: measure by the
    // mean absolute step.
    let step = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32;
    let one = node_audio(fbm(json!({"octaves": 1})), "f", 4.0);
    let six = node_audio(fbm(json!({"octaves": 6, "gain": 1.0})), "f", 4.0);
    println!(
        "mean |step|: 1 octave {:.2e}, 6 octaves gain 1 {:.2e}",
        step(&one),
        step(&six)
    );
    assert!(step(&six) > 5.0 * step(&one));
}

// ------------------------------------------------------------------ chaos

fn chaos(model: &str, rate: f64, extra: Value) -> Vec<Value> {
    let mut n =
        json!({"type": "chaos", "name": "c", "model": model, "rate": rate, "range": [-3, 5]});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![n]
}

#[test]
fn chaos_lorenz_stays_in_range_for_30_s_at_the_rate_cap() {
    for rate in [0.5, 10.0, 200.0, 2000.0] {
        let x = node_audio(chaos("lorenz", rate, json!({})), "c", 30.0);
        assert_finite(&x, "lorenz");
        let (lo, hi) = minmax(&x);
        println!(
            "lorenz rate {rate}, 30 s: min {lo:.3} max {hi:.3}, std {:.3}",
            std_dev(&x)
        );
        assert!(lo >= -3.0 && hi <= 5.0, "rate {rate}: {lo} {hi}");
        assert!(std_dev(&x) > 0.3, "it moves at rate {rate}");
    }
}

#[test]
fn chaos_maps_are_bounded_and_seeded() {
    for (model, rate) in [
        ("logistic", 30.0),
        ("logistic", 48_000.0),
        ("henon", 100.0),
        ("henon", 48_000.0),
    ] {
        let x = node_audio(chaos(model, rate, json!({})), "c", 10.0);
        assert_finite(&x, model);
        let (lo, hi) = minmax(&x);
        println!(
            "{model} rate {rate}: min {lo:.3} max {hi:.3}, std {:.3}",
            std_dev(&x)
        );
        assert!(lo >= -3.0 && hi <= 5.0);
        assert!(std_dev(&x) > 0.3);
    }
    let a = node_audio(chaos("logistic", 50.0, json!({"seed": 1})), "c", 2.0);
    let b = node_audio(chaos("logistic", 50.0, json!({"seed": 2})), "c", 2.0);
    let a2 = node_audio(chaos("logistic", 50.0, json!({"seed": 1})), "c", 2.0);
    let diff = a
        .iter()
        .zip(&b)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0f32, f32::max);
    println!("logistic seeds 1 vs 2: max difference {diff:.3}");
    assert!(diff > 0.5 && a == a2);
    // Extreme parameters stay finite and inside the range.
    for (model, param) in [
        ("logistic", 0.0),
        ("logistic", 4.0),
        ("logistic", 2.0),
        ("henon", 0.0),
        ("henon", 1.5),
        ("lorenz", 0.0),
        ("lorenz", 100.0),
    ] {
        let rate = if model == "lorenz" { 2000.0 } else { 48_000.0 };
        let x = node_audio(chaos(model, rate, json!({"param": param})), "c", 3.0);
        assert_finite(&x, model);
        let (lo, hi) = minmax(&x);
        assert!(lo >= -3.0 && hi <= 5.0, "{model} param {param}: {lo} {hi}");
    }
    println!(
        "param extremes (logistic 0/2/4, henon 0/1.5, lorenz 0/100 at the rate cap): finite, inside range"
    );
}

// ------------------------------------------------------------------ curve

#[test]
fn curve_hits_each_breakpoint_exactly() {
    let pts = [
        [-1.0f32, 0.0],
        [-0.25, 0.8],
        [0.0, 0.2],
        [0.5, -0.7],
        [1.0, 1.0],
    ];
    for curve in ["linear", "smooth"] {
        let mut worst = 0.0f32;
        for p in pts {
            let nodes = vec![
                json!({"type": "curve", "name": "c", "input": p[0], "points": pts, "curve": curve}),
            ];
            let x = node_audio(nodes, "c", 0.01);
            worst = worst.max((x[100] - p[1]).abs());
            assert_eq!(x[100].to_bits(), p[1].to_bits(), "{curve} at x {}", p[0]);
        }
        println!("curve {curve}: every breakpoint exact (worst error {worst})");
    }
    // Between points, and clamped outside.
    let at = |x: f32, curve: &str| {
        node_audio(
            vec![json!({"type": "curve", "name": "c", "input": x, "points": pts, "curve": curve})],
            "c",
            0.01,
        )[10]
    };
    println!(
        "linear at -0.625: {}, smooth at -0.625: {}, below {}, above {}",
        at(-0.625, "linear"),
        at(-0.625, "smooth"),
        at(-9.0, "linear"),
        at(9.0, "linear")
    );
    assert!((at(-0.625, "linear") - 0.4).abs() < 1e-6);
    assert!((at(-0.625, "smooth") - 0.4).abs() < 1e-6);
    assert!((at(0.25, "linear") - (-0.25)).abs() < 1e-6);
    assert_eq!(at(-9.0, "linear"), 0.0);
    assert_eq!(at(9.0, "smooth"), 1.0);
    // Used as a signal shaper: a ramp through it follows the table.
    let x = node_audio(
        vec![
            json!({"type": "ramp", "name": "r", "from": -1, "to": 1, "time": 1.0, "curve": "linear"}),
            json!({"type": "curve", "name": "c", "input": "r", "points": pts}),
        ],
        "c",
        1.0,
    );
    assert_finite(&x, "curve");
    assert!((x[SR as usize / 2] - 0.2).abs() < 1e-3);
}

// ----------------------------------------------------------------- switch

#[test]
fn switch_between_two_sines_does_not_click() {
    // select crosses 0.5 at 0.4123 s, a time where the two sines differ.
    let nodes = |fade: f64| {
        vec![
            json!({"type": "osc", "name": "lo", "freq": 200, "amp": 0.5}),
            json!({"type": "osc", "name": "hi", "freq": 800, "amp": 0.5}),
            json!({"type": "ramp", "name": "sel", "from": 0, "to": 1, "time": 0.8246, "curve": "linear"}),
            json!({"type": "switch", "name": "s", "inputs": ["lo", "hi"], "select": "sel", "fade": fade}),
        ]
    };
    let max_step = |x: &[f32]| {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    };
    let faded = node_audio(nodes(0.005), "s", 1.0);
    let hard = node_audio(nodes(0.0), "s", 1.0);
    assert_finite(&faded, "switch");
    let (cf, ch) = (clicks(&faded, SR), clicks(&hard, SR));
    println!(
        "switch 200 -> 800 Hz at 0.4123 s: fade 0.005 clicks {}, max step {:.4}; fade 0 clicks {}, max step {:.4}",
        cf.len(),
        max_step(&faded),
        ch.len(),
        max_step(&hard)
    );
    assert!(cf.is_empty(), "no click with a fade: {cf:?}");
    assert!(max_step(&hard) > 2.0 * max_step(&faded));
    // Before the switch it is the 200 Hz sine, after it the 800 Hz one.
    let (a, b) = (&faded[..SR as usize / 3], &faded[SR as usize / 2..]);
    println!(
        "before {:.1} Hz, after {:.1} Hz",
        peak_hz(a, SR),
        peak_hz(b, SR)
    );
    assert!((peak_hz(a, SR) - 200.0).abs() < 3.0 && (peak_hz(b, SR) - 800.0).abs() < 3.0);
    // The fade is linear and sums to one: at the midpoint both are present,
    // and away from it the output is the input bit for bit.
    let lo = node_audio(
        vec![json!({"type": "osc", "name": "lo", "freq": 200, "amp": 0.5})],
        "lo",
        1.0,
    );
    assert_eq!(faded[1000].to_bits(), lo[1000].to_bits());
}

#[test]
fn switch_blend_and_stereo() {
    let nodes = vec![
        json!({"type": "gain", "name": "one", "input": 1.0, "gain": 1.0}),
        json!({"type": "gain", "name": "three", "input": 3.0, "gain": 1.0}),
        json!({"type": "gain", "name": "five", "input": 5.0, "gain": 1.0}),
        json!({"type": "ramp", "name": "sel", "from": 0, "to": 2, "time": 1.0, "curve": "linear"}),
        json!({"type": "switch", "name": "s", "inputs": ["one", "three", "five"], "select": "sel", "fade": 0, "blend": true}),
    ];
    let x = node_audio(nodes, "s", 1.0);
    let (a, b, c) = (x[0], x[SR as usize / 2], x[SR as usize * 3 / 4]);
    println!(
        "blend select 0 -> 2 over 1 s: at 0 {a:.3}, at 0.5 s {b:.3} (expect 3), at 0.75 s {c:.3} (expect 4)"
    );
    assert!((a - 1.0).abs() < 1e-3 && (b - 3.0).abs() < 1e-3 && (c - 4.0).abs() < 1e-3);
    // One stereo input makes the output stereo, mono inputs on both sides.
    let st = vec![
        json!({"type": "osc", "name": "m", "freq": 300, "amp": 0.3}),
        json!({"type": "pan", "name": "p", "input": "m", "pan": -1.0}),
        json!({"type": "switch", "name": "s", "inputs": ["m", "p"], "select": 1}),
    ];
    let a = render(&st, 0.2);
    assert_eq!(a.channels.len(), 2);
    println!(
        "stereo switch: left rms {:.1} dB, right rms {:.1} dB",
        rms_db(&a.channels[0]),
        rms_db(&a.channels[1])
    );
    assert!(rms_db(&a.channels[0]) > rms_db(&a.channels[1]) + 20.0);
}

// ------------------------------------------------------------ pitch_follow

#[test]
fn pitch_follow_tracks_an_exponential_sweep() {
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 200, "to": 800, "time": 1.0, "curve": "exp"}),
        json!({"type": "osc", "name": "tone", "freq": "f", "amp": 0.5}),
        json!({"type": "pitch_follow", "name": "p", "input": "tone", "smooth": 0.02}),
    ];
    let p = node_audio(&nodes, "p", 1.0);
    assert_finite(&p, "pitch_follow");
    let mut worst = 0.0f32;
    let mut worst_t = 0.0;
    // The frequency the ramp is at, from 50 ms on to the last sample.
    for i in (SR as usize / 20..SR as usize).step_by(100) {
        let t = i as f32 / SR as f32;
        let want = 200.0 * 4.0f32.powf(t);
        let err = (p[i] / want - 1.0).abs();
        if err > worst {
            worst = err;
            worst_t = t;
        }
    }
    for t in [
        0.02f32, 0.05, 0.1, 0.2, 0.4, 0.6, 0.8, 0.9, 0.95, 0.98, 0.999,
    ] {
        let i = (t * SR as f32) as usize;
        println!(
            "  t {t}: {:.2} %",
            (p[i] / (200.0 * 4.0f32.powf(t)) - 1.0) * 100.0
        );
    }
    println!(
        "pitch_follow on a 200 -> 800 Hz exponential sweep: worst error {:.2} % at t = {worst_t:.3} s (50 ms .. 1 s); at the end {:.1} Hz",
        worst * 100.0,
        p[SR as usize - 1]
    );
    assert!(worst < 0.02, "worst {worst} at {worst_t}");
}

#[test]
fn pitch_follow_steady_tone_holds_through_silence() {
    // 300 Hz for 0.3 s, then silence: the track holds 300 Hz.
    let nodes = vec![
        json!({"type": "osc", "name": "tone", "freq": 300, "amp": 0.5}),
        json!({"type": "env", "name": "gate", "points": [[0.0, 1.0], [0.3, 1.0], [0.3001, 0.0], [1.0, 0.0]]}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "tone", "b": "gate"}),
        json!({"type": "pitch_follow", "name": "p", "input": "burst"}),
    ];
    let p = node_audio(&nodes, "p", 1.0);
    let (a, b) = (p[SR as usize / 10], p[SR as usize * 9 / 10]);
    println!("tone then silence: 0.1 s {a:.2} Hz, 0.9 s {b:.2} Hz (held)");
    assert!((a - 300.0).abs() < 3.0 && (b - 300.0).abs() < 3.0);
    // Noise is never confident: the track is 0.
    let nodes = vec![
        json!({"type": "noise", "name": "n"}),
        json!({"type": "pitch_follow", "name": "p", "input": "n"}),
    ];
    let p = node_audio(&nodes, "p", 0.5);
    let big = p.iter().filter(|v| **v != 0.0).count();
    println!("white noise: {big} of {} samples non-zero", p.len());
    assert!(big < p.len() / 10);
}

// --------------------------------------------------------- centroid_follow

#[test]
fn centroid_follow_reads_tones_noise_and_a_sweep() {
    let tone = |hz: f64| {
        vec![
            json!({"type": "osc", "name": "tone", "freq": hz, "amp": 0.5}),
            json!({"type": "centroid_follow", "name": "c", "input": "tone"}),
        ]
    };
    for hz in [500.0, 1000.0, 4000.0] {
        let c = node_audio(tone(hz), "c", 0.5);
        let mid = c[SR as usize / 4];
        println!(
            "sine {hz} Hz: centroid {mid:.3} Hz ({:+.4} %)",
            (mid / hz as f32 - 1.0) * 100.0
        );
        assert!((mid / hz as f32 - 1.0).abs() < 0.02);
    }
    // White noise: centroid near a quarter of the sample rate (12 kHz).
    let nodes = vec![
        json!({"type": "noise", "name": "n"}),
        json!({"type": "centroid_follow", "name": "c", "input": "n"}),
    ];
    let c = node_audio(nodes, "c", 0.5);
    println!("white noise: centroid {:.0} Hz", c[SR as usize / 4]);
    assert!((c[SR as usize / 4] - 12_000.0).abs() < 600.0);
    // A 200 -> 3200 Hz exponential sweep: centroid follows within 5 %.
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 200, "to": 3200, "time": 1.0, "curve": "exp"}),
        json!({"type": "osc", "name": "tone", "freq": "f", "amp": 0.5}),
        json!({"type": "centroid_follow", "name": "c", "input": "tone"}),
    ];
    let c = node_audio(nodes, "c", 1.0);
    let mut worst = 0.0f32;
    for i in (SR as usize / 20..SR as usize * 19 / 20).step_by(200) {
        let t = i as f32 / SR as f32;
        worst = worst.max((c[i] / (200.0 * 16.0f32.powf(t)) - 1.0).abs());
    }
    println!(
        "sweep 200 -> 3200 Hz: worst centroid error {:.4} %",
        worst * 100.0
    );
    assert!(worst < 0.05);
    // Silence: 0 (never any sound).
    let nodes = vec![
        json!({"type": "osc", "name": "tone", "freq": 440, "amp": 0.0}),
        json!({"type": "centroid_follow", "name": "c", "input": "tone"}),
    ];
    assert!(node_audio(nodes, "c", 0.2).iter().all(|v| *v == 0.0));
}

// ----------------------------------------------------------- all together

fn all_nodes() -> Vec<Value> {
    vec![
        json!({"type": "osc", "name": "src", "freq": 330, "amp": 0.5}),
        json!({"type": "chaos", "name": "ch", "model": "lorenz", "rate": 3, "range": [0, 1]}),
        json!({"type": "chaos", "name": "lg", "model": "logistic", "rate": 40}),
        json!({"type": "chaos", "name": "hn", "model": "henon", "rate": 40}),
        json!({"type": "fbm", "name": "fm", "rate": 3, "range": [200, 600]}),
        json!({"type": "curve", "name": "cv", "input": "ch", "points": [[0, 100], [1, 900]], "curve": "smooth"}),
        json!({"type": "switch", "name": "sw", "inputs": ["src", "lg"], "select": "ch", "blend": true}),
        json!({"type": "pitch_follow", "name": "pf", "input": "src"}),
        json!({"type": "centroid_follow", "name": "cf", "input": "src"}),
        json!({"type": "osc", "name": "mix1", "freq": "fm", "amp": 0.2}),
        json!({"type": "math", "name": "sum", "op": "add", "a": "mix1", "b": "hn"}),
    ]
}

#[test]
fn modulators_are_bit_identical() {
    assert_bit_identical(all_nodes(), 0.6);
}

#[test]
fn modulators_are_finite_at_the_extremes() {
    let mut nodes = vec![
        json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6}),
        json!({"type": "ramp", "name": "wild", "from": -1.0e9, "to": 1.0e9, "time": 0.2}),
    ];
    nodes.extend([
        json!({"type": "chaos", "name": "a", "model": "lorenz", "rate": 2000, "param": 100}),
        json!({"type": "chaos", "name": "b", "model": "henon", "rate": 48000, "param": 1.5}),
        json!({"type": "fbm", "name": "c", "rate": 48000, "octaves": 1, "gain": 1}),
        json!({"type": "fbm", "name": "d", "rate": 0.001, "octaves": 12}),
        json!({"type": "curve", "name": "e", "input": "wild", "points": [[-1.0e30, -5], [1.0e30, 5]]}),
        json!({"type": "curve", "name": "e2", "input": "wild", "points": [[0, 1], [1, 2]], "curve": "smooth"}),
        json!({"type": "switch", "name": "f", "inputs": ["src", "src", "src"], "select": "wild", "fade": 0}),
        json!({"type": "switch", "name": "g", "inputs": ["src", "wild"], "select": "wild", "blend": true}),
        json!({"type": "pitch_follow", "name": "h", "input": "src", "min": 20, "max": 12000, "smooth": 10}),
        json!({"type": "pitch_follow", "name": "i", "input": "wild", "smooth": 0}),
        json!({"type": "centroid_follow", "name": "j", "input": "src", "window": 0.0007}),
        json!({"type": "centroid_follow", "name": "k", "input": "wild", "window": 0.6}),
    ]);
    for name in ["a", "b", "c", "d", "e", "e2", "f", "g", "h", "i", "j", "k"] {
        let x = node_audio(&nodes, name, 0.7);
        assert_finite(&x, name);
        assert_eq!(x.len(), (0.7 * SR as f32).round() as usize, "{name} length");
    }
    println!(
        "extremes: every node at the ends of each field, input 1e6 and a +-1e9 sweep: all finite, document length"
    );
}

#[test]
fn modulators_errors_say_the_field_value_and_fix() {
    let cases: Vec<(Vec<Value>, &str, Vec<&str>)> = vec![
        (
            vec![json!({"type": "chaos", "name": "c", "rate": 99999})],
            "c",
            vec!["rate: 99999", "a step per sample at most"],
        ),
        (
            vec![json!({"type": "chaos", "name": "c", "model": "lorenz", "rate": 3000})],
            "c",
            vec!["rate: 3000", "lorenz integrates at most 2000"],
        ),
        (
            vec![json!({"type": "chaos", "name": "c", "rate": 10, "param": 5})],
            "c",
            vec!["param: 5", "0 to 4"],
        ),
        (
            vec![json!({"type": "fbm", "name": "c", "rate": 40000, "octaves": 4})],
            "c",
            vec!["rate: 40000", "lower rate or octaves"],
        ),
        (
            vec![json!({"type": "fbm", "name": "c", "rate": 2, "octaves": 13})],
            "c",
            vec!["octaves: 13", "1 to 12"],
        ),
        (
            vec![json!({"type": "curve", "name": "c", "input": 0.5, "points": [[0, 0], [0, 1]]})],
            "c",
            vec!["points[1]", "sort the points by x"],
        ),
        (
            vec![json!({"type": "curve", "name": "c", "input": 0.5, "points": [[0, 0]]})],
            "c",
            vec!["points: 1 pairs", "give 2 to 4096"],
        ),
        (
            vec![
                json!({"type": "osc", "name": "a", "freq": 1}),
                json!({"type": "switch", "name": "c", "inputs": ["a"], "select": 0}),
            ],
            "c",
            vec!["inputs: 1 inputs", "give 2 to 64"],
        ),
        (
            vec![
                json!({"type": "osc", "name": "a", "freq": 1}),
                json!({"type": "pitch_follow", "name": "c", "input": "a", "min": 10}),
            ],
            "c",
            vec!["min: 10", "20 Hz or more"],
        ),
        (
            vec![
                json!({"type": "osc", "name": "a", "freq": 1}),
                json!({"type": "pitch_follow", "name": "c", "input": "a", "max": 20000}),
            ],
            "c",
            vec!["max: 20000", "a quarter of the sample rate"],
        ),
        (
            vec![
                json!({"type": "osc", "name": "a", "freq": 1}),
                json!({"type": "centroid_follow", "name": "c", "input": "a", "window": 5}),
            ],
            "c",
            vec!["window: 5", "32768 samples"],
        ),
        (
            vec![
                json!({"type": "osc", "name": "a", "freq": 1}),
                json!({"type": "chaos", "name": "c", "rate": 2, "model": "rossler"}),
            ],
            "c",
            vec!["rossler", "logistic"],
        ),
    ];
    for (nodes, read, wants) in cases {
        let e = render_error(nodes, read);
        println!("{e}");
        for w in wants {
            assert!(e.contains(w), "`{w}` missing from: {e}");
        }
    }
}

#[test]
fn modulators_are_registered_with_their_flags() {
    for (ty, causal, random) in [
        ("chaos", true, true),
        ("fbm", true, true),
        ("curve", true, false),
        ("switch", true, false),
        ("pitch_follow", false, false),
        ("centroid_follow", false, false),
    ] {
        let ex = sfx_graph::example(ty).expect("registered");
        let mut body = ex.clone();
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, "x").unwrap();
        assert_eq!(
            (node.causal(), node.random()),
            (causal, random),
            "{ty}: causal / random"
        );
        let text = sfx_graph::describe(Some(ty)).unwrap();
        assert!(text.contains(ty));
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
}

// ----------------------------------------------------------------- recipe

/// The recipe in docs/nodes/S9.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "fbm", "name": "drift", "rate": 1.5, "octaves": 4, "range": [-1, 1], "seed": 5}),
        json!({"type": "chaos", "name": "wobble", "model": "lorenz", "rate": 6, "range": [0, 1], "seed": 2}),
        json!({"type": "curve", "name": "pitch", "input": "wobble", "points": [[0, 180], [0.5, 240], [1, 520]], "curve": "smooth"}),
        json!({"type": "math", "name": "bend", "op": "mul", "a": "drift", "b": 30}),
        json!({"type": "math", "name": "hz", "op": "add", "a": "pitch", "b": "bend"}),
        json!({"type": "osc", "name": "voice", "shape": "saw", "freq": "hz", "amp": 0.4}),
        json!({"type": "osc", "name": "air", "freq": 1100, "amp": 0.2}),
        json!({"type": "pitch_follow", "name": "heard", "input": "voice", "min": 100, "max": 800}),
        json!({"type": "ramp", "name": "pick", "from": 0, "to": 1, "time": 1.0, "curve": "linear"}),
        json!({"type": "switch", "name": "choose", "inputs": ["voice", "air"], "select": "pick", "fade": 0.02}),
    ]
}

#[test]
fn modulators_recipe_renders() {
    let hz = node_audio(recipe(), "hz", 1.0);
    let heard = node_audio(recipe(), "heard", 1.0);
    let out = node_audio(recipe(), "choose", 1.0);
    assert_finite(&out, "choose");
    // pitch_follow reads the voice's pitch (saw: first harmonic).
    let mut errs: Vec<f32> = (SR as usize / 10..SR as usize * 9 / 10)
        .step_by(500)
        .map(|i| (heard[i] / hz[i] - 1.0).abs())
        .collect();
    errs.sort_by(f32::total_cmp);
    let worst = errs[errs.len() / 2];
    let (lo, hi) = minmax(&hz);
    println!(
        "recipe: voice hz {lo:.0}..{hi:.0}, pitch_follow median error {:.1} % (0.1 .. 0.9 s), output rms {:.1} dB",
        worst * 100.0,
        rms_db(&out)
    );
    assert!(rms_db(&out) > -40.0);
    assert!(worst < 0.05, "{worst}");
}
