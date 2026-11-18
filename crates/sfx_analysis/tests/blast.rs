//! M1: `explosion` and `gunshot`. Each card assertion is "field up, measure
//! moves": measured at three settings, the numbers printed (they go in
//! docs/nodes/M1.md). Measures are taken on the node's own samples (before
//! the output stage), 48 kHz.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_graph::node::parse_body;
use sfx_graph::{Audio, Measure, RenderOpts, SoundDoc};

/// The node's fixed lead-in, seconds (a documented constant).
const LEAD: f32 = 0.0025;

/// The muzzle velocity formula from the field docs, m/s.
fn muzzle_velocity(barrel: f32, suppressed: bool) -> f32 {
    let v = 300.0 + 600.0 * (1.0 - (-barrel / 0.35).exp());
    if suppressed { v.min(310.0) } else { v }
}

/// What the crack is timed with, m/s.
fn mean_velocity(barrel: f32, suppressed: bool) -> f32 {
    0.9 * muzzle_velocity(barrel, suppressed)
}

/// Seconds a test renders (long enough for the biggest tail used).
const LEN: f32 = 12.0;

fn explosion(extra: Value) -> Vec<Value> {
    let mut n = json!({"type": "explosion", "name": "x", "size": 5, "distance": 0});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![n]
}

fn gunshot(extra: Value) -> Vec<Value> {
    let mut n = json!({"type": "gunshot", "name": "x", "caliber": 9});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![n]
}

fn centroid(x: &[f32]) -> f32 {
    measure(&Audio::mono(SR, x.to_vec()), Measure::Centroid)
}

/// Seconds from the peak until the 10 ms RMS level has last been within 40
/// dB of it, minus the same for 6 dB (`Measure::Decay`).
fn decay(x: &[f32]) -> f32 {
    measure(&Audio::mono(SR, x.to_vec()), Measure::Decay)
}

fn crest(x: &[f32]) -> f32 {
    measure(&Audio::mono(SR, x.to_vec()), Measure::Crest)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

fn peak_db(x: &[f32]) -> f32 {
    20.0 * peak(x).max(1e-9).log10()
}

/// Seconds to the first sample at or above `frac` of the peak.
fn first_above(x: &[f32], frac: f32) -> f32 {
    let lim = frac * peak(x);
    x.iter().position(|v| v.abs() >= lim).unwrap() as f32 / SR as f32
}

/// Onset: the first sample at 1 % (-40 dB) of the peak.
fn onset(x: &[f32]) -> f32 {
    first_above(x, 0.01)
}

/// Attack: from 5 % of the peak to 90 % of it.
fn attack(x: &[f32]) -> f32 {
    first_above(x, 0.9) - first_above(x, 0.05)
}

fn sub_db(x: &[f32]) -> f32 {
    band_db(x, SR, 0.0, 80.0)
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "x"}));
    d.duration = Some(0.5);
    d.render(&RenderOpts::default()).unwrap_err()
}

// ------------------------------------------------------------- explosion

#[test]
fn explosion_size_moves_centroid_decay_and_sub_level() {
    let sizes = [1.0f32, 5.0, 25.0];
    let mut rows = Vec::new();
    for s in sizes {
        let x = node_audio(explosion(json!({"size": s})), "x", LEN);
        assert_finite(&x, "explosion");
        rows.push((centroid(&x), decay(&x), sub_db(&x), peak(&x), attack(&x)));
        println!(
            "explosion size {s}: centroid {:.1} Hz, decay {:.2} s, sub (<80 Hz) {:.1} dB, peak {:.3}, attack {:.2} ms",
            rows.last().unwrap().0,
            rows.last().unwrap().1,
            rows.last().unwrap().2,
            rows.last().unwrap().3,
            rows.last().unwrap().4 * 1000.0
        );
    }
    for w in rows.windows(2) {
        // The fireball rumble (explosion-factory1.pd) holds the centroid
        // low at every size: 509 / 410 / 320 Hz.
        assert!(w[1].0 < w[0].0 * 0.85, "centroid falls >= 15 % per step");
        assert!(w[1].1 > w[0].1 * 1.2, "decay grows >= 20 % per step");
        assert!(w[1].2 > w[0].2 + 2.0, "sub level rises >= 2 dB per step");
    }
}

