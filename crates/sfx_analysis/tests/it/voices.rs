//! `voices` (a sub-patch per event) and `sound` (instances of other sounds).

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sfx_analysis::{describe, pitch};
use sfx_graph::{Buf, Ctx, Params, RenderOpts, SoundDoc, SoundSource, evaluate};

const SR: usize = 48_000;

fn doc(nodes: Vec<Value>, secs: f32) -> SoundDoc {
    crate::common::bare_doc(nodes, Some(secs))
}

/// A click: an impulse into a damped resonator (distinct onsets).
fn click_patch() -> Value {
    json!([
        {"type": "impulse", "name": "hit"},
        {"type": "resonators", "name": "ring", "input": "hit", "modes": [[1800, 0.02, 1.0]]}
    ])
}

fn events_of(nodes: &[Value], name: &str, secs: f32) -> usize {
    let ctx = Ctx {
        n: (secs * SR as f32) as usize,
        ..Ctx::default()
    };
    let ev = evaluate(nodes, &Params::new(), &ctx, false).unwrap();
    match &*ev.buf(name).unwrap() {
        Buf::Events(v) => v.len(),
        other => panic!("{}", other.kind()),
    }
}

#[test]
fn voices_over_poisson_gives_about_the_expected_onsets() {
    let nodes = vec![
        json!({"type": "poisson", "name": "ev", "rate": 3, "seed": 4}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.15, "patch": click_patch(), "max": 100}),
        json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
    ];
    let d = doc(nodes.clone(), 8.0);
    let events = events_of(&nodes[..2], "ev", 8.0);
    // The rate is 3/s over 8 s: 24 events, +-20 %.
    assert!((19..=29).contains(&events), "{events} events");
    let r = d.render(&RenderOpts::default()).unwrap();
    let onsets = describe(&r.audio).onsets.len();
    let lo = (events as f32 * 0.8).floor() as usize;
    let hi = (events as f32 * 1.2).ceil() as usize;
    assert!(
        (lo..=hi).contains(&onsets),
        "{onsets} onsets for {events} events"
    );
}

