//! M5: `tube` and `string`. Each field is shown to move its measure at three
//! settings; the printed numbers go in docs/nodes/waveguide.md.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::{RenderOpts, SoundDoc};

const C: f32 = 343.0;

fn merged(mut base: Value, extra: Value) -> Value {
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

fn tube(extra: Value) -> Vec<Value> {
    vec![merged(
        json!({"type": "tube", "name": "t", "length": 0.6, "pressure": 0.8, "seed": 1}),
        extra,
    )]
}

fn string(extra: Value) -> Vec<Value> {
    vec![merged(
        json!({"type": "string", "name": "s", "freq": 220, "seed": 1}),
        extra,
    )]
}

/// The error a document with these nodes fails to render with (the node
/// is named `x`).
fn render_error(node: Value) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = vec![
        merged(node, json!({"name": "x"})),
        json!({"type": "output", "name": "out", "input": "x"}),
    ];
    d.duration = Some(0.2);
    d.render(&RenderOpts::default()).unwrap_err()
}

fn tail(x: &[f32], from: f32) -> &[f32] {
    &x[(from * SR as f32) as usize..]
}

fn peak_abs(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// Spectral centroid, Hz.
fn centroid(x: &[f32]) -> f32 {
    let s = spectrum(x, SR);
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (f, d) in &s {
        let p = 10f64.powf(f64::from(*d) / 10.0);
        num += f64::from(*f) * p;
        den += p;
    }
    (num / den.max(1e-30)) as f32
}

/// The strongest spectral peak within +-3 % of `hz`, parabolic-interpolated
/// (0 when the window holds no local maximum).
fn partial_hz(x: &[f32], hz: f32) -> f32 {
    partial_between(x, hz * 0.97, hz * 1.03)
}

/// The strongest spectral peak between `lo` and `hi` Hz, parabolic-interpolated.
fn partial_between(x: &[f32], lo: f32, hi: f32) -> f32 {
    let s = spectrum(x, SR);
    let bin = s[1].0 - s[0].0;
    let mut best = (0.0f32, f32::MIN);
    for k in 1..s.len() - 1 {
        if s[k].0 < lo || s[k].0 > hi {
            continue;
        }
        if s[k].1 > s[k - 1].1 && s[k].1 >= s[k + 1].1 && s[k].1 > best.1 {
            let (a, b, c) = (s[k - 1].1, s[k].1, s[k + 1].1);
            let denom = a - 2.0 * b + c;
            let off = if denom.abs() > 1e-9 {
                (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            best = (s[k].0 + off * bin, b);
        }
    }
    best.0
}

/// Seconds for the 50 ms RMS level to fall 20 dB from its value at 0.05 s.
fn decay_20db(x: &[f32]) -> f32 {
    let win = (0.05 * SR as f32) as usize;
    let level = |t: f32| {
        let a = (t * SR as f32) as usize;
        rms_db(&x[a..(a + win).min(x.len())])
    };
    let start = level(0.05);
    let mut t = 0.1;
    while (t * SR as f32) as usize + win < x.len() {
        if level(t) < start - 20.0 {
            return t - 0.05;
        }
        t += 0.025;
    }
    x.len() as f32 / SR as f32
}

// ------------------------------------------------------------------- tube

#[test]
fn tube_pitch_follows_length_and_the_ends() {
    // Reed, lip and jet self-oscillate: the strongest peak is the pitch.
    let mut worst: f32 = 0.0;
    for excite in ["reed", "lip", "jet"] {
        for open in [true, false] {
            for length in [0.3f32, 0.6, 1.2] {
                let f0 = if open {
                    C / (2.0 * length)
                } else {
                    C / (4.0 * length)
                };
                let x = node_audio(
                    tube(
                        json!({"excite": excite, "length": length, "open_end": open, "pressure": 0.9}),
                    ),
                    "t",
                    1.5,
                );
                assert_finite(&x, "tube");
                let pk = peak_hz(tail(&x, 0.5), SR);
                let err = pk / f0 - 1.0;
                worst = worst.max(err.abs());
                println!(
                    "{excite} L {length} open {open}: c/(2L or 4L) {f0:.1} Hz, peak {pk:.1} Hz ({:+.2} %)",
                    err * 100.0
                );
                assert!(err.abs() < 0.03, "{excite} {length} {open}: {pk} vs {f0}");
            }
        }
    }
    println!("tube pitch: worst error {:.2} %", worst * 100.0);
}

#[test]
fn tube_noise_drive_rings_at_the_resonances() {
    // Noise has no single pitch: the comb peak nearest c / (2L) (or
    // c / (4L)) sits within 3 %.
    for (open, f0) in [(true, C / 1.2), (false, C / 2.4)] {
        let x = node_audio(
            tube(json!({"excite": "noise", "open_end": open, "pressure": 0.8})),
            "t",
            3.0,
        );
        assert_finite(&x, "tube");
        let pk = partial_hz(tail(&x, 0.5), f0);
        println!(
            "noise open {open}: want {f0:.1} Hz, nearest comb peak {pk:.1} Hz ({:+.2} %)",
            (pk / f0 - 1.0) * 100.0
        );
        assert!((pk / f0 - 1.0).abs() < 0.03, "{pk} vs {f0}");
    }
}

#[test]
fn tube_pressure_has_a_threshold_and_then_a_steady_tone() {
    for excite in ["reed", "lip", "jet"] {
        let level = |p: f32| {
            let x = node_audio(tube(json!({"excite": excite, "pressure": p})), "t", 2.0);
            assert_finite(&x, "tube");
            rms_db(tail(&x, 1.0))
        };
        let (below, above, high) = (level(0.1), level(0.8), level(1.0));
        println!(
            "{excite}: rms at pressure 0.1 {below:.1} dB, 0.8 {above:.1} dB, 1.0 {high:.1} dB"
        );
        assert!(below < -60.0, "{excite}: below threshold is near silence");
        assert!(above > below + 40.0, "{excite}: a tone above it");
        assert!(
            high >= above - 0.5,
            "{excite}: pressure does not lower the level"
        );
        // Steady: the level of two later windows agree within 1 dB.
        let x = node_audio(tube(json!({"excite": excite, "pressure": 0.8})), "t", 3.0);
        let (a, b) = (
            rms_db(&x[SR as usize..2 * SR as usize]),
            rms_db(tail(&x, 2.0)),
        );
        println!(
            "{excite}: steady, windows at 1 s and 2 s differ by {:.2} dB",
            (a - b).abs()
        );
        assert!((a - b).abs() < 1.0, "{excite}: {a} vs {b}");
    }
}

#[test]
fn tube_a_pressure_ramp_crosses_the_threshold() {
    // pressure rises 0 -> 1 over 2 s; the tone starts partway up and the
    // level before it is under -60 dB.
    for excite in ["reed", "lip", "jet"] {
        let nodes = vec![
            json!({"type": "ramp", "name": "p", "from": 0, "to": 1, "time": 2.0}),
            json!({"type": "tube", "name": "t", "length": 0.6, "excite": excite, "pressure": "p", "seed": 1}),
        ];
        let x = node_audio(nodes, "t", 2.0);
        assert_finite(&x, "tube");
        let win = (0.05 * SR as f32) as usize;
        let onset = (0..x.len() - win)
            .step_by(win / 2)
            .find(|&a| rms_db(&x[a..a + win]) > -30.0)
            .map(|a| a as f32 / SR as f32);
        println!(
            "{excite}: tone passes -30 dB at pressure {:.2}",
            onset.unwrap_or(f32::NAN) / 2.0
        );
        let onset = onset.unwrap();
        assert!((0.15..0.9).contains(&(onset / 2.0)), "{excite}: {onset}");
        assert!(rms_db(&x[..(0.3 * SR as f32) as usize]) < -60.0);
    }
}

#[test]
fn tube_open_and_closed_ends_change_the_harmonics() {
    // Closed at one end: odd harmonics only. Open at both: the even ones
    // are there too (a jet, which overblows into a full series).
    let harm = |x: &[f32], f0: f32, n: f32| band_db(x, SR, 0.97 * n * f0, 1.03 * n * f0);
    let closed = node_audio(tube(json!({"excite": "reed", "open_end": false})), "t", 2.0);
    let f0 = C / 2.4;
    let (h1, h2, h3) = (
        harm(tail(&closed, 0.5), f0, 1.0),
        harm(tail(&closed, 0.5), f0, 2.0),
        harm(tail(&closed, 0.5), f0, 3.0),
    );
    println!("closed reed: harmonics 1 {h1:.1}, 2 {h2:.1}, 3 {h3:.1} dB");
    assert!(h2 < h1 - 30.0 && h3 > h2 + 5.0, "even harmonics are absent");
    let open = node_audio(tube(json!({"excite": "reed", "open_end": true})), "t", 2.0);
    let f0 = C / 1.2;
    let (o1, o2) = (
        harm(tail(&open, 0.5), f0, 1.0),
        harm(tail(&open, 0.5), f0, 2.0),
    );
    println!("open reed: harmonics 1 {o1:.1}, 2 {o2:.1} dB");
    assert!(
        o2 > o1 - 40.0 && o2 > h2 + 5.0,
        "the second harmonic is present"
    );
}

#[test]
fn tube_bore_darkens_the_tone() {
    // A wider bore radiates more at the open end: the end low-pass drops.
    let mut c = Vec::new();
    for bore in [0.004f32, 0.02, 0.08] {
        let x = node_audio(tube(json!({"excite": "noise", "bore": bore})), "t", 2.0);
        assert_finite(&x, "tube");
        c.push(centroid(tail(&x, 0.5)));
    }
    println!(
        "noise-driven tube, bore 4, 20, 80 mm: centroid {:.0}, {:.0}, {:.0} Hz",
        c[0], c[1], c[2]
    );
    assert!(c[0] > c[1] && c[1] > c[2]);
}

#[test]
fn tube_is_bit_identical_and_seeded() {
    for excite in ["reed", "lip", "jet", "noise"] {
        assert_bit_identical(tube(json!({"excite": excite})), 0.5);
    }
    let a = node_audio(tube(json!({"excite": "noise", "seed": 1})), "t", 0.3);
    let b = node_audio(tube(json!({"excite": "noise", "seed": 2})), "t", 0.3);
    assert!(
        a.iter().zip(&b).any(|(p, q)| p != q),
        "a different seed differs"
    );
}

#[test]
fn tube_is_finite_at_the_extremes() {
    for excite in ["reed", "lip", "jet", "noise"] {
        for (length, bore, p, open) in [
            (0.03, 0.001, 1.0, true),
            (0.03, 0.5, 1.0, false),
            (20.0, 0.001, 1.0, true),
            (20.0, 0.5, 0.5, false),
            (0.6, 0.01, 0.0, true),
            (1.0, 0.001, 1.0, false),
        ] {
            let x = node_audio(
                tube(
                    json!({"excite": excite, "length": length, "bore": bore, "pressure": p, "open_end": open}),
                ),
                "t",
                0.6,
            );
            assert_finite(&x, "tube");
            let m = peak_abs(&x);
            assert!(m < 4.0, "{excite} {length} {bore} {p} {open}: peak {m}");
            println!("{excite} L {length} bore {bore} p {p} open {open}: peak {m:.3}");
        }
    }
    // A wild pressure signal is clamped to 0..1.
    let nodes = vec![
        json!({"type": "osc", "name": "w", "freq": 7, "amp": 100}),
        json!({"type": "tube", "name": "t", "length": 0.5, "pressure": "w"}),
    ];
    let x = node_audio(nodes, "t", 1.0);
    assert_finite(&x, "tube");
    assert!(peak_abs(&x) < 4.0);
}

#[test]
fn tube_errors_say_what_to_do() {
    let e = render_error(json!({"type": "tube", "length": 0.001, "pressure": 0.5}));
    println!("{e}");
    assert!(e.contains("length: 0.001") && e.contains("from 0.0"), "{e}");
    let e = render_error(json!({"type": "tube", "length": 50, "pressure": 0.5}));
    assert!(e.contains("length: 50") && e.contains("20"), "{e}");
    let e = render_error(json!({"type": "tube", "length": 0.5, "pressure": 0.5, "bore": 0.9}));
    assert!(e.contains("bore: 0.9") && e.contains("0.001 to 0.5"), "{e}");
    let e = render_error(json!({"type": "tube", "length": 0.5, "pressure": 1.5}));
    assert!(e.contains("pressure: 1.5") && e.contains("0 to 1"), "{e}");
    let e = render_error(json!({"type": "tube", "length": 0.5, "pressure": 0.5, "excite": "bell"}));
    assert!(e.contains("excite") && e.contains("reed"), "{e}");
}

// ----------------------------------------------------------------- string

#[test]
fn string_pitch_matches_freq() {
    let mut worst: f32 = 0.0;
    for excite in ["pluck", "strike", "bow"] {
        for f in [55.0f32, 110.0, 220.0, 440.0, 880.0, 1760.0] {
            let x = node_audio(string(json!({"freq": f, "excite": excite})), "s", 1.5);
            assert_finite(&x, "string");
            // The fundamental and the 2nd, 3rd partials all sit on the
            // series f, 2f, 3f.
            let seg = tail(&x, 0.2);
            let errs: Vec<f32> = (1..=3)
                .filter(|&n| n as f32 * f < 2500.0)
                .map(|n| partial_hz(seg, n as f32 * f) / (n as f32 * f) - 1.0)
                .collect();
            println!("{excite} {f} Hz: partial errors {errs:.4?}");
            for e in &errs {
                worst = worst.max(e.abs());
                assert!(e.abs() < 0.01, "{excite} {f}: {errs:?}");
            }
        }
    }
    println!("string pitch: worst partial error {:.3} %", worst * 100.0);
}

#[test]
fn string_freq_takes_a_signal() {
    // A 200 -> 400 Hz glide: the fundamental early and late.
    let nodes = vec![
        json!({"type": "ramp", "name": "g", "from": 200, "to": 400, "time": 2.0, "curve": "linear"}),
        json!({"type": "string", "name": "s", "freq": "g", "excite": "bow", "seed": 1}),
    ];
    let x = node_audio(nodes, "s", 2.0);
    assert_finite(&x, "string");
    let at = |t: f32| {
        let a = ((t - 0.05) * SR as f32) as usize;
        &x[a..a + SR as usize / 10]
    };
    let (early, late) = (partial_hz(at(0.5), 250.0), partial_hz(at(1.8), 380.0));
    println!(
        "glide 200 -> 400 Hz over 2 s: 0.5 s {early:.1} Hz (want 250), 1.8 s {late:.1} Hz (want 380)"
    );
    assert!((early / 250.0 - 1.0).abs() < 0.03);
    assert!((late / 380.0 - 1.0).abs() < 0.03);
}

#[test]
fn string_damping_shortens_the_decay() {
    let t: Vec<f32> = [0.0f32, 0.3, 0.6]
        .iter()
        .map(|&d| {
            let x = node_audio(string(json!({"damping": d})), "s", 6.0);
            assert_finite(&x, "string");
            decay_20db(&x)
        })
        .collect();
    println!(
        "damping 0, 0.3, 0.6: 20 dB decay in {:.2}, {:.2}, {:.2} s",
        t[0], t[1], t[2]
    );
    assert!(t[0] > t[1] && t[1] > t[2]);
    assert!(t[0] > 2.0 * t[2]);
}

#[test]
fn string_stiffness_stretches_the_partials() {
    // Inharmonicity: how far partials 5 to 8 sit above whole multiples of
    // the measured fundamental, averaged.
    let inh: Vec<(f32, f32)> = [0.0f32, 0.1, 0.25, 0.5, 1.0]
        .iter()
        .map(|&s| {
            let x = node_audio(
                string(json!({"stiffness": s, "damping": 0.0, "position": 0.13})),
                "s",
                2.0,
            );
            assert_finite(&x, "string");
            let f1 = partial_hz(&x, 220.0);
            let mean = (5..=8)
                .map(|n| {
                    let h = n as f32 * f1;
                    partial_between(&x, h * 0.97, h * 1.12) / h - 1.0
                })
                .sum::<f32>()
                / 4.0;
            (f1, mean)
        })
        .collect();
    println!(
        "stiffness 0, 0.1, 0.25, 0.5, 1: inharmonicity (partials 5-8) {:.4}, {:.4}, {:.4}, {:.4}, {:.4}; fundamentals {:.2?}",
        inh[0].1,
        inh[1].1,
        inh[2].1,
        inh[3].1,
        inh[4].1,
        inh.iter().map(|p| p.0).collect::<Vec<_>>()
    );
    assert!(inh[0].1.abs() < 0.002, "no stiffness: harmonic");
    // Three settings (0, 0.5, 1) must rise; the in-between ones show the
    // curve is monotone too.
    for w in inh.windows(2) {
        assert!(w[1].1 > w[0].1 + 0.0005, "{inh:?}");
    }
    for (f1, _) in &inh {
        assert!((f1 / 220.0 - 1.0).abs() < 0.01, "the pitch is kept: {f1}");
    }
}

#[test]
fn string_position_puts_nulls_in_the_spectrum() {
    // Excited a quarter of the way along, the 4th harmonic is missing; in
    // the middle, the 2nd and 4th.
    let harm = |x: &[f32], n: f32| band_db(x, SR, 0.98 * n * 220.0, 1.02 * n * 220.0);
    let h: Vec<[f32; 4]> = [0.15f32, 0.25, 0.5]
        .iter()
        .map(|&p| {
            let x = node_audio(string(json!({"position": p, "damping": 0.1})), "s", 1.0);
            [harm(&x, 1.0), harm(&x, 2.0), harm(&x, 3.0), harm(&x, 4.0)]
        })
        .collect();
    for (p, h) in [0.15, 0.25, 0.5].iter().zip(&h) {
        println!("position {p}: harmonics 1-4 {h:.1?} dB");
    }
    // The bridge reflection is lossy and the nut is not, so the nulls are
    // finite (about 13 to 23 dB here), not infinite.
    assert!(h[1][3] < h[0][3] - 10.0, "0.25 removes the 4th");
    assert!(
        h[2][1] < h[0][1] - 15.0 && h[2][3] < h[0][3] - 10.0,
        "0.5 removes the 2nd and 4th"
    );
}

#[test]
fn string_bow_pressure_sets_the_bow_down() {
    let run = |p: f32| {
        let x = node_audio(
            string(json!({"excite": "bow", "bow_pressure": p})),
            "s",
            2.0,
        );
        assert_finite(&x, "string");
        x
    };
    let (zero, mid, hard) = (run(0.0), run(0.5), run(0.9));
    let (lz, lm, lh) = (
        rms_db(tail(&zero, 1.0)),
        rms_db(tail(&mid, 1.0)),
        rms_db(tail(&hard, 1.0)),
    );
    println!("bow pressure 0, 0.5, 0.9: rms {lz:.1}, {lm:.1}, {lh:.1} dB");
    assert!(lz < -100.0, "a lifted bow is silent");
    assert!(lm > -30.0 && lh > -30.0);
    for x in [&mid, &hard] {
        let f = partial_hz(tail(x, 1.0), 220.0);
        println!(
            "steady bow: fundamental {f:.2} Hz; rms 1-1.5 s vs 1.5-2 s differ {:.2} dB",
            (rms_db(&x[SR as usize..3 * SR as usize / 2]) - rms_db(tail(x, 1.5))).abs()
        );
        assert!((f / 220.0 - 1.0).abs() < 0.01);
        let (a, b) = (
            rms_db(&x[SR as usize..3 * SR as usize / 2]),
            rms_db(tail(x, 1.5)),
        );
        assert!((a - b).abs() < 1.0, "steady: {a} vs {b}");
    }
}

#[test]
fn string_lifting_the_bow_lets_it_ring_down() {
    let nodes = vec![
        json!({"type": "ramp", "name": "p", "from": 0.6, "to": 0.0, "time": 0.05, "at": 1.0, "curve": "linear"}),
        json!({"type": "string", "name": "s", "freq": 220, "excite": "bow", "bow_pressure": "p", "damping": 0.6, "seed": 1}),
    ];
    let x = node_audio(nodes, "s", 2.0);
    assert_finite(&x, "string");
    let (during, after) = (
        rms_db(&x[SR as usize / 2..SR as usize]),
        rms_db(tail(&x, 1.8)),
    );
    println!("bow lifted at 1 s: rms 0.5-1 s {during:.1} dB, 1.8-2 s {after:.1} dB");
    assert!(after < during - 30.0);
}

#[test]
fn string_pluck_and_strike_differ_and_seeds_differ() {
    let pluck = node_audio(string(json!({"excite": "pluck"})), "s", 1.0);
    let strike = node_audio(string(json!({"excite": "strike"})), "s", 1.0);
    let other = node_audio(string(json!({"excite": "pluck", "seed": 2})), "s", 1.0);
    println!(
        "centroid pluck {:.0} Hz, strike {:.0} Hz; peak {:.3}, {:.3}",
        centroid(&pluck),
        centroid(&strike),
        peak_abs(&pluck),
        peak_abs(&strike)
    );
    assert!(pluck.iter().zip(&strike).any(|(a, b)| a != b));
    assert!(
        pluck.iter().zip(&other).any(|(a, b)| a != b),
        "seeds differ"
    );
    assert!(peak_abs(&pluck) < 1.5 && peak_abs(&strike) < 1.5);
    // A strike has no randomness: its seed changes nothing.
    let s2 = node_audio(string(json!({"excite": "strike", "seed": 9})), "s", 1.0);
    assert!(
        strike
            .iter()
            .zip(&s2)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn string_is_bit_identical() {
    for excite in ["pluck", "bow", "strike"] {
        assert_bit_identical(string(json!({"excite": excite, "stiffness": 0.5})), 0.5);
    }
}

#[test]
fn string_is_finite_at_the_extremes() {
    for excite in ["pluck", "strike", "bow"] {
        for (f, pos, damp, stiff, bp) in [
            (20.0, 0.0, 0.0, 1.0, 1.0),
            (20.0, 1.0, 1.0, 0.0, 0.0),
            (3000.0, 0.0, 0.0, 1.0, 1.0),
            (3000.0, 1.0, 1.0, 1.0, 0.5),
            (220.0, 0.5, 0.0, 1.0, 1.0),
            (220.0, 0.0, 0.0, 0.0, 1.0),
        ] {
            let x = node_audio(
                string(
                    json!({"freq": f, "excite": excite, "position": pos, "damping": damp, "stiffness": stiff, "bow_pressure": bp}),
                ),
                "s",
                1.0,
            );
            assert_finite(&x, "string");
            let m = peak_abs(&x);
            assert!(m < 4.0, "{excite} {f} {pos} {damp} {stiff} {bp}: peak {m}");
            println!("{excite} f {f} pos {pos} damp {damp} stiff {stiff} bow {bp}: peak {m:.3}");
        }
    }
    // A wild frequency signal is clamped to the model's range.
    let nodes = vec![
        json!({"type": "osc", "name": "w", "freq": 9, "amp": 100000}),
        json!({"type": "string", "name": "s", "freq": "w", "excite": "bow"}),
    ];
    let x = node_audio(nodes, "s", 1.0);
    assert_finite(&x, "string");
    assert!(peak_abs(&x) < 4.0);
}

#[test]
fn string_errors_say_what_to_do() {
    let e = render_error(json!({"type": "string", "freq": 5}));
    println!("{e}");
    assert!(e.contains("freq: 5") && e.contains("20 to 3000"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 5000}));
    assert!(e.contains("freq: 5000"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 220, "position": 2}));
    assert!(e.contains("position: 2") && e.contains("0 to 1"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 220, "damping": -1}));
    assert!(e.contains("damping: -1"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 220, "stiffness": 3}));
    assert!(e.contains("stiffness: 3"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 220, "bow_pressure": 2}));
    assert!(e.contains("bow_pressure: 2"), "{e}");
    let e = render_error(json!({"type": "string", "freq": 220, "excite": "kick"}));
    assert!(e.contains("excite") && e.contains("pluck"), "{e}");
}

