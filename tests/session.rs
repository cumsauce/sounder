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
        claim: Some(Claim(Measure::Centroid, 1)),
        game: false,
        map: vec![MapEntry {
            node: "f".into(),
            field: "freq".into(),
            from: 300.0,
            to: 8000.0,
            curve: MapCurve::Exp,
            param: None,
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
    let (o, g) = (s.render_opts(), s.generation());
    let before = s
        .sound(None)
        .unwrap()
        .rendered(&o, g)
        .unwrap()
        .audio
        .mix_mono();
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
    let (o, g) = (s.render_opts(), s.generation());
    let after = s
        .sound(None)
        .unwrap()
        .rendered(&o, g)
        .unwrap()
        .audio
        .mix_mono();
    assert_ne!(after, before, "the knob must change the samples");
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
            role: None,
            material: None,
            approved: false,
            builtin: false,
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
    .unwrap(); // settings on a sound with no nodes yet are kept, unrendered
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
            format: export::Format::Wav(sfx_graph::wav::Bits::Int24),
            sample_rate: Some(44_100),
            base_dir: None,
            sources: Vec::new(),
            sounds: None,
            catalog_name: Some("boom".into()),
            file_name: None,
            timeline: Default::default(),
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
    // A loop gets loop points; the default format is Ogg Vorbis, sample-exact.
    let mut l = doc.clone();
    l.kind = Kind::Loop;
    l.duration = Some(0.5);
    let ex = export::export(
        &l,
        &export::ExportOpts {
            dir: dir.clone(),
            variations: 1,
            format: export::Format::default(),
            sample_rate: None,
            base_dir: None,
            sources: Vec::new(),
            sounds: None,
            catalog_name: None,
            file_name: None,
            timeline: Default::default(),
        },
    )
    .unwrap();
    assert_eq!(ex.manifest["loop"]["end"], 24_000);
    assert_eq!(ex.manifest["format"], "ogg");
    let ogg = dir.join("boom.ogg");
    let bytes = std::fs::read(&ogg).unwrap();
    assert_eq!(&bytes[..4], b"OggS");
    let wav_bytes = 24_000 * 2; // mono 16-bit
    assert!(bytes.len() < wav_bytes / 3, "ogg {} B", bytes.len());
    let mut dec = vorbis_rs::VorbisDecoder::new(std::fs::File::open(&ogg).unwrap()).unwrap();
    assert_eq!(dec.sampling_frequency().get(), 48_000);
    let mut n = 0;
    while let Some(b) = dec.decode_audio_block().unwrap() {
        n += b.samples()[0].len();
    }
    assert_eq!(n, 24_000, "decoded length is the loop length");
}

#[test]
fn sound_nodes_reach_open_and_saved_sounds_and_follow_edits() {
    let mut s = Session::new(Some(project("instance")));
    // `thud` is open; `layers` instances it.
    s.new_sound("thud", Kind::Oneshot).unwrap();
    s.apply_ops(
        None,
        vec![
            json!({"type": "osc", "name": "o", "freq": 90, "amp": 0.8}),
            json!({"type": "decay", "name": "e", "time": 0.15}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "m", "lufs": -18}),
        ],
        DocEdit::default(),
    )
    .unwrap();
    s.new_sound("layers", Kind::Oneshot).unwrap();
    let report = s
        .apply_ops(
            None,
            vec![
                json!({"type": "sound", "name": "a", "sound": "thud"}),
                json!({"type": "sound", "name": "b", "sound": "thud", "at": 0.3, "gain_db": -6}),
                json!({"type": "mix", "name": "m", "inputs": ["a", "b"]}),
                json!({"type": "output", "name": "out", "input": "m"}),
            ],
            DocEdit {
                duration: Some(Some(0.6)),
                ..DocEdit::default()
            },
        )
        .unwrap();
    assert!(
        report.rendered.audio.peak() > 0.05,
        "{:?}",
        report.rendered.warnings
    );
    let (text, _) = s.summary(None).unwrap();
    assert!(
        text.contains("a (sound)") && !text.contains("<- thud"),
        "a sound name is not a node reference: {text}"
    );
    let before = s.summary(None).unwrap().1["lufs"].as_f64().unwrap();
    // Edit the instanced sound: the parent's summary follows (no stale cache).
    s.apply_ops(
        Some("thud"),
        vec![json!({"type": "output", "name": "out", "input": "m", "lufs": -30})],
        DocEdit::default(),
    )
    .unwrap();
    let after = s.summary(Some("layers")).unwrap().1["lufs"]
        .as_f64()
        .unwrap();
    assert!(after < before - 5.0, "{before} -> {after}");
    // A saved (catalog) sound is found when it is not open.
    let lib = s.library_dir().unwrap();
    let doc = s.sound(Some("thud")).unwrap().doc.clone();
    catalog::save(
        &lib,
        &doc,
        catalog::CatalogEntry {
            name: "thud".into(),
            description: String::new(),
            tags: vec![],
            prompt: None,
            kind: doc.kind,
            length: 0.2,
            lufs: -30.0,
            macros: vec![],
            role: None,
            material: None,
            approved: false,
            builtin: false,
        },
    )
    .unwrap();
    s.close_sound("thud", true).unwrap();
    let rep = s
        .apply_ops(
            Some("layers"),
            vec![
                json!({"type": "gain", "name": "g", "input": "m", "db": 0}),
                json!({"type": "output", "name": "out", "input": "g"}),
            ],
            DocEdit::default(),
        )
        .unwrap();
    assert!(
        rep.rendered.audio.peak() > 0.001,
        "instanced from the catalog"
    );
}

