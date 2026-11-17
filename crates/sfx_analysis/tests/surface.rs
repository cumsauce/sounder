//! M6: `rolling`, `cloth`, `footstep`. Each test measures and prints (these
//! lines go in docs/nodes/surface.md). The footstep presets are tested
//! against the round-0 briefs' numbers (design/rounds/briefs.json).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::stft::distance;
use sfx_analysis::{Descriptors, describe};
use sfx_graph::node::parse_body;
use sfx_graph::{Audio, RenderOpts, SoundDoc};

fn with(mut node: Value, extra: Value) -> Value {
    node.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    node
}

fn desc(x: &[f32]) -> Descriptors {
    describe(&Audio::mono(SR, x.to_vec()))
}

fn centroid(x: &[f32]) -> f32 {
    desc(x).centroid_all_hz
}

fn peak_db(x: &[f32]) -> f32 {
    20.0 * x.iter().fold(0f32, |a, b| a.max(b.abs())).max(1e-9).log10()
}

fn render_error(node: Value) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = vec![node];
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(0.5);
    d.render(&RenderOpts::default()).unwrap_err()
}

fn render_with_seed(nodes: &[Value], duration: f32, seed: u64) -> Vec<f32> {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes.to_vec();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "n"}));
    d.duration = Some(duration);
    let r = d
        .render(&RenderOpts {
            seed: Some(seed),
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"));
    r.audio.channels[0].clone()
}

// ------------------------------------------------------------------ rolling

fn rolling(extra: Value) -> Value {
    with(
        json!({"type": "rolling", "name": "n", "speed": 1.0, "seed": 1}),
        extra,
    )
}

#[test]
fn rolling_speed_zero_is_exact_silence() {
    let x = node_audio(vec![rolling(json!({"speed": 0.0}))], "n", 0.5);
    assert!(x.iter().all(|v| *v == 0.0), "speed 0 gives only zeros");
    // A ramp from 0 starts silent and gets louder.
    let nodes = vec![
        json!({"type": "ramp", "name": "v", "from": 0, "to": 2, "time": 1.0}),
        rolling(json!({"speed": "v"})),
    ];
    let y = node_audio(nodes, "n", 1.0);
    let (a, b) = (rms_db(&y[..9600]), rms_db(&y[38400..]));
    println!("speed 0 -> exact zeros; ramp 0 -> 2 m/s: rms first 0.2 s {a:.1} dB, last 0.2 s {b:.1} dB");
    assert!(b > a + 10.0);
}

#[test]
fn rolling_speed_raises_centroid_and_level() {
    let mut last: Option<(f32, f32)> = None;
    for v in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
        let x = node_audio(vec![rolling(json!({"speed": v}))], "n", 1.0);
        assert_finite(&x, "rolling");
        let (c, l) = (centroid(&x), rms_db(&x));
        println!("rolling speed {v} m/s: centroid {c:.0} Hz, rms {l:.1} dB");
        if let Some((pc, pl)) = last {
            assert!(c > pc * 1.7, "doubling speed at least x1.7 centroid: {pc} -> {c}");
            assert!(l > pl + 4.5, "doubling speed at least +4.5 dB: {pl} -> {l}");
        }
        last = Some((c, l));
    }
}

#[test]
fn rolling_roughness_raises_centroid_and_size_lowers_it() {
    let mut last = 0.0;
    for r in [0.0f32, 0.5, 1.0] {
        let c = centroid(&node_audio(vec![rolling(json!({"roughness": r}))], "n", 1.0));
        println!("rolling roughness {r}: centroid {c:.0} Hz");
        assert!(c > last * 1.5, "{last} -> {c}");
        last = c;
    }
    let mut last = f32::MAX;
    for s in [0.001f32, 0.002, 0.004, 0.01] {
        let x = node_audio(vec![rolling(json!({"size": s}))], "n", 1.0);
        let (c, l) = (centroid(&x), rms_db(&x));
        println!("rolling size {s} m: centroid {c:.0} Hz, rms {l:.1} dB");
        assert!(c < last * 0.65, "{last} -> {c}");
        // Size does not move the level.
        assert!(l > -18.0 && l < -10.0, "{l}");
        last = c;
    }
}

