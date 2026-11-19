//! `transient`, `gate`, `sub`, `exciter`.

use crate::common::*;
use serde_json::{Value, json};

fn crest_db(x: &[f32]) -> f32 {
    peak_db(x) - rms_db(x)
}

fn win(x: &[f32], from: f32, to: f32) -> &[f32] {
    &x[(from * SR as f32) as usize..(to * SR as f32) as usize]
}

/// Level (dB) of the highest bin in `lo..hi`.
fn max_in(x: &[f32], lo: f32, hi: f32) -> f32 {
    spectrum(x, SR)
        .into_iter()
        .filter(|(f, _)| *f >= lo && *f < hi)
        .map(|p| p.1)
        .fold(f32::MIN, f32::max)
}

// ---------------------------------------------------------------- transient

/// White noise with a 20 ms linear rise to a plateau, then a slow fall.
fn burst(extra: Value) -> Vec<Value> {
    burst_with(
        json!([[0, 0], [0.02, 1], [0.04, 0.8], [0.6, 0.2], [0.8, 0.2]]),
        extra,
    )
}

fn burst_with(points: Value, extra: Value) -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "env", "name": "e", "points": points}),
        json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "e"}),
        {
            let mut t = json!({"type": "transient", "name": "t", "input": "src"});
            for (k, v) in extra.as_object().unwrap() {
                t[k] = v.clone();
            }
            t
        },
    ]
}

#[test]
fn transient_attack_raises_crest_and_keeps_the_body() {
    let nodes = burst(json!({"attack_db": 6}));
    let dry = node_audio(&nodes, "src", 0.8);
    let wet = node_audio(&nodes, "t", 0.8);
    assert_finite(&wet, "transient");
    let crest = |x: &[f32]| crest_db(win(x, 0.0, 0.6));
    let (cd, cw) = (crest(&dry), crest(&wet));
    let body = rms_db(win(&wet, 0.19, 0.21)) - rms_db(win(&dry, 0.19, 0.21));
    println!(
        "transient attack +6: crest {cd:.2} -> {cw:.2} dB (+{:.2}); level at 200 ms {body:+.3} dB",
        cw - cd
    );
    assert!(cw - cd >= 3.0, "crest rose {:.2} dB", cw - cd);
    assert!(body.abs() <= 1.0, "level 200 ms in moved {body:.2} dB");
}

#[test]
fn transient_cut_and_sustain_do_the_opposite() {
    let cut = burst(json!({"attack_db": -6}));
    let dry = node_audio(&cut, "src", 0.8);
    let wet = node_audio(&cut, "t", 0.8);
    let (cd, cw) = (crest_db(win(&dry, 0.0, 0.6)), crest_db(win(&wet, 0.0, 0.6)));
    println!("transient attack -6: crest {cd:.2} -> {cw:.2} dB");
    assert!(cw < cd - 1.0);
    // sustain +6 lifts the falling part (a 14 dB drop over 0.1 s) and not the
    // steady plateau or the steady tail.
    let drop = json!([[0, 0], [0.02, 1], [0.2, 1], [0.3, 0.2], [0.8, 0.2]]);
    let sus = burst_with(drop.clone(), json!({"sustain_db": 6}));
    let dry = node_audio(burst_with(drop, json!({})), "src", 0.8);
    let wet = node_audio(&sus, "t", 0.8);
    let tail = rms_db(win(&wet, 0.22, 0.3)) - rms_db(win(&dry, 0.22, 0.3));
    let plateau = rms_db(win(&wet, 0.1, 0.19)) - rms_db(win(&dry, 0.1, 0.19));
    let flat = rms_db(win(&wet, 0.6, 0.8)) - rms_db(win(&dry, 0.6, 0.8));
    println!(
        "transient sustain +6: falling part {tail:+.2} dB, plateau {plateau:+.2} dB, flat tail {flat:+.2} dB"
    );
    assert!(tail > 2.0 && plateau.abs() < 1.0 && flat.abs() < 1.0);
}

