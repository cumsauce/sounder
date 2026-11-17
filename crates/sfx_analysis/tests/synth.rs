//! `fm` and `wavetable`: Bessel sideband levels, harmonic recipes, aliasing,
//! morphing, extremes, determinism and error messages.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::SoundDoc;

/// Bessel J_k(x) by the integral (1/pi) int_0^pi cos(k t - x sin t) dt
/// (Simpson, 2000 intervals: exact to ~1e-12 for these orders).
fn bessel(k: i32, x: f64) -> f64 {
    let n = 2000;
    let h = std::f64::consts::PI / f64::from(n);
    let f = |t: f64| (f64::from(k) * t - x * t.sin()).cos();
    let mut s = f(0.0) + f(std::f64::consts::PI);
    for i in 1..n {
        s += f(f64::from(i) * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    s * h / 3.0 / std::f64::consts::PI
}

/// dB of the power of a sine of amplitude `a` (the kit's scale).
fn sine_db(a: f64) -> f32 {
    (20.0 * a.abs().log10() - 3.0103) as f32
}

fn fm(fields: Value) -> Vec<Value> {
    let mut v = json!({"type": "fm", "name": "x"});
    v.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    vec![v]
}

fn wt(fields: Value) -> Vec<Value> {
    let mut v = json!({"type": "wavetable", "name": "x"});
    v.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    vec![v]
}

fn centroid(x: &[f32]) -> f32 {
    let s = spectrum(x, SR);
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (f, d) in s {
        let p = 10f64.powf(f64::from(d) / 10.0);
        num += f64::from(f) * p;
        den += p;
    }
    (num / den.max(1e-30)) as f32
}

fn render_err(nodes: &[Value]) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes.to_vec();
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": "x"}));
    d.duration = Some(0.1);
    match d.render(&sfx_graph::RenderOpts {
        cached: false,
        ..Default::default()
    }) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn fm_index_zero_is_a_pure_sine() {
    let x = node_audio(
        fm(json!({"freq": 1000, "index": 0, "mod_ratio": 2.5, "amp": 0.5})),
        "x",
        1.0,
    );
    let total = band_db(&x, SR, 0.0, 24_000.0);
    let fund = band_db(&x, SR, 950.0, 1050.0);
    let rest = 10.0
        * (10f32.powf(total / 10.0) - 10f32.powf(fund / 10.0))
            .max(1e-30)
            .log10();
    let thd = rest - fund;
    println!(
        "fm index 0: peak {:.3} Hz, level {fund:.2} dB, THD+N {thd:.1} dB",
        peak_hz(&x, SR)
    );
    assert!((peak_hz(&x, SR) - 1000.0).abs() < 0.5);
    assert!((fund - sine_db(0.5)).abs() < 0.05);
    assert!(thd < -80.0, "THD+N {thd}");
}

#[test]
fn fm_sidebands_match_bessel() {
    // fc 2000, fm 200 (mod_ratio 0.1): sidebands at 2000 +- 200 k; the first
    // folded one (k = 10) is J_10(2) = 3e-7, so J0..J3 stand alone.
    let x = node_audio(
        fm(json!({"freq": 2000, "mod_ratio": 0.1, "index": 2})),
        "x",
        1.0,
    );
    for k in 0..=3 {
        let want = sine_db(bessel(k, 2.0));
        for sign in [1i32, -1] {
            if k == 0 && sign < 0 {
                continue;
            }
            let f = 2000.0 + (sign * k) as f32 * 200.0;
            let got = band_db(&x, SR, f - 40.0, f + 40.0);
            println!(
                "fm 2000/200 index 2: sideband {} at {f} Hz: {got:.2} dB, J{k}(2) says {want:.2}",
                sign * k
            );
            assert!((got - want).abs() < 1.0, "k {k} at {f}: {got} vs {want}");
        }
    }
}

#[test]
fn fm_one_to_one_harmonics_follow_the_folded_bessel_sum() {
    // ratio 1:1 puts every sideband on a harmonic of f0 and folds the
    // negative ones back: A_n = J_{n-1}(I) + (-1)^n J_{n+1}(I).
    let f0 = 500.0;
    let x = node_audio(fm(json!({"freq": f0, "index": 2})), "x", 1.0);
    for n in 1..=4 {
        let a = bessel(n - 1, 2.0) + (-1f64).powi(n) * bessel(n + 1, 2.0);
        let want = sine_db(a);
        let f = f0 * n as f32;
        let got = band_db(&x, SR, f - 40.0, f + 40.0);
        println!("fm 1:1 index 2: harmonic {n}: {got:.2} dB, folded Bessel sum says {want:.2}");
        assert!((got - want).abs() < 1.0, "harmonic {n}: {got} vs {want}");
    }
}

#[test]
fn fm_inharmonic_ratio_puts_peaks_at_carrier_plus_minus_k_modulator() {
    let (fc, fmod) = (1000.0f32, 1400.0f32);
    let x = node_audio(
        fm(json!({"freq": 1000, "mod_ratio": 1.4, "index": 2})),
        "x",
        1.0,
    );
    let ps = peaks(&x, SR, 8, 30.0);
    println!("fm 1000 / 1400 index 2 peaks: {ps:?}");
    assert!(ps.len() >= 5);
    for (f, _) in &ps {
        let near = (-8..=8)
            .map(|k| (fc + k as f32 * fmod).abs())
            .any(|c| (c - f).abs() < 3.0);
        assert!(near, "peak at {f} Hz is not fc +- k fm");
    }
    // 400 Hz (= |fc - fm|) and 1800 Hz (= |fc - 2 fm|) are not harmonics of fc.
    for f in [400.0, 1800.0] {
        let got = band_db(&x, SR, f - 30.0, f + 30.0);
        println!("fm 1000 / 1400: {f} Hz at {got:.2} dB");
        assert!(got > -15.0);
    }
    assert!(alias_db(&x, SR, fc) > -10.0, "mostly off the grid of fc");
}

#[test]
fn fm_feedback_turns_the_modulator_into_a_saw_like_wave() {
    // ratio 0 freezes the carrier at phase 0 and a tiny index makes the
    // output the modulator itself (sin(I m) ~ I m): its harmonics show.
    let mk = |fb: f64| {
        node_audio(
            fm(json!({"freq": 500, "ratio": 0, "index": 0.01, "feedback": fb})),
            "x",
            1.0,
        )
    };
    let harm = |x: &[f32], k: f32| {
        band_db(x, SR, 500.0 * k - 40.0, 500.0 * k + 40.0) - band_db(x, SR, 460.0, 540.0)
    };
    let (plain, fb) = (mk(0.0), mk(1.5));
    let (p2, p3) = (harm(&plain, 2.0), harm(&plain, 3.0));
    let (f2, f3) = (harm(&fb, 2.0), harm(&fb, 3.0));
    println!(
        "fm modulator alone, feedback 0: H2 {p2:.1} dB H3 {p3:.1} dB; feedback 1.5: H2 {f2:.1} dB H3 {f3:.1} dB"
    );
    // (-77 dB is the kit's -120 dB floor against the -43 dB fundamental.)
    assert!(p2 < -70.0 && p3 < -70.0);
    assert!(f2 > -20.0 && f3 > -30.0);
}

#[test]
fn fm_mod_shapes_all_render_and_differ() {
    let mut cents = Vec::new();
    for shape in ["sine", "saw", "square", "tri"] {
        let x = node_audio(
            fm(json!({"freq": 400, "index": 3, "mod_shape": shape})),
            "x",
            0.5,
        );
        assert_finite(&x, shape);
        cents.push((shape, centroid(&x)));
    }
    println!("fm mod_shape centroids: {cents:?}");
    for i in 0..cents.len() {
        for j in 0..i {
            assert!((cents[i].1 - cents[j].1).abs() > 5.0, "{cents:?}");
        }
    }
}

#[test]
fn fm_signal_inputs_and_extremes_stay_finite() {
    for (freq, idx, fb, r, mr) in [
        (0.0, 5.0, 0.0, 1.0, 1.0),
        (20.0, 1000.0, 100.0, 8.0, 0.01),
        (24_000.0, 20.0, 5.0, 1.0, 4.0),
        (1.0e7, 50.0, 2.0, 1.0, 1.0),
        (-440.0, 4.0, 1.0, -2.0, 3.0),
        (440.0, -4.0, 0.0, 1.0, -1.0),
    ] {
        let x = node_audio(
            fm(json!({"freq": freq, "index": idx, "feedback": fb, "ratio": r, "mod_ratio": mr})),
            "x",
            0.2,
        );
        assert_finite(&x, &format!("fm {freq} {idx} {fb}"));
        assert!(x.iter().all(|v| v.abs() <= 1.0 + 1e-6));
    }
    // A swept freq and a swept index.
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 100, "to": 8000, "time": 0.5}),
        json!({"type": "ramp", "name": "i", "from": 0, "to": 12, "time": 0.5, "curve": "linear"}),
        json!({"type": "fm", "name": "x", "freq": "f", "index": "i", "amp": 0.5}),
    ];
    assert_finite(&node_audio(&nodes, "x", 0.5), "swept fm");
}

