//! A wooden door's hinge creaking open: a `ramp` of sliding speed drives
//! `friction` (stick-slip, seeded roughness, a slow `wander` on the pressure),
//! and the slips excite a `modal` wooden panel. Writes
//! `door_creak.wav` and `door_creak.png`.
//!
//! cargo run --example door_creak [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::checks::{BandCheck, Range};
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("door_creak");
    let ops = vec![
        // The hinge turns faster as the door swings: slip rate 70 -> 270 Hz.
        json!({"type": "ramp", "name": "swing", "from": 0.25, "to": 0.95, "time": 1.4, "curve": "smooth"}),
        json!({"type": "wander", "name": "weight", "rate": 1.5, "range": [0.45, 1.0], "seed": 5}),
        json!({"type": "friction", "name": "hinge", "speed": "swing", "pressure": "weight", "grain": 280,
               "roughness": 0.35, "seed": 2}),
        // The door: wood, a panel 0.45 m across (lowest mode ~420 Hz).
        json!({"type": "modal", "name": "door", "excite": "hinge", "material": "wood",
               "size": 0.45, "strike_position": 0.2, "seed": 1}),
        json!({"type": "onepole", "name": "no_dc", "input": "door", "mode": "hp", "freq": 25}),
        json!({"type": "output", "name": "out", "input": "no_dc", "lufs": -22}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            duration: Some(Some(1.5)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("door_creak.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "door creak".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("door_creak.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).table());
    // A door swinging for 1.5 s, at the loudness asked for.
    let expect = Expect {
        duration: Some(Range {
            min: Some(1.4),
            max: Some(1.6),
        }),
        lufs: Some(Range {
            min: Some(-23.0),
            max: Some(-21.0),
        }),
        // The creak rises: by the last fifth the energy has left the 63 Hz
        // band and sits in the panel's 500 Hz one.
        bands: Some(vec![
            BandCheck {
                band: json!("63"),
                segment: Some(4),
                min_db: None,
                max_db: Some(-45.0),
            },
            BandCheck {
                band: json!("500"),
                segment: Some(4),
                min_db: Some(-26.0),
                max_db: None,
            },
        ]),
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
