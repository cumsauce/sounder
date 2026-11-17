//! Glass breaking: an impact (`contact` -> `modal` glass, cut by a fast
//! decay) then a scatter of shards: a `poisson` stream whose rate thins out
//! drives `voices`, each voice a small glass `modal` whose size comes from
//! `$rand`. Writes `glass_break.wav` and `glass_break.png`.
//!
//! cargo run --example glass_break [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::checks::{BandCheck, Range};
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("glass_break");
    let shard =
        json!({"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.006, "thickness": 0.003});
    let ops = vec![
        // The blow: a hard 0.3 kg striker at 4 m/s on a 0.5 m pane. The pane
        // does not keep ringing: it breaks, so the ring is cut by a fast decay.
        json!({"type": "contact", "name": "blow", "speed": 4, "mass": 0.3, "material": "glass",
               "hardness": 0.9, "at": 0.004}),
        json!({"type": "modal", "name": "pane", "excite": "blow", "material": "glass", "size": 0.5,
               "strike_position": 0.27, "seed": 1}),
        json!({"type": "decay", "name": "pane_env", "time": 0.18}),
        json!({"type": "math", "name": "crack", "op": "mul", "a": "pane", "b": "pane_env"}),
        // The shards: ~100 events over 1.6 s, thinning out (rate 220 -> 6 a
        // second), each a small glass plate 4..10 cm across that rings for
        // about a tenth of a second.
        json!({"type": "ramp", "name": "thinning", "from": 220, "to": 6, "time": 1.6, "curve": "exp"}),
        json!({"type": "poisson", "name": "falling", "rate": "thinning", "seed": 7,
               "values": {"min": 0.1, "max": 0.8, "curve": "exp"}}),
        json!({"type": "voices", "name": "shards", "events": "falling", "length": 0.25, "max": 400, "seed": 3,
               "map": {"size": "0.04 + $rand * 0.06", "where": "0.15 + $rand * 0.6"},
               "patch": [
                   {"type": "impulse", "name": "tap", "at": [0.0]},
                   {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
                   {"type": "modal", "name": "piece", "excite": "strike", "material": shard,
                    "size": "$size", "strike_position": "$where", "modes": 12}]}),
        json!({"type": "mix", "name": "all", "inputs": [
            {"input": "crack", "gain": 0.12},
            {"input": "shards", "gain": 1.0}]}),
        json!({"type": "onepole", "name": "no_dc", "input": "all", "mode": "hp", "freq": 40}),
        json!({"type": "output", "name": "out", "input": "no_dc", "lufs": -22}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            duration: Some(Some(1.8)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("glass_break.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "glass break".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("glass_break.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(audio).table());
    // 1.8 s at the loudness asked for; loud first, thinning out: the first
    // fifth holds the most energy in the 2k band and the last fifth far less.
    let expect = Expect {
        duration: Some(Range {
            min: Some(1.7),
            max: Some(1.9),
        }),
        lufs: Some(Range {
            min: Some(-23.0),
            max: Some(-21.0),
        }),
        bands: Some(vec![
            BandCheck {
                band: json!("2k"),
                segment: Some(0),
                min_db: Some(-30.0),
                max_db: None,
            },
            BandCheck {
                band: json!("2k"),
                segment: Some(4),
                min_db: None,
                max_db: Some(-35.0),
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
