//! A footstep on gravel: a low thump plus a burst of tiny stones (a `burst`
//! of events into `voices`, each a short band-passed click with its own
//! pitch). Six seeds give six different steps that sit within 1 LU of each
//! other (`variations`). Writes `gravel_step_1..6.wav` and a tiled listen
//! image `gravel_step.png`.
//!
//! cargo run --example gravel_step [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::checks::{Toggle, Variations};
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("gravel_step");
    let ops = vec![
        // The heel: a short low thump.
        json!({"type": "once", "name": "heel_at", "at": 0.0}),
        json!({"type": "noise", "name": "thump_n", "color": "brown", "seed": 5}),
        json!({"type": "decay", "name": "thump_e", "time": 0.09}),
        json!({"type": "math", "name": "thump", "op": "mul", "a": "thump_n", "b": "thump_e"}),
        json!({"type": "filter", "name": "heel", "input": "thump", "mode": "lp", "freq": 220}),
        // The stones: 70 events in 0.22 s, thinning out, each with a level.
        json!({"type": "burst", "name": "stones_at", "count": 70, "spread": 0.22, "curve": "exp", "at": 0.005,
               "values": {"min": 0.15, "max": 1.0, "curve": "exp"}}),
        json!({"type": "voices", "name": "stones", "events": "stones_at", "length": 0.04, "max": 100,
               "map": {"hz": "900 + $rand * 4500", "q": "2 + $rand * 5"},
               "patch": [
                   {"type": "noise", "name": "n", "color": "white"},
                   {"type": "filter", "name": "tick", "input": "n", "mode": "bp", "freq": "$hz", "q": "$q"},
                   {"type": "decay", "name": "shape", "time": 0.006},
                   {"type": "math", "name": "stone", "op": "mul", "a": "tick", "b": "shape"},
                   {"type": "gain", "name": "level", "input": "stone", "gain": "$value"}]}),
        json!({"type": "mix", "name": "mixed", "inputs": [{"input": "heel", "gain": 0.8}, {"input": "stones", "gain": 1.0}]}),
        json!({"type": "output", "name": "out", "input": "mixed", "lufs": -26}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(ops, DocEdit::default(), &opts)?;
    print!("{}", rep.text);

    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut renders = Vec::new();
    for seed in 1..=6u64 {
        let r = doc.render(&RenderOpts {
            seed: Some(seed),
            ..RenderOpts::default()
        })?;
        wav::write(
            &dir.join(format!("gravel_step_{seed}.wav")),
            &r.audio,
            wav::Bits::Int16,
        )?;
        renders.push((format!("seed {seed}"), r.audio));
    }
    let columns: Vec<Column> = renders
        .iter()
        .map(|(l, a)| Column {
            label: l.clone(),
            audio: a,
        })
        .collect();
    let png_path = dir.join("gravel_step.png");
    std::fs::write(&png_path, listen_png(&columns, View::default())?)
        .map_err(|e| format!("{}: {e}", png_path.display()))?;

    println!("\n{}", describe(&rep.rendered.audio).table());
    let expect = Expect {
        variations: Some(Toggle::With(Variations {
            seeds: Some(6),
            lufs_spread: Some(1.0),
            min_distance: Some(1.0),
        })),
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
    println!(
        "\nwrote {} and 6 WAVs in {}",
        png_path.display(),
        dir.display()
    );
    if checks.iter().any(|c| !c.pass) {
        return Err("a check failed".into());
    }
    Ok(())
}
