//! The nodes ported straight from Farnell's Pd patches (`bell`,
//! `ball_bounce`, `cricket`, `twang`): what each patch does, measured on the
//! node. Against the patches rendered by real Pd (2008-era compatibility,
//! Pd-extended pow~) they sat 3.6 / 6.5 / 1.9 / 4.8 dB of STFT distance away
//! (docs/nodes/farnell.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn render_error(nodes: Vec<Value>) -> String {
    crate::common::render_error(nodes, "n", 0.1)
}

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

#[test]
fn bell_partials_sit_on_the_patch_ratios() {
    let x = node_audio(one("bell", json!({"freq": 750, "at": 0.05})), "n", 1.5);
    assert_finite(&x, "bell");
    // The loudest lines are partials of the patch (ratios 0.5 .. 24 of 750).
    let ratios = [
        0.5001f32, 1.0, 2.0, 2.002, 2.4999, 2.571, 3.0, 6.242, 9.6, 11.0, 12.483, 16.0, 24.0,
    ];
    for (hz, _) in peaks(&x, SR, 5, 30.0) {
        let near = ratios
            .iter()
            .any(|r| ((r * 750.0) - hz).abs() / (r * 750.0) < 0.02);
        println!("bell peak {hz:.0} Hz");
        assert!(near, "{hz} Hz is not a partial of the patch");
    }
    assert!(x.iter().all(|v| v.abs() <= 0.9 + 1e-3), "clipped at 0.9");
}

#[test]
fn bell_strikes_where_asked_and_rings_longer_with_ring() {
    let a = describe(&mono_audio(&node_audio(
        one("bell", json!({"at": [0.05, 1.0]})),
        "n",
        2.0,
    )));
    let onsets: Vec<f32> = a.onsets.iter().map(|o| o.t).collect();
    println!("bell onsets {onsets:?}");
    assert!(onsets.iter().any(|t| (t - 0.05).abs() < 0.02));
    assert!(onsets.iter().any(|t| (t - 1.0).abs() < 0.02));
    let short = node_audio(one("bell", json!({"ring": 0.5})), "n", 3.0);
    let long = node_audio(one("bell", json!({"ring": 2.0})), "n", 3.0);
    let tail = |x: &[f32]| rms_db(&x[(0.6 * SR as f32) as usize..(1.0 * SR as f32) as usize]);
    println!(
        "bell 0.6-1 s rms: ring 0.5 {:.1} dB, ring 2 {:.1} dB",
        tail(&short),
        tail(&long)
    );
    assert!(tail(&long) > tail(&short) + 10.0);
}

#[test]
fn ball_bounces_speed_up_and_fade() {
    let x = node_audio(one("ball_bounce", json!({})), "n", 4.0);
    assert_finite(&x, "ball");
    let d = describe(&mono_audio(&x));
    let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
    println!("ball onsets {t:?}");
    assert!(t.len() >= 8, "{t:?}");
    let gaps: Vec<f32> = t.windows(2).map(|w| w[1] - w[0]).collect();
    // The first gap is the patch's 300 ms; later ones are shorter.
    assert!((gaps[0] - 0.3).abs() < 0.03, "{gaps:?}");
    assert!(gaps[gaps.len() - 1] < 0.5 * gaps[0], "{gaps:?}");
    let early = rms_db(&x[..SR as usize / 2]);
    let late = rms_db(&x[(2.0 * SR as f32) as usize..(2.5 * SR as f32) as usize]);
    assert!(late < early - 6.0, "{early} {late}");
}

#[test]
fn cricket_chirps_every_period_at_its_pitch() {
    let x = node_audio(one("cricket", json!({})), "n", 3.0);
    assert_finite(&x, "cricket");
    let p = peak_hz(&x, SR);
    println!("cricket peak {p:.0} Hz");
    assert!(
        (4400.0..4700.0).contains(&p) || (8800.0..9200.0).contains(&p),
        "{p}"
    );
    // A chirp every 41 x 17 ms = 0.697 s.
    let d = describe(&mono_audio(&x));
    // Chirp starts: onsets more than 0.3 s after the one before (the
    // pulses inside a chirp are a few tens of ms apart).
    let mut starts: Vec<f32> = Vec::new();
    for o in &d.onsets {
        if starts.last().is_none_or(|l| o.t - l > 0.3) {
            starts.push(o.t);
        }
    }
    println!("cricket chirp starts {starts:?}");
    let gaps: Vec<f32> = starts.windows(2).map(|w| w[1] - w[0]).collect();
    // Onset detection can miss a chirp's first pulse, so single gaps move
    // by up to ~70 ms; the mean is the 0.697 s cycle.
    let mean = (starts[starts.len() - 1] - starts[0]) / gaps.len() as f32;
    assert!((mean - 0.697).abs() < 0.03, "{gaps:?}");
    let hi = node_audio(one("cricket", json!({"pitch": 6000})), "n", 1.0);
    assert!(peak_hz(&hi, SR) > p * 1.2);
}

