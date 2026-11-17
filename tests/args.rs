//! Tool arguments and the fixes that came with them (task F1): unknown
//! arguments are errors that name the tool, the key and the keys it takes;
//! `describe_op` takes `type` / `node` as aliases; `verify` runs named checks
//! and `expect`; a top-level `delete` shows the working form; `export`
//! warns about identical variations; `mono` / `stereo` are reserved names.
//!
//! Most tests drive `Session` like `tests/session.rs`; the last one drives
//! the real stdio server through `mcp_driver.py` so every one of the 16
//! tools is checked for the unknown-argument error.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sfx_analysis::Expect;
use sfx_graph::{DocEdit, Kind, wav};
use sounder::export;
use sounder::session::{Session, check_tool_args};

fn project(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_a_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Two decaying noise hits half a second apart, plus two spare nodes.
fn graph() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "decay", "name": "e", "time": 0.2}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 1000}),
        json!({"type": "output", "name": "out", "input": "f", "lufs": -20}),
    ]
}

fn session(tag: &str) -> Session {
    let mut s = Session::new(Some(project(tag)));
    s.new_sound("t", Kind::Oneshot).unwrap();
    s.apply_ops(None, graph(), DocEdit::default()).unwrap();
    s
}

fn names(results: &[sfx_analysis::CheckResult]) -> Vec<String> {
    results.iter().map(|r| r.name.clone()).collect()
}

