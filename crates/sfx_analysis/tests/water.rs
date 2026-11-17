//! `water` and `splash` (M4): each field moves its measure at three
//! settings; level at `lufs -20` raises no missed-target warning; a
//! `kind: loop` water passes the default checks including `loop_seam`;
//! 30 s at flow 1 renders inside a stated time budget; bit-identical,
//! finite at the extremes, error messages.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use std::time::Instant;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, report, run};
use sfx_graph::node::parse_body;
use sfx_graph::{Kind, RenderOpts, SoundDoc};

fn water(extra: Value) -> Vec<Value> {
    let mut w = json!({"type": "water", "name": "w", "flow": 0.5, "seed": 3});
    w.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![w]
}

fn splash(extra: Value) -> Vec<Value> {
    let mut w = json!({"type": "splash", "name": "s", "size": 0.1, "seed": 3});
    w.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![w]
}

/// Spectral centroid, Hz, over 100 Hz .. 16 kHz (power-weighted).
fn centroid(x: &[f32]) -> f32 {
    let s = spectrum(x, SR);
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (f, d) in s {
        if (100.0..16_000.0).contains(&f) {
            let p = 10f64.powf(f64::from(d) / 10.0);
            num += f64::from(f) * p;
            den += p;
        }
    }
    (num / den.max(1e-30)) as f32
}

/// Share of the power under `hz`, 0..1.
fn share_below(x: &[f32], hz: f32) -> f32 {
    let lo = 10f32.powf(band_db(x, SR, 0.0, hz) / 10.0);
    let all = 10f32.powf(band_db(x, SR, 0.0, SR as f32 / 2.0) / 10.0);
    lo / all
}

/// Rises in a 1 ms rectified envelope: points where the envelope exceeds
/// 3x its own level 5 ms earlier and 0.01 absolute, re-armed once it falls
/// under 1.5x. Counts bubble births that stand out of the others.
fn rises(x: &[f32]) -> usize {
    let w = (SR / 1000) as usize;
    let env: Vec<f32> = x
        .chunks(w)
        .map(|c| c.iter().map(|v| v.abs()).sum::<f32>() / c.len() as f32)
        .collect();
    let (mut count, mut armed) = (0, true);
    for i in 5..env.len() {
        if armed && env[i] > 3.0 * env[i - 5] && env[i] > 0.01 {
            count += 1;
            armed = false;
        } else if env[i] < 1.5 * env[i - 5] {
            armed = true;
        }
    }
    count
}

/// Largest LUFS spread over 8 seeds of one setting, dB.
const SPREAD: f32 = 1.5;

/// Seconds from the start until a 10 ms RMS window stays under -50 dB
/// relative to the loudest one for good.
fn decay_s(x: &[f32]) -> f32 {
    let w = (SR / 100) as usize;
    let rms: Vec<f32> = x
        .chunks(w)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let top = rms.iter().cloned().fold(0.0f32, f32::max);
    let last = rms.iter().rposition(|v| *v > top * 0.00316).unwrap_or(0);
    (last + 1) as f32 * 0.01
}

fn render_doc(
    nodes: &[Value],
    duration: f32,
    lufs: Option<f32>,
    kind: Kind,
) -> sfx_graph::Rendered {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes.to_vec();
    d.kind = kind;
    d.duration = Some(duration);
    let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    let mut out = json!({"type": "output", "name": "out", "input": last});
    if let Some(l) = lufs {
        out["lufs"] = json!(l);
    }
    d.nodes.push(out);
    d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    let last = d.nodes.last().unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": last}));
    d.duration = Some(0.5);
    d.render(&RenderOpts::default()).unwrap_err()
}