#[test]
fn rolling_seeds_differ_and_negative_speed_mirrors() {
    let a = node_audio(vec![rolling(json!({"seed": 1}))], "n", 0.5);
    let b = node_audio(vec![rolling(json!({"seed": 2}))], "n", 0.5);
    let d = distance(&a, &b, SR);
    println!("rolling seed 1 vs 2: STFT distance {d:.2} dB");
    assert!(d > 3.0, "{d}");
    let p = node_audio(vec![rolling(json!({"speed": 1.0}))], "n", 0.5);
    let m = node_audio(vec![rolling(json!({"speed": -1.0}))], "n", 0.5);
    assert!(p.iter().zip(&m).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn rolling_is_finite_at_the_extremes_and_bit_identical() {
    let mut worst = 0f32;
    for (speed, size, rough) in [
        (json!("huge"), 0.0001, 1.0),
        (json!("huge"), 10.0, 0.0),
        (json!(0.0), 0.0001, 1.0),
        (json!(1e-9), 10.0, 1.0),
        (json!(50.0), 0.01, 1.0),
    ] {
        let x = node_audio(
            vec![
                json!({"type": "ramp", "name": "huge", "from": 1e9, "to": -1e9, "time": 0.5}),
                rolling(json!({"speed": speed, "size": size, "roughness": rough})),
            ],
            "n",
            0.5,
        );
        assert_finite(&x, "rolling extreme");
        worst = worst.max(peak_db(&x));
    }
    println!("rolling extremes (a +-1e9 m/s sweep clamped, size 0.0001 and 10, roughness 0 and 1): finite, peak at most {worst:.1} dB");
    assert!(worst < 40.0);
    assert_bit_identical(vec![rolling(json!({"speed": 2.0, "roughness": 0.8}))], 0.5);
}

#[test]
fn rolling_errors_say_what_to_do() {
    let e = render_error(rolling(json!({"speed": 500.0, "size": 0.002})));
    println!("{e}");
    assert!(e.contains("speed: 500") && e.contains("under 50 m/s"), "{e}");
    let e = render_error(rolling(json!({"speed": 30.0, "size": 0.002})));
    println!("{e}");
    assert!(e.contains("speed: 30") && e.contains("bigger size") && e.contains("under 24.000"), "{e}");
    let e = render_error(rolling(json!({"roughness": 1.5})));
    assert!(e.contains("roughness: 1.5") && e.contains("0 to 1"), "{e}");
    let e = render_error(rolling(json!({"size": 0.0})));
    assert!(e.contains("size: 0") && e.contains("0.0001 to 10"), "{e}");
    let e = parse_body("rolling", &json!({"speed": 1, "colour": 1}), "node").unwrap_err();
    assert!(e.contains("colour"), "{e}");
}

#[test]
fn rolling_excites_modal() {
    // The recipe in the doc: roll over a wooden panel.
    let nodes = vec![
        json!({"type": "ramp", "name": "v", "from": 0.2, "to": 2.0, "time": 1.0}),
        json!({"type": "rolling", "name": "surf", "speed": "v", "size": 0.004, "seed": 3}),
        json!({"type": "modal", "name": "n", "excite": "surf", "material": "wood", "size": 0.45}),
    ];
    let x = node_audio(nodes, "n", 1.0);
    assert_finite(&x, "rolling into modal");
    let (a, b) = (rms_db(&x[..9600]), rms_db(&x[38400..]));
    println!("rolling -> modal wood: rms first 0.2 s {a:.1} dB, last 0.2 s {b:.1} dB, peak {:.1} dB", peak_db(&x));
    assert!(b > a + 6.0);
}

// -------------------------------------------------------------------- cloth

fn cloth(extra: Value) -> Value {
    with(
        json!({"type": "cloth", "name": "n", "motion": 1.0, "seed": 1}),
        extra,
    )
}

#[test]
fn cloth_motion_zero_is_exact_silence() {
    let x = node_audio(vec![cloth(json!({"motion": 0.0}))], "n", 1.0);
    assert!(x.iter().all(|v| *v == 0.0));
    println!("cloth motion 0: exact zeros");
}

#[test]
fn cloth_motion_raises_rate_band_and_level() {
    let mut last: Option<(f32, f32, f32)> = None;
    for m in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
        let x = node_audio(vec![cloth(json!({"motion": m}))], "n", 2.0);
        assert_finite(&x, "cloth");
        let d = desc(&x);
        let (c, l, rate) = (d.centroid_all_hz, rms_db(&x), d.density.per_s);
        println!(
            "cloth motion {m}: centroid {c:.0} Hz, rms {l:.1} dB, density {rate:.0} events/s"
        );
        if let Some((pc, pl, _)) = last {
            assert!(c > pc * 1.03, "centroid up: {pc} -> {c}");
            assert!(l > pl + 3.0, "level up: {pl} -> {l}");
        }
        last = Some((c, l, rate));
    }
    // The events/s measure counts distinct ticks, so it rises while bursts
    // are sparse (up to motion 1) and then saturates as they overlap.
    let rate = |m: f32| desc(&node_audio(vec![cloth(json!({"motion": m}))], "n", 2.0)).density.per_s;
    let (a, b, c) = (rate(0.25), rate(0.5), rate(1.0));
    assert!(a < b && b < c, "{a} {b} {c}");
}

#[test]
fn cloth_weight_lowers_centroid() {
    let mut last = f32::MAX;
    for w in [0.0f32, 0.5, 1.0] {
        let x = node_audio(vec![cloth(json!({"weight": w}))], "n", 2.0);
        let c = centroid(&x);
        println!("cloth weight {w}: centroid {c:.0} Hz, rms {:.1} dB", rms_db(&x));
        assert!(c < last * 0.85, "{last} -> {c}");
        last = c;
    }
}

#[test]
fn cloth_follows_a_motion_signal() {
    let nodes = vec![
        json!({"type": "ramp", "name": "m", "from": 0.0, "to": 2.0, "time": 2.0}),
        cloth(json!({"motion": "m"})),
    ];
    let x = node_audio(nodes, "n", 2.0);
    let n = x.len();
    let (a, b) = (&x[..n / 4], &x[3 * n / 4..]);
    let (la, lb) = (rms_db(a), rms_db(b));
    let (ca, cb) = (centroid(a), centroid(b));
    println!("cloth motion ramp 0 -> 2: first quarter rms {la:.1} dB centroid {ca:.0} Hz; last quarter {lb:.1} dB, {cb:.0} Hz");
    assert!(lb > la + 10.0 && cb > ca);
}

#[test]
fn cloth_seeds_differ_and_extremes_are_finite_within_budget() {
    let a = node_audio(vec![cloth(json!({"seed": 1}))], "n", 1.0);
    let b = node_audio(vec![cloth(json!({"seed": 2}))], "n", 1.0);
    let d = distance(&a, &b, SR);
    println!("cloth seed 1 vs 2: STFT distance {d:.2} dB");
    assert!(d > 3.0, "{d}");
    // A signal far over the cap is clamped to 4; NaN-free.
    let t = std::time::Instant::now();
    let x = node_audio(
        vec![
            json!({"type": "ramp", "name": "huge", "from": 1e9, "to": 3e9, "time": 30.0}),
            cloth(json!({"motion": "huge", "weight": 1.0})),
        ],
        "n",
        30.0,
    );
    let el = t.elapsed();
    assert_finite(&x, "cloth max motion 30 s");
    println!(
        "cloth motion clamped to 4, weight 1, 30 s: {el:.2?}, peak {:.1} dB",
        peak_db(&x)
    );
    assert!(el.as_secs_f32() < 20.0, "{el:?}");
    for w in [0.0f32, 1.0] {
        let x = node_audio(vec![cloth(json!({"motion": 4.0, "weight": w}))], "n", 1.0);
        assert_finite(&x, "cloth extreme");
    }
    assert_bit_identical(vec![cloth(json!({"motion": 1.5, "weight": 0.2}))], 1.0);
}

#[test]
fn cloth_errors_say_what_to_do() {
    let e = render_error(cloth(json!({"motion": 5.0})));
    println!("{e}");
    assert!(e.contains("motion: 5") && e.contains("0 to 4"), "{e}");
    let e = render_error(cloth(json!({"weight": -0.1})));
    assert!(e.contains("weight: -0.1") && e.contains("0 to 1"), "{e}");
}

// ----------------------------------------------------------------- footstep

fn step(surface: &str, extra: Value) -> Value {
    with(
        json!({"type": "footstep", "name": "n", "surface": surface, "seed": 1}),
        extra,
    )
}

/// The footstep as the human gets it: through the default output stage.
fn step_audio(surface: &str, extra: Value) -> Audio {
    render(vec![step(surface, extra)], 1.0)
}

fn audible_s(d: &Descriptors) -> f32 {
    d.seconds - d.silence_start_s - d.silence_end_s
}

#[test]
fn footstep_concrete_is_heel_then_toe() {
    let a = step_audio("concrete", json!({}));
    let d = describe(&a);
    let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
    println!(
        "concrete: onsets {:?} s, attack {:.1} ms, centroid {:.0} Hz, audible {:.2} s, peak {:.1} dB",
        t, d.attack_s * 1000.0, d.centroid_all_hz, audible_s(&d), d.peak_db
    );
    assert_eq!(t.len(), 2, "heel and toe: {t:?}");
    let gap = t[1] - t[0];
    assert!((0.060..=0.150).contains(&gap), "gap {gap}");
    assert!(d.attack_s < 0.010);
    assert!((800.0..=3000.0).contains(&d.centroid_all_hz), "{}", d.centroid_all_hz);
    assert!((0.15..=0.4).contains(&audible_s(&d)), "{}", audible_s(&d));
    assert!(d.pitch_hz.is_none(), "{:?}", d.pitch_hz);
}

#[test]
fn footstep_gravel_is_many_flat_ticks() {
    let a = step_audio("gravel", json!({}));
    let d = describe(&a);
    println!(
        "gravel: {} onsets, {} density events, flatness {:.3} (occupied band {:?}), centroid {:.0} Hz, audible {:.2} s",
        d.onsets.len(), d.density.count, d.flatness_all, d.flatness_occupied, d.centroid_all_hz, audible_s(&d)
    );
    assert!(d.onsets.len() >= 8, "{}", d.onsets.len());
    assert!(d.density.count >= 8);
    assert!((2000.0..=6000.0).contains(&d.centroid_all_hz), "{}", d.centroid_all_hz);
    assert!((0.25..=0.6).contains(&audible_s(&d)), "{}", audible_s(&d));
    assert!(d.flatness_occupied.unwrap_or(0.0) > 0.4);
}

#[test]
fn footstep_grass_is_a_soft_pitchless_swish() {
    let a = step_audio("grass", json!({}));
    let d = describe(&a);
    println!(
        "grass: attack {:.1} ms, pitch {:?}, centroid {:.0} Hz, audible {:.2} s, flatness {:.3}",
        d.attack_s * 1000.0, d.pitch_hz, d.centroid_all_hz, audible_s(&d), d.flatness_all
    );
    assert!((0.010..=0.040).contains(&d.attack_s), "{}", d.attack_s);
    assert!(d.pitch_hz.is_none(), "{:?}", d.pitch_hz);
    assert!((1500.0..=5000.0).contains(&d.centroid_all_hz), "{}", d.centroid_all_hz);
    assert!((0.25..=0.6).contains(&audible_s(&d)), "{}", audible_s(&d));
}

#[test]
fn footstep_wood_has_a_low_body_under_a_short_tick() {
    let raw = node_audio(vec![step("wood", json!({}))], "n", 1.0);
    let a = Audio::mono(SR, raw.clone());
    let d = describe(&a);
    let body = d
        .modes
        .iter()
        .find(|m| (80.0..=300.0).contains(&m.hz) && m.decay_s >= 0.1);
    let (hz, _) = peaks(&raw, SR, 1, 0.0)[0];
    let ms = |t: f32| (t * SR as f32) as usize;
    // The tick: 1.5-6 kHz in the first 15 ms vs the same band from 120 ms on
    // (after the toe's tick, which sits at ~0.1 s, has died).
    let tick = band_db(&raw[..ms(0.015)], SR, 1500.0, 6000.0);
    let tail = band_db(&raw[ms(0.13)..ms(0.3)], SR, 1500.0, 6000.0);
    let body_db = band_db(&raw[..ms(0.08)], SR, 80.0, 300.0);
    println!(
        "wood: body mode {:?}, strongest peak {hz:.0} Hz, audible {:.2} s; 1.5-6k first 15 ms {tick:.1} dB, from 130 ms {tail:.1} dB, body 80-300 Hz {body_db:.1} dB",
        body.map(|m| (m.hz.round(), m.decay_s)),
        audible_s(&d)
    );
    assert!(body.is_some(), "no 80-300 Hz mode ringing 0.1 s: {:?}", d.modes);
    assert!((80.0..=300.0).contains(&hz), "{hz}");
    assert!(tick > tail + 20.0, "the tick is short: {tick} vs {tail}");
    assert!(tick > body_db - 30.0, "the tick is there: {tick} vs {body_db}");
    assert!((0.2..=0.5).contains(&audible_s(&d)), "{}", audible_s(&d));
}

#[test]
fn footstep_weight_is_continuous_and_darkens() {
    for surface in ["concrete", "gravel", "grass", "wood"] {
        let mut last = (f32::MAX, f32::MIN);
        let mut line = String::new();
        for w in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let x = node_audio(vec![step(surface, json!({"weight": w}))], "n", 1.0);
            assert_finite(&x, surface);
            let (c, l) = (centroid(&x), rms_db(&x));
            line += &format!(" w{w}: {c:.0} Hz {l:.1} dB;");
            assert!(c < last.0, "{surface} weight {w}: centroid {c} !< {}", last.0);
            assert!(l > last.1, "{surface} weight {w}: level {l} !> {}", last.1);
            last = (c, l);
        }
        println!("{surface} weight sweep (centroid, rms):{line}");
    }
}

