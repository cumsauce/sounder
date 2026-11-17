//! A wind loop: the `wind` node (five resonant bands following a 12 m/s speed, 0.6
//! gusts, an Aeolian whistle) in an 8 s loop document.
//! Writes `wind.wav` and `wind.png` (the listen image) and prints the
//! descriptors and the default checks for a loop.
//!
//! cargo run --example wind [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("wind");
    doc.kind = Kind::Loop;
    doc.duration = Some(8.0);
    let ops = vec![
        json!({"type": "wind", "name": "gale", "speed": 12, "gust": 0.6, "whistle": 0.3, "diameter": 0.004, "seed": 5}),
        json!({"type": "output", "name": "out", "input": "gale", "lufs": -26}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("wind.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "wind".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("wind.png");
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