#[test]
fn tube_and_string_follow_the_sample_rate() {
    for sr in [44_100u32, 96_000] {
        let mut d = SoundDoc::named("test");
        d.sample_rate = sr;
        d.duration = Some(1.5);
        d.nodes = vec![
            json!({"type": "tube", "name": "t", "length": 0.6, "excite": "reed", "pressure": 0.9, "open_end": false}),
            json!({"type": "string", "name": "s", "freq": 220, "excite": "bow"}),
            json!({"type": "output", "name": "out", "input": "t"}),
        ];
        let r = d.render(&RenderOpts::default()).unwrap();
        let x = &r.audio.channels[0];
        let t = peak_hz(&x[sr as usize / 2..], sr);
        let want = C / 2.4;
        println!("{sr} Hz: tube {t:.1} Hz (want {want:.1})");
        assert!((t / want - 1.0).abs() < 0.03);
        d.nodes[2] = json!({"type": "output", "name": "out", "input": "s"});
        let r = d.render(&RenderOpts::default()).unwrap();
        let x = &r.audio.channels[0];
        let t = spectrum(&x[sr as usize / 2..], sr)
            .into_iter()
            .filter(|p| (213.4..226.6).contains(&p.0))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
            .0;
        println!("{sr} Hz: string {t:.2} Hz (want 220)");
        assert!((t / 220.0 - 1.0).abs() < 0.01, "{t}");
    }
}