#[test]
fn footstep_pace_sets_the_gap_continuously() {
    let gap = |pace: Value| {
        let d = describe(&step_audio("concrete", json!({ "pace": pace })));
        assert_eq!(d.onsets.len(), 2, "{:?}", d.onsets);
        d.onsets[1].t - d.onsets[0].t
    };
    let mut last = f32::MAX;
    let mut line = String::new();
    for p in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let g = gap(json!(p));
        line += &format!(" pace {p}: {:.0} ms;", g * 1000.0);
        assert!(g < last, "pace {p}: gap {g} !< {last}");
        assert!((0.055..=0.155).contains(&g), "{g}");
        last = g;
    }
    println!("heel-toe gap:{line}");
    // The words are points on the same scale.
    for (word, num) in [("walk", 0.5), ("run", 0.9), ("sneak", 0.1)] {
        let a = node_audio(vec![step("grass", json!({"pace": word}))], "n", 0.7);
        let b = node_audio(vec![step("grass", json!({"pace": num}))], "n", 0.7);
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()), "{word}");
        println!("pace \"{word}\" is bit-identical to pace {num}");
    }
    let c = node_audio(vec![step("grass", json!({}))], "n", 0.7);
    let w = node_audio(vec![step("grass", json!({"pace": "walk"}))], "n", 0.7);
    assert!(c.iter().zip(&w).all(|(x, y)| x.to_bits() == y.to_bits()));
}