#[test]
fn fm_is_bit_identical_and_errors_are_clear() {
    assert_bit_identical(
        fm(json!({"freq": 330, "index": 3, "feedback": 0.7, "mod_ratio": 1.5, "mod_shape": "saw"})),
        0.3,
    );
    let e = render_err(&fm(json!({"freq": 220, "feedback": -1})));
    println!("fm error: {e}");
    assert!(e.contains("feedback") && e.contains("-1"));
    let e = render_err(&fm(json!({"freq": 220, "mod_shape": "pulse"})));
    println!("fm mod_shape error: {e}");
    assert!(e.contains("mod_shape") || e.contains("pulse"));
}

#[test]
fn wavetable_one_zero_half_has_two_peaks_six_db_apart() {
    let f0 = 1000.0;
    let x = node_audio(wt(json!({"freq": f0, "harmonics": [1, 0, 0.5]})), "x", 1.0);
    let p1 = band_db(&x, SR, f0 - 40.0, f0 + 40.0);
    let p3 = band_db(&x, SR, 3.0 * f0 - 40.0, 3.0 * f0 + 40.0);
    println!(
        "wavetable [1,0,0.5]: f0 {p1:.2} dB, 3 f0 {p3:.2} dB, gap {:.2}",
        p1 - p3
    );
    assert!((p1 - sine_db(1.0)).abs() < 0.1);
    assert!((p1 - p3 - 6.02).abs() < 0.2);
    // Nothing else above -70 dB: every bin away from the two peaks.
    let worst = spectrum(&x, SR)
        .into_iter()
        .filter(|(f, _)| (f - f0).abs() > 60.0 && (f - 3.0 * f0).abs() > 60.0)
        .map(|(_, d)| d)
        .fold(f32::MIN, f32::max);
    println!("wavetable [1,0,0.5]: loudest other bin {worst:.1} dB");
    assert!(worst < -70.0, "{worst}");
}

