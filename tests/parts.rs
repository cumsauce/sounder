//! The built-in parts: each parses and renders alone, its knob holds its
//! claim, the five door-slam layers reassembled with `sound` nodes pass
//! `layers` (impact), and `catalog search` finds them.

#![allow(clippy::unwrap_used)]

use serde_json::json;
use sfx_analysis::{CheckCx, Expect, run};
use sfx_graph::{RenderOpts, SoundDoc, SoundSource};
use sounder::catalog::{self, Query};
use sounder::parts;

fn opts() -> RenderOpts {
    let names = parts::builtin().into_iter().map(|p| p.doc.name).collect();
    RenderOpts {
        sounds: Some(SoundSource::new(names, |n| parts::find(n).map(|p| p.doc))),
        ..RenderOpts::default()
    }
}

fn failing(doc: &SoundDoc, expect: serde_json::Value) -> Vec<String> {
    let o = opts();
    let r = doc.render(&o).unwrap();
    let e: Expect = serde_json::from_value(expect).unwrap();
    run(
        &e,
        &CheckCx {
            doc,
            rendered: &r,
            opts: &o,
        },
    )
    .unwrap()
    .into_iter()
    .filter(|c| !c.pass)
    .map(|c| format!("{}: {}", c.name, c.detail))
    .collect()
}

#[test]
fn every_part_parses_renders_and_its_knob_holds() {
    let all = parts::builtin();
    assert_eq!(all.len(), 126, "a file in parts/ does not parse");
    for p in &all {
        let name = &p.doc.name;
        assert!(parts::ROLES.contains(&p.role.as_str()), "{name}: role");
        assert!(!p.doc.macros.is_empty(), "{name}: no knob");
        let r = p
            .doc
            .render(&opts())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let x = &r.audio.channels[0];
        assert!(x.iter().all(|v| v.is_finite()), "{name}: not finite");
        println!(
            "{name} ({}): {:.3} s, peak {:.1} dBFS",
            p.role,
            r.audio.seconds(),
            r.peak_db
        );
        assert!(r.peak_db > -50.0, "{name}: silent");
        let bad = failing(&p.doc, json!({"macros": true}));
        assert!(bad.is_empty(), "{name}: {bad:?}");
    }
}

#[test]
fn the_door_slam_layers_reassemble_into_a_passing_impact() {
    let doc: SoundDoc = serde_json::from_value(json!({
        "name": "slam_from_parts", "kind": "oneshot", "sample_rate": 48000,
        "duration": 0.8, "seed": 1, "params": {}, "macros": [],
        "nodes": [
            {"type": "sound", "name": "pre", "sound": "wood_brush"},
            {"type": "sound", "name": "crack", "sound": "noise_crack", "at": 0.045},
            {"type": "sound", "name": "body", "sound": "wood_panel_body", "at": 0.045},
            {"type": "sound", "name": "weight", "sound": "low_thump", "at": 0.045},
            {"type": "sound", "name": "after", "sound": "wood_knock", "at": 0.073},
            {"type": "mix", "name": "dry", "inputs": ["pre", "crack", "body", "weight", "after"], "to": "mono"},
            {"type": "finish", "name": "fin", "input": "dry", "punch": 0.4, "tail_db": -6, "glue": 0.3, "warmth": 0.6, "weight_db": 4},
            {"type": "output", "name": "out", "input": "fin", "lufs": -28}
        ]
    }))
    .unwrap();
    let bad = failing(&doc, json!({"layers": {"preset": "impact"}}));
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn search_filters_by_text_role_material_and_approval() {
    let dir = std::env::temp_dir().join(format!("sounder_parts_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let all = catalog::list_with_builtin(&dir);
    let names = |q: Query| -> Vec<String> {
        catalog::search(&all, &q)
            .into_iter()
            .map(|e| e.name)
            .collect()
    };
    assert_eq!(names(Query::default()).len(), 126);
    assert_eq!(
        names(Query {
            role: Some("body".into()),
            material: Some("wood".into()),
            text: Some("panel".into()),
            ..Query::default()
        }),
        ["wood_panel_body"]
    );
    // `any` material parts fit a wood query too.
    let wood = names(Query {
        material: Some("wood".into()),
        ..Query::default()
    });
    assert!(wood.contains(&"low_thump".to_string()) && wood.contains(&"wood_knock".to_string()));
    assert!(!wood.contains(&"glass_bowl_body".to_string()));
    // Word-based, any word, most word hits first.
    assert_eq!(
        names(Query {
            text: Some("DOOR settling".into()),
            ..Query::default()
        })[0],
        "wood_knock"
    );
    for text in [
        "motor",
        "motors",
        "whir",
        "whirring",
        "machine",
        "vending machine motor",
        "dispense",
    ] {
        let hits = names(Query {
            text: Some(text.into()),
            ..Query::default()
        });
        assert!(
            hits.contains(&"small_motor_spin".to_string()),
            "`{text}` should find small_motor_spin: {hits:?}"
        );
    }
    assert_eq!(
        names(Query {
            text: Some("zzzqx".into()),
            ..Query::default()
        }),
        Vec::<String>::new()
    );
    assert!(
        names(Query {
            approved: Some(false),
            ..Query::default()
        })
        .is_empty()
    );
    assert!(
        names(Query {
            text: Some("zeppelin".into()),
            ..Query::default()
        })
        .is_empty()
    );
}

#[test]
fn a_library_sound_shadows_a_built_in_and_approval_is_the_humans() {
    let dir = std::env::temp_dir().join(format!("sounder_parts_lib_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut doc = parts::find("low_thump").unwrap().doc;
    doc.seed = 7;
    let mut entry = parts::find("low_thump").unwrap().entry();
    entry.approved = false;
    entry.builtin = false;
    catalog::save(&dir, &doc, entry).unwrap();
    assert_eq!(catalog::load(&dir, "low_thump").unwrap().seed, 7);
    let all = catalog::list_with_builtin(&dir);
    assert_eq!(all.iter().filter(|e| e.name == "low_thump").count(), 1);
    assert!(!all.iter().find(|e| e.name == "low_thump").unwrap().approved);
    catalog::set_approved(&dir, "low_thump", true).unwrap();
    assert!(catalog::list(&dir)[0].approved);
    assert!(catalog::set_approved(&dir, "ghost", true).is_err());
    // A built-in loads without a library file.
    assert_eq!(
        catalog::load(&dir, "wood_knock").unwrap().name,
        "wood_knock"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