/// The nodes of examples/waveguide.rs (which the root crate runs; this
/// test renders the same graph without it).
#[test]
fn the_example_document_renders_as_described() {
    let mut d = SoundDoc::named("waveguide");
    d.duration = Some(3.0);
    d.nodes = vec![
        json!({"type": "ramp", "name": "breath_up", "from": 0.0, "to": 0.8, "time": 0.08, "curve": "linear"}),
        json!({"type": "ramp", "name": "breath_down", "from": 1.0, "to": 0.0, "time": 0.1, "at": 1.2, "curve": "linear"}),
        json!({"type": "math", "name": "breath", "op": "mul", "a": "breath_up", "b": "breath_down"}),
        json!({"type": "tube", "name": "pipe", "length": 0.6, "excite": "reed", "pressure": "breath", "open_end": false, "seed": 1}),
        json!({"type": "ramp", "name": "press", "from": 0.6, "to": 0.0, "time": 0.05, "at": 2.2, "curve": "linear"}),
        json!({"type": "string", "name": "fiddle", "freq": 220, "excite": "bow", "bow_pressure": "press", "damping": 0.5, "stiffness": 0.2, "seed": 1}),
        json!({"type": "fade", "name": "fiddle_in", "input": "fiddle", "in": 0.3}),
        json!({"type": "mix", "name": "both", "inputs": ["pipe", "fiddle_in"]}),
        json!({"type": "output", "name": "out", "input": "both", "lufs": -20}),
    ];
    let r = d.render(&RenderOpts::default()).unwrap();
    let x = &r.audio.channels[0];
    assert_finite(x, "example");
    let at = |a: f32, b: f32| &x[(a * SR as f32) as usize..(b * SR as f32) as usize];
    let (pipe, both, ring) = (
        rms_db(at(0.5, 1.1)),
        rms_db(at(1.5, 2.1)),
        rms_db(at(2.8, 3.0)),
    );
    println!(
        "example: rms 0.5-1.1 s {pipe:.1} dB (pipe + bow), 1.5-2.1 s {both:.1} dB (bow only), 2.8-3.0 s {ring:.1} dB; pipe fundamental {:.1} Hz, string {:.1} Hz; peak {:.3}",
        partial_hz(at(0.5, 1.1), 142.9),
        partial_hz(at(1.5, 2.1), 220.0),
        peak_abs(x)
    );
    assert!((partial_hz(at(0.5, 1.1), 142.9) / 142.9 - 1.0).abs() < 0.03);
    assert!((partial_hz(at(1.5, 2.1), 220.0) / 220.0 - 1.0).abs() < 0.01);
    assert!(
        ring < both - 20.0,
        "the string rings down once the bow lifts"
    );
}
