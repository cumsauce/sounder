//! The creature nodes ported from Farnell's Pd patches: `roar`
//! (MAMMALS/roar.pd), `fly` (INSECTS/buzzing-housefly.pd, flywing.pd) and
//! `cicada` (INSECTS/cicada1-3.pd): what each patch does, measured on the
//! node (docs/nodes/farnell_creatures.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn render_error(nodes: Vec<Value>) -> String {
    crate::common::render_error(nodes, "n", 0.5)
}

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn roar(extra: Value) -> Vec<f32> {
    node_audio(one("roar", extra), "n", 2.2)
}

/// Seconds of the first and last samples over -40 dB of the peak.
fn active(x: &[f32]) -> (f32, f32) {
    let thr = peak(x) * 0.01;
    let first = x.iter().position(|v| v.abs() > thr).unwrap_or(0);
    let last = x.iter().rposition(|v| v.abs() > thr).unwrap_or(0);
    (first as f32 / SR as f32, last as f32 / SR as f32)
}

#[test]
fn roar_swells_out_of_silence_and_ends_at_its_length() {
    let x = roar(json!({}));
    assert_finite(&x, "roar");
    println!("roar peak {:.1} dB", peak_db(&x));
    let (a, b) = active(&x);
    println!("roar active {a:.2}-{b:.2} s");
    // The loudness is sqrt(2 (line - 0.5)): it starts at `at`, peaks where
    // the rise turns to the fall (0.6 x length) and is gone by `length`.
    assert!(a < 0.15, "{a}");
    assert!(b < 1.0 && b > 0.85, "{b}");
    assert!(x[(1.02 * SR as f32) as usize..].iter().all(|v| *v == 0.0));
    let pk = x
        .iter()
        .enumerate()
        .max_by(|p, q| p.1.abs().total_cmp(&q.1.abs()))
        .unwrap()
        .0 as f32
        / SR as f32;
    println!("roar peak at {pk:.2} s");
    assert!((pk - 0.6).abs() < 0.2, "{pk}");
    let long = roar(json!({"length": 2.0}));
    let (a2, b2) = active(&long);
    println!("roar length 2 active {a2:.2}-{b2:.2} s");
    assert!(b2 > b + 0.8 && b2 < 2.0);
    let late = roar(json!({"at": 0.5}));
    assert!(late[..(0.5 * SR as f32) as usize].iter().all(|v| *v == 0.0));
    assert!(peak(&late) > 0.1);
}

#[test]
fn roar_pitch_and_formants_fall_with_size() {
    let c = |size: f32| {
        let x = roar(json!({"size": size}));
        let d = describe(&mono_audio(&x));
        println!("roar size {size}: centroid {:.0} Hz", d.centroid_all_hz);
        d.centroid_all_hz
    };
    let (small, big) = (c(0.5), c(2.0));
    assert!(small > big * 1.4, "{small} {big}");
    // The cord frequency is base + line x range = 60.7 + 65 x ~0.75 Hz at
    // size 1: the lowest strong line sits near 110 Hz and halves at size 2.
    let lo = |size: f32| {
        let x = roar(json!({"size": size}));
        let (a, b) = active(&x);
        let mid = &x[(a * SR as f32) as usize..(b * SR as f32) as usize];
        let (hz, _) = peaks(mid, SR, 1, 30.0)[0];
        let v = peaks(mid, SR, 12, 20.0)
            .into_iter()
            .map(|p| p.0)
            .filter(|h| *h > 40.0)
            .fold(f32::MAX, f32::min);
        println!("roar size {size}: strongest {hz:.0} Hz, lowest line {v:.0} Hz");
        v
    };
    assert!(lo(2.0) < lo(1.0) * 0.8);
}

#[test]
fn roar_aggression_adds_noise_and_roughness() {
    let f = |ag: f32| {
        let x = roar(json!({"aggression": ag, "seed": 1}));
        let d = describe(&mono_audio(&x));
        println!(
            "roar aggression {ag}: flatness {:.3} centroid {:.0} Hz",
            d.flatness_all, d.centroid_all_hz
        );
        d
    };
    let (calm, mean) = (f(0.1), f(0.9));
    assert!(mean.flatness_all > calm.flatness_all, "flatness");
}

#[test]
fn roar_vowels_put_energy_at_their_formants() {
    let band = |v: &str| {
        let x = roar(json!({"vowel": v}));
        assert_finite(&x, v);
        (
            band_db(&x, SR, 600.0, 800.0),
            band_db(&x, SR, 2200.0, 3000.0),
        )
    };
    let (a_lo, a_hi) = band("A"); // F1 710
    let (i_lo, i_hi) = band("i"); // F1 280, F2 2250, F3 2900
    println!("A: {a_lo:.1} / {a_hi:.1} dB, i: {i_lo:.1} / {i_hi:.1} dB");
    assert!(a_lo - a_hi > i_lo - i_hi + 3.0);
    let comb = roar(json!({}));
    let vow = roar(json!({"vowel": "roar"}));
    assert_eq!(fnv(&comb), fnv(&vow), "roar is the default throat");
}

