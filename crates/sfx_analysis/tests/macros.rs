//! Macros: optional claims, knobs that write doc params (and so reach inside
//! a `voices` patch), replacing a macro in one call, sweep failures reported
//! when the macro is made.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_analysis::{CheckCx, Expect, run};
use sfx_graph::{Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, Params, RenderOpts, SoundDoc};

/// Three events, each a sine voice whose pitch is `200 + $size * 6000` Hz:
/// the knob only reaches the voice through the patch's `map`.
fn voices_doc() -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.duration = Some(0.8);
    d.params.insert("size".into(), 0.5);
    d.nodes = vec![
        json!({"type": "pattern", "name": "ev", "times": [0.05, 0.3, 0.55]}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.2,
               "map": {"hz": "200 + $size * 6000"},
               "patch": [
                   {"type": "osc", "name": "o", "freq": "$hz", "amp": 0.5},
                   {"type": "decay", "name": "e", "time": 0.08},
                   {"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}]}),
        json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
    ];
    d
}

fn param_entry(param: &str, from: f64, to: f64, curve: MapCurve) -> MapEntry {
    MapEntry {
        node: String::new(),
        field: String::new(),
        param: Some(param.into()),
        from,
        to,
        curve,
        col: None,
        index: None,
        key: None,
    }
}

fn knob(name: &str, claim: Option<Claim>, map: Vec<MapEntry>) -> Macro {
    Macro {
        name: name.into(),
        value: 0.5,
        default: None,
        doc: String::new(),
        claim,
        game: false,
        map,
    }
}

fn edit_macros(list: Vec<Macro>) -> DocEdit {
    DocEdit {
        macros: Some(list),
        ..DocEdit::default()
    }
}

fn centroid_at(d: &SoundDoc, name: &str, x: f64) -> f32 {
    let r = d
        .render(&RenderOpts {
            macros: BTreeMap::from([(name.to_string(), x)]),
            ..RenderOpts::default()
        })
        .unwrap();
    measure(&r.audio, Measure::Centroid)
}

#[test]
fn a_macro_driving_a_param_inside_a_voices_map_moves_the_centroid_monotonically() {
    let mut d = voices_doc();
    let rep = d
        .apply(
            vec![],
            edit_macros(vec![knob(
                "size",
                Some(Claim(Measure::Centroid, 1)),
                vec![param_entry("size", 0.0, 1.0, MapCurve::Linear)],
            )]),
            &RenderOpts::default(),
        )
        .unwrap();
    println!("apply:\n{}", rep.text);
    let cs: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|x| centroid_at(&d, "size", *x))
        .collect();
    println!("centroid over the knob: {cs:?}");
    assert!(cs.windows(2).all(|w| w[1] > w[0] * 1.2), "{cs:?}");
    // Bit-identical on a second render, and the cache does not return the
    // render of another knob position.
    let a = d.render(&RenderOpts::default()).unwrap();
    let b = d
        .render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap();
    assert!(a.audio.channels == b.audio.channels);
    // verify's macros check holds the claim.
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
    let line = res.iter().find(|c| c.name == "macro size").unwrap();
    println!("verify: {}", line.detail);
    assert!(line.pass, "{line:?}");
}

#[test]
fn a_param_macro_also_drives_top_level_fields_that_read_the_param() {
    let mut d = SoundDoc::named("t");
    d.duration = Some(0.3);
    d.params.insert("hz".into(), 300.0);
    d.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": "$hz", "amp": 0.5}),
        json!({"type": "output", "name": "out", "input": "o", "lufs": -20}),
    ];
    d.apply(
        vec![],
        edit_macros(vec![knob(
            "pitch",
            None,
            vec![param_entry("hz", 200.0, 3200.0, MapCurve::Exp)],
        )]),
        &RenderOpts::default(),
    )
    .unwrap();
    let (lo, hi) = (centroid_at(&d, "pitch", 0.0), centroid_at(&d, "pitch", 1.0));
    println!("pitch knob centroid: {lo} -> {hi} Hz");
    assert!(lo < 400.0 && hi > 2500.0, "{lo} {hi}");
    // The param's own value is overridden: editing it warns and changes nothing.
    let before = d.render(&RenderOpts::default()).unwrap();
    let rep = d
        .apply(
            vec![],
            DocEdit {
                params: Some(Params::from([("hz".to_string(), 5000.0)])),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap();
    println!("{}", rep.text);
    assert!(
        rep.text.contains("param `hz` is driven by macro `pitch`")
            && rep.text.contains("your value is ignored"),
        "{}",
        rep.text
    );
    assert!(rep.rendered.audio.channels == before.audio.channels);
}

#[test]
fn two_macros_may_not_write_the_same_param() {
    let mut d = voices_doc();
    let err = d
        .apply(
            vec![],
            edit_macros(vec![
                knob(
                    "size",
                    None,
                    vec![param_entry("size", 0.0, 1.0, MapCurve::Linear)],
                ),
                knob(
                    "big",
                    None,
                    vec![param_entry("size", 0.2, 0.9, MapCurve::Linear)],
                ),
            ]),
            &RenderOpts::default(),
        )
        .unwrap_err();
    println!("{err}");
    assert!(
        err.contains("macro `size`")
            && err.contains("macro `big`")
            && err.contains("param $size")
            && err.contains("split the param or drop one"),
        "{err}"
    );
    assert!(d.macros.is_empty());
}

#[test]
fn a_param_entry_names_unknown_params_and_refuses_mixed_targets() {
    let mut d = voices_doc();
    let err = d
        .apply(
            vec![],
            edit_macros(vec![knob(
                "k",
                None,
                vec![param_entry("nope", 0.0, 1.0, MapCurve::Linear)],
            )]),
            &RenderOpts::default(),
        )
        .unwrap_err();
    println!("{err}");
    assert!(
        err.contains("macro `k`") && err.contains("no param named `nope`") && err.contains("size"),
        "{err}"
    );
    let mut both = param_entry("size", 0.0, 1.0, MapCurve::Linear);
    both.node = "v".into();
    both.field = "gain".into();
    let err = d
        .apply(
            vec![],
            edit_macros(vec![knob("k2", None, vec![both])]),
            &RenderOpts::default(),
        )
        .unwrap_err();
    println!("{err}");
    assert!(
        err.contains("macro `k2`") && err.contains("one or the other"),
        "{err}"
    );
    // The same from JSON.
    let e = serde_json::from_value::<Macro>(json!({"name": "k3", "map": [{"from": 0, "to": 1}]}))
        .unwrap_err()
        .to_string();
    println!("{e}");
    assert!(e.contains("macro `k3`") && e.contains("param"), "{e}");
}

#[test]
fn without_a_claim_there_is_no_sweep_and_no_failure() {
    let mut d = voices_doc();
    d.apply(
        vec![],
        edit_macros(vec![knob(
            "size",
            None,
            vec![param_entry("size", 0.0, 1.0, MapCurve::Linear)],
        )]),
        &RenderOpts::default(),
    )
    .unwrap();
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
    println!("{res:?}");
    assert!(
        !res.iter().any(|c| c.name == "macro size"),
        "a claimless macro was swept: {res:?}"
    );
    assert!(res.iter().all(|c| c.pass), "{res:?}");
    let line = res.iter().find(|c| c.name == "macros").unwrap();
    println!("{}", line.detail);
    assert!(line.detail.contains("1 macros unclaimed") && line.detail.contains("size"));
}

#[test]
fn a_claimless_macro_serializes_without_a_claim_and_round_trips() {
    let m: Macro = serde_json::from_value(json!({
        "name": "grit", "map": [{"node": "f", "field": "freq", "from": 1, "to": 2}]
    }))
    .unwrap();
    assert!(m.claim.is_none());
    let v = serde_json::to_value(&m).unwrap();
    assert!(v.get("claim").is_none(), "{v}");
    assert_eq!(serde_json::from_value::<Macro>(v).unwrap(), m);
    // null is accepted too.
    let n: Macro = serde_json::from_value(json!({
        "name": "grit", "claim": null,
        "map": [{"node": "f", "field": "freq", "from": 1, "to": 2}]
    }))
    .unwrap();
    assert!(n.claim.is_none());
}

#[test]
fn a_claimless_macro_with_bad_json_names_the_macro() {
    let bad = |v: Value| serde_json::from_value::<Macro>(v).unwrap_err().to_string();
    let e =
        bad(json!({"name": "grit", "map": [{"node": "f", "field": "freq", "frm": 1, "to": 2}]}));
    println!("{e}");
    assert!(e.contains("macro `grit`") && e.contains("frm"), "{e}");
    let e = bad(json!({"name": "warmth", "map": "f.freq"}));
    println!("{e}");
    assert!(e.contains("macro `warmth`"), "{e}");
    let e = bad(json!({"name": "snap", "claim": ["centr", 1], "map": []}));
    println!("{e}");
    assert!(e.contains("macro `snap`") && e.contains("centr"), "{e}");
    let e = bad(json!({"name": "air"}));
    println!("{e}");
    assert!(e.contains("macro `air`") && e.contains("map"), "{e}");
    let e = bad(json!({"map": []}));
    println!("{e}");
    assert!(e.contains("macro") && e.contains("name"), "{e}");
}

#[test]
fn delete_and_define_the_same_macro_in_one_call_replaces_it() {
    let mut d = voices_doc();
    d.apply(
        vec![],
        edit_macros(vec![knob(
            "size",
            None,
            vec![param_entry("size", 0.0, 1.0, MapCurve::Linear)],
        )]),
        &RenderOpts::default(),
    )
    .unwrap();
    let mut new = knob(
        "size",
        Some(Claim(Measure::Centroid, 1)),
        vec![param_entry("size", 0.4, 0.6, MapCurve::Linear)],
    );
    new.doc = "the new one".into();
    let rep = d
        .apply(
            vec![],
            DocEdit {
                macros: Some(vec![new]),
                delete_macros: vec!["size".into()],
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap();
    assert!(rep.changed);
    assert_eq!(d.macros.len(), 1);
    assert_eq!(d.macros[0].doc, "the new one");
    assert_eq!(d.macros[0].map[0].to, 0.6);
    assert!(d.macros[0].claim.is_some());
    // Deleting a name that exists nowhere is still an error.
    let err = d
        .apply(
            vec![],
            DocEdit {
                delete_macros: vec!["ghost".into()],
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap_err();
    assert!(err.contains("ghost"), "{err}");
    // And a plain delete still deletes.
    d.apply(
        vec![],
        DocEdit {
            delete_macros: vec!["size".into()],
            ..DocEdit::default()
        },
        &RenderOpts::default(),
    )
    .unwrap();
    assert!(d.macros.is_empty());
}

#[test]
fn a_sweep_that_fails_to_render_is_reported_when_the_macro_is_made() {
    // A poisson stream whose rate the knob raises: 0.5 s at up to 4000/s
    // is far over the voices max (50), so the render fails past the start.
    let mut d = SoundDoc::named("t");
    d.duration = Some(0.5);
    d.params.insert("rate".into(), 20.0);
    d.nodes = vec![
        json!({"type": "poisson", "name": "ev", "rate": "$rate", "seed": 3}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.02, "max": 50,
               "patch": [
                   {"type": "noise", "name": "n", "amp": 0.3},
                   {"type": "decay", "name": "e", "time": 0.01},
                   {"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}]}),
        json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
    ];
    let err = d
        .apply(
            vec![],
            edit_macros(vec![knob(
                "density",
                Some(Claim(Measure::Lufs, 1)),
                vec![param_entry("rate", 20.0, 4000.0, MapCurve::Exp)],
            )]),
            &RenderOpts::default(),
        )
        .unwrap_err();
    println!("{err}");
    assert!(
        err.contains("macro `density` at ")
            && err.contains("events but max is 50")
            && err.contains("delete_macros"),
        "{err}"
    );
    assert!(d.macros.is_empty(), "the failed call changed nothing");
    // A range that renders at every position is accepted.
    d.apply(
        vec![],
        edit_macros(vec![knob(
            "density",
            None,
            vec![param_entry("rate", 20.0, 60.0, MapCurve::Linear)],
        )]),
        &RenderOpts::default(),
    )
    .unwrap();
}

#[test]
fn node_field_macros_behave_as_before() {
    let mut d = SoundDoc::named("t");
    d.duration = Some(0.3);
    d.nodes = vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "filter", "name": "f", "input": "n", "mode": "lp", "freq": 1000}),
        json!({"type": "output", "name": "out", "input": "f", "lufs": -20}),
    ];
    let mut m = knob(
        "brightness",
        Some(Claim(Measure::Centroid, 1)),
        vec![MapEntry {
            node: "f".into(),
            field: "freq".into(),
            param: None,
            from: 300.0,
            to: 8000.0,
            curve: MapCurve::Exp,
            col: None,
            index: None,
            key: None,
        }],
    );
    m.doc = "cutoff".into();
    d.apply(vec![], edit_macros(vec![m]), &RenderOpts::default())
        .unwrap();
    let (lo, hi) = (
        centroid_at(&d, "brightness", 0.0),
        centroid_at(&d, "brightness", 1.0),
    );
    println!("brightness centroid {lo} -> {hi}");
    assert!(hi > lo * 2.0);
    // Two macros on one node field are still refused.
    let dup = knob(
        "weight",
        None,
        vec![MapEntry {
            node: "f".into(),
            field: "freq".into(),
            param: None,
            from: 100.0,
            to: 200.0,
            curve: MapCurve::Linear,
            col: None,
            index: None,
            key: None,
        }],
    );
    let err = d
        .apply(vec![], edit_macros(vec![dup]), &RenderOpts::default())
        .unwrap_err();
    assert!(err.contains("both write f.freq"), "{err}");
}
