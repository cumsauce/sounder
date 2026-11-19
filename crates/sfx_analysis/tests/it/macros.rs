mod claims {
    //! Macros: optional claims, knobs that write doc params (and so reach inside
    //! a `voices` patch), replacing a macro in one call, sweep failures reported
    //! when the macro is made.

    use std::collections::BTreeMap;

    use serde_json::{Value, json};
    use sfx_analysis::measure::measure;
    use sfx_analysis::{CheckCx, Expect, run};
    use sfx_graph::{
        Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, Params, RenderOpts, SoundDoc,
    };

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
            err.contains("macro `k`")
                && err.contains("no param named `nope`")
                && err.contains("size"),
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
        let e =
            serde_json::from_value::<Macro>(json!({"name": "k3", "map": [{"from": 0, "to": 1}]}))
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
        let e = bad(
            json!({"name": "grit", "map": [{"node": "f", "field": "freq", "frm": 1, "to": 2}]}),
        );
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
}

mod ring {
    //! The `ring` macro claim, the median over the strong onsets of each
    //! hit's own -20 dB fall (ms), so a knob on a modal `loss` reads the same in
    //! a single hit, a knock and a strike followed by a later ring.

    use std::collections::BTreeMap;

    use serde_json::{Value, json};
    use sfx_analysis::measure::{measure, ring_ms};
    use sfx_graph::{Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc};

    fn entry(node: &str, field: &str, from: f64, to: f64, curve: MapCurve) -> MapEntry {
        MapEntry {
            node: node.into(),
            field: field.into(),
            param: None,
            from,
            to,
            curve,
            col: None,
            index: None,
            key: None,
        }
    }

