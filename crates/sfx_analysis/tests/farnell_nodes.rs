//! The nodes ported straight from Farnell's Pd patches (`bell`,
//! `ball_bounce`, `cricket`, `twang`): what each patch does, measured on the
//! node. Against the patches rendered by real Pd (2008-era compatibility,
//! Pd-extended pow~) they sat 3.6 / 6.5 / 1.9 / 4.8 dB of STFT distance away
//! (docs/nodes/farnell_nodes.md).

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_graph::{RenderOpts, SoundDoc};

fn mono(x: &[f32]) -> sfx_graph::Audio {
    sfx_graph::Audio {
        sample_rate: SR,
        channels: vec![x.to_vec()],
    }
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
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
    let a = describe(&mono(&node_audio(
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
    let d = describe(&mono(&x));
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
    let d = describe(&mono(&x));
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
