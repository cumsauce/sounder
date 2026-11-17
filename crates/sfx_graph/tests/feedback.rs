//! Feedback loops: legal only through a delay, evaluated per sample.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_graph::{Buf, Ctx, Params, RenderOpts, SoundDoc, clear_cache, evaluate};

fn ctx(secs: f32) -> Ctx {
    Ctx {
        sample_rate: 48_000,
        n: (secs * 48_000.0) as usize,
        seed: 1,
        looped: false,
        base_dir: None,
        ..Ctx::default()
    }
}

fn audio(ev: &sfx_graph::Evaluated, name: &str) -> Vec<f32> {
    match &*ev.buf(name).unwrap() {
        Buf::Audio(v) => v.clone(),
        other => panic!("{name} is {}", other.kind()),
    }
}

fn eval(nodes: &[Value], secs: f32, cached: bool) -> Result<sfx_graph::Evaluated, String> {
    evaluate(nodes, &Params::new(), &ctx(secs), cached)
}

#[test]
fn a_loop_through_delay_matches_the_blocks_feedback_field() {
    // Block form: delay's own feedback. Loop form: delay -> gain -> mix -> delay.
    let nodes = vec![
        json!({"type": "impulse", "name": "imp", "at": [0.0, 0.05]}),
        json!({"type": "delay", "name": "blk", "input": "imp", "time": 0.01, "feedback": 0.5}),
        json!({"type": "mix", "name": "m", "inputs": ["imp", "g"]}),
        json!({"type": "delay", "name": "lp", "input": "m", "time": 0.01}),
        json!({"type": "gain", "name": "g", "input": "lp", "gain": 0.5}),
    ];
    let ev = eval(&nodes, 0.3, false).unwrap();
    let (a, b) = (audio(&ev, "blk"), audio(&ev, "lp"));
    assert!(a.iter().any(|v| v.abs() > 0.1));
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!((x - y).abs() < 1e-6, "sample {i}: {x} vs {y}");
    }
    // The echoes really decay by the gain: 1, .5, .25 at 10 ms steps.
    assert!(
        (b[480] - 1.0).abs() < 1e-5 && (b[960] - 0.5).abs() < 1e-5 && (b[1440] - 0.25).abs() < 1e-5
    );
}

#[test]
fn a_cycle_without_a_delay_names_the_nodes() {
    let nodes = vec![
        json!({"type": "impulse", "name": "i"}),
        json!({"type": "mix", "name": "a", "inputs": ["i", "c"]}),
        json!({"type": "filter", "name": "b", "input": "a", "freq": 1000}),
        json!({"type": "gain", "name": "c", "input": "b", "gain": 0.5}),
    ];
    let e = eval(&nodes, 0.1, false).unwrap_err();
    assert!(e.contains("cycle without a delay"), "{e}");
    assert!(
        e.contains("a -> c -> b -> a")
            || e.contains("c -> b -> a -> c")
            || e.contains("b -> a -> c -> b"),
        "{e}"
    );
    // A node reading itself is the shortest such cycle.
    let e = eval(
        &[json!({"type": "gain", "name": "x", "input": "x", "gain": 0.5})],
        0.1,
        false,
    )
    .unwrap_err();
    assert!(e.contains("x -> x"), "{e}");
    // A delay reading itself is legal (silence, but a legal loop).
    let ev = eval(
        &[json!({"type": "delay", "name": "d", "input": "d", "time": 0.01})],
        0.1,
        false,
    )
    .unwrap();
    assert!(audio(&ev, "d").iter().all(|v| *v == 0.0));
}

#[test]
fn nodes_without_a_per_sample_form_are_refused_inside_a_loop() {
    let nodes = vec![
        json!({"type": "osc", "name": "o", "freq": "dd"}),
        json!({"type": "delay", "name": "dd", "input": "o", "time": 0.01}),
    ];
    let e = eval(&nodes, 0.1, false).unwrap_err();
    assert!(
        e.contains("node `o` (osc) is inside a feedback loop")
            && e.contains("loops can contain delay"),
        "{e}"
    );
    // A bad field inside a loop names the node.
    let nodes = vec![
        json!({"type": "impulse", "name": "i"}),
        json!({"type": "mix", "name": "m", "inputs": ["i", {"input": "g", "pan": 0.5}]}),
        json!({"type": "delay", "name": "d", "input": "m", "time": 0.01}),
        json!({"type": "gain", "name": "g", "input": "d", "gain": 0.5}),
    ];
    let e = eval(&nodes, 0.1, false).unwrap_err();
    assert!(
        e.contains("node `m` (mix)") && e.contains("pan and at are not available inside a loop"),
        "{e}"
    );
}