#[test]
fn flow_moves_onset_density_lufs_and_centroid() {
    let mut dens = Vec::new();
    let mut lufs = Vec::new();
    let mut cent = Vec::new();
    for flow in [0.1, 0.35, 0.8] {
        // density 60 keeps the bubbles apart enough to count their onsets.
        let nodes = water(json!({"flow": flow, "density": 60}));
        let r = render_doc(&nodes, 8.0, None, Kind::Oneshot);
        let x = node_audio(&nodes, "w", 8.0);
        assert_finite(&x, "w");
        dens.push(rises(&x) as f32 / 8.0);
        lufs.push(r.lufs);
        cent.push(centroid(&x));
    }
    println!("flow 0.1 / 0.35 / 0.8: rises/s {dens:.1?}, LUFS {lufs:.1?}, centroid Hz {cent:.0?}");
    assert!(dens[0] < dens[1] && dens[1] < dens[2], "{dens:?}");
    assert!(
        lufs[0] + 3.0 < lufs[1] && lufs[1] + 3.0 < lufs[2],
        "{lufs:?}"
    );
    assert!(
        cent[0] > cent[1] * 1.1 && cent[1] > cent[2] * 1.1,
        "{cent:?}"
    );
    // The default density at the same three flows: louder, still lower.
    let (mut l2, mut c2) = (Vec::new(), Vec::new());
    for flow in [0.1, 0.35, 0.8] {
        let nodes = water(json!({"flow": flow}));
        l2.push(render_doc(&nodes, 8.0, None, Kind::Oneshot).lufs);
        c2.push(centroid(&node_audio(&nodes, "w", 8.0)));
    }
    println!("density 300: LUFS {l2:.1?}, centroid Hz {c2:.0?}");
    assert!(l2[0] + 3.0 < l2[1] && l2[1] + 3.0 < l2[2], "{l2:?}");
    assert!(c2[0] > c2[1] * 1.1 && c2[1] > c2[2] * 1.1, "{c2:?}");
}

#[test]
fn flow_zero_is_silence_and_a_ramp_is_followed() {
    let x = node_audio(water(json!({"flow": 0.0})), "w", 1.0);
    assert!(x.iter().all(|v| *v == 0.0), "flow 0 is silence");
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": 0.0, "to": 1.0, "time": 8.0}),
        json!({"type": "water", "name": "w", "flow": "f", "seed": 3}),
    ];
    let x = node_audio(&nodes, "w", 8.0);
    let q = x.len() / 4;
    let levels: Vec<f32> = (0..4).map(|i| rms_db(&x[i * q..(i + 1) * q])).collect();
    println!("ramp 0 -> 1 over 8 s, rms per quarter: {levels:.1?} dB");
    assert!(levels.windows(2).all(|w| w[0] < w[1]), "{levels:?}");
}

#[test]
fn depth_moves_the_centroid_down() {
    let c: Vec<f32> = [0.0, 0.5, 1.0]
        .iter()
        .map(|d| centroid(&node_audio(water(json!({"depth": d})), "w", 6.0)))
        .collect();
    println!("depth 0 / 0.5 / 1: centroid {c:.0?} Hz");
    assert!(c[0] > c[1] * 1.1 && c[1] > c[2] * 1.1, "{c:?}");
}

#[test]
fn splash_amount_adds_energy_and_peaks() {
    let mut peak = Vec::new();
    let mut lufs = Vec::new();
    for s in [0.0, 0.5, 1.0] {
        let nodes = water(json!({"splash": s, "flow": 0.7}));
        let x = node_audio(&nodes, "w", 8.0);
        peak.push(x.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        lufs.push(render_doc(&nodes, 8.0, None, Kind::Oneshot).lufs);
    }
    println!("splash 0 / 0.5 / 1: peak {peak:.3?}, LUFS {lufs:.1?}");
    assert!(peak[0] < peak[1] && peak[1] < peak[2], "{peak:?}");
    assert!(lufs[0] < lufs[1] && lufs[1] < lufs[2], "{lufs:?}");
}

#[test]
fn splash_size_moves_centroid_decay_and_low_band() {
    let (mut cent, mut dec, mut low) = (Vec::new(), Vec::new(), Vec::new());
    for size in [0.02, 0.1, 0.5] {
        let x = node_audio(splash(json!({"size": size})), "s", 3.0);
        assert_finite(&x, "s");
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "size {size}: peak {peak}");
        cent.push(centroid(&x));
        dec.push(decay_s(&x));
        low.push(share_below(&x, 200.0));
    }
    println!(
        "splash size 0.02 / 0.1 / 0.5: centroid {cent:.0?} Hz, decay {dec:.2?} s, share under 200 Hz {low:.4?}"
    );
    assert!(
        cent[0] > cent[1] * 1.1 && cent[1] > cent[2] * 5.0,
        "{cent:?}"
    );
    assert!(dec[0] < dec[1] && dec[1] < dec[2], "{dec:?}");
    assert!(low[0] < low[1] && low[1] < low[2], "{low:?}");
}

