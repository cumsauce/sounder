//! `plate`, the nonlinear plate. Each test prints what it measures (the
//! figures in docs/nodes/plate.md come from these lines).

use serde_json::{Value, json};

use crate::common::*;

const MS: f32 = SR as f32 / 1000.0;

/// A steel sheet struck by a 1 kg mallet; `extra` overrides fields.
fn struck(extra: Value) -> Vec<Value> {
    let plate = with_fields(
        json!({"type": "plate", "name": "p", "excite": "c", "material": "metal",
            "f0": 60, "thickness": 0.002, "modes": 128, "seed": 1}),
        extra,
    );
    vec![
        json!({"type": "contact", "name": "c", "speed": 4, "mass": 1, "material": "metal",
            "hardness": 0.5}),
        plate,
    ]
}

fn ring(extra: Value, secs: f32) -> Vec<f32> {
    node_audio(struck(extra), "p", secs)
}

/// Spectral centroid of `x` between `a` and `b` milliseconds.
fn centroid(x: &[f32], a: f32, b: f32) -> f32 {
    spectrum_centroid(&x[(a * MS) as usize..(b * MS) as usize])
}

/// Frequency of the strongest component of `x` in `lo..hi` Hz (0.05 Hz
/// steps, a direct DFT under a Hann window).
fn strongest_in(x: &[f32], lo: f32, hi: f32) -> f32 {
    let n = x.len();
    let w = |i: usize| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
    let mut best = (lo, 0.0f64);
    let mut f = lo;
    while f <= hi {
        let step = std::f64::consts::TAU * f64::from(f) / f64::from(SR);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let a = step * i as f64;
            let s = f64::from(*v) * w(i);
            re += s * a.cos();
            im += s * a.sin();
        }
        let p = re * re + im * im;
        if p > best.1 {
            best = (f, p);
        }
        f += 0.05;
    }
    best.0
}

#[test]
fn plate_is_finite_across_the_parameter_range() {
    // Every corner of drive, cascade, loss, modes, thickness and strike,
    // with an impulse 100x a unit strike and a noise burst.
    let mut worst = 0.0f32;
    for drive in [0.0, 1.0, 10.0] {
        for cascade in [0.0, 4.0] {
            for (loss, modes, thick) in [(0.01, 256, 0.0005), (100.0, 1, 0.02), (1.0, 64, 0.002)] {
                for excite in [
                    json!({"type": "impulse", "name": "c", "amp": 100}),
                    json!({"type": "noise", "name": "c", "amp": 1}),
                ] {
                    let x = node_audio(
                        vec![
                            excite,
                            json!({"type": "plate", "name": "p", "excite": "c", "material": "metal",
                                "f0": 40, "thickness": thick, "modes": modes, "loss": loss,
                                "drive": drive, "cascade": cascade, "strike_position": 0.9}),
                        ],
                        "p",
                        0.5,
                    );
                    assert_finite(&x, "plate");
                    worst = worst.max(peak(&x));
                }
            }
        }
    }
    println!("plate corners: all finite, largest peak {worst:.1}");
}

#[test]
fn plate_soft_strike_is_the_linear_plate() {
    // drive 0 is `modal`'s plate to rounding (modal's resonators are f32,
    // these f64, so the difference grows with time: compare the first
    // 100 ms and the whole second); drive 0.25 keeps its brightness and
    // level within a few percent.
    let modal = node_audio(
        vec![
            json!({"type": "contact", "name": "c", "speed": 4, "mass": 1, "material": "metal",
                "hardness": 0.5}),
            json!({"type": "modal", "name": "p", "excite": "c", "material": "metal",
                "f0": 60, "thickness": 0.002, "modes": 128, "seed": 1}),
        ],
        "p",
        1.0,
    );
    let lin = ring(json!({"drive": 0}), 1.0);
    let soft = ring(json!({"drive": 0.25}), 1.0);
    let diff = |a: &[f32], b: &[f32]| {
        let d: Vec<f32> = a.iter().zip(b).map(|(x, y)| x - y).collect();
        rms(&d) / rms(b)
    };
    let early = (100.0 * MS) as usize;
    let (d_early, d_all) = (diff(&lin[..early], &modal[..early]), diff(&lin, &modal));
    let (c_lin, c_soft) = (centroid(&modal, 30.0, 80.0), centroid(&soft, 30.0, 80.0));
    let level = rms_db(&soft) - rms_db(&modal);
    println!(
        "plate drive 0 vs modal: relative rms difference {d_early:.1e} over 100 ms, {d_all:.1e} over 1 s; drive 0.25: centroid 30-80 ms {c_soft:.0} Hz (modal {c_lin:.0}), level {level:+.2} dB"
    );
    assert!(
        d_early < 1e-3 && d_all < 1e-2,
        "drive 0 is not modal's plate"
    );
    assert!((c_soft / c_lin - 1.0).abs() < 0.05 && level.abs() < 0.5);
}

#[test]
fn plate_centroid_rises_with_drive() {
    let mut last = 0.0;
    for drive in [0.0, 1.0, 2.0, 4.0] {
        let x = ring(json!({"drive": drive}), 0.2);
        let c = centroid(&x, 30.0, 80.0);
        println!("plate drive {drive}: centroid 30-80 ms {c:.0} Hz");
        assert!(
            c > last * 1.1,
            "drive {drive}: centroid {c:.0} not above {last:.0}"
        );
        last = c;
    }
}

