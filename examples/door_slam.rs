//! A heavy wooden door slam built in layers on a dry bus (AGENTS.md
//! "Layering a designed sound"): a friction brush as the door sticks on its
//! frame, a noise crack, a wood `modal` struck by a noise burst, band-passed
//! noise for the body, low-passed brown noise for the weight, and a small
//! wood knock after. The settings are `slam_b2` from the Phase 0 test
//! (`docs/plans/phase0-results.md`). Writes `door_slam.wav` and
//! `door_slam.png`, and checks that the result is dry, mono and atonal.
//!
//! cargo run --example door_slam [out_dir]

use std::path::PathBuf;

use serde_json::json;
use sfx_analysis::checks::{BandCheck, Range};
use sfx_analysis::{CheckCx, Column, Expect, View, describe, listen_png, report, run};
use sfx_graph::{DocEdit, RenderOpts, SoundDoc, wav};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let mut doc = SoundDoc::named("door_slam");
    let ops = vec![
        // pre: the door brushes the frame for ~40 ms before it hits the jamb.
        json!({"type": "env", "name": "scr_env", "curve": "smooth",
               "points": [[0, 0], [0.008, 0], [0.02, 1], [0.042, 0.7], [0.05, 0]]}),
        json!({"type": "math", "name": "scr_speed", "op": "mul", "a": "scr_env", "b": 1.4}),
        json!({"type": "friction", "name": "scr_f", "speed": "scr_speed", "pressure": "scr_env",
               "roughness": 0.85, "grain": 800}),
        json!({"type": "noise", "name": "scr_n", "color": "pink"}),
        json!({"type": "math", "name": "scr_nb", "op": "mul", "a": "scr_n", "b": "scr_env"}),
        json!({"type": "mix", "name": "scr_m", "inputs": [
            {"input": "scr_f", "gain": 0.6}, {"input": "scr_nb", "gain": 0.5}]}),
        json!({"type": "filter", "name": "scr", "input": "scr_m", "mode": "bp", "freq": 1956, "q": 0.6}),
        json!({"type": "gain", "name": "scr_g", "input": "scr", "db": -10}),
        // crack: 8 ms of band-passed noise at the impact (50 ms in).
        json!({"type": "noise", "name": "crk_n", "color": "white"}),
        json!({"type": "decay", "name": "crk_e", "time": 0.008, "at": 0.05}),
        json!({"type": "math", "name": "crk_raw", "op": "mul", "a": "crk_n", "b": "crk_e"}),
        json!({"type": "filter", "name": "crk", "input": "crk_raw", "mode": "bp", "freq": 1928, "q": 0.5}),
        json!({"type": "gain", "name": "crk_g", "input": "crk", "db": -3}),
        // body: a wood panel struck by a noise burst (no clean pitch), plus
        // band-passed noise that decays with it.
        json!({"type": "noise", "name": "hit_n", "color": "pink"}),
        json!({"type": "decay", "name": "hit_e", "time": 0.012, "at": 0.05}),
        json!({"type": "math", "name": "hit_burst", "op": "mul", "a": "hit_n", "b": "hit_e"}),
        json!({"type": "modal", "name": "panel", "excite": "hit_burst", "material": "wood",
               "size": 0.5, "loss": 1.69, "modes": 96, "strike_position": 0.31, "gain_db": -10.7}),
        json!({"type": "decay", "name": "body_e", "time": 0.59, "at": 0.05}),
        json!({"type": "math", "name": "body_raw", "op": "mul", "a": "hit_n", "b": "body_e"}),
        json!({"type": "filter", "name": "body_bp", "input": "body_raw", "mode": "bp", "freq": 344, "q": 0.5}),
        json!({"type": "gain", "name": "body_g", "input": "body_bp", "db": 6.35}),
        // weight: low-passed brown noise, not a sine (a sine reads as a drum).
        json!({"type": "noise", "name": "thd_n", "color": "brown"}),
        json!({"type": "decay", "name": "thd_e", "time": 0.44, "at": 0.05}),
        json!({"type": "math", "name": "thd_raw", "op": "mul", "a": "thd_n", "b": "thd_e"}),
        json!({"type": "filter", "name": "thd", "input": "thd_raw", "mode": "lp", "freq": 192, "slope": 24}),
        json!({"type": "gain", "name": "thd_g", "input": "thd", "db": -6.6}),
        // after: a small wood knock 28 ms after the hit (wood on wood, not a
        // metal latch).
        json!({"type": "contact", "name": "knock_c", "speed": 1, "mass": 0.3, "material": "wood",
               "hardness": 0.6, "at": 0.078}),
        json!({"type": "modal", "name": "knock_m", "excite": "knock_c", "material": "wood",
               "size": 0.54, "loss": 3}),
        json!({"type": "filter", "name": "knock", "input": "knock_m", "mode": "lp", "freq": 3500}),
        json!({"type": "gain", "name": "knock_g", "input": "knock", "db": -7.4}),
        // The dry mono bus.
        json!({"type": "mix", "name": "dry", "to": "mono",
               "inputs": ["scr_g", "crk_g", "panel", "body_g", "thd_g", "knock_g"]}),
        json!({"type": "transient", "name": "punch", "input": "dry", "attack_db": 3.1, "sustain_db": -6}),
        json!({"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5,
               "oversample": 4, "mix": 0.25}),
        json!({"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 30}),
        json!({"type": "eq", "name": "tone", "input": "dc", "bands": [
            {"mode": "peak", "freq": 200, "gain_db": 5.6, "q": 0.8}, {"mode": "lp", "freq": 10000}]}),
        json!({"type": "output", "name": "out", "input": "tone", "lufs": -28}),
    ];
    let opts = RenderOpts::default();
    let rep = doc.apply(
        ops,
        DocEdit {
            duration: Some(Some(0.8)),
            ..DocEdit::default()
        },
        &opts,
    )?;
    print!("{}", rep.text);

    let audio = &rep.rendered.audio;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let wav_path = dir.join("door_slam.wav");
    wav::write(&wav_path, audio, wav::Bits::Int16)?;
    let png = listen_png(
        &[Column {
            label: "door slam".into(),
            audio,
        }],
        View::default(),
    )?;
    let png_path = dir.join("door_slam.png");
    std::fs::write(&png_path, png).map_err(|e| format!("{}: {e}", png_path.display()))?;

    let d = describe(audio);
    println!("\n{}", d.table());
    // At the loudness asked for, with weight in the 125 Hz band at the hit
    // and almost nothing left in it by the last fifth (no reverb tail).
    let expect = Expect {
        lufs: Some(Range {
            min: Some(-29.0),
            max: Some(-27.0),
        }),
        bands: Some(vec![
            BandCheck {
                band: json!("125"),
                segment: Some(0),
                min_db: Some(-35.0),
                max_db: None,
            },
            BandCheck {
                band: json!("125"),
                segment: Some(4),
                min_db: None,
                max_db: Some(-60.0),
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
    // The listener's rules: mono (dry, no reverb), atonal (the pitch tracker
    // finds almost no voiced frames; the tuned-drum version read 0.9), and
    // weight in the low-mids (at least 20 % of the energy under 200 Hz).
    let rules = [
        (
            audio.channels.len() == 1,
            format!("mono: {} channel(s)", audio.channels.len()),
        ),
        (
            d.voiced <= 0.2,
            format!("atonal: voiced {:.2} (max 0.20)", d.voiced),
        ),
        (
            d.band_shares[0] >= 20.0,
            format!(
                "weight: {:.1} % of the energy under 200 Hz (min 20)",
                d.band_shares[0]
            ),
        ),
    ];
    for (pass, text) in &rules {
        println!("{} {text}", if *pass { "PASS" } else { "FAIL" });
    }
    println!("\nwrote {} and {}", wav_path.display(), png_path.display());
    if checks.iter().any(|c| !c.pass) || rules.iter().any(|(p, _)| !p) {
        return Err("a check failed".into());
    }
    Ok(())
}
