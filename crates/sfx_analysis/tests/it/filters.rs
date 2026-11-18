//! `filter` slope, `ladder`, `eq`.

use crate::common::*;
use serde_json::{Value, json};
use sfx_graph::{RenderOpts, SoundDoc};

fn sine(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(secs * SR as f32) as usize)
        .map(|i| {
            amp * (std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR)).sin() as f32
        })
        .collect()
}

fn render_err(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.duration = Some(0.2);
    match d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    }) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    }
}

/// A constant-amplitude tone through `node` (a JSON body with `input`
/// "tone"), as the dB gain measured on the second half.
fn tone_gain(node: Value, hz: f32) -> f32 {
    let secs = 1.0;
    // The osc node reads `freq`; amplitude 0.1 keeps the ladder linear.
    let nodes = [
        json!({"type": "osc", "name": "tone", "shape": "sine", "freq": hz}),
        json!({"type": "gain", "name": "src", "input": "tone", "gain": 0.1}),
        node,
    ];
    let name = nodes[2]["name"].as_str().unwrap().to_string();
    let y = node_audio(&nodes, &name, secs);
    let half = y.len() / 2;
    let x = sine(hz, 0.1, secs);
    rms_db(&y[half..]) - rms_db(&x[half..])
}

fn noise_nodes(amp: f32) -> Vec<Value> {
    vec![json!({"type": "noise", "name": "src", "seed": 3, "amp": amp})]
}

// ------------------------------------------------------------------ slope

#[test]
fn slope_sets_the_level_an_octave_above_the_cutoff() {
    let secs = 8.0;
    let src = node_audio(noise_nodes(0.3), "src", secs);
    let ref_db = band_db(&src, SR, 1950.0, 2050.0);
    for (slope, want) in [(12, -12.3f32), (24, -24.1), (48, -48.2)] {
        let mut nodes = noise_nodes(0.3);
        nodes.push(
            json!({"type": "filter", "name": "f", "input": "src", "mode": "lp", "freq": 1000, "slope": slope}),
        );
        let y = node_audio(&nodes, "f", secs);
        let octave = band_db(&y, SR, 1950.0, 2050.0) - ref_db;
        let pass = band_db(&y, SR, 200.0, 400.0) - band_db(&src, SR, 200.0, 400.0);
        println!(
            "lp 1 kHz slope {slope}: 2 kHz {octave:.2} dB (Butterworth {want}), 200-400 Hz {pass:.2} dB"
        );
        assert!(
            (octave - (-(slope as f32))).abs() < 2.0,
            "slope {slope}: {octave}"
        );
        assert!((octave - want).abs() < 1.0, "slope {slope}: {octave}");
        assert!(pass.abs() < 0.3, "passband not flat: {pass}");
    }
}

#[test]
fn slope_on_a_highpass_and_the_q_of_the_last_section() {
    let secs = 8.0;
    let src = node_audio(noise_nodes(0.3), "src", secs);
    let ref_db = band_db(&src, SR, 475.0, 525.0);
    for (slope, want) in [(24, -24.1f32), (48, -48.2)] {
        let mut nodes = noise_nodes(0.3);
        nodes.push(
            json!({"type": "filter", "name": "f", "input": "src", "mode": "hp", "freq": 1000, "slope": slope}),
        );
        let y = node_audio(&nodes, "f", secs);
        let octave = band_db(&y, SR, 475.0, 525.0) - ref_db;
        println!("hp 1 kHz slope {slope}: 500 Hz {octave:.2} dB (Butterworth {want})");
        assert!((octave - (-(slope as f32))).abs() < 2.0);
    }
    // q = 4 on a 24 dB lp raises the level at the cutoff.
    let at = |q: f32| {
        tone_gain(
            json!({"type": "filter", "name": "f", "input": "src", "mode": "lp", "freq": 1000, "slope": 24, "q": q}),
            1000.0,
        )
    };
    let (flat, peaky) = (at(std::f32::consts::FRAC_1_SQRT_2), at(4.0));
    println!("24 dB lp at fc: q 0.707 {flat:.2} dB, q 4 {peaky:.2} dB");
    assert!((flat - (-3.0)).abs() < 0.3);
    assert!(peaky > 6.0);
}

#[test]
fn slope_12_is_the_old_filter_bit_for_bit() {
    let mk = |extra: Value| {
        let mut f = json!({"type": "filter", "name": "f", "input": "src", "mode": "lp", "freq": 900, "q": 1.5});
        if let Value::Object(m) = extra {
            f.as_object_mut().unwrap().extend(m);
        }
        let mut nodes = noise_nodes(0.3);
        nodes.push(f);
        node_audio(nodes, "f", 0.3)
    };
    let a = mk(json!({}));
    let b = mk(json!({"slope": 12}));
    assert!(a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()));
}

