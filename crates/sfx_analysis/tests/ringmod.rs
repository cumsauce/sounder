//! `ringmod`: the reference node's tests. Each measures something and prints
//! it (these lines go in docs/nodes/ringmod.md); the template for every
//! node's test file: the card's measurement, bit-identical, finite at the
//! extremes, and one error-message test.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::node::parse_body;
use sfx_graph::{RenderOpts, SoundDoc};

/// A 1 kHz sine of amplitude 0.5 (-9.03 dB in the kit's convention).
fn src() -> Value {
    json!({"type": "osc", "name": "src", "freq": 1000, "amp": 0.5})
}

/// `src` through a `ringmod` named `rm` with extra fields.
fn rm(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "ringmod", "name": "rm", "input": "src"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![src(), r]
}

/// The error a document with these nodes fails to render with.
fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "rm"}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
}

#[test]
fn ringmod_sidebands() {
    let x = node_audio(rm(json!({"freq": 300})), "rm", 1.0);
    assert_finite(&x, "rm");
    let p = peaks(&x, SR, 2, 40.0);
    let mut hz: Vec<f32> = p.iter().map(|p| p.0).collect();
    hz.sort_by(f32::total_cmp);
    let at_1k = band_db(&x, SR, 990.0, 1010.0);
    let (low, high) = (
        band_db(&x, SR, 690.0, 710.0),
        band_db(&x, SR, 1290.0, 1310.0),
    );
    println!(
        "1 kHz in, carrier 300 Hz sine: peaks {:.2} and {:.2} Hz; bands 700 Hz {low:.2} dB, 1300 Hz {high:.2} dB, 1 kHz {at_1k:.1} dB; rms {:.2} dB",
        hz[0],
        hz[1],
        rms_db(&x)
    );
    assert!((hz[0] - 700.0).abs() < 1.0 && (hz[1] - 1300.0).abs() < 1.0);
    // 0.5 * 1 gives two sidebands of amplitude 0.25: -15.05 dB each.
    assert!(
        (low - (-15.05)).abs() < 0.3 && (high - (-15.05)).abs() < 0.3,
        "{low} {high}"
    );
    assert!(
        at_1k < -60.0,
        "the input frequency is suppressed: {at_1k:.1} dB"
    );
}

#[test]
fn ringmod_square_carrier_adds_odd_harmonic_sidebands() {
    let x = node_audio(rm(json!({"freq": 300, "shape": "square"})), "rm", 1.0);
    assert_finite(&x, "rm");
    // A square's harmonics 1, 3, 5 at 1, 1/3, 1/5 of 4/pi: sidebands at
    // 1000 -+ 300, 900 (100 and 1900), 1500 (500 and 2500).
    let p = peaks(&x, SR, 6, 30.0);
    let mut hz: Vec<f32> = p.iter().map(|p| p.0).collect();
    hz.sort_by(f32::total_cmp);
    let fundamental = band_db(&x, SR, 1290.0, 1310.0);
    let third = band_db(&x, SR, 1890.0, 1910.0);
    println!(
        "1 kHz in, carrier 300 Hz square: peaks {hz:.1?}; 1300 Hz {fundamental:.2} dB, 1900 Hz {third:.2} dB (third is {:.2} dB down)",
        fundamental - third
    );
    for want in [100.0, 500.0, 700.0, 1300.0, 1900.0, 2500.0] {
        assert!(
            hz.iter().any(|h| (h - want).abs() < 1.5),
            "no peak near {want} Hz in {hz:?}"
        );
    }
    assert!((fundamental - third - 9.54).abs() < 0.5, "1/3 is 9.54 dB");
    assert!(band_db(&x, SR, 990.0, 1010.0) < -50.0);
}

#[test]
fn ringmod_mix_crossfades_dry_and_wet() {
    let dry = node_audio(vec![src()], "src", 0.5);
    let zero = node_audio(rm(json!({"freq": 300, "mix": 0})), "rm", 0.5);
    assert!(
        dry.iter()
            .zip(&zero)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "mix 0 passes the input unchanged"
    );
    let half = node_audio(rm(json!({"freq": 300, "mix": 0.5})), "rm", 1.0);
    let (d, s) = (
        band_db(&half, SR, 990.0, 1010.0),
        band_db(&half, SR, 1290.0, 1310.0),
    );
    println!("mix 0.5: dry 1 kHz {d:.2} dB, sideband 1300 Hz {s:.2} dB");
    // Dry 0.5 * 0.5 = 0.25 (-15.05 dB); sideband 0.5 * 0.25 = 0.125 (-21.07).
    assert!((d - (-15.05)).abs() < 0.3 && (s - (-21.07)).abs() < 0.3);
}

