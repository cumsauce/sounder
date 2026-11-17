//! R3-D: `expect onsets` takes `gap: [lo, hi]` (every gap between
//! consecutive onsets in range, failure text lists the offender and the
//! times). Existing behaviour is unchanged without the field.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::checks::Expect;
use sfx_analysis::{CheckCx, CheckResult, run};
use sfx_graph::{RenderOpts, SoundDoc};

/// Short noise ticks at the given times (seconds).
fn hits(times: &[f32]) -> SoundDoc {
    let mut d = SoundDoc::named("hits");
    d.nodes = vec![
        json!({"type": "pattern", "name": "ev", "times": times}),
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "decay", "name": "e", "time": 0.02}),
        json!({"type": "math", "name": "tick", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "grains", "name": "g", "source": "tick", "events": "ev", "size": 0.08}),
        json!({"type": "output", "name": "out", "input": "g", "peak_db": -3}),
    ];
    d.duration = Some(times.last().copied().unwrap_or(0.0) + 0.4);
    d
}

fn verify(d: &SoundDoc, onsets: Value) -> CheckResult {
    let e: Expect = serde_json::from_value(json!({"onsets": onsets})).unwrap();
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
    let mut v = run(
        &e,
        &CheckCx {
            doc: d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(v.len(), 1, "{v:?}");
    let c = v.remove(0);
    println!("{}: {}", c.name, c.detail);
    c
}

#[test]
fn three_hits_in_range_pass() {
    let d = hits(&[0.1, 0.3, 0.55]);
    let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
    assert!(c.pass, "{c:?}");
    assert!(
        c.detail.contains("3 onsets") && c.detail.contains("2 gaps"),
        "{c:?}"
    );
}

#[test]
fn a_gap_out_of_range_fails_and_names_the_gap_and_times() {
    // Gaps 0.2 and 0.45: the second is too long.
    let d = hits(&[0.1, 0.3, 0.75]);
    let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
    assert!(!c.pass, "{c:?}");
    assert!(
        c.detail.contains("gap 2 is 0.4") && c.detail.contains("1 of 2 gaps"),
        "{c:?}"
    );
    assert!(c.detail.contains("onsets at 0.0"), "{c:?}");
    // Too close.
    let d = hits(&[0.1, 0.3, 0.4]);
    let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
    assert!(!c.pass && c.detail.contains("gap 2 is 0.1"), "{c:?}");
}

#[test]
fn the_count_still_fails_on_its_own_and_gap_alone_works() {
    let d = hits(&[0.1, 0.3, 0.55]);
    let c = verify(&d, json!({"min": 4, "gap": [0.1, 0.5]}));
    assert!(!c.pass && c.detail.contains("want"), "{c:?}");
    let c = verify(&d, json!({"gap": [0.1, 0.5]}));
    assert!(c.pass, "{c:?}");
}

#[test]
fn spacing_ms_sets_the_resolution_of_the_gaps() {
    // Ticks 20 ms apart: the default detector merges them, the fine one sees
    // them, and the gap range applies to what was counted.
    let d = hits(&[0.1, 0.12, 0.14, 0.16]);
    let c = verify(
        &d,
        json!({"spacing_ms": 5, "min": 4, "gap": [0.015, 0.025]}),
    );
    assert!(c.pass, "{c:?}");
    let c = verify(&d, json!({"spacing_ms": 5, "min": 4, "gap": [0.05, 0.1]}));
    assert!(!c.pass && c.detail.contains("outside"), "{c:?}");
}

#[test]
fn a_bad_range_is_an_error_and_no_gap_changes_nothing() {
    let d = hits(&[0.1, 0.3]);
    let e: Expect = serde_json::from_value(json!({"onsets": {"gap": [0.5, 0.1]}})).unwrap();
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap();
    let err = run(
        &e,
        &CheckCx {
            doc: &d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_err();
    assert!(err.contains("gap") && err.contains("lo <= hi"), "{err}");
    let c = verify(&d, json!({"min": 2, "max": 2}));
    assert!(c.pass, "{c:?}");
    assert!(!c.detail.contains("gaps"), "{c:?}");
}
