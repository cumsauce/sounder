//! `lava` and `cave` (after the foley project's beds; docs/nodes/cave.md
//! has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, describe, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn node(kind: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": kind, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn both_render_finite_and_follow_the_seed() {
    for kind in ["lava", "cave"] {
        let a = node_audio(node(kind, json!({})), "n", 3.0);
        let b = node_audio(node(kind, json!({"seed": 2})), "n", 3.0);
        assert!(a.iter().all(|v| v.is_finite() && v.abs() <= 20.0), "{kind}");
        assert!(rms(&a) > 1e-4, "{kind}: silent");
        assert_ne!(a, b, "{kind}: the seed changes nothing");
    }
}

#[test]
fn lava_is_a_low_rumble_with_bursts() {
    let x = node_audio(node("lava", json!({})), "n", 6.0);
    let d = describe(&mono_audio(&x));
    println!(
        "lava: rms {:.3}, centroid {:.0} Hz, onsets {}, shares {:?}",
        rms(&x),
        d.centroid_all_hz,
        d.onsets.len(),
        d.band_shares
    );
    assert!(d.band_shares[0] > 0.5, "the rumble sits under 200 Hz");
    assert!(rms(&x) > 0.005 && rms(&x) < 3.0, "{}", rms(&x));
    let calm = node_audio(node("lava", json!({"activity": 0})), "n", 6.0);
    let dc = describe(&mono_audio(&calm));
    assert!(
        dc.centroid_all_hz < d.centroid_all_hz,
        "bursts add the mid band: {} vs {}",
        dc.centroid_all_hz,
        d.centroid_all_hz
    );
    let big = node_audio(node("lava", json!({"size": 200})), "n", 3.0);
    let small = node_audio(node("lava", json!({"size": 1})), "n", 3.0);
    assert!(rms(&big) > rms(&small) * 2.0);
}

#[test]
fn cave_drips_sit_in_the_drip_band_and_openness_fades_it() {
    let wet = node_audio(node("cave", json!({"drips": 1.0, "depth": 0})), "n", 4.0);
    let d = describe(&mono_audio(&wet));
    println!(
        "cave wet: rms {:.4}, centroid {:.0} Hz, onsets {}",
        rms(&wet),
        d.centroid_all_hz,
        d.onsets.len()
    );
    assert!(
        (700.0..1500.0).contains(&d.centroid_all_hz),
        "drips ring 0.8-1.4 kHz: {}",
        d.centroid_all_hz
    );
    assert!(d.onsets.len() >= 8, "{} onsets", d.onsets.len());
    let dry = node_audio(node("cave", json!({"drips": 0.0})), "n", 4.0);
    let dd = describe(&mono_audio(&dry));
    println!(
        "cave dry: rms {:.4}, centroid {:.0} Hz",
        rms(&dry),
        dd.centroid_all_hz
    );
    assert!(dd.centroid_all_hz < 400.0, "a dry deep cave only rumbles");
    let open = node_audio(
        node("cave", json!({"drips": 1.0, "openness": 1.0})),
        "n",
        2.0,
    );
    assert_eq!(rms(&open), 0.0, "an open cave is silent");
    let half = node_audio(
        node("cave", json!({"drips": 1.0, "openness": 0.5})),
        "n",
        2.0,
    );
    let shut = node_audio(node("cave", json!({"drips": 1.0})), "n", 2.0);
    assert!(rms(&half) < rms(&shut));
}

#[test]
fn a_loop_document_passes_the_default_checks() {
    for kind in ["lava", "cave"] {
        let mut d = SoundDoc::named("loop");
        d.nodes = node(kind, json!({"seed": 5}));
        // The cave's 55 Hz rumble leaves a little DC in a 4 s window: a 20 Hz
        // high-pass before the output, as the dc check advises.
        d.nodes
            .push(json!({"type": "onepole", "name": "hp", "input": "n", "mode": "hp", "freq": 20}));
        // The filter's start-up breaks the node's own seam, so the loop ends
        // with crossfade_loop, as the loop rules say.
        d.nodes
            .push(json!({"type": "crossfade_loop", "name": "xf", "input": "hp"}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "xf"}));
        d.duration = Some(4.0);
        d.kind = Kind::Loop;
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap();
        let res = run(
            &Expect::defaults_for(&d),
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap();
        assert!(res.iter().all(|r| r.pass), "{kind}:\n{}", report(&res));
    }
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = render_error(node("lava", json!({"size": 0})), "n", 0.1);
    assert!(e.contains("give square metres"), "{e}");
    let e = render_error(node("lava", json!({"activity": 9})), "n", 0.1);
    assert!(e.contains("activity"), "{e}");
    let e = render_error(node("cave", json!({"depth": 2})), "n", 0.1);
    assert!(e.contains("give a fraction"), "{e}");
}
