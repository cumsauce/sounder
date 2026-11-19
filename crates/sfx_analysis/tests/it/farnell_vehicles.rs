//! The vehicle nodes ported from Farnell's patches: `helicopter`
//! (HELICOPTER/heli1.pd) and `rocket` (ROCKETLAUNCHER/rocket.pd), what each
//! patch does, measured on the node (docs/nodes/farnell_vehicles.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn heli(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "helicopter", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn rocket(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "rocket", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

/// The lag in seconds, within `lo..hi`, at which the 3 ms-smoothed
/// magnitude of `x` best repeats itself.
fn envelope_period(x: &[f32], lo: f32, hi: f32) -> f32 {
    let hop = (SR as f32 * 0.001) as usize;
    let env: Vec<f32> = x
        .chunks(hop)
        .map(|c| c.iter().map(|v| v.abs()).sum::<f32>() / c.len() as f32)
        .collect();
    let mean = env.iter().sum::<f32>() / env.len() as f32;
    let env: Vec<f32> = env.iter().map(|v| v - mean).collect();
    let (mut best, mut at) = (f32::MIN, 0);
    for lag in (lo * 1000.0) as usize..=(hi * 1000.0) as usize {
        let r: f32 = env.iter().zip(&env[lag..]).map(|(a, b)| a * b).sum();
        if r > best {
            best = r;
            at = lag;
        }
    }
    at as f32 / 1000.0
}

/// Just the main rotor: no engine to speak of (1 Hz, high-passed away), no
/// tail, a close listener.
fn rotor_only(extra: Value) -> Vec<Value> {
    let mut f = json!({"engine": 0, "tail": 0});
    f.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    heli(f)
}

#[test]
fn helicopter_blade_passes_follow_rotor_rate() {
    let a = node_audio(rotor_only(json!({"rotor_rate": 14.2})), "n", 3.0);
    assert_finite(&a, "helicopter");
    let pa = envelope_period(&a[SR as usize / 2..], 0.05, 0.09);
    let b = node_audio(rotor_only(json!({"rotor_rate": 25})), "n", 3.0);
    let pb = envelope_period(&b[SR as usize / 2..], 0.03, 0.05);
    println!("rotor envelope period at 14.2 / 25 passes a second: {pa:.4} / {pb:.4} s");
    assert!((pa - 1.0 / 14.2).abs() < 0.004, "{pa}");
    assert!((pb - 1.0 / 25.0).abs() < 0.004, "{pb}");
}

#[test]
fn rotor_rate_is_a_signal() {
    // A ramp from 10 to 30 passes a second over 6 s: the pulse gets shorter.
    let mut nodes = heli(json!({"engine": 0, "tail": 0, "rotor_rate": "sweep"}));
    nodes.push(json!({"type": "ramp", "name": "sweep", "from": 10, "to": 30, "time": 6, "curve": "linear"}));
    let x = node_audio(nodes, "n", 6.0);
    let sec = SR as usize;
    let early = envelope_period(&x[sec / 2..2 * sec], 0.03, 0.14);
    let late = envelope_period(&x[4 * sec..], 0.03, 0.14);
    println!("period early / late {early:.4} / {late:.4} s");
    assert!(early > late * 1.3, "{early} {late}");
}

/// What `engine` adds: the render at `speed` minus the one at 0, whose
/// rotor, noise and distance paths are the same (same seed and draws).
fn engine_part(speed: f32) -> Vec<f32> {
    let on = node_audio(heli(json!({"engine": speed, "tail": 0})), "n", 2.0);
    let off = node_audio(heli(json!({"engine": 0, "tail": 0})), "n", 2.0);
    on.iter()
        .zip(&off)
        .map(|(a, b)| a - b)
        .skip(SR as usize / 2)
        .collect()
}

#[test]
fn engine_speed_sets_the_pulse_rate_of_the_engine() {
    let slow = engine_part(0.2);
    let fast = engine_part(0.9);
    let (cs, cf) = (spectrum_centroid(&slow), spectrum_centroid(&fast));
    println!(
        "engine part rms {:.1} / {:.1} dB",
        rms_db(&slow),
        rms_db(&fast)
    );
    println!("engine centroid at speed 0.2 / 0.9: {cs:.0} / {cf:.0} Hz");
    assert!(cf > cs * 1.3, "{cs} {cf}");
    // The pulses run at the book's 1 + 90 x speed Hz: lines on its
    // harmonics, none between them.
    let fe = 1.0 + 90.0 * 0.9;
    let spec = spectrum(&fast, SR);
    let near = |hz: f32| {
        spec.iter()
            .filter(|(f, _)| (f - hz).abs() < 3.0)
            .map(|(_, d)| *d)
            .fold(f32::MIN, f32::max)
    };
    let on = near(3.0 * fe) + near(4.0 * fe) + near(5.0 * fe);
    let off = near(3.5 * fe) + near(4.5 * fe) + near(5.5 * fe);
    println!("engine lines on 3-5 x {fe:.0} Hz: {on:.1} dB, between: {off:.1} dB");
    assert!(on > off + 6.0);
}

