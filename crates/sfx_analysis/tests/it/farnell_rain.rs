//! `rain`, from Farnell's RAIN patches: each surface renders what its patch
//! does (docs/nodes/farnell_rain.md has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, describe, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn render_error(nodes: Vec<Value>) -> String {
    crate::common::render_error(nodes, "n", 0.1)
}

fn rain(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "rain", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

/// Drops a second the 1 ms envelope counter finds.
fn density(x: &[f32]) -> f32 {
    describe(&mono_audio(x)).density.per_s
}

fn surface(s: &str, extra: Value) -> Vec<f32> {
    let mut e = json!({"surface": s});
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    node_audio(rain(e), "n", 10.0)
}

const SURFACES: [&str; 4] = ["leaf", "water", "window", "ground"];

#[test]
fn each_surface_renders_finite_and_follows_the_seed() {
    for s in SURFACES {
        let a = surface(s, json!({"seed": 1}));
        let b = surface(s, json!({"seed": 2}));
        assert_eq!(a.len(), 10 * SR as usize, "{s}");
        assert_finite(&a, s);
        assert!(peak(&a) > 1e-4 && peak(&a) < 1.0, "{s}: peak {}", peak(&a));
        assert_ne!(fnv(&a), fnv(&b), "{s}: seeds should differ");
        println!(
            "{s}: peak {:.1} dB rms {:.1} dB centroid {:.0} Hz density {:.1}/s",
            peak_db(&a),
            rms_db(&a),
            describe(&mono_audio(&a)).centroid_all_hz,
            density(&a)
        );
        assert_bit_identical(rain(json!({"surface": s, "seed": 3})), 2.0);
    }
}

#[test]
fn intensity_sets_the_drops_a_second() {
    // leaf: the patch's own rate is a couple a second; the threshold moves.
    let natural = density(&surface("leaf", json!({})));
    println!("leaf natural {natural:.1} drops/s");
    for (r, lo, hi) in [(4.0, 3.0, 5.0), (12.0, 10.0, 14.0), (30.0, 26.0, 34.0)] {
        let d = density(&surface("leaf", json!({"intensity": r})));
        println!("leaf intensity {r}: {d:.1}");
        assert!((lo..=hi).contains(&d), "leaf {r}: {d}");
    }
    // ground: three random streams, about 9 a second together.
    let natural = density(&surface("ground", json!({})));
    println!("ground natural {natural:.1} drops/s");
    assert!((6.0..=13.0).contains(&natural), "{natural}");
    for (r, lo, hi) in [(4.0, 2.5, 5.5), (30.0, 24.0, 38.0)] {
        let d = density(&surface("ground", json!({"intensity": r})));
        println!("ground intensity {r}: {d:.1}");
        assert!((lo..=hi).contains(&d), "ground {r}: {d}");
    }
    // water and window: more intensity, more drops (window rings, so read level).
    let (w2, w20) = (
        rms_db(&surface("water", json!({"intensity": 2}))),
        rms_db(&surface("water", json!({"intensity": 20}))),
    );
    println!("water rms {w2:.1} / {w20:.1} dB at 2 / 20 drops/s");
    assert!(w20 > w2 + 5.0);
    let (g2, g20) = (
        rms_db(&surface("window", json!({"intensity": 2}))),
        rms_db(&surface("window", json!({"intensity": 20}))),
    );
    println!("window rms {g2:.1} / {g20:.1} dB at 2 / 20 drops/s");
    assert!(g20 > g2 + 5.0);
}

#[test]
fn surfaces_sit_where_their_patches_put_them() {
    // leaf: hip~ 500 twice, so almost nothing under 200 Hz.
    let leaf = surface("leaf", json!({"intensity": 10}));
    let (lo, mid) = (
        band_db(&leaf, SR, 20.0, 200.0),
        band_db(&leaf, SR, 200.0, 2000.0),
    );
    println!("leaf <200 {lo:.1} dB, 200-2k {mid:.1} dB");
    assert!(lo < mid - 6.0);
    // water: bubbles at 400-7200 Hz, none under 200, a little above 8 k.
    let water = surface("water", json!({"intensity": 5}));
    let (lo, mid, hi) = (
        band_db(&water, SR, 20.0, 200.0),
        band_db(&water, SR, 300.0, 3000.0),
        band_db(&water, SR, 8000.0, 20000.0),
    );
    println!("water <200 {lo:.1}, 300-3k {mid:.1}, >8k {hi:.1} dB");
    assert!(mid > lo + 20.0 && mid > hi + 30.0);
    // window: the glass network rings its bands near 2 kHz (and their
    // ringmodulated images), not the 9 kHz spikes that excite it.
    let win = surface("window", json!({"intensity": 10}));
    let (mid, hi) = (
        band_db(&win, SR, 1000.0, 3000.0),
        band_db(&win, SR, 8000.0, 20000.0),
    );
    println!("window 1-3k {mid:.1}, >8k {hi:.1} dB");
    assert!(mid > hi + 8.0);
    assert!(peak(&win) < 0.5, "the glass loop stays stable");
    // ground: clicks of 0.15-0.5 ms high-passed at 900 Hz: bright, none low.
    let g = surface("ground", json!({}));
    let c = describe(&mono_audio(&g)).centroid_all_hz;
    println!("ground centroid {c:.0} Hz");
    assert!(c > 1200.0 && band_db(&g, SR, 20.0, 200.0) < band_db(&g, SR, 2000.0, 8000.0) - 10.0);
}

#[test]
fn water_drops_chirp_upward() {
    // One bubble per drop rises by base^ramp: the pitch late in a drop is
    // above its start. Read it from two windows after the first onset.
    let x = surface("water", json!({"intensity": 1, "seed": 4}));
    let floor = 0.05 * peak(&x);
    let t = x.iter().position(|v| v.abs() > floor).expect("a drop") as f32 / SR as f32;
    let at = |a: f32, b: f32| {
        let (i, j) = ((a * SR as f32) as usize, (b * SR as f32) as usize);
        peak_hz(&x[i..j.min(x.len())], SR)
    };
    println!("first drop at {t:.3} s");
    let (early, late) = (at(t, t + 0.02), at(t + 0.03, t + 0.08));
    println!("water drop pitch {early:.0} -> {late:.0} Hz");
    assert!(late > early * 1.1, "{early} {late}");
}

#[test]
fn a_loop_document_passes_the_default_checks() {
    for s in SURFACES {
        let mut d = SoundDoc::named("loop");
        d.nodes = rain(json!({"surface": s, "intensity": 12, "seed": 5}));
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
        let seam = res.iter().find(|r| r.name == "loop_seam").unwrap();
        println!("{s} loop: {}", seam.detail);
        assert!(res.iter().all(|r| r.pass), "{s}:\n{}", report(&res));
    }
}

#[test]
fn bad_fields_say_what_to_give() {
    for bad in [json!({"intensity": 0}), json!({"intensity": 900})] {
        let e = render_error(rain(bad));
        assert!(e.contains("give drops a second"), "{e}");
    }
    let e = render_error(rain(json!({"surface": "roof"})));
    assert!(e.contains("leaf") && e.contains("ground"), "{e}");
}
