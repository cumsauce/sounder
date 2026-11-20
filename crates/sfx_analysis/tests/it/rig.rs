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