#[test]
fn the_tail_rotor_adds_its_150_hz_buzz() {
    // The tail rotor turns at 1.17 x the engine (75 Hz at the default
    // speed); its amplitude 1 / (1 + 2.25 cos^2) repeats twice a turn.
    let off = node_audio(heli(json!({"tail": 0})), "n", 2.0);
    let on = node_audio(heli(json!({"tail": 1})), "n", 2.0);
    let b = |x: &[f32]| band_db(&x[SR as usize / 2..], SR, 140.0, 160.0);
    println!("140-160 Hz tail 0 / 1: {:.1} / {:.1} dB", b(&off), b(&on));
    assert!(b(&on) > b(&off) + 6.0);
}

#[test]
fn distance_darkens_and_quiets_the_helicopter() {
    let near = node_audio(heli(json!({"distance": 0})), "n", 2.0);
    let far = node_audio(heli(json!({"distance": 0.9})), "n", 2.0);
    let hi = |x: &[f32]| band_db(&x[SR as usize / 2..], SR, 800.0, 8000.0);
    println!("800-8k near / far {:.1} / {:.1} dB", hi(&near), hi(&far));
    assert!(hi(&far) < hi(&near) - 12.0);
    assert!(spectrum_centroid(&far) < spectrum_centroid(&near));
}

#[test]
fn helicopter_seeds_differ_and_the_same_seed_repeats() {
    let a = node_audio(heli(json!({"seed": 1})), "n", 1.0);
    let b = node_audio(heli(json!({"seed": 2})), "n", 1.0);
    assert_ne!(fnv(&a), fnv(&b));
    assert_bit_identical(heli(json!({"seed": 3})), 1.0);
    // The raw level the patch gives (the output stage normalises).
    println!(
        "helicopter raw rms {:.1} dB, peak {:.1} dB",
        rms_db(&a),
        peak_db(&a)
    );
}

#[test]
fn rocket_is_silent_until_ignition_then_roars_and_fades() {
    let x = node_audio(rocket(json!({"ignition": 0.5, "burn": 3})), "n", 5.0);
    assert_finite(&x, "rocket");
    let sec = SR as usize;
    assert!(rms_db(&x[..sec / 2 - 100]) < -100.0);
    let early = rms_db(&x[sec..2 * sec]);
    let late = rms_db(&x[4 * sec..]);
    println!(
        "rocket rms 1-2 s {early:.1} dB, after 4 s {late:.1} dB, peak {:.1}",
        peak_db(&x)
    );
    assert!(early > -60.0);
    assert!(late < early - 12.0);
}

#[test]
fn rocket_pipe_climbs_with_thrust() {
    // The pipe resonance is 40 + 800 (T^30 + 0.3 T^4) Hz: a thrust of 1
    // puts four resonances at 1, 3, 5, 7 x 1 kHz, 0.5 at 1, 3, 5, 7 x 55 Hz.
    let full = node_audio(rocket(json!({"thrust": 1, "ignition": 0})), "n", 3.0);
    let half = node_audio(rocket(json!({"thrust": 0.5, "ignition": 0})), "n", 3.0);
    let tail = |x: &[f32]| x[SR as usize..].to_vec();
    let (cf, ch) = (
        spectrum_centroid(&tail(&full)),
        spectrum_centroid(&tail(&half)),
    );
    println!("rocket centroid at thrust 1 / 0.5: {cf:.0} / {ch:.0} Hz");
    assert!(cf > ch * 3.0, "{cf} {ch}");
    let band = |x: &[f32], lo, hi| band_db(&tail(x), SR, lo, hi);
    println!(
        "900-1300 Hz at thrust 1 / 0.5: {:.1} / {:.1} dB; 40-400 Hz: {:.1} / {:.1} dB",
        band(&full, 900.0, 1300.0),
        band(&half, 900.0, 1300.0),
        band(&full, 40.0, 400.0),
        band(&half, 40.0, 400.0)
    );
    assert!(band(&full, 900.0, 1300.0) > band(&half, 900.0, 1300.0) + 15.0);
    // Low against high: the half-thrust pipe sits under 400 Hz, the full
    // one's odd harmonics reach 7.5 kHz.
    let tilt = |x: &[f32]| band(x, 40.0, 400.0) - band(x, 2000.0, 8000.0);
    println!(
        "low - high tilt at thrust 1 / 0.5: {:.1} / {:.1} dB",
        tilt(&full),
        tilt(&half)
    );
    assert!(tilt(&half) > tilt(&full) + 20.0);
}

#[test]
fn rocket_sweeps_down_as_its_default_thrust_burns_out() {
    let x = node_audio(rocket(json!({"burn": 4})), "n", 5.0);
    let d = describe(&mono_audio(&x));
    println!("rocket centroid by tenth {:?}", d.centroid_tenths);
    let c: Vec<f32> = d.centroid_tenths.iter().map(|v| v.unwrap_or(0.0)).collect();
    assert!(c[0] > c[8] * 3.0, "{c:?}");
    assert_bit_identical(rocket(json!({"seed": 2})), 1.0);
    let a = node_audio(rocket(json!({"seed": 1})), "n", 1.0);
    let b = node_audio(rocket(json!({"seed": 2})), "n", 1.0);
    assert_ne!(fnv(&a), fnv(&b));
}

#[test]
fn the_vehicle_nodes_reject_bad_fields() {
    for (nodes, what) in [
        (heli(json!({"tail": 2})), "tail"),
        (rocket(json!({"burn": 0.1})), "burn"),
        (rocket(json!({"ignition": -1})), "ignition"),
    ] {
        let e = render_error(nodes, "n", 0.1);
        println!("{what}: {e}");
        assert!(e.contains(what), "{e}");
    }
}