#[test]
fn wavetable_phases_shift_each_harmonic() {
    let x = node_audio(
        wt(json!({"freq": 100, "harmonics": [1], "phases": [0.25]})),
        "x",
        0.05,
    );
    println!("wavetable phase 0.25: first sample {}", x[0]);
    assert!((x[0] - 1.0).abs() < 1e-5, "a cosine starts at 1");
    let plain = node_audio(wt(json!({"freq": 100, "harmonics": [1]})), "x", 0.05);
    assert!(plain[0].abs() < 1e-5, "a sine starts at 0");
}

#[test]
fn wavetable_drops_harmonics_above_nyquist_without_aliasing() {
    let h: Vec<f32> = (1..=32).map(|k| 1.0 / k as f32).collect();
    let x = node_audio(wt(json!({"freq": 5000, "harmonics": h})), "x", 1.0);
    let a = alias_db(&x, SR, 5000.0);
    println!("wavetable 5 kHz, 32 harmonics: alias_db {a:.1}");
    assert!(a < -60.0, "{a}");
    // 1 to 4 survive (5, 10, 15, 20 kHz); 25 kHz and up are gone.
    let h4 = band_db(&x, SR, 19_900.0, 20_100.0);
    assert!((h4 - sine_db(0.25)).abs() < 0.3, "{h4}");
    // A slow sweep over many harmonics stays on the grid at every instant.
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 300, "to": 3000, "time": 1.0, "curve": "linear"}),
        json!({"type": "wavetable", "name": "x", "freq": "f", "harmonics": h}),
    ];
    let y = node_audio(&nodes, "x", 1.0);
    assert_finite(&y, "swept wavetable");
    // No click when a harmonic crosses Nyquist: the 2nd harmonic alone,
    // swept from 20 kHz to 25 kHz, fades out smoothly (its peak per 10 ms
    // never moves more than 0.05 and ends at zero).
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 10000, "to": 12500, "time": 1.0, "curve": "linear"}),
        json!({"type": "wavetable", "name": "x", "freq": "f", "harmonics": [0, 1]}),
    ];
    let z = node_audio(&nodes, "x", 1.0);
    let env: Vec<f32> = z
        .chunks(480)
        .map(|c| c.iter().fold(0.0f32, |a, b| a.max(b.abs())))
        .collect();
    let jump = env
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    println!(
        "2nd harmonic swept through Nyquist: largest 10 ms peak change {jump:.3}, final peak {:.4}",
        env.last().unwrap()
    );
    assert!(jump < 0.05 && *env.last().unwrap() < 1e-6);
}