#[test]
fn ringmod_takes_a_signal_for_freq_and_runs_both_channels() {
    // A falling carrier: the sidebands sweep, nothing stays at 1 kHz.
    let nodes = vec![
        src(),
        json!({"type": "ramp", "name": "sw", "from": 500, "to": 100, "time": 1.0}),
        json!({"type": "ringmod", "name": "rm", "input": "src", "freq": "sw"}),
    ];
    let x = node_audio(&nodes, "rm", 1.0);
    assert_finite(&x, "rm");
    let (up, down) = (
        band_db(&x, SR, 1090.0, 1510.0),
        band_db(&x, SR, 490.0, 910.0),
    );
    let at_1k = band_db(&x, SR, 995.0, 1005.0);
    println!(
        "swept carrier 500 -> 100 Hz: upper sidebands {up:.1} dB, lower {down:.1} dB, 1 kHz {at_1k:.1} dB"
    );
    assert!(up > -25.0 && down > -25.0 && at_1k < -35.0);
    // Stereo in, stereo out, the same carrier on both channels.
    let stereo = vec![
        src(),
        json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
        json!({"type": "ringmod", "name": "rm", "input": "st", "freq": 300}),
    ];
    let a = render(&stereo, 0.5);
    assert_eq!(a.channels.len(), 2);
    let (l, r) = (&a.channels[0], &a.channels[1]);
    let p = peaks(l, SR, 2, 40.0);
    println!(
        "stereo: L peaks {:.1} / {:.1} Hz, L rms {:.2} dB, R rms {:.2} dB",
        p[0].0,
        p[1].0,
        rms_db(l),
        rms_db(r)
    );
    assert!(rms_db(l) > -30.0 && (rms_db(l) - rms_db(r)).abs() < 0.1);
}

#[test]
fn ringmod_is_bit_identical() {
    for shape in ["sine", "tri", "square", "saw"] {
        assert_bit_identical(rm(json!({"freq": 437.5, "shape": shape, "mix": 0.7})), 0.3);
    }
}

#[test]
fn ringmod_is_finite_at_the_extremes() {
    for shape in ["sine", "tri", "square", "saw"] {
        for freq in [-24_000.0, -1.0, 0.0, 1e-6, 24_000.0] {
            for mix in [0.0, 1.0] {
                let nodes = vec![
                    json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6}),
                    json!({"type": "ringmod", "name": "rm", "input": "src", "freq": freq, "shape": shape, "mix": mix}),
                ];
                let x = node_audio(&nodes, "rm", 0.2);
                assert_finite(&x, &format!("{shape} freq {freq} mix {mix}"));
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak <= 1.3e6, "{shape} {freq}: peak {peak}");
            }
        }
    }
    // A signal on freq that leaves the legal range is clamped, not NaN.
    let nodes = vec![
        src(),
        json!({"type": "ramp", "name": "sw", "from": -1.0e9, "to": 1.0e9, "time": 0.2}),
        json!({"type": "ringmod", "name": "rm", "input": "src", "freq": "sw", "shape": "saw"}),
    ];
    let x = node_audio(&nodes, "rm", 0.2);
    assert_finite(&x, "freq signal far out of range");
    println!(
        "extremes: 4 shapes x freq -24000..24000 x mix 0/1 at input 1e6, and a +-1e9 Hz sweep: all finite"
    );
}

#[test]
fn ringmod_errors_say_the_field_value_and_fix() {
    let e = render_error(rm(json!({"freq": 30000})));
    println!("{e}");
    assert!(
        e.contains("freq: 30000: give a number between -24000 and 24000 Hz"),
        "{e}"
    );
    let e = render_error(rm(json!({"freq": 300, "mix": 1.5})));
    println!("{e}");
    assert!(e.contains("mix: 1.5: give a number from 0 to 1"), "{e}");
    let e = render_error(rm(json!({"freq": 300, "shape": "pulse"})));
    println!("{e}");
    assert!(e.contains("pulse") && e.contains("sine"), "{e}");
    let e = render_error(rm(json!({"freq": 300, "frq": 5})));
    assert!(e.contains("frq") && e.contains("freq"), "{e}");
    // A stereo carrier input is refused with the fix.
    let e = render_error(vec![
        src(),
        json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
        json!({"type": "ringmod", "name": "rm", "input": "src", "freq": "st"}),
    ]);
    println!("{e}");
    assert!(
        e.contains("freq") && e.contains("stereo") && e.contains("mix node"),
        "{e}"
    );
}

#[test]
fn ringmod_is_registered_causal_and_not_random() {
    let ex = sfx_graph::example("ringmod").expect("registered");
    let mut body = ex.clone();
    body.as_object_mut().unwrap().remove("type");
    body.as_object_mut().unwrap().remove("name");
    let node = parse_body("ringmod", &body, "ringmod1").unwrap();
    assert!(node.causal() && !node.random());
    let text = sfx_graph::describe(Some("ringmod")).unwrap();
    assert!(text.contains("ring modulator") && text.contains("Carrier frequency"));
    assert!(sfx_graph::describe(None).unwrap().contains("- ringmod:"));
}

/// The recipe in docs/nodes/ringmod.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "osc", "name": "tone", "freq": 660, "amp": 0.6}),
        json!({"type": "ramp", "name": "carrier", "from": 220, "to": 90, "time": 0.6, "curve": "exp"}),
        json!({"type": "ringmod", "name": "clang", "input": "tone", "freq": "carrier", "mix": 0.8}),
        json!({"type": "decay", "name": "fall", "time": 0.25}),
        json!({"type": "math", "name": "hit", "op": "mul", "a": "clang", "b": "fall"}),
    ]
}

#[test]
fn ringmod_recipe_renders() {
    let a = render(recipe(), 0.6);
    let x = node_audio(recipe(), "hit", 0.6);
    assert_finite(&x, "hit");
    let early = &x[..SR as usize / 10];
    let p = peaks(early, SR, 3, 30.0);
    println!(
        "recipe: {} samples, rms {:.2} dB, first 100 ms peaks {:.0?} Hz",
        a.len(),
        rms_db(&x),
        p.iter().map(|p| p.0).collect::<Vec<_>>()
    );
    assert_eq!(a.len(), (0.6 * SR as f32) as usize);
    assert!(rms_db(&x) > -40.0);
    assert!(p.len() >= 2);
}
