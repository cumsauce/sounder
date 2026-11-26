//! The `layers` check on the Phase 0 door slams (`docs/history/phase0-results.md`),
//! kept as fixtures in `tests/fixtures/phase0/`: the round-0 agent build
//! (`slam_a`), the first layered build the listener heard as a tuned drum
//! with a reverb tail (`slam_b`, as the listener heard it: after the search), and the rebuild they called "basically
//! perfect" (`slam_b2`). The `impact` preset must fail the first two for the
//! reasons the listener gave and pass the third.

use serde_json::json;
use sfx_analysis::checks::{LayersCheck, Range};
use sfx_analysis::{CheckCx, CheckResult, Expect, run};
use sfx_graph::{RenderOpts, SoundDoc};

fn fixture(name: &str) -> SoundDoc {
    let path = format!(
        "{}/tests/fixtures/phase0/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn layers(doc: &SoundDoc, l: LayersCheck) -> Vec<CheckResult> {
    let opts = RenderOpts {
        cached: false,
        ..RenderOpts::default()
    };
    let rendered = doc.render(&opts).unwrap();
    let e = Expect {
        layers: Some(l),
        ..Expect::default()
    };
    let r = run(
        &e,
        &CheckCx {
            doc,
            rendered: &rendered,
            opts: &opts,
        },
    )
    .unwrap();
    for c in &r {
        println!(
            "{}: {} {}: {}",
            doc.name,
            if c.pass { "PASS" } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    r
}

fn impact() -> LayersCheck {
    LayersCheck {
        preset: Some("impact".into()),
        ..LayersCheck::default()
    }
}

fn failed(r: &[CheckResult]) -> Vec<&str> {
    r.iter()
        .filter(|c| !c.pass)
        .map(|c| c.name.trim_start_matches("layers "))
        .collect()
}

#[test]
fn impact_preset_fails_the_rejected_slams_and_passes_the_approved_one() {
    let a = layers(&fixture("slam_a"), impact());
    assert_eq!(failed(&a), ["weight", "body", "crest"], "slam_a");
    let b = layers(&fixture("slam_b"), impact());
    assert_eq!(failed(&b), ["ringing", "crest", "dry"], "slam_b");
    let b2 = layers(&fixture("slam_b2"), impact());
    assert!(failed(&b2).is_empty(), "slam_b2: {:?}", failed(&b2));
    assert_eq!(b2.len(), 7, "the impact preset runs seven measures");
}

#[test]
fn a_failure_names_the_layer_to_add() {
    let a = layers(&fixture("slam_a"), impact());
    let body = a.iter().find(|c| c.name == "layers body").unwrap();
    assert!(body.detail.contains("add a body layer"), "{}", body.detail);
    let b = layers(&fixture("slam_b"), impact());
    let dry = b.iter().find(|c| c.name == "layers dry").unwrap();
    assert!(dry.detail.contains("reverb `room`"), "{}", dry.detail);
    let ringing = b.iter().find(|c| c.name == "layers ringing").unwrap();
    assert!(ringing.detail.contains("tuned drum"), "{}", ringing.detail);
}

#[test]
fn given_fields_override_the_preset() {
    // The approved slam rings 130 ms to -20 dB: a tail range that excludes
    // it fails, and a crest bound given beside the preset replaces the
    // preset's.
    let l = LayersCheck {
        preset: Some("impact".into()),
        tail_ms: Some(Range {
            min: Some(180.0),
            max: None,
        }),
        crest_db: Some(Range {
            min: None,
            max: Some(30.0),
        }),
        ..LayersCheck::default()
    };
    let r = layers(&fixture("slam_b2"), l);
    assert_eq!(failed(&r), ["tail"]);
    let b = layers(
        &fixture("slam_b"),
        LayersCheck {
            preset: Some("impact".into()),
            crest_db: Some(Range {
                min: None,
                max: Some(30.0),
            }),
            ..LayersCheck::default()
        },
    );
    assert!(!failed(&b).contains(&"crest"));
}

#[test]
fn an_unknown_preset_is_an_error_naming_the_presets() {
    let l: LayersCheck = serde_json::from_value(json!({"preset": "whoosh"})).unwrap();
    let e = l.resolved().unwrap_err();
    assert!(e.contains("whoosh") && e.contains("impact"), "{e}");
    let bad = serde_json::from_value::<LayersCheck>(json!({"wieght": {"min": 10}}));
    assert!(bad.is_err());
}

/// The `spec` check on a small two-layer hit: a crack and a weight, each
/// named in the spec with a slot and a band.
mod spec {
    use super::*;
    use sfx_graph::Spec;

    fn hit(spec: serde_json::Value) -> SoundDoc {
        let mut doc: SoundDoc = serde_json::from_value(json!({"name": "hit", "nodes": [
            {"type": "noise", "name": "wn", "color": "white"},
            {"type": "decay", "name": "crk_e", "time": 0.01, "at": 0.01},
            {"type": "math", "name": "crk_raw", "op": "mul", "a": "wn", "b": "crk_e"},
            {"type": "filter", "name": "crk", "input": "crk_raw", "mode": "hp", "freq": 3000},
            {"type": "noise", "name": "bn", "color": "brown"},
            {"type": "decay", "name": "thd_e", "time": 0.3, "at": 0.01},
            {"type": "math", "name": "thd_raw", "op": "mul", "a": "bn", "b": "thd_e"},
            {"type": "filter", "name": "thd", "input": "thd_raw", "mode": "lp", "freq": 200, "slope": 24},
            {"type": "contact", "name": "c", "speed": 1, "mass": 0.3, "material": "wood", "at": 0.01},
            {"type": "modal", "name": "m", "excite": "c", "material": "wood", "size": 0.5},
            {"type": "mix", "name": "dry", "inputs": ["crk", "thd"]},
            {"type": "output", "name": "out", "input": "dry", "peak_db": -3}
        ]}))
        .unwrap();
        doc.spec = Some(serde_json::from_value::<Spec>(spec).unwrap());
        doc
    }

    fn check(doc: &SoundDoc) -> Vec<CheckResult> {
        let opts = RenderOpts::default();
        let rendered = doc.render(&opts).unwrap();
        let e = Expect::defaults_for(doc);
        assert_eq!(
            e.spec,
            Some(true),
            "a sound with a spec checks it by default"
        );
        let r = run(
            &e,
            &CheckCx {
                doc,
                rendered: &rendered,
                opts: &opts,
            },
        )
        .unwrap();
        r.into_iter()
            .filter(|c| c.name.starts_with("spec"))
            .collect()
    }

    #[test]
    fn a_built_spec_passes() {
        let r = check(&hit(json!({"brief": "a dull thud", "components": [
            {"role": "crack", "node": "crk", "at": [0.01, 0.04]},
            {"role": "weight", "node": "thd", "at": [0.01, 0.4]}
        ]})));
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|c| c.pass), "{r:?}");
    }

    #[test]
    fn each_failure_says_what_is_wrong() {
        let r = check(&hit(json!({"components": [
            {"role": "body", "material": "wood"},
            {"role": "pre", "node": "nope"},
            {"role": "body", "node": "m", "material": "metal"},
            {"role": "crack", "node": "crk", "at": [0.2, 0.3]},
            {"role": "weight", "node": "crk"}
        ]})));
        let detail = |i: usize| {
            assert!(!r[i].pass, "{:?}", r[i]);
            r[i].detail.as_str()
        };
        assert!(detail(0).contains("not built"));
        assert!(detail(1).contains("no node named `nope`"));
        // `m` exists but nothing mixes it in; its material is checked once it does.
        assert!(detail(2).contains("does not reach the output"));
        assert!(detail(3).contains("of its energy in 0.2-0.3 s"));
        assert!(detail(4).contains("in 20-400 Hz (the role's band)"));
    }

    #[test]
    fn a_material_that_differs_fails() {
        let mut doc =
            hit(json!({"components": [{"role": "body", "node": "m", "material": "metal"}]}));
        for n in &mut doc.nodes {
            if n["name"] == "dry" {
                n["inputs"] = json!(["crk", "thd", "m"]);
            }
        }
        let r = check(&doc);
        assert!(
            !r[0].pass && r[0].detail.contains("the layer's nodes say wood"),
            "{r:?}"
        );
    }

    #[test]
    fn a_layer_shorter_than_its_length_fails() {
        let r = check(&hit(json!({"components": [
            {"role": "weight", "node": "thd", "length_ms": [20, 300]},
            {"role": "crack", "node": "crk", "length_ms": [40, 300]},
            {"role": "crack", "node": "crk", "length_ms": [0.01, 0.1]}
        ]})));
        assert!(r[0].pass, "{r:?}");
        assert!(!r[1].pass && r[1].detail.contains("too short"), "{r:?}");
        assert!(!r[2].pass && r[2].detail.contains("too long"), "{r:?}");
    }

    #[test]
    fn a_bad_role_or_range_is_refused_by_the_edit() {
        let mut doc = hit(json!({}));
        for (spec, want) in [
            (json!({"components": [{"role": "thud"}]}), "role `thud`"),
            (
                json!({"components": [{"role": "body", "at": [0.3, 0.1]}]}),
                "end after start",
            ),
            (
                json!({"components": [{"role": "body", "band": [900, 100]}]}),
                "high above low",
            ),
        ] {
            let edit = sfx_graph::DocEdit {
                spec: Some(Some(serde_json::from_value(spec).unwrap())),
                ..sfx_graph::DocEdit::default()
            };
            let e = doc
                .apply(Vec::new(), edit, &RenderOpts::default())
                .unwrap_err();
            assert!(e.contains(want), "{e}");
        }
    }
}

/// The `heavy` and `tear` presets, on synthetic sounds built from
/// band-limited noise with envelopes.
mod presets {
    use super::*;

    /// One layer: `color` noise x a decay of `time` s starting at `at`,
    /// through a filter (`mode` at `freq`, slope 24), `db` dB.
    fn layer(
        name: &str,
        color: &str,
        mode: &str,
        freq: f32,
        time: f32,
        at: f32,
        db: f32,
    ) -> Vec<serde_json::Value> {
        vec![
            json!({"type": "noise", "name": format!("{name}_n"), "color": color}),
            json!({"type": "decay", "name": format!("{name}_e"), "time": time, "at": at}),
            json!({"type": "math", "name": format!("{name}_r"), "op": "mul",
                   "a": format!("{name}_n"), "b": format!("{name}_e")}),
            {
                let mut f = json!({"type": "filter", "name": format!("{name}_f"),
                    "input": format!("{name}_r"), "mode": mode, "freq": freq});
                if mode != "bp" {
                    f["slope"] = json!(24);
                }
                f
            },
            json!({"type": "gain", "name": name, "input": format!("{name}_f"), "db": db}),
        ]
    }

    fn sound(name: &str, layers: Vec<Vec<serde_json::Value>>, duration: f32) -> SoundDoc {
        let mut nodes: Vec<serde_json::Value> = layers.iter().flatten().cloned().collect();
        let inputs: Vec<String> = layers
            .iter()
            .map(|l| l.last().unwrap()["name"].as_str().unwrap().to_string())
            .collect();
        nodes.push(json!({"type": "mix", "name": "sum", "inputs": inputs}));
        nodes.push(json!({"type": "output", "name": "out", "input": "sum", "peak_db": -3}));
        let mut doc = SoundDoc::named(name);
        doc.nodes = nodes;
        doc.duration = Some(duration);
        doc
    }

    fn preset(p: &str) -> LayersCheck {
        LayersCheck {
            preset: Some(p.into()),
            ..LayersCheck::default()
        }
    }

    /// A crash: a 3 s rumble, a 0.5 s mid body, a short quiet crack.
    fn crash() -> SoundDoc {
        sound(
            "crash",
            vec![
                layer("rumble", "brown", "lp", 160.0, 1.2, 0.02, -12.0),
                layer("mid", "pink", "bp", 700.0, 0.3, 0.02, -3.0),
                layer("crk", "white", "hp", 3000.0, 0.02, 0.02, -26.0),
            ],
            3.0,
        )
    }

    /// A metal tear: noise above 1 kHz with a slow envelope and a little
    /// low end.
    fn tear() -> SoundDoc {
        sound(
            "tear",
            vec![
                layer("hi", "white", "hp", 2500.0, 1.2, 0.02, -14.0),
                layer("mid", "pink", "bp", 1200.0, 1.0, 0.02, 0.0),
                layer("lo", "brown", "lp", 150.0, 0.8, 0.02, -30.0),
            ],
            2.0,
        )
    }

    /// A small knock: a wood-ish mid body with a thud and a crack.
    fn knock() -> SoundDoc {
        sound(
            "knock",
            vec![
                layer("thud", "brown", "lp", 200.0, 0.1, 0.01, -16.0),
                layer("body", "pink", "bp", 700.0, 0.05, 0.01, 0.0),
                layer("crk", "white", "hp", 3000.0, 0.01, 0.01, -14.0),
            ],
            0.4,
        )
    }

    fn fail_names(r: &[CheckResult]) -> Vec<String> {
        failed(r).into_iter().map(String::from).collect()
    }

    #[test]
    fn a_crash_passes_heavy_and_fails_impact() {
        let h = layers(&crash(), preset("heavy"));
        assert!(failed(&h).is_empty(), "heavy: {:?}", fail_names(&h));
        let i = layers(&crash(), preset("impact"));
        assert!(
            failed(&i).contains(&"weight"),
            "impact: {:?}",
            fail_names(&i)
        );
    }

    #[test]
    fn a_tear_passes_tear_and_fails_impact() {
        let t = layers(&tear(), preset("tear"));
        assert!(failed(&t).is_empty(), "tear: {:?}", fail_names(&t));
        assert_eq!(t.len(), 5, "weight, body, crack, voiced, dry");
        let i = layers(&tear(), preset("impact"));
        assert!(
            failed(&i).contains(&"weight"),
            "impact: {:?}",
            fail_names(&i)
        );
    }

    #[test]
    fn a_small_knock_passes_impact_and_fails_heavy() {
        let i = layers(&knock(), preset("impact"));
        assert!(failed(&i).is_empty(), "impact: {:?}", fail_names(&i));
        let h = layers(&knock(), preset("heavy"));
        assert!(
            failed(&h).contains(&"weight"),
            "heavy: {:?}",
            fail_names(&h)
        );
    }

    #[test]
    fn a_tear_is_not_a_heavy_hit_and_a_crash_is_not_a_tear() {
        let h = layers(&tear(), preset("heavy"));
        assert!(failed(&h).contains(&"weight"), "{:?}", fail_names(&h));
        let t = layers(&crash(), preset("tear"));
        assert!(failed(&t).contains(&"weight"), "{:?}", fail_names(&t));
    }

    #[test]
    fn failures_speak_in_the_terms_of_the_kind_of_sound() {
        let h = layers(&knock(), preset("heavy"));
        let w = h.iter().find(|c| c.name == "layers weight").unwrap();
        assert!(w.detail.contains("rumble"), "{}", w.detail);
        let t = layers(&crash(), preset("tear"));
        let w = t.iter().find(|c| c.name == "layers weight").unwrap();
        assert!(w.detail.contains("high-pass"), "{}", w.detail);
    }

    #[test]
    fn heavy_judges_the_tail_and_tear_does_not() {
        let h = layers(&crash(), preset("heavy"));
        assert!(h.iter().any(|c| c.name == "layers tail" && c.pass));
        let t = layers(&tear(), preset("tear"));
        assert!(
            !t.iter()
                .any(|c| c.name == "layers tail" || c.name == "layers crest")
        );
    }

    #[test]
    fn the_presets_are_listed_in_the_error_for_an_unknown_one() {
        for p in ["heavy", "tear"] {
            assert!(preset(p).resolved().is_ok());
        }
        let e = preset("whisper").resolved().unwrap_err();
        for name in ["impact", "heavy", "tear", "footstep"] {
            assert!(e.contains(name), "{e}");
        }
    }
}