#[test]
fn voices_render_per_event_params_and_are_deterministic() {
    // Three events at 0.1, 0.5, 0.9 s; the pitch comes from the event index.
    let nodes = vec![
        json!({"type": "pattern", "name": "ev", "times": [0.1, 0.5, 0.9]}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.3,
               "map": {"hz": "220 * (1 + $index)"},
               "patch": [
                   {"type": "osc", "name": "o", "freq": "$hz", "amp": 0.4},
                   {"type": "decay", "name": "e", "time": 0.5},
                   {"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}]}),
        json!({"type": "output", "name": "out", "input": "v", "fade_in": 0, "fade_out": 0, "ceiling_db": 6}),
    ];
    let d = doc(nodes, 1.3);
    let r = d.render(&RenderOpts::default()).unwrap();
    let x = r.audio.mix_mono();
    for (k, hz) in [(0.1f32, 220.0f32), (0.5, 440.0), (0.9, 660.0)] {
        let a = (k * SR as f32) as usize;
        let seg = &x[a + 2000..a + 12_000];
        let frames = pitch::yin(seg, SR as u32);
        let voiced: Vec<f32> = frames.iter().filter_map(|f| f.hz).collect();
        assert!(!voiced.is_empty(), "event at {k}: no pitch");
        let med = voiced[voiced.len() / 2];
        let cents = 1200.0 * (med / hz).log2();
        assert!(
            cents.abs() < 20.0,
            "event at {k} s read {med} Hz, wanted {hz} ({cents:+.1} cents)"
        );
    }
    // Silent between events.
    assert!(
        x[(0.42 * SR as f32) as usize..(0.49 * SR as f32) as usize]
            .iter()
            .all(|v| v.abs() < 1e-3)
    );
    // Same bytes at any thread count (voices render in parallel, sum in order).
    let render = |t: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .unwrap()
            .install(|| {
                d.render(&RenderOpts {
                    cached: false,
                    ..RenderOpts::default()
                })
                .unwrap()
                .audio
            })
    };
    assert_eq!(render(1), render(8));
}

#[test]
fn rand_differs_per_event_and_per_seed() {
    let nodes = |seed: u64| {
        vec![
            json!({"type": "pattern", "name": "ev", "times": [0.1, 0.6]}),
            json!({"type": "voices", "name": "v", "events": "ev", "length": 0.3,
                   "map": {"hz": "200 + $rand * 1000"},
                   "patch": [{"type": "osc", "name": "o", "freq": "$hz", "amp": 0.4}]}),
            json!({"type": "output", "name": "out", "input": "v"}),
        ]
        .into_iter()
        .map(|mut n| {
            if n["type"] == "voices" {
                n["seed"] = json!(seed);
            }
            n
        })
        .collect::<Vec<_>>()
    };
    let hz = |d: &SoundDoc, t: f32| {
        let x = d.render(&RenderOpts::default()).unwrap().audio.mix_mono();
        let a = (t * SR as f32) as usize + 3000;
        let f = pitch::yin(&x[a..a + 9000], SR as u32);
        f.iter().filter_map(|f| f.hz).next().unwrap_or(0.0)
    };
    let (mut a, mut b) = (doc(nodes(0), 1.0), doc(nodes(0), 1.0));
    b.seed = 2;
    a.seed = 1;
    let (a1, a2) = (hz(&a, 0.1), hz(&a, 0.6));
    assert!(
        (a1 - a2).abs() > 20.0,
        "two events share a pitch: {a1} {a2}"
    );
    let (b1, b2) = (hz(&b, 0.1), hz(&b, 0.6));
    assert!(
        (a1 - b1).abs() > 5.0 || (a2 - b2).abs() > 5.0,
        "the document seed changes the draws"
    );
    assert!((200.0..=1200.0).contains(&a1) && (200.0..=1200.0).contains(&a2));
}

#[test]
fn voices_errors_say_what_to_do() {
    let mk = |extra: Value| {
        let mut v = json!({"type": "voices", "name": "v", "events": "ev", "patch": click_patch()});
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        doc(
            vec![
                json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 10}),
                v,
                json!({"type": "output", "name": "out", "input": "v"}),
            ],
            1.5,
        )
    };
    let e = mk(json!({"max": 4}))
        .render(&RenderOpts::default())
        .unwrap_err();
    assert!(
        e.contains("10 events but max is 4") && e.contains("raise max"),
        "{e}"
    );
    let e = mk(json!({"map": {"x": "$nope"}}))
        .render(&RenderOpts::default())
        .unwrap_err();
    assert!(e.contains("map.x") && e.contains("nope"), "{e}");
    let e = mk(json!({"patch": [{"type": "osc", "name": "o", "freq": 440, "frq": 1}]}))
        .render(&RenderOpts::default())
        .unwrap_err();
    assert!(e.contains("patch (event #0)") && e.contains("frq"), "{e}");
    let e = mk(json!({"patch": []}))
        .render(&RenderOpts::default())
        .unwrap_err();
    assert!(e.contains("patch: give at least one node"), "{e}");
    // Events from an audio node: the events accessor explains.
    let bad = doc(
        vec![
            json!({"type": "noise", "name": "ev"}),
            json!({"type": "voices", "name": "v", "events": "ev", "patch": click_patch()}),
            json!({"type": "output", "name": "out", "input": "v"}),
        ],
        0.5,
    );
    assert!(
        bad.render(&RenderOpts::default())
            .unwrap_err()
            .contains("on_threshold")
    );
    // A voice that is stereo makes the voices stereo.
    let st = doc(
        vec![
            json!({"type": "once", "name": "ev", "at": 0.05}),
            json!({"type": "voices", "name": "v", "events": "ev", "length": 0.2,
                   "patch": [{"type": "osc", "name": "o", "freq": 300}, {"type": "pan", "name": "p", "input": "o", "pan": -1}]}),
            json!({"type": "output", "name": "out", "input": "v"}),
        ],
        0.4,
    );
    assert!(st.render(&RenderOpts::default()).unwrap().audio.is_stereo());
}

fn source(docs: Vec<SoundDoc>) -> SoundSource {
    let names: Vec<String> = docs.iter().map(|d| d.name.clone()).collect();
    let map: BTreeMap<String, SoundDoc> = docs.into_iter().map(|d| (d.name.clone(), d)).collect();
    SoundSource::new(names, move |n| map.get(n).cloned())
}

fn thud() -> SoundDoc {
    let mut d = SoundDoc::named("thud");
    d.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": 90, "amp": 0.8}),
        json!({"type": "decay", "name": "e", "time": 0.15}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
        json!({"type": "output", "name": "out", "input": "m", "lufs": -18}),
    ];
    d
}

