//! A clarinet-like tube (reed, closed at one end) breathed in by an envelope,
//! and a bowed string beside it, one after the other. Writes `waveguide.wav`
//! and `waveguide.png` (the listen image) and prints the descriptors and the
//! default checks.
//!
//! cargo run --example waveguide [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("waveguide");
    doc.duration = Some(3.0);
    let ops = vec![
        // The breath: 0 -> 0.8 over 80 ms, held, then let go at 1.2 s.
        json!({"type": "ramp", "name": "breath_up", "from": 0.0, "to": 0.8, "time": 0.08, "curve": "linear"}),
        json!({"type": "ramp", "name": "breath_down", "from": 1.0, "to": 0.0, "time": 0.1, "at": 1.2, "curve": "linear"}),
        json!({"type": "math", "name": "breath", "op": "mul", "a": "breath_up", "b": "breath_down"}),
        // 0.6 m closed at one end: c / (4 L) = 142.9 Hz, odd harmonics.
        json!({"type": "tube", "name": "pipe", "length": 0.6, "excite": "reed", "pressure": "breath", "open_end": false, "seed": 1}),
        // A bowed string at 220 Hz, a quarter of a second behind: bow
        // pressure lifted at 2.2 s so it rings down.
        json!({"type": "ramp", "name": "press", "from": 0.6, "to": 0.0, "time": 0.05, "at": 2.2, "curve": "linear"}),
        json!({"type": "string", "name": "fiddle", "freq": 220, "excite": "bow", "bow_pressure": "press", "damping": 0.5, "stiffness": 0.2, "seed": 1}),
        json!({"type": "fade", "name": "fiddle_in", "input": "fiddle", "in": 0.3}),
        json!({"type": "mix", "name": "both", "inputs": ["pipe", "fiddle_in"]}),
        json!({"type": "output", "name": "out", "input": "both", "lufs": -20}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("waveguide.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "waveguide".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("waveguide.png");
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
