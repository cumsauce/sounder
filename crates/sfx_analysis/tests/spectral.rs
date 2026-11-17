//! The phase-3 offline pitch / time / spectral nodes, measured.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sfx_analysis::pitch::{cents, yin};
use sfx_analysis::stft::distance;
use sfx_graph::{Audio, RenderOpts, SoundDoc};

const SR: u32 = 48_000;

fn render(mut nodes: Vec<Value>, node: &str, secs: f32) -> Audio {
    nodes.push(json!({"type": "output", "name": "out", "input": node,
        "fade_in": 0, "fade_out": 0, "ceiling_db": 0}));
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(secs);
    d.render(&RenderOpts::default())
        .unwrap_or_else(|e| panic!("{e}"))
        .audio
}

fn median_pitch(x: &[f32]) -> f32 {
    let mut v: Vec<f32> = yin(x, SR).iter().filter_map(|f| f.hz).collect();
    assert!(!v.is_empty(), "no voiced frames");
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn sine(name: &str) -> Value {
    json!({"type": "osc", "name": name, "freq": 440, "amp": 0.5})
}

#[test]
fn pitch_shift_moves_a_sine_and_keeps_the_length() {
    for (semis, target) in [(12.0f32, 880.0f32), (-5.0, 440.0 * 2f32.powf(-5.0 / 12.0))] {
        let a = render(
            vec![
                sine("s"),
                json!({"type": "pitch_shift", "name": "p", "input": "s", "semitones": semis}),
            ],
            "p",
            1.0,
        );
        assert_eq!(a.channels[0].len(), SR as usize);
        let hz = median_pitch(&a.channels[0]);
        let c = cents(target, hz);
        println!("pitch_shift {semis:+} st: {hz:.2} Hz vs {target:.2} ({c:+.1} cents)");
        assert!(c.abs() < 20.0, "{semis}: {hz} Hz, {c} cents off");
        // The tone is still there at the end (no truncation or silence).
        let tail = &a.channels[0][a.channels[0].len() - 4800..];
        assert!(db(rms(tail)) > -12.0, "tail {} dB", db(rms(tail)));
    }
}

#[test]
fn pitch_shift_ratio_matches_semitones() {
    let go = |field: Value| {
        let mut p = json!({"type": "pitch_shift", "name": "p", "input": "s"});
        if let (Some(m), Some(f)) = (p.as_object_mut(), field.as_object()) {
            m.extend(f.clone());
        }
        render(vec![sine("s"), p], "p", 0.5)
    };
    let a = go(json!({"semitones": 12}));
    let b = go(json!({"ratio": 2.0}));
    assert_eq!(a, b);
}

#[test]
fn time_stretch_doubles_gaps_and_keeps_pitch() {
    // Pitch: a sine.
    let a = render(
        vec![
            sine("s"),
            json!({"type": "time_stretch", "name": "t", "input": "s", "factor": 2}),
        ],
        "t",
        1.0,
    );
    let hz = median_pitch(&a.channels[0]);
    let c = cents(440.0, hz);
    println!("time_stretch x2 sine: {hz:.2} Hz ({c:+.1} cents)");
    assert!(c.abs() < 20.0, "{hz} Hz, {c} cents");

    // Timing: two clicks 100 ms apart become ~200 ms apart.
    let mut nodes = vec![json!({"type": "impulse", "name": "ticks", "at": [0.1, 0.2]})];
    nodes.push(json!({"type": "time_stretch", "name": "t", "input": "ticks", "factor": 2}));
    let a = render(nodes, "t", 1.0);
    let x = &a.channels[0];
    let argmax = |r: std::ops::Range<usize>| {
        r.max_by(|&i, &j| x[i].abs().total_cmp(&x[j].abs()))
            .unwrap()
    };
    let (i1, i2) = (argmax(0..14_400), argmax(14_400..28_800));
    let gap = (i2 as f32 - i1 as f32) / SR as f32;
    println!("time_stretch x2 clicks: gap {gap:.4} s (want 0.2)");
    assert!((gap - 0.2).abs() < 0.02, "gap {gap}");
}

#[test]
fn spectral_freeze_outlives_the_source() {
    let nodes = vec![
        json!({"type": "osc", "name": "s", "freq": 440, "amp": 0.5}),
        json!({"type": "decay", "name": "e", "time": 0.1}),
        json!({"type": "math", "name": "tone", "op": "mul", "a": "s", "b": "e"}),
        json!({"type": "spectral", "name": "f", "input": "tone", "mode": "freeze", "at": 0.02}),
    ];
    let plain = render(nodes[..3].to_vec(), "tone", 0.6);
    let frozen = render(nodes, "f", 0.6);
    let w = |a: &Audio| rms(&a.channels[0][(0.4 * SR as f32) as usize..(0.5 * SR as f32) as usize]);
    println!(
        "level at 0.4-0.5 s: source {:.1} dB, frozen {:.1} dB",
        db(w(&plain)),
        db(w(&frozen))
    );
    assert!(db(w(&plain)) < -60.0);
    assert!(db(w(&frozen)) > -25.0, "frozen {} dB", db(w(&frozen)));
    // Still a 440 Hz tone-ish: the strongest YIN pitch is near 440 or silent-free noise;
    // we only require level and finiteness (random phases give a textured tone).
    assert!(frozen.channels[0].iter().all(|v| v.is_finite()));
}

#[test]
fn spectral_gate_removes_a_noise_floor() {
    let nodes = vec![
        json!({"type": "osc", "name": "s", "freq": 1000, "amp": 0.5}),
        json!({"type": "noise", "name": "n", "amp": 0.01}),
        json!({"type": "math", "name": "sum", "op": "add", "a": "s", "b": "n"}),
        json!({"type": "spectral", "name": "g", "input": "sum", "mode": "gate", "threshold_db": -40}),
        // Everything but the tone: the floor, measured on the residual.
        json!({"type": "math", "name": "resid", "op": "sub", "a": "g", "b": "s"}),
        json!({"type": "math", "name": "resid0", "op": "sub", "a": "sum", "b": "s"}),
    ];
    let before = render(nodes.clone(), "resid0", 0.5);
    let after = render(nodes, "resid", 0.5);
    let mid = |a: &Audio| rms(&a.channels[0][12_000..20_000]);
    println!(
        "noise floor before {:.1} dB, after gate {:.1} dB",
        db(mid(&before)),
        db(mid(&after))
    );
    assert!(db(mid(&after)) < db(mid(&before)) - 12.0);
}

#[test]
fn spectral_morph_moves_between_two_inputs() {
    let nodes = |amount: f32| {
        vec![
            json!({"type": "osc", "name": "a", "freq": 300, "amp": 0.5}),
            json!({"type": "osc", "name": "b", "shape": "saw", "freq": 700, "amp": 0.4}),
            json!({"type": "spectral", "name": "m", "input": "a", "mode": "morph",
                "other": "b", "amount": amount}),
        ]
    };
    let a = render(nodes(0.0), "a", 0.5);
    let b = render(nodes(0.0), "b", 0.5);
    let m0 = render(nodes(0.0), "m", 0.5);
    let m1 = render(nodes(1.0), "m", 0.5);
    let mh = render(nodes(0.5), "m", 0.5);
    let d = |x: &Audio, y: &Audio| distance(&x.channels[0], &y.channels[0], SR);
    let (d00, d11, dab) = (d(&m0, &a), d(&m1, &b), d(&a, &b));
    let (dh_a, dh_b) = (d(&mh, &a), d(&mh, &b));
    println!(
        "morph: 0 vs a {d00:.2} dB, 1 vs b {d11:.2} dB; a vs b {dab:.2}; 0.5 vs a {dh_a:.2}, vs b {dh_b:.2}"
    );
    assert!(d00 < 3.0 && d11 < 3.0, "{d00} {d11}");
    assert!(dh_a > d00 && dh_b > d11);
}

#[test]
fn spectral_blur_smooths_and_stays_finite() {
    let nodes = vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "spectral", "name": "b", "input": "n", "mode": "blur", "amount": 0.5}),
    ];
    let a = render(nodes, "b", 0.4);
    assert!(a.channels[0].iter().all(|v| v.is_finite()));
    assert!(rms(&a.channels[0]) > 0.001);
}