#[test]
fn editing_a_macro_driven_field_warns_that_the_value_is_ignored() {
    let mut s = Session::new(Some(project("macro_warn")));
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
    // `f.freq` is driven by `brightness`: setting it is a no-op the agent must hear about.
    let r = s
        .apply_ops(
            None,
            vec![json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 500})],
            DocEdit::default(),
        )
        .unwrap();
    assert!(
        r.text
            .contains("WARNING node `f` field `freq` is driven by macro `brightness`")
            && r.text.contains("your value is ignored"),
        "{}",
        r.text
    );
    // A field no macro drives does not warn.
    let r = s
        .apply_ops(None, vec![json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 500, "q": 2.0})], DocEdit::default())
        .unwrap();
    assert!(!r.text.contains("driven by macro"), "{}", r.text);
}

#[test]
fn hostile_names_and_values_are_errors_not_crashes() {
    let d = std::env::temp_dir().join(format!("sounder_hostile_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(sounder::catalog::load(&d, "../x").is_err());
    let mut doc = sfx_graph::SoundDoc::named("t");
    doc.duration = Some(0.1);
    doc.nodes = vec![
        serde_json::json!({"type": "osc", "name": "o", "freq": 440}),
        serde_json::json!({"type": "gain", "name": "g", "input": "o", "db": 1000}),
        serde_json::json!({"type": "output", "name": "out", "input": "g"}),
    ];
    let r = doc.render(&sfx_graph::RenderOpts::default()).unwrap();
    assert!(r.audio.channels[0].iter().all(|x| x.is_finite()));
    doc.nodes = vec![
        serde_json::json!({"type": "wander", "name": "w", "range": [200, 800], "rate": 1e15}),
        serde_json::json!({"type": "output", "name": "out", "input": "w"}),
    ];
    assert!(doc.render(&sfx_graph::RenderOpts::default()).is_err());
}

#[test]
fn saved_sounds_that_are_not_open_are_listed_as_recent() {
    let proj = project("recent");
    let mut s = Session::new(Some(proj.clone()));
    build(&mut s, "old_one");
    let lib = s.library_dir().unwrap();
    let doc = s.sound(None).unwrap().doc.clone();
    let entry = |n: &str| sounder::catalog::CatalogEntry {
        name: n.into(),
        description: String::new(),
        tags: vec![],
        prompt: None,
        kind: sfx_graph::Kind::Oneshot,
        length: 0.4,
        lufs: -18.0,
        macros: vec![],
        role: None,
        material: None,
        approved: false,
        builtin: false,
    };
    sounder::catalog::save(&lib, &doc, entry("old_one")).unwrap();
    // Open: not in `recent`.
    assert!(
        s.window_state(None).unwrap()["recent"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    s.close_sound("old_one", true).unwrap();
    build(&mut s, "fresh");
    let r = s.window_state(None).unwrap();
    let names: Vec<&str> = r["recent"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["name"].as_str())
        .collect();
    assert_eq!(names, ["old_one"], "{r}");
}

#[test]
fn a_spec_is_listed_in_summary_and_checked_by_verify() {
    let mut s = Session::new(Some(project("spec")));
    build(&mut s, "thud");
    let (text, _) = s.summary(None).unwrap();
    assert!(text.contains("no spec yet"), "{text}");
    let spec: sfx_graph::Spec =
        serde_json::from_value(json!({"brief": "a dull thud", "components": [
            {"role": "weight", "node": "f", "band": [20, 1500]},
            {"role": "crack", "note": "still to build"}
        ]}))
        .unwrap();
    let edit = DocEdit {
        spec: Some(Some(spec)),
        ..DocEdit::default()
    };
    assert!(s.apply_ops(None, Vec::new(), edit).unwrap().changed);
    let (text, json) = s.summary(None).unwrap();
    assert!(text.contains("brief: a dull thud"), "{text}");
    assert!(
        text.contains("- crack (no node yet, 1500-16000 Hz (role)): still to build [not built"),
        "{text}"
    );
    assert_eq!(json["spec"]["components"][0]["node"], "f");
    let r = s.check(None, None).unwrap();
    let spec: Vec<_> = r.iter().filter(|c| c.name.starts_with("spec")).collect();
    assert_eq!(spec.len(), 2, "{r:?}");
    assert!(spec[0].pass && !spec[1].pass, "{spec:?}");
    // One component at a time: name the crack's node, append a layer.
    let edit = DocEdit {
        spec_update: vec![
            json!({"index": 1, "node": "f", "note": null}),
            json!({"index": 2, "role": "after", "material": "wood"}),
        ],
        ..DocEdit::default()
    };
    s.apply_ops(None, Vec::new(), edit).unwrap();
    let (_, json) = s.summary(None).unwrap();
    assert_eq!(json["spec"]["components"][1]["node"], "f");
    assert!(json["spec"]["components"][1].get("note").is_none());
    assert_eq!(json["spec"]["components"][2]["role"], "after");
    for (item, want) in [
        (
            json!({"index": 9, "node": "f"}),
            "the spec has 3 components",
        ),
        (json!({"node": "f"}), "`index`"),
        (json!({"index": 0, "role": "thud"}), "role `thud`"),
    ] {
        let edit = DocEdit {
            spec_update: vec![item],
            ..DocEdit::default()
        };
        let e = s.apply_ops(None, Vec::new(), edit).unwrap_err();
        assert!(e.contains(want), "{e}");
    }
    // null removes it.
    let edit = DocEdit {
        spec: Some(None),
        ..DocEdit::default()
    };
    s.apply_ops(None, Vec::new(), edit).unwrap();
    assert!(s.summary(None).unwrap().0.contains("no spec yet"));
}

/// The `heard_as` worker protocol, with a fake worker standing in for the
/// model: a ready line, then one JSON reply per request; and the off path.
#[test]
fn heard_as_talks_to_its_worker_and_says_when_it_is_off() {
    let dir = project("hearing");
    let script = dir.join("fake.py");
    std::fs::write(
        &script,
        r#"import json, sys
print(json.dumps({"ready": "fake-model"}), flush=True)
for line in sys.stdin:
    req = json.loads(line)
    out = {"top": [["a heavy thud", 0.4], ["a drum being hit", 0.3]]}
    if req.get("brief"):
        out["brief"] = {"score": 0.2, "rank": 4, "of": 102}
    print(json.dumps(out), flush=True)
"#,
    )
    .unwrap();
    let mut h = sounder::hearing::Hearing::default();
    h.python = Some("python3".into());
    h.script = Some(script);
    let audio = sfx_graph::Audio {
        sample_rate: 48000,
        channels: vec![vec![0.0; 480]],
    };
    let (heard, model) = h.hear(&audio, "a dull thud").unwrap();
    assert_eq!(model, "fake-model");
    let line = heard.line(&model);
    assert!(
        line.contains("a heavy thud 0.40") && line.contains("ranks 4 of 102"),
        "{line}"
    );
    // The worker stays up: a second request, no brief.
    let (heard, _) = h.hear(&audio, "").unwrap();
    assert!(heard.brief.is_none());

    let mut off = sounder::hearing::Hearing::default();
    off.python = Some(dir.join("no-such-python"));
    let e = off.hear(&audio, "").unwrap_err();
    assert!(e.contains("could not start"), "{e}");
    // Once off, it stays off without trying again.
    assert_eq!(off.hear(&audio, "").unwrap_err(), e);
}