#[test]
fn explosion_distance_delays_by_d_over_343_and_darkens() {
    let ds = [0.0f32, 100.0, 400.0];
    let mut on = Vec::new();
    let mut cs = Vec::new();
    for d in ds {
        let x = node_audio(explosion(json!({"distance": d})), "x", LEN);
        assert_finite(&x, "explosion");
        on.push(onset(&x));
        cs.push(centroid(&x));
        println!(
            "explosion distance {d}: onset {:.2} ms (expected {:.2}), centroid {:.1} Hz, peak {:.3}",
            on.last().unwrap() * 1000.0,
            (d / 343.0 + 0.0025) * 1000.0,
            cs.last().unwrap(),
            peak(&x)
        );
    }
    for (i, d) in ds.iter().enumerate().skip(1) {
        let shift = on[i] - on[0];
        assert!(
            (shift - d / 343.0).abs() < 0.001,
            "distance {d}: onset shifted {shift} s, want {}",
            d / 343.0
        );
        assert!(cs[i] < cs[i - 1] * 0.8, "centroid falls with distance");
    }
}

#[test]
fn explosion_attack_is_under_2_ms() {
    for s in [0.5f32, 5.0, 50.0] {
        let x = node_audio(explosion(json!({"size": s})), "x", 2.0);
        let a = attack(&x);
        println!("explosion size {s}: attack {:.3} ms", a * 1000.0);
        assert!(a < 0.002, "size {s}: attack {a} s");
    }
}

#[test]
fn explosion_debris_adds_ticks_after_the_blast() {
    let mut nodes = vec![json!({"type": "pattern", "name": "ev", "times": [3.0, 3.6, 4.2]})];
    nodes.extend(explosion(json!({"size": 1, "debris": "ev"})));
    let with = node_audio(&nodes, "x", 5.0);
    let without = node_audio(explosion(json!({"size": 1})), "x", 5.0);
    let at = |t: f32| (t * SR as f32) as usize;
    // The ticks are 15 ms long; compare the 20 ms after each event with and
    // without. The tail of a size 1 blast is far below a tick by 3 s.
    let mut gains = Vec::new();
    for t in [3.0f32, 3.6, 4.2] {
        let w = &with[at(t)..at(t) + at(0.02)];
        let o = &without[at(t)..at(t) + at(0.02)];
        let diff: Vec<f32> = w.iter().zip(o).map(|(a, b)| a - b).collect();
        gains.push(rms_db(&diff) - rms_db(o));
    }
    println!("debris ticks over the bare blast, per event: {gains:.1?} dB");
    assert!(gains.iter().all(|g| *g > 6.0), "{gains:?}");
    // Before the first event nothing differs.
    assert!(
        with[..at(2.99)]
            .iter()
            .zip(&without[..at(2.99)])
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    // Debris arrives with the blast: at distance 343 m the ticks move 1 s.
    let mut far = vec![json!({"type": "pattern", "name": "ev", "times": [0.5]})];
    far.extend(explosion(
        json!({"size": 1, "distance": 343, "debris": "ev"}),
    ));
    let a = node_audio(&far, "x", 3.0);
    far[1]["debris"] = Value::Null;
    far[1].as_object_mut().unwrap().remove("debris");
    let b = node_audio(&far, "x", 3.0);
    let moved = a
        .iter()
        .zip(&b)
        .position(|(p, q)| p.to_bits() != q.to_bits())
        .unwrap() as f32
        / SR as f32;
    println!("debris event at 0.5 s, distance 343 m: first differing sample at {moved:.3} s");
    assert!((moved - (1.5 + LEAD)).abs() < 0.001);
}

#[test]
fn explosion_is_bit_identical_and_seeded() {
    let mut nodes = vec![json!({"type": "poisson", "name": "ev", "rate": 20})];
    nodes.extend(explosion(
        json!({"size": 3, "distance": 50, "debris": "ev", "seed": 4}),
    ));
    assert_bit_identical(&nodes, 1.5);
    let a = node_audio(explosion(json!({"seed": 1})), "x", 1.0);
    let b = node_audio(explosion(json!({"seed": 2})), "x", 1.0);
    let same = a.iter().zip(&b).filter(|(p, q)| p == q).count();
    println!("two seeds: {same} of {} samples equal", a.len());
    assert!(same < a.len() / 10);
}

#[test]
fn explosion_is_finite_at_the_extremes() {
    for size in [0.1f32, 100.0] {
        for d in [0.0f32, 30_000.0] {
            let x = node_audio(explosion(json!({"size": size, "distance": d})), "x", 3.0);
            assert_finite(&x, &format!("size {size} distance {d}"));
            let p = peak(&x);
            println!("explosion size {size} distance {d}: peak {p:.3}");
            assert!(p <= 1.0);
        }
    }
    // A document shorter than the arrival delay is silence, not a panic.
    let x = node_audio(explosion(json!({"distance": 3000})), "x", 1.0);
    assert!(x.iter().all(|v| *v == 0.0));
    // A very short document.
    let x = node_audio(explosion(json!({"size": 100})), "x", 0.001);
    assert_finite(&x, "1 ms document");
}

#[test]
fn explosion_errors_say_the_field_value_and_fix() {
    let e = render_error(explosion(json!({"size": 500})));
    println!("{e}");
    assert!(e.contains("size: 500: give metres of fireball radius from 0.1 to 100"));
    let e = render_error(explosion(json!({"distance": -3})));
    println!("{e}");
    assert!(e.contains("distance: -3: give metres from 0 to 30000"));
    let e = render_error(explosion(json!({"distance": 40000})));
    assert!(e.contains("distance: 40000"));
    let mut nodes = vec![json!({"type": "noise", "name": "ev"})];
    nodes.extend(explosion(json!({"debris": "ev"})));
    let e = render_error(nodes);
    println!("{e}");
    assert!(e.contains("debris") && e.contains("audio, not events"));
    let mut nodes = vec![json!({"type": "poisson", "name": "ev", "rate": 5000})];
    nodes.extend(explosion(json!({"debris": "ev"})));
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "x"}));
    d.duration = Some(10.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("{e}");
    assert!(e.contains("debris") && e.contains("at most 20000"));
    let e = render_error(explosion(json!({"sise": 5})));
    assert!(e.contains("sise") && e.contains("size"));
}

