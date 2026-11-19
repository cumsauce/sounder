//! `surf`: waves on a shore (after the foley project's shore bed;
//! docs/nodes/surf.md has the numbers these `println!`s give).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, describe, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn surf(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "surf", "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Mean energy over `w`-second windows, to read the wave envelope.
fn windows(x: &[f32], w: f32) -> Vec<f32> {
    let n = (w * 48000.0) as usize;
    x.chunks(n).map(rms).collect()
}

#[test]
fn both_shores_render_finite_and_follow_the_seed() {
    for s in ["sand", "shingle"] {
        let a = node_audio(surf(json!({"shore": s})), "n", 8.0);
        let b = node_audio(surf(json!({"shore": s, "seed": 2})), "n", 8.0);
        assert!(a.iter().all(|v| v.is_finite()), "{s}");
        assert!(rms(&a) > 1e-4, "{s}: silent");
        assert_ne!(a, b, "{s}: the seed changes nothing");
    }
}

#[test]
fn waves_come_every_period_and_a_storm_has_more() {
    let calm = node_audio(surf(json!({"period": 12})), "n", 30.0);
    let storm = node_audio(surf(json!({"period": 2})), "n", 30.0);
    // Count rises of the 100 ms envelope through a fraction of its peak.
    let count = |x: &[f32]| {
        // The wash is the floor (the median); a wave lifts 0.5 s windows 40 % over it.
        let mut e = windows(x, 0.5);
        let mut sorted = e.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let floor = sorted[sorted.len() / 2];
        e.push(0.0);
        let (mut over, mut n) = (false, 0);
        for v in e {
            if v > 1.4 * floor && !over {
                n += 1;
                over = true;
            }
            if v < 1.15 * floor {
                over = false;
            }
        }
        n
    };
    let (c, s) = (count(&calm), count(&storm));
    println!("surges in 30 s: calm {c}, storm {s}");
    assert!((2..=3).contains(&c), "a wave every 12 s: {c}");
    // Waves overlap in a storm sea: no gaps to count, but more energy.
    println!("rms calm {:.4} storm {:.4}", rms(&calm), rms(&storm));
    assert!(rms(&storm) > rms(&calm) * 1.2, "{s}");
}

#[test]
fn a_wave_breaks_after_its_rise_and_the_crash_is_the_loudest_part() {
    let x = node_audio(surf(json!({"period": 20})), "n", 7.0);
    let e = windows(&x, 0.25);
    let (peak_i, _) = e
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |a, (i, v)| if *v > a.1 { (i, *v) } else { a });
    let t = peak_i as f32 * 0.25;
    println!("loudest 250 ms window starts at {t} s");
    // It breaks 1.2-1.8 s in.
    assert!((0.75..=2.25).contains(&t), "crash at {t} s");
    assert!(e[0] < e[peak_i] * 0.7, "the face rises first");
    assert!(e[e.len() - 1] < e[peak_i] * 0.7, "the backwash has drained");
}

#[test]
fn shingle_knocks_in_the_backwash_and_sand_does_not() {
    let hi = |shore: &str| {
        let x = node_audio(
            surf(json!({"shore": shore, "period": 30, "seed": 3})),
            "n",
            7.0,
        );
        // 4-6 s: the backwash, past the foam's hiss.
        let w = &x[(4.0 * 48000.0) as usize..(6.5 * 48000.0) as usize];
        let d = describe(&mono_audio(w));
        d.band_shares[2] + d.band_shares[3]
    };
    let (sand, shingle) = (hi("sand"), hi("shingle"));
    println!("2 kHz+ share in the backwash: sand {sand:.3}, shingle {shingle:.3}");
    assert!(shingle > sand, "pebbles add highs: {sand} {shingle}");
}

#[test]
fn taller_waves_are_louder_and_far_ones_quieter() {
    let r = |extra: Value| rms(&node_audio(surf(extra), "n", 8.0));
    let (lap, swell) = (
        r(json!({"wave_height": 0.3})),
        r(json!({"wave_height": 2.5})),
    );
    assert!(swell > lap * 2.0, "{lap} {swell}");
    let (near, far) = (r(json!({"distance": 0})), r(json!({"distance": 1})));
    println!("rms near {near:.4} far {far:.4}");
    assert!(far < near * 0.6 && far > near * 0.02, "{near} {far}");
}

#[test]
fn a_loop_document_passes_the_default_checks() {
    for (shore, period) in [("sand", 4.0), ("shingle", 3.0)] {
        let mut d = SoundDoc::named("loop");
        d.nodes = surf(json!({"shore": shore, "period": period, "seed": 5}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(12.0);
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
        assert!(res.iter().all(|r| r.pass), "{shore}:\n{}", report(&res));
    }
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = render_error(surf(json!({"wave_height": 9})), "n", 0.1);
    assert!(e.contains("give metres"), "{e}");
    let e = render_error(surf(json!({"period": 0.2})), "n", 0.1);
    assert!(e.contains("give seconds"), "{e}");
    let e = render_error(surf(json!({"distance": 2})), "n", 0.1);
    assert!(e.contains("give 0"), "{e}");
}
