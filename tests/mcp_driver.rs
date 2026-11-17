//! The phase 2 check: a driver script (`mcp_driver.py --calls`, throwaway
//! mode, against `sounder --headless`) runs new_sound -> sound_ops -> listen
//! -> verify -> undo -> redo -> catalog save -> export, and every line is
//! `ok` where it should be; the exported WAV exists and its manifest LUFS
//! matches `listen`. A failing node in a batch leaves the doc unchanged.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs the driver on `calls` and returns each call's result line.
fn drive(calls: &Value, data: &Path) -> Option<Vec<Value>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("mcp_driver.py");
    let file = data.join("calls.json");
    std::fs::write(&file, serde_json::to_vec(calls).unwrap()).unwrap();
    let out = match Command::new("python3")
        .arg(&script)
        .arg("--calls")
        .arg(&file)
        .env("SOUNDER_BINARY", env!("CARGO_BIN_EXE_sounder"))
        .env("FRIENDSLOP_DATA_DIR", data.join("data"))
        .env("SOUNDER_STDERR", data.join("server.log"))
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            // Missing python3 is a failure, not a pass: the driver test is
            // the only end-to-end check of the stdio server.
            panic!("python3 is required for the driver tests: {e}");
        }
    };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert!(
        lines.len() == calls.as_array().unwrap().len() + 1,
        "driver printed {} lines for {} calls:\n{text}\nstderr: {}",
        lines.len(),
        calls.as_array().unwrap().len(),
        String::from_utf8_lossy(&out.stderr)
    );
    Some(lines[1..].to_vec())
}