#[test]
fn slope_errors_name_the_field() {
    let mut nodes = noise_nodes(0.3);
    nodes.push(json!({"type": "filter", "name": "f", "input": "src", "freq": 1000, "slope": 18}));
    let e = render_err(nodes);
    println!("{e}");
    assert!(e.contains("slope: 18: give 12, 24 or 48"), "{e}");

    let mut nodes = noise_nodes(0.3);
    nodes.push(
        json!({"type": "filter", "name": "f", "input": "src", "mode": "bp", "freq": 1000, "slope": 24}),
    );
    let e = render_err(nodes);
    println!("{e}");
    assert!(e.contains("slope: 24") && e.contains("lp and hp"), "{e}");
}

#[test]
fn slope_in_a_loop_errors_and_slope_12_is_allowed() {
    let lp = |slope: Value| {
        vec![
            json!({"type": "noise", "name": "n"}),
            json!({"type": "decay", "name": "e", "time": 0.004}),
            json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "mix", "name": "m", "inputs": ["burst", "g"]}),
            json!({"type": "delay", "name": "d", "input": "m", "time": 0.00227}),
            json!({"type": "filter", "name": "f", "input": "d", "mode": "lp", "freq": 9000, "slope": slope}),
            json!({"type": "gain", "name": "g", "input": "f", "gain": 0.9}),
        ]
    };
    let e = render_err(lp(json!(24)));
    println!("{e}");
    assert!(e.contains("slope: 24") && e.contains("loop"), "{e}");
    let ok = node_audio(lp(json!(12)), "f", 0.1);
    assert_finite(&ok, "loop with slope 12");
}

#[test]
fn filter_slope_is_deterministic_and_finite_at_the_extremes() {
    let nodes = [
        json!({"type": "noise", "name": "src", "seed": 2}),
        json!({"type": "filter", "name": "f", "input": "src", "mode": "lp", "freq": 700, "slope": 48, "q": 3}),
    ];
    assert_bit_identical(nodes, 0.3);
    for (mode, freq, q) in [
        ("lp", 1.0, 1000.0),
        ("lp", 23_000.0, 1000.0),
        ("hp", 1.0, 0.025),
        ("hp", 23_000.0, 1000.0),
    ] {
        let nodes = [
            json!({"type": "noise", "name": "src", "seed": 2}),
            json!({"type": "filter", "name": "f", "input": "src", "mode": mode, "freq": freq, "slope": 48, "q": q}),
        ];
        let y = node_audio(nodes, "f", 0.5);
        assert_finite(&y, &format!("{mode} {freq} Hz q {q}"));
        println!(
            "slope 48 {mode} {freq} Hz q {q}: peak {:.2}",
            y.iter().fold(0.0f32, |a, v| a.max(v.abs()))
        );
    }
}

// ----------------------------------------------------------------- ladder

fn ladder_ring(freq: f32, res: f32) -> Vec<f32> {
    node_audio(
        [
            json!({"type": "impulse", "name": "src", "at": [0.0]}),
            json!({"type": "ladder", "name": "l", "input": "src", "freq": freq, "resonance": res}),
        ],
        "l",
        0.5,
    )
}

#[test]
fn ladder_self_oscillates_at_freq_after_an_impulse() {
    for freq in [500.0f32, 1000.0, 2000.0, 4000.0] {
        let y = ladder_ring(freq, 1.1);
        let tail = &y[(0.1 * SR as f32) as usize..];
        let hz = peak_hz(tail, SR);
        let last = rms_db(&y[y.len() - SR as usize / 10..]);
        let err = (hz - freq) / freq * 100.0;
        println!(
            "ladder res 1.1 at {freq} Hz: rings at {hz:.1} Hz ({err:+.2} %), last 0.1 s {last:.1} dB rms"
        );
        assert!(err.abs() < 2.0, "{freq}: {hz}");
        assert!(last > -30.0, "{freq}: stopped ringing ({last} dB)");
    }
}

#[test]
fn ladder_below_resonance_one_rings_down() {
    let y = ladder_ring(1000.0, 0.8);
    let early = rms_db(&y[..SR as usize / 10]);
    let late = rms_db(&y[y.len() - SR as usize / 10..]);
    println!("ladder res 0.8: first 0.1 s {early:.1} dB, last 0.1 s {late:.1} dB");
    assert!(late < early - 30.0);
}

