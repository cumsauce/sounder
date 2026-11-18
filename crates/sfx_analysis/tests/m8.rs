//! M8: `glottis` roughness / subharmonic / fry and `tract` nasal /
//! frication / growl. The first test pins the default outputs (recorded
//! before the new fields existed); the rest measure each field.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::measure::roughness_pct;
use sfx_graph::{RenderOpts, SoundDoc};

/// FNV-1a over the bit patterns of every sample.
fn hash(x: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// (label, nodes, node to hash) for the default-output pins.
fn pins() -> Vec<(&'static str, Vec<Value>, &'static str)> {
    let g = |extra: Value| {
        // natural: false keeps the pre-voice_fix output these pins hold.
        let mut o = json!({"type": "glottis", "name": "g", "f0": 140, "natural": false});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    };
    let t = |extra: Value| {
        let mut o = json!({"type": "tract", "name": "t", "input": "g", "natural": false});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    };
    vec![
        ("glottis default", vec![g(json!({}))], "g"),
        (
            "glottis breathy vibrato seeded",
            vec![g(
                json!({"tension": 0.1, "breath": 0.3, "vibrato_cents": 30, "vibrato_hz": 6, "seed": 3}),
            )],
            "g",
        ),
        (
            "glottis pressed, amp, no breath",
            vec![g(json!({"tension": 1, "breath": 0, "amp": 0.5, "f0": 90}))],
            "g",
        ),
        (
            "glottis f0 ramp",
            vec![
                json!({"type": "ramp", "name": "r", "from": 80, "to": 300, "time": 0.4}),
                g(json!({"f0": "r", "seed": 9})),
            ],
            "g",
        ),
        ("tract default", vec![g(json!({})), t(json!({}))], "t"),
        (
            "tract e",
            vec![g(json!({"seed": 2})), t(json!({"vowel": "e"}))],
            "t",
        ),
        (
            "tract o to i, morph, scale",
            vec![
                g(json!({"vibrato_cents": 20})),
                t(json!({"vowel": "o", "vowel_to": "i", "morph": 0.4, "scale": 1.3})),
            ],
            "t",
        ),
        (
            "tract formants list",
            vec![
                g(json!({"f0": 200})),
                t(json!({"formants": [[400, 60], [1500, 100], [2600, 150]]})),
            ],
            "t",
        ),
        (
            "tract on noise",
            vec![
                json!({"type": "noise", "name": "g", "color": "white"}),
                t(json!({"vowel": "u"})),
            ],
            "t",
        ),
    ]
}

const PINNED: &[(&str, u64)] = &[
    ("glottis default", 0x7733151fc79996c2),
    ("glottis breathy vibrato seeded", 0xc97c06f84fe3d7e5),
    ("glottis pressed, amp, no breath", 0x2133b0cf5a01bfa4),
    ("glottis f0 ramp", 0xa6945f0a49f3602a),
    ("tract default", 0x961146eeb06f2fd3),
    ("tract e", 0x0e422db4677f7f59),
    ("tract o to i, morph, scale", 0xae635e6fd25789e8),
    ("tract formants list", 0xe82fa998a4780f1c),
    ("tract on noise", 0x99934a530705f412),
];

#[test]
fn defaults_are_bit_identical_to_before() {
    for (label, nodes, node) in pins() {
        let h = hash(&node_audio(&nodes, node, 0.5));
        eprintln!("PIN {label}: {h:#018x}");
        let want = PINNED.iter().find(|(l, _)| *l == label).unwrap().1;
        assert_eq!(h, want, "{label}: default output changed");
    }
}

// ------------------------------------------------------------ helpers

/// A glottis named `g` with `extra` fields merged in.
fn glottis(f0: f32, extra: Value) -> Value {
    let mut o = json!({"type": "glottis", "name": "g", "f0": f0, "breath": 0, "natural": false});
    for (k, v) in extra.as_object().unwrap() {
        o[k] = v.clone();
    }
    o
}

/// Power-weighted mean frequency of `x`, Hz.
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

/// The 20 to 150 Hz envelope share (S8a `roughness`) of `nodes`, read from
/// the output stage of a 1 s render.
fn rough(nodes: &[Value]) -> f32 {
    roughness_pct(&render(nodes, 1.0))
}

/// Silence for a tract that should only add its own noise.
fn silence() -> Value {
    json!({"type": "osc", "name": "g", "freq": 100, "amp": 0})
}

/// A tract whose single formant sits past 0.45 x sr, so the cascade is a
/// flat pass-through (x 0.8) and only the new stages shape the sound.
fn flat_tract(extra: Value) -> Value {
    let mut o = json!({"type": "tract", "name": "t", "input": "g", "formants": [[30000, 100]]});
    for (k, v) in extra.as_object().unwrap() {
        o[k] = v.clone();
    }
    o
}

fn white() -> Value {
    json!({"type": "noise", "name": "g", "color": "white"})
}

/// The error text of a document that must not render.
fn err(nodes: &[Value]) -> String {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes.to_vec();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "t"}));
    d.duration = Some(0.2);
    match d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    }) {
        Ok(_) => panic!("rendered, expected an error"),
        Err(e) => e.to_string(),
    }
}

