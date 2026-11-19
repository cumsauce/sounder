//! `finish`: the bus chain as one node. No field set is the input bit for
//! bit; each field moves what it names, in the direction it names; width
//! keeps the band under 150 Hz mono. Numbers quoted in
//! `docs/nodes/finish.md` come from here (run with `--nocapture`).

use crate::common::{SR, band_db, node_audio, rms_db, spectrum};
use serde_json::{Value, json};
use sfx_graph::{RenderOpts, SoundDoc};

/// A hit with a body: pink noise under a 0.4 s decay, twice.
fn hits() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "color": "pink", "seed": 4}),
        json!({"type": "pattern", "name": "taps", "times": [0.01, 0.3]}),
        json!({"type": "trigger_env", "name": "env", "events": "taps", "attack": 0.001, "decay": 0.25}),
        json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "env"}),
    ]
}

fn with_finish(src: Vec<Value>, fields: Value) -> Vec<Value> {
    let mut f = json!({"type": "finish", "name": "f", "input": "src"});
    for (k, v) in fields.as_object().unwrap() {
        f[k] = v.clone();
    }
    let mut nodes = src;
    nodes.push(f);
    nodes
}

fn crest_db(x: &[f32]) -> f32 {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    20.0 * peak.log10() - rms_db(x)
}

fn render_err(nodes: Vec<Value>) -> String {
    crate::common::render_error(nodes, "f", 0.6)
}

#[test]
fn no_field_is_the_input_bit_for_bit() {
    let nodes = with_finish(hits(), json!({}));
    let a = node_audio(&nodes, "src", 0.6);
    let b = node_audio(&nodes, "f", 0.6);
    assert!(
        a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()),
        "finish with no field changed the samples"
    );
    // A field at 0 is off too.
    let zero = with_finish(
        hits(),
        json!({"punch": 0, "tail_db": 0, "glue": 0, "warmth": 0, "weight_db": 0, "air_db": 0}),
    );
    let c = node_audio(&zero, "f", 0.6);
    assert!(a.iter().zip(&c).all(|(p, q)| p.to_bits() == q.to_bits()));
}

#[test]
fn punch_raises_the_crest_and_glue_lowers_it() {
    let crest = |fields: Value| crest_db(&node_audio(with_finish(hits(), fields), "f", 0.6));
    let p: Vec<f32> = [0.0, 0.5, 1.0]
        .iter()
        .map(|v| crest(json!({"punch": v})))
        .collect();
    let g: Vec<f32> = [0.0, 0.5, 1.0]
        .iter()
        .map(|v| crest(json!({"glue": v})))
        .collect();
    println!("crest dB, punch 0 / .5 / 1: {p:.2?}; glue 0 / .5 / 1: {g:.2?}");
    assert!(p[0] < p[1] && p[1] < p[2], "punch: {p:?}");
    assert!(p[2] - p[0] > 1.0, "punch moves the crest too little: {p:?}");
    assert!(g[0] > g[1] && g[1] > g[2], "glue: {g:?}");
    assert!(g[0] - g[2] > 1.0, "glue moves the crest too little: {g:?}");
}

#[test]
fn tail_db_turns_the_tail_down_and_leaves_the_attack() {
    let level = |fields: Value| {
        let x = node_audio(with_finish(hits(), fields), "f", 0.6);
        // The first 2 ms of the first hit, and its tail (60-250 ms).
        (rms_db(&x[480..576]), rms_db(&x[2880..12000]))
    };
    let (a0, t0) = level(json!({}));
    let (a6, t6) = level(json!({"tail_db": -6}));
    println!(
        "tail_db -6: attack {:.2} dB, tail {:.2} dB",
        a6 - a0,
        t6 - t0
    );
    assert!((t6 - t0) < -3.0, "tail moved {}", t6 - t0);
    assert!((a6 - a0).abs() < 1.5, "attack moved {}", a6 - a0);
}

