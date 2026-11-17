//! S3: `shape` oversampling, the asym / diode / tube / table curves and `mix`,
//! and `compress.key`.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::{RenderOpts, SoundDoc};

fn tone(hz: f32, amp: f32) -> Vec<Value> {
    vec![
        json!({"type": "osc", "name": "tone0", "shape": "sine", "freq": hz}),
        json!({"type": "gain", "name": "tone", "input": "tone0", "gain": amp}),
    ]
}

fn with(mut nodes: Vec<Value>, node: Value) -> Vec<Value> {
    nodes.push(node);
    nodes
}

/// Off-grid energy relative to on-grid energy, dB, counting only bins up to
/// `top` Hz (the same rule as `common::alias_db`).
fn alias_db_to(x: &[f32], f0: f32, top: f32) -> f32 {
    let s = spectrum(x, SR);
    let tol = 8.0 * SR as f32 / x.len() as f32;
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (f, d) in &s {
        if *f < tol || *f > top {
            continue;
        }
        let p = 10f64.powf(f64::from(*d) / 10.0);
        let nearest = (f / f0).round().max(1.0) * f0;
        if (f - nearest).abs() <= tol {
            on += p;
        } else {
            off += p;
        }
    }
    (10.0 * (off.max(1e-30) / on.max(1e-30)).log10()) as f32
}

fn clip_alias(oversample: u32) -> f32 {
    let nodes = with(
        tone(5000.0, 1.0),
        json!({"type": "shape", "name": "s", "input": "tone", "mode": "clip", "drive": 8,
               "oversample": oversample}),
    );
    let x = node_audio(&nodes, "s", 1.0);
    assert_finite(&x, "s");
    // Skip the first samples (the oversampler fills its history).
    alias_db_to(&x[480..], 5000.0, 20_000.0)
}

#[test]
fn oversampling_cuts_the_aliasing_of_a_clipped_sine() {
    let base = clip_alias(1);
    let mut better = Vec::new();
    for (k, need) in [(2u32, 12.0f32), (4, 22.0), (8, 35.0)] {
        let less = base - clip_alias(k);
        println!("clip drive 8, 5 kHz: {k}x gives {less:.1} dB less off-grid energy below 20 kHz");
        assert!(less >= need, "{k}x: only {less:.1} dB less (need {need})");
        better.push(less);
    }
    println!("clip drive 8, 5 kHz, 1x: off-grid {base:.1} dB below the harmonics");
    assert!(better[0] < better[1] && better[1] < better[2]);
}

/// 2nd harmonic level in dB relative to the fundamental.
fn h2_dbc(nodes: &[Value], name: &str) -> f32 {
    let x = node_audio(nodes, name, 1.0);
    // Skip the output fades by looking at the node, not the output.
    band_db(&x, SR, 1950.0, 2050.0) - band_db(&x, SR, 950.0, 1050.0)
}

#[test]
fn asym_makes_a_second_harmonic_where_tanh_does_not() {
    let t = with(
        tone(1000.0, 0.5),
        json!({"type": "shape", "name": "t", "input": "tone", "mode": "tanh", "drive": 2}),
    );
    let a = with(
        tone(1000.0, 0.5),
        json!({"type": "shape", "name": "a", "input": "tone", "mode": "asym", "drive": 2}),
    );
    let (ht, ha) = (h2_dbc(&t, "t"), h2_dbc(&a, "a"));
    println!("1 kHz, drive 2: 2nd harmonic tanh {ht:.1} dBc, asym (bias 0.5) {ha:.1} dBc");
    assert!(ht < -70.0, "tanh {ht}");
    assert!(ha > -30.0, "asym {ha}");
    for mode in ["diode", "tube"] {
        let n = with(
            tone(1000.0, 0.5),
            json!({"type": "shape", "name": "m", "input": "tone", "mode": mode, "drive": 3}),
        );
        let h = h2_dbc(&n, "m");
        println!("1 kHz, drive 3: 2nd harmonic {mode} {h:.1} dBc");
        assert!(h > -40.0, "{mode} {h}");
    }
}

#[test]
fn asym_with_bias_zero_is_tanh() {
    let a = with(
        tone(300.0, 0.7),
        json!({"type": "shape", "name": "a", "input": "tone", "mode": "asym", "bias": 0, "drive": 3}),
    );
    let t = with(
        tone(300.0, 0.7),
        json!({"type": "shape", "name": "a", "input": "tone", "mode": "tanh", "drive": 3}),
    );
    assert_eq!(node_audio(&a, "a", 0.2), node_audio(&t, "a", 0.2));
}