fn keys(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

fn obj(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().unwrap().clone()
}

// ------------------------------------------------------------ unknown args

#[test]
fn an_unknown_argument_names_the_tool_the_key_and_the_keys_it_takes() {
    let accepted = keys(&["kind", "type", "node"]);
    let e = check_tool_args("describe_op", &obj(json!({"name": "filter"})), &accepted).unwrap_err();
    println!("{e}");
    assert!(e.starts_with("describe_op:"), "{e}");
    assert!(e.contains("`name`"), "{e}");
    assert!(e.contains("it takes: kind, type, node"), "{e}");
    // Two bad keys: both are named.
    let e = check_tool_args(
        "listen",
        &obj(json!({"zoom_in": 1, "node": "x", "tile": 2})),
        &keys(&["node", "zoom"]),
    )
    .unwrap_err();
    assert!(e.contains("`zoom_in`") && e.contains("`tile`") && !e.contains("`node`"));
    assert!(e.contains("unknown arguments"), "{e}");
    // Known keys pass.
    check_tool_args("listen", &obj(json!({"node": "x"})), &keys(&["node"])).unwrap();
}

#[test]
fn a_top_level_delete_error_shows_the_working_form_and_the_form_works() {
    let accepted = keys(&["sound", "ops", "params", "doc"]);
    let e =
        check_tool_args("sound_ops", &obj(json!({"delete": ["f", "m"]})), &accepted).unwrap_err();
    println!("{e}");
    assert!(e.contains("sound_ops:") && e.contains("`delete`"), "{e}");
    assert!(
        e.contains(r#"ops: [{"type": "delete", "name": "f"}, {"type": "delete", "name": "m"}]"#),
        "{e}"
    );

    // The form it shows removes several names in one step.
    let mut s = Session::new(Some(project("del")));
    s.new_sound("t", Kind::Oneshot).unwrap();
    let mut g = graph();
    g.push(json!({"type": "noise", "name": "spare1"}));
    g.push(json!({"type": "noise", "name": "spare2"}));
    g.push(json!({"type": "noise", "name": "spare3"}));
    s.apply_ops(None, g, DocEdit::default()).unwrap();
    assert_eq!(s.sound(None).unwrap().doc.nodes.len(), 8);
    let r = s
        .apply_ops(
            None,
            vec![
                json!({"type": "delete", "name": "spare1"}),
                json!({"type": "delete", "name": "spare2"}),
                json!({"type": "delete", "name": "spare3"}),
            ],
            DocEdit::default(),
        )
        .unwrap();
    assert_eq!(r.removed, vec!["spare1", "spare2", "spare3"]);
    assert_eq!(s.sound(None).unwrap().doc.nodes.len(), 5);
    // A name that is not there is an error naming it.
    let e = s
        .apply_ops(
            None,
            vec![json!({"type": "delete", "name": "nope"})],
            DocEdit::default(),
        )
        .unwrap_err();
    assert!(e.contains("nope"), "{e}");
}

// ------------------------------------------------------------------ verify

#[test]
fn verify_checks_adds_to_the_defaults_only_replaces_them() {
    let mut s = session("verify");
    let plain = names(&s.verify(None, &[], &[], &[]).unwrap());
    let with = names(&s.verify(None, &keys(&["clicks"]), &[], &[]).unwrap());
    let only = names(&s.verify(None, &[], &keys(&["clicks"]), &[]).unwrap());
    println!("defaults {plain:?}\nchecks   {with:?}\nonly     {only:?}");
    assert!(!plain.contains(&"clicks".to_string()));
    assert!(with.contains(&"clicks".to_string()), "{with:?}");
    for d in &plain {
        assert!(with.contains(d), "{d} missing from {with:?}");
    }
    assert_eq!(only, vec!["clicks".to_string()]);
}

#[test]
fn verify_expect_takes_settings_like_sound_ops_expect() {
    let mut s = session("verify_expect");
    let e: Expect = serde_json::from_value(json!({
        "duration": {"min": 0.05, "max": 5.0},
        "onsets": {"min": 1, "max": 10},
    }))
    .unwrap();
    let r = s.verify(None, &[], &[], &[&e]).unwrap();
    println!("{}", sfx_analysis::report(&r));
    // Given alone: just these two, no defaults.
    assert_eq!(
        names(&r),
        vec!["duration".to_string(), "onsets".to_string()]
    );
    assert!(r.iter().all(|c| c.pass), "{r:?}");
    // With `checks`, on top of the defaults.
    let r = s.verify(None, &keys(&["clicks"]), &[], &[&e]).unwrap();
    let n = names(&r);
    assert!(
        n.contains(&"finite".to_string())
            && n.contains(&"duration".to_string())
            && n.contains(&"clicks".to_string()),
        "{n:?}"
    );
    // A window that cannot hold the sound fails and says what was measured.
    let tight: Expect = serde_json::from_value(json!({"duration": {"max": 0.01}})).unwrap();
    let r = s.verify(None, &[], &[], &[&tight]).unwrap();
    assert!(!r[0].pass, "{r:?}");
}

#[test]
fn verify_names_the_checks_it_cannot_run_bare_and_the_ones_it_does_not_know() {
    let mut s = session("verify_err");
    let e = s.verify(None, &keys(&["onsets"]), &[], &[]).unwrap_err();
    println!("{e}");
    assert!(
        e.contains("`onsets`") && e.contains("needs settings"),
        "{e}"
    );
    assert!(
        e.contains(r#"{"expect": {"onsets": {"min": 1, "max": 3}}}"#),
        "{e}"
    );
    let e = s.verify(None, &[], &keys(&["clix"]), &[]).unwrap_err();
    assert!(
        e.contains("unknown check `clix`") && e.contains("clicks"),
        "{e}"
    );
}

// ------------------------------------------------------------------ export

fn export_opts(dir: PathBuf, variations: u32) -> export::ExportOpts {
    export::ExportOpts {
        dir,
        variations,
        bits: wav::Bits::Int16,
        sample_rate: None,
        base_dir: None,
        sounds: None,
        catalog_name: None,
    }
}

#[test]
fn identical_variations_warn_for_a_noise_free_sound_and_not_for_a_seeded_one() {
    // Nothing reads the seed: three identical files.
    let mut s = Session::new(Some(project("export_same")));
    s.new_sound("tone", Kind::Oneshot).unwrap();
    s.apply_ops(
        None,
        vec![
            json!({"type": "osc", "name": "o", "freq": 440, "amp": 0.3}),
            json!({"type": "decay", "name": "e", "time": 0.1}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "m"}),
        ],
        DocEdit::default(),
    )
    .unwrap();
    let doc = s.sound(None).unwrap().doc.clone();
    let dir = project("export_same_out");
    let ex = export::export(&doc, &export_opts(dir.clone(), 3)).unwrap();
    println!("{:?}", ex.warnings);
    assert_eq!(ex.warnings.len(), 1, "{:?}", ex.warnings);
    assert!(
        ex.warnings[0].contains("bit-identical")
            && ex.warnings[0].contains("no node reads the seed")
            && ex.warnings[0].contains("add jitter or a seeded node"),
        "{:?}",
        ex.warnings
    );
    // One file: nothing to compare.
    let ex = export::export(&doc, &export_opts(dir, 1)).unwrap();
    assert!(ex.warnings.is_empty());

    // Noise reads the seed: no warning.
    let mut s = session("export_seeded");
    let doc = s.sound(None).unwrap().doc.clone();
    let ex = export::export(&doc, &export_opts(project("export_seeded_out"), 3)).unwrap();
    println!("seeded: {:?}", ex.warnings);
    assert!(ex.warnings.is_empty(), "{:?}", ex.warnings);
}

// ------------------------------------------------------------ reserved names

#[test]
fn a_node_called_mono_is_refused_with_an_error_that_says_why() {
    let mut s = session("mono");
    for name in ["mono", "stereo"] {
        let e = s
            .apply_ops(
                None,
                vec![json!({"type": "gain", "name": name, "input": "f", "gain": 0.5})],
                DocEdit::default(),
            )
            .unwrap_err();
        println!("{e}");
        assert!(
            e.contains(&format!("`{name}` is reserved")) && e.contains("`to`"),
            "{e}"
        );
        assert!(!e.contains("letters, digits"), "{e}");
    }
    // Nothing changed.
    assert_eq!(s.sound(None).unwrap().doc.nodes.len(), 5);
    // The words still work as values of `to`.
    s.apply_ops(
        None,
        vec![
            json!({"type": "mix", "name": "mm", "inputs": ["f"], "to": "mono"}),
            json!({"type": "output", "name": "out", "input": "mm", "lufs": -20}),
        ],
        DocEdit::default(),
    )
    .unwrap();
}

// ------------------------------------------------- the real server, 16 tools

fn drive(calls: &Value, dir: &Path) -> Vec<Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("mcp_driver.py");
    let file = dir.join("calls.json");
    std::fs::write(&file, serde_json::to_vec(calls).unwrap()).unwrap();
    let out = Command::new("python3")
        .arg(&script)
        .arg("--calls")
        .arg(&file)
        .env("SOUNDER_BINARY", env!("CARGO_BIN_EXE_sounder"))
        .env("FRIENDSLOP_DATA_DIR", dir.join("data"))
        .env("SOUNDER_STDERR", dir.join("server.log"))
        .output()
        .expect("python3 is required for the driver tests");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert_eq!(
        lines.len(),
        calls.as_array().unwrap().len() + 1,
        "driver output:\n{text}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    lines[1..].to_vec()
}

fn text_of(v: &Value) -> String {
    v["text"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

const TOOLS: [&str; 16] = [
    "summary",
    "sound_ops",
    "listen",
    "verify",
    "describe_op",
    "extract_modes",
    "play",
    "new_sound",
    "catalog",
    "export",
    "undo",
    "redo",
    "jump",
    "history",
    "add_annotation",
    "session_log",
];

#[test]
fn every_tool_of_the_real_server_rejects_an_unknown_argument() {
    let dir = project("server");
    let mut calls = vec![json!({"name": "new_sound", "args": {"name": "s"}})];
    for t in TOOLS {
        calls.push(json!({"name": t, "args": {"bogus_key": 1}}));
    }
    // The cases from round 0, and the accepted forms.
    calls.push(json!({"name": "describe_op", "args": {"name": "filter"}}));
    calls.push(json!({"name": "listen", "args": {"zoom_in": [0, 1]}}));
    calls.push(json!({"name": "sound_ops", "args": {"delete": ["a", "b"]}}));
    calls.push(json!({"name": "verify", "args": {"expect": {"duration": {"max": 1}, "bogus": 1}}}));
    calls.push(json!({"name": "describe_op", "args": {"type": "filter"}}));
    calls.push(json!({"name": "describe_op", "args": {"node": "filter"}}));
    calls.push(json!({"name": "describe_op", "args": {"kind": "filter"}}));
    calls.push(json!({"name": "describe_op", "args": {}}));
    calls.push(json!({"name": "describe_op", "args": {"kind": "filter", "type": "osc"}}));
    calls.push(json!({"name": "sound_ops", "args": {"ops": graph()}}));
    calls.push(json!({"name": "verify", "args": {"checks": ["clicks"]}}));
    calls.push(json!({"name": "verify", "args": {"only": ["clicks"]}}));
    calls.push(json!({"name": "verify", "args": {"clicks": true}}));
    calls.push(json!({"name": "verify", "args": {"expect": {"duration": {"min": 0.05, "max": 5}, "onsets": {"min": 1, "max": 10}}}}));
    calls.push(json!({"name": "verify", "args": {"checks": ["onsets"]}}));
    calls.push(json!({"name": "listen", "args": {"zoom": {"t": [0, 0.1]}, "descriptors": false}}));
    calls.push(json!({"name": "listen", "args": {"zoom": {"t0": 0}}}));
    calls.push(json!({"name": "export", "args": {"path": dir.join("out").display().to_string(), "variations": 2}}));
    let r = drive(&Value::Array(calls), &dir);
    let ok = |i: usize| r[i]["ok"].as_bool().unwrap_or(false);

    // 1..=16: one per tool, in TOOLS order.
    for (i, t) in TOOLS.iter().enumerate() {
        let txt = text_of(&r[i + 1]);
        println!("{t}: {txt}");
        assert!(!ok(i + 1), "{t} accepted bogus_key: {txt}");
        assert!(
            txt.starts_with(&format!("{t}: unknown argument `bogus_key`"))
                && txt.contains("it takes:"),
            "{t}: {txt}"
        );
    }
    let at = |k: usize| 17 + k;
    // describe_op `name`, listen `zoom_in`, sound_ops `delete`, nested expect key.
    let t = text_of(&r[at(0)]);
    assert!(
        !ok(at(0)) && t.contains("`name`") && t.contains("kind, type, node"),
        "{t}"
    );
    let t = text_of(&r[at(1)]);
    assert!(
        !ok(at(1)) && t.contains("`zoom_in`") && t.contains("zoom"),
        "{t}"
    );
    let t = text_of(&r[at(2)]);
    assert!(
        !ok(at(2))
            && t.contains(
                r#"ops: [{"type": "delete", "name": "a"}, {"type": "delete", "name": "b"}]"#
            ),
        "{t}"
    );
    assert!(!ok(at(3)), "nested unknown key: {}", text_of(&r[at(3)]));
    // Aliases give the node's example, not the list; no args lists.
    for k in 4..=6 {
        let t = text_of(&r[at(k)]);
        assert!(ok(at(k)) && t.contains("\"type\": \"filter\""), "{k}: {t}");
    }
    let list = text_of(&r[at(7)]);
    assert!(ok(at(7)) && list.contains("filter") && list.contains("osc"));
    assert!(list.len() > text_of(&r[at(6)]).len() / 4);
    let t = text_of(&r[at(8)]);
    assert!(!ok(at(8)) && t.contains("disagree"), "{t}");
    // Build, then the verify forms.
    assert!(ok(at(9)), "{}", text_of(&r[at(9)]));
    let t = text_of(&r[at(10)]);
    assert!(
        ok(at(10)) && t.contains("PASS clicks") && t.contains("PASS finite"),
        "{t}"
    );
    let t = text_of(&r[at(11)]);
    assert!(
        ok(at(11)) && t.contains("clicks") && !t.contains("finite"),
        "{t}"
    );
    let t = text_of(&r[at(12)]);
    assert!(
        ok(at(12)) && t.contains("clicks") && !t.contains("finite"),
        "{t}"
    );
    let t = text_of(&r[at(13)]);
    assert!(
        ok(at(13)) && t.contains("duration") && t.contains("onsets"),
        "{t}"
    );
    let t = text_of(&r[at(14)]);
    assert!(!ok(at(14)) && t.contains("needs settings"), "{t}");
    assert!(ok(at(15)), "{}", text_of(&r[at(15)]));
    // A nested zoom key is refused too (serde names the keys it takes).
    let t = text_of(&r[at(16)]);
    println!("zoom: {t}");
    assert!(!ok(at(16)) && t.contains("t0"), "{t}");
    // This graph is noisy, so the variations differ: no warning.
    let t = text_of(&r[at(17)]);
    assert!(ok(at(17)) && !t.contains("WARNING"), "{t}");
}