#[test]
fn warmth_adds_harmonics_and_no_dc() {
    let tone = vec![json!({"type": "osc", "name": "src", "freq": 200, "amp": 0.5})];
    let h2: Vec<(f32, f32)> = [0.0, 0.5, 1.0]
        .iter()
        .map(|w| {
            let x = node_audio(with_finish(tone.clone(), json!({"warmth": w})), "f", 0.5);
            let tail = &x[4800..];
            let mean = tail.iter().sum::<f32>() / tail.len() as f32;
            (
                band_db(tail, SR, 380.0, 420.0),
                20.0 * mean.abs().max(1e-12).log10(),
            )
        })
        .collect();
    println!("2nd harmonic dB / DC dB at warmth 0 / .5 / 1: {h2:.1?}");
    assert!(h2[0].0 < h2[1].0 && h2[1].0 < h2[2].0, "{h2:?}");
    assert!(h2.iter().all(|(_, dc)| *dc < -40.0), "DC left: {h2:?}");
}

#[test]
fn weight_and_air_are_shelves() {
    let noise =
        vec![json!({"type": "noise", "name": "src", "color": "white", "seed": 2, "amp": 0.2})];
    let level = |fields: Value, lo: f32, hi: f32| {
        band_db(
            &node_audio(with_finish(noise.clone(), fields), "f", 1.0),
            SR,
            lo,
            hi,
        )
    };
    let low = level(json!({"weight_db": 6}), 30.0, 60.0) - level(json!({}), 30.0, 60.0);
    let mid = level(json!({"weight_db": 6, "air_db": 6}), 900.0, 1100.0)
        - level(json!({}), 900.0, 1100.0);
    let high = level(json!({"air_db": 6}), 12000.0, 16000.0) - level(json!({}), 12000.0, 16000.0);
    println!(
        "weight +6: {low:.2} dB at 30-60 Hz; air +6: {high:.2} dB at 12-16 kHz; both: {mid:.2} dB at 1 kHz"
    );
    assert!((5.0..7.0).contains(&low), "{low}");
    assert!((5.0..7.0).contains(&high), "{high}");
    assert!(mid.abs() < 1.0, "the shelves move 1 kHz by {mid}");
}

#[test]
fn width_keeps_the_low_band_mono() {
    let stereo = vec![
        json!({"type": "noise", "name": "n", "color": "pink", "seed": 9, "amp": 0.3}),
        json!({"type": "decorrelate", "name": "src", "input": "n"}),
    ];
    let side = |w: f32| {
        let mut d = SoundDoc::named("test");
        d.nodes = with_finish(stereo.clone(), json!({"width": w}));
        d.nodes
            .push(json!({"type": "output", "name": "_out", "input": "f"}));
        d.duration = Some(1.0);
        let a = d
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap()
            .audio;
        let (l, r) = (&a.channels[0], &a.channels[1]);
        let s: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a - b)).collect();
        let m: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect();
        (
            band_db(&s, SR, 20.0, 60.0) - band_db(&m, SR, 20.0, 60.0),
            band_db(&s, SR, 1000.0, 4000.0),
        )
    };
    let (low1, hi1) = side(1.0);
    let (low2, hi2) = side(2.0);
    let (_, hi0) = side(0.0);
    println!(
        "side under 60 Hz vs mid: {low1:.1} dB (width 1), {low2:.1} dB (width 2); side 1-4 kHz: {hi0:.1} / {hi1:.1} / {hi2:.1} dB at width 0 / 1 / 2"
    );
    assert!(low1 < -30.0 && low2 < -30.0, "{low1} {low2}");
    assert!(
        (hi2 - hi1 - 6.0).abs() < 1.0,
        "width 2 should double the side: {hi1} -> {hi2}"
    );
    assert!(
        hi0 < hi1 - 40.0,
        "width 0 should be mono above 150 Hz: {hi0}"
    );
    let _ = spectrum; // keep the helper import used in every configuration
}

#[test]
fn errors_name_the_field() {
    let e = render_err(with_finish(hits(), json!({"width": 1.5})));
    assert!(e.contains("width") && e.contains("mono"), "{e}");
    let e = render_err(with_finish(hits(), json!({"punch": 2})));
    assert!(e.contains("punch") && e.contains("between 0 and 1"), "{e}");
    let e = render_err(with_finish(hits(), json!({"air_db": 20})));
    assert!(e.contains("air_db"), "{e}");
}

#[test]
fn renders_the_same_bits_on_any_thread_count() {
    crate::common::assert_bit_identical(
        with_finish(
            hits(),
            json!({"punch": 0.5, "glue": 0.5, "warmth": 0.5, "weight_db": 3, "air_db": -2}),
        ),
        0.6,
    );
}