fn pin(label: &str) -> u64 {
    PINNED.iter().find(|(l, _)| *l == label).unwrap().1
}

// -------------------------------------------------------------- glottis

#[test]
fn fields_at_zero_equal_the_defaults() {
    // The pins use the default breath (0.05), so no `glottis()` helper here.
    let plain = json!({"type": "glottis", "name": "g", "f0": 140, "natural": false,
        "roughness": 0, "subharmonic": 0, "fry": 0});
    let x = node_audio([plain], "g", 0.5);
    assert_eq!(hash(&x), pin("glottis default"), "glottis fields at 0");
    let t = node_audio(
        [
            json!({"type": "glottis", "name": "g", "f0": 140, "natural": false}),
            json!({"type": "tract", "name": "t", "input": "g", "natural": false, "nasal": 0, "growl": 0,
                "frication": {"freq": 4000, "level": 0}}),
        ],
        "t",
        0.5,
    );
    assert_eq!(hash(&t), pin("tract default"), "tract fields at 0");
}

#[test]
fn roughness_raises_the_envelope_roughness() {
    // f0 250 keeps the pulse train's own envelope out of 20..150 Hz.
    let mut prev = -1.0f32;
    let mut first = 0.0;
    for r in [0.0, 0.5, 1.0] {
        let v = rough(&[glottis(250.0, json!({"roughness": r, "seed": 1}))]);
        eprintln!("glottis roughness {r}: S8a roughness {v:.2} %");
        assert!(v > prev, "roughness {r}: {v} not above {prev}");
        if r == 0.0 {
            first = v;
        }
        prev = v;
    }
    assert!(prev > 1.5 * first.max(0.1), "{prev} vs {first}");
}

#[test]
fn roughness_is_seeded() {
    let make = |seed: u64| {
        node_audio(
            [glottis(200.0, json!({"roughness": 0.8, "seed": seed}))],
            "g",
            0.4,
        )
    };
    assert_eq!(hash(&make(4)), hash(&make(4)));
    assert_ne!(hash(&make(4)), hash(&make(5)));
}

#[test]
fn roughness_keeps_the_pitch() {
    // Jitter is zero-mean: the YIN pitch stays near f0 at roughness 0.5.
    let x = node_audio(
        [glottis(200.0, json!({"roughness": 0.5, "seed": 2}))],
        "g",
        1.0,
    );
    let mut v: Vec<f32> = sfx_analysis::pitch::yin(&x, SR)
        .iter()
        .filter_map(|f| f.hz)
        .collect();
    v.sort_by(f32::total_cmp);
    let hz = v[v.len() / 2];
    eprintln!("roughness 0.5: median YIN pitch {hz:.1} Hz (f0 200)");
    assert!((hz - 200.0).abs() < 12.0, "{hz}");
}

#[test]
fn subharmonic_puts_a_peak_at_half_f0() {
    let f0 = 200.0;
    let mut prev = f32::MIN;
    let mut at_one = (0.0, 0.0);
    for b in [0.0, 0.25, 0.5, 1.0] {
        let x = node_audio([glottis(f0, json!({"subharmonic": b}))], "g", 1.0);
        let half = band_db(&x, SR, 0.5 * f0 - 4.0, 0.5 * f0 + 4.0);
        let full = band_db(&x, SR, f0 - 4.0, f0 + 4.0);
        eprintln!("glottis subharmonic {b}: f0/2 {half:.1} dB, f0 {full:.1} dB");
        assert!(half > prev, "subharmonic {b}: {half} not above {prev}");
        prev = half;
        if b == 0.0 {
            assert!(half < -100.0, "no subharmonic at 0: {half}");
        }
        if b == 1.0 {
            at_one = (half, full);
        }
    }
    assert!(at_one.0 > at_one.1 - 10.0, "{at_one:?}");
    // At 1 the strongest low peak is at f0 / 2.
    let x = node_audio([glottis(f0, json!({"subharmonic": 1}))], "g", 1.0);
    let (hz, _) = peaks(&x, SR, 6, 60.0)
        .into_iter()
        .find(|(f, _)| *f < 150.0)
        .unwrap();
    assert!((hz - 100.0).abs() < 2.0, "{hz}");
}

