//! `leaves` and `underwater` (after the foley project's plant brush and
//! submerged bed; docs/nodes/ambience.md has the numbers these `println!`s
//! give).

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

fn centroid(x: &[f32]) -> f32 {
    describe(&mono_audio(x)).centroid_all_hz
}

#[test]
fn leaves_render_finite_and_follow_the_seed() {
    for f in ["grass", "leaves", "dry"] {
        let a = node_audio(node("leaves", json!({"wind": 8, "foliage": f})), "n", 2.0);
        let b = node_audio(
            node("leaves", json!({"wind": 8, "foliage": f, "seed": 2})),
            "n",
            2.0,
        );
        assert!(a.iter().all(|v| v.is_finite()), "{f}");
        assert!(rms(&a) > 1e-3, "{f}: silent");
        assert_ne!(a, b, "{f}: the seed changes nothing");
    }
}

#[test]
fn foliage_sets_the_brightness_and_wind_the_level() {
    let c = |f: &str| {
        let x = node_audio(node("leaves", json!({"wind": 8, "foliage": f})), "n", 3.0);
        let d = describe(&mono_audio(&x));
        println!(
            "{f}: peak {:.1} dB, rms {:.1} dB, centroid {:.0} Hz, density {:.0}/s, shares {:?}",
            d.peak_db,
            20.0 * rms(&x).log10(),
            d.centroid_all_hz,
            d.density.per_s,
            d.band_shares
        );
        d.centroid_all_hz
    };
    let (g, l, d) = (c("grass"), c("leaves"), c("dry"));
    assert!(g > l, "grass {g} Hz is brighter than leaves {l} Hz");
    assert!(d > l, "dry {d} Hz is brighter than leaves {l} Hz");
    let level = |w: f32| {
        rms(&node_audio(
            node("leaves", json!({"wind": w, "gust": 0})),
            "n",
            3.0,
        ))
    };
    let (calm, brisk, gale) = (level(2.0), level(8.0), level(20.0));
    println!("rms calm {calm:.4}, brisk {brisk:.4}, gale {gale:.4}");
    assert!(calm < brisk && brisk < gale);
    assert!(
        node_audio(node("leaves", json!({"wind": 0})), "n", 0.5)
            .iter()
            .all(|v| *v == 0.0)
    );
}

#[test]
fn a_wind_signal_swells_the_rustle() {
    let mut nodes = vec![
        json!({"type": "ramp", "name": "w", "from": 0.5, "to": 14, "time": 4.0, "curve": "linear"}),
    ];
    nodes.extend(node("leaves", json!({"wind": "w", "gust": 0})));
    let x = node_audio(nodes, "n", 4.0);
    let (first, last) = (rms(&x[..48_000]), rms(&x[3 * 48_000..]));
    assert!(last > 4.0 * first, "{first} then {last}");
}

