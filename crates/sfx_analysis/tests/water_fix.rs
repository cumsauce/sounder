//! water_fix: the batch-2 blind labels heard `water`'s hiss as wind and
//! `splash` as a "drop kick" / "weird crunchies". These tests pin the
//! measurable causes going away (numbers in docs/nodes/water_fix.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn mono(x: &[f32]) -> sfx_graph::Audio {
    sfx_graph::Audio {
        sample_rate: SR,
        channels: vec![x.to_vec()],
    }
}

fn one(kind: &str, fields: Value) -> Vec<Value> {
    let mut n = json!({"type": kind, "name": "n"});
    n.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    vec![n]
}

/// Energy share of `x` between `lo` and `hi` Hz (0..1), from `band_db`.
fn share(x: &[f32], lo: f32, hi: f32) -> f32 {
    let p = |a: f32, b: f32| 10f32.powf(band_db(x, SR, a, b) / 10.0);
    p(lo.max(1.0), hi) / p(1.0, SR as f32 / 2.0).max(1e-30)
}

/// Coefficient of variation of the 10 ms RMS envelope of `x` above `hz`
/// (a second-order high-pass by difference twice): a steady hiss reads low,
/// a hiss that comes and goes with the bubbles reads high.
fn hf_env_cv(x: &[f32], hz: f32) -> f32 {
    // One-pole high-passes, twice.
    let a = (-std::f32::consts::TAU * hz / SR as f32).exp();
    let mut y = x.to_vec();
    for _ in 0..2 {
        let (mut px, mut py) = (0.0f32, 0.0f32);
        for v in &mut y {
            let o = a * (py + *v - px);
            px = *v;
            py = o;
            *v = o;
        }
    }
    let w = (SR / 100) as usize;
    let rms: Vec<f32> = y
        .chunks(w)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let m = rms.iter().sum::<f32>() / rms.len() as f32;
    let var = rms.iter().map(|r| (r - m).powi(2)).sum::<f32>() / rms.len() as f32;
    var.sqrt() / m.max(1e-30)
}

#[test]
fn water_hiss_follows_the_bubbles() {
    let mut bad = Vec::new();
    // The labelled graph: flow 0.5, depth 0.5, density 300.
    for (flow, density, before) in [
        (0.5, 300.0, 0.254),
        (1.0, 300.0, 0.060),
        (0.8, 600.0, 0.112),
    ] {
        let x = node_audio(
            one(
                "water",
                json!({"flow": flow, "density": density, "seed": 1}),
            ),
            "n",
            3.0,
        );
        assert_finite(&x, "water");
        let d = describe(&mono(&x));
        let cv = hf_env_cv(&x, 3000.0);
        let low_mid = share(&x, 300.0, 1200.0);
        println!(
            "water flow {flow} density {density}: hf envelope cv {cv:.3}, 300-1200 Hz share {:.3}, flatness {:.3}, centroid {:.0} Hz, fine onsets/s {:.1}",
            low_mid,
            d.flatness_all,
            d.centroid_all_hz,
            d.fine_onsets.len() as f32 / d.seconds
        );
        // Before the fix the hiss was a steady bed (cv `before`); it now
        // rides the pops. Round 1 had 1.6x and was heard as "crispy, like
        // boiling oil"; round 2 moves it halfway back (a 45 % steady share):
        // at least 1.2x as much envelope movement.
        if cv <= 1.2 * before {
            bad.push((flow, density, cv, before));
        }
    }
    assert!(
        bad.is_empty(),
        "hiss above 3 kHz is still a steady bed (flow, density, cv, before): {bad:?}"
    );
}

#[test]
fn splash_has_no_kick_and_a_watery_slap() {
    let mut bad = Vec::new();
    for size in [0.01, 0.05, 0.15, 0.3, 0.6] {
        let x = node_audio(
            one("splash", json!({"size": size, "at": 0.0, "seed": 2})),
            "n",
            2.5,
        );
        assert_finite(&x, "splash");
        let d = describe(&mono(&x));
        let first = &x[..(0.02 * SR as f32) as usize];
        let thump = share(&x, 0.0, 250.0);
        let slap = share(first, 1000.0, 6000.0);
        let hp_first = peak_hz(first, SR);
        // Where the sample peak is (the slap must be the loudest instant).
        let peak_s = x
            .iter()
            .enumerate()
            .fold(
                (0, 0.0f32),
                |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
            )
            .0 as f32
            / SR as f32;
        println!(
            "splash {size}: <250 Hz share {thump:.3}, first 20 ms 1-6 kHz share {slap:.3} (peak {hp_first:.0} Hz), voiced {:.2}, centroid {:.0} Hz, sample peak at {peak_s:.4} s, loudest 10 ms at {:.3} s, fine onsets {}",
            d.voiced,
            d.centroid_all_hz,
            d.peak_t,
            d.fine_onsets.len()
        );
        // Book check 1: the first 20 ms are the slap (3.5 kHz) over the
        // impact bump, so 1-6 kHz holds at least 0.35 of them at every size.
        let min_slap = 0.35;
        if thump >= 0.05 || slap <= min_slap || peak_s >= 0.03 {
            bad.push(size);
        }
    }
    assert!(
        bad.is_empty(),
        "splash sizes with a thump, no slap or a late peak: {bad:?}"
    );
}

#[test]
fn splash_and_water_are_deterministic() {
    assert_bit_identical(one("splash", json!({"size": 0.2, "seed": 4})), 1.5);
    assert_bit_identical(
        one("water", json!({"flow": 0.6, "splash": 0.5, "seed": 4})),
        2.0,
    );
}