#[test]
fn fry_raises_the_envelope_roughness_and_adds_half_f0() {
    let mut prev = -1.0f32;
    for f in [0.0, 0.5, 1.0] {
        let v = rough(&[glottis(250.0, json!({"fry": f, "seed": 1}))]);
        eprintln!("glottis fry {f}: S8a roughness {v:.2} %");
        assert!(v > prev, "fry {f}: {v} not above {prev}");
        prev = v;
    }
    let off = node_audio([glottis(200.0, json!({}))], "g", 1.0);
    let on = node_audio([glottis(200.0, json!({"fry": 1, "seed": 1}))], "g", 1.0);
    let (a, b) = (
        band_db(&off, SR, 90.0, 110.0),
        band_db(&on, SR, 90.0, 110.0),
    );
    eprintln!("fry 1: band 90..110 Hz {b:.1} dB (off {a:.1})");
    assert!(b > a + 30.0, "{b} vs {a}");
}

// ---------------------------------------------------------------- tract

#[test]
fn nasal_adds_a_murmur_and_an_antiformant() {
    let bands = |n: f32| {
        let x = node_audio([white(), flat_tract(json!({"nasal": n}))], "t", 1.0);
        (
            band_db(&x, SR, 200.0, 300.0),
            band_db(&x, SR, 900.0, 1100.0),
            band_db(&x, SR, 4000.0, 8000.0),
        )
    };
    let off = bands(0.0);
    let (mut low_prev, mut mid_prev) = (off.0, off.1);
    for n in [0.5, 1.0] {
        let b = bands(n);
        eprintln!(
            "tract nasal {n}: 200-300 {:+.1} dB, 900-1100 {:+.1} dB, 4k-8k {:+.1} dB vs off",
            b.0 - off.0,
            b.1 - off.1,
            b.2 - off.2
        );
        assert!(b.0 > low_prev && b.1 < mid_prev, "{n}: {b:?}");
        assert!((b.2 - off.2).abs() < 0.5, "highs pass unchanged: {b:?}");
        (low_prev, mid_prev) = (b.0, b.1);
    }
    assert!(low_prev > off.0 + 5.0, "murmur rises >= 5 dB");
    assert!(mid_prev < off.1 - 10.0, "antiformant falls >= 10 dB");
}

#[test]
fn frication_band_follows_freq_and_level() {
    let mut prev = 0.0f32;
    for f in [1500.0f32, 4000.0, 9000.0] {
        let x = node_audio(
            [
                silence(),
                flat_tract(json!({"frication": {"freq": f, "level": 1}})),
            ],
            "t",
            1.0,
        );
        let c = centroid(&x);
        eprintln!(
            "frication freq {f}: centroid {c:.0} Hz, rms {:.1} dB",
            rms_db(&x)
        );
        assert!(c > prev * 1.5, "{f}: {c} vs {prev}");
        assert!(c > 0.6 * f && c < 1.6 * f, "centroid {c} near {f}");
        prev = c;
    }
    // Level: 20 dB a decade.
    let mut levels = Vec::new();
    for l in [0.01f32, 0.1, 1.0] {
        let x = node_audio(
            [
                silence(),
                flat_tract(json!({"frication": {"freq": 5000, "level": l}})),
            ],
            "t",
            1.0,
        );
        levels.push(rms_db(&x));
    }
    eprintln!("frication level 0.01 / 0.1 / 1: rms {levels:.1?} dB");
    for w in levels.windows(2) {
        assert!((w[1] - w[0] - 20.0).abs() < 1.5, "{levels:?}");
    }
}

#[test]
fn frication_takes_signals_for_freq_and_level() {
    let x = node_audio(
        [
            silence(),
            json!({"type": "ramp", "name": "hz", "from": 1500, "to": 10000, "time": 1.0}),
            json!({"type": "ramp", "name": "lv", "from": 0, "to": 1, "time": 1.0}),
            flat_tract(json!({"frication": {"freq": "hz", "level": "lv"}})),
        ],
        "t",
        1.0,
    );
    let q = x.len() / 4;
    let (a, b) = (&x[..q], &x[3 * q..]);
    let (ca, cb) = (centroid(a), centroid(b));
    let (ra, rb) = (rms_db(a), rms_db(b));
    eprintln!(
        "frication ramps: first quarter centroid {ca:.0} Hz rms {ra:.1}, last {cb:.0} Hz rms {rb:.1}"
    );
    assert!(cb > 2.0 * ca);
    assert!(rb > ra + 15.0);
}

#[test]
fn growl_raises_the_envelope_roughness() {
    let mut prev = -1.0f32;
    let mut first = 0.0;
    for g in [0.0, 0.5, 1.0] {
        let v = rough(&[
            glottis(250.0, json!({"breath": 0.05})),
            json!({"type": "tract", "name": "t", "input": "g", "growl": g, "seed": 1}),
        ]);
        eprintln!("tract growl {g}: S8a roughness {v:.2} %");
        assert!(v > prev, "growl {g}: {v} not above {prev}");
        if g == 0.0 {
            first = v;
        }
        prev = v;
    }
    assert!(prev > 4.0 * first.max(0.1), "{prev} vs {first}");
}

