//! R4-C: macros reach into `voices` patches (`voices/patch_node`), address
//! points of an `env` and the state of an `expr`, and `contact` / `modal`
//! take `gain_db`.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use std::collections::BTreeMap;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_graph::{DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc};

fn entry(node: &str, field: &str, from: f64, to: f64) -> MapEntry {
    MapEntry {
        node: node.into(),
        field: field.into(),
        param: None,
        from,
        to,
        curve: MapCurve::Linear,
        col: None,
        index: None,
        key: None,
    }
}

fn knob(name: &str, map: Vec<MapEntry>) -> Macro {
    Macro {
        name: name.into(),
        value: 0.5,
        default: None,
        doc: String::new(),
        claim: None,
        game: false,
        map,
    }
}

fn with_knob(nodes: Vec<Value>, duration: f32, m: Macro) -> Result<SoundDoc, String> {
    let mut d = SoundDoc::named("t");
    d.duration = Some(duration);
    d.nodes = nodes;
    d.apply(
        vec![],
        DocEdit {
            macros: Some(vec![m]),
            ..DocEdit::default()
        },
        &RenderOpts::default(),
    )?;
    Ok(d)
}

fn at(d: &SoundDoc, name: &str, x: f64) -> sfx_graph::Audio {
    d.render(&RenderOpts {
        macros: BTreeMap::from([(name.to_string(), x)]),
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap()
    .audio
}

fn shards() -> Vec<Value> {
    vec![
        json!({"type": "pattern", "name": "ev", "times": [0.02, 0.2, 0.4]}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.3,
               "patch": [
                   {"type": "impulse", "name": "tap", "at": [0.0]},
                   {"type": "modal", "name": "piece", "excite": "tap",
                    "material": "glass", "size": 0.3, "thickness": 0.003}]}),
        json!({"type": "output", "name": "out", "input": "v", "peak_db": -6}),
    ]
}

#[test]
fn a_macro_on_a_voices_patch_node_moves_the_centroid_monotonically() {
    // Smaller piece, higher pitch: size 0.6 -> 0.1 m.
    let d = with_knob(
        shards(),
        0.8,
        knob("shrink", vec![entry("v/piece", "size", 0.6, 0.1)]),
    )
    .unwrap();
    let cs: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|x| measure(&at(&d, "shrink", *x), Measure::Centroid))
        .collect();
    println!("centroid over v/piece.size: {cs:?}");
    assert!(cs.windows(2).all(|w| w[1] > w[0] * 1.05), "{cs:?}");
}

#[test]
fn a_bad_patch_path_errors_with_the_patch_nodes() {
    let e = with_knob(
        shards(),
        0.8,
        knob("k", vec![entry("v/nope", "size", 0.6, 0.1)]),
    )
    .err()
    .unwrap();
    println!("{e}");
    assert!(
        e.contains("tap") && e.contains("piece") && e.contains("`v`"),
        "{e}"
    );
    let e = with_knob(
        shards(),
        0.8,
        knob("k", vec![entry("ev/x", "size", 0.6, 0.1)]),
    )
    .err()
    .unwrap();
    println!("{e}");
    assert!(e.contains("not a voices node"), "{e}");
    // A patch node's bare name still errors, and now says how to address it.
    let e = with_knob(
        shards(),
        0.8,
        knob("k", vec![entry("piece", "size", 0.6, 0.1)]),
    )
    .err()
    .unwrap();
    assert!(e.contains("v/piece"), "{e}");
}

fn peak_time(x: &[f32], sr: u32) -> f32 {
    let (k, _) = x
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap();
    k as f32 / sr as f32
}

fn swell(extra_key: &str) -> (SoundDoc, f32) {
    // y runs 0.4 -> 1.0; x (time) 0.1 -> 0.5, both inside the neighbours.
    let (from, to) = if extra_key == "time" || extra_key == "x" {
        (0.1, 0.5)
    } else {
        (0.4, 1.0)
    };
    let nodes = vec![
        json!({"type": "osc", "name": "tone", "freq": 220, "amp": 0.5}),
        json!({"type": "env", "name": "shape",
               "points": [[0, 0], [0.2, 0.4], [0.6, 0.8], [1.0, 0]]}),
        json!({"type": "math", "name": "note", "op": "mul", "a": "tone", "b": "shape"}),
        json!({"type": "output", "name": "out", "input": "note"}),
    ];
    let mut m = entry("shape", "points", from, to);
    m.index = Some(1);
    m.key = Some(extra_key.into());
    (with_knob(nodes, 1.0, knob("early", vec![m])).unwrap(), 0.0)
}

