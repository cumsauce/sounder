//! `step` (foley's footstep contact; docs/nodes/step.md has the numbers
//! these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn step(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "step", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn every_surface_renders_finite_and_follows_the_seed() {
    for s in [
        "concrete", "gravel", "grass", "dirt", "wood", "metal", "ice", "snow", "carpet",
    ] {
        let a = node_audio(step(json!({"surface": s})), "n", 0.6);
        let b = node_audio(step(json!({"surface": s, "seed": 2})), "n", 0.6);
        assert!(a.iter().all(|v| v.is_finite() && v.abs() <= 2.0), "{s}");
        assert!(rms(&a) > 1e-4, "{s}: silent");
        assert_ne!(a, b, "{s}: the seed changes nothing");
        let d = describe(&mono_audio(&a));
        println!(
            "{s}: centroid {:.0} Hz, shares {:?}",
            d.centroid_all_hz, d.band_shares
        );
    }
}

#[test]
fn a_walk_has_one_onset_group_per_step_and_gravel_crunches() {
    let walk = node_audio(
        step(json!({"surface": "concrete", "at": [0.005, 0.6, 1.2]})),
        "n",
        1.8,
    );
    let d = describe(&mono_audio(&walk));
    assert!(
        (3..=8).contains(&d.onsets.len()),
        "{} onsets",
        d.onsets.len()
    );
    let gravel = describe(&mono_audio(&node_audio(
        step(json!({"surface": "gravel"})),
        "n",
        0.6,
    )));
    let carpet = describe(&mono_audio(&node_audio(
        step(json!({"surface": "carpet"})),
        "n",
        0.6,
    )));
    assert!(
        gravel.centroid_all_hz > carpet.centroid_all_hz,
        "grains are brighter than carpet"
    );
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = render_error_raw(step(json!({"speed": 9})), 0.5);
    assert!(e.contains("speed") && e.contains("0.2 to 3"), "{e}");
    let e = render_error_raw(step(json!({"at": [-1]})), 0.5);
    assert!(e.contains("at"), "{e}");
    let e = render_error_raw(step(json!({"surface": "lava"})), 0.5);
    assert!(e.contains("concrete"), "{e}");
}

#[test]
fn grit_takes_the_crunch_out_and_the_toe_rings_short() {
    let crunch = describe(&mono_audio(&node_audio(
        step(json!({"surface": "gravel"})),
        "n",
        0.6,
    )));
    let bare = describe(&mono_audio(&node_audio(
        step(json!({"surface": "gravel", "grit": 0})),
        "n",
        0.6,
    )));
    println!(
        "gravel grit 1 centroid {:.0} Hz, grit 0 {:.0} Hz",
        crunch.centroid_all_hz, bare.centroid_all_hz
    );
    assert!(
        bare.centroid_all_hz < crunch.centroid_all_hz,
        "grit 0 is duller"
    );
    let e = render_error_raw(step(json!({"grit": 3})), 0.5);
    assert!(e.contains("grit") && e.contains("0 to 2"), "{e}");
}

#[test]
fn body_and_texture_add_back_to_the_step() {
    for s in ["gravel", "grass", "snow", "concrete"] {
        let all = node_audio(step(json!({"surface": s, "at": [0.005, 0.5]})), "n", 1.0);
        let body = node_audio(
            step(json!({"surface": s, "at": [0.005, 0.5], "layer": "body"})),
            "n",
            1.0,
        );
        let tex = node_audio(
            step(json!({"surface": s, "at": [0.005, 0.5], "layer": "texture"})),
            "n",
            1.0,
        );
        let err = all
            .iter()
            .zip(body.iter().zip(&tex))
            .fold(0f32, |m, (a, (b, t))| m.max((a - b - t).abs()));
        assert!(err < 1e-5, "{s}: body + texture is off by {err}");
        println!(
            "{s}: body rms {:.4}, texture rms {:.4}",
            rms(&body),
            rms(&tex)
        );
    }
}