#[test]
fn transient_zero_is_neutral_and_deterministic() {
    let nodes = burst(json!({}));
    let dry = node_audio(&nodes, "src", 0.8);
    let wet = node_audio(&nodes, "t", 0.8);
    assert!(
        dry.iter()
            .zip(&wet)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert_bit_identical(burst(json!({"attack_db": 9, "sustain_db": -9})), 0.8);
}

#[test]
fn transient_finite_at_extremes_and_errors() {
    for (a, s, sp) in [
        (24.0, -24.0, 0.001),
        (-24.0, 24.0, 1.0),
        (24.0, 24.0, 0.001),
    ] {
        let nodes = burst(json!({"attack_db": a, "sustain_db": s, "speed": sp}));
        assert_finite(&node_audio(&nodes, "t", 0.5), "transient");
    }
    // silence in, silence out
    let x = node_audio(
        [
            json!({"type": "gain", "name": "z", "input": 0.0, "gain": 1.0}),
            json!({"type": "transient", "name": "t", "input": "z", "attack_db": 24}),
        ],
        "t",
        0.1,
    );
    assert!(x.iter().all(|v| *v == 0.0));
    let e = std::panic::catch_unwind(|| render(burst(json!({"attack_db": 40})), 0.1))
        .unwrap_err()
        .downcast::<String>()
        .unwrap();
    println!("error: {e}");
    assert!(e.contains("attack_db") && e.contains("-24") && e.contains("40"));
}

// --------------------------------------------------------------------- gate

/// A -50 dB noise bed with 100 ms bursts at -10 dB at 0.1, 0.6 and 1.1 s.
fn bed(gate: Value) -> Vec<Value> {
    let mut points = vec![json!([0, 0])];
    for t in [0.1f32, 0.6, 1.1] {
        points.push(json!([t, 0]));
        points.push(json!([t + 0.001, 1]));
        points.push(json!([t + 0.1, 1]));
        points.push(json!([t + 0.101, 0]));
    }
    points.push(json!([1.6, 0]));
    let mut g = json!({"type": "gate", "name": "g", "input": "src", "threshold_db": -30});
    for (k, v) in gate.as_object().unwrap() {
        g[k] = v.clone();
    }
    vec![
        json!({"type": "noise", "name": "bed", "seed": 1, "amp": 0.0055}),
        json!({"type": "noise", "name": "b", "seed": 2, "amp": 0.55}),
        json!({"type": "env", "name": "e", "points": points}),
        json!({"type": "math", "name": "bursts", "op": "mul", "a": "b", "b": "e"}),
        json!({"type": "math", "name": "src", "op": "add", "a": "bed", "b": "bursts"}),
        g,
    ]
}

#[test]
fn gate_silences_the_bed_and_passes_the_bursts() {
    let nodes = bed(json!({}));
    let dry = node_audio(&nodes, "src", 1.6);
    let wet = node_audio(&nodes, "g", 1.6);
    assert_finite(&wet, "gate");
    let bed_level = rms_db(win(&dry, 0.3, 0.5));
    let burst_level = rms_db(win(&dry, 0.12, 0.19));
    let mut worst_gap = f32::MIN;
    let mut worst_move = 0.0f32;
    for (burst_at, gap_from) in [(0.1, 0.45), (0.6, 0.95)] {
        let gap = rms_db(win(&wet, gap_from, gap_from + 0.1));
        worst_gap = worst_gap.max(gap);
        let (b0, b1) = (burst_at + 0.01, burst_at + 0.1);
        let move_db = peak_db(win(&wet, b0, b1)) - peak_db(win(&dry, b0, b1));
        if move_db.abs() > worst_move.abs() {
            worst_move = move_db;
        }
    }
    println!(
        "gate: bed {bed_level:.1} dBFS rms, bursts {burst_level:.1}; gap after gate {worst_gap:.1} dBFS; burst peak moved {worst_move:+.3} dB"
    );
    assert!(bed_level > -52.0 && bed_level < -48.0);
    assert!(worst_gap < -80.0, "gap {worst_gap:.1}");
    assert!(worst_move.abs() < 0.5, "peak moved {worst_move:.3} dB");
}

#[test]
fn gate_hold_keeps_it_open_and_ratio_expands() {
    // The burst ends at 0.2: with a long hold the bed after it still passes.
    let held = node_audio(bed(json!({"hold": 0.15, "release": 0.02})), "g", 1.6);
    let dry = node_audio(bed(json!({})), "src", 1.6);
    let during = rms_db(win(&held, 0.25, 0.3)) - rms_db(win(&dry, 0.25, 0.3));
    let after = rms_db(win(&held, 0.45, 0.55));
    println!("gate hold 0.15: gain at 0.25 s {during:+.2} dB, bed at 0.45 s {after:.1} dBFS");
    assert!(during.abs() < 1.0, "bed still passes during the hold");
    assert!(after < -80.0);
    // Ratio 2 on a steady -40 dB sine, threshold -20: reduction (-20 dB) x (2 - 1).
    let nodes = [
        json!({"type": "osc", "name": "s", "freq": 1000, "amp": 0.01}),
        json!({"type": "gate", "name": "g", "input": "s", "threshold_db": -20, "ratio": 2}),
    ];
    let wet = node_audio(&nodes, "g", 0.5);
    let dry = node_audio(&nodes, "s", 0.5);
    let red = rms_db(win(&wet, 0.2, 0.4)) - rms_db(win(&dry, 0.2, 0.4));
    println!("gate ratio 2: sine at -40 dBFS peak reduced by {red:.2} dB (expected -20)");
    assert!((red + 20.0).abs() < 1.5);
    // Ratio 1 changes nothing.
    let nodes = [
        json!({"type": "osc", "name": "s", "freq": 1000, "amp": 0.01}),
        json!({"type": "gate", "name": "g", "input": "s", "threshold_db": -20, "ratio": 1}),
    ];
    let wet = node_audio(&nodes, "g", 0.3);
    let dry = node_audio(&nodes, "s", 0.3);
    assert!((rms_db(&wet) - rms_db(&dry)).abs() < 0.01);
}

#[test]
fn gate_key_drives_the_detector() {
    // A quiet tone gated by a loud key: open while the key pulse lasts.
    let nodes = [
        json!({"type": "osc", "name": "tone", "freq": 440, "amp": 0.2}),
        json!({"type": "env", "name": "pulse", "points": [[0, 0], [0.2, 0], [0.201, 1], [0.4, 1], [0.401, 0], [0.8, 0]]}),
        json!({"type": "osc", "name": "ks", "freq": 100, "amp": 1.0}),
        json!({"type": "math", "name": "k", "op": "mul", "a": "ks", "b": "pulse"}),
        json!({"type": "gate", "name": "g", "input": "tone", "threshold_db": -20,
               "key": "k", "release": 0.005, "hold": 0.0}),
    ];
    let wet = node_audio(&nodes, "g", 0.8);
    let open = rms_db(win(&wet, 0.25, 0.38));
    let shut = rms_db(win(&wet, 0.6, 0.78));
    let closed_before = rms_db(win(&wet, 0.05, 0.15));
    println!("gate key: open {open:.1} dB, before {closed_before:.1} dB, after {shut:.1} dB");
    assert!(open > -20.0);
    assert!(shut < -80.0 && closed_before < -80.0);
}

#[test]
fn gate_deterministic_finite_and_errors() {
    assert_bit_identical(bed(json!({"ratio": 3})), 1.0);
    for g in [
        json!({"threshold_db": -200, "ratio": 1000, "attack": 0, "release": 0, "hold": 0}),
        json!({"threshold_db": 0, "release": 10, "attack": 10, "hold": 10}),
        json!({"threshold_db": -30, "ratio": 1}),
    ] {
        assert_finite(&node_audio(bed(g), "g", 0.7), "gate");
    }
    let e = std::panic::catch_unwind(|| render(bed(json!({"ratio": 0.5})), 0.1))
        .unwrap_err()
        .downcast::<String>()
        .unwrap();
    println!("error: {e}");
    assert!(e.contains("ratio: 0.5") && e.contains(">= 1"));
}

// ---------------------------------------------------------------------- sub

fn saw_sub(extra: Value) -> Vec<Value> {
    let mut s = json!({"type": "sub", "name": "s", "input": "saw"});
    for (k, v) in extra.as_object().unwrap() {
        s[k] = v.clone();
    }
    vec![
        json!({"type": "osc", "name": "saw", "shape": "saw", "freq": 220, "amp": 0.5}),
        s,
    ]
}

#[test]
fn sub_adds_an_octave_below() {
    for (label, extra) in [
        ("lowpass 120 (default)", json!({})),
        ("lowpass 250", json!({"lowpass": 250})),
    ] {
        let x = node_audio(saw_sub(extra), "s", 1.0);
        assert_finite(&x, "sub");
        let (sub, fund) = (max_in(&x, 100.0, 120.0), max_in(&x, 210.0, 230.0));
        let hz = peaks(win(&x, 0.2, 1.0), SR, 3, 40.0);
        println!(
            "sub on 220 Hz saw, {label}: 110 Hz {sub:.1} dB, 220 Hz {fund:.1} dB (gap {:.1}); peaks {:?}",
            fund - sub,
            hz
        );
        assert!(
            fund - sub <= 12.0,
            "{label}: 110 Hz is {:.1} dB down",
            fund - sub
        );
        // The sub is a sine, not a square: its 3rd harmonic (330 Hz) is the saw's 3rd
        // partial band; check the 110 Hz line is far above the 55 Hz subharmonic.
        assert!(sub > max_in(&x, 40.0, 70.0) + 20.0);
    }
}

#[test]
fn sub_two_octaves_and_mix_zero() {
    let x = node_audio(saw_sub(json!({"octaves": 2, "lowpass": 250})), "s", 1.0);
    let (low, mid) = (max_in(&x, 50.0, 60.0), max_in(&x, 100.0, 120.0));
    println!("sub octaves 2: 55 Hz {low:.1} dB, 110 Hz {mid:.1} dB");
    assert!(low > mid + 10.0, "55 Hz must dominate 110 Hz");
    let f = peak_hz(&x[..(SR as usize)], SR);
    assert!(
        (f - 220.0).abs() < 2.0 || (f - 55.0).abs() < 2.0,
        "peak {f}"
    );
    // mix 0 returns the input bit for bit.
    let y = node_audio(saw_sub(json!({"mix": 0})), "s", 0.3);
    let d = node_audio(saw_sub(json!({"mix": 0})), "saw", 0.3);
    assert!(y.iter().zip(&d).all(|(a, b)| a.to_bits() == b.to_bits()));
}

#[test]
fn sub_deterministic_finite_and_errors() {
    assert_bit_identical(saw_sub(json!({"lowpass": 250})), 0.5);
    for extra in [
        json!({"lowpass": 20, "octaves": 2, "mix": 4}),
        json!({"lowpass": 1000, "mix": 4}),
    ] {
        for src in [
            json!({"type": "noise", "name": "saw", "amp": 1.0}),
            json!({"type": "osc", "name": "saw", "shape": "square", "freq": 9000}),
            json!({"type": "gain", "name": "saw", "input": 1.0, "gain": 1.0}),
        ] {
            let mut s = json!({"type": "sub", "name": "s", "input": "saw"});
            for (k, v) in extra.as_object().unwrap() {
                s[k] = v.clone();
            }
            assert_finite(&node_audio([src, s], "s", 0.4), "sub");
        }
    }
    let e = std::panic::catch_unwind(|| render(saw_sub(json!({"octaves": 3})), 0.1))
        .unwrap_err()
        .downcast::<String>()
        .unwrap();
    println!("error: {e}");
    assert!(e.contains("octaves: 3") && e.contains("1 or 2"));
}

// ------------------------------------------------------------------ exciter

fn tone_exc(extra: Value) -> Vec<Value> {
    let mut s = json!({"type": "exciter", "name": "x", "input": "tone"});
    for (k, v) in extra.as_object().unwrap() {
        s[k] = v.clone();
    }
    vec![
        json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.5}),
        s,
    ]
}