#[test]
fn an_env_point_y_mapped_moves_the_peak_time() {
    let (d, _) = swell("y");
    let ts: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|x| {
            let a = at(&d, "early", *x);
            peak_time(&a.channels[0], a.sample_rate)
        })
        .collect();
    println!("peak time over the point's y: {ts:?}");
    assert!(ts[0] > 0.5 && ts[4] < 0.3, "{ts:?}");
    assert!(ts.windows(2).all(|w| w[1] <= w[0] + 0.01), "{ts:?}");
    // `value` is the same number as `y`.
    let (d2, _) = swell("value");
    assert_eq!(
        at(&d, "early", 0.9).channels,
        at(&d2, "early", 0.9).channels
    );
}

#[test]
fn an_env_point_x_by_time_and_a_bad_key_lists_the_names() {
    let (d, _) = swell("time");
    let a = at(&d, "early", 0.0);
    let b = at(&d, "early", 1.0);
    assert_ne!(a.channels, b.channels);
    let (dx, _) = swell("x");
    assert_eq!(
        at(&dx, "early", 0.3).channels,
        at(&d, "early", 0.3).channels
    );
    let nodes = vec![
        json!({"type": "osc", "name": "tone", "freq": 220}),
        json!({"type": "env", "name": "shape", "points": [[0, 0], [0.5, 1]]}),
        json!({"type": "math", "name": "note", "op": "mul", "a": "tone", "b": "shape"}),
    ];
    let mut m = entry("shape", "points", 0.1, 0.4);
    m.index = Some(1);
    m.key = Some("level".into());
    let e = with_knob(nodes, 1.0, knob("k", vec![m])).err().unwrap();
    println!("{e}");
    assert!(e.contains("x, time") && e.contains("y, value"), "{e}");
}

#[test]
fn an_expr_state_constant_mapped_changes_the_output() {
    let nodes = vec![
        json!({"type": "osc", "name": "tone", "freq": 300, "amp": 0.5}),
        json!({"type": "expr", "name": "scaled", "inputs": {"a": "tone"},
               "state": {"g": 0.1}, "out": "a * g", "update": {"g": "g"}}),
        json!({"type": "output", "name": "out", "input": "scaled", "fade_in": 0.0, "fade_out": 0.0}),
    ];
    let mut m = entry("scaled", "state", 0.1, 1.0);
    m.key = Some("g".into());
    let d = with_knob(nodes.clone(), 0.2, knob("level", vec![m])).unwrap();
    let rms = |x: f64| rms_db(&at(&d, "level", x).channels[0]);
    let (lo, hi) = (rms(0.0), rms(1.0));
    println!("rms at 0 / 1: {lo:.2} / {hi:.2} dB");
    assert!((hi - lo - 20.0).abs() < 0.2, "{lo} {hi}");
    // A state name the expr does not have lists the ones it has.
    let mut bad = entry("scaled", "state", 0.1, 1.0);
    bad.key = Some("h".into());
    let e = with_knob(nodes, 0.2, knob("k", vec![bad])).err().unwrap();
    println!("{e}");
    assert!(e.contains("entries: g"), "{e}");
}

fn strike(extra: Value) -> Vec<Value> {
    let mut c = json!({"type": "contact", "name": "c", "speed": 1.5, "mass": 0.2,
                       "material": "wood", "hardness": 0.7});
    c.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![c]
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn contact_gain_db_lowers_the_peak_by_that_many_db() {
    let base = node_audio(strike(json!({})), "c", 0.1);
    let zero = node_audio(strike(json!({"gain_db": 0})), "c", 0.1);
    let down = node_audio(strike(json!({"gain_db": -6})), "c", 0.1);
    assert!(
        base.iter()
            .zip(&zero)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    let d = 20.0 * (peak(&down) / peak(&base)).log10();
    println!("contact gain_db -6: {d:.3} dB");
    assert!((d + 6.0).abs() < 0.1, "{d}");
}

#[test]
fn modal_gain_db_moves_the_level_and_zero_is_bit_identical() {
    let modal = |extra: Value| {
        let mut m = json!({"type": "modal", "name": "m", "excite": "hit",
                           "material": "wood", "size": 0.5});
        m.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![json!({"type": "impulse", "name": "hit", "at": [0.01]}), m]
    };
    let base = node_audio(modal(json!({})), "m", 0.3);
    let zero = node_audio(modal(json!({"gain_db": 0})), "m", 0.3);
    let down = node_audio(modal(json!({"gain_db": -6})), "m", 0.3);
    assert!(
        base.iter()
            .zip(&zero)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    let d = 20.0 * (peak(&down) / peak(&base)).log10();
    println!("modal gain_db -6: {d:.3} dB");
    assert!((d + 6.0).abs() < 0.1, "{d}");
}
