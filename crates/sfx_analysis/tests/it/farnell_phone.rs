//! `phone_tone` (PHONETONES, DTMF) and `two_tone` (ALARMS/2tone-*.pd):
//! what each patch does, measured on the node (docs/nodes/farnell_phone.md).

use crate::common::*;
use serde_json::{Value, json};

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn seg(x: &[f32], a: f32, b: f32) -> &[f32] {
    &x[(a * SR as f32) as usize..(b * SR as f32) as usize]
}

/// Energy at `hz` (dB, Goertzel-free: the spectrum bin nearest).
fn at_hz(x: &[f32], hz: f32) -> f32 {
    band_db(x, SR, hz - 8.0, hz + 8.0)
}

#[test]
fn dial_tone_is_350_plus_440_when_clean() {
    let x = node_audio(
        one("phone_tone", json!({"kind": "dial", "line": 0})),
        "n",
        1.0,
    );
    assert_finite(&x, "dial");
    let pk = peaks(&x, SR, 2, 20.0);
    println!("dial peaks {pk:?}");
    let mut f: Vec<f32> = pk.iter().map(|p| p.0).collect();
    f.sort_by(f32::total_cmp);
    assert!(
        (f[0] - 350.0).abs() < 8.0 && (f[1] - 440.0).abs() < 8.0,
        "{f:?}"
    );
    assert!((peak(&x) - 1.0).abs() < 0.05);
}

#[test]
fn the_line_band_limits_and_holds_the_level() {
    let clean = node_audio(
        one("phone_tone", json!({"kind": "dial", "line": 0})),
        "n",
        1.0,
    );
    let full = node_audio(
        one("phone_tone", json!({"kind": "dial", "line": 1})),
        "n",
        1.0,
    );
    assert_finite(&full, "dial line");
    let (pc, pf) = (peak(&clean), peak(&full));
    println!("dial peak clean {pc:.2}, line {pf:.2}");
    assert!((pf - pc).abs() < 0.1 * pc);
    // The clip makes harmonics and the 2 kHz / 400 Hz resonances take over:
    // the line version has energy near 2 kHz that the clean tone lacks.
    let hi = |x: &[f32]| band_db(seg(x, 0.2, 1.0), SR, 1500.0, 2600.0);
    println!(
        "dial 1.5-2.6 kHz: clean {:.1}, line {:.1} dB",
        hi(&clean),
        hi(&full)
    );
    assert!(hi(&full) > hi(&clean) + 20.0);
    // Half way sits between.
    let half = node_audio(
        one("phone_tone", json!({"kind": "dial", "line": 0.5})),
        "n",
        1.0,
    );
    assert!(hi(&half) > hi(&clean) + 5.0 && hi(&half) < hi(&full) + 3.0);
}

#[test]
fn busy_is_480_plus_620_gated_at_two_hertz() {
    let x = node_audio(one("phone_tone", json!({"kind": "busy"})), "n", 2.0);
    assert_finite(&x, "busy");
    let on = seg(&x, 0.05, 0.2);
    let off = seg(&x, 0.3, 0.45);
    println!("busy on {:.1} dB, off {:.1} dB", rms_db(on), rms_db(off));
    assert!(rms_db(on) > rms_db(off) + 50.0);
    let mut f: Vec<f32> = peaks(on, SR, 2, 20.0).iter().map(|p| p.0).collect();
    f.sort_by(f32::total_cmp);
    assert!(
        (f[0] - 480.0).abs() < 15.0 && (f[1] - 620.0).abs() < 15.0,
        "{f:?}"
    );
    // 0.25 s on, 0.25 s off: the second on-window starts at 0.5 s.
    assert!(rms_db(seg(&x, 0.55, 0.7)) > rms_db(off) + 50.0);
}

#[test]
fn ringback_rings_three_seconds_then_pauses() {
    let x = node_audio(
        one("phone_tone", json!({"kind": "ringback", "line": 0})),
        "n",
        7.0,
    );
    assert_finite(&x, "ringback");
    let on = seg(&x, 0.5, 2.5);
    let off = seg(&x, 3.5, 5.5);
    println!(
        "ringback on {:.1} dB, off {:.1} dB",
        rms_db(on),
        rms_db(off)
    );
    assert!(rms_db(on) > rms_db(off) + 60.0);
    assert!(rms_db(seg(&x, 6.2, 6.9)) > rms_db(off) + 60.0);
    let mut f: Vec<f32> = peaks(on, SR, 2, 20.0).iter().map(|p| p.0).collect();
    f.sort_by(f32::total_cmp);
    assert!(
        (f[0] - 440.0).abs() < 8.0 && (f[1] - 480.0).abs() < 8.0,
        "{f:?}"
    );
}