#[test]
fn fly_buzzes_at_its_wingbeat_with_a_resonance_ripple() {
    let x = node_audio(one("fly", json!({"flutter": 0})), "n", 1.0);
    assert_finite(&x, "fly");
    println!("fly peak {:.1} dB rms {:.1} dB", peak_db(&x), rms_db(&x));
    // With no flutter the wings sit at exactly 220 Hz: lines on its
    // harmonics, and the downstroke ripple at res x 2.4 x 220 = 2640 Hz.
    let lines = peaks(&x, SR, 6, 25.0);
    println!("fly lines {lines:?}");
    for (hz, _) in &lines {
        let k = hz / 220.0;
        assert!((k - k.round()).abs() < 0.03 * k.max(1.0), "{hz} Hz");
    }
    let ripple = band_db(&x, SR, 2300.0, 3000.0);
    let mid = band_db(&x, SR, 3600.0, 4300.0);
    println!("fly 2.3-3 kHz {ripple:.1} dB, 3.6-4.3 kHz {mid:.1} dB");
    assert!(ripple > mid + 3.0);
    // High-passed at 700 Hz: little below the first lines.
    assert!(band_db(&x, SR, 20.0, 120.0) < band_db(&x, SR, 700.0, 4000.0) - 20.0);
    let hi = node_audio(one("fly", json!({"flutter": 0, "freq": 330})), "n", 1.0);
    let d = peaks(&hi, SR, 3, 20.0);
    assert!(
        d.iter()
            .any(|(h, _)| (h / 330.0 - (h / 330.0).round()).abs() < 0.03)
    );
}

#[test]
fn fly_wanders_a_few_hz_and_its_seed_matters() {
    let a = node_audio(one("fly", json!({"seed": 1})), "n", 3.0);
    let b = node_audio(one("fly", json!({"seed": 2})), "n", 3.0);
    assert_ne!(fnv(&a), fnv(&b));
    let steady = node_audio(one("fly", json!({"flutter": 0})), "n", 3.0);
    let fp = |x: &[f32]| -> Vec<f32> {
        let d = describe(&mono_audio(x));
        d.pitch_tenths.iter().flatten().copied().collect()
    };
    println!("fly pitch by tenth {:?}", fp(&a));
    println!("fly steady pitch by tenth {:?}", fp(&steady));
    let table = node_audio(one("fly", json!({"wave": "table", "flutter": 0})), "n", 1.0);
    assert_finite(&table, "fly table");
    println!("fly table peak {:.1} dB", peak_db(&table));
    assert!(peak(&table) > 0.05);
    assert!(peak_hz(&table, SR) < 800.0, "the table is a low buzz");
}

#[test]
fn cicada_buzz_lives_in_the_5_to_8_khz_band_with_a_90_hz_pulse() {
    let x = node_audio(one("cicada", json!({})), "n", 2.0);
    assert_finite(&x, "cicada");
    let d = describe(&mono_audio(&x));
    println!(
        "cicada buzz peak {:.1} dB, centroid {:.0} Hz, energy shares {:?}",
        peak_db(&x),
        d.centroid_all_hz,
        d.band_shares
    );
    let band = band_db(&x, SR, 4000.0, 9000.0);
    let low = band_db(&x, SR, 20.0, 2000.0);
    println!("cicada 4-9 kHz {band:.1} dB, under 2 kHz {low:.1} dB");
    assert!(band > low + 15.0);
    assert!((4500.0..8500.0).contains(&d.centroid_all_hz));
    let hi = node_audio(one("cicada", json!({"pitch": 1.5})), "n", 2.0);
    assert!(describe(&mono_audio(&hi)).centroid_all_hz > d.centroid_all_hz * 1.15);
}

#[test]
fn cicada_kinds_differ_and_calls_gate_the_noise() {
    let mut hashes = Vec::new();
    for kind in ["buzz", "rattle", "call"] {
        let x = node_audio(one("cicada", json!({"kind": kind, "seed": 4})), "n", 2.0);
        assert_finite(&x, kind);
        println!(
            "cicada {kind}: peak {:.1} dB rms {:.1} dB",
            peak_db(&x),
            rms_db(&x)
        );
        assert!(peak(&x) > 1e-4, "{kind} is silent");
        hashes.push(fnv(&x));
    }
    assert!(hashes[0] != hashes[1] && hashes[1] != hashes[2]);
    // A tick call is one narrow pulse a second: mostly silence.
    let tick = node_audio(
        one("cicada", json!({"kind": "call", "call": "tick"})),
        "n",
        3.0,
    );
    let trill = node_audio(
        one("cicada", json!({"kind": "call", "call": "trill"})),
        "n",
        3.0,
    );
    println!(
        "cicada call tick rms {:.1} dB peak {:.1} dB, trill rms {:.1} dB peak {:.1} dB",
        rms_db(&tick),
        peak_db(&tick),
        rms_db(&trill),
        peak_db(&trill)
    );
    assert!(rms_db(&tick) - peak_db(&tick) < rms_db(&trill) - peak_db(&trill) - 6.0);
    let a = node_audio(one("cicada", json!({"seed": 1})), "n", 1.0);
    let b = node_audio(one("cicada", json!({"seed": 2})), "n", 1.0);
    assert_ne!(fnv(&a), fnv(&b));
}

#[test]
fn the_creature_nodes_are_deterministic_and_reject_bad_fields() {
    assert_bit_identical(one("roar", json!({"seed": 3, "vowel": "E"})), 2.0);
    assert_bit_identical(one("fly", json!({"seed": 3})), 1.0);
    for kind in ["buzz", "rattle", "call"] {
        assert_bit_identical(one("cicada", json!({"kind": kind, "seed": 3})), 1.0);
    }
    for (ty, f) in [
        ("roar", json!({"size": 20})),
        ("roar", json!({"aggression": 2})),
        ("roar", json!({"vowel": "zz"})),
        ("roar", json!({"length": 0.01})),
        ("fly", json!({"freq": 5})),
        ("fly", json!({"wave": "x"})),
        ("cicada", json!({"kind": "frog"})),
        ("cicada", json!({"call": "x", "kind": "call"})),
        ("cicada", json!({"pitch": 9})),
    ] {
        let e = render_error(one(ty, f));
        println!("{ty}: {e}");
        assert!(e.contains("give"), "{e}");
    }
}