#[test]
fn plate_hard_strike_darkens_as_it_decays() {
    let x = ring(json!({"drive": 2}), 2.0);
    let spans = [
        (0.0, 20.0),
        (30.0, 80.0),
        (100.0, 300.0),
        (300.0, 600.0),
        (1000.0, 1500.0),
    ];
    let cs: Vec<f32> = spans.iter().map(|&(a, b)| centroid(&x, a, b)).collect();
    let lin = ring(json!({"drive": 0}), 2.0);
    let ls: Vec<f32> = spans.iter().map(|&(a, b)| centroid(&lin, a, b)).collect();
    println!("plate drive 2: centroid over {spans:?} ms: {cs:.0?} Hz (linear {ls:.0?})");
    for w in cs[1..].windows(2) {
        assert!(w[1] < w[0], "the centroid does not fall: {cs:?}");
    }
}

#[test]
fn plate_pitch_glides_down() {
    // The lowest mode is sharp while the plate is loud and settles to the
    // linear frequency as it decays.
    let x = ring(json!({"drive": 2, "cascade": 0}), 3.0);
    let lin = ring(json!({"drive": 0}), 3.0);
    let at = |y: &[f32], a: f32, b: f32| {
        strongest_in(&y[(a * MS) as usize..(b * MS) as usize], 50.0, 75.0)
    };
    let early = at(&x, 0.0, 400.0);
    let late = at(&x, 2000.0, 3000.0);
    let linear = at(&lin, 0.0, 400.0);
    let cents = |a: f32, b: f32| 1200.0 * (a / b).log2();
    println!(
        "plate drive 2 cascade 0: lowest mode {early:.2} Hz in 0-0.4 s, {late:.2} Hz in 2-3 s, linear {linear:.2} Hz ({:.0} and {:.0} cents over it)",
        cents(early, linear),
        cents(late, linear)
    );
    assert!(early > late && cents(early, linear) > 10.0);
    assert!(cents(late, linear).abs() < cents(early, linear) / 2.0);
}

#[test]
fn plate_softer_strike_in_one_input_is_more_linear() {
    // Two strikes 20 dB apart in one input: the soft one's centroid stays
    // near the linear plate's, the loud one's rises.
    let doc = |drive: f32| {
        vec![
            json!({"type": "contact", "name": "a", "speed": 4, "mass": 1, "material": "metal", "hardness": 0.5}),
            json!({"type": "contact", "name": "b", "speed": 0.4, "mass": 1, "material": "metal", "hardness": 0.5, "at": 2.0}),
            json!({"type": "mix", "name": "c", "inputs": ["a", "b"]}),
            json!({"type": "plate", "name": "p", "excite": "c", "material": "metal",
                "f0": 60, "thickness": 0.002, "loss": 4, "drive": drive, "seed": 1}),
        ]
    };
    let hard = node_audio(doc(2.0), "p", 2.1);
    let lin = node_audio(doc(0.0), "p", 2.1);
    let (h_loud, l_loud) = (centroid(&hard, 5.0, 50.0), centroid(&lin, 5.0, 50.0));
    let (h_soft, l_soft) = (
        centroid(&hard, 2005.0, 2050.0),
        centroid(&lin, 2005.0, 2050.0),
    );
    println!(
        "plate two strikes 20 dB apart, drive 2: loud {h_loud:.0} Hz (linear {l_loud:.0}), soft {h_soft:.0} Hz (linear {l_soft:.0})"
    );
    assert!(h_loud > 1.2 * l_loud);
    assert!((h_soft / l_soft - 1.0).abs() < 0.05);
}

#[test]
fn plate_is_deterministic() {
    let a = ring(json!({"drive": 2}), 1.0);
    let b = ring(json!({"drive": 2}), 1.0);
    assert_eq!(fnv(&a), fnv(&b));
    assert_bit_identical(struck(json!({"drive": 2})), 1.0);
    let other = ring(json!({"drive": 2, "seed": 2}), 1.0);
    assert_ne!(fnv(&a), fnv(&other));
}

#[test]
fn plate_largest_size_and_modes_render_in_time() {
    // The biggest plate (f0 20 Hz, 256 modes, drive 10) for 3 s: finite,
    // and the time it takes (the doc quotes the release figure).
    let t = std::time::Instant::now();
    let x = node_audio(
        vec![
            json!({"type": "contact", "name": "c", "speed": 4, "mass": 1, "material": "metal", "hardness": 0.5}),
            json!({"type": "plate", "name": "p", "excite": "c", "material": "metal",
                "f0": 20, "thickness": 0.001, "modes": 256, "drive": 10, "cascade": 4}),
        ],
        "p",
        3.0,
    );
    let secs = t.elapsed().as_secs_f32();
    assert_finite(&x, "plate");
    println!(
        "plate f0 20 Hz, 256 modes, drive 10, 3 s: {secs:.2} s, peak {:.2}",
        peak(&x)
    );
    assert!(secs < 10.0);
}

#[test]
fn plate_errors_name_the_range() {
    let e = render_error_last(
        vec![
            json!({"type": "impulse", "name": "c"}),
            json!({"type": "plate", "name": "p", "excite": "c", "material": "metal", "f0": 60, "drive": 11}),
        ],
        0.1,
    );
    assert!(e.contains("drive: 11"), "{e}");
    let e = render_error_last(
        vec![
            json!({"type": "impulse", "name": "c"}),
            json!({"type": "plate", "name": "p", "excite": "c", "material": "metal", "f0": 60, "cascade": -1}),
        ],
        0.1,
    );
    assert!(e.contains("cascade: -1"), "{e}");
}