#[test]
fn dtmf_digits_carry_their_row_and_column() {
    let x = node_audio(
        one("phone_tone", json!({"kind": "dtmf", "digits": "1#"})),
        "n",
        0.7,
    );
    assert_finite(&x, "dtmf");
    // Digit 1 = 697 + 1209 at 0.01-0.21 s; # = 941 + 1477 from 0.31 s.
    let a = seg(&x, 0.05, 0.2);
    let b = seg(&x, 0.35, 0.5);
    println!("dtmf 1 peaks {:?}", peaks(a, SR, 2, 20.0));
    for (s, hz) in [(a, [697.0, 1209.0]), (b, [941.0, 1477.0])] {
        let mut f: Vec<f32> = peaks(s, SR, 2, 20.0).iter().map(|p| p.0).collect();
        f.sort_by(f32::total_cmp);
        assert!(
            (f[0] - hz[0]).abs() < 12.0 && (f[1] - hz[1]).abs() < 12.0,
            "{f:?} {hz:?}"
        );
    }
    // The gap between the digits is silent, and the other digit's tones are not in it.
    assert!(rms_db(seg(&x, 0.23, 0.29)) < rms_db(a) - 50.0);
    assert!(at_hz(a, 941.0) < at_hz(a, 697.0) - 30.0);
    // Four 300 ms slots: the last digit ends before 1.3 s for "1234".
    let y = node_audio(
        one(
            "phone_tone",
            json!({"kind": "dtmf", "digits": "1-2-3-4", "tempo": 2}),
        ),
        "n",
        1.0,
    );
    assert!(rms_db(seg(&y, 0.8, 1.0)) < rms_db(seg(&y, 0.0, 0.1)) - 50.0);
}

#[test]
fn pulse_dial_clicks_once_per_pulse() {
    let x = node_audio(
        one("phone_tone", json!({"kind": "pulse_dial", "digits": "3"})),
        "n",
        1.5,
    );
    assert_finite(&x, "pulse");
    let d = sfx_analysis::describe(&mono_audio(&x));
    let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
    println!("pulse onsets {t:?}");
    // Three pulses a 100 ms apart, each making at the start of its period.
    assert!(t.len() >= 3, "{t:?}");
    for (k, want) in [0.02f32, 0.12, 0.22].iter().enumerate() {
        assert!(
            t.iter().any(|v| (v - want).abs() < 0.02),
            "pulse {k}: {t:?}"
        );
    }
    assert!(rms_db(seg(&x, 0.8, 1.4)) < rms_db(seg(&x, 0.0, 0.5)) - 40.0);
    // 0 is ten pulses.
    let z = node_audio(
        one("phone_tone", json!({"kind": "pulse_dial", "digits": "0"})),
        "n",
        1.5,
    );
    let dz = sfx_analysis::describe(&mono_audio(&z));
    assert!(dz.onsets.len() >= 8, "{}", dz.onsets.len());
}

#[test]
fn two_tone_alternates_high_then_low() {
    let x = node_audio(one("two_tone", json!({})), "n", 2.0);
    assert_finite(&x, "two_tone");
    // rate 2: 800 Hz for 0.25 s, then 600 Hz for 0.25 s.
    let hi = seg(&x, 0.05, 0.2);
    let lo = seg(&x, 0.3, 0.45);
    println!("two_tone peaks {} / {}", peak_hz(hi, SR), peak_hz(lo, SR));
    assert!((peak_hz(hi, SR) - 800.0).abs() < 15.0);
    assert!((peak_hz(lo, SR) - 600.0).abs() < 15.0);
    assert!((peak(&x) - 0.2).abs() < 0.02);
    let fast = node_audio(
        one("two_tone", json!({"rate": 4, "low": 500, "high": 1000})),
        "n",
        1.0,
    );
    assert!((peak_hz(seg(&fast, 0.03, 0.11), SR) - 1000.0).abs() < 30.0);
    assert!((peak_hz(seg(&fast, 0.15, 0.23), SR) - 500.0).abs() < 30.0);
}

#[test]
fn two_tone_shapes_differ() {
    let r = |s: &str| node_audio(one("two_tone", json!({"shape": s})), "n", 1.0);
    let (sw, ga, sm) = (r("switch"), r("gated"), r("smooth"));
    for (x, w) in [(&sw, "switch"), (&ga, "gated"), (&sm, "smooth")] {
        assert_finite(x, w);
    }
    // Gated is silent between the tones (0.4 to 0.5 of the cycle = 0.2-0.25 s).
    let gap = rms_db(seg(&ga, 0.215, 0.245));
    let tone = rms_db(seg(&ga, 0.05, 0.18));
    println!("gated gap {gap:.1} dB, tone {tone:.1} dB");
    assert!(gap < tone - 30.0);
    // Smooth has a level dip at the cross (equal power keeps it near flat),
    // switch has none; both hold the level.
    assert!(rms_db(seg(&sm, 0.2, 0.3)) > tone - 4.0);
    assert!(fnv(&sw) != fnv(&sm) && fnv(&sw) != fnv(&ga));
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = |n: Vec<Value>| render_error(n, "n", 0.1);
    for (n, word) in [
        (
            one("phone_tone", json!({"kind": "dtmf", "digits": "12x"})),
            "not a key",
        ),
        (
            one("phone_tone", json!({"kind": "pulse_dial", "digits": "5*"})),
            "not a key",
        ),
        (
            one("phone_tone", json!({"kind": "dial", "line": 2})),
            "give 0 to 1",
        ),
        (one("two_tone", json!({"rate": 0})), "give cycles"),
    ] {
        let m = e(n);
        assert!(m.contains(word), "{m}");
    }
    assert_bit_identical(
        one(
            "phone_tone",
            json!({"kind": "dtmf", "digits": "911", "line": 0.4}),
        ),
        1.0,
    );
    assert_bit_identical(one("two_tone", json!({"shape": "smooth"})), 1.0);
}