fn text_of(v: &Value) -> String {
    v["text"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_driver_script_builds_checks_undoes_saves_and_exports() {
    let dir = temp_dir("driver");
    let out_dir = dir.join("exported");
    let mut calls: Value = serde_json::from_slice(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/calls/phase2.json"))
            .unwrap(),
    )
    .unwrap();
    for c in calls.as_array_mut().unwrap() {
        let s = c
            .to_string()
            .replace("@OUT@", &out_dir.display().to_string());
        *c = serde_json::from_str(&s).unwrap();
    }
    let Some(r) = drive(&calls, &dir) else {
        return;
    };
    let ok = |i: usize| r[i]["ok"].as_bool().unwrap_or(false);
    let t = |i: usize| text_of(&r[i]);

    assert!(ok(0), "{}", t(0));
    assert!(ok(1), "build + expect: {}", t(1));
    assert!(
        t(1).contains("PASS peak_db") && t(1).contains("PASS silent_edges"),
        "{}",
        t(1)
    );
    // The failing batch is refused, names node and field, and changes nothing.
    assert!(!ok(2), "{}", t(2));
    assert!(
        t(2).contains("node #5 (filter \"lp\")") && t(2).contains("frq"),
        "{}",
        t(2)
    );
    assert!(ok(3));
    assert!(
        t(3).contains("v1") && t(3).contains("nodes (7)") && !t(3).contains("extra"),
        "{}",
        t(3)
    );
    // Macros: stored, and verify confirms both claims.
    assert!(ok(4), "{}", t(4));
    assert!(ok(5), "verify: {}", t(5));
    assert!(
        t(5).contains("PASS macro brightness") && t(5).contains("PASS macro weight"),
        "{}",
        t(5)
    );
    assert!(r[5]["json"]["ok"].as_bool().unwrap_or(false));
    // listen: an image and descriptors; a macro sweep tiles three columns.
    assert!(ok(6) && r[6]["n_images"] == 1, "{}", t(6));
    assert!(
        t(6).contains("LUFS") && t(6).contains("bands dB"),
        "{}",
        t(6)
    );
    assert!(ok(7) && r[7]["n_images"] == 1);
    assert_eq!(
        r[7]["json"]["columns"].as_array().map(Vec::len),
        Some(0),
        "descriptors: false leaves them out"
    );
    // A dishonest knob fails verify and says what to do; deleting it clears it.
    assert!(ok(8), "{}", t(8));
    assert!(!ok(9), "{}", t(9));
    assert!(
        t(9).contains("FAIL macro snap") && t(9).contains("rename the macro"),
        "{}",
        t(9)
    );
    assert!(ok(10));
    // Comments: pinned, listed in summary as a request, resolved with a reply.
    assert!(ok(11) && ok(12));
    assert!(
        t(12).contains("open comments (1)") && t(12).contains("too resonant"),
        "{}",
        t(12)
    );
    assert!(ok(13), "{}", t(13));
    // History: one step per sound_ops; undo / redo / jump.
    assert!(ok(14) && ok(15) && ok(16), "{} {} {}", t(14), t(15), t(16));
    assert!(t(15).contains("now at history step"), "{}", t(15));
    assert!(ok(17), "{}", t(17));
    assert!(
        t(17).contains("`+sweep +tone") && t(17).contains("<-"),
        "{}",
        t(17)
    );
    assert!(ok(18), "{}", t(18));
    assert!(
        !ok(19) && t(19).contains("no history node #99"),
        "{}",
        t(19)
    );
    // Save and export.
    assert!(ok(20), "{}", t(20));
    assert!(
        ok(21) && t(21).contains("a laser zap") && t(21).contains("knobs: brightness, weight"),
        "{}",
        t(21)
    );
    assert!(ok(22), "{}", t(22));
    for i in 1..=3 {
        assert!(
            out_dir.join(format!("laser_{i}.wav")).is_file(),
            "laser_{i}.wav"
        );
    }
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(out_dir.join("laser.sfx.json")).unwrap()).unwrap();
    assert_eq!(manifest["files"].as_array().map(Vec::len), Some(3));
    assert_eq!(manifest["seeds"], serde_json::json!([1, 2, 3]));
    // The manifest's loudness for seed 1 matches listen's for the same render.
    let lufs_listen = r[6]["json"]["columns"][0]["descriptors"]["lufs"]
        .as_f64()
        .unwrap();
    let lufs_manifest = manifest["files"][0]["lufs"].as_f64().unwrap();
    assert!(
        (lufs_listen - lufs_manifest).abs() < 0.2,
        "listen {lufs_listen} vs manifest {lufs_manifest}"
    );
    // The WAV on disk reads back at about that loudness too.
    let wav = sfx_graph::wav::read(&out_dir.join("laser_1.wav")).unwrap();
    let lufs_file = sfx_graph::loudness::integrated(&wav.channels, wav.sample_rate);
    assert!(
        (lufs_file - lufs_manifest as f32).abs() < 0.2,
        "file {lufs_file} vs manifest {lufs_manifest}"
    );
    // play is the headless error; extract_modes returns a resonators node.
    assert!(!ok(23) && t(23).contains("--headless"), "{}", t(23));
    assert!(ok(24), "{}", t(24));
    assert_eq!(r[24]["json"]["type"], "resonators");
    assert!(ok(25));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_phase_3_nodes_work_through_the_driver() {
    let dir = temp_dir("phase3");
    let calls: Value = serde_json::from_slice(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/calls/phase3.json"))
            .unwrap(),
    )
    .unwrap();
    let Some(r) = drive(&calls, &dir) else {
        return;
    };
    let ok = |i: usize| r[i]["ok"].as_bool().unwrap_or(false);
    let t = |i: usize| text_of(&r[i]);
    assert!(ok(0) && ok(1), "{} {}", t(0), t(1));
    // A loop of voices (poisson -> voices) passes its seam check, and a loop's
    // defaults are loop_seam rather than silent_edges.
    assert!(ok(2), "{}", t(2));
    assert!(ok(3), "{}", t(3));
    assert!(
        t(3).contains("PASS loop_seam") && !t(3).contains("silent_edges"),
        "{}",
        t(3)
    );
    // A Karplus-Strong string (a feedback loop through a delay) reads its pitch.
    assert!(ok(4), "{}", t(4));
    assert!(ok(5), "{}", t(5));
    assert!(t(5).contains("PASS pitch"), "{}", t(5));
    // `sound` instances another open sound; the name is not a node reference.
    assert!(ok(6), "{}", t(6));
    assert!(
        ok(7) && t(7).contains("a (sound)") && !t(7).contains("<- thud"),
        "{}",
        t(7)
    );
    // expr, grains and spectral nodes render.
    assert!(ok(8), "{}", t(8));
    assert!(ok(9), "{}", t(9));
    assert!(ok(10), "{}", t(10));
    assert!(ok(11), "pitch_shift +12 of 300 Hz: {}", t(11));
    // Errors say what to do.
    assert!(
        !ok(12) && t(12).contains("cycle without a delay") && t(12).contains("delay"),
        "{}",
        t(12)
    );
    assert!(
        !ok(13) && t(13).contains("10 events but max is 4") && t(13).contains("raise max"),
        "{}",
        t(13)
    );
    assert!(
        !ok(14) && t(14).contains("would instance itself"),
        "{}",
        t(14)
    );
    assert!(ok(15) && r[15]["n_images"] == 1, "{}", t(15));
    let _ = std::fs::remove_dir_all(&dir);
}
