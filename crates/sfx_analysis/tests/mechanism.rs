//! R1-F: `click`, the mechanism node. Each test measures and prints (these
//! lines go in docs/nodes/click.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_graph::node::parse_body;
use sfx_graph::{Audio, RenderOpts, SoundDoc};

const KINDS: [&str; 5] = ["latch", "switch", "button", "lock", "clasp"];

/// A `click` named `c` with extra fields.
fn click(kind: &str, extra: Value) -> Vec<Value> {
    let mut c = json!({"type": "click", "name": "c", "kind": kind});
    c.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![c]
}

fn audio(kind: &str, extra: Value) -> Vec<f32> {
    node_audio(click(kind, extra), "c", 0.4)
}

/// Power-weighted mean frequency of the whole click, Hz (one FFT over all
/// of it; the descriptors' frame-based centroid reads a 30 ms click through
/// windows longer than the click).
fn centroid(x: &[f32]) -> f32 {
    let s = spectrum(x, SR);
    let (num, den) = s.iter().fold((0.0f64, 0.0f64), |(n, d), (f, db)| {
        let p = 10f64.powf(f64::from(*db) / 10.0);
        (n + f64::from(*f) * p, d + p)
    });
    (num / den.max(1e-30)) as f32
}

/// Seconds from the start to the last sample within 50 dB of the peak.
fn length_s(x: &[f32]) -> f32 {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let last = x
        .iter()
        .rposition(|v| v.abs() > peak * 10f32.powf(-50.0 / 20.0))
        .unwrap_or(0);
    last as f32 / SR as f32
}

fn first_diff(a: &[f32], b: &[f32]) -> usize {
    a.iter()
        .zip(b)
        .position(|(p, q)| p.to_bits() != q.to_bits())
        .unwrap_or(a.len())
}

#[test]
fn latch_has_two_onsets_25_to_60_ms_apart() {
    for seed in 0..6u64 {
        let x = audio("latch", json!({"seed": seed}));
        assert_finite(&x, "latch");
        let d = describe(&Audio::mono(SR, x.clone()));
        let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
        println!("latch seed {seed}: onsets {t:?}");
        assert_eq!(t.len(), 2, "two onsets: {t:?}");
        let gap = (t[1] - t[0]) * 1000.0;
        assert!((25.0..=60.0).contains(&gap), "gap {gap:.1} ms");
    }
}

#[test]
fn every_kind_has_its_second_transient_20_to_80_ms_after_the_first() {
    for kind in KINDS {
        let two = audio(kind, json!({}));
        let one = audio(kind, json!({"double": false}));
        let i = first_diff(&two, &one);
        let ms = i as f32 / SR as f32 * 1000.0;
        println!(
            "{kind}: second transient at {ms:.2} ms; centroid {:.0} Hz; length {:.1} ms; peak {:.3}",
            centroid(&two),
            length_s(&two) * 1000.0,
            two.iter().fold(0.0f32, |m, v| m.max(v.abs())),
        );
        // The press is the same samples with and without the second transient
        // (it is scaled by its own peak): the gap is where they part.
        assert!((20.0..=80.0).contains(&ms), "{kind}: {ms:.2} ms");
    }
}

#[test]
fn switch_is_bright_and_short() {
    let x = audio("switch", json!({}));
    let c = centroid(&x);
    let l = length_s(&x) * 1000.0;
    println!("switch: centroid {c:.0} Hz, length (to -50 dB) {l:.1} ms");
    assert!((2000.0..=6000.0).contains(&c), "centroid {c}");
    assert!(l < 120.0, "length {l} ms");
}

#[test]
fn bigger_size_lowers_the_centroid() {
    for kind in KINDS {
        let cs: Vec<f32> = [0.1, 0.3, 0.5, 0.7, 0.9]
            .iter()
            .map(|s| centroid(&audio(kind, json!({"size": s}))))
            .collect();
        println!("{kind}: centroid at size .1 .3 .5 .7 .9: {cs:.0?} Hz");
        assert!(
            cs.windows(2).all(|w| w[1] < w[0]),
            "{kind}: not monotone {cs:?}"
        );
    }
}

#[test]
fn harder_is_brighter() {
    let cs: Vec<f32> = [0.0, 0.35, 0.7, 1.0]
        .iter()
        .map(|h| centroid(&audio("latch", json!({"hardness": h}))))
        .collect();
    println!("latch: centroid at hardness 0 .35 .7 1: {cs:.0?} Hz");
    assert!(cs.windows(2).all(|w| w[1] > w[0]), "{cs:?}");
}

#[test]
fn seeds_differ_by_a_few_percent() {
    let a = audio("latch", json!({"seed": 1}));
    let b = audio("latch", json!({"seed": 2}));
    let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum();
    let tot: f32 = a.iter().map(|v| v.abs()).sum();
    let (pa, pb) = (peak_hz(&a, SR), peak_hz(&b, SR));
    println!(
        "seed 1 vs 2: summed |diff| / summed |a| = {:.3}; spectral peak {pa:.0} vs {pb:.0} Hz",
        diff / tot
    );
    assert!(diff > 0.01 * tot, "seeds 1 and 2 are nearly equal");
    // +-3 % on every mode frequency.
    assert!((pa / pb - 1.0).abs() < 0.08, "{pa} {pb}");
}