#[test]
fn exciter_adds_highs_and_nothing_below() {
    let x = node_audio(
        tone_exc(json!({"freq": 3000, "drive": 2, "mix": 0.3})),
        "x",
        1.0,
    );
    let fund = max_in(&x, 900.0, 1100.0);
    // Everything under 2 kHz except the 1 kHz line itself and its window skirts.
    let spec = spectrum(&x, SR);
    let below = spec
        .iter()
        .filter(|(f, _)| *f < 2000.0 && (*f < 900.0 || *f > 1100.0))
        .map(|p| p.1)
        .fold(f32::MIN, f32::max);
    let h3 = max_in(&x, 2950.0, 3050.0);
    let h5 = max_in(&x, 4950.0, 5050.0);
    let dry = node_audio(tone_exc(json!({})), "tone", 1.0);
    let h3_dry = max_in(&dry, 2950.0, 3050.0);
    println!(
        "exciter: 1 kHz {fund:.1} dB; under 2 kHz (off the line) {below:.1} dB; 3 kHz {h3:.1} dB (dry {h3_dry:.1}); 5 kHz {h5:.1} dB"
    );
    assert!(below - fund < -70.0, "{:.1} dB under 2 kHz", below - fund);
    // A 1 kHz line is 1.6 octaves under the corner: the first high-pass takes
    // 19 dB off it, so what the exciter adds is small. It is there (the dry
    // tone has nothing at 3 kHz) and the in-band test below shows a real level.
    assert!(
        h3 > h3_dry + 15.0 && h3 - fund > -95.0,
        "3 kHz {:.1}",
        h3 - fund
    );
}

