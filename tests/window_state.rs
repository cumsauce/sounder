//! `Session::window_state` against the shape in `design/window-api.md`:
//! every key and type the page reads, built from a real session.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sfx_graph::{Claim, DocEdit, Kind, Macro, MapCurve, MapEntry, Measure};
use sounder::session::{Annotation, Session};

fn project(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_ws_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn knob() -> Macro {
    Macro {
        name: "brightness".into(),
        value: 0.5,
        default: None,
        doc: "how cutting the zap is".into(),
        claim: Claim(Measure::Centroid, 1),
        game: false,
        map: vec![MapEntry {
            node: "lp".into(),
            field: "freq".into(),
            from: 300.0,
            to: 8000.0,
            curve: MapCurve::Exp,
            col: None,
            index: None,
            key: None,
        }],
    }
}

fn session(tag: &str) -> Session {
    let mut s = Session::new(Some(project(tag)));
    s.autosave_ok = false;
    let _ = s.begin_call(Some("look"), None, None);
    s.new_sound("zap", Kind::Oneshot).unwrap();
    let graph = vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "decay", "name": "e", "time": 0.2}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "filter", "name": "lp", "input": "m", "mode": "lp", "freq": 1000, "q": 1.5}),
        json!({"type": "output", "name": "out", "input": "lp", "lufs": -20}),
    ];
    let edit = DocEdit {
        macros: Some(vec![knob()]),
        params: Some([("base".to_string(), 220.0)].into_iter().collect()),
        ..DocEdit::default()
    };
    s.apply_ops(None, graph, edit).unwrap();
    s.end_call(Some("look"));
    s.log_call("sound_ops", true, 12, Some("look"));
    s
}

fn is_num(v: &Value) -> bool {
    v.is_number()
}

