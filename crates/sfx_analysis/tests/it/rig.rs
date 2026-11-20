//! `rig`: bodies and joints stepped together.

use crate::common::*;
use serde_json::{Value, json};

fn out(nodes: Vec<Value>) -> Vec<Value> {
    let mut n = nodes;
    n.push(json!({"type": "output", "name": "out", "input": "r", "fade_in": 0.0005}));
    n
}

/// Start times of each contact (s), from the `force` port, touches within
/// 3 ms of the last merged.
fn landings(nodes: &[Value], secs: f32) -> Vec<f32> {
    let f = node_audio(nodes, "r.force", secs);
    let (mut t, mut off) = (Vec::new(), usize::MAX / 2);
    for (i, x) in f.iter().enumerate() {
        if *x > 1e-4 {
            if off > 144 {
                t.push(i as f32 / 48_000.0);
            }
            off = 0;
        } else {
            off += 1;
        }
    }
    t
}

fn ball_on_plate(extra_joint: Value) -> Vec<Value> {
    let joint = with_fields(
        json!({"kind": "contact", "a": "ball", "b": "plate", "gap": 0.2, "bounce": 0.6, "hardness": 0.9}),
        extra_joint,
    );
    out(vec![json!({"type": "rig", "name": "r", "bodies": [
        {"name": "ball", "mass": 0.05},
        {"name": "plate", "mass": 3.0, "fixed": true, "material": "metal", "size": 0.4,
         "thickness": 0.003, "loss": 0.5}],
        "joints": [joint]})])
}

#[test]
fn a_ball_dropped_on_a_plate_bounces_under_gravity() {
    let t = landings(&ball_on_plate(json!({})), 2.0);
    // It falls 0.2 m first: about 0.2 s.
    assert!((t[0] - 0.202).abs() < 0.01, "first landing {t:?}");
    assert!(t.len() >= 4, "landings {t:?}");
    let gaps: Vec<f32> = t.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps[1] < gaps[0] && gaps[2] < gaps[1], "gaps {gaps:?}");
}

#[test]
fn a_free_body_drops_on_the_ground_and_rings() {
    let n = out(vec![json!({"type": "rig", "name": "r", "bodies": [
        {"name": "pipe", "mass": 2.0, "material": "metal", "shape": "tube", "f0": 160, "loss": 0.5}],
        "joints": [{"a": "pipe", "b": "ground", "gap": 0.1, "bounce": 0.3}]})]);
    let x = node_audio(&n, "r", 1.0);
    assert_finite(&x, "rig");
    // While it lands (0.14-0.25 s); once it rests the contact pins its
    // lowest mode (see the limits in docs/nodes/rig.md).
    let hz = peak_hz(&x[7000..12_000], 48_000);
    assert!((hz / 160.0 - 1.0).abs() < 0.05, "rings at {hz} Hz");
    assert!(!landings(&n, 1.0).is_empty());
}

#[test]
fn a_loose_screw_rattles_and_a_tight_one_does_not() {
    let sheet = |gap: f64| {
        out(vec![json!({"type": "rig", "name": "r", "bodies": [
            {"name": "hammer", "mass": 0.4, "velocity": -3.0},
            {"name": "sheet", "mass": 2.0, "fixed": true, "material": "metal", "size": 0.5,
             "thickness": 0.002, "loss": 0.4},
            {"name": "screw", "mass": 0.005, "material": "metal", "shape": "bar", "f0": 3000,
             "loss": 0.6}],
            "gravity": 0,
            "joints": [
                {"a": "hammer", "b": "sheet", "at_b": 0.4, "hardness": 0.9},
                {"kind": "spring", "a": "screw", "b": "sheet", "at_b": 0.8, "gap": gap,
                 "stiffness": 2e5, "damping": 0.5}]})])
    };
    let loose = node_audio(sheet(0.0002), "r", 1.0);
    let tight = node_audio(sheet(0.0), "r", 1.0);
    assert_finite(&loose, "loose");
    // The screw's own ring (near 3 kHz) is struck again and again by the
    // free play, so it carries more of the late sound.
    let late = |x: &[f32]| {
        band_db(&x[9600..], 48_000, 2500.0, 3500.0) - band_db(&x[9600..], 48_000, 100.0, 2000.0)
    };
    assert!(
        late(&loose) > late(&tight) + 3.0,
        "loose {} tight {}",
        late(&loose),
        late(&tight)
    );
}

