//! R2-D: `crumple`. Each test measures and prints (these lines go in
//! docs/nodes/crumple.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_analysis::measure::fine_onsets;
use sfx_graph::node::parse_body;
use sfx_graph::{Audio, RenderOpts, SoundDoc};

fn crumple(extra: Value) -> Value {
    let mut n = json!({"type": "crumple", "name": "n", "seed": 1});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    n
}

fn node(extra: Value, secs: f32) -> Vec<f32> {
    node_audio(vec![crumple(extra)], "n", secs)
}

fn centroid(x: &[f32]) -> f32 {
    describe(&Audio::mono(SR, x.to_vec())).centroid_all_hz
}

/// Fine onsets (spacing 5 ms) of the node's own samples, per second.
fn onsets_per_s(x: &[f32]) -> f32 {
    fine_onsets(&Audio::mono(SR, x.to_vec()), 5.0).len() as f32 / (x.len() as f32 / SR as f32)
}

fn render_error(node: Value) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = vec![node];
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(0.5);
    d.render(&RenderOpts::default()).unwrap_err()
}

#[test]
fn crumple_fine_onset_count_tracks_density() {
    for d in [20.0f32, 40.0, 80.0, 160.0] {
        // Three seeds, 2 s each: the mean of the counts per second.
        let rates: Vec<f32> = (1..=3)
            .map(|s| onsets_per_s(&node(json!({"density": d, "seed": s}), 2.0)))
            .collect();
        let mean = rates.iter().sum::<f32>() / 3.0;
        println!("crumple density {d}: fine onsets per s {rates:.1?}, mean {mean:.1}");
        assert!(
            (mean / d - 1.0).abs() <= 0.25,
            "density {d}: counted {mean}/s"
        );
        // Each seed alone is also within 25 %.
        for r in rates {
            assert!((r / d - 1.0).abs() <= 0.25, "density {d}: one seed {r}/s");
        }
    }
}

#[test]
fn crumple_has_no_saturation_to_400() {
    // Past 200/s the bursts are closer than any 1 ms-chunk detector splits
    // (it looks 3 ms back), so count with a 2 ms refractory time up to 200,
    // where the count must keep rising, and check that the level does not
    // pile up between 50 and 400.
    let mut last = 0.0f32;
    for d in [50.0f32, 100.0, 200.0] {
        let x = node(json!({"density": d}), 2.0);
        assert_finite(&x, "crumple");
        let n = fine_onsets(&Audio::mono(SR, x.clone()), 2.0).len() as f32 / 2.0;
        println!(
            "crumple density {d}: fine onsets (2 ms) per s {n:.1}, rms {:.1} dB",
            rms_db(&x)
        );
        assert!(n > last + 5.0, "{last} -> {n}");
        last = n;
    }
    let (a, b) = (
        rms_db(&node(json!({"density": 50}), 2.0)),
        rms_db(&node(json!({"density": 400}), 2.0)),
    );
    assert!((a - b).abs() < 6.0, "rms {a} dB at 50/s, {b} dB at 400/s");
}

/// Mean centroid over four seeds (a few snaps dominate one render).
fn mean_centroid(extra: Value) -> f32 {
    (1..=4)
        .map(|seed| {
            let mut e = extra.clone();
            e["seed"] = json!(seed);
            centroid(&node(e, 2.0))
        })
        .sum::<f32>()
        / 4.0
}

#[test]
fn crumple_hardness_raises_the_centroid() {
    let mut last = 0.0f32;
    for h in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let c = mean_centroid(json!({"hardness": h}));
        println!("crumple hardness {h}: centroid {c:.0} Hz (mean of 4 seeds)");
        assert!(c > last * 1.03, "hardness {h}: {last} -> {c}");
        last = c;
    }
}

#[test]
fn crumple_size_lowers_the_centroid() {
    let mut last = f32::MAX;
    for s in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let c = mean_centroid(json!({"size": s}));
        println!("crumple size {s}: centroid {c:.0} Hz (mean of 4 seeds)");
        assert!(c < last * 0.98, "size {s}: {last} -> {c}");
        last = c;
    }
}

#[test]
fn crumple_seeds_differ() {
    let a = node(json!({"seed": 1}), 1.0);
    let b = node(json!({"seed": 2}), 1.0);
    let same = a.iter().zip(&b).filter(|(p, q)| p == q).count();
    let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum::<f32>()
        / a.iter().map(|p| p.abs()).sum::<f32>();
    println!(
        "crumple seeds 1 vs 2: {same} equal samples of {}, relative difference {diff:.2}",
        a.len()
    );
    assert!(diff > 0.5, "{diff}");
    // The document seed changes it too.
    let d = |seed: u64| {
        let mut doc = SoundDoc::named("t");
        doc.nodes = vec![
            crumple(json!({})),
            json!({"type": "output", "name": "out", "input": "n"}),
        ];
        doc.duration = Some(0.5);
        doc.seed = seed;
        doc.render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap()
        .audio
        .channels[0]
            .clone()
    };
    assert_ne!(d(1), d(2));
}

