//! R4-A: the `ring` macro claim, the median over the strong onsets of each
//! hit's own -20 dB fall (ms), so a knob on a modal `loss` reads the same in
//! a single hit, a knock and a strike followed by a later ring.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sfx_analysis::measure::{measure, ring_ms};
use sfx_graph::{Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc};

fn entry(node: &str, field: &str, from: f64, to: f64, curve: MapCurve) -> MapEntry {
    MapEntry {
        node: node.into(),
        field: field.into(),
        param: None,
        from,
        to,
        curve,
        col: None,
        index: None,
        key: None,
    }
}

fn doc_with(nodes: Vec<Value>, secs: f32, claim: Claim, map: Vec<MapEntry>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.duration = Some(secs);
    d.nodes = nodes;
    let knob = Macro {
        name: "knob".into(),
        value: 0.5,
        default: None,
        doc: String::new(),
        claim: Some(claim),
        game: false,
        map,
    };
    d.apply(
        vec![],
        DocEdit {
            macros: Some(vec![knob]),
            ..DocEdit::default()
        },
        &RenderOpts::default(),
    )
    .unwrap();
    d
}

fn at(d: &SoundDoc, x: f64) -> sfx_graph::Audio {
    d.render(&RenderOpts {
        macros: BTreeMap::from([("knob".to_string(), x)]),
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap()
    .audio
}

const POS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn rings(d: &SoundDoc) -> Vec<f32> {
    POS.iter().map(|x| ring_ms(&at(d, *x))).collect()
}

fn out(input: &str) -> Value {
    json!({"type": "output", "name": "out", "input": input, "peak_db": -3, "fade_in": 0.0005})
}

/// A modal wood plate at 400 Hz struck at each time; `loss` is the knob.
fn plate_doc(times: Value, secs: f32) -> SoundDoc {
    doc_with(
        vec![
            json!({"type": "impulse", "name": "hit", "at": times}),
            json!({"type": "modal", "name": "body", "excite": "hit", "material": "wood", "f0": 400}),
            out("body"),
        ],
        secs,
        Claim(Measure::Ring, 1),
        // knob 0 -> loss 3 (short), 1 -> loss 0.3 (long): ring rises.
        vec![entry("body", "loss", 3.0, 0.3, MapCurve::Exp)],
    )
}

#[test]
fn ring_falls_with_modal_loss_for_a_single_hit() {
    let d = plate_doc(json!([0.01]), 1.5);
    let r = rings(&d);
    println!("single hit, ring over knob (loss 3 -> 0.3): {r:?}");
    assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
    assert!(r[4] > 4.0 * r[0], "{r:?}");
    // The claim holds under verify's macros sweep.
    let opts = RenderOpts::default();
    let rendered = d.render(&opts).unwrap();
    let res = sfx_analysis::run(
        &sfx_analysis::Expect::defaults_for(&d),
        &sfx_analysis::CheckCx {
            doc: &d,
            rendered: &rendered,
            opts: &opts,
        },
    )
    .unwrap();
    let line = res.iter().find(|c| c.name == "macro knob").unwrap();
    println!("verify: {}", line.detail);
    assert!(line.pass, "{line:?}");
}

#[test]
fn ring_falls_with_modal_loss_for_a_three_hit_knock() {
    let d = plate_doc(json!([0.02, 0.22, 0.42]), 1.5);
    let r = rings(&d);
    println!("knock (0.2 s), ring over knob: {r:?}");
    assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
}

#[test]
fn ring_falls_with_modal_loss_for_a_strike_and_a_later_ring() {
    // A short wooden strike at 0.02 s, a metal ring at 0.5 s; one knob scales
    // both losses.
    let d = doc_with(
        vec![
            json!({"type": "impulse", "name": "k1", "at": [0.02]}),
            json!({"type": "impulse", "name": "k2", "at": [0.5]}),
            json!({"type": "modal", "name": "strike", "excite": "k1", "material": "wood", "f0": 250}),
            json!({"type": "modal", "name": "bell", "excite": "k2", "material": "metal", "f0": 1500}),
            json!({"type": "mix", "name": "both", "inputs": ["strike", "bell"]}),
            out("both"),
        ],
        2.5,
        Claim(Measure::Ring, 1),
        vec![
            entry("strike", "loss", 3.0, 0.3, MapCurve::Exp),
            entry("bell", "loss", 3.0, 0.3, MapCurve::Exp),
        ],
    );
    let r = rings(&d);
    println!("strike + ring, ring over knob: {r:?}");
    assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
}

#[test]
fn ring_rises_with_a_decay_node_time() {
    let d = doc_with(
        vec![
            json!({"type": "noise", "name": "n", "color": "white", "seed": 1}),
            json!({"type": "decay", "name": "e", "time": 0.05}),
            json!({"type": "math", "name": "hit", "op": "mul", "a": "n", "b": "e"}),
            out("hit"),
        ],
        1.0,
        Claim(Measure::Ring, 1),
        vec![entry("e", "time", 0.02, 0.3, MapCurve::Exp)],
    );
    let r = rings(&d);
    println!("noise * decay, ring over knob: {r:?}");
    assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
}

#[test]
fn ring_is_a_measure_with_a_name_and_a_parseable_claim() {
    assert_eq!(Measure::Ring.name(), "ring");
    assert_eq!(Measure::Ring.unit(), "ms");
    let c: Claim = serde_json::from_value(json!(["ring", 1])).unwrap();
    assert_eq!(c, Claim(Measure::Ring, 1));
    // Silence has no hits and no decay.
    let silent = sfx_graph::Audio {
        sample_rate: 48_000,
        channels: vec![vec![0.0; 4800]],
    };
    assert_eq!(measure(&silent, Measure::Ring), 0.0);
}
