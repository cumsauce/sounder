//! A piston engine revving up (`engine`) and an electric motor with a
//! gearbox spinning up (`motor`). Writes `engine.wav`, `engine.png`,
//! `motor.wav` and `motor.png` (the listen images) and prints the
//! descriptors and the default checks for each.
//!
//! cargo run --example engine [out_dir]

use std::path::Path;
use std::path::PathBuf;

use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn build(name: &str, secs: f32, ops: Vec<Value>, dir: &Path) -> Result<(), String> {
    let mut doc = SoundDoc::named(name);
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            duration: Some(Some(secs)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    let wav_path = dir.join(format!("{name}.wav"));
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: name.into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join(format!("{name}.png"));
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
    println!(
        "\nwrote {} and {}\n",
        wav_path.display(),
        png_path.display()
    );
    Ok(())
}

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    // A six-cylinder revving from 900 to 5200 rpm (pitch 90 -> 520 Hz at
    // 4 strokes) with the throttle opening, shaped by a low-pass.
    build(
        "engine",
        3.0,
        vec![
            json!({"type": "ramp", "name": "revs", "from": 900, "to": 5200, "time": 2.8, "curve": "exp"}),
            json!({"type": "ramp", "name": "throttle", "from": 0.15, "to": 0.9, "time": 2.8}),
            json!({"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "load": "throttle", "exhaust_length": 1.5, "seed": 1}),
            json!({"type": "filter", "name": "body", "input": "eng", "mode": "lp", "freq": 6000}),
            json!({"type": "output", "name": "out", "input": "body", "lufs": -18}),
        ],
        &dir,
    )?;

    // A motor with a 24-tooth gear, 800 -> 6000 rpm: the mesh tone climbs
    // from 320 Hz to 2.4 kHz.
    build(
        "motor",
        2.5,
        vec![
            json!({"type": "ramp", "name": "revs", "from": 800, "to": 6000, "time": 2.0, "curve": "exp"}),
            json!({"type": "motor", "name": "mot", "rpm": "revs", "poles": 4, "gear_teeth": 24, "load": 0.4, "whine": 0.4, "carrier": 6000, "seed": 1}),
            json!({"type": "output", "name": "out", "input": "mot", "lufs": -20}),
        ],
        &dir,
    )?;
    Ok(())
}