#[test]
fn ladder_rolls_off_and_resonance_costs_passband_gain() {
    let g = |res: f32, hz: f32| {
        tone_gain(
            json!({"type": "ladder", "name": "l", "input": "src", "freq": 1000, "resonance": res}),
            hz,
        )
    };
    let (p0, o0, o2) = (g(0.0, 100.0), g(0.0, 2000.0), g(0.0, 4000.0));
    let (p5, peak) = (g(0.5, 100.0), g(0.9, 1000.0));
    println!(
        "ladder lp 1 kHz, res 0: 100 Hz {p0:.2} dB, 2 kHz {o0:.2} dB, 4 kHz {o2:.2} dB; res 0.5: 100 Hz {p5:.2} dB (1/(1+2) = -9.54); res 0.9 at fc {peak:.2} dB"
    );
    assert!(p0.abs() < 0.3);
    assert!(o0 < -20.0 && o2 - o0 < -18.0, "24 dB an octave or steeper");
    assert!((p5 - (-9.54)).abs() < 0.5);
}

#[test]
fn ladder_modes_pass_the_right_band() {
    let g = |mode: &str, hz: f32| {
        tone_gain(
            json!({"type": "ladder", "name": "l", "input": "src", "freq": 1000, "mode": mode}),
            hz,
        )
    };
    let (hp_lo, hp_hi) = (g("hp", 100.0), g("hp", 8000.0));
    let (bp_lo, bp_mid, bp_hi) = (g("bp", 100.0), g("bp", 1000.0), g("bp", 8000.0));
    println!(
        "ladder hp: 100 Hz {hp_lo:.1} dB, 8 kHz {hp_hi:.2} dB; bp: 100 Hz {bp_lo:.1}, 1 kHz {bp_mid:.2}, 8 kHz {bp_hi:.1} dB"
    );
    assert!(hp_lo < -40.0 && hp_hi.abs() < 0.5);
    assert!(bp_lo < -20.0 && bp_hi < -20.0 && bp_mid > -1.0);
}

#[test]
fn ladder_is_deterministic_and_finite_at_the_extremes() {
    assert_bit_identical(
        [
            json!({"type": "noise", "name": "src", "seed": 4}),
            json!({"type": "ladder", "name": "l", "input": "src", "freq": 800, "resonance": 0.9, "drive": 3}),
        ],
        0.3,
    );
    for mode in ["lp", "bp", "hp"] {
        for freq in [20.0f32, 20_000.0, 30_000.0] {
            for res in [0.0f32, 1.0, 1.2, 5.0] {
                let nodes = [
                    json!({"type": "noise", "name": "src", "seed": 4}),
                    json!({"type": "ladder", "name": "l", "input": "src", "mode": mode, "freq": freq, "resonance": res, "drive": 20}),
                ];
                let y = node_audio(nodes, "l", 0.5);
                assert_finite(&y, &format!("{mode} {freq} Hz res {res}"));
                let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                if res == 1.2 {
                    println!("ladder {mode} {freq} Hz res 1.2 drive 20: peak {peak:.2}");
                }
                assert!(peak < 50.0, "{mode} {freq} {res}: peak {peak}");
            }
        }
    }
    // Audio-rate modulation of the cutoff and a stereo input.
    let nodes = [
        json!({"type": "noise", "name": "src", "seed": 4}),
        json!({"type": "lfo", "name": "m", "shape": "sine", "freq": 200}),
        json!({"type": "math", "name": "sweep", "op": "mul", "a": "m", "b": 3000}),
        json!({"type": "math", "name": "cut", "op": "add", "a": "sweep", "b": 3500}),
        json!({"type": "ladder", "name": "l", "input": "src", "freq": "cut", "resonance": 1.1}),
    ];
    assert_finite(&node_audio(nodes, "l", 0.3), "modulated ladder");
}

#[test]
fn ladder_errors_name_the_field() {
    let mut nodes = noise_nodes(0.3);
    nodes.push(json!({"type": "ladder", "name": "l", "input": "src", "freq": 1000, "drive": 500}));
    let e = render_err(nodes);
    println!("{e}");
    assert!(
        e.contains("drive: 500: give a number from 0.01 to 100"),
        "{e}"
    );
}

// --------------------------------------------------------------------- eq

fn eq_gain(bands: Value, hz: f32) -> f32 {
    tone_gain(
        json!({"type": "eq", "name": "q", "input": "src", "bands": bands}),
        hz,
    )
}

#[test]
fn eq_peak_measures_its_gain_and_leaves_far_frequencies_alone() {
    let bands = json!([{"mode": "peak", "freq": 1000, "gain_db": 6, "q": 4}]);
    let (at, lo, hi) = (
        eq_gain(bands.clone(), 1000.0),
        eq_gain(bands.clone(), 250.0),
        eq_gain(bands, 4000.0),
    );
    println!("eq peak +6 dB at 1 kHz, q 4: 1 kHz {at:.3} dB, 250 Hz {lo:.3} dB, 4 kHz {hi:.3} dB");
    assert!((at - 6.0).abs() < 0.5);
    assert!(lo.abs() < 0.5 && hi.abs() < 0.5);
    // At the default q (0.707) the bell is wide: a measured, not a promised, edge.
    let wide = json!([{"mode": "peak", "freq": 1000, "gain_db": 6}]);
    println!(
        "eq peak +6 dB, default q: 250 Hz {:.2} dB, 4 kHz {:.2} dB",
        eq_gain(wide.clone(), 250.0),
        eq_gain(wide, 4000.0)
    );
}

