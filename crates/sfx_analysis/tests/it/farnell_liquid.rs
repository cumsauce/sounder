//! `stream` and `pour`, from Farnell's RUNNINGWATER/waterflow.pd and
//! POUREDLIQUIDS/pour1.pd: what each patch does, measured on the node
//! (docs/nodes/farnell_liquid.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_analysis::{CheckCx, Expect, report, run};
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn stream(extra: Value, secs: f32) -> Vec<f32> {
    node_audio(one("stream", extra), "n", secs)
}

fn pour(extra: Value, secs: f32) -> Vec<f32> {
    node_audio(one("pour", extra), "n", secs)
}

/// The spectral centroid of each of `parts` equal slices of `x`.
fn centroids(x: &[f32], parts: usize) -> Vec<f32> {
    let m = x.len() / parts;
    (0..parts)
        .map(|k| spectrum_centroid(&x[k * m..(k + 1) * m]))
        .collect()
}

#[test]
fn stream_renders_finite_at_the_length_asked_around_its_depth() {
    let x = stream(json!({"depth": 800, "seed": 1}), 2.0);
    assert_eq!(x.len(), 2 * SR as usize);
    assert_finite(&x, "stream");
    let level = rms_db(&x);
    println!("stream rms {level:.1} dB, peak {:.3}", peak(&x));
    assert!(level > -35.0 && peak(&x) < 0.95, "{level}");
    // The sine sits at depth + up to +-560 Hz: most of the energy is
    // between 200 and 1500 Hz and almost none above 3 kHz.
    let mid = band_db(&x, SR, 200.0, 1500.0);
    let hi = band_db(&x, SR, 3000.0, 20000.0);
    println!("stream 200-1500 Hz {mid:.1} dB, above 3 kHz {hi:.1} dB");
    assert!(mid > hi + 20.0);
}

#[test]
fn stream_depth_moves_the_pitch() {
    // Only the upward glides sound, so the line sits at depth plus part of
    // the 0.35 x range excursions.
    let low = stream(json!({"depth": 400, "range": 400, "seed": 2}), 2.0);
    let high = stream(json!({"depth": 1600, "range": 400, "seed": 2}), 2.0);
    let (a, b) = (peak_hz(&low, SR), peak_hz(&high, SR));
    println!("stream peak depth 400: {a:.0} Hz, depth 1600: {b:.0} Hz");
    assert!((400.0..=560.0).contains(&a), "{a}");
    assert!((1600.0..=1760.0).contains(&b), "{b}");
}

#[test]
fn stream_flow_sets_how_many_glides_sound() {
    let ons = |flow: f32| {
        let x = stream(
            json!({"flow": flow, "layers": 1, "range": 3000, "seed": 3}),
            3.0,
        );
        let a = describe(&mono_audio(&x));
        println!(
            "flow {flow}: rms {:.1} dB, fine onsets {}",
            rms_db(&x),
            a.onsets.len()
        );
        (rms_db(&x), a.onsets.len())
    };
    let (slow_l, _) = ons(0.2);
    let (fast_l, _) = ons(4.0);
    // More pitch jumps a second, more loud upward glides.
    assert!(fast_l > slow_l + 3.0, "{slow_l} {fast_l}");
}

#[test]
fn stream_seeds_differ_and_the_same_seed_repeats() {
    let a = stream(json!({"seed": 1}), 1.0);
    let b = stream(json!({"seed": 2}), 1.0);
    let c = stream(json!({"seed": 1}), 1.0);
    assert_eq!(fnv(&a), fnv(&c));
    assert_ne!(fnv(&a), fnv(&b));
    assert_bit_identical(one("stream", json!({"seed": 5})), 1.0);
}

#[test]
fn stream_layers_thicken_without_getting_louder() {
    let one_l = rms_db(&stream(json!({"layers": 1, "seed": 4}), 3.0));
    let four = rms_db(&stream(json!({"layers": 4, "seed": 4}), 3.0));
    println!("stream rms: 1 layer {one_l:.1} dB, 4 layers {four:.1} dB");
    assert!((one_l - four).abs() < 4.0);
}

#[test]
fn stream_rejects_bad_fields() {
    let e = render_error(one("stream", json!({"flow": 50})), "n", 0.2);
    assert!(e.contains("flow"), "{e}");
    let e = render_error(one("stream", json!({"layers": 0})), "n", 0.2);
    assert!(e.contains("layers"), "{e}");
    let e = render_error(one("stream", json!({"depth": 5})), "n", 0.2);
    assert!(e.contains("depth"), "{e}");
}

