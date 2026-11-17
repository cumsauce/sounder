//! A campfire loop: the `fire` node (roar, hiss, crackle pops) in an 8 s loop document, then a 30 Hz high-pass
//! and the output stage.
//! Writes `fire.wav` and `fire.png` (the listen image) and prints the
//! descriptors and the default checks for a loop.
//!
//! cargo run --example fire [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("fire");
    doc.kind = Kind::Loop;
    doc.duration = Some(8.0);
    let ops = vec![
        json!({"type": "fire", "name": "flames", "size": 0.7, "intensity": 0.6, "crackle": 0.6, "seed": 3}),
        json!({"type": "onepole", "name": "clean", "input": "flames", "mode": "hp", "freq": 30}),
        json!({"type": "output", "name": "out", "input": "clean", "lufs": -28}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("fire.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "fire".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("fire.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).with_loop(true).table());
    let checks = run(
        &Expect::defaults_for(&doc),
        &CheckCx {
            doc: &doc,
            rendered: &rep.rendered,
            opts: &opts,
        },
    )?;
    print!("{}", report(&checks));
    println!("\nwrote {} and {}", wav_path.display(), png_path.display());
    Ok(())
}