#[test]
fn state_has_every_key_the_contract_names() {
    let mut s = session("shape");
    // A comment, a knob move, and the sweep run by `verify`.
    s.begin_call(Some("window"), None, None);
    s.add_annotation(
        None,
        Annotation {
            id: 0,
            text: "too hissy".into(),
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
    s.end_call(Some("window"));
    s.begin_call(Some("look"), Some("zap"), None);
    s.check(None, None).unwrap();
    s.end_call(Some("look"));

    let st = s.window_state(Some("zap")).unwrap();
    assert!(is_num(&st["rev"]) && st["rev"].as_u64().unwrap() > 0);
    assert_eq!(st["doc"], "zap");
    assert_eq!(st["solo"], Value::Null);
    assert_eq!(st["playing"], false);

    let sounds = st["sounds"].as_array().unwrap();
    let z = &sounds[0];
    assert_eq!(z["name"], "zap");
    assert_eq!(z["kind"], "oneshot");
    assert_eq!(z["by"], "look");
    assert!(z["ago"].is_string());
    assert!(z["working"].is_boolean() && z["saved"] == false);
    assert_eq!(z["open_comments"], 1);

    let agents = st["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1, "the window is not an agent: {agents:?}");
    assert_eq!(agents[0]["name"], "look");
    assert!(agents[0]["live"].as_bool().unwrap());

    let feed = &st["feed"][0];
    assert_eq!(feed["agent"], "look");
    assert_eq!(feed["tool"], "sound_ops");
    assert!(feed["what"].as_str().unwrap().starts_with("zap · "));
    assert!(feed["ago"].as_str().unwrap().contains(':'));
    assert_eq!(feed["doc"], "zap");

    let snd = &st["sound"];
    for k in [
        "name",
        "kind",
        "version",
        "seed",
        "sample_rate",
        "duration",
        "seconds",
        "peak_db",
        "lufs",
        "causal",
        "warnings",
        "nodes",
        "params",
        "macros",
        "moves",
        "comments",
        "history",
        "checks",
        "descriptors",
    ] {
        assert!(snd.get(k).is_some(), "sound.{k} missing");
    }
    assert_eq!(snd["duration"], Value::Null);
    assert!(snd["seconds"].as_f64().unwrap() > 0.0);
    assert!(is_num(&snd["peak_db"]) && is_num(&snd["lufs"]));
    assert!(snd["causal"].is_boolean() && snd["warnings"].is_array());
    assert_eq!(snd["params"]["base"], 220.0);

    // Nodes in evaluation order with reads, raw fields, depth.
    let nodes = snd["nodes"].as_array().unwrap();
    let names: Vec<&str> = nodes.iter().map(|n| n["name"].as_str().unwrap()).collect();
    let pos = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(
        pos("n") < pos("m")
            && pos("e") < pos("m")
            && pos("m") < pos("lp")
            && pos("lp") < pos("out")
    );
    let lp = &nodes[pos("lp")];
    for k in [
        "name",
        "type",
        "reads",
        "fields",
        "effective",
        "unread",
        "open_comments",
        "depth",
    ] {
        assert!(lp.get(k).is_some(), "node.{k} missing");
    }
    assert_eq!(lp["type"], "filter");
    assert_eq!(lp["reads"], json!(["m"]));
    assert_eq!(lp["fields"]["mode"], "lp");
    assert_eq!(lp["fields"]["input"], "m");
    assert!(lp["fields"].get("type").is_none() && lp["fields"].get("name").is_none());
    assert_eq!(lp["open_comments"], 1);
    assert_eq!(lp["depth"], 2);
    assert_eq!(nodes[pos("n")]["depth"], 0);
    // The macro drives freq: raw says 1000, effective is the knob's value.
    assert_eq!(lp["fields"]["freq"], 1000);
    let eff = lp["effective"]["freq"].as_f64().unwrap();
    let want = 300.0 * (8000.0f64 / 300.0).powf(0.8);
    assert!((eff - want).abs() < 1.0, "{eff} vs {want}");
    assert!(lp["effective"].get("q").is_none());
    assert_eq!(nodes[pos("n")]["effective"], json!({}));

    // Macros with the claim result (the sweep ran) and the human's moves.
    let m = &snd["macros"][0];
    for k in [
        "name", "value", "default", "doc", "claim", "game", "map", "result",
    ] {
        assert!(m.get(k).is_some(), "macro.{k} missing");
    }
    assert_eq!(m["value"], 0.8);
    assert_eq!(m["default"], 0.5);
    assert_eq!(m["claim"], json!(["centroid", 1]));
    let e = &m["map"][0];
    for k in [
        "node", "field", "from", "to", "curve", "col", "index", "key",
    ] {
        assert!(e.get(k).is_some(), "map.{k} missing");
    }
    assert_eq!(e["curve"], "exp");
    assert!(e["col"].is_null());
    assert!(m["result"]["pass"].is_boolean(), "{m}");
    assert!(m["result"]["detail"].as_str().unwrap().contains("centroid"));
    assert_eq!(
        snd["moves"],
        json!([{"name": "brightness", "from": 0.5, "to": 0.8}])
    );

    let c = &snd["comments"][0];
    for k in [
        "id", "text", "by", "node", "time", "hz", "resolved", "reply",
    ] {
        assert!(c.get(k).is_some(), "comment.{k} missing");
    }
    assert_eq!((c["id"].as_u64(), c["by"].as_str()), (Some(1), Some("you")));
    assert_eq!(c["resolved"], false);
    assert!(c["reply"].is_null());

    let h = snd["history"].as_array().unwrap();
    assert_eq!(h.len(), 2);
    for k in ["id", "label", "by", "version", "current"] {
        assert!(h[1].get(k).is_some(), "history.{k} missing");
    }
    assert_eq!(h[1]["by"], "look");
    assert_eq!(h[1]["current"], true);
    assert_eq!(h[0]["current"], false);

    let checks = snd["checks"].as_array().unwrap();
    assert!(!checks.is_empty());
    assert!(
        checks
            .iter()
            .all(|c| c["name"].is_string() && c["pass"].is_boolean() && c["detail"].is_string())
    );

    let d = &snd["descriptors"];
    for k in ["lufs", "centroid_hz", "modes"] {
        assert!(d.get(k).is_some(), "descriptors.{k} missing: {d}");
    }
    assert!(d["bands"].is_array() || d.get("bands_db").is_some() || d.get("bands").is_some());
}

#[test]
fn checks_and_claims_are_empty_until_computed_and_never_stale() {
    let mut s = session("lazy");
    let st = s.window_state(Some("zap")).unwrap();
    assert!(st["sound"]["checks"].as_array().unwrap().is_empty());
    assert!(st["sound"]["macros"][0]["result"].is_null());
    assert_eq!(s.window_needs_settle(Some("zap")).as_deref(), Some("zap"));
    s.check(Some("zap"), None).unwrap();
    assert_eq!(s.window_needs_settle(Some("zap")), None);
    let st = s.window_state(Some("zap")).unwrap();
    assert!(!st["sound"]["checks"].as_array().unwrap().is_empty());
    assert!(st["sound"]["macros"][0]["result"].is_object());
    // A knob move invalidates the claim result and the checks.
    s.move_knob(Some("zap"), "brightness", 0.9).unwrap();
    let st = s.window_state(Some("zap")).unwrap();
    assert!(st["sound"]["macros"][0]["result"].is_null());
    assert!(st["sound"]["checks"].as_array().unwrap().is_empty());
    assert_eq!(s.window_needs_settle(Some("zap")).as_deref(), Some("zap"));
}

#[test]
fn unread_marks_nodes_agents_changed_since_the_window_looked() {
    let mut s = session("unread");
    s.begin_call(Some("window"), Some("zap"), None);
    s.window_mark_seen("zap");
    let st = s.window_state(None).unwrap();
    assert!(
        st["sound"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["unread"] == false)
    );
    s.end_call(Some("window"));
    s.begin_call(Some("look"), Some("zap"), None);
    s.apply_ops(
        None,
        vec![json!({"type": "decay", "name": "e", "time": 0.3})],
        DocEdit::default(),
    )
    .unwrap();
    s.end_call(Some("look"));
    s.begin_call(Some("window"), Some("zap"), None);
    let st = s.window_state(None).unwrap();
    let unread: Vec<&str> = st["sound"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["unread"] == true)
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    assert_eq!(unread, vec!["e"]);
    s.window_mark_seen("zap");
    let st = s.window_state(None).unwrap();
    assert!(
        st["sound"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["unread"] == false)
    );
}

#[test]
fn empty_session_and_unrenderable_sound() {
    let mut s = Session::new(Some(project("empty")));
    s.autosave_ok = false;
    let st = s.window_state(None).unwrap();
    assert_eq!(st["doc"], Value::Null);
    assert_eq!(st["sound"], Value::Null);
    assert!(st["sounds"].as_array().unwrap().is_empty());
    assert!(
        s.window_state(Some("ghost"))
            .unwrap_err()
            .contains("not open")
    );
    // A sound with no nodes does not render: a warning, not an error.
    s.new_sound("blank", Kind::Oneshot).unwrap();
    let st = s.window_state(None).unwrap();
    assert_eq!(st["sound"]["name"], "blank");
    assert!(st["sound"]["seconds"].is_null());
    assert!(
        st["sound"]["warnings"][0]
            .as_str()
            .unwrap()
            .contains("does not render")
    );
}

#[test]
fn rev_bumps_on_every_visible_change() {
    let mut s = session("rev");
    let r0 = s.rev();
    s.move_knob(Some("zap"), "brightness", 0.2).unwrap();
    let r1 = s.rev();
    assert!(r1 > r0);
    s.begin_call(Some("look"), Some("zap"), None);
    s.summary(None).unwrap();
    s.end_call(Some("look"));
    assert!(
        s.rev() > r1,
        "reading the moves changes what the window shows"
    );
    let r2 = s.rev();
    s.window_state(None).unwrap();
    assert_eq!(s.rev(), r2, "state itself changes nothing");
    let a = s.node_audio(Some("zap"), Some("lp")).unwrap();
    assert!(a.len() > 1000);
    assert!(s.node_audio(Some("zap"), Some("ghost")).is_err());
}