#[test]
fn twang_rings_at_its_free_mode_and_dies() {
    let x = node_audio(one("twang", json!({"freq": 426})), "n", 3.0);
    assert_finite(&x, "twang");
    let p = peak_hz(&x, SR);
    println!("twang peak {p:.0} Hz");
    // The driving phasor runs at freq / 4 and is the strongest line; the
    // free modes ring at freq and above.
    assert!((p - 106.5).abs() < 8.0 || (p - 426.0).abs() < 15.0, "{p}");
    assert!(band_db(&x, SR, 400.0, 450.0) > band_db(&x, SR, 300.0, 350.0) + 6.0);
    let early = rms_db(&x[..SR as usize / 2]);
    let late = rms_db(&x[(2.0 * SR as f32) as usize..]);
    println!("twang rms first 0.5 s {early:.1} dB, after 2 s {late:.1} dB");
    assert!(late < early - 20.0);
    let low = node_audio(one("twang", json!({"freq": 200})), "n", 1.5);
    assert!(peak_hz(&low, SR) < peak_hz(&x, SR));
}

#[test]
fn the_ported_nodes_are_deterministic_and_reject_bad_fields() {
    assert_bit_identical(one("bell", json!({"seed": 3})), 1.0);
    assert_bit_identical(one("twang", json!({"seed": 3})), 1.0);
    assert_bit_identical(one("cricket", json!({})), 1.0);
    assert_bit_identical(one("ball_bounce", json!({})), 1.0);
    for (ty, f) in [
        ("bell", json!({"freq": 5})),
        ("ball_bounce", json!({"length": 0.1})),
        ("cricket", json!({"period": 3, "pulses": 7})),
        ("twang", json!({"freq": 2000})),
    ] {
        let e = render_error(one(ty, f));
        println!("{ty}: {e}");
        assert!(e.contains("give"), "{e}");
    }
}

// Round 2: siren, door_creak, jet, red_alert (5 kHz-scale STFT distance to
// the Pd renders: 6.1 / 8.0 / 6.3 / 4.2 dB).

#[test]
fn siren_sweeps_between_low_and_high() {
    let x = node_audio(one("siren", json!({"rate": 0.5})), "n", 2.0);
    assert_finite(&x, "siren");
    // The horn's band-pass sits at 1.5 kHz; the sweep moves the fundamental
    // between 300 and 1100 Hz, so the first and last quarters of a slow
    // sweep differ in pitch.
    let d = describe(&mono_audio(&x));
    let p: Vec<f32> = d.pitch_tenths.iter().flatten().copied().collect();
    println!("siren pitch by tenth {:?}", d.pitch_tenths);
    let (lo, hi) = (
        p.iter().cloned().fold(f32::MAX, f32::min),
        p.iter().cloned().fold(0.0, f32::max),
    );
    assert!(lo > 250.0 && hi < 1300.0 && hi > 1.8 * lo, "{lo} {hi}");
}

#[test]
fn door_creaks_only_when_pushed() {
    let still = node_audio(one("door_creak", json!({"force": 0.2})), "n", 1.0);
    assert!(rms_db(&still) < -100.0, "{}", rms_db(&still));
    let slow = describe(&mono_audio(&node_audio(
        one("door_creak", json!({"force": 0.4, "seed": 2})),
        "n",
        2.0,
    )));
    let fast = describe(&mono_audio(&node_audio(
        one("door_creak", json!({"force": 0.9, "seed": 2})),
        "n",
        2.0,
    )));
    println!(
        "door creak density {:.1} / {:.1} events a second",
        slow.density.per_s, fast.density.per_s
    );
    assert!(fast.density.per_s > slow.density.per_s * 1.3);
}

#[test]
fn jet_rises_in_pitch_and_level_with_speed() {
    let lo = node_audio(one("jet", json!({"speed": 0.3})), "n", 8.0);
    let hi = node_audio(one("jet", json!({"speed": 0.9})), "n", 8.0);
    assert_finite(&hi, "jet");
    let tail = |x: &[f32]| x[(6.0 * SR as f32) as usize..].to_vec();
    let c = |x: Vec<f32>| describe(&mono_audio(&x)).centroid_all_hz;
    let (cl, ch) = (c(tail(&lo)), c(tail(&hi)));
    println!("jet centroid at speed 0.3 / 0.9: {cl:.0} / {ch:.0} Hz");
    assert!(ch > cl * 1.5);
    // The 0.2 Hz spool: the first half second is quieter than later.
    assert!(rms_db(&hi[..SR as usize / 2]) < rms_db(&tail(&hi)) - 3.0);
}

#[test]
fn red_alert_whoops_where_asked_and_rises() {
    let x = node_audio(one("red_alert", json!({"at": [0.01, 1.3]})), "n", 2.6);
    assert_finite(&x, "red_alert");
    let d = describe(&mono_audio(&x));
    let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
    println!("red alert onsets {t:?}");
    assert!(t.iter().any(|v| (v - 0.01).abs() < 0.03));
    assert!(t.iter().any(|v| (v - 1.3).abs() < 0.03));
    for e in [
        render_error(one("red_alert", json!({"pitch": 9}))),
        render_error(one("siren", json!({"low": 900, "high": 500}))),
    ] {
        assert!(e.contains("give"), "{e}");
    }
    assert_bit_identical(one("door_creak", json!({"force": 0.7, "seed": 1})), 1.0);
    assert_bit_identical(one("jet", json!({"speed": 0.5})), 1.0);
}
