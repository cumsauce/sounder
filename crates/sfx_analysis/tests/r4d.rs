//! R4-D: `splash` `tail` / `burst`, `thunder` `tail`, `water` `tone`.
//! Each test prints its measurement (these lines go in docs/nodes/R4-D.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, describe, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.to_bits() == q.to_bits())
}

fn node(kind: &str, base: Value, extra: Value) -> Vec<Value> {
    let mut n = json!({"type": kind, "name": "n"});
    let o = n.as_object_mut().unwrap();
    o.extend(base.as_object().unwrap().clone());
    o.extend(extra.as_object().unwrap().clone());
    vec![n]
}

fn mono(x: &[f32]) -> sfx_graph::Audio {
    sfx_graph::Audio {
        sample_rate: SR,
        channels: vec![x.to_vec()],
    }
}

/// Seconds from the first sound until the 10 ms RMS envelope is under `db`
/// dBFS for good.
fn audible_s(x: &[f32], db: f32) -> f32 {
    let w = (SR / 100) as usize;
    let floor = 10f32.powf(db / 20.0);
    let rms: Vec<f32> = x
        .chunks(w)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let first = rms.iter().position(|v| *v > 0.0).unwrap_or(0);
    let last = rms.iter().rposition(|v| *v > floor).unwrap_or(first);
    last.saturating_sub(first) as f32 * 0.01
}

// ---------------------------------------------------------------- splash

fn splash(extra: Value) -> Vec<Value> {
    node("splash", json!({"size": 0.15, "seed": 3}), extra)
}

#[test]
fn splash_defaults_are_bit_identical() {
    for size in [0.02, 0.15, 0.6] {
        let base = json!({"size": size, "seed": 3});
        let a = node_audio(node("splash", base.clone(), json!({})), "n", 3.0);
        let b = node_audio(
            node("splash", base, json!({"tail": 1.0, "burst": 1.0})),
            "n",
            3.0,
        );
        assert!(same_bits(&a, &b), "size {size}");
    }
}

#[test]
fn splash_tail_and_burst_reshape_it() {
    let plain = node_audio(splash(json!({})), "n", 3.0);
    let long = node_audio(splash(json!({"tail": 2.0, "burst": 2.0})), "n", 3.0);
    assert_finite(&long, "splash");
    let first_20ms = (0.02 * SR as f32) as usize;
    let peak_at = |x: &[f32]| {
        x.iter()
            .enumerate()
            .fold(
                (0, 0.0f32),
                |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
            )
            .0
    };
    let (p_plain, p_long) = (peak_at(&plain), peak_at(&long));
    let d = describe(&mono(&long));
    let after = d.fine_onsets.iter().filter(|t| **t > 0.2).count();
    let d0 = describe(&mono(&plain));
    let after0 = d0.fine_onsets.iter().filter(|t| **t > 0.2).count();
    println!(
        "0.15 m splash: default peaks at {:.1} ms, {after0} fine onsets after 0.2 s; tail 2 burst 2 peaks at {:.1} ms, {after} fine onsets after 0.2 s, length {:.2} s vs {:.2} s",
        p_plain as f32 * 1000.0 / SR as f32,
        p_long as f32 * 1000.0 / SR as f32,
        long.len() as f32 / SR as f32,
        plain.len() as f32 / SR as f32,
    );
    assert!(p_long < first_20ms, "peak at sample {p_long}");
    assert!(after >= 10, "{after} fine onsets after 0.2 s");
}

#[test]
fn splash_tail_lengthens_the_cloud_monotonically() {
    let mut onsets = Vec::new();
    for tail in [0.5f32, 1.0, 2.0] {
        let x = node_audio(splash(json!({"tail": tail})), "n", 3.0);
        let d = describe(&mono(&x));
        let n = d.fine_onsets.iter().filter(|t| **t > 0.1).count();
        let last = d.fine_onsets.last().copied().unwrap_or(0.0);
        println!("tail {tail}: {n} fine onsets after 0.1 s, last at {last:.2} s");
        onsets.push(last);
    }
    assert!(onsets[0] < onsets[1] && onsets[1] < onsets[2], "{onsets:?}");
}

