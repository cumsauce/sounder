//! fire_fix: the blind listen heard `fire` as "wind next to a gurgling
//! liquid" and `spark` as "a cicada / a frog". These measure the causes the
//! fix removes: tonal, ringing crackle pops; a smooth roar envelope; a spark
//! envelope locked to one strong mains line. Numbers in docs/nodes/elements.md.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

/// The crackle alone: fire through a 24 dB high-pass at 1.2 kHz (the roar
/// sits under it), the hiss kept low by intensity 0.3.
fn crackle_only(seed: u64) -> Vec<Value> {
    let mut n = one(
        "fire",
        json!({"size": 0.6, "intensity": 0.3, "crackle": 1.0, "seed": seed}),
    );
    n.push(json!({"type": "filter", "name": "hp", "input": "n", "mode": "hp", "freq": 1200, "slope": 24}));
    n.push(json!({"type": "output", "name": "out", "input": "hp", "peak_db": -1}));
    n
}

#[test]
fn fire_crackle_is_short_and_unpitched() {
    let (mut voiced, mut flat, mut ring) = (0.0, 0.0, 0.0);
    for seed in 1..=3u64 {
        let d = describe(&render(crackle_only(seed), 6.0));
        let rows: Vec<f32> = d.onset_rows.iter().filter_map(|r| r.decay20_ms).collect();
        let med = {
            let mut r = rows.clone();
            r.sort_by(f32::total_cmp);
            r.get(r.len() / 2).copied().unwrap_or(0.0)
        };
        println!(
            "fire crackle seed {seed}: voiced {:.2}, flatness middle {:.2}, median -20 dB time {med:.1} ms over {} onsets",
            d.voiced,
            d.flatness.middle,
            rows.len()
        );
        voiced += d.voiced / 3.0;
        flat += d.flatness.middle / 3.0;
        ring += med / 3.0;
    }
    println!("fire crackle mean: voiced {voiced:.2}, flatness {flat:.2}, -20 dB {ring:.1} ms");
    assert!(voiced < 0.2, "voiced {voiced}");
    assert!(flat > 0.3, "flatness {flat}");
}

/// Mean |step| between neighbouring 40 ms RMS frames, dB: flicker.
fn flicker_db(x: &[f32]) -> f32 {
    let w = (0.04 * SR as f32) as usize;
    let e: Vec<f32> = x.chunks(w).filter(|c| c.len() == w).map(rms_db).collect();
    e.windows(2).map(|p| (p[1] - p[0]).abs()).sum::<f32>() / (e.len() - 1) as f32
}

#[test]
fn fire_roar_flutters() {
    let mut f = 0.0;
    for seed in 1..=3u64 {
        let mut n = one(
            "fire",
            json!({"size": 0.6, "intensity": 0.5, "crackle": 0, "seed": seed}),
        );
        n.push(json!({"type": "filter", "name": "lp", "input": "n", "mode": "lp", "freq": 600, "slope": 24}));
        let x = node_audio(n, "lp", 6.0);
        f += flicker_db(&x) / 3.0;
    }
    println!("fire roar (under 600 Hz): mean 40 ms frame step {f:.2} dB");
    assert!(f > 1.8, "{f}");
}

/// Strongest envelope-spectrum line within 3 Hz of `hz`, dB over the median
/// of 20..400 Hz (|x| smoothed at 300 Hz).
fn env_line(x: &[f32], hz: f32) -> f32 {
    let a = (-2.0 * std::f32::consts::PI * 300.0 / SR as f32).exp();
    let mut y = 0.0;
    let e: Vec<f32> = x
        .iter()
        .map(|v| {
            y = a * y + (1.0 - a) * v.abs();
            y
        })
        .collect();
    let m = e.iter().sum::<f32>() / e.len() as f32;
    let e: Vec<f32> = e.iter().map(|v| v - m).collect();
    let s: Vec<(f32, f32)> = spectrum(&e, SR)
        .into_iter()
        .filter(|(f, _)| (20.0..400.0).contains(f))
        .collect();
    let mut v: Vec<f32> = s.iter().map(|p| p.1).collect();
    v.sort_by(f32::total_cmp);
    let at = s
        .iter()
        .filter(|(f, _)| (f - hz).abs() < 3.0)
        .map(|p| p.1)
        .fold(-200.0, f32::max);
    at - v[v.len() / 2]
}

#[test]
fn spark_hums_at_twice_the_mains() {
    // Book check 1: the spark is Farnell's transformer (a beating hum at twice
    // the mains, gated arcing chirps, formant snaps); the old strike model
    // was heard as ripping plastic. The hum must show in the envelope at
    // twice the mains, for both mains.
    for (buzz, hz) in [(50, 100.0), (60, 120.0)] {
        let mut line = 0.0;
        for seed in 1..=3u64 {
            let x = node_audio(
                one("spark", json!({"rate": 2, "buzz": buzz, "seed": seed})),
                "n",
                4.0,
            );
            line += env_line(&x, hz) / 3.0;
        }
        println!("spark buzz {buzz}: {hz} Hz envelope line {line:.1} dB over the median");
        assert!(line > 6.0, "buzz {buzz}: {line}");
    }
}
