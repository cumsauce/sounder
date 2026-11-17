//! A laser zap: a saw swept 2000 -> 200 Hz, an exponential decay, a
//! low-pass that follows the sweep, and the output stage. Writes
//! `laser.wav` and `laser.png` (the listen image) and prints the
//! descriptors and the default checks.
//!
//! cargo run --example laser [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("laser");
    let ops = vec![
        json!({"type": "ramp", "name": "sweep", "from": 2000, "to": 200, "time": 0.25, "curve": "exp"}),
        json!({"type": "osc", "name": "tone", "shape": "saw", "freq": "sweep"}),
        json!({"type": "decay", "name": "amp", "time": 0.3}),
        json!({"type": "math", "name": "zap", "op": "mul", "a": "tone", "b": "amp"}),
        json!({"type": "math", "name": "cutoff", "op": "mul", "a": "sweep", "b": 3}),
        json!({"type": "filter", "name": "lp", "input": "zap", "mode": "lp", "freq": "cutoff", "q": 1.5}),
        json!({"type": "output", "name": "out", "input": "lp", "lufs": -18}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("laser.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "laser".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("laser.png");
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