#[test]
fn wavetable_fades_harmonics_near_nyquist() {
    // 2nd harmonic at 22 kHz sits in the fade (21.6 kHz to 24 kHz): weight
    // 1 - 400/2400. 3rd and up are above Nyquist: dropped.
    let x = node_audio(
        wt(json!({"freq": 11_000, "harmonics": [1, 1, 1]})),
        "x",
        1.0,
    );
    let h2 = band_db(&x, SR, 21_900.0, 22_100.0);
    let want = sine_db(1.0 - 400.0 / 2400.0);
    let above = band_db(&x, SR, 12_000.0, 21_000.0);
    println!(
        "wavetable 11 kHz: 22 kHz harmonic {h2:.2} dB (taper says {want:.2}); 12-21 kHz {above:.1} dB"
    );
    assert!((h2 - want).abs() < 0.3);
    assert!(above < -80.0);
    let gone = node_audio(
        wt(json!({"freq": 13_000, "harmonics": [1, 1, 1]})),
        "x",
        0.5,
    );
    let top = band_db(&gone, SR, 14_000.0, 24_000.0);
    println!("wavetable 13 kHz: energy above the fundamental {top:.1} dB");
    assert!(top < -80.0);
}

#[test]
fn wavetable_morph_moves_the_centroid_monotonically() {
    let to: Vec<f32> = (1..=16).map(|k| 1.0 / k as f32).collect();
    let mut last = 0.0;
    let mut line = Vec::new();
    for m in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let x = node_audio(
            wt(json!({"freq": 200, "harmonics": [1], "harmonics_to": to, "morph": m})),
            "x",
            1.0,
        );
        let c = centroid(&x);
        line.push(c);
        assert!(c > last, "morph {m}: centroid {c} not above {last}");
        last = c;
    }
    println!("wavetable morph 0..1 centroids (Hz): {line:?}");
    // Morph as a signal: a ramp over the sound, centroid per quarter.
    let nodes = vec![
        json!({"type": "ramp", "name": "m", "from": 0, "to": 1, "time": 2.0, "curve": "linear"}),
        json!({"type": "wavetable", "name": "x", "freq": 200, "harmonics": [1],
               "harmonics_to": to, "morph": "m"}),
    ];
    let y = node_audio(&nodes, "x", 2.0);
    let q = y.len() / 4;
    let cs: Vec<f32> = (0..4).map(|i| centroid(&y[i * q..(i + 1) * q])).collect();
    println!("wavetable morph ramp, centroid per quarter: {cs:?}");
    assert!(cs.windows(2).all(|w| w[1] > w[0]), "{cs:?}");
}

#[test]
fn wavetable_extremes_stay_finite_and_are_bit_identical() {
    let all: Vec<f32> = (1..=256).map(|k| 1.0 / k as f32).collect();
    for freq in [0.0, 1.0, 20.0, 24_000.0, 1.0e7, -440.0] {
        let x = node_audio(wt(json!({"freq": freq, "harmonics": all})), "x", 0.2);
        assert_finite(&x, &format!("wavetable at {freq}"));
    }
    assert_bit_identical(
        wt(
            json!({"freq": 180, "harmonics": all, "harmonics_to": [1, 0, 1], "morph": 0.4,
                 "phases": [0.1, 0.2]}),
        ),
        0.3,
    );
    // The cap is exact: 256 renders (above), 257 errors.
    let long: Vec<f32> = vec![0.1; 257];
    let e = render_err(&wt(json!({"freq": 100, "harmonics": long})));
    println!("wavetable error: {e}");
    assert!(e.contains("harmonics") && e.contains("256"));
}

#[test]
fn wavetable_errors_say_what_to_do() {
    let e = render_err(&wt(json!({"freq": 100, "harmonics": []})));
    println!("wavetable empty: {e}");
    assert!(e.contains("harmonics") && e.contains("at least one"));
    let e = render_err(&wt(json!({"freq": 100, "harmonics": [1], "morph": 0.5})));
    println!("wavetable morph without target: {e}");
    assert!(e.contains("morph") && e.contains("harmonics_to"));
    let e = render_err(&wt(
        json!({"freq": 100, "harmonics": [1, 2], "phases": [0, 0, 0]}),
    ));
    println!("wavetable phases: {e}");
    assert!(e.contains("phases"));
}
