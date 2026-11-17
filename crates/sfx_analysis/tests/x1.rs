//! X1: `spectral` modes shift / stretch / smear / hps and `pitch_shift.formant`.
//! The first test pins the output of every pre-existing mode by hash.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::{Buf, RenderOpts, SoundDoc};

/// FNV-1a over the bit patterns of every sample of every channel of node `name`.
fn hash_node(nodes: &[Value], name: &str, duration: f32) -> u64 {
    let mut d = SoundDoc::named("hash");
    d.nodes = nodes.to_vec();
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": name}));
    d.duration = Some(duration);
    let r = d
        .render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let buf = r.eval.buf(name).unwrap();
    let chans: Vec<Vec<f32>> = match &*buf {
        Buf::Audio(x) => vec![x.clone()],
        Buf::Stereo(x) => vec![
            x.iter().map(|s| s[0]).collect(),
            x.iter().map(|s| s[1]).collect(),
        ],
        o => panic!("{}", o.kind()),
    };
    let mut h = 0xcbf29ce484222325u64;
    for c in chans {
        for v in c {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

/// A fixed test signal: a decaying two-partial tone plus noise, mono, and a
/// panned stereo copy.
fn base() -> Vec<Value> {
    vec![
        json!({"type": "osc", "name": "a", "freq": 440, "amp": 0.4}),
        json!({"type": "osc", "name": "b", "freq": 1230, "amp": 0.2}),
        json!({"type": "noise", "name": "n", "amp": 0.05}),
        json!({"type": "mix", "name": "m0", "inputs": ["a", "b", "n"]}),
        json!({"type": "decay", "name": "e", "time": 0.3}),
        json!({"type": "math", "name": "src", "op": "mul", "a": "m0", "b": "e"}),
        json!({"type": "pan", "name": "st", "input": "src", "pan": 0.3}),
        json!({"type": "osc", "name": "o2", "shape": "saw", "freq": 300, "amp": 0.3}),
    ]
}

fn with(extra: Vec<Value>) -> Vec<Value> {
    let mut v = base();
    v.extend(extra);
    v
}

/// The hashes below were recorded on the commit before X1 touched
/// `spectral.rs`. A change here means an existing mode moved.
#[test]
fn existing_modes_are_bit_identical_to_before_x1() {
    let cases: Vec<(&str, Vec<Value>, u64)> = vec![
        (
            "pitch_shift +7",
            with(vec![
                json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7}),
            ]),
            0x0e3d0def90663e29,
        ),
        (
            "pitch_shift -12 stereo w1024",
            with(vec![
                json!({"type": "pitch_shift", "name": "x", "input": "st", "semitones": -12, "window": 1024}),
            ]),
            0xed0abbb02b355a38,
        ),
        (
            "pitch_shift ratio 1.3",
            with(vec![
                json!({"type": "pitch_shift", "name": "x", "input": "src", "ratio": 1.3}),
            ]),
            0xaa761df6380050d5,
        ),
        (
            "time_stretch 1.5",
            with(vec![
                json!({"type": "time_stretch", "name": "x", "input": "src", "factor": 1.5}),
            ]),
            0x1ec788c4aa15d739,
        ),
        (
            "time_stretch 0.7 stereo",
            with(vec![
                json!({"type": "time_stretch", "name": "x", "input": "st", "factor": 0.7}),
            ]),
            0x76c8f2baa9894175,
        ),
        (
            "freeze",
            with(vec![
                json!({"type": "spectral", "name": "x", "input": "src", "mode": "freeze", "at": 0.05, "length": 0.2, "seed": 3}),
            ]),
            0x9da65de238040347,
        ),
        (
            "blur",
            with(vec![
                json!({"type": "spectral", "name": "x", "input": "src", "mode": "blur", "amount": 0.6}),
            ]),
            0x0dd9c0ded8b4285b,
        ),
        (
            "gate stereo",
            with(vec![
                json!({"type": "spectral", "name": "x", "input": "st", "mode": "gate", "threshold_db": -50}),
            ]),
            0xf7cc2aa9486b2880,
        ),
        (
            "morph",
            with(vec![
                json!({"type": "spectral", "name": "x", "input": "src", "mode": "morph", "other": "o2", "amount": 0.5}),
            ]),
            0x2261d66e192d4099,
        ),
    ];
    let mut bad = Vec::new();
    for (what, nodes, want) in cases {
        let h = hash_node(&nodes, "x", 0.5);
        println!("HASH {what}: {h:#018x}");
        if h != want {
            bad.push(what);
        }
    }
    assert!(bad.is_empty(), "output changed: {bad:?}");
}
