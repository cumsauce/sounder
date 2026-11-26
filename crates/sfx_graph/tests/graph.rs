//! The graph: determinism, strict parsing, atomic edits, lengths.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_graph::{Ctx, DocEdit, Kind, Params, RenderOpts, SoundDoc, evaluate, wav};

/// A graph that touches randomness, modulation, stereo and parallel levels.
fn busy() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "color": "pink", "seed": 4}),
        json!({"type": "noise", "name": "v", "color": "velvet", "density": 900}),
        json!({"type": "wander", "name": "w", "rate": 7, "range": [300, 3000]}),
        json!({"type": "lfo", "name": "l", "freq": 3, "range": [0.2, 0.8]}),
        json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": "w", "q": 4}),
        json!({"type": "osc", "name": "o", "shape": "pulse", "freq": 110, "width": "l", "amp": 0.3}),
        json!({"type": "comb", "name": "c", "input": "v", "freq": 330, "feedback": 0.8, "damp": 0.2}),
        json!({"type": "mix", "name": "m", "inputs": ["f", {"input": "o", "pan": -0.4}, {"input": "c", "gain": 0.3, "at": 0.05}]}),
        json!({"type": "reverb", "name": "r", "input": "m", "size": 0.4, "decay": 0.8}),
        json!({"type": "compress", "name": "k", "input": "r", "threshold_db": -20, "ratio": 3}),
        json!({"type": "decay", "name": "d", "time": 0.6}),
        json!({"type": "gain", "name": "g", "input": "k", "gain": "d"}),
        json!({"type": "output", "name": "out", "input": "g", "lufs": -18}),
    ]
}

fn doc_with(nodes: Vec<Value>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d
}

fn render_bytes(doc: &SoundDoc, threads: usize) -> Vec<u8> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let r = doc
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        wav::to_bytes16(&r.audio)
    })
}

#[test]
fn same_seed_gives_identical_bytes_at_any_thread_count() {
    let doc = doc_with(busy());
    let one = render_bytes(&doc, 1);
    let many = render_bytes(&doc, 8);
    let again = render_bytes(&doc, 3);
    assert!(!one.is_empty());
    assert_eq!(one, many, "1 thread vs 8 threads");
    assert_eq!(one, again, "1 thread vs 3 threads");
    // The default global pool and the cache agree too.
    let cached = doc.render(&RenderOpts::default()).unwrap();
    assert_eq!(one, wav::to_bytes16(&cached.audio));
    // A different seed differs.
    let mut other = doc.clone();
    other.seed = 2;
    assert_ne!(one, render_bytes(&other, 1));
}