#[test]
fn a_sound_node_instances_another_document_layered_at_a_time() {
    let child = thud();
    let alone = child.render(&RenderOpts::default()).unwrap();
    let mut parent = doc(
        vec![
            json!({"type": "sound", "name": "a", "sound": "thud"}),
            json!({"type": "sound", "name": "b", "sound": "thud", "at": 0.5, "gain_db": -6}),
            json!({"type": "mix", "name": "m", "inputs": ["a", "b"]}),
            json!({"type": "output", "name": "out", "input": "m", "fade_in": 0, "fade_out": 0, "ceiling_db": 6}),
        ],
        1.0,
    );
    parent.name = "layers".into();
    let opts = RenderOpts {
        sounds: Some(source(vec![child.clone()])),
        ..RenderOpts::default()
    };
    let r = parent.render(&opts).unwrap();
    let x = r.audio.mix_mono();
    let first = &alone.audio.channels[0];
    for (i, v) in first.iter().take(4000).enumerate() {
        assert!((x[i] - v).abs() < 1e-5, "sample {i}: {} vs {v}", x[i]);
    }
    let off = SR / 2;
    for (i, v) in first.iter().take(4000).enumerate() {
        assert!(
            (x[off + i] - v * 0.5011872).abs() < 1e-4,
            "second instance sample {i}"
        );
    }
    // A render with no source says so.
    let e = parent.render(&RenderOpts::default()).unwrap_err();
    assert!(e.contains("cannot reach other sounds"), "{e}");
    // A missing name lists what exists.
    let e = parent
        .render(&RenderOpts {
            sounds: Some(source(vec![SoundDoc::named("other")])),
            ..RenderOpts::default()
        })
        .unwrap_err();
    assert!(
        e.contains("no sound named `thud`") && e.contains("open or saved sounds: other"),
        "{e}"
    );
    // Param and macro overrides reach the instance.
    let mut tone = SoundDoc::named("tone");
    tone.params.insert("hz".into(), 200.0);
    tone.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": "$hz", "amp": 0.5}),
        json!({"type": "output", "name": "out", "input": "o", "ceiling_db": 6}),
    ];
    tone.duration = Some(0.3);
    let mut p2 = doc(
        vec![
            json!({"type": "sound", "name": "s", "sound": "tone", "params": {"hz": 800}}),
            json!({"type": "output", "name": "out", "input": "s"}),
        ],
        0.3,
    );
    p2.name = "p2".into();
    let o2 = RenderOpts {
        sounds: Some(source(vec![tone])),
        ..RenderOpts::default()
    };
    let x = p2.render(&o2).unwrap().audio;
    let f = pitch::yin(&x.mix_mono()[2000..12_000], SR as u32);
    let med = f.iter().filter_map(|f| f.hz).next().unwrap_or(0.0);
    assert!((med - 800.0).abs() < 10.0, "{med}");
}

#[test]
fn instance_cycles_and_edits_are_handled() {
    let mut a = SoundDoc::named("a");
    a.nodes = vec![
        json!({"type": "sound", "name": "s", "sound": "b"}),
        json!({"type": "output", "name": "out", "input": "s"}),
    ];
    a.duration = Some(0.2);
    let mut b = SoundDoc::named("b");
    b.nodes = vec![
        json!({"type": "sound", "name": "s", "sound": "a"}),
        json!({"type": "output", "name": "out", "input": "s"}),
    ];
    b.duration = Some(0.2);
    let opts = RenderOpts {
        sounds: Some(source(vec![a.clone(), b])),
        ..RenderOpts::default()
    };
    let e = a.render(&opts).unwrap_err();
    assert!(
        e.contains("would instance itself") && e.contains("a -> b -> a"),
        "{e}"
    );
    // Editing the instanced sound re-renders the instance (the node's cache
    // key includes the instanced document).
    let mut parent = SoundDoc::named("p");
    parent.nodes = vec![
        json!({"type": "sound", "name": "s", "sound": "thud"}),
        json!({"type": "output", "name": "out", "input": "s"}),
    ];
    parent.duration = Some(0.3);
    let mut t1 = thud();
    let r1 = parent
        .render(&RenderOpts {
            sounds: Some(source(vec![t1.clone()])),
            ..RenderOpts::default()
        })
        .unwrap();
    t1.nodes[0] = json!({"type": "osc", "name": "o", "freq": 500, "amp": 0.8});
    let r2 = parent
        .render(&RenderOpts {
            sounds: Some(source(vec![t1])),
            ..RenderOpts::default()
        })
        .unwrap();
    assert_ne!(r1.audio, r2.audio);
}

