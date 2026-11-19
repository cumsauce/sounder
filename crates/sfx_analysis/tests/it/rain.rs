//! `rain`: physically sized drops (after the foley project's rain bed;
//! docs/nodes/rain.md has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, describe, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn rain(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "rain", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

const SURFACES: [&str; 7] = ["water", "stone", "leaf", "wood", "soft", "glass", "metal"];

#[test]
fn each_surface_renders_finite_and_follows_the_seed() {
    for s in SURFACES {
        let a = node_audio(rain(json!({"surface": s})), "n", 2.0);
        let b = node_audio(rain(json!({"surface": s, "seed": 2})), "n", 2.0);
        assert!(a.iter().all(|v| v.is_finite()), "{s}");
        assert!(rms(&a) > 1e-4, "{s}: silent");
        assert_ne!(a, b, "{s}: the seed changes nothing");
    }
}

#[test]
fn rain_is_dense_and_surfaces_differ() {
    let mut cents = vec![];
    for s in SURFACES {
        let x = node_audio(rain(json!({"surface": s})), "n", 4.0);
        let d = describe(&mono_audio(&x));
        println!(
            "{s}: rms {:.1} dB, centroid {:.0} Hz, density {:.0}/s, shares {:?}",
            20.0 * rms(&x).log10(),
            d.centroid_all_hz,
            d.density.per_s,
            d.band_shares
        );
        cents.push((s, d.centroid_all_hz));
    }
    let c = |n: &str| cents.iter().find(|(s, _)| *s == n).unwrap().1;
    assert!(c("stone") > c("leaf"), "stone ticks, leaves patter");
    assert!(c("water") > c("leaf"), "water sizzles");
}

#[test]
fn heavier_rain_is_louder_and_a_roof_muffles_it() {
    let light = rms(&node_audio(rain(json!({"rate": 2})), "n", 3.0));
    let heavy = rms(&node_audio(rain(json!({"rate": 50})), "n", 3.0));
    assert!(heavy > light * 1.5, "{light} {heavy}");
    let open = describe(&mono_audio(&node_audio(rain(json!({})), "n", 3.0)));
    let under = describe(&mono_audio(&node_audio(
        rain(json!({"roof": "wood"})),
        "n",
        3.0,
    )));
    println!(
        "centroid open {:.0} Hz, under wood {:.0} Hz",
        open.centroid_all_hz, under.centroid_all_hz
    );
    assert!(
        under.centroid_all_hz < open.centroid_all_hz,
        "under wood it drums duller"
    );
}

#[test]
fn a_loop_document_passes_the_default_checks() {
    for s in ["water", "stone", "leaf"] {
        let mut d = SoundDoc::named("loop");
        d.nodes = rain(json!({"surface": s, "seed": 5}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
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
        assert!(res.iter().all(|r| r.pass), "{s}:\n{}", report(&res));
    }
}

#[test]
fn bad_fields_say_what_to_give() {
    for bad in [json!({"rate": 0}), json!({"rate": 900})] {
        let e = crate::common::render_error(rain(bad), "n", 0.1);
        assert!(e.contains("give mm/h"), "{e}");
    }
    let e = crate::common::render_error(rain(json!({"roof_height": 0.1})), "n", 0.1);
    assert!(e.contains("roof_height"), "{e}");
}
