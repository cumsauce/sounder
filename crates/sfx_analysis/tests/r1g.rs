//! R1-G: `thunder` `brightness` / `length`, `gain` `gain_db`.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::stft::stft;
use sfx_graph::{RenderOpts, SoundDoc};

fn fnv(x: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn thunder(extra: Value) -> Vec<f32> {
    let mut t = json!({"type": "thunder", "name": "t", "distance": 600, "seed": 3});
    t.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    node_audio(vec![t], "t", 12.0)
}

fn centroid(x: &[f32]) -> f32 {
    let s = stft(x, SR, 2048, 1024);
    let mean = s.mean(0..s.frames.len());
    let total: f32 = mean.iter().sum();
    mean.iter()
        .enumerate()
        .map(|(b, p)| s.hz(b) * p)
        .sum::<f32>()
        / total.max(1e-20)
}

/// (first, last) seconds where the 100 ms rms is within 30 dB of the
/// loudest 100 ms.
fn audible_s(x: &[f32]) -> (f32, f32) {
    let w = SR as usize / 10;
    let env: Vec<f32> = x
        .chunks(w)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let top = env.iter().fold(0.0f32, |a, v| a.max(*v));
    let floor = top * 10f32.powf(-30.0 / 20.0);
    let first = env.iter().position(|e| *e > floor).unwrap_or(0);
    let last = env.iter().rposition(|e| *e > floor).unwrap_or(0);
    (first as f32 * 0.1, (last + 1) as f32 * 0.1)
}

/// FNV-1a of the f32 bits of the node, recorded before `brightness` and
/// `length` existed.
const BASELINES: [(&str, u64); 3] = [
    ("default", 0x6f38_e1e7_f185_1bf5),
    ("exposure", 0xae10_af28_2243_6477),
    ("near", 0x9d35_d7b6_76fb_22df),
];

fn baseline_renders() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        ("default", thunder(json!({}))),
        ("exposure", thunder(json!({"exposure": 0.4}))),
        ("near", thunder(json!({"distance": 150, "seed": 9}))),
    ]
}

#[test]
fn defaults_are_bit_identical_to_before() {
    for ((name, x), (_, want)) in baseline_renders().iter().zip(BASELINES) {
        println!("{name}: hash {:016x}", fnv(x));
        assert_eq!(fnv(x), want, "{name}");
    }
}

fn gain_doc(extra: Value) -> Result<Vec<f32>, String> {
    let mut g = json!({"type": "gain", "name": "g", "input": "src"});
    g.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let mut d = SoundDoc::named("test");
    d.nodes = vec![
        json!({"type": "osc", "name": "src", "freq": 1000, "amp": 0.5}),
        g,
        json!({"type": "output", "name": "out", "input": "g"}),
    ];
    d.duration = Some(0.2);
    d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    })
    .map(|r| r.audio.channels[0].clone())
}

#[test]
fn gain_db_is_db() {
    let a = gain_doc(json!({"db": -6})).unwrap();
    let b = gain_doc(json!({"gain_db": -6})).unwrap();
    println!(
        "db -6 rms {:.2} dB, gain_db -6 rms {:.2} dB",
        rms_db(&a),
        rms_db(&b)
    );
    assert_eq!(a, b, "gain_db -6 differs from db -6");
    // -6 dB on a 0.5 amplitude sine: rms -9.03 - 6 = about -15 dB (power).
    let full = gain_doc(json!({"db": 0})).unwrap();
    assert!((rms_db(&full) - rms_db(&a) - 6.0).abs() < 0.2);
    // A signal works too (a param).
    let mut d = SoundDoc::named("test");
    d.nodes = vec![
        json!({"type": "osc", "name": "src", "freq": 1000, "amp": 0.5}),
        json!({"type": "gain", "name": "g", "input": "src", "gain_db": "$g"}),
        json!({"type": "output", "name": "out", "input": "g"}),
    ];
    d.params.insert("g".into(), -6.0);
    d.duration = Some(0.2);
    let r = d.render(&RenderOpts::default()).unwrap();
    assert_eq!(r.audio.channels[0], a);
}

#[test]
fn gain_db_and_db_together_is_an_error() {
    let e = gain_doc(json!({"db": -6, "gain_db": -6})).unwrap_err();
    println!("both: {e}");
    assert!(e.contains("duplicate") || e.contains("both"), "{e}");
    assert!(e.contains("db"), "{e}");
    let e = gain_doc(json!({"gain_db": -6, "gain": 0.5})).unwrap_err();
    println!("gain_db + gain: {e}");
    assert!(e.contains("exactly one"), "{e}");
}

#[test]
fn explicit_neutral_values_are_the_default() {
    let base = thunder(json!({}));
    assert_eq!(base, thunder(json!({"brightness": 0.5, "length": 1})));
}