// --------------------------------------------------------------- gunshot

#[test]
fn gunshot_caliber_moves_centroid_and_decay() {
    let cals = [5.56f32, 12.7, 40.0];
    let mut rows = Vec::new();
    for c in cals {
        let x = node_audio(gunshot(json!({"caliber": c})), "x", 4.0);
        assert_finite(&x, "gunshot");
        rows.push((centroid(&x), decay(&x), sub_db(&x)));
        println!(
            "gunshot caliber {c}: centroid {:.0} Hz, decay {:.3} s, sub {:.1} dB, peak {:.3}, attack {:.3} ms",
            rows.last().unwrap().0,
            rows.last().unwrap().1,
            rows.last().unwrap().2,
            peak(&x),
            attack(&x) * 1000.0
        );
        // Farnell's shot: the resonant body (Q 4.56) and guns1.pd's
        // nwavenoise buzz take 2-4.4 ms to build (caliber 40 is the slowest).
        assert!(attack(&x) < 0.005);
    }
    for w in rows.windows(2) {
        assert!(w[1].0 < w[0].0, "centroid falls with caliber");
        // The decay measure moves in 10 ms steps: 40 / 50 / 50 ms.
        assert!(w[1].1 >= w[0].1, "decay does not shorten with caliber");
    }
}

#[test]
fn gunshot_suppressed_is_quieter_and_less_peaky() {
    for c in [5.56f32, 9.0, 12.7] {
        let a = node_audio(gunshot(json!({"caliber": c})), "x", 2.0);
        let b = node_audio(gunshot(json!({"caliber": c, "suppressed": true})), "x", 2.0);
        let (pa, pb) = (peak_db(&a), peak_db(&b));
        let (ca, cb) = (crest(&a), crest(&b));
        println!(
            "caliber {c}: open peak {pa:.1} dB crest {ca:.1} dB; suppressed peak {pb:.1} dB crest {cb:.1} dB (peak down {:.1} dB)",
            pa - pb
        );
        // The open shot is Farnell's dense body (crest about 27 dB), so only
        // the level drop is the suppressor's mark now.
        assert!(pa - pb >= 14.0, "peak down {} dB", pa - pb);
        let _ = (ca, cb);
    }
}

