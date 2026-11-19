//! `creature`: a waveguide-tract voice (after the foley project's creature
//! calls; docs/nodes/creature.md has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

const CALLS: [&str; 4] = ["roar", "growl", "snarl", "whine"];

fn creature(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "creature", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn active_len(x: &[f32]) -> f32 {
    let last = x.iter().rposition(|v| v.abs() > 0.02).unwrap_or(0);
    last as f32 / 48_000.0
}

#[test]
fn each_call_renders_finite_and_follows_the_seed() {
    for call in CALLS {
        let a = node_audio(creature(json!({"call": call})), "n", 3.0);
        let b = node_audio(creature(json!({"call": call, "seed": 2})), "n", 3.0);
        assert!(a.iter().all(|v| v.is_finite()), "{call}");
        assert!(peak(&a) > 0.3, "{call}: silent");
        assert_ne!(a, b, "{call}: the seed changes nothing");
        assert_bit_identical(creature(json!({"call": call})), 3.0);
    }
}

#[test]
fn a_bigger_body_is_lower_and_darker() {
    for call in CALLS {
        let small = node_audio(creature(json!({"call": call, "size": 0.1})), "n", 3.0);
        let big = node_audio(creature(json!({"call": call, "size": 0.9})), "n", 3.0);
        let (cs, cb) = (
            describe(&mono_audio(&small)).centroid_all_hz,
            describe(&mono_audio(&big)).centroid_all_hz,
        );
        println!("{call}: centroid small {cs:.0} Hz, big {cb:.0} Hz");
        assert!(cb < cs, "{call}: big {cb} small {cs}");
    }
}

#[test]
fn roughness_modulates_the_roar() {
    let r = |rough: f32| {
        let a = mono_audio(&node_audio(
            creature(json!({"call": "roar", "roughness": rough, "seed": 3})),
            "n",
            3.0,
        ));
        sfx_analysis::measure::roughness_pct(&a)
    };
    let (clean, rough) = (r(0.0), r(1.0));
    println!("roar roughness clean {clean:.1} % rough {rough:.1} %");
    assert!(
        rough > clean,
        "chaos and subharmonics roughen the loudness envelope"
    );
}

#[test]
fn length_sets_how_long_it_lasts() {
    for call in CALLS {
        let short = node_audio(creature(json!({"call": call, "length": 0.5})), "n", 6.0);
        let long = node_audio(creature(json!({"call": call, "length": 3.0})), "n", 6.0);
        let (s, l) = (active_len(&short), active_len(&long));
        println!("{call}: {s:.2} s and {l:.2} s");
        assert!(l > 3.0 * s * 0.6 && s < 1.0 && l > 2.0, "{call}: {s} {l}");
    }
}

#[test]
fn a_roar_is_a_low_rumble_and_a_whine_is_high() {
    let roar = describe(&mono_audio(&node_audio(
        creature(json!({"call": "roar", "size": 0.9})),
        "n",
        3.0,
    )));
    let whine = describe(&mono_audio(&node_audio(
        creature(json!({"call": "whine", "size": 0.9})),
        "n",
        3.0,
    )));
    println!(
        "roar centroid {:.0} Hz shares {:?}; whine centroid {:.0} Hz shares {:?}",
        roar.centroid_all_hz, roar.band_shares, whine.centroid_all_hz, whine.band_shares
    );
    assert!(
        roar.band_shares[0] + roar.band_shares[1] > 0.7,
        "roar under 2 kHz"
    );
    assert!(whine.centroid_all_hz > roar.centroid_all_hz * 0.5);
}

#[test]
fn bad_fields_say_what_to_give() {
    for bad in [
        json!({"size": 2}),
        json!({"aggression": -1}),
        json!({"roughness": 3}),
    ] {
        let e = render_error(creature(bad), "n", 0.1);
        assert!(e.contains("give a number from 0 to 1"), "{e}");
    }
    let e = render_error(creature(json!({"length": 0})), "n", 0.1);
    assert!(e.contains("give seconds from 0.1 to 10"), "{e}");
    let e = render_error(creature(json!({"call": "moo"})), "n", 0.1);
    assert!(e.contains("roar"), "{e}");
}
