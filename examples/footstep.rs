//! One footstep per surface from the `footstep` node alone: concrete, gravel,
//! grass and wood, then six seeds of the gravel step. Writes
//! `footstep_<surface>.wav`, a tiled `footstep.png` and prints descriptors.
//!
//! cargo run --example footstep [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{Column, View, describe, listen_png};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut renders = Vec::new();
    for surface in ["concrete", "gravel", "grass", "wood"] {
        let mut doc = SoundDoc::named(&format!("footstep_{surface}"));
        doc.duration = Some(0.8);
        let ops = vec![
            json!({"type": "footstep", "name": "step", "surface": surface, "weight": 0.5, "pace": "walk"}),
            json!({"type": "output", "name": "out", "input": "step", "lufs": -26}),
        ];
        let rep = doc.apply(ops, DocEdit::default(), &RenderOpts::default())?;
        let audio = rep.rendered.audio.clone();
        wav::write(
            &dir.join(format!("footstep_{surface}.wav")),
            &audio,
            wav::Bits::Int16,
        )?;
        println!("== {surface}\n{}", describe(&audio).table());
        renders.push((surface.to_string(), audio));
    }
    // Six seeds of one surface: the variations differ in timing and level.
    let mut doc = SoundDoc::named("footstep_gravel");
    doc.duration = Some(0.8);
    doc.apply(
        vec![
            json!({"type": "footstep", "name": "step", "surface": "gravel"}),
            json!({"type": "output", "name": "out", "input": "step", "lufs": -26}),
        ],
        DocEdit::default(),
        &RenderOpts::default(),
    )?;
    for seed in 1..=6u64 {
        let r = doc.render(&RenderOpts {
            seed: Some(seed),
            ..RenderOpts::default()
        })?;
        renders.push((format!("gravel seed {seed}"), r.audio));
    }
    let columns: Vec<Column> = renders
        .iter()
        .take(8)
        .map(|(l, a)| Column {
            label: l.clone(),
            audio: a,
        })
        .collect();
    let png = dir.join("footstep.png");
    std::fs::write(&png, listen_png(&columns, View::default())?)
        .map_err(|e| format!("{}: {e}", png.display()))?;
    println!("wrote wavs and {}", png.display());
    Ok(())
}
