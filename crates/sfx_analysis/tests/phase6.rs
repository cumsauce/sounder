//! Phase 6: `modal` `strike_glide` and `rattle`, and the exaggeration layers
//! `subdrop` and `riser`. Numbers quoted in `docs/nodes/exaggerate.md` come from
//! here (run with `--nocapture`).

#![allow(clippy::unwrap_used)]

mod common;

use common::{SR, band_db, node_audio, peak_hz, rms_db};
use serde_json::{Value, json};

fn struck(extra: Value) -> Vec<Value> {
    let mut m = json!({"type": "modal", "name": "m", "excite": "hit", "material": "metal",
                       "shape": "bar", "f0": 400, "loss": 0.3});
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    vec![json!({"type": "impulse", "name": "hit", "at": 0.01}), m]
}

fn cents(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

#[test]
fn modal_without_the_new_fields_is_unchanged() {
    let plain = node_audio(struck(json!({})), "m", 1.0);
    let zero = node_audio(struck(json!({"strike_glide": 0, "rattle": 0})), "m", 1.0);
    assert!(
        plain
            .iter()
            .zip(&zero)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn strike_glide_starts_sharp_and_settles() {
    let plain = node_audio(struck(json!({})), "m", 1.5);
    let glide = node_audio(struck(json!({"strike_glide": 100})), "m", 1.5);
    let (early, late) = (480 + 480..480 + 4800, 48_000..72_000);
    let e = cents(
        peak_hz(&glide[early.clone()], SR),
        peak_hz(&plain[early], SR),
    );
    let l = cents(peak_hz(&glide[late.clone()], SR), peak_hz(&plain[late], SR));
    println!("strike_glide 100: {e:.1} cents sharp at 20-110 ms, {l:.1} cents at 1.0-1.5 s");
    // The glide follows the level against the very first peak, and a struck
    // bar's first milliseconds are far louder than its ring: the full 100
    // cents only at the loudest instant.
    assert!(e > 25.0 && e < 110.0, "early {e}");
    assert!(l.abs() < e / 10.0, "late {l}");
}

fn wood(extra: Value) -> Vec<Value> {
    let mut m = json!({"type": "modal", "name": "m", "excite": "hit", "material": "wood",
                       "f0": 200, "loss": 0.6});
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    vec![json!({"type": "impulse", "name": "hit", "at": 0.01}), m]
}

#[test]
fn rattle_buzzes_while_loud_and_stops() {
    // A dull body (wood), where a buzz is what a rattle adds.
    let plain = node_audio(wood(json!({})), "m", 1.5);
    let rat = node_audio(wood(json!({"rattle": 0.5})), "m", 1.5);
    let early = 480..480 + 4800;
    let hf = band_db(&rat[early.clone()], SR, 2000.0, 12000.0)
        - band_db(&plain[early], SR, 2000.0, 12000.0);
    // By 0.2 s the wood ring is under the gap (still -48 dB): untouched.
    let tail = 9_600..24_000;
    let diff: Vec<f32> = rat[tail.clone()]
        .iter()
        .zip(&plain[tail.clone()])
        .map(|(a, b)| a - b)
        .collect();
    let tail_db = rms_db(&diff) - rms_db(&plain[tail]);
    let buzz_vs_body = band_db(&rat[480..5280], SR, 2000.0, 12000.0) - rms_db(&plain[480..5280]);
    println!(
        "rattle 0.5: +{hf:.1} dB at 2-12 kHz in the first 100 ms (the buzz {buzz_vs_body:.1} dB re the body); 0.2-0.5 s differs by {tail_db:.1} dB re its level"
    );
    assert!(hf > 10.0, "{hf}");
    assert!(tail_db < -60.0, "{tail_db}");
}

#[test]
fn subdrop_sweeps_from_to_and_drive_adds_harmonics() {
    let node = |drive: f32| {
        vec![
            json!({"type": "subdrop", "name": "s", "from": 90, "to": 32, "time": 0.6, "decay": 3.0, "drive": drive}),
        ]
    };
    let x = node_audio(node(0.0), "s", 1.5);
    let start = peak_hz(&x[0..2400], SR);
    let end = peak_hz(&x[48_000..72_000], SR);
    println!("subdrop 90 -> 32 Hz: {start:.1} Hz in the first 50 ms, {end:.1} Hz at 1.0-1.5 s");
    assert!(start > 70.0 && start < 100.0, "{start}");
    assert!((end - 32.0).abs() < 1.5, "{end}");
    let d = node_audio(node(0.8), "s", 1.5);
    let h3 = |y: &[f32]| band_db(&y[48_000..72_000], SR, 90.0, 102.0);
    println!(
        "3rd harmonic (96 Hz) at drive 0 / 0.8: {:.1} / {:.1} dB",
        h3(&x),
        h3(&d)
    );
    assert!(h3(&d) > h3(&x) + 20.0);
}

#[test]
fn riser_swells_to_the_hit_and_stops() {
    let x = node_audio(
        vec![json!({"type": "riser", "name": "r", "at": 1.5, "length": 1.4, "release": 0.01})],
        "r",
        2.0,
    );
    let at = |t0: f32, t1: f32| rms_db(&x[(t0 * 48_000.0) as usize..(t1 * 48_000.0) as usize]);
    let (a, b, c) = (at(0.2, 0.5), at(0.8, 1.1), at(1.35, 1.5));
    let after = at(1.55, 2.0);
    println!(
        "riser rms: {a:.1} / {b:.1} / {c:.1} dB at 0.2-0.5 / 0.8-1.1 / 1.35-1.5 s; after the hit {after:.1} dB"
    );
    assert!(a < b && b < c, "{a} {b} {c}");
    assert!(after < -100.0, "{after}");
    let cent =
        |t0: f32, t1: f32| peak_hz(&x[(t0 * 48_000.0) as usize..(t1 * 48_000.0) as usize], SR);
    assert!(cent(1.3, 1.5) > 3.0 * cent(0.2, 0.5), "the pitch rises");
}

#[test]
fn bad_values_are_errors_naming_the_field() {
    let err = |nodes: Vec<Value>| {
        let mut d = sfx_graph::SoundDoc::named("t");
        d.nodes = nodes;
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "x"}));
        d.duration = Some(1.0);
        d.render(&sfx_graph::RenderOpts::default()).err().unwrap()
    };
    let mut m = struck(json!({"strike_glide": 5000}));
    m[1]["name"] = json!("x");
    assert!(err(m).contains("strike_glide"));
    let e = err(vec![
        json!({"type": "riser", "name": "x", "at": 0.5, "length": 1.0}),
    ]);
    assert!(e.contains("at least 1"), "{e}");
    let e = err(vec![json!({"type": "subdrop", "name": "x", "from": 5})]);
    assert!(e.contains("from"), "{e}");
}

