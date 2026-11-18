//! The `layers` check on the Phase 0 door slams (`docs/history/phase0-results.md`),
//! kept as fixtures in `tests/fixtures/phase0/`: the round-0 agent build
//! (`slam_a`), the first layered build the listener heard as a tuned drum
//! with a reverb tail (`slam_b`, as the listener heard it: after the search), and the rebuild they called "basically
//! perfect" (`slam_b2`). The `impact` preset must fail the first two for the
//! reasons the listener gave and pass the third.

#![allow(clippy::unwrap_used)]

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
