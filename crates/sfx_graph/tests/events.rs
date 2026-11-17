//! Event nodes and `grains`: counts, timing, determinism.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sfx_graph::{Buf, Event, RenderOpts, Rendered, SoundDoc};

fn render(mut nodes: Vec<Value>, secs: f32, seed: u64, threads: usize) -> Rendered {
    nodes.push(json!({"type": "osc", "name": "o", "freq": 100, "amp": 0.1}));
    nodes.push(json!({"type": "output", "name": "out", "input": "o"}));
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(secs);
    d.seed = seed;
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(|| {
            d.render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap_or_else(|e| panic!("{e}"))
        })
}

fn events(node: Value, secs: f32, seed: u64) -> Vec<Event> {
    events_with(vec![node], secs, seed, 1)
}

fn events_with(nodes: Vec<Value>, secs: f32, seed: u64, threads: usize) -> Vec<Event> {
    let name = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    let r = render(nodes, secs, seed, threads);
    let b = r.eval.buf(&name).unwrap();
    match &*b {
        Buf::Events(v) => v.clone(),
        other => panic!("{name}: {}", other.kind()),
    }
}

fn gaps(e: &[Event]) -> Vec<f64> {
    e.windows(2).map(|w| w[1].t - w[0].t).collect()
}

#[test]
fn poisson_count_and_gaps_look_right() {
    let mut total = 0usize;
    let mut all_gaps = Vec::new();
    for seed in 1..=6 {
        let e = events(
            json!({"type": "poisson", "name": "p", "rate": 20}),
            5.0,
            seed,
        );
        assert!(
            (80..=120).contains(&e.len()),
            "seed {seed}: {} events",
            e.len()
        );
        assert!(e.windows(2).all(|w| w[0].t <= w[1].t));
        assert!(e.iter().enumerate().all(|(i, x)| x.id == i as u32));
        total += e.len();
        all_gaps.extend(gaps(&e));
    }
    eprintln!("poisson: mean count {}", total as f64 / 6.0);
    let mean = all_gaps.iter().sum::<f64>() / all_gaps.len() as f64;
    assert!((mean - 0.05).abs() < 0.0075, "mean gap {mean}");
    // Exponential: P(gap < mean) = 1 - 1/e ~ 0.632.
    let below = all_gaps.iter().filter(|g| **g < mean).count() as f64 / all_gaps.len() as f64;
    assert!((below - 0.632).abs() < 0.07, "fraction below mean {below}");
    eprintln!("poisson: mean gap {mean:.4}, fraction below mean {below:.3}");
}

#[test]
fn poisson_rate_can_be_a_signal() {
    let e = events_with(
        vec![
            json!({"type": "ramp", "name": "r", "from": 2, "to": 60, "time": 4.0}),
            json!({"type": "poisson", "name": "p", "rate": "r"}),
        ],
        4.0,
        3,
        1,
    );
    let early = e.iter().filter(|x| x.t < 2.0).count();
    let late = e.len() - early;
    eprintln!("variable rate: {early} early, {late} late");
    assert!(late > 2 * early, "{early} vs {late}");
}

#[test]
fn pattern_times_every_and_jitter() {
    let e = events(
        json!({"type": "pattern", "name": "p", "every": 0.25, "count": 4, "start": 0.1}),
        2.0,
        1,
    );
    let t: Vec<f64> = e.iter().map(|x| x.t).collect();
    assert_eq!(t.len(), 4);
    for (k, x) in t.iter().enumerate() {
        assert!((x - (0.1 + 0.25 * k as f64)).abs() < 1e-6, "{t:?}");
    }
    assert!(e.iter().all(|x| x.value == 1.0));
    let e = events(
        json!({"type": "pattern", "name": "p", "times": [0.5, 1.0, 1.5], "jitter": 0.02}),
        2.0,
        1,
    );
    assert_eq!(e.len(), 3);
    let mut moved = false;
    for (x, base) in e.iter().zip([0.5, 1.0, 1.5]) {
        assert!((x.t - base).abs() <= 0.02 + 1e-9, "{} vs {base}", x.t);
        moved |= (x.t - base).abs() > 1e-6;
    }
    assert!(moved);
    // Events past the document are dropped.
    let e = events(
        json!({"type": "pattern", "name": "p", "times": [0.5, 5.0]}),
        2.0,
        1,
    );
    assert_eq!(e.len(), 1);
}

#[test]
fn bounce_intervals_shrink_geometrically() {
    let e = events(
        json!({"type": "bounce", "name": "b", "first": 0.4, "restitution": 0.6, "start": 0.05}),
        3.0,
        1,
    );
    let g = gaps(&e);
    assert!(g.len() > 5);
    for (k, gap) in g.iter().enumerate() {
        let want = 0.4 * 0.6f64.powi(k as i32);
        assert!((gap - want).abs() < 1e-6, "gap {k}: {gap} vs {want}");
        assert!(*gap >= 0.004);
    }
    // The next interval would be under min_interval.
    assert!(0.4 * 0.6f64.powi(g.len() as i32) < 0.004);
    assert!((e[0].t - 0.05).abs() < 1e-6);
    for (k, x) in e.iter().enumerate() {
        assert!((f64::from(x.value) - 0.6f64.powi(k as i32)).abs() < 1e-5);
    }
}

#[test]
fn burst_count_spread_and_curve() {
    let e = events(
        json!({"type": "burst", "name": "b", "count": 9, "spread": 0.2, "at": 0.1}),
        1.0,
        1,
    );
    assert_eq!(e.len(), 9);
    assert!((e[0].t - 0.1).abs() < 1e-6);
    assert!((e[8].t - 0.3).abs() < 1e-6);
    let g = gaps(&e);
    assert!(g.iter().all(|x| (x - 0.025).abs() < 1e-6));
    let e = events(
        json!({"type": "burst", "name": "b", "count": 9, "spread": 0.2, "curve": "exp"}),
        1.0,
        1,
    );
    let g = gaps(&e);
    assert!(g.windows(2).all(|w| w[1] > w[0]), "exp gaps grow: {g:?}");
    assert!((e[8].t - 0.2).abs() < 1e-6);
}