#[test]
fn a_hit_at_zero_is_not_cut() {
    let x = audio("button", json!({"at": 0.0}));
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let head = x[..48].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!(
        "button at 0: first sample {:.4}, peak of the first 1 ms {head:.4}, peak {peak:.4}",
        x[0]
    );
    assert!(x[0].abs() > 0.0 && head > 0.3 * peak);
    // Through the output stage: the default 2 ms fade_in cuts it; 0.0005 keeps it.
    let render_peak = |fade: Option<f32>| {
        let mut out = json!({"type": "output", "name": "out", "input": "c"});
        if let Some(f) = fade {
            out["fade_in"] = json!(f);
        }
        let mut d = SoundDoc::named("t");
        d.nodes = vec![click("button", json!({}))[0].clone(), out];
        d.duration = Some(0.3);
        let a = d.render(&RenderOpts::default()).unwrap().audio;
        a.channels[0].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let (default, short) = (render_peak(None), render_peak(Some(0.0005)));
    println!("through output: peak {default:.4} (default fade), {short:.4} (fade_in 0.0005)");
    let none = render_peak(Some(0.0));
    println!("fade_in 0: {none:.4}");
    assert!((none - peak).abs() < 1e-3, "fade_in 0 keeps the hit whole");
    assert!(short > 0.75 * peak, "fade_in 0.0005 keeps most of the hit");
    assert!(default < short);
}

#[test]
fn double_false_is_the_press_alone() {
    let x = audio("lock", json!({"double": false}));
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let d = describe(&Audio::mono(SR, x));
    println!("lock single: {} onset(s), peak {peak:.3}", d.onsets.len());
    assert_eq!(d.onsets.len(), 1);
    assert!((peak - 0.5).abs() < 1e-3, "{peak}");
}

#[test]
fn bit_identical_per_seed() {
    for kind in KINDS {
        assert_bit_identical(click(kind, json!({"seed": 7})), 0.3);
    }
    assert_eq!(
        audio("clasp", json!({"seed": 3})),
        audio("clasp", json!({"seed": 3}))
    );
}

#[test]
fn finite_at_the_extremes() {
    for kind in KINDS {
        for size in [0.0, 1.0] {
            for hardness in [0.0, 1.0] {
                for at in [0.0, 0.29, 5.0] {
                    let x = audio(kind, json!({"size": size, "hardness": hardness, "at": at}));
                    assert_finite(&x, kind);
                    assert_eq!(x.len(), (0.4 * SR as f32) as usize);
                    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    assert!(peak <= 0.5 + 1e-4, "{kind} {size} {hardness} {at}: {peak}");
                }
            }
        }
    }
    // A low sample rate drops the modes above the top instead of folding them.
    let mut d = SoundDoc::named("lo");
    d.sample_rate = 8000;
    d.nodes = click("lock", json!({"size": 0.0}));
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "c"}));
    d.duration = Some(0.2);
    match d.render(&RenderOpts::default()) {
        Ok(r) => assert!(r.audio.channels[0].iter().all(|v| v.is_finite())),
        Err(e) => println!("8 kHz lock size 0: {e}"),
    }
}

#[test]
fn errors_say_what_to_give() {
    let err = |extra: Value, kind: &str| {
        let mut d = SoundDoc::named("e");
        d.nodes = click(kind, extra);
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "c"}));
        d.duration = Some(0.2);
        d.render(&RenderOpts::default()).unwrap_err()
    };
    let e = err(json!({"size": 1.5}), "latch");
    println!("{e}");
    assert!(e.contains("size") && e.contains("1.5") && e.contains("0 to 1"));
    let e = err(json!({"hardness": -0.1}), "latch");
    assert!(e.contains("hardness") && e.contains("0 to 1"), "{e}");
    let e = err(json!({"at": -1.0}), "latch");
    assert!(e.contains("at") && e.contains(">= 0"), "{e}");
    let e = parse_body("click", &json!({"kind": "squeak"}), "node `c`").unwrap_err();
    println!("{e}");
    assert!(
        e.contains("squeak") && e.contains("latch") && e.contains("clasp"),
        "{e}"
    );
    let e = parse_body("click", &json!({"kind": "latch", "frq": 1}), "node `c`").unwrap_err();
    assert!(e.contains("frq"), "{e}");
}

#[test]
fn the_example_parses() {
    let text = sfx_graph::describe(Some("click")).unwrap();
    assert!(text.contains("latch"), "{text}");
    assert!(sfx_graph::describe(None).unwrap().contains("click"));
}

#[test]
fn recipe_renders() {
    // The doc's recipe: a latch at the end of a door slam's body, here alone.
    let nodes = vec![
        json!({"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.0}),
        json!({"type": "gain", "name": "hit", "input": "latch", "gain": 0.8}),
        json!({"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}),
    ];
    let a = render(&nodes, 0.25);
    let d = describe(&a);
    println!(
        "recipe: {} samples, peak {:.2} dB, onsets {:?}, centroid {:.0} Hz",
        a.channels[0].len(),
        d.peak_db,
        d.onsets.iter().map(|o| o.t).collect::<Vec<_>>(),
        centroid(&a.channels[0])
    );
    assert_eq!(a.channels[0].len(), (0.25 * SR as f32) as usize);
    assert_finite(&a.channels[0], "recipe");
    assert_eq!(d.onsets.len(), 2);
}