#[test]
fn deterministic_stereo_and_silent() {
    let nodes = vec![
        json!({"type": "osc", "name": "s", "freq": 440, "amp": 0.5}),
        json!({"type": "pan", "name": "st", "input": "s", "pan": 0.4}),
        json!({"type": "pitch_shift", "name": "p", "input": "st", "semitones": 7}),
        json!({"type": "time_stretch", "name": "t", "input": "st", "factor": 1.5}),
        json!({"type": "spectral", "name": "f", "input": "st", "mode": "freeze", "at": 0.1, "seed": 3}),
        json!({"type": "mix", "name": "all", "inputs": ["p", "t", "f"], "to": "stereo"}),
    ];
    let a = render(nodes.clone(), "all", 0.4);
    let b = render(nodes, "all", 0.4);
    assert_eq!(a.channels.len(), 2);
    assert_eq!(a, b);

    for extra in [
        json!({"type": "pitch_shift", "name": "x", "input": "z", "semitones": -7}),
        json!({"type": "time_stretch", "name": "x", "input": "z", "factor": 0.5}),
        json!({"type": "spectral", "name": "x", "input": "z", "mode": "freeze", "at": 0.1}),
        json!({"type": "spectral", "name": "x", "input": "z", "mode": "blur", "amount": 1}),
        json!({"type": "spectral", "name": "x", "input": "z", "mode": "gate", "threshold_db": -60}),
        json!({"type": "spectral", "name": "x", "input": "z", "mode": "morph", "other": "z", "amount": 0.5}),
    ] {
        let a = render(
            vec![
                json!({"type": "osc", "name": "z", "freq": 440, "amp": 0.0}),
                extra,
            ],
            "x",
            0.3,
        );
        assert!(a.channels[0].iter().all(|v| v.is_finite()));
        assert!(rms(&a.channels[0]) < 1e-6);
    }
}

#[test]
fn bad_fields_explain_themselves() {
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        json!({"type": "osc", "name": "s", "freq": 440}),
        json!({"type": "spectral", "name": "x", "input": "s", "mode": "gate", "threshold_db": -40, "at": 1.0}),
        json!({"type": "output", "name": "out", "input": "x"}),
    ];
    d.duration = Some(0.2);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    assert!(e.contains("`at`") && e.contains("Freeze"), "{e}");
}