#[test]
fn a_param_inside_a_voices_patch_busts_the_cache() {
    let mk = |f: f64| {
        let mut d = doc(
            vec![
                json!({"type": "once", "name": "e"}),
                json!({"type": "voices", "name": "v", "events": "e", "length": 0.2, "patch": [
                    {"type": "osc", "name": "o", "freq": "$f"}
                ]}),
                json!({"type": "output", "name": "out", "input": "v"}),
            ],
            0.3,
        );
        d.params.insert("f".into(), f);
        d
    };
    let o = RenderOpts::default();
    let a = mk(220.0).render(&o).unwrap();
    let b = mk(880.0).render(&o).unwrap();
    assert_ne!(a.audio.mix_mono(), b.audio.mix_mono());
}

#[test]
fn absurd_lengths_and_nesting_are_errors_or_capped() {
    let d = doc(
        vec![
            json!({"type": "once", "name": "e"}),
            json!({"type": "voices", "name": "v", "events": "e", "length": 1e5, "patch": [
                {"type": "osc", "name": "o", "freq": 440}
            ]}),
            json!({"type": "output", "name": "out", "input": "v"}),
        ],
        0.2,
    );
    d.render(&RenderOpts::default()).unwrap();
    let deep = format!("{}1{}", "(".repeat(2000), ")".repeat(2000));
    let d = doc(
        vec![
            json!({"type": "expr", "name": "x", "out": deep}),
            json!({"type": "output", "name": "out", "input": "x"}),
        ],
        0.1,
    );
    assert!(d.render(&RenderOpts::default()).is_err());
}

#[test]
fn a_long_unary_chain_is_an_error() {
    let d = doc(
        vec![
            json!({"type": "expr", "name": "x", "out": format!("{}1", "-".repeat(100_000))}),
            json!({"type": "output", "name": "out", "input": "x"}),
        ],
        0.1,
    );
    assert!(d.render(&RenderOpts::default()).is_err());
}

#[test]
fn unbounded_work_is_an_error_not_a_silent_cut() {
    // A bounce that would never stop.
    let d = doc(
        vec![
            json!({"type": "bounce", "name": "b", "first": 1e-5, "restitution": 0.9999999, "min_interval": 1e-9}),
            json!({"type": "once", "name": "e"}),
            json!({"type": "output", "name": "out", "input": "e"}),
        ],
        0.1,
    );
    // The bounce node is unread, but still evaluated: it must refuse.
    assert!(d.render(&RenderOpts::default()).is_err());
    // A plate so large no mode is audible.
    let d = doc(
        vec![
            json!({"type": "impulse", "name": "i"}),
            json!({"type": "modal", "name": "m", "excite": "i", "material": "wood", "size": 1e6}),
            json!({"type": "output", "name": "out", "input": "m"}),
        ],
        0.1,
    );
    let e = d.render(&RenderOpts::default()).unwrap_err();
    assert!(e.contains("size"), "{e}");
}

mod breakage {
    use super::*;
    use crate::common::{band_db, node_audio, rms};

    /// `n` hits 0.1 s apart from `at`, every one of strength `v`.
    fn hits(n: usize, v: f32, at: f32) -> Value {
        let times: Vec<f32> = (0..n).map(|i| at + 0.1 * i as f32).collect();
        json!({"type": "pattern", "name": "ev", "times": times, "values": {"min": v, "max": v}})
    }

    fn chips(extra: Value, n: usize, v: f32) -> usize {
        let mut f = json!({"type": "fracture_on", "name": "f", "events": "ev", "seed": 3});
        for (k, v) in extra.as_object().unwrap() {
            f[k] = v.clone();
        }
        events_of(&[hits(n, v, 0.1), f], "f", 4.0)
    }

    #[test]
    fn soft_hits_never_break_and_hard_ones_do() {
        assert_eq!(chips(json!({"threshold": 0.5}), 5, 0.5), 0);
        assert_eq!(chips(json!({"threshold": 0.5}), 5, 0.2), 0);
        // value 1.0: s = 1, p = 1, 1 + chips events each.
        assert_eq!(chips(json!({"threshold": 0.5, "chips": 10}), 1, 1.0), 11);
    }

    #[test]
    fn count_and_probability_rise_with_strength() {
        let n = |v: f32| chips(json!({"threshold": 0.2, "chips": 20}), 20, v);
        let (lo, mid, hi) = (n(0.4), n(0.7), n(1.0));
        println!("chips from 20 hits at 0.4 / 0.7 / 1.0: {lo} {mid} {hi}");
        assert!(lo < mid && mid < hi, "{lo} {mid} {hi}");
        assert_eq!(hi, 20 * 21);
    }