#[test]
fn footstep_surfaces_differ_in_flatness_and_onsets() {
    let g = describe(&step_audio("gravel", json!({})));
    let w = describe(&step_audio("wood", json!({})));
    println!(
        "gravel: flatness {:.3}, {} onsets; wood: flatness {:.3}, {} onsets",
        g.flatness_all, g.onsets.len(), w.flatness_all, w.onsets.len()
    );
    assert!(g.flatness_all > 3.0 * w.flatness_all);
    assert!(g.onsets.len() >= w.onsets.len() + 6);
}

#[test]
fn footstep_shoe_and_at() {
    let c = |shoe: &str| centroid(&node_audio(vec![step("concrete", json!({"shoe": shoe}))], "n", 1.0));
    let (hard, sneaker, boot, bare) = (c("hard"), c("sneaker"), c("boot"), c("bare"));
    println!("concrete centroid by shoe: hard {hard:.0}, sneaker {sneaker:.0}, boot {boot:.0}, bare {bare:.0} Hz");
    assert!(hard > boot && boot > sneaker && sneaker > bare);
    // `at` moves the heel.
    let x = node_audio(vec![step("concrete", json!({"at": 0.3}))], "n", 1.0);
    let first = x.iter().position(|v| v.abs() > 1e-4).unwrap();
    println!("at 0.3: first sample over 1e-4 at {:.1} ms", first as f32 / 48.0);
    assert!((first as f32 / 48.0 - 300.0).abs() < 3.0);
}

