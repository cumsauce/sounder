//! An explosion 150 m away: `explosion` size 8 with 60 pieces of debris from
//! a `burst` event stream (dense early), then the output stage. Writes
//! `explosion.wav` and `explosion.png` (the listen image) and prints the
//! descriptors and the default checks.
//!
//! cargo run --example explosion [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("explosion");
    let ops = vec![
        json!({"type": "burst", "name": "rocks", "count": 60, "spread": 2.0, "curve": "exp", "values": {"min": 0.2, "max": 1.0}, "seed": 2}),
        json!({"type": "explosion", "name": "boom", "size": 8, "distance": 150, "debris": "rocks", "seed": 1}),
        json!({"type": "output", "name": "out", "input": "boom", "lufs": -14}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("explosion.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "explosion".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("explosion.png");
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
