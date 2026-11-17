//! A glass pane breaking, from `shatter`: one crack at 0.02 s, then 90
//! fragments with power-law sizes crowded in the first 0.5 s, each a small
//! glass `modal` plate whose size comes from the fragment's `$value` (big
//! piece, low ring). Writes `shatter.wav` and `shatter.png` and prints the
//! descriptors and the default checks.
//!
//! cargo run --example shatter [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("shatter");
    let glass =
        json!({"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.006, "thickness": 0.003});
    let ops = vec![
        json!({"type": "shatter", "name": "pieces", "pieces": 90, "spread": 0.5, "at": 0.02,
               "exponent": 2.0, "seed": 4}),
        // A fragment of size v rings as a plate 5 cm .. 40 cm across.
        json!({"type": "voices", "name": "shards", "events": "pieces", "length": 0.4, "max": 100,
               "seed": 3, "map": {"size": "0.05 + $value * 0.35", "where": "0.15 + $rand * 0.6"},
               "patch": [
                   {"type": "impulse", "name": "tap", "at": [0.0]},
                   {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
                   {"type": "modal", "name": "piece", "excite": "strike", "material": glass,
                    "size": "$size", "strike_position": "$where", "modes": 16}]}),
        json!({"type": "onepole", "name": "no_dc", "input": "shards", "mode": "hp", "freq": 40}),
        json!({"type": "output", "name": "out", "input": "no_dc", "lufs": -22}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            duration: Some(Some(1.2)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("shatter.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "shatter".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("shatter.png");
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