#[test]
fn splash_at_delays_and_seeds_differ() {
    let x = node_audio(splash(json!({"at": 0.5})), "s", 2.0);
    let first = x.iter().position(|v| v.abs() > 1e-6).unwrap();
    println!(
        "splash at 0.5: first nonzero sample {first} ({:.4} s)",
        first as f32 / SR as f32
    );
    assert!(first >= (0.5 * SR as f32) as usize && first < (0.52 * SR as f32) as usize);
    let a = node_audio(splash(json!({"seed": 1})), "s", 2.0);
    let b = node_audio(splash(json!({"seed": 2})), "s", 2.0);
    let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).powi(2)).sum::<f32>()
        / a.iter().map(|p| p * p).sum::<f32>();
    println!("splash seeds 1 vs 2: relative difference energy {diff:.2}");
    assert!(diff > 0.2, "{diff}");
    // Longer than the document: cut with no click at the end.
    let x = node_audio(splash(json!({"size": 0.5})), "s", 0.1);
    assert!(x.last().unwrap().abs() < 1e-3);
}

#[test]
fn typical_settings_reach_lufs_minus_20_without_a_warning() {
    for (flow, density, splash) in [
        (0.3, 300.0, 0.0),
        (0.6, 300.0, 0.2),
        (1.0, 300.0, 0.0),
        (0.6, 40.0, 0.0),
        (1.0, 3000.0, 0.0),
    ] {
        let nodes = water(json!({"flow": flow, "density": density, "splash": splash}));
        let r = render_doc(&nodes, 6.0, Some(-20.0), Kind::Oneshot);
        println!(
            "flow {flow} density {density} splash {splash}: LUFS {:.2}, peak {:.2} dBFS, warnings {:?}",
            r.lufs, r.peak_db, r.warnings
        );
        assert!(
            r.warnings.iter().all(|w| !w.contains("target")),
            "{:?}",
            r.warnings
        );
        assert!((r.lufs + 20.0).abs() < 1.0, "{}", r.lufs);
    }
    // A sparse trickle is peaky: print where -20 stops being reachable.
    for flow in [0.05, 0.1, 0.2] {
        let r = render_doc(
            &water(json!({"flow": flow})),
            6.0,
            Some(-20.0),
            Kind::Oneshot,
        );
        println!(
            "sparse: flow {flow} at -20: LUFS {:.2}, warnings {}",
            r.lufs,
            r.warnings.len()
        );
    }
    // A lone splash is a transient (crest 19..23 dB measured, sizes 0.01..1):
    // -20 LUFS is out of reach under the -1 dBFS ceiling, -24 is not.
    for size in [0.02, 0.1, 0.5] {
        let r = render_doc(
            &splash(json!({"size": size})),
            3.0,
            Some(-24.0),
            Kind::Oneshot,
        );
        println!(
            "splash {size} at -24: LUFS {:.2}, peak {:.2}, warnings {:?}",
            r.lufs, r.peak_db, r.warnings
        );
        assert!(
            r.warnings.iter().all(|w| !w.contains("target")),
            "{:?}",
            r.warnings
        );
        assert!((r.lufs + 24.0).abs() < 1.0, "{}", r.lufs);
    }
    let r = render_doc(
        &splash(json!({"size": 0.1})),
        3.0,
        Some(-20.0),
        Kind::Oneshot,
    );
    println!(
        "splash 0.1 at -20 (misses): LUFS {:.2}, warnings {:?}",
        r.lufs, r.warnings
    );
    assert!(r.lufs < -20.5 && r.lufs > -23.0, "{}", r.lufs);
}

#[test]
fn level_does_not_depend_on_a_lucky_big_bubble() {
    // The round-0 complaint: with bubble's r^1.5 law a few large bubbles set
    // the level. Here the same stream under 8 seeds, and a sparse one.
    for (flow, density) in [(0.5, 300.0), (0.5, 40.0)] {
        let l: Vec<f32> = (1..=8)
            .map(|seed| {
                let nodes = water(json!({"flow": flow, "density": density, "seed": seed}));
                render_doc(&nodes, 6.0, None, Kind::Oneshot).lufs
            })
            .collect();
        let (lo, hi) = (
            l.iter().cloned().fold(f32::MAX, f32::min),
            l.iter().cloned().fold(f32::MIN, f32::max),
        );
        println!(
            "flow {flow} density {density}, seeds 1..8: LUFS {lo:.2} .. {hi:.2} (spread {:.2} dB)",
            hi - lo
        );
        assert!(hi - lo < SPREAD, "{l:?}");
    }
}

