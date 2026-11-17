//! R2-F: `bounce` jitter, `wander` min / max, `ramp` duration.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::{Buf, Event, RenderOpts, SoundDoc};

fn doc(nodes: Vec<Value>, seed: u64, duration: f32) -> SoundDoc {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "osc", "name": "_quiet", "freq": 1, "amp": 0.0}));
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": "_quiet"}));
    d.duration = Some(duration);
    d.seed = seed;
    d
}

fn events(nodes: Vec<Value>, seed: u64) -> Vec<Event> {
    let r = doc(nodes, seed, 2.0)
        .render(&RenderOpts::default())
        .unwrap_or_else(|e| panic!("{e}"));
    let b = r.eval.buf("b").unwrap_or_else(|e| panic!("{e}"));
    match &*b {
        Buf::Events(v) => v.clone(),
        other => panic!("`b` is {}", other.kind()),
    }
}

fn error(nodes: Vec<Value>) -> String {
    match doc(nodes, 1, 0.5).render(&RenderOpts::default()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    }
}

fn bounce(extra: Value) -> Value {
    let mut n = json!({"type": "bounce", "name": "b", "first": 0.3, "restitution": 0.6});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    n
}

#[test]
fn bounce_jitter_default_unchanged_and_seeded() {
    let plain = events(vec![bounce(json!({}))], 1);
    let zero = events(vec![bounce(json!({"jitter": 0}))], 1);
    assert_eq!(plain.len(), zero.len());
    for (a, b) in plain.iter().zip(&zero) {
        assert_eq!(a.t.to_bits(), b.t.to_bits());
        assert_eq!(a.value.to_bits(), b.value.to_bits());
    }
    // gap k = 0.3 * 0.6^k from the first hit at 0
    assert!((plain[1].t - 0.3).abs() < 1e-6 && (plain[2].t - 0.48).abs() < 1e-6);

    let a = events(vec![bounce(json!({"jitter": 0.3}))], 1);
    let b = events(vec![bounce(json!({"jitter": 0.3}))], 2);
    let a2 = events(vec![bounce(json!({"jitter": 0.3}))], 1);
    assert_eq!(a.len(), plain.len());
    assert_eq!(a[0].t, 0.0, "first hit stays at start");
    let mut worst = 0.0f64;
    let mut gap = 0.3;
    let mut base = 0.0;
    for (k, hit) in a.iter().enumerate().skip(1) {
        base += gap;
        let off = (hit.t - base).abs();
        assert!(off <= 0.3 * gap + 1e-6, "hit {k} moved {off} of gap {gap}");
        worst = worst.max(off / gap);
        gap *= 0.6;
    }
    assert!(worst > 0.05, "some hit moved: worst fraction {worst}");
    assert!(
        a.iter().zip(&b).any(|(x, y)| x.t != y.t),
        "seed 1 and 2 differ in timing"
    );
    assert!(
        a.iter()
            .zip(&a2)
            .all(|(x, y)| x.t.to_bits() == y.t.to_bits()),
        "same seed is bit-identical"
    );
    assert!(a.windows(2).all(|w| w[0].t <= w[1].t), "sorted");
    println!(
        "bounce jitter 0.3: {} hits, largest shift {:.3} of a gap (limit 0.3)",
        a.len(),
        worst
    );
}

#[test]
fn bounce_jitter_range_error() {
    let e = error(vec![bounce(json!({"jitter": 0.8}))]);
    assert!(e.contains("jitter") && e.contains("0.5"), "{e}");
}

fn wander(extra: Value) -> Value {
    let mut n = json!({"type": "wander", "name": "w", "rate": 5, "seed": 3});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    n
}

#[test]
fn wander_min_max_equals_range() {
    let a = node_audio([wander(json!({"range": [200, 800]}))], "w", 1.0);
    let b = node_audio([wander(json!({"min": 200, "max": 800}))], "w", 1.0);
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    let (lo, hi) = a
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    assert!(lo >= 200.0 && hi <= 800.0 && hi - lo > 100.0, "{lo}..{hi}");
    println!("wander min/max 200..800: output spans {lo:.1}..{hi:.1}, bit-identical to range");
}

#[test]
fn wander_bad_combinations() {
    let both = error(vec![wander(json!({"range": [0, 1], "min": 0, "max": 1}))]);
    assert!(
        both.contains("range") && both.contains("min") && both.contains("max"),
        "{both}"
    );
    let half = error(vec![wander(json!({"min": 0}))]);
    assert!(half.contains("min") && half.contains("max"), "{half}");
    let half = error(vec![wander(json!({"max": 1}))]);
    assert!(half.contains("min") && half.contains("max"), "{half}");
}

fn ramp(extra: Value) -> Value {
    let mut n = json!({"type": "ramp", "name": "r", "from": 100, "to": 400, "curve": "linear"});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    n
}

#[test]
fn ramp_duration_equals_time() {
    let a = node_audio([ramp(json!({"time": 0.25}))], "r", 0.5);
    let b = node_audio([ramp(json!({"duration": 0.25}))], "r", 0.5);
    assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    let mid = a[(0.125 * SR as f32) as usize];
    assert!((mid - 250.0).abs() < 0.1, "midpoint {mid}");
    println!("ramp duration 0.25 == time 0.25 bit-identical; midpoint {mid:.2} (250)");
}

#[test]
fn ramp_errors_name_fields() {
    let e = error(vec![ramp(json!({"time": 0.2, "duration": 0.2}))]);
    assert!(
        e.contains("time") && e.contains("duration") && e.contains("not both"),
        "{e}"
    );
    let e = error(vec![ramp(json!({}))]);
    assert!(e.contains("time") && e.contains("duration"), "{e}");
    let e = error(vec![ramp(json!({"duration": -1}))]);
    assert!(e.contains("duration"), "{e}");
}