#[test]
fn brightness_raises_the_centroid_monotonically() {
    let mut prev = 0.0f32;
    let mut peaks = vec![];
    for b in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let x = thunder(json!({"brightness": b}));
        assert_finite(&x, "thunder");
        let c = centroid(&x);
        let above = band_db(&x, SR, 100.0, 20_000.0) - band_db(&x, SR, 1.0, 100.0);
        let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!(
            "brightness {b}: centroid {c:.0} Hz, >100 Hz vs <100 Hz {above:.1} dB, peak {pk:.3}"
        );
        assert!(c > prev, "brightness {b}: {c} not above {prev}");
        prev = c;
        peaks.push(pk);
    }
    // The peak is kept (the tilt changes tone, not level).
    let p0 = peaks[2];
    for p in &peaks {
        assert!((p / p0 - 1.0).abs() < 0.02, "peaks {peaks:?}");
    }
}

#[test]
fn brightness_moves_energy_above_100_hz() {
    let dark = thunder(json!({"brightness": 0.0}));
    let bright = thunder(json!({"brightness": 1.0}));
    let ratio = |x: &[f32]| band_db(x, SR, 100.0, 20_000.0) - band_db(x, SR, 1.0, 100.0);
    let (d, b) = (ratio(&dark), ratio(&bright));
    println!("above/below 100 Hz: dark {d:.1} dB, bright {b:.1} dB");
    assert!(b - d > 20.0, "{d} {b}");
}

#[test]
fn length_stretches_the_rumble() {
    let base = thunder(json!({}));
    let (first0, last0) = audible_s(&base);
    println!(
        "length 1: audible {first0:.2}-{last0:.2} s, centroid {:.0} Hz",
        centroid(&base)
    );
    let mut prev = last0;
    for (l, lo, hi) in [(2.0f32, 1.5, 2.4), (3.0, 2.2, 3.5)] {
        let x = thunder(json!({"length": l}));
        assert_finite(&x, "thunder");
        let (first, last) = audible_s(&x);
        let rumble = (last - first) / (last0 - first0);
        let c = centroid(&x);
        println!(
            "length {l}: audible {first:.2}-{last:.2} s ({rumble:.2}x the rumble), centroid {c:.0} Hz, rms {:.1} dB",
            rms_db(&x)
        );
        assert!(
            (first - first0).abs() < 0.1,
            "arrival moved: {first} vs {first0}"
        );
        assert!((lo..=hi).contains(&rumble), "length {l}: {rumble}x");
        assert!(last > prev, "{last} vs {prev}");
        prev = last;
        // The spectrum stays: the stretch is not a resample.
        let c0 = centroid(&base);
        assert!((c / c0 - 1.0).abs() < 0.4, "centroid {c} vs {c0}");
    }
    let short = thunder(json!({"length": 0.5}));
    let (f, l) = audible_s(&short);
    println!("length 0.5: audible {f:.2}-{l:.2} s");
    let rumble = (l - f) / (last0 - first0);
    assert!((0.35..=0.7).contains(&rumble), "length 0.5: {rumble}x");
}

#[test]
fn length_keeps_the_clap_and_is_bit_identical() {
    let base = thunder(json!({}));
    let long = thunder(json!({"length": 2}));
    // The clap: the first 0.1 s after the arrival matches in level.
    let a = base.iter().position(|v| *v != 0.0).unwrap();
    let n = (0.1 * SR as f32) as usize;
    let (r0, r1) = (rms_db(&base[a..a + n]), rms_db(&long[a..a + n]));
    println!("clap rms: length 1 {r0:.1} dB, length 2 {r1:.1} dB");
    assert!((r0 - r1).abs() < 3.0);
    assert_bit_identical(
        vec![
            json!({"type": "thunder", "name": "t", "distance": 600, "length": 2.5, "brightness": 0.9, "seed": 3}),
        ],
        12.0,
    );
}

#[test]
fn thunder_new_fields_are_validated() {
    let err = |extra: Value| {
        let mut t = json!({"type": "thunder", "name": "t"});
        t.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut d = SoundDoc::named("test");
        d.nodes = vec![t, json!({"type": "output", "name": "o", "input": "t"})];
        d.duration = Some(3.0);
        d.render(&RenderOpts::default()).unwrap_err()
    };
    for (extra, field) in [
        (json!({"brightness": 1.5}), "brightness"),
        (json!({"brightness": -0.1}), "brightness"),
        (json!({"length": 0.2}), "length"),
        (json!({"length": 4}), "length"),
    ] {
        let e = err(extra);
        println!("{e}");
        assert!(e.contains(field) && e.contains("give"), "{e}");
    }
}