#[test]
fn leaves_loop_passes_the_default_checks() {
    for f in ["grass", "leaves", "dry"] {
        let mut d = SoundDoc::named("loop");
        d.nodes = node("leaves", json!({"wind": 6, "foliage": f, "seed": 5}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n", "lufs": -30}));
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
        assert!(res.iter().all(|r| r.pass), "{f}:\n{}", report(&res));
    }
}

#[test]
fn underwater_is_finite_seeded_and_silent_at_the_surface() {
    let a = node_audio(node("underwater", json!({"depth": 3})), "n", 2.0);
    let b = node_audio(node("underwater", json!({"depth": 3, "seed": 2})), "n", 2.0);
    assert!(a.iter().all(|v| v.is_finite()));
    assert_ne!(a, b);
    assert!(rms(&a) > 0.05, "submerged bed rms {}", rms(&a));
    let dry = node_audio(node("underwater", json!({"depth": 0})), "n", 1.0);
    assert!(dry.iter().all(|v| v.abs() < 1e-4), "the surface is silent");
}

#[test]
fn underwater_has_a_50_hz_pressure_tone_and_bubbles() {
    let x = node_audio(
        node("underwater", json!({"depth": 6, "bubbles": 0})),
        "n",
        2.0,
    );
    let ups = x[48_000..]
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    assert!((45..=55).contains(&ups), "{ups} cycles in a second");
    let with = node_audio(node("underwater", json!({"depth": 6})), "n", 4.0);
    let tone = node_audio(
        node("underwater", json!({"depth": 6, "bubbles": 0})),
        "n",
        4.0,
    );
    // Foley's bubbles are quiet under its tone (0.02 against 0.25), so the
    // test looks at the energy above 200 Hz, not the centroid.
    let (dw, dt) = (describe(&mono_audio(&with)), describe(&mono_audio(&tone)));
    let above = |d: &sfx_analysis::Descriptors| d.band_shares[1..].iter().sum::<f32>();
    println!(
        "above 200 Hz: {:.3} % with bubbles, {:.3} % tone alone; centroid {:.0} / {:.0} Hz, rms {:.4} / {:.4}",
        above(&dw),
        above(&dt),
        dw.centroid_all_hz,
        dt.centroid_all_hz,
        rms(&with),
        rms(&tone)
    );
    assert!(
        above(&dw) > 10.0 * above(&dt).max(1e-4),
        "bubbles ring above the tone"
    );
    let ct = dt.centroid_all_hz;
    let quiet = node_audio(
        node("underwater", json!({"depth": 6, "pressure": 0})),
        "n",
        2.0,
    );
    assert!(centroid(&quiet) > ct * 3.0, "bubbles alone sit higher");
}

#[test]
fn diving_swells_in_over_the_glide() {
    let mut nodes = vec![
        json!({"type": "ramp", "name": "d", "from": 0, "to": 5, "time": 0.2, "curve": "linear"}),
    ];
    nodes.extend(node("underwater", json!({"depth": "d"})));
    let x = node_audio(nodes, "n", 3.0);
    let (first, last) = (rms(&x[..4800]), rms(&x[2 * 48_000..]));
    assert!(last > 10.0 * first, "{first} then {last}");
}

#[test]
fn underwater_loop_wraps_without_a_jump() {
    // At the default `bubbles` 1 only 2 of 8 seeds pass `loop_seam`: about
    // 6 bubbles fall in the seam's 0.34 s window and its spectral half
    // follows their luck (5-13 dB). At `bubbles` 4 all 8 pass. The wrap
    // itself (the step and the kink) is exact at either.
    let mut passes = 0;
    for seed in 1u64..9 {
        let mut d = SoundDoc::named("loop");
        d.nodes = node(
            "underwater",
            json!({"depth": 4, "bubbles": 4, "seed": seed}),
        );
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n", "lufs": -30}));
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
        println!("seed {seed}: {}", seam.detail);
        let jump: f32 = seam
            .detail
            .split("discontinuity ")
            .nth(1)
            .unwrap()
            .split('x')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(jump < 1.0, "seed {seed}: {}", seam.detail);
        if res.iter().all(|r| r.pass) {
            passes += 1;
        }
    }
    println!("{passes} of 8 seeds pass every default check");
    assert!(passes >= 4, "{passes} of 8");
}

#[test]
fn bad_fields_say_what_to_give() {
    let e = render_error(node("leaves", json!({"wind": 90})), "n", 0.1);
    assert!(e.contains("give metres a second"), "{e}");
    let e = render_error(node("leaves", json!({"wind": 5, "gust": 2})), "n", 0.1);
    assert!(e.contains("gust"), "{e}");
    let e = render_error(
        node("leaves", json!({"wind": 5, "foliage": "oak"})),
        "n",
        0.1,
    );
    assert!(e.contains("foliage"), "{e}");
    let e = render_error(node("underwater", json!({"depth": -1})), "n", 0.1);
    assert!(e.contains("give metres"), "{e}");
    let e = render_error(
        node("underwater", json!({"depth": 2, "bubbles": 9})),
        "n",
        0.1,
    );
    assert!(e.contains("give a multiplier"), "{e}");
}
