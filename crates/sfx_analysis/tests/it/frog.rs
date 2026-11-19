//! `frog`: calling frogs (after the foley project's life.rs;
//! docs/nodes/frog.md has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn frog(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "frog", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn centroid(extra: Value, secs: f32) -> f32 {
    let x = node_audio(frog(extra), "n", secs);
    describe(&mono_audio(&x)).centroid_all_hz
}

#[test]
fn renders_finite_and_follows_the_seed() {
    let a = node_audio(frog(json!({})), "n", 4.0);
    let b = node_audio(frog(json!({"seed": 2})), "n", 4.0);
    assert_finite(&a, "frog");
    assert!(rms(&a) > 1e-3, "silent");
    assert_ne!(a, b, "the seed changes nothing");
    assert_bit_identical(frog(json!({"chorus": 3})), 3.0);
}

#[test]
fn a_big_frog_is_lower_than_a_small_one() {
    let (small, big) = (
        centroid(json!({"size": 3}), 6.0),
        centroid(json!({"size": 15}), 6.0),
    );
    println!("3 cm {small:.0} Hz, 15 cm {big:.0} Hz");
    assert!(small > 1.5 * big, "{small} {big}");
    // The sac rings near 2.5 kHz at 3 cm and 500 Hz at 15 cm (the centroid sits lower: the pulses are broad).
    assert!((900.0..4500.0).contains(&small), "{small}");
    assert!((300.0..1800.0).contains(&big), "{big}");
}

#[test]
fn rate_and_chorus_fill_the_sound() {
    let active = |x: &[f32]| {
        let win = 4800;
        let on = x
            .chunks(win)
            .filter(|c| c.iter().any(|v| v.abs() > 0.02))
            .count();
        on as f32 / x.len().div_ceil(win) as f32
    };
    let slow = node_audio(frog(json!({"rate": 0.1, "seed": 3})), "n", 20.0);
    let fast = node_audio(frog(json!({"rate": 2.0, "seed": 3})), "n", 20.0);
    let crowd = node_audio(
        frog(json!({"rate": 0.5, "chorus": 8, "seed": 3})),
        "n",
        20.0,
    );
    let one = node_audio(frog(json!({"rate": 0.5, "seed": 3})), "n", 20.0);
    println!(
        "active share: slow {:.2}, fast {:.2}, one {:.2}, chorus {:.2}",
        active(&slow),
        active(&fast),
        active(&one),
        active(&crowd)
    );
    assert!(active(&fast) > active(&slow));
    assert!(active(&crowd) > active(&one));
    assert!(peak(&crowd) < 1.0, "a chorus stays under full scale");
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = render_error(frog(json!({"size": 40})), "n", 1.0);
    assert!(e.contains("size") && e.contains("2 to 20"), "{e}");
    let e = render_error(frog(json!({"rate": 9})), "n", 1.0);
    assert!(e.contains("rate") && e.contains("0.05 to 4"), "{e}");
    let e = render_error(frog(json!({"chorus": 0})), "n", 1.0);
    assert!(e.contains("chorus") && e.contains("1 to 24"), "{e}");
}