#[test]
fn values_are_seeded_and_bounded() {
    let node = json!({"type": "poisson", "name": "p", "rate": 50,
        "values": {"min": 0.2, "max": 0.8}});
    let a = events(node.clone(), 2.0, 5);
    assert!(a.iter().all(|x| (0.2..=0.8).contains(&x.value)));
    assert!(a.iter().any(|x| x.value < 0.4) && a.iter().any(|x| x.value > 0.6));
    let b = events(node, 2.0, 5);
    assert_eq!(a, b);
    let e = events(
        json!({"type": "burst", "name": "b", "count": 50, "spread": 0.5,
            "values": {"min": 0.01, "max": 1.0, "curve": "exp"}}),
        1.0,
        2,
    );
    assert!(e.iter().all(|x| (0.01..=1.0).contains(&x.value)));
}

#[test]
fn on_threshold_fires_per_rising_crossing_and_respects_hold() {
    let nodes = |hold: Option<f32>| {
        let mut t = json!({"type": "on_threshold", "name": "t", "input": "sq", "level": 0.5});
        if let Some(h) = hold {
            t["hold"] = json!(h);
        }
        vec![
            json!({"type": "osc", "name": "sq", "shape": "square", "freq": 5, "amp": 1.0}),
            t,
        ]
    };
    // 5 Hz over 1 s: the square starts high (an event at 0), then rises at
    // 0.2, 0.4, 0.6, 0.8.
    let e = events_with(nodes(None), 1.0, 1, 1);
    assert_eq!(e.len(), 5, "{e:?}");
    for (k, x) in e.iter().enumerate() {
        assert!((x.t - 0.2 * k as f64).abs() < 0.002, "{} at {k}", x.t);
        assert!(x.value >= 0.5);
    }
    // A hold longer than the period keeps every other crossing out.
    let e = events_with(nodes(Some(0.3)), 1.0, 1, 1);
    assert_eq!(e.len(), 3, "{e:?}");
    // A signal made only of audio is refused where events are wanted.
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": 100}),
        json!({"type": "grains", "name": "g", "source": "o", "events": "o", "size": 0.02}),
        json!({"type": "output", "name": "out", "input": "g"}),
    ];
    d.duration = Some(0.5);
    let err = d.render(&RenderOpts::default()).unwrap_err();
    assert!(err.contains("is audio, not events"), "{err}");
}

#[test]
fn events_are_deterministic_across_threads_and_vary_with_seed() {
    let mk = |seed, threads| {
        events_with(
            vec![
                json!({"type": "wander", "name": "w", "rate": 3, "range": [5, 40]}),
                json!({"type": "poisson", "name": "p", "rate": "w", "values": {"min": 0, "max": 1}}),
            ],
            3.0,
            seed,
            threads,
        )
    };
    let a = mk(1, 1);
    assert!(!a.is_empty());
    assert_eq!(a, mk(1, 8));
    assert_ne!(a, mk(2, 1));
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn grains_over_poisson_make_sound_at_the_events() {
    let nodes = |pan: Option<f32>| {
        let mut g = json!({"type": "grains", "name": "g", "source": "tone", "events": "p",
            "size": 0.02, "position": 0.1, "jitter": 0.05});
        if let Some(p) = pan {
            g["pan_spread"] = json!(p);
        }
        vec![
            json!({"type": "osc", "name": "tone", "freq": 800, "amp": 0.5}),
            json!({"type": "poisson", "name": "p", "rate": 10}),
            g,
        ]
    };
    let secs = 3.0;
    let r = render(nodes(None), secs, 4, 1);
    let ev = match &*r.eval.buf("p").unwrap() {
        Buf::Events(v) => v.clone(),
        _ => panic!(),
    };
    let g = r.eval.buf("g").unwrap();
    let Buf::Audio(x) = &*g else {
        panic!("mono expected, got {}", g.kind())
    };
    assert_eq!(x.len(), (secs * 48_000.0) as usize);
    assert!(x.iter().all(|v| v.is_finite()));
    assert!(rms(x) > 1e-3);
    // Count onsets: 5 ms windows that are loud after a quiet one.
    let w = 240;
    let loud: Vec<bool> = x.chunks(w).map(|c| rms(c) > 0.02).collect();
    let onsets = loud.windows(2).filter(|p| !p[0] && p[1]).count() + usize::from(loud[0]);
    eprintln!("grains: {} events, {onsets} onsets", ev.len());
    // Grains closer than ~22 ms run together into one loud stretch.
    let want = 1 + ev.windows(2).filter(|w| w[1].t - w[0].t > 0.022).count();
    let want = want as f64;
    assert!(
        (onsets as f64 - want).abs() <= 0.15 * want + 1.0,
        "{onsets} onsets for {want} events"
    );
    // Silence between grains: a window well before the first event is quiet.
    let t0 = ev[0].t;
    if t0 > 0.01 {
        assert!(rms(&x[..(t0 * 48_000.0 * 0.5) as usize]) < 1e-6);
    }
    let r = render(nodes(Some(0.8)), secs, 4, 1);
    let Buf::Stereo(s) = &*r.eval.buf("g").unwrap() else {
        panic!("stereo expected")
    };
    assert_eq!(s.len(), x.len());
    assert!(s.iter().any(|p| (p[0] - p[1]).abs() > 1e-4));
}