    fn doc_with(nodes: Vec<Value>, secs: f32, claim: Claim, map: Vec<MapEntry>) -> SoundDoc {
        let mut d = SoundDoc::named("t");
        d.duration = Some(secs);
        d.nodes = nodes;
        let knob = Macro {
            name: "knob".into(),
            value: 0.5,
            default: None,
            doc: String::new(),
            claim: Some(claim),
            game: false,
            map,
        };
        d.apply(
            vec![],
            DocEdit {
                macros: Some(vec![knob]),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap();
        d
    }

    fn at(d: &SoundDoc, x: f64) -> sfx_graph::Audio {
        d.render(&RenderOpts {
            macros: BTreeMap::from([("knob".to_string(), x)]),
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap()
        .audio
    }

    const POS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

    fn rings(d: &SoundDoc) -> Vec<f32> {
        POS.iter().map(|x| ring_ms(&at(d, *x))).collect()
    }

    fn out(input: &str) -> Value {
        json!({"type": "output", "name": "out", "input": input, "peak_db": -3, "fade_in": 0.0005})
    }

    /// A modal wood plate at 400 Hz struck at each time; `loss` is the knob.
    fn plate_doc(times: Value, secs: f32) -> SoundDoc {
        doc_with(
            vec![
                json!({"type": "impulse", "name": "hit", "at": times}),
                json!({"type": "modal", "name": "body", "excite": "hit", "material": "wood", "f0": 400}),
                out("body"),
            ],
            secs,
            Claim(Measure::Ring, 1),
            // knob 0 -> loss 3 (short), 1 -> loss 0.3 (long): ring rises.
            vec![entry("body", "loss", 3.0, 0.3, MapCurve::Exp)],
        )
    }

    #[test]
    fn ring_falls_with_modal_loss_for_a_single_hit() {
        let d = plate_doc(json!([0.01]), 1.5);
        let r = rings(&d);
        println!("single hit, ring over knob (loss 3 -> 0.3): {r:?}");
        assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
        assert!(r[4] > 4.0 * r[0], "{r:?}");
        // The claim holds under verify's macros sweep.
        let opts = RenderOpts::default();
        let rendered = d.render(&opts).unwrap();
        let res = sfx_analysis::run(
            &sfx_analysis::Expect::defaults_for(&d),
            &sfx_analysis::CheckCx {
                doc: &d,
                rendered: &rendered,
                opts: &opts,
            },
        )
        .unwrap();
        let line = res.iter().find(|c| c.name == "macro knob").unwrap();
        println!("verify: {}", line.detail);
        assert!(line.pass, "{line:?}");
    }

    #[test]
    fn ring_falls_with_modal_loss_for_a_three_hit_knock() {
        let d = plate_doc(json!([0.02, 0.22, 0.42]), 1.5);
        let r = rings(&d);
        println!("knock (0.2 s), ring over knob: {r:?}");
        assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
    }

    #[test]
    fn ring_falls_with_modal_loss_for_a_strike_and_a_later_ring() {
        // A short wooden strike at 0.02 s, a metal ring at 0.5 s; one knob scales
        // both losses.
        let d = doc_with(
            vec![
                json!({"type": "impulse", "name": "k1", "at": [0.02]}),
                json!({"type": "impulse", "name": "k2", "at": [0.5]}),
                json!({"type": "modal", "name": "strike", "excite": "k1", "material": "wood", "f0": 250}),
                json!({"type": "modal", "name": "bell", "excite": "k2", "material": "metal", "f0": 1500}),
                json!({"type": "mix", "name": "both", "inputs": ["strike", "bell"]}),
                out("both"),
            ],
            2.5,
            Claim(Measure::Ring, 1),
            vec![
                entry("strike", "loss", 3.0, 0.3, MapCurve::Exp),
                entry("bell", "loss", 3.0, 0.3, MapCurve::Exp),
            ],
        );
        let r = rings(&d);
        println!("strike + ring, ring over knob: {r:?}");
        assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
    }

    #[test]
    fn ring_rises_with_a_decay_node_time() {
        let d = doc_with(
            vec![
                json!({"type": "noise", "name": "n", "color": "white", "seed": 1}),
                json!({"type": "decay", "name": "e", "time": 0.05}),
                json!({"type": "math", "name": "hit", "op": "mul", "a": "n", "b": "e"}),
                out("hit"),
            ],
            1.0,
            Claim(Measure::Ring, 1),
            vec![entry("e", "time", 0.02, 0.3, MapCurve::Exp)],
        );
        let r = rings(&d);
        println!("noise * decay, ring over knob: {r:?}");
        assert!(r.windows(2).all(|w| w[1] > w[0]), "{r:?}");
    }

    #[test]
    fn ring_is_a_measure_with_a_name_and_a_parseable_claim() {
        assert_eq!(Measure::Ring.name(), "ring");
        assert_eq!(Measure::Ring.unit(), "ms");
        let c: Claim = serde_json::from_value(json!(["ring", 1])).unwrap();
        assert_eq!(c, Claim(Measure::Ring, 1));
        // Silence has no hits and no decay.
        let silent = sfx_graph::Audio {
            sample_rate: 48_000,
            channels: vec![vec![0.0; 4800]],
        };
        assert_eq!(measure(&silent, Measure::Ring), 0.0);
    }
}

mod patch_paths {
    //! Macros reach into `voices` patches (`voices/patch_node`), address
    //! points of an `env` and the state of an `expr`, and `contact` / `modal`
    //! take `gain_db`.

    use std::collections::BTreeMap;

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::measure::measure;
    use sfx_graph::{DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc};

    fn entry(node: &str, field: &str, from: f64, to: f64) -> MapEntry {
        MapEntry {
            node: node.into(),
            field: field.into(),
            param: None,
            from,
            to,
            curve: MapCurve::Linear,
            col: None,
            index: None,
            key: None,
        }
    }

    fn knob(name: &str, map: Vec<MapEntry>) -> Macro {
        Macro {
            name: name.into(),
            value: 0.5,
            default: None,
            doc: String::new(),
            claim: None,
            game: false,
            map,
        }
    }

    fn with_knob(nodes: Vec<Value>, duration: f32, m: Macro) -> Result<SoundDoc, String> {
        let mut d = SoundDoc::named("t");
        d.duration = Some(duration);
        d.nodes = nodes;
        d.apply(
            vec![],
            DocEdit {
                macros: Some(vec![m]),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )?;
        Ok(d)
    }

    fn at(d: &SoundDoc, name: &str, x: f64) -> sfx_graph::Audio {
        d.render(&RenderOpts {
            macros: BTreeMap::from([(name.to_string(), x)]),
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap()
        .audio
    }

    fn shards() -> Vec<Value> {
        vec![
            json!({"type": "pattern", "name": "ev", "times": [0.02, 0.2, 0.4]}),
            json!({"type": "voices", "name": "v", "events": "ev", "length": 0.3,
               "patch": [
                   {"type": "impulse", "name": "tap", "at": [0.0]},
                   {"type": "modal", "name": "piece", "excite": "tap",
                    "material": "glass", "size": 0.3, "thickness": 0.003}]}),
            json!({"type": "output", "name": "out", "input": "v", "peak_db": -6}),
        ]
    }

    #[test]
    fn a_macro_on_a_voices_patch_node_moves_the_centroid_monotonically() {
        // Smaller piece, higher pitch: size 0.6 -> 0.1 m.
        let d = with_knob(
            shards(),
            0.8,
            knob("shrink", vec![entry("v/piece", "size", 0.6, 0.1)]),
        )
        .unwrap();
        let cs: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|x| measure(&at(&d, "shrink", *x), Measure::Centroid))
            .collect();
        println!("centroid over v/piece.size: {cs:?}");
        assert!(cs.windows(2).all(|w| w[1] > w[0] * 1.05), "{cs:?}");
    }

    #[test]
    fn a_bad_patch_path_errors_with_the_patch_nodes() {
        let e = with_knob(
            shards(),
            0.8,
            knob("k", vec![entry("v/nope", "size", 0.6, 0.1)]),
        )
        .err()
        .unwrap();
        println!("{e}");
        assert!(
            e.contains("tap") && e.contains("piece") && e.contains("`v`"),
            "{e}"
        );
        let e = with_knob(
            shards(),
            0.8,
            knob("k", vec![entry("ev/x", "size", 0.6, 0.1)]),
        )
        .err()
        .unwrap();
        println!("{e}");
        assert!(e.contains("not a voices node"), "{e}");
        // A patch node's bare name still errors, and now says how to address it.
        let e = with_knob(
            shards(),
            0.8,
            knob("k", vec![entry("piece", "size", 0.6, 0.1)]),
        )
        .err()
        .unwrap();
        assert!(e.contains("v/piece"), "{e}");
    }

    fn peak_time(x: &[f32], sr: u32) -> f32 {
        let (k, _) = x
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap();
        k as f32 / sr as f32
    }

    fn swell(extra_key: &str) -> (SoundDoc, f32) {
        // y runs 0.4 -> 1.0; x (time) 0.1 -> 0.5, both inside the neighbours.
        let (from, to) = if extra_key == "time" || extra_key == "x" {
            (0.1, 0.5)
        } else {
            (0.4, 1.0)
        };
        let nodes = vec![
            json!({"type": "osc", "name": "tone", "freq": 220, "amp": 0.5}),
            json!({"type": "env", "name": "shape",
               "points": [[0, 0], [0.2, 0.4], [0.6, 0.8], [1.0, 0]]}),
            json!({"type": "math", "name": "note", "op": "mul", "a": "tone", "b": "shape"}),
            json!({"type": "output", "name": "out", "input": "note"}),
        ];
        let mut m = entry("shape", "points", from, to);
        m.index = Some(1);
        m.key = Some(extra_key.into());
        (with_knob(nodes, 1.0, knob("early", vec![m])).unwrap(), 0.0)
    }

    #[test]
    fn an_env_point_y_mapped_moves_the_peak_time() {
        let (d, _) = swell("y");
        let ts: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|x| {
                let a = at(&d, "early", *x);
                peak_time(&a.channels[0], a.sample_rate)
            })
            .collect();
        println!("peak time over the point's y: {ts:?}");
        assert!(ts[0] > 0.5 && ts[4] < 0.3, "{ts:?}");
        assert!(ts.windows(2).all(|w| w[1] <= w[0] + 0.01), "{ts:?}");
        // `value` is the same number as `y`.
        let (d2, _) = swell("value");
        assert_eq!(
            at(&d, "early", 0.9).channels,
            at(&d2, "early", 0.9).channels
        );
    }

    #[test]
    fn an_env_point_x_by_time_and_a_bad_key_lists_the_names() {
        let (d, _) = swell("time");
        let a = at(&d, "early", 0.0);
        let b = at(&d, "early", 1.0);
        assert_ne!(a.channels, b.channels);
        let (dx, _) = swell("x");
        assert_eq!(
            at(&dx, "early", 0.3).channels,
            at(&d, "early", 0.3).channels
        );
        let nodes = vec![
            json!({"type": "osc", "name": "tone", "freq": 220}),
            json!({"type": "env", "name": "shape", "points": [[0, 0], [0.5, 1]]}),
            json!({"type": "math", "name": "note", "op": "mul", "a": "tone", "b": "shape"}),
        ];
        let mut m = entry("shape", "points", 0.1, 0.4);
        m.index = Some(1);
        m.key = Some("level".into());
        let e = with_knob(nodes, 1.0, knob("k", vec![m])).err().unwrap();
        println!("{e}");
        assert!(e.contains("x, time") && e.contains("y, value"), "{e}");
    }

    #[test]
    fn an_expr_state_constant_mapped_changes_the_output() {
        let nodes = vec![
            json!({"type": "osc", "name": "tone", "freq": 300, "amp": 0.5}),
            json!({"type": "expr", "name": "scaled", "inputs": {"a": "tone"},
               "state": {"g": 0.1}, "out": "a * g", "update": {"g": "g"}}),
            json!({"type": "output", "name": "out", "input": "scaled", "fade_in": 0.0, "fade_out": 0.0}),
        ];
        let mut m = entry("scaled", "state", 0.1, 1.0);
        m.key = Some("g".into());
        let d = with_knob(nodes.clone(), 0.2, knob("level", vec![m])).unwrap();
        let rms = |x: f64| rms_db(&at(&d, "level", x).channels[0]);
        let (lo, hi) = (rms(0.0), rms(1.0));
        println!("rms at 0 / 1: {lo:.2} / {hi:.2} dB");
        assert!((hi - lo - 20.0).abs() < 0.2, "{lo} {hi}");
        // A state name the expr does not have lists the ones it has.
        let mut bad = entry("scaled", "state", 0.1, 1.0);
        bad.key = Some("h".into());
        let e = with_knob(nodes, 0.2, knob("k", vec![bad])).err().unwrap();
        println!("{e}");
        assert!(e.contains("entries: g"), "{e}");
    }

    fn strike(extra: Value) -> Vec<Value> {
        let mut c = json!({"type": "contact", "name": "c", "speed": 1.5, "mass": 0.2,
                       "material": "wood", "hardness": 0.7});
        c.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![c]
    }

    #[test]
    fn contact_gain_db_lowers_the_peak_by_that_many_db() {
        let base = node_audio(strike(json!({})), "c", 0.1);
        let zero = node_audio(strike(json!({"gain_db": 0})), "c", 0.1);
        let down = node_audio(strike(json!({"gain_db": -6})), "c", 0.1);
        assert!(
            base.iter()
                .zip(&zero)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let d = 20.0 * (peak(&down) / peak(&base)).log10();
        println!("contact gain_db -6: {d:.3} dB");
        assert!((d + 6.0).abs() < 0.1, "{d}");
    }

    #[test]
    fn modal_gain_db_moves_the_level_and_zero_is_bit_identical() {
        let modal = |extra: Value| {
            let mut m = json!({"type": "modal", "name": "m", "excite": "hit",
                           "material": "wood", "size": 0.5});
            m.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            vec![json!({"type": "impulse", "name": "hit", "at": [0.01]}), m]
        };
        let base = node_audio(modal(json!({})), "m", 0.3);
        let zero = node_audio(modal(json!({"gain_db": 0})), "m", 0.3);
        let down = node_audio(modal(json!({"gain_db": -6})), "m", 0.3);
        assert!(
            base.iter()
                .zip(&zero)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let d = 20.0 * (peak(&down) / peak(&base)).log10();
        println!("modal gain_db -6: {d:.3} dB");
        assert!((d + 6.0).abs() < 0.1, "{d}");
    }
}