    #[test]
    fn chips_follow_the_hit_and_spread_with_strength() {
        let nodes = [
            hits(1, 1.0, 0.5),
            json!({"type": "fracture_on", "name": "f", "events": "ev", "spread": 0.2, "chips": 30}),
        ];
        let ctx = Ctx {
            n: 2 * SR,
            ..Ctx::default()
        };
        let ev = evaluate(&nodes, &Params::new(), &ctx, false).unwrap();
        let Buf::Events(v) = &*ev.buf("f").unwrap() else {
            panic!()
        };
        assert!((v[0].t - 0.5).abs() < 1e-9 && v[0].value > 0.99);
        assert!(v.iter().all(|e| e.t >= 0.5 && e.t <= 0.7 + 1e-9));
        assert!(v.windows(2).all(|w| w[0].t <= w[1].t));
    }

    #[test]
    fn fracture_on_is_deterministic_and_composes_with_voices() {
        let nodes = vec![
            hits(2, 1.0, 0.1),
            json!({"type": "fracture_on", "name": "f", "events": "ev", "chips": 8}),
            json!({"type": "voices", "name": "v", "events": "f", "length": 0.1, "max": 64, "patch": click_patch()}),
            json!({"type": "output", "name": "out", "input": "v", "peak_db": -3}),
        ];
        let a = doc(nodes.clone(), 1.5)
            .render(&RenderOpts::default())
            .unwrap();
        let b = doc(nodes, 1.5).render(&RenderOpts::default()).unwrap();
        assert_eq!(a.audio.channels, b.audio.channels);
        assert!(describe(&a.audio).onsets.len() >= 4);
    }

    fn dust(extra: Value) -> Vec<f32> {
        let mut d = json!({"type": "dust", "name": "d", "events": "ev", "seed": 2});
        for (k, v) in extra.as_object().unwrap() {
            d[k] = v.clone();
        }
        node_audio([hits(1, 1.0, 0.2), d], "d", 3.0)
    }

    fn window_rms(x: &[f32], a: f32, b: f32) -> f32 {
        rms(&x[(a * SR as f32) as usize..(b * SR as f32) as usize])
    }

    #[test]
    fn dust_is_silent_before_the_hit_then_settles() {
        let x = dust(json!({"settle": 0.5}));
        assert_eq!(window_rms(&x, 0.0, 0.19), 0.0);
        let (early, mid, late) = (
            window_rms(&x, 0.25, 0.4),
            window_rms(&x, 0.7, 0.9),
            window_rms(&x, 2.0, 3.0),
        );
        println!("dust rms early / mid / late: {early:.5} {mid:.5} {late:.6}");
        assert!(early > mid && mid > late && early > 1e-4);
        assert!(late < early * 0.05);
    }

    #[test]
    fn longer_settle_keeps_the_dust_longer() {
        let (short, long) = (dust(json!({"settle": 0.3})), dust(json!({"settle": 2.0})));
        assert!(window_rms(&long, 1.0, 1.5) > 5.0 * window_rms(&short, 1.0, 1.5));
    }

    #[test]
    fn dust_level_follows_hit_strength() {
        let run = |v: f32| {
            let x = node_audio(
                [
                    hits(1, v, 0.2),
                    json!({"type": "dust", "name": "d", "events": "ev", "seed": 2}),
                ],
                "d",
                1.5,
            );
            window_rms(&x, 0.2, 1.0)
        };
        assert!(run(1.0) > 2.0 * run(0.4));
    }

    #[test]
    fn dust_materials_sit_in_their_bands() {
        // Share of the energy under 3 kHz: wood > masonry > glass.
        let low = |m: &str| {
            let x = dust(json!({"material": m}));
            band_db(&x, SR as u32, 300.0, 3000.0) - band_db(&x, SR as u32, 3000.0, 12000.0)
        };
        let (wood, mas, glass) = (low("wood"), low("masonry"), low("glass"));
        println!("dust low-minus-high dB wood / masonry / glass: {wood:.1} {mas:.1} {glass:.1}");
        assert!(wood > mas && mas > glass);
        let x = dust(json!({"hz": 4000, "q": 3}));
        assert!(
            band_db(&x, SR as u32, 3000.0, 5500.0) > band_db(&x, SR as u32, 200.0, 1500.0) + 12.0
        );
    }

    #[test]
    fn dust_is_deterministic_and_rejects_audio_events() {
        assert_eq!(dust(json!({})), dust(json!({})));
        assert_ne!(dust(json!({})), dust(json!({"seed": 9})));
        let e = crate::common::render_error_raw(
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "dust", "name": "d", "events": "n"}),
                json!({"type": "output", "name": "out", "input": "d"}),
            ],
            0.5,
        );
        assert!(e.contains("events"), "{e}");
    }
}
