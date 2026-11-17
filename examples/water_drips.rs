//! Water drips: about six drops a second, each a `bubble` (Minnaert resonance,
//! foley) whose radius is a seeded `$rand` draw (1-5 mm: 650 Hz to 3.3 kHz),
//! summed by `voices` over a `poisson` event stream, then 7 m of `air` (a
//! little treble absorption and the 1/distance drop). A one-shot with a
//! duration; writes `water_drips.wav` and `water_drips.png`.
//!
//! cargo run --example water_drips [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, Kind, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("water_drips");
    let ops = vec![
        json!({"type": "poisson", "name": "drops", "rate": 6, "seed": 3, "values": {"min": 0.5, "max": 1.0}}),
        json!({"type": "voices", "name": "drips", "events": "drops", "length": 0.2, "max": 60, "seed": 4,
               "map": {"r": "1 + $rand * 4"},
               "patch": [
                   {"type": "bubble", "name": "b", "radius_mm": "$r", "rise": 0.1},
                   {"type": "gain", "name": "level", "input": "b", "gain": "$value"}]}),
        json!({"type": "air", "name": "far", "input": "drips", "distance": 7, "humidity": 45}),
        json!({"type": "output", "name": "out", "input": "far", "lufs": -26}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            kind: Some(Kind::Oneshot),
            duration: Some(Some(5.0)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("water_drips.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "water drips".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("water_drips.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).table());
    let expect = Expect::defaults_for(&doc);
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