#[test]
fn a_table_with_the_identity_returns_the_input_bit_for_bit() {
    let want = node_audio(tone(777.0, 0.9), "tone", 0.5);
    for pts in [json!([[-1, -1], [1, 1]]), json!([[0, 0], [1, 1]])] {
        let nodes = with(
            tone(777.0, 0.9),
            json!({"type": "shape", "name": "s", "input": "tone", "mode": "table", "points": pts}),
        );
        let got = node_audio(&nodes, "s", 0.5);
        assert_eq!(got.len(), want.len());
        let bad = got
            .iter()
            .zip(&want)
            .position(|(a, b)| a.to_bits() != b.to_bits());
        println!("identity table {pts}: first differing sample {bad:?}");
        assert!(bad.is_none(), "{pts}: sample {bad:?} differs");
    }
}

#[test]
fn a_table_interpolates_holds_its_ends_and_extends_oddly() {
    // x >= 0 only: odd-extended. Segments: 0..0.5 slope 0.5, 0.5..1 slope 3.5.
    let pts = [[0.0f64, 0.0], [0.5, 0.25], [1.0, 2.0]];
    let curve = |u: f64| -> f64 {
        let a = u.abs();
        let y = if a >= 1.0 {
            2.0
        } else if a <= 0.5 {
            a * 0.5
        } else {
            0.25 + (a - 0.5) * 3.5
        };
        y * u.signum()
    };
    assert_eq!(pts.len(), 3);
    let nodes = with(
        tone(100.0, 1.0),
        json!({"type": "shape", "name": "s", "input": "tone", "mode": "table", "drive": 1.5,
               "points": [[0, 0], [0.5, 0.25], [1, 2]]}),
    );
    let input = node_audio(
        with(
            tone(100.0, 1.0),
            json!({"type": "gain", "name": "g", "input": "tone", "gain": 1.5}),
        ),
        "g",
        0.2,
    );
    let got = node_audio(&nodes, "s", 0.2);
    let mut worst = 0.0f64;
    for (u, y) in input.iter().zip(&got) {
        worst = worst.max((curve(f64::from(*u)) - f64::from(*y)).abs());
    }
    println!("odd-extended 3-point table: worst error {worst:.2e}");
    assert!(worst < 1e-5, "{worst}");
    assert!(got.iter().any(|v| *v < -1.9) && got.iter().any(|v| *v > 1.9));
    assert!(got.iter().all(|v| v.abs() <= 2.0 + 1e-6));
}

#[test]
fn every_mode_stays_finite_at_the_extremes() {
    let modes = [
        "tanh", "clip", "fold", "bitcrush", "rectify", "asym", "diode", "tube", "table",
    ];
    for mode in modes {
        for drive in [0.0f64, 1e-9, 1.0, 100.0, 1e6, 1e30, -50.0] {
            for os in [1u32, 2, 4, 8] {
                let mut node = json!({"type": "shape", "name": "s", "input": "tone", "mode": mode,
                    "drive": drive, "oversample": os, "mix": 0.7});
                if mode == "table" {
                    node["points"] = json!([[-1, -0.5], [0, 0.1], [2, 3]]);
                }
                let x = node_audio(with(tone(3000.0, 1.0), node), "s", 0.05);
                assert_finite(&x, &format!("{mode} drive {drive} {os}x"));
            }
        }
    }
    // Extreme bias and noise input.
    let nodes = vec![
        json!({"type": "noise", "name": "n"}),
        json!({"type": "shape", "name": "s", "input": "n", "mode": "asym", "bias": 1e30, "drive": 1e20}),
    ];
    assert_finite(&node_audio(&nodes, "s", 0.05), "asym bias 1e30");
    assert_bit_identical(
        with(
            tone(900.0, 1.0),
            json!({"type": "shape", "name": "s", "input": "tone", "mode": "tube", "drive": 6,
                   "oversample": 4, "mix": 0.5}),
        ),
        0.2,
    );
}

#[test]
fn mix_blends_dry_and_wet() {
    let t = tone(500.0, 0.8);
    let shape = |m: f64| {
        with(
            t.clone(),
            json!({"type": "shape", "name": "s", "input": "tone", "mode": "tanh", "drive": 4, "mix": m}),
        )
    };
    let dry = node_audio(&t, "tone", 0.2);
    let wet = node_audio(shape(1.0), "s", 0.2);
    let none = node_audio(shape(0.0), "s", 0.2);
    let half = node_audio(shape(0.5), "s", 0.2);
    assert_eq!(none, dry, "mix 0 is the input, bit for bit");
    let mut worst = 0.0f32;
    for i in 0..dry.len() {
        worst = worst.max((half[i] - 0.5 * (dry[i] + wet[i])).abs());
    }
    println!("mix 0.5: worst error against (dry + wet) / 2 is {worst:.2e}");
    assert!(worst < 1e-6);
}