#[test]
fn the_example_is_finite_and_deterministic() {
    let n = ball_on_plate(json!({}));
    assert_bit_identical(n, 1.0);
}

#[test]
fn errors_name_the_body_and_joint() {
    let e = render_error_raw(ball_on_plate(json!({"b": "table"})), 1.0);
    assert!(e.contains("no body `table`"), "{e}");
    let e = render_error_raw(
        out(vec![json!({"type": "rig", "name": "r", "bodies": [
            {"name": "x", "mass": 1.0}, {"name": "x", "mass": 1.0}], "joints": []})]),
        1.0,
    );
    assert!(e.contains("two bodies are named `x`"), "{e}");
}

#[test]
fn a_dead_drop_does_not_fly_again() {
    // A free tube with `bounce` 0 on the ground: its modes would bounce it
    // like rubber without the rebound cap; it buzzes but never flies.
    let n = out(vec![json!({"type": "rig", "name": "r", "bodies": [
        {"name": "pipe", "mass": 2.0, "material": "metal", "shape": "tube", "f0": 160, "loss": 0.4}],
        "joints": [{"a": "pipe", "b": "ground", "gap": 0.3, "bounce": 0.0}]})]);
    let t = landings(&n, 1.5);
    assert!(t.iter().all(|x| *x < 0.4), "landings {t:?}");
}

#[test]
fn a_bow_on_a_bar_sings_on_a_mode() {
    let n = out(vec![
        json!({"type": "rig", "name": "r", "gravity": 0, "bodies": [
        {"name": "bow", "mass": 0.05, "drive": -3.0},
        {"name": "bar", "mass": 0.3, "fixed": true, "material": "metal", "shape": "bar",
         "f0": 600, "loss": 0.5}],
        "joints": [{"a": "bow", "b": "bar", "at_b": 0.3, "hardness": 0.5,
                    "slide": 0.03, "friction": [0.8, 0.3], "stribeck": 0.05}]}),
    ]);
    let x = node_audio(&n, "r", 1.0);
    assert_finite(&x, "bow");
    let tail = &x[24_000..];
    assert!(
        rms(tail) > 0.02,
        "the bow should keep it sounding: {}",
        rms(tail)
    );
    let f = peak_hz(tail, 48_000);
    let near = [600.0f32, 1654.0, 3243.0]
        .iter()
        .any(|m| (f / m - 1.0).abs() < 0.04);
    assert!(near, "bowed peak {f} Hz is not a bar mode of 600 Hz");
}

#[test]
fn friction_belongs_to_a_contact() {
    let e = render_error_raw(
        out(vec![json!({"type": "rig", "name": "r", "bodies": [
            {"name": "x", "mass": 1.0}], "joints": [
            {"kind": "spring", "a": "x", "b": "ground", "friction": [0.5, 0.3]}]})]),
        1.0,
    );
    assert!(e.contains("for a contact joint"), "{e}");
}

fn ball_on(body: Value, at_b: f64, secs: f32) -> Vec<f32> {
    let n = out(vec![
        json!({"type": "rig", "name": "r", "gravity": 0, "bodies": [
        {"name": "ball", "mass": 0.01, "velocity": -1.0}, body],
        "joints": [{"a": "ball", "b": "w", "at_b": at_b, "hardness": 0.8, "bounce": 0.3}]}),
    ]);
    let x = node_audio(&n, "r", secs);
    assert_finite(&x, "rig");
    x
}