// ------------------------------------------------- determinism, extremes

fn everything() -> Vec<Value> {
    vec![
        json!({"type": "ramp", "name": "pitch", "from": 90, "to": 220, "time": 0.6}),
        json!({"type": "ramp", "name": "fz", "from": 2000, "to": 8000, "time": 0.6}),
        json!({"type": "wander", "name": "lv", "range": [0.0, 0.2], "rate": 3}),
        glottis(
            0.0,
            json!({"f0": "pitch", "breath": 0.1, "roughness": 0.4, "subharmonic": 0.3,
                "fry": 0.5, "seed": 7}),
        ),
        json!({"type": "tract", "name": "t", "input": "g", "vowel": "o", "vowel_to": "i",
            "morph": 0.5, "nasal": 0.6, "growl": 0.4, "seed": 3,
            "frication": {"freq": "fz", "level": "lv"}}),
    ]
}

#[test]
fn new_fields_are_bit_identical_on_any_thread_count() {
    assert_bit_identical(everything(), 0.6);
}

#[test]
fn recipe_renders() {
    let x = node_audio(everything(), "t", 0.6);
    assert_eq!(x.len(), 28_800);
    assert_finite(&x, "recipe");
    eprintln!(
        "recipe: rms {:.2} dB, peak {:.2}",
        rms_db(&x),
        x.iter().fold(0f32, |m, v| m.max(v.abs()))
    );
    assert!(rms_db(&x) > -40.0);
}

#[test]
fn extremes_stay_finite() {
    for f0 in [0.0f32, 1.0, 30_000.0] {
        for v in [0.0f32, 1.0] {
            let x = node_audio(
                [
                    glottis(
                        f0,
                        json!({"roughness": v, "subharmonic": v, "fry": v, "tension": 5, "breath": 3}),
                    ),
                    json!({"type": "tract", "name": "t", "input": "g", "nasal": v, "growl": v,
                        "scale": 10, "frication": {"freq": 21_600, "level": 1e6}}),
                ],
                "t",
                0.3,
            );
            assert_finite(&x, &format!("f0 {f0} fields {v}"));
        }
    }
    // Signals far outside their ranges are clamped, not trusted.
    let x = node_audio(
        [
            json!({"type": "ramp", "name": "wild", "from": -5, "to": 5, "time": 0.3}),
            json!({"type": "ramp", "name": "hz", "from": -1e9, "to": 1e9, "time": 0.3}),
            glottis(
                150.0,
                json!({"roughness": "wild", "subharmonic": "wild", "fry": "wild"}),
            ),
            json!({"type": "tract", "name": "t", "input": "g", "nasal": "wild", "growl": "wild",
                "frication": {"freq": "hz", "level": "wild"}}),
        ],
        "t",
        0.3,
    );
    assert_finite(&x, "wild signals");
}

#[test]
fn errors_name_the_field() {
    let g = json!({"type": "glottis", "name": "g", "f0": 150});
    let t = |extra: Value| {
        let mut o = json!({"type": "tract", "name": "t", "input": "g", "natural": false});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    };
    let gl = |extra: Value| {
        let mut o = json!({"type": "glottis", "name": "t", "f0": 150});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    };
    let cases: Vec<(String, &str)> = vec![
        (
            err(&[gl(json!({"roughness": 2}))]),
            "roughness: 2: give a number from 0 to 1",
        ),
        (
            err(&[gl(json!({"subharmonic": -1}))]),
            "subharmonic: -1: give a number from 0 to 1",
        ),
        (
            err(&[gl(json!({"fry": 1.5}))]),
            "fry: 1.5: give a number from 0 to 1",
        ),
        (
            err(&[g.clone(), t(json!({"nasal": 1.5}))]),
            "nasal: 1.5: give a number from 0 to 1",
        ),
        (
            err(&[g.clone(), t(json!({"growl": 3}))]),
            "growl: 3: give a number from 0 to 1",
        ),
        (
            err(&[
                g.clone(),
                t(json!({"frication": {"freq": 30000, "level": 1}})),
            ]),
            "frication.freq: 30000: give a number from 20 to 21600",
        ),
        (
            err(&[
                g.clone(),
                t(json!({"frication": {"freq": 4000, "level": -1}})),
            ]),
            "frication.level: -1: give a number >= 0",
        ),
    ];
    for (got, want) in &cases {
        eprintln!("error: {got}");
        assert!(got.contains(want), "`{got}` lacks `{want}`");
    }
    let unknown = err(&[g, t(json!({"frication": {"freq": 4000, "lvl": 1}}))]);
    eprintln!("error: {unknown}");
    assert!(unknown.contains("lvl"), "{unknown}");
}
