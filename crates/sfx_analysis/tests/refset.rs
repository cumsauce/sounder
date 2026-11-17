//! Reference sets (`refs/<set>/`): a sound in the family lands inside every
//! feature's span; a tonal ring is outside (voiced, ringing); the verify check
//! reports per feature; a missing or one-file set is an error that helps.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sfx_analysis::refset::{compare_set, load_set};
use sfx_analysis::{CheckCx, Expect, run};
use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

/// A noise hit band-passed at `hz` with a `decay` s body and a low thump.
fn hit(hz: f32, decay: f32, seed: u64) -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "color": "pink", "seed": seed}),
        json!({"type": "decay", "name": "e", "time": decay, "at": 0.01}),
        json!({"type": "math", "name": "b", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "filter", "name": "body", "input": "b", "mode": "bp", "freq": hz, "q": 0.7}),
        json!({"type": "noise", "name": "w", "color": "brown", "seed": seed + 10}),
        json!({"type": "decay", "name": "we", "time": decay * 0.7, "at": 0.01}),
        json!({"type": "math", "name": "wb", "op": "mul", "a": "w", "b": "we"}),
        json!({"type": "filter", "name": "thump", "input": "wb", "mode": "lp", "freq": 160, "slope": 24}),
        json!({"type": "mix", "name": "all", "inputs": ["body", {"input": "thump", "gain": 0.6}]}),
        json!({"type": "output", "name": "out", "input": "all", "peak_db": -3}),
    ]
}

fn doc(nodes: Vec<Value>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(1.0);
    d
}

fn render(nodes: Vec<Value>) -> Audio {
    doc(nodes).render(&RenderOpts::default()).unwrap().audio
}

fn project(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("sounder_refset_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    let set = p.join("refs").join("hits");
    std::fs::create_dir_all(&set).unwrap();
    for (i, (hz, decay)) in [(280.0, 0.5), (330.0, 0.6), (380.0, 0.7)]
        .iter()
        .enumerate()
    {
        let a = render(hit(*hz, *decay, i as u64 + 1));
        wav::write(&set.join(format!("hit_{i}.wav")), &a, wav::Bits::Float32).unwrap();
    }
    p
}

#[test]
fn a_family_member_is_inside_and_a_ringing_tone_is_outside() {
    let p = project("family");
    let members = load_set("hits", Some(&p)).unwrap();
    assert_eq!(members.len(), 3);
    let inside = compare_set(&render(hit(310.0, 0.55, 7)), "hits", &members);
    println!("{}", inside.text());
    assert!(inside.failing().is_empty(), "{}", inside.text());
    let tone = vec![
        json!({"type": "osc", "name": "o", "freq": 200}),
        json!({"type": "decay", "name": "e", "time": 2.0, "at": 0.01}),
        json!({"type": "math", "name": "t", "op": "mul", "a": "o", "b": "e"}),
        json!({"type": "output", "name": "out", "input": "t", "peak_db": -3}),
    ];
    let outside = compare_set(&render(tone), "hits", &members);
    println!("{}", outside.text());
    let names: Vec<&str> = outside.failing().iter().map(|f| f.name).collect();
    assert!(names.contains(&"voiced"), "{names:?}");
    assert!(
        names.contains(&"weight") || names.contains(&"body"),
        "{names:?}"
    );
    let _ = std::fs::remove_dir_all(&p);
}

#[test]
fn the_verify_check_reports_each_feature_and_the_nearest() {
    let p = project("verify");
    let d = doc(hit(310.0, 0.55, 7));
    let opts = RenderOpts {
        base_dir: Some(p.clone()),
        ..RenderOpts::default()
    };
    let r = d.render(&opts).unwrap();
    let e: Expect =
        serde_json::from_value(json!({"reference_set": {"set": "hits", "max_distance": 30}}))
            .unwrap();
    let res = run(
        &e,
        &CheckCx {
            doc: &d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap();
    for c in &res {
        println!(
            "{} {}: {}",
            if c.pass { "PASS" } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    assert!(res.iter().all(|c| c.pass));
    assert!(
        res.iter()
            .any(|c| c.name == "reference_set nearest" && c.detail.contains("hit_"))
    );
    assert!(res.iter().any(|c| c.name == "reference_set crest"));
    let _ = std::fs::remove_dir_all(&p);
}

#[test]
fn a_missing_or_one_file_set_is_an_error_that_helps() {
    let p = project("errors");
    let e = load_set("doors", Some(&p)).err().unwrap();
    assert!(e.contains("doors") && e.contains("hits"), "{e}");
    let one = p.join("refs").join("single");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::copy(p.join("refs/hits/hit_0.wav"), one.join("a.wav")).unwrap();
    let e = load_set("single", Some(&p)).err().unwrap();
    assert!(e.contains("at least two"), "{e}");
    let _ = std::fs::remove_dir_all(&p);
}
