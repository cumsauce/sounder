//! A brook: `water` with a slowly wandering flow and a few splashes, plus one
//! bigger `splash` at 1.5 s, at -22 LUFS. Writes `water.wav` and `water.png`
//! (the listen image), then a 4 s looping `water_loop.wav` that passes the
//! default checks including `loop_seam`, and prints descriptors and checks.
//!
//! cargo run --example water [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let opts = RenderOpts::default();

    let mut doc = SoundDoc::named("water");
    let ops = vec![
        json!({"type": "wander", "name": "level", "rate": 0.4, "range": [0.35, 0.7], "seed": 1}),
        json!({"type": "water", "name": "brook", "flow": "level", "depth": 0.3, "splash": 0.15, "seed": 2}),
        json!({"type": "splash", "name": "plop", "size": 0.12, "at": 1.5, "seed": 5}),
        json!({"type": "gain", "name": "plop_g", "input": "plop", "db": -6}),
        json!({"type": "mix", "name": "scene", "inputs": ["brook", "plop_g"]}),
        json!({"type": "output", "name": "out", "input": "scene", "lufs": -22}),
    ];
    let rep = doc.apply(
        ops,
        DocEdit {
            kind: Some(Kind::Oneshot),
            duration: Some(Some(6.0)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);
    let audio = &rep.rendered.audio;
    let wav_path = dir.join("water.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "water".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("water.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;
    println!("\n{}", describe(audio).table());
    let checks = run(
        &Expect::defaults_for(&doc),
        &CheckCx {
            doc: &doc,
            rendered: &rep.rendered,
            opts: &opts,
        },
    )?;
    print!("{}", report(&checks));
    println!("wrote {} and {}", wav_path.display(), png_path.display());
    let mut failed = checks.iter().any(|c| !c.pass);

    // A loop: constant flow, periodic by construction.
    let mut lp = SoundDoc::named("water_loop");
    let rep = lp.apply(
        vec![
            json!({"type": "water", "name": "w", "flow": 0.6, "depth": 0.4, "splash": 0.2, "seed": 4}),
            json!({"type": "output", "name": "out", "input": "w", "lufs": -22}),
        ],
        DocEdit {
            kind: Some(Kind::Loop),
            duration: Some(Some(4.0)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    let loop_wav = dir.join("water_loop.wav");
    wav::write(&loop_wav, &rep.rendered.audio, wav::Bits::Int16)?;
    let checks = run(
        &Expect::defaults_for(&lp),
        &CheckCx {
            doc: &lp,
            rendered: &rep.rendered,
            opts: &opts,
        },
    )?;
    println!("\nloop:");
    print!("{}", report(&checks));
    println!("wrote {}", loop_wav.display());
    failed |= checks.iter().any(|c| !c.pass);
    if failed {
        return Err("a check failed".into());
    }
    Ok(())
}