#[test]
fn loops_are_deterministic_cached_and_keyed_on_what_feeds_them() {
    let build = |gain: f64, freq: f64| {
        vec![
            json!({"type": "noise", "name": "n", "seed": 3}),
            json!({"type": "decay", "name": "e", "time": 0.01}),
            json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "mix", "name": "m", "inputs": ["burst", "g"]}),
            json!({"type": "delay", "name": "d", "input": "m", "time": 0.004}),
            json!({"type": "filter", "name": "f", "input": "d", "mode": "lp", "freq": freq}),
            json!({"type": "gain", "name": "g", "input": "f", "gain": gain}),
        ]
    };
    let doc = |gain, freq| {
        let mut d = SoundDoc::named("t");
        d.nodes = build(gain, freq);
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "g"}));
        d.duration = Some(0.3);
        d
    };
    let render = |d: &SoundDoc, threads: usize, cached: bool| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                d.render(&RenderOpts {
                    cached,
                    ..RenderOpts::default()
                })
                .unwrap()
                .audio
            })
    };
    let d = doc(0.95, 3000.0);
    let one = render(&d, 1, false);
    assert_eq!(one, render(&d, 8, false));
    assert_eq!(one, render(&d, 3, true));
    assert!(one.peak() > 0.01);
    // Cache: the second evaluation computes nothing; a changed gain inside the
    // loop re-runs the whole loop; its sources stay cached.
    clear_cache();
    let a = eval(&build(0.9, 3000.0), 0.2, true).unwrap();
    assert_eq!(a.runs, 7);
    assert_eq!(eval(&build(0.9, 3000.0), 0.2, true).unwrap().runs, 0);
    assert_eq!(
        eval(&build(0.8, 3000.0), 0.2, true).unwrap().runs,
        4,
        "m, d, f, g re-run; n, e, burst cached"
    );
    // Stable: a loop gain under 1 decays.
    let tail = audio(&a, "g");
    let late: f32 = tail[tail.len() - 2000..]
        .iter()
        .map(|v| v.abs())
        .fold(0.0, f32::max);
    let early: f32 = tail[..2000].iter().map(|v| v.abs()).fold(0.0, f32::max);
    assert!(late < early * 0.5, "{late} vs {early}");
}

#[test]
fn the_loop_reads_outside_signals_per_sample() {
    // An echo whose gain is an outside ramp: the loop must see the ramp's
    // value at each sample, not a constant.
    let nodes = vec![
        json!({"type": "impulse", "name": "i", "at": [0.0]}),
        json!({"type": "ramp", "name": "k", "from": 0.9, "to": 0.1, "time": 0.2, "curve": "linear"}),
        json!({"type": "mix", "name": "m", "inputs": ["i", "g"]}),
        json!({"type": "delay", "name": "d", "input": "m", "time": 0.01}),
        json!({"type": "math", "name": "g", "op": "mul", "a": "d", "b": "k"}),
    ];
    let ev = eval(&nodes, 0.1, false).unwrap();
    let d = audio(&ev, "d");
    let k = |i: usize| 0.9 - 0.8 * (i as f32 / 48_000.0 / 0.2);
    assert!((d[480] - 1.0).abs() < 1e-5);
    let e2 = d[480] * k(480);
    assert!((d[960] - e2).abs() < 1e-4, "{} vs {e2}", d[960]);
    let e3 = e2 * k(960);
    assert!((d[1440] - e3).abs() < 1e-4, "{} vs {e3}", d[1440]);
}

#[test]
fn expr_can_sit_in_a_loop_and_matches_the_equivalent_nodes() {
    // A saturating echo: delay -> (gain 0.9 -> tanh) -> mix, written once with
    // gain + shape nodes and once as a single expr in the loop.
    let nodes = vec![
        json!({"type": "impulse", "name": "i", "at": [0.0, 0.03]}),
        json!({"type": "mix", "name": "m1", "inputs": ["i", "s"]}),
        json!({"type": "delay", "name": "d1", "input": "m1", "time": 0.005}),
        json!({"type": "gain", "name": "g", "input": "d1", "gain": 0.9}),
        json!({"type": "shape", "name": "s", "input": "g", "mode": "tanh"}),
        json!({"type": "mix", "name": "m2", "inputs": ["i", "e"]}),
        json!({"type": "delay", "name": "d2", "input": "m2", "time": 0.005}),
        json!({"type": "expr", "name": "e", "inputs": {"a": "d2"}, "out": "tanh(a * 0.9)"}),
    ];
    let ev = eval(&nodes, 0.2, false).unwrap();
    let (a, b) = (audio(&ev, "d1"), audio(&ev, "d2"));
    assert!(a.iter().any(|v| v.abs() > 0.1));
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!((x - y).abs() < 1e-5, "sample {i}: {x} vs {y}");
    }
    // State lives per sample inside the loop too: a one-pole smoother in expr.
    let nodes = vec![
        json!({"type": "impulse", "name": "i"}),
        json!({"type": "mix", "name": "m", "inputs": ["i", "g"]}),
        json!({"type": "delay", "name": "d", "input": "m", "time": 0.002}),
        json!({"type": "expr", "name": "g", "inputs": {"a": "d"}, "state": {"y1": 0.0},
               "out": "y1 + 0.3 * (a * 0.95 - y1)", "update": {"y1": "y1 + 0.3 * (a * 0.95 - y1)"}}),
    ];
    let ev = eval(&nodes, 0.1, false).unwrap();
    let d = audio(&ev, "d");
    assert!(d.iter().all(|v| v.is_finite()) && d.iter().any(|v| v.abs() > 0.05));
}