#[test]
fn parse_errors_name_the_node_and_the_field() {
    let mut d = SoundDoc::named("t");
    let bad_field = d
        .apply(
            vec![
                json!({"type": "osc", "name": "o", "freq": 440}),
                json!({"type": "filter", "name": "lp", "input": "o", "frq": 800}),
                json!({"type": "output", "name": "out", "input": "lp"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(bad_field.contains("node #1 (filter \"lp\")"), "{bad_field}");
    assert!(bad_field.contains("frq"), "{bad_field}");
    assert!(
        bad_field.contains("[q]"),
        "lists the node's fields: {bad_field}"
    );

    let bad_value = d
        .apply(
            vec![
                json!({"type": "osc", "name": "o", "freq": 440, "phase": "half"}),
                json!({"type": "output", "name": "out", "input": "o"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(bad_value.contains("osc \"o\""), "{bad_value}");
    assert!(bad_value.contains("phase"), "{bad_value}");

    let bad_ref = d
        .apply(
            vec![
                json!({"type": "osc", "name": "tone", "freq": 440}),
                json!({"type": "output", "name": "out", "input": "tonee"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(
        bad_ref.contains("`out`") && bad_ref.contains("did you mean `tone`"),
        "{bad_ref}"
    );

    let bad_type = d
        .apply(
            vec![json!({"type": "fliter", "name": "x"})],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(bad_type.contains("did you mean `filter`"), "{bad_type}");
    // Nothing was applied by any of them.
    assert!(d.nodes.is_empty() && d.version == 0);
}

#[test]
fn a_failing_batch_leaves_the_doc_unchanged() {
    let mut d = SoundDoc::named("t");
    let ok = d
        .apply(
            vec![
                json!({"type": "osc", "name": "o", "freq": 440}),
                json!({"type": "decay", "name": "e", "time": 0.2}),
                json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
                json!({"type": "output", "name": "out", "input": "m"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap();
    assert!(ok.changed && ok.label == "+o +e +m +out", "{}", ok.label);
    let before = d.clone();
    // A kernel error (a stereo signal into a mono field) surfaces at apply.
    let err = d
        .apply(
            vec![
                json!({"type": "pan", "name": "p", "input": "o", "pan": 0.5}),
                json!({"type": "osc", "name": "o2", "freq": "p"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(err.contains("stereo") && err.contains("mix"), "{err}");
    assert_eq!(d, before);
    // Changing a node's type is refused.
    let err = d
        .apply(
            vec![json!({"type": "noise", "name": "o"})],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(err.contains("is a osc"), "{err}");
    assert_eq!(d, before);
    // Re-sending an identical node records no step.
    let same = d
        .apply(
            vec![json!({"type": "decay", "name": "e", "time": 0.2})],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap();
    assert!(!same.changed);
    assert_eq!(d.version, before.version);
    // A tweak records one and re-renders only what it reaches.
    let tweak = d
        .apply(
            vec![json!({"type": "decay", "name": "e", "time": 0.3})],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap();
    assert!(tweak.changed && tweak.label == "~e");
    assert_eq!(
        tweak.rendered.eval.runs, 3,
        "e, m and out re-run; o is cached"
    );
    // Delete.
    let del = d
        .apply(
            vec![
                json!({"type": "delete", "name": "e"}),
                json!({"type": "output", "name": "out", "input": "o", "fade_out": 0.05}),
                json!({"type": "delete", "name": "m"}),
            ],
            DocEdit {
                duration: Some(Some(0.5)),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap();
    assert_eq!(del.label, "~out -e -m");
}

#[test]
fn cycles_are_errors_that_name_the_nodes() {
    let d = doc_with(vec![
        json!({"type": "filter", "name": "a", "input": "b", "freq": 500}),
        json!({"type": "gain", "name": "b", "input": "a", "db": -3}),
        json!({"type": "output", "name": "out", "input": "a"}),
    ]);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    assert!(
        e.contains("cycle without a delay")
            && (e.contains("a -> b -> a") || e.contains("b -> a -> b")),
        "{e}"
    );
}

#[test]
fn auto_length_stops_60_db_under_the_peak_on_a_slow_ring() {
    // A short hit plus a metal modal with loss 1 at 1.35 s used to run to
    // about 7 s (-80 dBFS); it stops 60 dB under its own peak, at any level.
    for amp in [1.0, 0.01] {
        let d = doc_with(vec![
            json!({"type": "noise", "name": "n", "color": "white"}),
            json!({"type": "decay", "name": "e", "time": 0.004, "at": 1.35}),
            json!({"type": "math", "name": "b", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "modal", "name": "m", "excite": "b", "material": "metal", "f0": 300, "loss": 1.0}),
            json!({"type": "gain", "name": "g", "input": "m", "gain": amp}),
            json!({"type": "output", "name": "out", "input": "g"}),
        ]);
        let r = d.render(&RenderOpts::default()).unwrap();
        let s = r.audio.seconds();
        assert!(r.auto_length && s < 4.0, "amp {amp}: {s}");
    }
}

#[test]
fn auto_length_ends_when_the_sound_decays() {
    let d = doc_with(vec![
        json!({"type": "osc", "name": "o", "freq": 300}),
        json!({"type": "decay", "name": "e", "time": 0.5}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
        json!({"type": "output", "name": "out", "input": "m"}),
    ]);
    let r = d.render(&RenderOpts::default()).unwrap();
    // -60 dB under the peak at 0.5 s, plus 10 ms kept.
    let s = r.audio.seconds();
    assert!(r.auto_length && (0.5..0.6).contains(&s), "{s}");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);

    // A sound that never decays stops at 30 s and says so.
    let drone = doc_with(vec![
        json!({"type": "osc", "name": "o", "freq": 300, "amp": 0.1}),
        json!({"type": "output", "name": "out", "input": "o"}),
    ]);
    let r = drone.render(&RenderOpts::default()).unwrap();
    assert!((r.audio.seconds() - 30.0).abs() < 0.02);
    assert!(
        r.warnings.iter().any(|w| w.contains("never decays")),
        "{:?}",
        r.warnings
    );

    // A loop needs a duration.
    let mut lp = drone.clone();
    lp.kind = Kind::Loop;
    assert!(
        lp.render(&RenderOpts::default())
            .unwrap_err()
            .contains("duration")
    );
    lp.duration = Some(1.0);
    assert_eq!(
        lp.render(&RenderOpts::default()).unwrap().audio.len(),
        48_000
    );
}

#[test]
fn params_macros_and_expressions_resolve() {
    use sfx_graph::{Claim, Macro, MapCurve, MapEntry, Measure};
    let mut d = doc_with(vec![
        json!({"type": "osc", "name": "o", "freq": "=$base * 2"}),
        json!({"type": "output", "name": "out", "input": "o", "gain_db": -6}),
    ]);
    d.params.insert("base".into(), 220.0);
    d.macros.push(Macro {
        name: "loud".into(),
        value: 0.5,
        default: Some(0.5),
        doc: "level".into(),
        claim: Some(Claim(Measure::Lufs, 1)),
        game: true,
        map: vec![MapEntry {
            node: "out".into(),
            field: "gain_db".into(),
            from: -24.0,
            to: 0.0,
            curve: MapCurve::Linear,
            param: None,
            col: None,
            index: None,
            key: None,
        }],
    });
    d.duration = Some(0.2);
    let r = d.render(&RenderOpts::default()).unwrap();
    // The macro overrides the node's own gain_db: -12 dB at 0.5.
    assert!((r.peak_db + 12.0).abs() < 0.1, "{}", r.peak_db);
    let loud = d
        .render(&RenderOpts {
            macros: [("loud".to_string(), 1.0)].into(),
            ..RenderOpts::default()
        })
        .unwrap();
    assert!(
        loud.peak_db <= -0.99,
        "0 dB then the -1 dBFS ceiling: {}",
        loud.peak_db
    );
    let e = d
        .render(&RenderOpts {
            macros: [("nope".to_string(), 1.0)].into(),
            ..RenderOpts::default()
        })
        .unwrap_err();
    assert!(e.contains("no macro named `nope`"), "{e}");
}

#[test]
fn macro_validation_names_the_problem() {
    use sfx_graph::{Claim, Macro, MapEntry, Measure};
    let entry = |node: &str, field: &str, from: f64, to: f64| MapEntry {
        node: node.into(),
        field: field.into(),
        from,
        to,
        curve: sfx_graph::MapCurve::Linear,
        param: None,
        col: None,
        index: None,
        key: None,
    };
    let mac = |name: &str, map: Vec<MapEntry>| Macro {
        name: name.into(),
        value: 0.5,
        default: None,
        doc: String::new(),
        claim: Some(Claim(Measure::Centroid, 1)),
        game: false,
        map,
    };
    let base = vec![
        json!({"type": "osc", "name": "o", "freq": 440}),
        json!({"type": "filter", "name": "f", "input": "o", "freq": 1000}),
        json!({"type": "output", "name": "out", "input": "f"}),
    ];
    let mut d = doc_with(base.clone());
    let ok = |d: &mut SoundDoc, m: Vec<Macro>| {
        d.apply(
            vec![],
            DocEdit {
                macros: Some(m),
                duration: Some(Some(0.1)),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
    };
    let e = ok(
        &mut d,
        vec![mac("bright", vec![entry("g", "freq", 1.0, 2.0)])],
    )
    .unwrap_err();
    assert!(
        e.contains("no node named `g`") && e.contains("nodes: o, f, out"),
        "{e}"
    );
    let e = ok(
        &mut d,
        vec![
            mac("a", vec![entry("f", "freq", 100.0, 5000.0)]),
            mac("b", vec![entry("f", "freq", 1.0, 2.0)]),
        ],
    )
    .unwrap_err();
    assert!(e.contains("both write f.freq"), "{e}");
    // A field the node does not have: the graph no longer parses at the ends.
    let e = ok(
        &mut d,
        vec![mac("c", vec![entry("f", "frq", 100.0, 5000.0)])],
    )
    .unwrap_err();
    assert!(
        e.contains("frq") && e.contains("with the macros at their values"),
        "{e}"
    );
    let mut bad_exp = entry("f", "freq", 0.0, 5000.0);
    bad_exp.curve = sfx_graph::MapCurve::Exp;
    let e = ok(&mut d, vec![mac("d", vec![bad_exp])]).unwrap_err();
    assert!(e.contains("curve exp needs from and to above 0"), "{e}");
    let e = ok(
        &mut d,
        vec![mac("two words", vec![entry("f", "freq", 1.0, 2.0)])],
    )
    .unwrap_err();
    assert!(e.contains("one word"), "{e}");
    assert!(d.macros.is_empty());
    // A good one is stored with its default, and deleting the node it maps is refused.
    ok(
        &mut d,
        vec![mac("brightness", vec![entry("f", "freq", 300.0, 6000.0)])],
    )
    .unwrap();
    assert_eq!(d.macros[0].default, Some(0.5));
    let e = d
        .apply(
            vec![
                json!({"type": "delete", "name": "f"}),
                json!({"type": "output", "name": "out", "input": "o"}),
            ],
            DocEdit::default(),
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(
        e.contains("no node named `f`") && e.contains("delete_macros"),
        "{e}"
    );
}

#[test]
fn every_node_type_renders_its_example() {
    // Each example wired to stand-in sources so it evaluates for real.
    let sources = vec![
        json!({"type": "osc", "name": "src", "freq": 330, "amp": 0.3}),
        json!({"type": "noise", "name": "noise1"}),
        json!({"type": "impulse", "name": "hit"}),
        json!({"type": "impulse", "name": "burst"}),
        json!({"type": "decay", "name": "amp_env", "time": 0.2}),
        json!({"type": "lfo", "name": "lfo1", "freq": 2}),
        json!({"type": "ramp", "name": "sweep", "from": 800, "to": 200, "time": 0.2}),
        json!({"type": "ramp", "name": "cutoff_env", "from": 4000, "to": 300, "time": 0.2}),
        json!({"type": "osc", "name": "tone", "freq": 220}),
        json!({"type": "osc", "name": "steps", "shape": "square", "freq": 5}),
        json!({"type": "osc", "name": "drums", "shape": "square", "freq": 2}),
        json!({"type": "noise", "name": "n1", "color": "brown"}),
        json!({"type": "osc", "name": "col", "freq": 1}),
        json!({"type": "pan", "name": "verb", "input": "src", "pan": 0.3}),
        json!({"type": "mix", "name": "body", "inputs": ["src"]}),
        json!({"type": "mix", "name": "click", "inputs": ["hit"]}),
        json!({"type": "mix", "name": "mix1", "inputs": ["src"]}),
        json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 3}),
        // phase 4: modal
        json!({"type": "contact", "name": "contact1", "speed": 1.0, "mass": 0.1, "material": "wood"}),
    ];
    let mut streams = Vec::new();
    for t in sfx_graph::NODE_TYPES {
        if matches!(*t, "sample" | "convolve" | "output" | "sound") {
            continue; // need files / are the output
        }
        let mut ex = sfx_graph::example(t).unwrap();
        ex["name"] = json!("under_test");
        let mut nodes = sources.clone();
        nodes.retain(|n| n["name"] != "under_test");
        nodes.push(ex);
        // Event streams are not audio: they are evaluated and read, not
        // mixed. Which nodes are streams is decided by what they render.
        let cx = Ctx {
            n: 14_400,
            ..Ctx::default()
        };
        let ev = evaluate(&nodes, &Params::new(), &cx, false)
            .unwrap_or_else(|e| panic!("{t}: evaluating: {e}"));
        let is_events = |name: &str| matches!(*ev.buf(name).unwrap(), sfx_graph::Buf::Events(_));
        // Every audio stand-in and the node under test feed the output.
        let names: Vec<Value> = nodes
            .iter()
            .filter(|n| !is_events(n["name"].as_str().unwrap()))
            .map(|n| n["name"].clone())
            .collect();
        nodes.push(json!({"type": "mix", "name": "all", "inputs": names, "to": "stereo"}));
        nodes.push(json!({"type": "output", "name": "out", "input": "all"}));
        let mut d = doc_with(nodes);
        d.duration = Some(0.3);
        let r = d
            .render(&RenderOpts::default())
            .unwrap_or_else(|e| panic!("{t}: {e}"));
        assert!(
            r.audio.channels.iter().flatten().all(|x| x.is_finite()),
            "{t}"
        );
        // The same answer from the full render as from the bare evaluation.
        assert_eq!(
            matches!(
                *r.eval.buf("under_test").unwrap(),
                sfx_graph::Buf::Events(_)
            ),
            is_events("under_test"),
            "{t}"
        );
        if is_events("under_test") {
            streams.push(*t);
        }
    }
    for t in [
        "once",
        "pattern",
        "poisson",
        "bounce",
        "burst",
        "on_threshold",
    ] {
        assert!(streams.contains(&t), "{t} should render an event stream");
    }
}

#[test]
fn sample_and_convolve_read_project_files() {
    let dir = std::env::temp_dir().join(format!("sfx_graph_files_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let click: Vec<f32> = (0..4800).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
    wav::write(
        &dir.join("ir.wav"),
        &sfx_graph::Audio::mono(48_000, click),
        wav::Bits::Float32,
    )
    .unwrap();
    let tone: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    wav::write(
        &dir.join("tone.wav"),
        &sfx_graph::Audio::mono(48_000, tone.clone()),
        wav::Bits::Float32,
    )
    .unwrap();
    let mut d = doc_with(vec![
        json!({"type": "sample", "name": "s", "file": "tone.wav"}),
        json!({"type": "convolve", "name": "c", "input": "s", "ir": "ir.wav"}),
        json!({"type": "output", "name": "out", "input": "c", "fade_in": 0, "fade_out": 0, "ceiling_db": 0}),
    ]);
    d.duration = Some(0.5);
    let r = d
        .render(&RenderOpts {
            base_dir: Some(dir.clone()),
            ..RenderOpts::default()
        })
        .unwrap();
    // A unit impulse response passes the sample through unchanged.
    for (a, b) in r.audio.channels[0].iter().zip(&tone).skip(10).take(20_000) {
        assert!((a - b).abs() < 1e-4, "{a} vs {b}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ---- phase 3: expr ----

/// Evaluates `nodes` (no output node needed) and returns node `name`'s mono
/// samples.
fn node_samples(nodes: &[Value], name: &str, n: usize) -> Vec<f32> {
    let ctx = sfx_graph::Ctx {
        sample_rate: 48000,
        n,
        seed: 1,
        looped: false,
        base_dir: None,
        ..sfx_graph::Ctx::default()
    };
    let ev = sfx_graph::evaluate(nodes, &Default::default(), &ctx, false)
        .unwrap_or_else(|e| panic!("{e}"));
    match &*ev.ports[name][0].1 {
        sfx_graph::Buf::Audio(v) => v.clone(),
        other => panic!("{name}: {}", other.kind()),
    }
}

fn eval_error(nodes: Vec<Value>) -> String {
    let ctx = sfx_graph::Ctx {
        sample_rate: 48000,
        n: 100,
        seed: 1,
        looped: false,
        base_dir: None,
        ..sfx_graph::Ctx::default()
    };
    sfx_graph::evaluate(&nodes, &Default::default(), &ctx, false).unwrap_err()
}

#[test]
fn expr_matches_the_equivalent_math_graph() {
    let srcs = vec![
        json!({"type": "osc", "name": "a", "freq": 220, "amp": 0.7}),
        json!({"type": "noise", "name": "b", "color": "pink", "seed": 2}),
    ];
    let mut g = srcs.clone();
    g.push(json!({"type": "math", "name": "m1", "op": "mul", "a": "a", "b": "b"}));
    g.push(json!({"type": "math", "name": "m2", "op": "add", "a": "m1", "b": 0.5}));
    g.push(
        json!({"type": "expr", "name": "e", "inputs": {"a": "a", "b": "b"},
        "out": "a * b + 0.5"}),
    );
    let n = 4800;
    let want = node_samples(&g, "m2", n);
    let got = node_samples(&g, "e", n);
    let worst = want
        .iter()
        .zip(&got)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    // Measured worst difference: 6.0e-8.
    assert!(worst < 1e-6, "worst difference {worst}");
}

#[test]
fn expr_one_pole_with_state_tracks_the_onepole_node() {
    // The onepole node is a TPT filter: with k = g / (1 + g), g = tan(pi fc /
    // sr), the output is lp = s + k (x - s) and the state moves s += 2 k (x -
    // s). That is `state` + `update` written out.
    let sr = 48000.0f64;
    let k = 0.05f64;
    let g = k / (1.0 - k);
    let fc = g.atan() * sr / std::f64::consts::PI;
    let g = vec![
        json!({"type": "noise", "name": "a", "color": "white", "seed": 9}),
        json!({"type": "onepole", "name": "op", "input": "a", "freq": fc}),
        json!({"type": "expr", "name": "e", "inputs": {"a": "a"}, "state": {"s": 0},
            "out": format!("s + {k} * (a - s)"),
            "update": {"s": format!("s + 2 * {k} * (a - s)")}}),
    ];
    let n = 9600;
    let want = node_samples(&g, "op", n);
    let got = node_samples(&g, "e", n);
    let worst = want
        .iter()
        .zip(&got)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    // Measured worst difference: 8.9e-8 (f64 recurrence vs the f32 TPT).
    assert!(worst < 1e-6, "worst difference {worst}");
}

#[test]
fn expr_state_updates_read_the_old_state() {
    // A two-sample delay line: s2 <- s1, s1 <- a. If updates saw each other's
    // new values, s2 would equal s1.
    let g = vec![
        json!({"type": "impulse", "name": "a", "at": [0.0]}),
        json!({"type": "expr", "name": "e", "inputs": {"a": "a"},
            "state": {"s1": 0, "s2": 0}, "out": "s2",
            "update": {"s1": "a", "s2": "s1"}}),
    ];
    let y = node_samples(&g, "e", 8);
    assert_eq!(&y[..4], &[0.0, 0.0, 1.0, 0.0], "{y:?}");
}

#[test]
fn expr_t_sr_and_noise() {
    let g = vec![
        json!({"type": "expr", "name": "t", "out": "t * sr"}),
        json!({"type": "expr", "name": "n", "out": "noise(1)"}),
        json!({"type": "expr", "name": "n2", "out": "noise(1)", "seed": 3}),
    ];
    let t = node_samples(&g, "t", 5);
    assert_eq!(t, [0.0, 1.0, 2.0, 3.0, 4.0]);
    let a = node_samples(&g, "n", 2000);
    assert_eq!(a, node_samples(&g, "n", 2000), "deterministic");
    assert!(a.iter().all(|x| (-1.0..1.0).contains(x)));
    assert!(a.windows(2).any(|w| w[0] != w[1]));
    assert_ne!(a, node_samples(&g, "n2", 2000), "own seed changes it");
}

#[test]
fn expr_sanitizes_non_finite_values() {
    let g = vec![
        json!({"type": "expr", "name": "e", "state": {"s": 1},
        "out": "s", "update": {"s": "1 / 0"}}),
        json!({"type": "expr", "name": "nan", "out": "log(0 - 1)"}),
    ];
    assert_eq!(node_samples(&g, "e", 4), [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(node_samples(&g, "nan", 2), [0.0, 0.0]);
}

#[test]
fn expr_errors_name_the_node_the_expression_and_the_position() {
    let e = eval_error(vec![
        json!({"type": "osc", "name": "src", "freq": 100}),
        json!({"type": "expr", "name": "x", "inputs": {"a": "src"}, "state": {"y1": 0},
            "out": "a + foo * 2"}),
    ]);
    assert!(
        e.starts_with("node `x` (expr): out `a + foo * 2`: unknown name `foo` at 4"),
        "{e}"
    );
    assert!(e.contains("names: t, a, y1, pi, sr"), "{e}");
    let e = eval_error(vec![
        json!({"type": "expr", "name": "x", "state": {"y1": 0},
        "out": "y1", "update": {"y2": "1"}}),
    ]);
    assert!(
        e.contains("update: `y2` is not a state variable (state: y1)"),
        "{e}"
    );
    let e = eval_error(vec![
        json!({"type": "expr", "name": "x", "state": {"y1": 0},
        "out": "y1", "update": {"y1": "sin(y1"}}),
    ]);
    assert!(e.contains("update.y1 `sin(y1`: missing `)`"), "{e}");
    // `out` / `update` are not mangled as $param / =expr fields, and an input
    // is the only thing that counts as a reference.
    let e = eval_error(vec![
        json!({"type": "expr", "name": "x", "inputs": {"a": "nope"},
        "out": "a"}),
    ]);
    assert!(e.contains("reads `nope`, which is not a node"), "{e}");
}

#[test]
fn expr_params_expressions_get_functions_and_ternary() {
    let mut d = SoundDoc::named("t");
    d.params.insert("hz".into(), 300.0);
    d.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": "=$hz > 200 ? max($hz, 440) : 100", "amp": 0.3}),
        json!({"type": "output", "name": "out", "input": "o"}),
    ];
    d.duration = Some(0.1);
    assert!(d.render(&RenderOpts::default()).is_ok());
}

#[test]
fn expr_recipes_from_the_docs_run() {
    let g = vec![
        json!({"type": "osc", "name": "tone", "freq": 110, "amp": 0.9}),
        // Wavefolder.
        json!({"type": "expr", "name": "fold", "inputs": {"a": "tone"},
            "out": "sin(a * 3 * pi / 2)"}),
        // Bouncing ball: hits every `gap` seconds, each gap 0.7x the last.
        json!({"type": "expr", "name": "bounce",
            "state": {"ph": 0, "gap": 0.3, "amp": 1},
            "out": "amp * exp(0 - ph * 40)",
            "update": {"ph": "ph >= gap ? 0 : ph + 1 / sr",
                       "gap": "ph >= gap ? max(gap * 0.7, 0.01) : gap",
                       "amp": "ph >= gap ? amp * 0.7 : amp"}}),
    ];
    let n = 48000;
    let f = node_samples(&g, "fold", n);
    assert!(f.iter().all(|x| x.abs() <= 1.0) && f.iter().any(|x| x.abs() > 0.5));
    let b = node_samples(&g, "bounce", n);
    // Count hits: the envelope jumps up at each bounce.
    let hits = b.windows(2).filter(|w| w[1] > w[0] + 0.05).count();
    assert!((5..=30).contains(&hits), "{hits} bounces");
    assert!(b[0] > 0.99 && b.iter().all(|x| (0.0..=1.0).contains(x)));
}

// ------------------------------------------------------------ video frames

#[test]
fn frame_times_resolve_at_the_document_fps_and_need_one() {
    let hit = |at: Value| {
        vec![
            json!({"type": "impulse", "name": "i", "at": at}),
            json!({"type": "output", "name": "out", "input": "i", "fade_in": 0.0005}),
        ]
    };
    let mut d = doc_with(hit(json!("f:12")));
    d.duration = Some(1.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    assert!(e.contains("no fps"), "{e}");
    d.fps = Some(24.0);
    let first = |d: &SoundDoc| {
        let r = d.render(&RenderOpts::default()).unwrap();
        r.audio.channels[0]
            .iter()
            .position(|x| x.abs() > 0.1)
            .unwrap()
    };
    assert_eq!(first(&d), 24000); // frame 12 at 24 fps = 0.5 s at 48 kHz
    d.nodes = hit(json!(["0.25", "6f"]));
    d.nodes[0]["at"] = json!([0.75, "6f"]);
    let r = d.render(&RenderOpts::default()).unwrap();
    assert!(r.audio.channels[0][12000].abs() > 0.1); // 6f = 0.25 s
    // The same graph in seconds renders the same bits.
    let mut s = doc_with(hit(json!(0.5)));
    s.duration = Some(1.0);
    let mut f = doc_with(hit(json!("12f")));
    f.duration = Some(1.0);
    f.fps = Some(24.0);
    assert_eq!(
        s.render(&RenderOpts::default()).unwrap().audio.channels,
        f.render(&RenderOpts::default()).unwrap().audio.channels
    );
    // Fractional rates.
    assert!((sfx_graph::Time::Frames(30.0).seconds(Some(29.97)).unwrap() - 1.001).abs() < 1e-3);
    // Node names that look like frames are still names unless they match.
    assert_eq!(sfx_graph::params::frames_of("f:abc"), None);
    assert_eq!(sfx_graph::params::frames_of("f"), None);
    assert_eq!(sfx_graph::params::frames_of(" f:7 "), Some(7.0));
}

#[test]
fn fps_is_set_and_cleared_by_an_edit_and_is_absent_by_default() {
    let mut d = doc_with(vec![]);
    assert!(!serde_json::to_string(&d).unwrap().contains("fps"));
    let edit = DocEdit {
        fps: Some(Some(0.0)),
        ..DocEdit::default()
    };
    let e = d.apply(vec![], edit, &RenderOpts::default()).unwrap_err();
    assert!(e.contains("fps"), "{e}");
    let edit = DocEdit {
        fps: Some(Some(25.0)),
        ..DocEdit::default()
    };
    d.apply(vec![], edit, &RenderOpts::default()).unwrap();
    assert_eq!(d.fps, Some(25.0));
    let back: SoundDoc = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back.fps, Some(25.0));
}
