//! The page's fixture (`ui/fixture.json`, what the page renders in a plain
//! browser) and the real `Session::window_state` must have the same keys:
//! the Rust side and `ui/` were built in parallel against
//! `design/window-api.md`, and this is what keeps them honest.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_graph::{Claim, DocEdit, Kind, Macro, MapCurve, MapEntry, Measure};
use sounder::session::{Annotation, Session};

fn project() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_contract_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every key path in `v` (arrays contribute their first element's paths).
fn paths(v: &Value, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                out.insert(p.clone());
                paths(x, &p, out);
            }
        }
        Value::Array(a) => {
            if let Some(x) = a.first() {
                paths(x, &format!("{prefix}[]"), out);
            }
        }
        _ => {}
    }
}

fn real_state() -> Value {
    let mut s = Session::new(Some(project()));
    s.begin_call(Some("look"), None, None);
    s.new_sound("laser", Kind::Oneshot).unwrap();
    s.apply_ops(
        None,
        vec![
            json!({"type": "ramp", "name": "sweep", "from": 2000, "to": 200, "time": 0.25, "curve": "exp"}),
            json!({"type": "osc", "name": "tone", "shape": "saw", "freq": "sweep"}),
            json!({"type": "decay", "name": "amp", "time": 0.3}),
            json!({"type": "math", "name": "zap", "op": "mul", "a": "tone", "b": "amp"}),
            json!({"type": "filter", "name": "lp", "input": "zap", "mode": "lp", "freq": 3000, "q": 1.5}),
            json!({"type": "osc", "name": "unused", "freq": 100}),
            json!({"type": "output", "name": "out", "input": "lp", "lufs": -18}),
        ],
        DocEdit {
            macros: Some(vec![Macro {
                name: "brightness".into(),
                value: 0.5,
                default: None,
                doc: "how cutting it is".into(),
                claim: Some(Claim(Measure::Centroid, 1)),
                game: false,
                map: vec![MapEntry {
                    node: "lp".into(),
                    field: "q".into(),
                    from: 0.8,
                    to: 2.4,
                    curve: MapCurve::Linear,
                    param: None,
                    col: None,
                    index: None,
                    key: None,
                }],
            }]),
            ..DocEdit::default()
        },
    )
    .unwrap();
    s.add_annotation(
        None,
        Annotation {
            id: 0,
            text: "too whistly".into(),
            by: Some("you".into()),
            node: Some("lp".into()),
            time: Some([0.0, 0.07]),
            hz: Some([2500.0, 9000.0]),
            resolved: false,
            reply: None,
            resolved_by: None,
        },
    )
    .unwrap();
    s.move_knob(None, "brightness", 0.8).unwrap();
    s.check(None, None).unwrap();
    s.log_call("sound_ops", true, 5, Some("look"));
    s.end_call(Some("look"));
    s.window_state(None).unwrap()
}

#[test]
fn the_fixture_and_the_real_state_share_their_keys() {
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/fixture.json"))
            .unwrap(),
    )
    .unwrap();
    // The fixture is `state` with `sound` filled from sound_data[state.doc].
    let mut fx = fixture["state"].clone();
    fx["sound"] = fixture["sound_data"]["laser"].clone();
    let real = real_state();
    let (mut a, mut b) = (Default::default(), Default::default());
    paths(&fx, "", &mut a);
    paths(&real, "", &mut b);
    // Keys the page reads that Rust must serve.
    let missing: Vec<&String> = a.difference(&b).collect();
    // Keys Rust serves that the page's fixture lacks (the page may ignore
    // them, but then the fixture is out of date).
    let extra: Vec<&String> = b.difference(&a).collect();
    // `catalog` lives beside `state` in the fixture. Not compared: keys that
    // exist only because of what the fixture's data happens to contain (a
    // macro-driven `from` / `to` field on a ramp node, an agent's reply on a
    // comment, comment times) and the descriptors' extra detail, which the page
    // does not read.
    let data_dependent = |k: &str| {
        k.starts_with("catalog")
            || k.starts_with("recent[]")
            || k.starts_with("sound.nodes[].effective.")
            || k.starts_with("sound.comments[].reply.")
            || k == "sound.comments[].ago"
    };
    let missing: Vec<&&String> = missing.iter().filter(|k| !data_dependent(k)).collect();
    let extra: Vec<&&String> = extra
        .iter()
        .filter(|k| !k.starts_with("sound.descriptors.") && !k.starts_with("sound.moves[]"))
        .collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "the page's fixture has keys the real state lacks: {missing:?}\nthe real state has keys the fixture lacks: {extra:?}"
    );
}