/// Error of a shape that is the identity (clip, small signal) against the
/// input, dB relative to the input rms, over the middle of the signal.
fn identity_error(hz: f32, oversample: u32, mix: f64) -> f32 {
    let nodes = with(
        tone(hz, 0.3),
        json!({"type": "shape", "name": "s", "input": "tone", "mode": "clip",
               "oversample": oversample, "mix": mix}),
    );
    let y = node_audio(&nodes, "s", 0.5);
    let x = node_audio(&nodes, "tone", 0.5);
    let err: Vec<f32> = x[2000..22000]
        .iter()
        .zip(&y[2000..22000])
        .map(|(a, b)| a - b)
        .collect();
    rms_db(&err) - rms_db(&x[2000..22000])
}

#[test]
fn oversampling_keeps_the_signal_in_time() {
    for os in [2u32, 4, 8] {
        for hz in [200.0f32, 2000.0, 8000.0] {
            let wet = identity_error(hz, os, 1.0);
            let blended = identity_error(hz, os, 0.5);
            println!(
                "{os}x, {hz} Hz through an identity curve: error {wet:.1} dB (mix 1), {blended:.1} dB (mix 0.5)"
            );
            // The delay is rounded to a whole sample at mix 1 (at most half a
            // sample), interpolated when blending.
            let half_sample = 20.0 * (std::f32::consts::PI * hz / SR as f32).sin().log10();
            assert!(
                wet < half_sample + 3.0,
                "{os}x {hz} Hz: {wet} vs half-sample bound {half_sample}"
            );
            // Third-order interpolation of a half-sample delay: better than
            // 80 dB at 2 kHz, about 38 dB at 8 kHz (a sixth of the rate).
            let need = if hz <= 2000.0 { -80.0 } else { -35.0 };
            assert!(blended < need, "{os}x {hz} Hz mix 0.5: {blended} dB");
        }
    }
}

#[test]
fn the_key_makes_the_compressor_listen_to_another_signal() {
    // A quiet steady tone (-26 dBFS, under the threshold), keyed by a 4 Hz
    // pulse at full scale: 20 dB over the -20 dB threshold at 4:1 is 15 dB.
    let mut nodes = tone(1000.0, 0.05);
    nodes.extend([
        json!({"type": "osc", "name": "sq", "shape": "square", "freq": 4}),
        json!({"type": "math", "name": "pulse", "op": "max", "a": "sq", "b": 0}),
        json!({"type": "compress", "name": "c", "input": "tone", "key": "pulse",
               "threshold_db": -20, "ratio": 4, "attack": 0.001, "release": 0.01, "knee_db": 0}),
        json!({"type": "compress", "name": "plain", "input": "tone",
               "threshold_db": -20, "ratio": 4, "attack": 0.001, "release": 0.01, "knee_db": 0}),
    ]);
    let key = node_audio(&nodes, "pulse", 2.0);
    let tone_x = node_audio(&nodes, "tone", 2.0);
    let c = node_audio(&nodes, "c", 2.0);
    let plain = node_audio(&nodes, "plain", 2.0);
    // Windows well inside each pulse and each gap, after the first second.
    let (mut on, mut off) = (Vec::new(), Vec::new());
    let mut i = SR as usize;
    while i + 960 < key.len() {
        let hi = key[i..i + 960].iter().all(|v| *v > 0.9);
        let lo = key[i - 480..i + 960 + 480].iter().all(|v| v.abs() < 1e-3);
        let gain_db = |w: std::ops::Range<usize>| rms_db(&c[w.clone()]) - rms_db(&tone_x[w]);
        if hi {
            on.push(gain_db(i..i + 960));
        } else if lo {
            off.push(gain_db(i..i + 960));
        }
        i += 960;
    }
    assert!(
        on.len() >= 2 && off.len() >= 2,
        "{} {}",
        on.len(),
        off.len()
    );
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let (g_on, g_off) = (mean(&on), mean(&off));
    let g_plain = rms_db(&plain[SR as usize..]) - rms_db(&tone_x[SR as usize..]);
    println!(
        "keyed compressor: gain {g_on:.2} dB in the pulses ({} windows), {g_off:.2} dB between; without the key {g_plain:.2} dB",
        on.len()
    );
    assert!((g_on - (-15.0)).abs() < 1.0, "{g_on}");
    assert!(g_off.abs() < 1.0, "{g_off}");
    assert!(g_plain.abs() < 0.05, "{g_plain}");
}