#[test]
fn eq_bands_chain_and_shelves_and_cuts_work() {
    let bands = json!([
        {"mode": "lowshelf", "freq": 200, "gain_db": 5},
        {"mode": "highshelf", "freq": 5000, "gain_db": -8},
        {"mode": "notch", "freq": 1000, "q": 5}
    ]);
    let g = |hz: f32| eq_gain(bands.clone(), hz);
    let (lo, notch, hi) = (g(40.0), g(1000.0), g(16_000.0));
    println!(
        "eq lowshelf +5 / highshelf -8 / notch 1 kHz: 40 Hz {lo:.2}, 1 kHz {notch:.1}, 16 kHz {hi:.2} dB"
    );
    assert!((lo - 5.0).abs() < 0.5);
    assert!((hi - (-8.0)).abs() < 0.7);
    assert!(notch < -30.0);
}

#[test]
fn eq_with_no_bands_is_the_input_bit_for_bit() {
    let mut nodes = noise_nodes(0.5);
    nodes.push(json!({"type": "eq", "name": "q", "input": "src", "bands": []}));
    let a = node_audio(&nodes, "src", 0.3);
    let b = node_audio(&nodes, "q", 0.3);
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(&b).all(|(p, q)| p.to_bits() == q.to_bits()));
    // A stereo input stays stereo.
    let out = render(
        [
            json!({"type": "noise", "name": "n", "seed": 1}),
            json!({"type": "pan", "name": "st", "input": "n", "pan": 0.5}),
            json!({"type": "eq", "name": "q", "input": "st", "bands": []}),
        ],
        0.2,
    );
    assert_eq!(out.channels.len(), 2);
}

#[test]
fn eq_is_deterministic_and_finite_at_the_extremes() {
    assert_bit_identical(
        [
            json!({"type": "noise", "name": "src", "seed": 6}),
            json!({"type": "eq", "name": "q", "input": "src", "bands": [
                {"mode": "peak", "freq": 3000, "gain_db": 9, "q": 3},
                {"mode": "hp", "freq": 80}]}),
        ],
        0.3,
    );
    for mode in ["lp", "hp", "bp", "notch", "peak", "lowshelf", "highshelf"] {
        for (freq, gain, q) in [
            (1.0, 60.0, 1000.0),
            (23_000.0, -60.0, 1000.0),
            (1.0, -60.0, 0.025),
            (23_000.0, 60.0, 0.025),
        ] {
            let nodes = [
                json!({"type": "noise", "name": "src", "seed": 6}),
                json!({"type": "eq", "name": "q", "input": "src", "bands":
                    [{"mode": mode, "freq": freq, "gain_db": gain, "q": q}]}),
            ];
            assert_finite(
                &node_audio(nodes, "q", 0.3),
                &format!("{mode} {freq} {gain} {q}"),
            );
        }
    }
}

#[test]
fn eq_errors_name_the_band_and_the_cap() {
    let band = |i: usize| json!({"mode": "peak", "freq": 100 + 50 * i, "gain_db": 1});
    let mut nodes = noise_nodes(0.3);
    nodes.push(
        json!({"type": "eq", "name": "q", "input": "src", "bands": (0..17).map(band).collect::<Vec<_>>()}),
    );
    let e = render_err(nodes);
    println!("{e}");
    assert!(e.contains("bands: 17 bands: give at most 16"), "{e}");

    let mut nodes = noise_nodes(0.3);
    nodes.push(json!({"type": "eq", "name": "q", "input": "src", "bands": [
        {"mode": "peak", "freq": 1000, "gain_db": 1},
        {"mode": "peak", "freq": 30000, "gain_db": 1}]}));
    let e = render_err(nodes);
    println!("{e}");
    assert!(e.contains("bands[1].freq: 30000: give 1 to"), "{e}");

    let mut nodes = noise_nodes(0.3);
    nodes.push(json!({"type": "eq", "name": "q", "input": "src", "bands": [
        {"mode": "peak", "freq": 1000, "gain_db": 90}]}));
    let e = render_err(nodes);
    assert!(e.contains("bands[0].gain_db: 90: give -60 to 60 dB"), "{e}");
}

#[test]
fn ladder_and_eq_describe_and_parse() {
    for ty in ["ladder", "eq"] {
        let ex = sfx_graph::example(ty).unwrap_or_else(|| panic!("no example for {ty}"));
        assert_eq!(ex["type"], ty);
        let text = sfx_graph::describe(Some(ty)).unwrap();
        assert!(text.contains("input"));
    }
}