#[test]
fn a_stream_loop_wraps_without_a_seam() {
    // `loop_seam` also compares the spectra of the first and last 0.34 s,
    // which differ by chance in a thin random stream: the default two layers
    // and over pass it, one layer is held to the wrap itself below.
    for (what, fields) in [
        ("default", json!({})),
        (
            "loop:true flow 2",
            json!({"loop": true, "flow": 2, "seed": 3}),
        ),
        ("3 layers", json!({"layers": 3, "seed": 7})),
    ] {
        let mut d = SoundDoc::named("loop");
        d.nodes = one("stream", fields);
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(3.0);
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
        let text = report(&res);
        let seam = res.iter().find(|r| r.name == "loop_seam").unwrap();
        println!("stream {what}: {}", seam.detail);
        assert!(res.iter().all(|r| r.pass), "{what}:\n{text}");
    }
}

#[test]
fn a_stream_loop_continues_across_the_wrap_sample_by_sample() {
    // One layer, so the wrap is the whole story: the step from the last
    // sample to the first is an ordinary step of the signal.
    for seed in 1..=4 {
        let x = stream(json!({"layers": 1, "loop": true, "seed": seed}), 2.0);
        let mut steps: Vec<f32> = x.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        steps.sort_by(f32::total_cmp);
        let p99 = steps[steps.len() * 99 / 100].max(1e-6);
        let wrap = (x[0] - x[x.len() - 1]).abs();
        println!("stream seed {seed}: wrap step {:.2}x p99", wrap / p99);
        assert!(wrap <= 2.0 * p99, "seed {seed}");
        // The same buffer as a non-loop differs (the period is rounded).
        let plain = stream(json!({"layers": 1, "loop": false, "seed": seed}), 2.0);
        assert_ne!(fnv(&x), fnv(&plain));
    }
}

#[test]
fn pour_rises_in_pitch_as_it_fills() {
    let x = pour(json!({"fill_time": 6, "seed": 1}), 7.5);
    assert_eq!(x.len(), (7.5 * SR as f32) as usize);
    assert_finite(&x, "pour");
    let c = centroids(&x, 6);
    println!("pour centroid by 1.25 s: {c:.0?} Hz, peak {:.3}", peak(&x));
    // The first and last seconds of the fill, before the fade.
    assert!(c[4] > c[1] * 1.3, "{c:?}");
}

#[test]
fn pour_vessel_size_lowers_the_pitch() {
    // Centroid over the fill (before the fade).
    let over_fill = |vessel: f32| {
        let x = pour(json!({"fill_time": 4, "vessel": vessel, "seed": 2}), 5.0);
        spectrum_centroid(&x[..4 * SR as usize])
    };
    let (small, big) = (over_fill(0.5), over_fill(2.0));
    println!("pour centroid: vessel 0.5 {small:.0} Hz, vessel 2 {big:.0} Hz");
    assert!(small > big * 1.3, "{small} {big}");
}

#[test]
fn pour_fades_in_and_out_around_the_fill() {
    let x = pour(json!({"fill_time": 3, "seed": 3}), 6.0);
    let sec = |a: f32, b: f32| rms_db(&x[(a * SR as f32) as usize..(b * SR as f32) as usize]);
    let (start, mid, end, after) = (sec(0.0, 0.1), sec(1.5, 2.5), sec(3.6, 4.0), sec(5.5, 6.0));
    println!("pour rms: start {start:.1}, mid {mid:.1}, end {end:.1}, after {after:.1} dB");
    assert!(mid > start + 6.0, "faded in");
    assert!(
        mid > after + 20.0,
        "silent after the 1.2 s fade (and the 0.5 Hz low-pass)"
    );
}

#[test]
fn pour_bubbles_add_energy_and_zero_removes_them() {
    let none = pour(json!({"fill_time": 3, "bubbles": 0, "seed": 4}), 4.0);
    let some = pour(json!({"fill_time": 3, "bubbles": 2, "seed": 4}), 4.0);
    assert_finite(&none, "no bubbles");
    println!(
        "pour rms: bubbles 0 {:.1} dB, bubbles 2 {:.1} dB",
        rms_db(&none),
        rms_db(&some)
    );
    assert_ne!(fnv(&none), fnv(&some));
}

#[test]
fn pour_seeds_differ_and_repeat() {
    let a = pour(json!({"fill_time": 1, "seed": 1}), 2.0);
    let b = pour(json!({"fill_time": 1, "seed": 2}), 2.0);
    let c = pour(json!({"fill_time": 1, "seed": 1}), 2.0);
    assert_eq!(fnv(&a), fnv(&c));
    assert_ne!(fnv(&a), fnv(&b));
    assert_bit_identical(one("pour", json!({"fill_time": 1, "seed": 5})), 2.0);
}

#[test]
fn pour_rejects_bad_fields() {
    let e = render_error(one("pour", json!({"fill_time": 0})), "n", 0.2);
    assert!(e.contains("fill_time"), "{e}");
    let e = render_error(one("pour", json!({"vessel": 9})), "n", 0.2);
    assert!(e.contains("vessel"), "{e}");
}