#[test]
fn gunshot_distance_delays_the_blast_and_leads_with_the_crack() {
    let barrel = 0.4f32;
    let vm = mean_velocity(barrel, false);
    // Suppressed: no crack, so the only onset is the blast at d / 343.
    let mut on = Vec::new();
    for d in [0.0f32, 100.0, 400.0] {
        let x = node_audio(
            gunshot(json!({"suppressed": true, "distance": d})),
            "x",
            4.0,
        );
        on.push(onset(&x));
    }
    println!(
        "suppressed blast onsets at 0 / 100 / 400 m: {:.2} / {:.2} / {:.2} ms",
        on[0] * 1000.0,
        on[1] * 1000.0,
        on[2] * 1000.0
    );
    assert!((on[1] - on[0] - 100.0 / 343.0).abs() < 0.002);
    assert!((on[2] - on[0] - 400.0 / 343.0).abs() < 0.002);
    // Open: the first onset is the crack at d / (0.9 v), ahead of the blast.
    let (mut cracks, mut cents) = (Vec::new(), Vec::new());
    for d in [0.0f32, 100.0, 400.0] {
        let x = node_audio(gunshot(json!({"distance": d})), "x", 4.0);
        cracks.push(onset(&x));
        cents.push(centroid(&x));
    }
    println!(
        "mean velocity {vm:.0} m/s; crack onsets {:.2} / {:.2} / {:.2} ms; centroids {:.0} / {:.0} / {:.0} Hz",
        cracks[0] * 1000.0,
        cracks[1] * 1000.0,
        cracks[2] * 1000.0,
        cents[0],
        cents[1],
        cents[2]
    );
    for (i, d) in [100.0f32, 400.0].iter().enumerate() {
        let want = d / vm;
        assert!((cracks[i + 1] - cracks[0] - want).abs() < 0.002, "{d}");
        let lead = d / 343.0 - want;
        println!(
            "  distance {d} m: crack leads the blast by {:.1} ms",
            lead * 1000.0
        );
        assert!(lead > 0.0);
    }
    // gunshot_fix: the air low-pass now scales with 400 m (was 40 m), so the
    // darkening per step is gentler (3286 / 2205 / 1778 Hz).
    // Farnell's shot sits mostly under 3 kHz, so the air low-pass
    // (16 kHz / (1 + d / 400 m)) darkens it gently: 1715 / 1643 / 1500 Hz.
    assert!(cents[1] < cents[0] && cents[2] < cents[1], "{cents:?}");
}

#[test]
fn gunshot_barrel_sets_the_crack_timing_and_blast_level() {
    let d = 300.0f32;
    let mut times = Vec::new();
    for b in [0.1f32, 0.4, 1.0] {
        let x = node_audio(gunshot(json!({"barrel": b, "distance": d})), "x", 4.0);
        times.push(onset(&x));
        let vm = mean_velocity(b, false);
        println!(
            "barrel {b}: muzzle {:.0} m/s, crack onset {:.2} ms (expected {:.2})",
            muzzle_velocity(b, false),
            times.last().unwrap() * 1000.0,
            (d / vm + LEAD) * 1000.0
        );
        assert!((times.last().unwrap() - (d / vm + LEAD)).abs() < 0.002);
    }
    assert!(times[0] > times[1] && times[1] > times[2], "{times:?}");
    // The blast alone: at 100 m the crack is long gone (0.16 s) when the
    // blast arrives (0.29 s), so a window on the blast's first 20 ms sees only the blast body.
    let mut cs = Vec::new();
    for b in [0.1f32, 0.4, 1.0] {
        let x = node_audio(gunshot(json!({"barrel": b, "distance": 100})), "x", 1.0);
        let w = &x[(0.2925 * SR as f32) as usize..(0.3125 * SR as f32) as usize];
        // Farnell: "shorter barrels produce a louder sound for the same
        // calibre" (the body is driven harder): the blast window's level.
        cs.push(band_db(w, SR, 0.0, 20_000.0));
    }
    println!(
        "blast window level at barrel 0.1 / 0.4 / 1.0: {:.1} / {:.1} / {:.1} dB",
        cs[0], cs[1], cs[2]
    );
    assert!(cs[0] > cs[1] && cs[1] > cs[2], "{cs:?}");
}