#[test]
fn exciter_in_band_tone_gets_odd_harmonics() {
    let nodes = [
        json!({"type": "osc", "name": "tone", "freq": 2500, "amp": 0.5}),
        json!({"type": "exciter", "name": "x", "input": "tone", "freq": 2000, "drive": 4, "mix": 0.5}),
    ];
    let x = node_audio(nodes, "x", 1.0);
    let fund = max_in(&x, 2400.0, 2600.0);
    let h3 = max_in(&x, 7400.0, 7600.0) - fund;
    let h2 = max_in(&x, 4900.0, 5100.0) - fund;
    println!(
        "exciter on 2.5 kHz, freq 2000 drive 4 mix 0.5: 3rd harmonic {h3:.1} dB, 2nd {h2:.1} dB re the tone"
    );
    assert!(h3 > -40.0, "3rd harmonic {h3:.1}");
    assert!(h2 < h3 - 30.0, "odd shaper: 2nd harmonic {h2:.1}");
}

#[test]
fn exciter_mix_zero_is_the_input_and_aliasing_is_reported() {
    let y = node_audio(tone_exc(json!({"mix": 0})), "x", 0.3);
    let d = node_audio(tone_exc(json!({"mix": 0})), "tone", 0.3);
    assert!(y.iter().zip(&d).all(|(a, b)| a.to_bits() == b.to_bits()));
    // Not asserted: the aliasing of a driven 5 kHz tone (no oversampling).
    let nodes = [
        json!({"type": "osc", "name": "tone", "freq": 5000, "amp": 0.5}),
        json!({"type": "exciter", "name": "x", "input": "tone", "freq": 3000, "drive": 8, "mix": 1}),
    ];
    let a = node_audio(nodes, "x", 1.0);
    println!(
        "exciter drive 8 mix 1 on 5 kHz: alias {:.1} dB",
        alias_db(&a, SR, 5000.0)
    );
}

#[test]
fn exciter_deterministic_finite_stereo_and_errors() {
    assert_bit_identical(tone_exc(json!({"drive": 8})), 0.5);
    for extra in [
        json!({"freq": 20, "drive": 100, "mix": 10}),
        json!({"freq": 21600, "drive": 100, "mix": 10}),
        json!({"freq": 3000, "drive": 0.0001}),
    ] {
        let mut nodes = tone_exc(extra);
        nodes[0] = json!({"type": "noise", "name": "tone", "amp": 1.0});
        assert_finite(&node_audio(nodes, "x", 0.3), "exciter");
    }
    // Stereo in, stereo out (each channel on its own).
    let a = render(
        [
            json!({"type": "osc", "name": "o", "freq": 800}),
            json!({"type": "pan", "name": "p", "input": "o", "pan": -1}),
            json!({"type": "exciter", "name": "x", "input": "p"}),
        ],
        0.2,
    );
    assert_eq!(a.channels.len(), 2);
    let e = std::panic::catch_unwind(|| render(tone_exc(json!({"freq": 30000})), 0.1))
        .unwrap_err()
        .downcast::<String>()
        .unwrap();
    println!("error: {e}");
    assert!(e.contains("freq: 30000") && e.contains("21600"));
}