#[test]
fn footstep_seeds_make_different_steps() {
    for surface in ["concrete", "gravel", "grass", "wood"] {
        let steps: Vec<Vec<f32>> = (1..=6u64)
            .map(|s| render_with_seed(&[step(surface, json!({"seed": 0}))], 0.8, s))
            .collect();
        let mut dists = Vec::new();
        for i in 0..6 {
            for j in i + 1..6 {
                dists.push(distance(&steps[i], &steps[j], SR));
            }
        }
        let (lo, hi) = dists.iter().fold((f32::MAX, 0f32), |(l, h), d| (l.min(*d), h.max(*d)));
        let gaps: Vec<f32> = steps
            .iter()
            .map(|x| {
                let d = desc(x);
                if d.onsets.len() >= 2 { d.onsets[1].t - d.onsets[0].t } else { 0.0 }
            })
            .collect();
        let levels: Vec<f32> = steps.iter().map(|x| rms_db(x)).collect();
        let spread = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
        println!(
            "{surface} 6 doc seeds: STFT distance {lo:.2}..{hi:.2} dB; rms spread {:.2} dB; heel-toe gap spread {:.1} ms",
            spread(&levels),
            spread(&gaps) * 1000.0
        );
        assert!(lo > 1.0, "{surface}: two seeds must differ: {lo}");
        assert!(hi < 14.0, "{surface}: but stay one sound: {hi}");
        assert!(spread(&levels) > 0.5);
        // The node's own seed varies it the same way.
        let a = node_audio(vec![step(surface, json!({"seed": 1}))], "n", 0.8);
        let b = node_audio(vec![step(surface, json!({"seed": 2}))], "n", 0.8);
        let d = distance(&a, &b, SR);
        println!("{surface} node seed 1 vs 2: {d:.2} dB");
        assert!(d > 1.0 && d < 14.0);
    }
}