#[test]
fn a_loop_passes_the_default_checks_including_loop_seam() {
    for (flow, splash) in [(0.5, 0.0), (0.9, 0.5), (0.15, 0.0)] {
        let nodes = water(json!({"flow": flow, "splash": splash}));
        let mut d = SoundDoc::named("loop");
        d.nodes = nodes;
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "w", "lufs": -20}));
        d.kind = Kind::Loop;
        d.duration = Some(4.0);
        let opts = RenderOpts {
            cached: false,
            ..RenderOpts::default()
        };
        let rendered = d.render(&opts).unwrap();
        let checks = run(
            &Expect::defaults_for(&d),
            &CheckCx {
                doc: &d,
                rendered: &rendered,
                opts: &opts,
            },
        )
        .unwrap();
        let text = report(&checks);
        println!("loop flow {flow} splash {splash}:\n{text}");
        assert!(text.contains("loop_seam"), "the seam check ran");
        assert!(checks.iter().all(|c| c.pass), "{text}");
    }
    // The wrap: with loop on, the last 100 ms continue into the first 100
    // ms (bubble tails wrap); the sample step across the seam is ordinary.
    let nodes = water(json!({"flow": 0.5, "loop": true}));
    let x = node_audio(&nodes, "w", 2.0);
    let step = (x[0] - x[x.len() - 1]).abs();
    let typical = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32;
    println!("loop true: seam step {step:.5}, mean step {typical:.5}");
    assert!(step < 8.0 * typical);
    // Explicit loop: true / false only change the result by the wrap.
    let off = node_audio(water(json!({"flow": 0.5, "loop": false})), "w", 2.0);
    assert!(off != x);
}

#[test]
fn thirty_seconds_at_max_flow_renders_inside_the_budget() {
    // Budget: 10 s on the test profile (optimised sfx_graph). Measured
    // value printed below.
    let nodes = water(json!({"flow": 1.0, "splash": 0.5}));
    let t = Instant::now();
    let x = node_audio(&nodes, "w", 30.0);
    let took = t.elapsed().as_secs_f32();
    assert_finite(&x, "w");
    let dens = node_audio(water(json!({"flow": 1.0, "density": 3000})), "w", 30.0);
    let took_max = t.elapsed().as_secs_f32() - took;
    assert_finite(&dens, "w");
    println!(
        "30 s, flow 1, default density + splash 0.5: {took:.2} s; density 3000: {took_max:.2} s"
    );
    assert!(took < 10.0 && took_max < 10.0);
}

#[test]
fn water_and_splash_are_bit_identical() {
    assert_bit_identical(water(json!({"flow": 0.7, "splash": 0.4})), 2.0);
    assert_bit_identical(
        water(json!({"flow": 0.7, "loop": true, "splash": 0.4})),
        2.0,
    );
    assert_bit_identical(splash(json!({"size": 0.3, "at": 0.2})), 2.0);
}

#[test]
fn extremes_are_finite() {
    for flow in [0.0, 1.0] {
        for depth in [0.0, 1.0] {
            for density in [0.0, 3000.0] {
                let x = node_audio(
                    water(json!({"flow": flow, "depth": depth, "density": density, "splash": 1.0})),
                    "w",
                    1.0,
                );
                assert_finite(&x, "w");
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(
                    peak < 20.0,
                    "flow {flow} depth {depth} density {density}: {peak}"
                );
            }
        }
    }
    // A flow signal far out of range is clamped.
    let nodes = vec![
        json!({"type": "ramp", "name": "f", "from": -1.0e9, "to": 1.0e9, "time": 1.0}),
        json!({"type": "water", "name": "w", "flow": "f", "seed": 1}),
    ];
    assert_finite(&node_audio(&nodes, "w", 1.0), "ramp flow");
    for size in [0.005, 1.0] {
        let x = node_audio(splash(json!({"size": size})), "s", 3.0);
        assert_finite(&x, "s");
    }
    // A very short document and a low sample rate.
    assert_finite(
        &node_audio(water(json!({"flow": 1.0, "loop": true})), "w", 0.001),
        "short loop",
    );
    let mut d = SoundDoc::named("low");
    d.sample_rate = 8000;
    d.nodes = vec![
        json!({"type": "water", "name": "w", "flow": 1.0, "splash": 1.0, "depth": 0.0}),
        json!({"type": "splash", "name": "s", "size": 0.005}),
        json!({"type": "mix", "name": "m", "inputs": ["w", "s"]}),
        json!({"type": "output", "name": "out", "input": "m"}),
    ];
    d.duration = Some(1.0);
    let a = d.render(&RenderOpts::default()).unwrap().audio;
    assert!(a.channels[0].iter().all(|v| v.is_finite()));
    println!(
        "extremes: flow 0/1 x depth 0/1 x density 0/3000 at splash 1, +-1e9 flow ramp, splash 0.005 and 1, 1 ms loop, 8 kHz: all finite"
    );
}