#[test]
fn crumple_into_lufs_minus_20() {
    for seed in 1..=6u64 {
        let mut d = SoundDoc::named("test");
        d.nodes = vec![
            crumple(json!({"density": 40, "seed": seed})),
            json!({"type": "output", "name": "out", "input": "n", "lufs": -20}),
        ];
        d.duration = Some(1.0);
        let r = d
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap();
        let n = fine_onsets(&r.audio, 5.0).len();
        println!(
            "crumple 40/s seed {seed} into lufs -20: {n} fine onsets, {:.2} LUFS, peak {:.2} dBFS, warnings {:?}",
            r.lufs, r.peak_db, r.warnings
        );
        assert!(n >= 20, "seed {seed}: {n} onsets");
        assert!(
            !r.warnings.iter().any(|w| w.contains("lufs")),
            "seed {seed}: {:?}",
            r.warnings
        );
        assert!((r.lufs + 20.0).abs() <= 1.0, "seed {seed}: {}", r.lufs);
    }
}

#[test]
fn crumple_density_signal_and_silence() {
    // Density 0 is exact silence.
    assert!(node(json!({"density": 0.0}), 0.5).iter().all(|v| *v == 0.0));
    // A linear ramp 20 -> 180 puts about 60 bursts in the first second and
    // 140 in the second.
    let nodes = vec![
        json!({"type": "ramp", "name": "r", "from": 20, "to": 180, "time": 2.0, "curve": "linear"}),
        crumple(json!({"density": "r"})),
    ];
    let x = node_audio(nodes, "n", 2.0);
    let (a, b) = x.split_at(x.len() / 2);
    let (ra, rb) = (onsets_per_s(a), onsets_per_s(b));
    println!(
        "crumple density ramp 20 -> 180: first second {ra:.0}/s (60 expected), second {rb:.0}/s (140 expected)"
    );
    assert!((ra / 60.0 - 1.0).abs() <= 0.25 && (rb / 140.0 - 1.0).abs() <= 0.25);
}

#[test]
fn crumple_is_bit_identical() {
    for extra in [
        json!({}),
        json!({"density": 160, "size": 0.9, "hardness": 0.1}),
    ] {
        assert_bit_identical(vec![crumple(extra)], 0.5);
    }
}

#[test]
fn crumple_is_finite_at_the_extremes() {
    for density in [0.0f32, 0.001, 400.0] {
        for size in [0.0f32, 1.0] {
            for hardness in [0.0f32, 1.0] {
                let x = node(
                    json!({"density": density, "size": size, "hardness": hardness}),
                    0.5,
                );
                assert_finite(&x, "crumple");
                let peak = x.iter().fold(0f32, |a, b| a.max(b.abs()));
                println!(
                    "crumple density {density} size {size} hardness {hardness}: peak {peak:.2}, rms {:.1} dB",
                    rms_db(&x)
                );
                assert!(peak < 10.0, "peak {peak}");
            }
        }
    }
    // A wild density signal is clamped.
    let x = node_audio(
        vec![
            json!({"type": "osc", "name": "o", "freq": 3, "amp": 1e9}),
            crumple(json!({"density": "o"})),
        ],
        "n",
        0.5,
    );
    assert_finite(&x, "crumple with a wild density");
}

#[test]
fn crumple_errors_name_the_field() {
    for (extra, field) in [
        (json!({"density": 500}), "density: 500"),
        (json!({"density": -1}), "density: -1"),
        (json!({"size": 1.5}), "size: 1.5"),
        (json!({"hardness": -0.1}), "hardness: -0.1"),
    ] {
        let e = render_error(crumple(extra));
        println!("{e}");
        assert!(e.contains(field) && e.contains("give a number"), "{e}");
    }
    let e = parse_body("crumple", &json!({"dense": 3}), "node #0").err();
    println!("{e:?}");
    assert!(e.unwrap().contains("dense"));
}

#[test]
fn crumple_recipe_renders() {
    // The recipe in docs/nodes/crumple.md: a hand squeezing a sheet of foil.
    let nodes = vec![
        json!({"type": "wander", "name": "squeeze", "rate": 3, "range": [30, 140], "seed": 2}),
        json!({"type": "crumple", "name": "foil", "density": "squeeze", "size": 0.4, "hardness": 0.85, "seed": 3}),
        json!({"type": "output", "name": "out", "input": "foil", "lufs": -20}),
    ];
    let mut d = SoundDoc::named("recipe");
    d.nodes = nodes;
    d.duration = Some(1.5);
    let r = d
        .render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap();
    let n = fine_onsets(&r.audio, 5.0).len();
    println!(
        "recipe 1.5 s: {} samples, {n} fine onsets, {:.2} LUFS, peak {:.2} dBFS, warnings {:?}",
        r.audio.channels[0].len(),
        r.lufs,
        r.peak_db,
        r.warnings
    );
    assert_eq!(r.audio.channels[0].len(), 72_000);
    assert!(n > 40, "{n}");
}
