//! A rain loop: a filtered noise bed plus a few hundred drops, each a short
//! band-passed noise tick whose pitch and level come from the event stream
//! (`poisson` -> `voices`). A loop: it has a duration, no edge fades, and
//! passes the `loop_seam` check. Writes `rain_loop.wav` and `rain_loop.png`.
//!
//! cargo run --example rain_loop [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("rain_loop");
    let ops = vec![
        // The bed: pink noise, band-limited, wandering a little in level.
        json!({"type": "noise", "name": "hiss", "color": "pink", "seed": 1}),
        json!({"type": "filter", "name": "bed", "input": "hiss", "mode": "bp", "freq": 3500, "q": 0.4}),
        json!({"type": "wander", "name": "swell", "rate": 0.7, "range": [0.7, 1.0], "seed": 2}),
        json!({"type": "math", "name": "bed_level", "op": "mul", "a": "bed", "b": "swell"}),
        // The drops: ~120 a second, each with a seeded level.
        json!({"type": "poisson", "name": "drops", "rate": 120, "seed": 3, "values": {"min": 0.25, "max": 1.0}}),
        json!({"type": "voices", "name": "rain", "events": "drops", "length": 0.05, "max": 700, "seed": 4,
               "map": {"hz": "1500 + $rand * 5500"},
               "patch": [
                   {"type": "noise", "name": "n", "color": "white"},
                   {"type": "filter", "name": "tick", "input": "n", "mode": "bp", "freq": "$hz", "q": 4},
                   {"type": "decay", "name": "shape", "time": 0.012},
                   {"type": "math", "name": "drop", "op": "mul", "a": "tick", "b": "shape"},
                   {"type": "gain", "name": "level", "input": "drop", "gain": "$value"}]}),
        json!({"type": "mix", "name": "mixed", "inputs": [{"input": "bed_level", "gain": 0.35}, {"input": "rain", "gain": 1.0}]}),
        json!({"type": "onepole", "name": "no_dc", "input": "mixed", "mode": "hp", "freq": 30}),
        json!({"type": "output", "name": "out", "input": "no_dc", "lufs": -24}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            kind: Some(Kind::Loop),
            duration: Some(Some(4.0)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("rain_loop.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "rain loop".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("rain_loop.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).table());
    let expect = Expect {
        loop_seam: Some(true),
        ..Expect::defaults_for(&doc)
    };
    let checks = run(
        &expect,
        &CheckCx {
            doc: &doc,
            rendered: &rep.rendered,
            opts: &opts,
        },
    )?;
    print!("{}", report(&checks));
    println!("\nwrote {} and {}", wav_path.display(), png_path.display());
    if checks.iter().any(|c| !c.pass) {
        return Err("a check failed".into());
    }
    Ok(())
}