#[test]
fn errors_say_the_field_value_and_fix() {
    let e = render_error(water(json!({"flow": 2})));
    println!("{e}");
    assert!(e.contains("flow: 2: give a number from 0 to 1"), "{e}");
    let e = render_error(water(json!({"density": 5000})));
    println!("{e}");
    assert!(
        e.contains("density: 5000: give bubbles a second from 0 to 3000"),
        "{e}"
    );
    let e = render_error(water(json!({"depth": 1.5})));
    println!("{e}");
    assert!(e.contains("depth: 1.5: give a number from 0 to 1"), "{e}");
    let e = render_error(water(json!({"splash": -1})));
    assert!(e.contains("splash: -1: give a number from 0 to 1"), "{e}");
    let e = render_error(water(json!({"flwo": 1})));
    assert!(e.contains("flwo") && e.contains("flow"), "{e}");
    let e = render_error(splash(json!({"size": 5})));
    println!("{e}");
    assert!(
        e.contains("size: 5: give the splash radius in metres from 0.005 to 1"),
        "{e}"
    );
    let e = render_error(splash(json!({"size": 0})));
    println!("{e}");
    assert!(e.contains("size"), "{e}");
    let e = render_error(splash(json!({"at": -1})));
    println!("{e}");
    assert!(e.contains("at"), "{e}");
    // The work cap: a very long, dense, splashy document.
    let mut d = SoundDoc::named("cap");
    d.nodes = vec![
        json!({"type": "water", "name": "w", "flow": 1.0, "density": 3000, "splash": 1.0}),
        json!({"type": "output", "name": "out", "input": "w"}),
    ];
    d.duration = Some(600.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("{e}");
    assert!(
        e.contains("limit 800000") && e.contains("lower density"),
        "{e}"
    );
}

#[test]
fn registered_with_the_right_flags() {
    for (ty, causal) in [("water", true), ("splash", false)] {
        let mut body = sfx_graph::example(ty).expect("registered");
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, "x").unwrap();
        assert!(node.random());
        assert_eq!(node.causal(), causal, "{ty}");
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
    let text = sfx_graph::describe(Some("water")).unwrap();
    assert!(text.contains("Depth of the water") && text.contains("loop"));
    let body = json!({"flow": 0.5, "loop": true});
    let node = parse_body("water", &body, "x").unwrap();
    assert!(!node.causal(), "a looping water is not causal");
}

/// The recipe in docs/nodes/M4.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "wander", "name": "level", "rate": 0.4, "range": [0.35, 0.7], "seed": 1}),
        json!({"type": "water", "name": "brook", "flow": "level", "depth": 0.3, "splash": 0.15, "seed": 2}),
        json!({"type": "splash", "name": "plop", "size": 0.12, "at": 1.5, "seed": 5}),
        json!({"type": "gain", "name": "plop_g", "input": "plop", "db": -6}),
        json!({"type": "mix", "name": "scene", "inputs": ["brook", "plop_g"]}),
    ]
}

#[test]
fn recipe_renders() {
    let r = render_doc(&recipe(), 6.0, Some(-22.0), Kind::Oneshot);
    println!(
        "recipe: LUFS {:.2}, peak {:.2} dBFS, warnings {:?}",
        r.lufs, r.peak_db, r.warnings
    );
    assert!(r.warnings.iter().all(|w| !w.contains("target")));
    assert!((r.lufs + 22.0).abs() < 1.0);
    // The default checks examples/water.rs prints for the same document.
    let mut d = SoundDoc::named("water");
    d.nodes = recipe();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "scene", "lufs": -22}));
    d.duration = Some(6.0);
    let opts = RenderOpts::default();
    let rendered = d.render(&opts).unwrap();
    let checks = run(
        &Expect::defaults_for(&d),
        &CheckCx {
            doc: &d,
            rendered: &rendered,
            opts: &opts,
        },
    )
    .unwrap();
    let text = report(&checks);
    println!("{text}");
    assert!(checks.iter().all(|c| c.pass), "{text}");
}