fn render_err(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("e");
    d.nodes = nodes;
    d.duration = Some(0.1);
    match d.render(&RenderOpts::default()) {
        Ok(_) => panic!("rendered, expected an error"),
        Err(e) => e.to_string(),
    }
}

fn shape_with(extra: Value) -> Vec<Value> {
    let mut node = json!({"type": "shape", "name": "s", "input": "tone"});
    for (k, v) in extra.as_object().unwrap() {
        node[k] = v.clone();
    }
    let mut n = tone(440.0, 0.5);
    n.push(node);
    n.push(json!({"type": "output", "name": "o", "input": "s"}));
    n
}

#[test]
fn errors_name_the_field_and_the_fix() {
    let e = render_err(shape_with(json!({"oversample": 3})));
    println!("oversample 3: {e}");
    assert!(
        e.contains("oversample") && e.contains("1, 2, 4 or 8"),
        "{e}"
    );
    let e = render_err(shape_with(json!({"mode": "table"})));
    println!("table without points: {e}");
    assert!(e.contains("points") && e.contains("needs points"), "{e}");
    let e = render_err(shape_with(
        json!({"mode": "table", "points": [[0, 0], [1, 1], [0.5, 2]]}),
    ));
    println!("unsorted points: {e}");
    assert!(e.contains("points") && e.contains("increasing"), "{e}");
    let e = render_err(shape_with(
        json!({"mode": "table", "points": [[0, 0.2], [1, 1]]}),
    ));
    println!("odd table with y(0) != 0: {e}");
    assert!(e.contains("x = 0 needs y = 0"), "{e}");
    let e = render_err(shape_with(json!({"mode": "table", "points": [[0, 0]]})));
    println!("one point at zero: {e}");
    assert!(e.contains("at least two"), "{e}");
    let e = render_err(shape_with(json!({"points": [[0, 0], [1, 1]]})));
    println!("points on tanh: {e}");
    assert!(e.contains("points") && e.contains("table"), "{e}");
    let e = render_err(shape_with(json!({"bias": 0.2})));
    assert!(e.contains("bias") && e.contains("asym"), "{e}");
    let many: Vec<Value> = (0..5000).map(|i| json!([i as f64, i as f64])).collect();
    let e = render_err(shape_with(json!({"mode": "table", "points": many})));
    println!("5000 points: {e}");
    assert!(e.contains("at most 4096"), "{e}");
}

/// A feedback loop: a noise burst into a delay whose return goes through
/// `shape_node` and a gain.
fn loop_with(shape_node: Value) -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n"}),
        json!({"type": "decay", "name": "e", "time": 0.004}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "mix", "name": "m", "inputs": ["burst", "g"]}),
        json!({"type": "delay", "name": "d", "input": "m", "time": 0.00227}),
        shape_node,
        json!({"type": "gain", "name": "g", "input": "s", "gain": 0.95}),
        json!({"type": "onepole", "name": "hp", "input": "g", "mode": "hp", "freq": 30}),
    ]
}

#[test]
fn inside_a_feedback_loop_oversampling_is_refused_and_the_new_modes_work() {
    let e = render_err(loop_with(
        json!({"type": "shape", "name": "s", "input": "d", "oversample": 2}),
    ));
    println!("oversample in a loop: {e}");
    assert!(
        e.contains("oversample") && e.contains("feedback loop"),
        "{e}"
    );
    for (mode, extra) in [
        ("asym", json!({})),
        ("diode", json!({})),
        ("tube", json!({})),
        ("table", json!({"points": [[0, 0], [0.3, 0.6], [1, 0.9]]})),
    ] {
        let mut node = json!({"type": "shape", "name": "s", "input": "d", "mode": mode,
                              "drive": 2, "mix": 0.8, "oversample": 1});
        for (k, v) in extra.as_object().unwrap() {
            node[k] = v.clone();
        }
        let x = node_audio(loop_with(node), "hp", 0.2);
        assert_finite(&x, mode);
        println!("loop with {mode}: rms {:.1} dB", rms_db(&x));
        assert!(rms_db(&x) > -80.0, "{mode} is silent");
    }
}
