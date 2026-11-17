//! The graph: determinism, strict parsing, atomic edits, lengths.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

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
    assert!(e.contains("cycle") && e.contains("a -> b -> a"), "{e}");
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
    // -60 dB at 0.5 s, -80 dB at ~0.67 s, plus 10 ms kept.
    let s = r.audio.seconds();
    assert!(r.auto_length && (0.6..0.75).contains(&s), "{s}");
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
fn params_inputs_and_expressions_resolve() {
    let mut d = doc_with(vec![
        json!({"type": "osc", "name": "o", "freq": "=$base * 2"}),
        json!({"type": "output", "name": "out", "input": "o", "gain_db": "$level"}),
    ]);
    d.params.insert("base".into(), 220.0);
    d.inputs.insert(
        "level".into(),
        sfx_graph::InputSpec {
            default: -12.0,
            min: Some(-40.0),
            max: Some(0.0),
            doc: None,
        },
    );
    d.duration = Some(0.2);
    let r = d.render(&RenderOpts::default()).unwrap();
    assert!((r.peak_db + 12.0).abs() < 0.1, "{}", r.peak_db);
    // Overrides are clamped to the input's range.
    let loud = d
        .render(&RenderOpts {
            inputs: [("level".to_string(), 30.0)].into(),
            ..RenderOpts::default()
        })
        .unwrap();
    assert!(
        loud.peak_db <= -0.99,
        "clamped to 0 dB then the -1 dB ceiling: {}",
        loud.peak_db
    );
    assert!(
        d.render(&RenderOpts {
            inputs: [("nope".to_string(), 1.0)].into(),
            ..RenderOpts::default()
        })
        .unwrap_err()
        .contains("no input named `nope`")
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
    ];
    for t in sfx_graph::NODE_TYPES {
        if matches!(*t, "sample" | "convolve" | "output") {
            continue; // need files / are the output
        }
        let mut ex = sfx_graph::example(t).unwrap();
        ex["name"] = json!("under_test");
        let mut nodes = sources.clone();
        nodes.retain(|n| n["name"] != "under_test");
        nodes.push(ex);
        // Every stand-in and the node under test feed the output.
        let names: Vec<Value> = nodes.iter().map(|n| n["name"].clone()).collect();
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
