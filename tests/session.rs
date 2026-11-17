//! The session: history, attribution, knob moves, comments, autosave,
//! catalog and export. No server, no env vars: each test uses its own
//! project folder (the library is `<project>/.sounder`).

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use std::path::PathBuf;

use serde_json::{Value, json};
use sfx_graph::{Claim, DocEdit, Kind, Macro, MapCurve, MapEntry, Measure};
use sounder::session::{Annotation, Session, Travel};
use sounder::{catalog, export};

fn project(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_s_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn lp_graph() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "amp": 0.3}),
        json!({"type": "decay", "name": "e", "time": 0.2}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 1000}),
        json!({"type": "output", "name": "out", "input": "f", "lufs": -20}),
    ]
}

fn brightness() -> Macro {
    Macro {
        name: "brightness".into(),
        value: 0.5,
        default: None,
        doc: "cutoff".into(),
        claim: Claim(Measure::Centroid, 1),
        game: false,
        map: vec![MapEntry {
            node: "f".into(),
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

fn build(s: &mut Session, name: &str) {
    s.new_sound(name, Kind::Oneshot).unwrap();
    s.apply_ops(None, lp_graph(), DocEdit::default()).unwrap();
}

#[test]
fn history_undo_redo_jump_and_one_step_per_call() {
    let mut s = Session::new(Some(project("hist")));
    build(&mut s, "t");
    s.apply_ops(
        None,
        vec![json!({"type": "decay", "name": "e", "time": 0.3})],
        DocEdit::default(),
    )
    .unwrap();
    // An identical re-send records nothing.
    s.apply_ops(
        None,
        vec![json!({"type": "decay", "name": "e", "time": 0.3})],
        DocEdit::default(),
    )
    .unwrap();
    let h = s.history_text(None, 30).unwrap();
    assert!(h.contains("3 history step(s)") && h.contains("`~e`"), "{h}");
    s.travel(None, Travel::Undo, false).unwrap();
    assert_eq!(s.sound(None).unwrap().doc.nodes[1]["time"], json!(0.2));
    s.travel(None, Travel::Redo, false).unwrap();
    assert_eq!(s.sound(None).unwrap().doc.nodes[1]["time"], json!(0.3));
    s.travel(None, Travel::Jump(0), false).unwrap();
    assert!(s.sound(None).unwrap().doc.nodes.is_empty());
    assert!(
        s.travel(None, Travel::Jump(9), false)
            .unwrap_err()
            .contains("no history node #9")
    );
    // Editing at an older node branches; the old branch stays reachable.
    s.apply_ops(None, lp_graph(), DocEdit::default()).unwrap();
    assert_eq!(s.sound(None).unwrap().history.nodes.len(), 4);
    s.travel(None, Travel::Jump(2), false).unwrap();
    assert_eq!(s.sound(None).unwrap().doc.nodes[1]["time"], json!(0.3));
}

#[test]
fn clients_are_credited_notified_and_protected() {
    let mut s = Session::new(Some(project("clients")));
    s.begin_call(Some("alice"), None, None);
    build(&mut s, "door");
    s.end_call(Some("alice"));
    // Bob works in the same sound and edits it.
    s.begin_call(Some("bob"), Some("door"), None);
    s.apply_ops(
        Some("door"),
        vec![json!({"type": "decay", "name": "e", "time": 0.4})],
        DocEdit::default(),
    )
    .unwrap();
    s.end_call(Some("bob"));
    // Alice is told what bob changed since her last call.
    let note = s.begin_call(Some("alice"), None, None).unwrap();
    assert!(
        note.contains("others changed `door`") && note.contains("~e (by bob)"),
        "{note}"
    );
    // ...and cannot undo his step without force.
    let e = s.travel(None, Travel::Undo, false).unwrap_err();
    assert!(e.contains("made by bob") && e.contains("force"), "{e}");
    s.travel(None, Travel::Undo, true).unwrap();
    // Each client has its own active sound.
    s.new_sound("window", Kind::Oneshot).unwrap();
    s.end_call(Some("alice"));
    s.begin_call(Some("bob"), None, None);
    assert_eq!(s.active_name().as_deref(), Some("door"));
    s.end_call(Some("bob"));
    s.begin_call(Some("alice"), None, None);
    assert_eq!(s.active_name().as_deref(), Some("window"));
    let e = s.sound(Some("nope")).err().unwrap();
    assert!(
        e.contains("is not open") && e.contains("door, window"),
        "{e}"
    );
}

#[test]
fn macros_knob_moves_and_summary() {
    let mut s = Session::new(Some(project("macros")));
    build(&mut s, "t");
    let edit = DocEdit {
        macros: Some(vec![brightness()]),
        ..DocEdit::default()
    };
    s.apply_ops(None, vec![], edit).unwrap();
    assert_eq!(s.sound(None).unwrap().doc.macros[0].default, Some(0.5));
    let (text, json) = s.summary(None).unwrap();
    assert!(
        text.contains("brightness 0.50 (agent default 0.50)") && text.contains("claim unchecked"),
        "{text}"
    );
    assert_eq!(json["macros"][0]["name"], "brightness");
    // verify confirms the claim; summary then says so.
    let checks = s.check(None, None).unwrap();
    assert!(
        checks
            .iter()
            .any(|c| c.name == "macro brightness" && c.pass),
        "{checks:?}"
    );
    let (text, _) = s.summary(None).unwrap();
    assert!(text.contains("claim holds"), "{text}");
    // The human turns the knob; the agent reads the move once.
    let before = s.sound(None).unwrap().rendered(None).unwrap().lufs;
    s.move_knob(None, "brightness", 0.9).unwrap();
    s.move_knob(None, "brightness", 0.8).unwrap();
    assert!(
        s.move_knob(None, "nope", 0.1)
            .unwrap_err()
            .contains("no macro named `nope`")
    );
    let (text, json) = s.summary(None).unwrap();
    assert!(text.contains("brightness 0.50 -> 0.80"), "{text}");
    assert_eq!(json["knob_moves"][0]["to"], json!(0.8));
    assert!(
        text.contains("agent default 0.50"),
        "the reset value stays: {text}"
    );
    let (text, _) = s.summary(None).unwrap();
    assert!(!text.contains("knob"), "reading clears them: {text}");
    // The move changed the sound; no history step was recorded for it.
    let after = s.sound(None).unwrap().rendered(None).unwrap().lufs;
    assert!((after - before).abs() < 3.0);
    assert_eq!(s.sound(None).unwrap().history.nodes.len(), 3);
    // After the agent read it, a move is from there (0.80): moving away and
    // back leaves no net move.
    s.move_knob(None, "brightness", 0.5).unwrap();
    assert_eq!(s.sound(None).unwrap().knob_moves[0].from, 0.8);
    s.move_knob(None, "brightness", 0.8).unwrap();
    assert!(s.sound(None).unwrap().knob_moves.is_empty());
}

#[test]
fn comments_are_resolved_not_deleted() {
    let mut s = Session::new(Some(project("comments")));
    build(&mut s, "t");
    let a = Annotation {
        id: 0,
        text: "too whistly".into(),
        by: Some("you".into()),
        node: Some("f".into()),
        time: Some([0.0, 0.07]),
        hz: Some([2500.0, 9000.0]),
        resolved: false,
        reply: None,
        resolved_by: None,
    };
    s.add_annotation(None, a.clone()).unwrap();
    let e = s
        .add_annotation(
            None,
            Annotation {
                node: Some("zzz".into()),
                ..a
            },
        )
        .unwrap_err();
    assert!(
        e.contains("no node named `zzz`") && e.contains("nodes: n, e, m, f, out"),
        "{e}"
    );
    let (text, _) = s.summary(None).unwrap();
    assert!(
        text.contains("open comments (1)") && text.contains("node `f` 0.00-0.07 s 2500-9000 Hz"),
        "{text}"
    );
    s.resolve_annotation(None, 1, "lowered the cutoff", false)
        .unwrap();
    let (text, _) = s.summary(None).unwrap();
    assert!(
        !text.contains("open comments") && text.contains("1 resolved"),
        "{text}"
    );
    s.resolve_annotation(None, 1, "", true).unwrap();
    assert!(s.summary(None).unwrap().0.contains("open comments (1)"));
    assert!(
        s.resolve_annotation(None, 7, "x", false)
            .unwrap_err()
            .contains("no comment #7")
    );
}

#[test]
fn a_failing_batch_leaves_the_sound_and_history_alone() {
    let mut s = Session::new(Some(project("atomic")));
    build(&mut s, "t");
    let before = s.sound(None).unwrap().doc.clone();
    let steps = s.sound(None).unwrap().history.nodes.len();
    let e = s
        .apply_ops(
            None,
            vec![
                json!({"type": "osc", "name": "o2", "freq": 440}),
                json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": "oops"}),
            ],
            DocEdit::default(),
        )
        .unwrap_err();
    assert!(e.contains("oops"), "{e}");
    assert_eq!(s.sound(None).unwrap().doc, before);
    assert_eq!(s.sound(None).unwrap().history.nodes.len(), steps);
}

#[test]
fn autosave_restores_sounds_history_and_comments() {
    let proj = project("autosave");
    {
        let mut s = Session::new(Some(proj.clone()));
        build(&mut s, "keep");
        s.apply_ops(
            None,
            vec![],
            DocEdit {
                macros: Some(vec![brightness()]),
                ..DocEdit::default()
            },
        )
        .unwrap();
        s.add_annotation(
            None,
            Annotation {
                id: 0,
                text: "hello".into(),
                by: None,
                node: None,
                time: None,
                hz: None,
                resolved: false,
                reply: None,
                resolved_by: None,
            },
        )
        .unwrap();
        s.move_knob(None, "brightness", 0.7).unwrap();
        s.flush_autosave(true);
    }
    let mut s = Session::new(Some(proj));
    assert_eq!(s.restore().unwrap(), 1);
    let snd = s.sound(Some("keep")).unwrap();
    assert_eq!(snd.doc.macros[0].value, 0.7);
    assert_eq!(snd.doc.macros[0].default, Some(0.5));
    assert_eq!(snd.history.nodes.len(), 3);
    assert_eq!(snd.annotations.len(), 1);
    assert_eq!(snd.knob_moves.len(), 1, "an unread move survives a restart");
    assert_eq!(s.active_name().as_deref(), Some("keep"));
    // The restored sound still renders and undoes.
    s.travel(None, Travel::Undo, false).unwrap();
    assert!(
        !s.summary(None).unwrap().0.contains("macros:"),
        "step 1 had no macros yet"
    );
}

#[test]
fn catalog_roundtrip_and_closing() {
    let mut s = Session::new(Some(project("catalog")));
    build(&mut s, "t");
    s.apply_ops(
        None,
        vec![],
        DocEdit {
            macros: Some(vec![brightness()]),
            ..DocEdit::default()
        },
    )
    .unwrap();
    let lib = s.library_dir().unwrap();
    let doc = s.sound(None).unwrap().doc.clone();
    catalog::save(
        &lib,
        &doc,
        catalog::CatalogEntry {
            name: "t".into(),
            description: "noise burst".into(),
            tags: vec!["test".into()],
            prompt: None,
            kind: doc.kind,
            length: 0.2,
            lufs: -20.0,
            macros: vec![],
        },
    )
    .unwrap();
    let list = catalog::format_list(&catalog::list(&lib));
    assert!(
        list.contains("`t`: noise burst [test]") && list.contains("knobs: brightness"),
        "{list}"
    );
    let loaded = catalog::load(&lib, "t").unwrap();
    assert_eq!(loaded.nodes, doc.nodes);
    assert!(catalog::load(&lib, "zz").unwrap_err().contains("saved: t"));
    // Loading over an open sound needs force; closing unsaved work too.
    assert!(
        s.open_doc(loaded.clone(), "t", false)
            .unwrap_err()
            .contains("already open")
    );
    s.open_doc(loaded, "t", true).unwrap();
    assert!(
        s.close_sound("t", false).is_ok(),
        "just loaded: nothing unsaved"
    );
    s.new_sound("u", Kind::Loop).unwrap();
    s.apply_ops(
        None,
        vec![],
        DocEdit {
            duration: Some(Some(1.0)),
            ..DocEdit::default()
        },
    )
    .unwrap_err();
    assert!(s.close_sound("u", false).unwrap_err().contains("unsaved"));
    catalog::delete(&lib, "t").unwrap();
    assert!(catalog::list(&lib).is_empty());
}

#[test]
fn export_writes_wavs_and_a_manifest_with_game_inputs() {
    let proj = project("export");
    let mut s = Session::new(Some(proj.clone()));
    build(&mut s, "boom");
    let mut m = brightness();
    m.game = true;
    s.apply_ops(
        None,
        vec![],
        DocEdit {
            macros: Some(vec![m]),
            ..DocEdit::default()
        },
    )
    .unwrap();
    let doc = s.sound(None).unwrap().doc.clone();
    let dir = export::dir_for(Some(&proj), None);
    assert_eq!(dir, proj.join("assets").join("sounds"));
    let ex = export::export(
        &doc,
        &export::ExportOpts {
            dir: dir.clone(),
            variations: 2,
            bits: sfx_graph::wav::Bits::Int24,
            sample_rate: Some(44_100),
            base_dir: None,
            catalog_name: Some("boom".into()),
        },
    )
    .unwrap();
    assert_eq!(ex.files.len(), 2);
    assert!(
        dir.join("boom_1.wav").is_file()
            && dir.join("boom_2.wav").is_file()
            && dir.join("boom.sfx.json").is_file()
    );
    assert_eq!(ex.manifest["sample_rate"], 44_100);
    assert_eq!(ex.manifest["bits"], 24);
    assert_eq!(ex.manifest["inputs"][0]["name"], "brightness");
    assert_eq!(ex.manifest["inputs"][0]["default"], 0.5);
    assert_eq!(ex.manifest["source"], "boom");
    let wav = sfx_graph::wav::read(&dir.join("boom_1.wav")).unwrap();
    assert_eq!(wav.sample_rate, 44_100);
    // A loop gets loop points.
    let mut l = doc.clone();
    l.kind = Kind::Loop;
    l.duration = Some(0.5);
    let ex = export::export(
        &l,
        &export::ExportOpts {
            dir,
            variations: 1,
            bits: sfx_graph::wav::Bits::Int16,
            sample_rate: None,
            base_dir: None,
            catalog_name: None,
        },
    )
    .unwrap();
    assert_eq!(ex.manifest["loop"]["end"], 24_000);
}
