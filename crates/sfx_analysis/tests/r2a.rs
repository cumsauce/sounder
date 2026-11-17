//! R2-A: `variations` compares what the output stage normalizes (sample peak
//! for `peak_db`, loudness for `lufs`), and a wrong shape for a check that
//! takes `true` or settings names the check and its fields.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::checks::Expect;
use sfx_analysis::{CheckCx, CheckResult, run};
use sfx_graph::{RenderOpts, SoundDoc};

/// A footstep-like seeded hit: a noise burst whose length and filter move with
/// the seed (so its loudness varies a lot), then an output stage.
fn hit(output: Value) -> SoundDoc {
    let mut d = SoundDoc::named("step");
    d.nodes = vec![
        json!({"type": "noise", "name": "n", "color": "pink"}),
        json!({"type": "wander", "name": "w", "rate": 30, "range": [300, 3000]}),
        json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": "w", "q": 2}),
        json!({"type": "decay", "name": "e", "time": 0.03}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "f", "b": "e"}),
        output,
    ];
    d.duration = Some(0.3);
    d
}

fn verify(d: &SoundDoc, e: Value) -> Vec<CheckResult> {
    let e: Expect = serde_json::from_value(e).unwrap_or_else(|e| panic!("{e}"));
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
    run(
        &e,
        &CheckCx {
            doc: d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn peak_db_output_compares_the_peak_and_passes() {
    let d = hit(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
    let v = verify(&d, json!({"variations": {"seeds": 6}}));
    println!("{}", v[0].detail);
    assert!(v[0].pass, "{:?}", v[0]);
    assert!(
        v[0].detail.contains("peak spread") && v[0].detail.contains("reported only"),
        "{:?}",
        v[0]
    );
}

#[test]
fn peak_db_output_fails_on_peak_spread_with_text_saying_what_was_compared() {
    // A negative limit cannot be met: the message names the peak comparison.
    let d = hit(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
    let v = verify(
        &d,
        json!({"variations": {"seeds": 4, "peak_spread_db": -1}}),
    );
    println!("{}", v[0].detail);
    assert!(!v[0].pass, "{:?}", v[0]);
    assert!(
        v[0].detail.contains("peak was compared") && v[0].detail.contains("peak_spread_db"),
        "{:?}",
        v[0]
    );
}

#[test]
fn lufs_output_keeps_the_loudness_limit() {
    let d = hit(json!({"type": "output", "name": "out", "input": "m", "lufs": -20}));
    let v = verify(&d, json!({"variations": {"seeds": 4, "lufs_spread": -1}}));
    println!("{}", v[0].detail);
    assert!(!v[0].pass, "{:?}", v[0]);
    assert!(
        v[0].detail.contains("loudness spread") && v[0].detail.contains("lufs_spread"),
        "{:?}",
        v[0]
    );
    assert!(!v[0].detail.contains("peak spread"), "{:?}", v[0]);
    // The old LU limit, unchanged (this hit spreads about 3 LU).
    let v = verify(&d, json!({"variations": {"seeds": 4, "lufs_spread": 4}}));
    assert!(v[0].pass, "{:?}", v[0]);
    // No target at all: the LU limit too.
    let d = hit(json!({"type": "output", "name": "out", "input": "m"}));
    let v = verify(&d, json!({"variations": true}));
    assert!(v[0].detail.contains("loudness spread"), "{:?}", v[0]);
}

#[test]
fn a_bad_shape_names_the_check_and_its_fields() {
    for (check, bad, field) in [
        ("variations", json!({"count": 4}), "count"),
        ("variations", json!(4), "expected true or {seeds"),
        ("dc", json!({"min": 1}), "min"),
        ("aliasing", json!("yes"), "expected true or {max_db}"),
    ] {
        let e = serde_json::from_value::<Expect>(json!({ check: bad })).unwrap_err();
        let m = e.to_string();
        println!("{m}");
        assert!(
            m.starts_with(&format!("{check}: expected true or {{")) && m.contains(field),
            "{m}"
        );
        assert!(!m.contains("untagged enum"), "{m}");
    }
}

#[test]
fn toggles_still_parse_in_every_good_form() {
    let e: Expect = serde_json::from_value(
        json!({"variations": {"seeds": 4}, "dc": true, "aliasing": {"max_db": -30}}),
    )
    .unwrap();
    assert!(e.variations.is_some() && e.dc.is_some() && e.aliasing.is_some());
    let e: Expect = serde_json::from_value(json!({"dc": false})).unwrap();
    assert!(e.dc.is_some());
}