#[test]
fn footstep_is_finite_at_the_extremes_and_bit_identical() {
    let mut worst = f32::MIN;
    let mut at = String::new();
    for surface in ["concrete", "gravel", "grass", "wood"] {
        for (w, p) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)] {
            for shoe in ["hard", "bare"] {
                let x = node_audio(
                    vec![step(surface, json!({"weight": w, "pace": p, "shoe": shoe, "at": 0.5}))],
                    "n",
                    1.0,
                );
                assert_finite(&x, surface);
                let pk = peak_db(&x);
                if pk > worst {
                    worst = pk;
                    at = format!("{surface} weight {w} pace {p} shoe {shoe}");
                }
            }
        }
        // `at` past the end: silence, not an error.
        let x = node_audio(vec![step(surface, json!({"at": 5.0}))], "n", 0.5);
        assert!(x.iter().all(|v| *v == 0.0));
        assert_bit_identical(vec![step(surface, json!({"weight": 0.7, "pace": "run"}))], 0.8);
    }
    println!("footstep weight/pace/shoe extremes, 4 surfaces: finite, highest raw peak {worst:.1} dB ({at}); at 5 s past a 0.5 s sound: zeros");
    assert!(worst < 1.0, "{worst}");
}

#[test]
fn footstep_errors_say_what_to_do() {
    let e = parse_body("footstep", &json!({"surface": "lava"}), "node").unwrap_err();
    println!("{e}");
    assert!(e.contains("lava") && e.contains("concrete"), "{e}");
    let e = render_error(step("wood", json!({"pace": "jog"})));
    println!("{e}");
    assert!(e.contains("jog") && e.contains("walk, run, sneak"), "{e}");
    let e = render_error(step("wood", json!({"pace": 1.5})));
    assert!(e.contains("pace: 1.5") && e.contains("0 (sneak) to 1 (run)"), "{e}");
    let e = render_error(step("wood", json!({"weight": 2.0})));
    assert!(e.contains("weight: 2") && e.contains("0 (light) to 1 (heavy)"), "{e}");
    let e = render_error(step("wood", json!({"shoe": "clog"})));
    assert!(e.contains("clog") && e.contains("sneaker"), "{e}");
    let e = render_error(step("wood", json!({"at": -1.0})));
    assert!(e.contains("at: -1"), "{e}");
}

#[test]
fn footstep_one_liners_render_through_the_output_stage() {
    // The point of the node: four surfaces, one node each, no other nodes.
    for surface in ["concrete", "gravel", "grass", "wood"] {
        let a = render(vec![step(surface, json!({}))], 0.6);
        let d = describe(&a);
        println!("{surface}: peak {:.1} dBFS, {:.1} LUFS", d.peak_db, d.lufs);
        assert!(d.peak_db > -20.0 && d.peak_db < -3.0);
    }
}
