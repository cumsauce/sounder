//! The senses read known signals correctly.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::checks::{Expect, clicks};
use sfx_analysis::{CheckCx, Column, View, describe, listen_png, run};
use sfx_graph::{Audio, Kind, RenderOpts, SoundDoc, db_to_gain, loudness};

const SR: u32 = 48_000;

fn sine(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(secs * SR as f32) as usize)
        .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
        .collect()
}

fn doc(nodes: Vec<Value>, duration: Option<f32>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = duration;
    d
}

fn check(d: &SoundDoc, e: Expect) -> Vec<sfx_analysis::CheckResult> {
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
    run(
        &e,
        &CheckCx {
            doc: d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

fn expect(v: Value) -> Expect {
    serde_json::from_value(v).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_1khz_sine_reads_1000hz_and_a_1khz_centroid() {
    let d = describe(&Audio::mono(SR, sine(1000.0, 0.5, 1.0)));
    let p = d.pitch_hz.unwrap_or_else(|| panic!("no pitch"));
    let cents = 1200.0 * (p / 1000.0).log2();
    assert!(cents.abs() <= 5.0, "pitch {p} Hz ({cents} cents)");
    assert!(d.voiced > 0.9, "voiced {}", d.voiced);
    assert!(d.harmonicity > 0.9, "harmonicity {}", d.harmonicity);
    let c = d.centroid_hz.middle;
    assert!((c - 1000.0).abs() < 50.0, "centroid {c}");
    // The 1k band holds it at about -9 dB (amplitude 0.5 = -6 dBFS peak, -9 dB power).
    assert!(
        (d.bands_total[4] + 9.0).abs() < 1.0,
        "1k band {}",
        d.bands_total[4]
    );
    // The abrupt start and end leak a little broadband energy.
    assert!(d.bands_total[2] < -40.0, "250 band {}", d.bands_total[2]);
    assert!(d.flatness.middle < 0.05, "flatness {}", d.flatness.middle);
    // The strongest mode is the sine itself.
    assert!(
        d.modes.iter().any(|m| (m.hz - 1000.0).abs() < 5.0),
        "{:?}",
        d.modes
    );
}

#[test]
fn a_minus_18_dbfs_sine_reads_the_right_lufs() {
    // BS.1770: a 997 Hz sine at 0 dBFS peak reads -3.01 LUFS.
    let x = sine(997.0, db_to_gain(-18.0), 3.0);
    let l = loudness::integrated(std::slice::from_ref(&x), SR);
    assert!((l - (-21.01)).abs() <= 0.2, "{l} LUFS");
    // At another sample rate the K-weighting is re-derived.
    let x44: Vec<f32> = (0..(3 * 44_100))
        .map(|i| db_to_gain(-18.0) * (std::f32::consts::TAU * 997.0 * i as f32 / 44_100.0).sin())
        .collect();
    let l44 = loudness::integrated(&[x44], 44_100);
    assert!((l44 - (-21.01)).abs() <= 0.2, "{l44} LUFS at 44.1 kHz");
    // Stereo: both channels count (+3 dB).
    let ls = loudness::integrated(&[x.clone(), x], SR);
    assert!((ls - (-18.0)).abs() <= 0.2, "{ls} LUFS stereo");
}

#[test]
fn white_noise_is_flat() {
    let d = doc(
        vec![
            json!({"type": "noise", "name": "n", "amp": 0.3}),
            json!({"type": "output", "name": "out", "input": "n"}),
        ],
        Some(2.0),
    );
    let r = d.render(&RenderOpts::default()).unwrap();
    let desc = describe(&r.audio);
    for f in [desc.flatness.start, desc.flatness.middle, desc.flatness.end] {
        assert!(f > 0.9, "flatness {f}");
    }
    assert!(
        desc.pitch_hz.is_none() || desc.voiced < 0.2,
        "noise has no pitch"
    );
    assert!(desc.harmonicity < 0.5);
}

#[test]
fn true_peak_sees_between_samples() {
    // A sine at a quarter of the rate, 45 degrees off: samples at +-0.707,
    // the waveform peaks at 1.
    let x: Vec<f32> = (0..4800)
        .map(|i| (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin())
        .collect();
    let sample_peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let tp = loudness::true_peak(&[x]);
    assert!((sample_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    assert!(tp > 0.95, "true peak {tp}");
}

#[test]
fn silent_edges_fails_on_a_buffer_cut_mid_wave() {
    let cut = doc(
        vec![
            json!({"type": "osc", "name": "o", "freq": 440, "phase": 0.25, "amp": 0.5}),
            json!({"type": "output", "name": "out", "input": "o", "fade_in": 0, "fade_out": 0}),
        ],
        Some(0.1013),
    );
    let r = check(&cut, expect(json!({"silent_edges": true})));
    assert!(!r[0].pass, "{:?}", r[0]);
    assert!(
        r[0].detail.contains("fade_in") && r[0].detail.contains("fade_out"),
        "{}",
        r[0].detail
    );
    // The output's default fades fix it.
    let faded = doc(
        vec![
            json!({"type": "osc", "name": "o", "freq": 440, "phase": 0.25, "amp": 0.5}),
            json!({"type": "output", "name": "out", "input": "o"}),
        ],
        Some(0.1013),
    );
    let r = check(&faded, expect(json!({"silent_edges": true})));
    assert!(r[0].pass, "{:?}", r[0]);
}

#[test]
fn the_click_detector_finds_a_step() {
    let mut x = sine(200.0, 0.3, 0.5);
    assert!(clicks(&x, SR).is_empty());
    x[12_000] += 0.5;
    let t = clicks(&x, SR);
    assert!(!t.is_empty() && (t[0] - 0.25).abs() < 0.001, "{t:?}");
}

#[test]
fn loop_seam_passes_a_periodic_loop_and_fails_a_broken_one() {
    let mk = |hz: f32| {
        let mut d = doc(
            vec![
                json!({"type": "osc", "name": "o", "freq": hz, "amp": 0.5}),
                json!({"type": "output", "name": "out", "input": "o"}),
            ],
            Some(1.0),
        );
        d.kind = Kind::Loop;
        d
    };
    let r = check(&mk(220.0), expect(json!({"loop_seam": true})));
    assert!(r[0].pass, "{:?}", r[0]);
    // Half a cycle off: no jump but a kink. A quarter off: a jump.
    for hz in [220.5, 220.25] {
        let r = check(&mk(hz), expect(json!({"loop_seam": true})));
        assert!(!r[0].pass, "{hz}: {:?}", r[0]);
    }
}

#[test]
fn pitch_bands_onsets_and_duration_checks() {
    let d = doc(
        vec![
            json!({"type": "impulse", "name": "i", "at": [0.0, 0.3, 0.6]}),
            json!({"type": "resonators", "name": "r", "input": "i", "modes": [[440, 0.2, 0.5]]}),
            json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
        ],
        None,
    );
    let r = check(
        &d,
        expect(json!({
            "pitch": {"hz": 440, "tol_cents": 10},
            "bands": [{"band": "500", "min_db": -40}, {"band": "4k", "max_db": -60}, {"band": 440, "segment": 0, "min_db": -40}],
            "onsets": {"min": 3, "max": 3},
            "duration": {"min": 0.7, "max": 1.0},
            "lufs": {"min": -21, "max": -19},
        })),
    );
    for c in &r {
        assert!(c.pass, "{c:?}");
    }
    let wrong = check(&d, expect(json!({"pitch": {"hz": 500}})));
    assert!(
        !wrong[0].pass && wrong[0].detail.contains("scale the frequencies"),
        "{:?}",
        wrong[0]
    );
}

#[test]
fn defaults_and_variations_and_rebuild() {
    let d = doc(
        vec![
            json!({"type": "noise", "name": "n", "color": "pink"}),
            json!({"type": "wander", "name": "w", "rate": 12, "range": [400, 2500]}),
            json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": "w", "q": 3}),
            json!({"type": "decay", "name": "e", "time": 0.25}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "f", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "m", "lufs": -20}),
        ],
        None,
    );
    for c in check(&d, Expect::defaults()) {
        assert!(c.pass, "{c:?}");
    }
    let v = check(
        &d,
        expect(json!({"variations": {"seeds": 4, "lufs_spread": 1.5}})),
    );
    assert!(v[0].pass, "{:?}", v[0]);
    // A sound with no randomness has identical seeds: fails min_distance.
    let fixed = doc(
        vec![
            json!({"type": "osc", "name": "o", "freq": 300}),
            json!({"type": "decay", "name": "e", "time": 0.2}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "m"}),
        ],
        None,
    );
    let v = check(&fixed, expect(json!({"variations": true})));
    assert!(
        !v[0].pass && v[0].detail.contains("sound alike"),
        "{:?}",
        v[0]
    );
}

#[test]
fn unknown_checks_are_rejected() {
    let e = serde_json::from_value::<Expect>(json!({"peek_db": {"max": 0}})).unwrap_err();
    assert!(e.to_string().contains("peek_db"), "{e}");
}

#[test]
fn the_listen_image_fits_the_budget() {
    let a = Audio::mono(SR, sine(440.0, 0.5, 0.5));
    let b = Audio {
        sample_rate: SR,
        channels: vec![sine(220.0, 0.3, 1.5), sine(330.0, 0.3, 1.5)],
    };
    for cols in [1usize, 4] {
        let columns: Vec<Column> = (0..cols)
            .map(|k| Column {
                label: format!("seed {k}"),
                audio: if k % 2 == 0 { &a } else { &b },
            })
            .collect();
        let png = listen_png(&columns, View::default()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]) as usize;
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]) as usize;
        assert!(
            w * h <= sfx_analysis::image::MAX_PIXELS,
            "{cols} columns: {w}x{h}"
        );
    }
    // Zoom.
    let png = listen_png(
        &[Column {
            label: "zoom".into(),
            audio: &a,
        }],
        View {
            t: Some([0.1, 0.2]),
            hz: Some([200.0, 2000.0]),
        },
    )
    .unwrap();
    assert!(png.len() > 1000);
}

fn knob(
    name: &str,
    claim: (sfx_graph::Measure, i8),
    map: Vec<sfx_graph::MapEntry>,
) -> sfx_graph::Macro {
    sfx_graph::Macro {
        name: name.into(),
        value: 0.5,
        default: Some(0.5),
        doc: String::new(),
        claim: sfx_graph::Claim(claim.0, claim.1),
        game: false,
        map,
    }
}

fn entry(node: &str, field: &str, from: f64, to: f64, exp: bool) -> sfx_graph::MapEntry {
    sfx_graph::MapEntry {
        node: node.into(),
        field: field.into(),
        from,
        to,
        curve: if exp {
            sfx_graph::MapCurve::Exp
        } else {
            sfx_graph::MapCurve::Linear
        },
        col: None,
        index: None,
        key: None,
    }
}

#[test]
fn the_macros_check_keeps_names_honest() {
    use sfx_graph::Measure;
    let mut d = doc(
        vec![
            json!({"type": "noise", "name": "n", "color": "white", "amp": 0.3}),
            json!({"type": "decay", "name": "e", "time": 0.3}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 1000, "q": 1}),
            json!({"type": "filter", "name": "h", "input": "f", "mode": "hp", "freq": 100}),
            json!({"type": "output", "name": "out", "input": "h", "lufs": -20}),
        ],
        None,
    );
    d.macros = vec![
        // Opening the filter raises the centroid: an honest name.
        knob(
            "brightness",
            (Measure::Centroid, 1),
            vec![entry("f", "freq", 300.0, 8000.0, true)],
        ),
        // A high-pass opened up raises the centroid, but claimed "weight"
        // (centroid falls): the check fails.
        knob(
            "weight",
            (Measure::Centroid, -1),
            vec![entry("h", "freq", 100.0, 6000.0, true)],
        ),
        knob(
            "length",
            (Measure::Decay, 1),
            vec![entry("e", "time", 0.05, 0.5, true)],
        ),
        // Claimed to change decay but wired to the filter only: fails.
        knob(
            "ring",
            (Measure::Decay, 1),
            vec![entry("f", "q", 0.7, 0.8, false)],
        ),
    ];
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap();
    let res = run(
        &Expect::defaults_for(&d),
        &CheckCx {
            doc: &d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap();
    let by = |n: &str| {
        res.iter()
            .find(|c| c.name == format!("macro {n}"))
            .unwrap_or_else(|| panic!("{n}: {res:?}"))
    };
    assert!(by("brightness").pass, "{:?}", by("brightness"));
    assert!(by("length").pass, "{:?}", by("length"));
    let w = by("weight");
    assert!(
        !w.pass && w.detail.contains("does not fall") && w.detail.contains("rename the macro"),
        "{w:?}"
    );
    assert!(!by("ring").pass, "{:?}", by("ring"));
    // A sound with no macros gets no macro check by default.
    d.macros.clear();
    assert!(Expect::defaults_for(&d).macros.is_none());
}
