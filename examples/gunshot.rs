//! A 7.62 mm rifle shot heard from 120 m in the open: the supersonic crack arrives about
//! 0.1 s before the muzzle blast. Writes `gunshot.wav` and `gunshot.png` (the
//! listen image) and prints the descriptors and the default checks. Add
//! `"suppressed": true` for the subsonic, suppressed version.
//!
//! cargo run --example gunshot [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("gunshot");
    let ops = vec![
        json!({"type": "gunshot", "name": "shot", "weapon": "rifle", "distance": 120, "space": "open", "seed": 1}),
        json!({"type": "output", "name": "out", "input": "shot", "lufs": -14}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("gunshot.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "gunshot".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("gunshot.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).table());
    let checks = run(
        &Expect::defaults(),
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