/// A cinematic hit: a noise crack, a wood body, a riser into it and a
/// subdrop under it. The subdrop is tonal on purpose, so the `layers`
/// `impact` preset (made for realistic hits) reads it as voiced.
#[test]
fn a_cinematic_hit_is_voiced_where_a_realistic_one_is_not() {
    let hit = 1.0;
    let nodes = vec![
        json!({"type": "riser", "name": "rise", "at": hit, "length": 0.9, "from": 300, "to": 3000, "amp": 0.3}),
        json!({"type": "noise", "name": "n", "color": "white", "seed": 2}),
        json!({"type": "decay", "name": "ce", "time": 0.02, "at": hit}),
        json!({"type": "math", "name": "crack", "op": "mul", "a": "n", "b": "ce"}),
        json!({"type": "decay", "name": "be", "time": 0.012, "at": hit}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "be"}),
        json!({"type": "modal", "name": "body", "excite": "burst", "material": "wood", "size": 0.6, "loss": 1.5, "rattle": 0.4}),
        json!({"type": "subdrop", "name": "drop", "from": 90, "to": 30, "time": 0.8, "decay": 1.5, "at": hit, "drive": 0.3}),
        json!({"type": "mix", "name": "all", "inputs": ["rise", {"input": "crack", "gain": 0.5}, "body", {"input": "drop", "gain": 0.7}]}),
    ];
    let mut d = sfx_graph::SoundDoc::named("cine");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "all", "peak_db": -1}));
    d.duration = Some(2.5);
    let a = d.render(&sfx_graph::RenderOpts::default()).unwrap().audio;
    let desc = sfx_analysis::describe(&a);
    println!(
        "cinematic hit: voiced {:.2}, pitch {:?}, energy shares {:?}",
        desc.voiced, desc.pitch_hz, desc.band_shares
    );
    assert!(
        desc.voiced > 0.3,
        "the subdrop should read as pitched: {}",
        desc.voiced
    );
}