#[test]
fn splash_burst_moves_the_peak_into_the_opening() {
    let level_20ms = |burst: f32| {
        let x = node_audio(splash(json!({"burst": burst})), "n", 3.0);
        assert_finite(&x, "splash");
        let n = (0.02 * SR as f32) as usize;
        let head = x[..n].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let rest = x[n..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        (head, rest)
    };
    let (h0, r0) = level_20ms(0.0);
    let (h1, r1) = level_20ms(1.0);
    let (h2, r2) = level_20ms(2.0);
    println!(
        "burst 0 / 1 / 2: peak in first 20 ms {h0:.3} / {h1:.3} / {h2:.3}, after {r0:.3} / {r1:.3} / {r2:.3}"
    );
    assert!(h0 < h1 && h1 < h2);
    assert!(h2 > r2, "burst 2 is the loudest moment");
}

#[test]
fn splash_new_fields_are_checked_and_finite_at_the_extremes() {
    for extra in [
        json!({"tail": 0.0}),
        json!({"tail": 3.0, "burst": 2.0}),
        json!({"burst": 0.0, "tail": 0.0}),
    ] {
        let x = node_audio(splash(extra.clone()), "n", 3.0);
        assert_finite(&x, &extra.to_string());
    }
    for bad in [
        json!({"tail": 3.5}),
        json!({"burst": 2.5}),
        json!({"tail": -1}),
    ] {
        let mut d = SoundDoc::named("t");
        d.nodes = splash(bad);
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(1.0);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        println!("error: {e}");
        assert!(e.contains("give 0.."), "{e}");
    }
}

// --------------------------------------------------------------- thunder

fn thunder(extra: Value) -> Vec<f32> {
    node_audio(
        node("thunder", json!({"distance": 1500, "seed": 3}), extra),
        "n",
        16.0,
    )
}

#[test]
fn thunder_tail_off_is_bit_identical() {
    let a = thunder(json!({}));
    let b = thunder(json!({"tail": 0.0}));
    assert!(same_bits(&a, &b));
}

#[test]
fn thunder_tail_shortens_the_decay_monotonically() {
    let mut t20 = Vec::new();
    for tail in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let x = thunder(json!({"tail": tail}));
        assert_finite(&x, "thunder");
        let a = x.iter().position(|v| *v != 0.0).unwrap();
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let end = x.iter().rposition(|v| v.abs() > 1e-3 * peak).unwrap();
        let desc = describe(&mono(&x));
        let d = desc.decay20_s.unwrap_or(0.0);
        let d60 = desc.decay60_s.unwrap_or(0.0);
        println!(
            "tail {tail}: first sound {:.2} s, last sample over -60 dB at {:.2} s, listen decay -20 dB {d:.2} s, -60 dB {d60:.2} s, audible (10 ms rms over -45 dBFS) {:.2} s, peak {:.2} dB",
            a as f32 / SR as f32,
            end as f32 / SR as f32,
            audible_s(&x, -45.0),
            20.0 * peak.log10()
        );
        t20.push(audible_s(&x, -45.0));
    }
    // Foley's tail is a string of separate spikes, so the audible end moves
    // in steps (a spike either clears the floor or not): non-increasing,
    // and well shorter at the top.
    assert!(t20.windows(2).all(|w| w[1] <= w[0]), "{t20:?}");
    assert!(t20[4] + 2.0 < t20[0], "{t20:?}");
}