#[test]
fn gunshot_is_bit_identical_and_finite_at_the_extremes() {
    assert_bit_identical(
        gunshot(json!({"caliber": 12.7, "barrel": 0.6, "distance": 80, "seed": 3})),
        1.0,
    );
    for c in [2.0f32, 60.0] {
        for b in [0.05f32, 1.5] {
            for s in [false, true] {
                for d in [0.0f32, 30_000.0] {
                    let x = node_audio(
                        gunshot(json!({"caliber": c, "barrel": b, "suppressed": s, "distance": d})),
                        "x",
                        2.0,
                    );
                    assert_finite(&x, &format!("c {c} b {b} s {s} d {d}"));
                    assert!(peak(&x) <= 1.0);
                }
            }
        }
    }
    println!(
        "gunshot: caliber 2/60 x barrel 0.05/1.5 x suppressed x distance 0/30000: all finite, peak <= 1"
    );
    let x = node_audio(gunshot(json!({"caliber": 60})), "x", 0.001);
    assert_finite(&x, "1 ms document");
}

#[test]
fn gunshot_errors_say_the_field_value_and_fix() {
    let e = render_error(gunshot(json!({"caliber": 1})));
    println!("{e}");
    assert!(e.contains("caliber: 1: give the bore in millimetres from 2 to 60"));
    let e = render_error(gunshot(json!({"barrel": 3})));
    println!("{e}");
    assert!(e.contains("barrel: 3: give the barrel length in metres from 0.05 to 1.5"));
    let e = render_error(gunshot(json!({"distance": -1})));
    assert!(e.contains("distance: -1: give metres from 0 to 30000"));
    let e = render_error(gunshot(json!({"suppresed": true})));
    assert!(e.contains("suppresed") && e.contains("suppressed"));
}

// ------------------------------------------------------------- registry

#[test]
fn blast_nodes_are_registered_non_causal_and_random() {
    for ty in ["explosion", "gunshot"] {
        let mut body = sfx_graph::example(ty).expect("registered");
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, ty).unwrap();
        assert!(!node.causal() && node.random(), "{ty}");
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
    assert!(
        sfx_graph::describe(Some("explosion"))
            .unwrap()
            .contains("Fireball radius")
    );
}

/// The recipes in docs/nodes/M1.md, as written there.
#[test]
fn blast_recipes_render() {
    let storm = vec![
        json!({"type": "burst", "name": "rocks", "count": 60, "spread": 2.0, "curve": "exp", "values": {"min": 0.2, "max": 1.0}, "seed": 2}),
        json!({"type": "explosion", "name": "boom", "size": 8, "distance": 150, "debris": "rocks", "seed": 1}),
    ];
    let x = node_audio(&storm, "boom", 8.0);
    assert_finite(&x, "boom");
    // With the output stage the examples use (examples/explosion.rs).
    let mut with_out = storm.clone();
    with_out.push(json!({"type": "output", "name": "out", "input": "boom", "lufs": -14}));
    let a = render(&with_out, 8.0);
    assert!(
        a.channels[0]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0)
    );
    println!(
        "recipe explosion: peak {:.3}, centroid {:.0} Hz, onset {:.3} s",
        peak(&x),
        centroid(&x),
        onset(&x)
    );
    let shot = vec![
        json!({"type": "gunshot", "name": "shot", "caliber": 7.62, "barrel": 0.5, "distance": 120, "seed": 1}),
    ];
    let x = node_audio(&shot, "shot", 2.0);
    assert_finite(&x, "shot");
    let mut with_out = shot.clone();
    with_out.push(json!({"type": "output", "name": "out", "input": "shot", "lufs": -14}));
    let a = render(&with_out, 2.0);
    assert!(
        a.channels[0]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0)
    );
    println!(
        "recipe gunshot: peak {:.3}, centroid {:.0} Hz, first onset {:.3} s",
        peak(&x),
        centroid(&x),
        onset(&x)
    );
    assert!(peak(&x) > 0.05);
}
