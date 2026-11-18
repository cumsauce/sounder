//! A string in a signal field that contains `$` or is arithmetic on
//! numbers is an expression exactly as if it started with `=`.

use serde_json::{Value, json};
use sfx_graph::{Buf, Ctx, Params, RenderOpts, SoundDoc, evaluate};

fn doc(nodes: Vec<Value>, secs: f32) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(secs);
    d
}

fn render(nodes: Vec<Value>) -> Result<Vec<f32>, String> {
    let d = doc(nodes, 1.0);
    let r = d.render(&RenderOpts::default())?;
    Ok(r.audio.channels[0].clone())
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |a, b| a.max(b.abs()))
}

/// The three round-3 examples, each inside a `voices` patch.
fn voices_doc(patch: Value) -> Vec<Value> {
    vec![
        json!({"type": "pattern", "name": "ev", "times": [0.1, 0.4, 0.7], "values": {"min": 0.3, "max": 1.0}, "seed": 2}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.2, "patch": patch}),
        json!({"type": "output", "name": "out", "input": "v"}),
    ]
}

#[test]
fn round_three_examples_render_with_and_without_the_equals() {
    let patches = [
        // gain from the event value
        json!([
            {"type": "noise", "name": "n"},
            {"type": "gain", "name": "g", "input": "n", "gain": "0.1 + $value * 0.9"}
        ]),
        // contact speed from the event value
        json!([
            {"type": "contact", "name": "c", "speed": "1.5 * $value", "mass": 0.1, "material": "wood"}
        ]),
        // amplitude from the voice's random draw
        json!([
            {"type": "osc", "name": "o", "freq": 440, "amp": "2*$rand"}
        ]),
    ];
    for p in patches {
        let bare = render(voices_doc(p.clone())).unwrap_or_else(|e| panic!("{p}: {e}"));
        let text = p.to_string();
        // The same patch with each expression written with a leading `=`.
        let eq = text
            .replace("\"0.1 + $value * 0.9\"", "\"=0.1 + $value * 0.9\"")
            .replace("\"1.5 * $value\"", "\"=1.5 * $value\"")
            .replace("\"2*$rand\"", "\"=2*$rand\"");
        let with_eq = render(voices_doc(serde_json::from_str(&eq).unwrap())).unwrap();
        println!("peak {:.4} (len {})", peak(&bare), bare.len());
        assert!(peak(&bare) > 1e-4, "silent: {p}");
        assert_eq!(bare, with_eq, "bare and `=` forms differ: {p}");
    }
}

#[test]
fn the_expression_is_evaluated_per_event() {
    // amp = 1 * value: a louder event gives a louder voice.
    let nodes = vec![
        json!({"type": "pattern", "name": "ev", "times": [0.1], "values": {"min": 0.25, "max": 0.25}}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.1,
               "patch": [{"type": "osc", "name": "o", "freq": 200, "amp": "4 * $value"}]}),
    ];
    let ctx = Ctx {
        n: 48_000,
        ..Ctx::default()
    };
    let ev = evaluate(&nodes, &Params::new(), &ctx, false).unwrap();
    let Buf::Audio(a) = &*ev.buf("v").unwrap() else {
        panic!("not mono audio");
    };
    let p = peak(a);
    println!("peak with amp 4*0.25: {p:.4}");
    assert!((p - 1.0).abs() < 0.05, "{p}");
}

#[test]
fn document_level_strings_work_too() {
    let mut d = doc(
        vec![
            json!({"type": "osc", "name": "o", "freq": "100 * $k", "amp": "0.5"}),
            json!({"type": "output", "name": "out", "input": "o"}),
        ],
        0.5,
    );
    d.params.insert("k".into(), 4.0);
    let a = d.render(&RenderOpts::default()).unwrap();
    let x = a.audio.mix_mono();
    let mut v: Vec<f32> = sfx_analysis::pitch::yin(&x[4000..20_000], 48_000)
        .iter()
        .filter_map(|f| f.hz)
        .collect();
    v.sort_by(f32::total_cmp);
    println!("pitch {v:?}");
    assert!(
        !v.is_empty() && (v[v.len() / 2] - 400.0).abs() < 8.0,
        "{v:?}"
    );
}

#[test]
fn names_with_dots_and_underscores_still_resolve() {
    let nodes = vec![
        json!({"type": "noise", "name": "src_1"}),
        json!({"type": "filter", "name": "lp_2", "input": "src_1", "mode": "lp", "freq": 2000}),
        json!({"type": "reverb", "name": "verb", "input": "lp_2"}),
        json!({"type": "gain", "name": "g", "input": "verb.wet", "gain": 0.5}),
        json!({"type": "output", "name": "out", "input": "g"}),
    ];
    let a = render(nodes).unwrap();
    assert!(peak(&a) > 1e-4);
    // And a node named like a number-ish token stays a node.
    let nodes = vec![
        json!({"type": "noise", "name": "n2"}),
        json!({"type": "output", "name": "out", "input": "n2"}),
    ];
    assert!(peak(&render(nodes).unwrap()) > 1e-4);
}

#[test]
fn unknown_names_carry_the_equals_hint() {
    let e = render(vec![
        json!({"type": "noise", "name": "n"}),
        json!({"type": "gain", "name": "g", "input": "n", "gain": "nope"}),
        json!({"type": "output", "name": "out", "input": "g"}),
    ])
    .unwrap_err();
    println!("{e}");
    assert!(e.contains("reads `nope`, which is not a node"), "{e}");
    assert!(
        e.contains("not a node; write expressions with a leading `=`, e.g. \"=1.5 * $value\""),
        "{e}"
    );
}

#[test]
fn refs_do_not_report_expression_text() {
    let body = json!({"input": "src", "gain": "0.1 + $value * 0.9", "freq": "2*$rand",
        "a": "1.5 * $x", "b": "2*3", "c": "$p", "d": "=1+2", "e": "verb.wet"});
    let mut r: Vec<(String, String)> = sfx_graph::node::refs_typed("gain", &body);
    r.sort();
    assert_eq!(
        r,
        [
            ("src".to_string(), String::new()),
            ("verb".to_string(), "wet".to_string())
        ]
    );
}

#[test]
fn is_expr_classification() {
    use sfx_graph::params::is_expr;
    for s in [
        "$a",
        "=1",
        "1.5 * $value",
        "0.1 + $value*0.9",
        "2*$rand",
        "2*3",
        "-1",
        "1.5",
        "(1+2)/3",
        "max(1, $a)",
    ] {
        assert!(is_expr(s), "{s}");
    }
    for s in [
        "lp",
        "a_1",
        "verb.wet",
        "n2",
        "7",
        "kick.wav",
        "sounds/x.wav",
        "mono",
        "wood",
        "",
    ] {
        assert!(!is_expr(s), "{s}");
    }
}