#[test]
fn thunder_tail_bounds() {
    let mut d = SoundDoc::named("t");
    d.nodes = node("thunder", json!({}), json!({"tail": 1.5}));
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(8.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("error: {e}");
    assert!(e.contains("tail"));
}

// ----------------------------------------------------------------- water

fn water(extra: Value) -> Vec<Value> {
    node("water", json!({"flow": 0.5, "seed": 3}), extra)
}

#[test]
fn water_tone_default_is_bit_identical() {
    for looped in [false, true] {
        let mut d = SoundDoc::named("w");
        d.nodes = water(json!({"loop": looped}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(2.0);
        let a = node_audio(water(json!({"loop": looped})), "n", 2.0);
        let b = node_audio(water(json!({"loop": looped, "tone": 0.5})), "n", 2.0);
        assert!(same_bits(&a, &b), "loop {looped}");
    }
}

fn share_above(x: &[f32], hz: f32) -> f32 {
    let hi = 10f32.powf(band_db(x, SR, hz, SR as f32 / 2.0) / 10.0);
    let all = 10f32.powf(band_db(x, SR, 0.0, SR as f32 / 2.0) / 10.0);
    hi / all
}

#[test]
fn water_tone_tilts_the_spectrum_above_2k() {
    let mut shares = Vec::new();
    for tone in [0.0f32, 0.5, 1.0] {
        let x = node_audio(water(json!({"tone": tone})), "n", 4.0);
        assert_finite(&x, "water");
        shares.push(share_above(&x, 2000.0));
    }
    let db = |a: f32, b: f32| 10.0 * (a / b).log10();
    println!(
        "tone 0 / 0.5 / 1: share above 2 kHz {:.3} / {:.3} / {:.3}; tone 1 vs 0.5 {:+.1} dB, tone 0 vs 0.5 {:+.1} dB",
        shares[0],
        shares[1],
        shares[2],
        db(shares[2], shares[1]),
        db(shares[0], shares[1])
    );
    assert!(shares[0] < shares[1] && shares[1] < shares[2]);
    // The share is relative to the total, which rises with the band (so
    // tone 1 against 0.5 cannot gain 6 dB of share from a 0.23 start): the
    // whole range 0..1 does, and the band level itself is checked below.
    assert!(db(shares[2], shares[0]) >= 6.0);
}

#[test]
fn water_tone_raises_the_band_level_by_six_db() {
    let lo = node_audio(water(json!({"tone": 0.5})), "n", 4.0);
    let hi = node_audio(water(json!({"tone": 1.0})), "n", 4.0);
    let up = band_db(&hi, SR, 2000.0, 20_000.0) - band_db(&lo, SR, 2000.0, 20_000.0);
    let low_move = band_db(&hi, SR, 50.0, 500.0) - band_db(&lo, SR, 50.0, 500.0);
    println!("tone 1 vs 0.5: band 2-20 kHz {up:+.1} dB, band 50-500 Hz {low_move:+.1} dB");
    assert!(up >= 6.0, "{up}");
    assert!(low_move.abs() < 1.0, "{low_move}");
}

#[test]
fn water_tone_keeps_loop_seam_at_seeds_1_to_4() {
    for tone in [0.0f32, 1.0] {
        for seed in 1..=4u64 {
            let mut d = SoundDoc::named("loop");
            d.seed = seed;
            d.nodes = water(json!({"tone": tone}));
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "n", "lufs": -20}));
            d.kind = Kind::Loop;
            d.duration = Some(3.0);
            let opts = RenderOpts {
                cached: false,
                ..RenderOpts::default()
            };
            let rendered = d.render(&opts).unwrap();
            let checks = run(
                &Expect {
                    loop_seam: Some(true),
                    ..Expect::default()
                },
                &CheckCx {
                    doc: &d,
                    rendered: &rendered,
                    opts: &opts,
                },
            )
            .unwrap();
            let text = report(&checks);
            println!(
                "tone {tone} seed {seed}: {}",
                text.lines().next().unwrap_or("")
            );
            assert!(
                checks.iter().all(|c| c.pass),
                "tone {tone} seed {seed}: {text}"
            );
        }
    }
}

#[test]
fn water_tone_bounds() {
    let mut d = SoundDoc::named("t");
    d.nodes = water(json!({"tone": 1.5}));
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(1.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("error: {e}");
    assert!(e.contains("tone"));
}