#[test]
fn a_struck_string_body_rings_at_its_f0() {
    for f0 in [110.0f32, 330.0] {
        let x = ball_on(
            json!({"name": "w", "mass": 0.005, "waveguide": {"kind": "string", "f0": f0}}),
            0.2,
            1.0,
        );
        let hz = peak_hz(&x[4800..], 48_000);
        assert!((hz / f0 - 1.0).abs() < 0.02, "f0 {f0}: rings at {hz} Hz");
    }
    // length and tension: f0 = sqrt(T L / m) / 2L = sqrt(126 * 0.65 / 0.004) / 1.3, about 110 Hz.
    let x = ball_on(
        json!({"name": "w", "mass": 0.004, "waveguide": {"length": 0.65, "tension": 126}}),
        0.2,
        1.0,
    );
    let want = (126.0f32 * 0.65 / 0.004).sqrt() / 1.3;
    let hz = peak_hz(&x[4800..], 48_000);
    assert!(
        (hz / want - 1.0).abs() < 0.02,
        "want {want}: rings at {hz} Hz"
    );
}

#[test]
fn a_tube_body_resonates_at_its_length() {
    for (ends, want) in [("open", 343.0 / 1.0), ("closed", 343.0 / 2.0)] {
        let x = ball_on(
            json!({"name": "w", "mass": 0.1, "waveguide": {"kind": "tube", "length": 0.5,
                   "ends": ends, "decay": 1.0}}),
            0.3,
            0.6,
        );
        // The lowest strong partial (an open end radiates the upper ones
        // more).
        let hz = peaks(&x[2400..], 48_000, 6, 30.0)
            .iter()
            .fold(f32::INFINITY, |a, p| a.min(p.0));
        assert!(
            (hz / want - 1.0).abs() < 0.03,
            "{ends}: want {want}, got {hz} Hz"
        );
    }
}

fn drum_over_tube(strength: f64) -> Vec<Value> {
    out(vec![
        json!({"type": "rig", "name": "r", "gravity": 0, "bodies": [
        {"name": "stick", "mass": 0.03, "velocity": -2.0},
        {"name": "skin", "mass": 0.05, "fixed": true, "material": "metal", "shape": "plate",
         "f0": 900, "loss": 2.0},
        {"name": "shell", "mass": 0.1, "waveguide": {"kind": "tube", "length": 0.8,
         "ends": "closed", "decay": 1.0}}],
        "joints": [
            {"a": "stick", "b": "skin", "at_b": 0.4, "hardness": 0.6},
            {"kind": "air", "a": "skin", "b": "shell", "at_a": 0.4, "strength": strength}]}),
    ])
}

#[test]
fn the_air_joint_moves_energy_into_the_tube() {
    // The shell sounds c / 4L = 107 Hz; the skin's lowest mode 900 Hz.
    let band = |s: f64| {
        let x = node_audio(drum_over_tube(s), "r", 1.0);
        assert_finite(&x, "drum");
        band_db(&x[2400..], 48_000, 90.0, 125.0) - band_db(&x[2400..], 48_000, 800.0, 4000.0)
    };
    let (off, on) = (band(0.0), band(2.0));
    assert!(
        on > off + 10.0,
        "tube band vs skin: off {off} dB, on {on} dB"
    );
}

#[test]
fn waveguide_rigs_are_bit_identical() {
    assert_bit_identical(drum_over_tube(2.0), 0.5);
}

#[test]
fn a_string_held_by_a_stiff_spring_stays_finite() {
    // A joint feels its own push on a waveguide (a spring of 2e5 N/m at the
    // bridge end of a 2 g string).
    for (k, pos) in [(2e4, 0.98), (2e5, 0.5)] {
        let n = out(vec![
            json!({"type": "rig", "name": "r", "gravity": 0, "bodies": [
            {"name": "box", "mass": 0.5, "fixed": true},
            {"name": "pick", "mass": 0.002, "velocity": -1.5},
            {"name": "w", "mass": 0.002, "waveguide": {"f0": 196}}],
            "joints": [{"a": "pick", "b": "w", "at_b": 0.15, "hardness": 0.8},
                {"kind": "spring", "a": "box", "b": "w", "at_b": pos, "stiffness": k}]}),
        ]);
        assert_finite(&node_audio(&n, "r", 0.5), "string on a spring");
    }
}
