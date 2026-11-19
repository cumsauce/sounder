//! The plan's end-to-end acceptance, through the driver and only
//! `sound_ops` / `listen` / `verify` / `export`: build a UI click, a laser, a
//! wooden door creak, a gravel footstep (6 variations) and a rain loop, each
//! passing its `expect`, and export them; then match a reference WAV and
//! report the STFT distance at the start and after iterating.
//!
//! (The fourth acceptance item, a human listening in the window and an agent
//! resolving two comments, is manual.)

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
fn five_sounds_pass_their_expect_export_and_a_reference_is_matched() {
    let dir = temp_dir("acceptance");
    let out = dir.join("assets");
    let mut calls: Value = serde_json::from_slice(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/calls/acceptance.json"))
            .unwrap(),
    )
    .unwrap();
    for c in calls.as_array_mut().unwrap() {
        *c = serde_json::from_str(&c.to_string().replace("@OUT@", &out.display().to_string()))
            .unwrap();
    }
    let file = dir.join("calls.json");
    std::fs::write(&file, serde_json::to_vec(&calls).unwrap()).unwrap();
    let run = match Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("mcp_driver.py"))
        .arg("--calls")
        .arg(&file)
        .env("SOUNDER_BINARY", env!("CARGO_BIN_EXE_sounder"))
        .env("SOUNDER_HEARING", "off")
        .env("FRIENDSLOP_DATA_DIR", dir.join("data"))
        .env("SOUNDER_STDERR", dir.join("server.log"))
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("python3 not available ({e}): skipping");
            return;
        }
    };
    let lines: Vec<Value> = String::from_utf8_lossy(&run.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    assert_eq!(
        lines.len(),
        calls.as_array().unwrap().len() + 1,
        "{}",
        String::from_utf8_lossy(&run.stdout)
    );
    let r = &lines[1..];
    let ok = |i: usize| r[i]["ok"].as_bool().unwrap_or(false);
    let t = |i: usize| text_of(&r[i]);

    // Each of the five sounds passes its expect, then exports.
    for (i, name) in [
        (0, "ui_click"),
        (2, "laser"),
        (4, "door_creak"),
        (6, "gravel_step"),
        (8, "rain_loop"),
    ] {
        assert!(ok(i), "{name}: {}", t(i));
        assert!(
            t(i).contains("expect:") && !t(i).contains("FAIL"),
            "{name}: {}",
            t(i)
        );
        assert!(ok(i + 1), "{name} export: {}", t(i + 1));
    }
    for f in [
        "ui_click.ogg",
        "laser.wav",
        "door_creak.ogg",
        "rain_loop.ogg",
        "gravel_step_1.ogg",
        "gravel_step_6.ogg",
    ] {
        assert!(out.join(f).is_file(), "{f} was not exported");
    }
    assert!(out.join("gravel_step.sfx.json").is_file());
    let gravel: Value =
        serde_json::from_slice(&std::fs::read(out.join("gravel_step.sfx.json")).unwrap()).unwrap();
    assert_eq!(gravel["files"].as_array().map(Vec::len), Some(6));
    let rain: Value =
        serde_json::from_slice(&std::fs::read(out.join("rain_loop.sfx.json")).unwrap()).unwrap();
    assert!(
        rain["loop"]["end"].as_u64().unwrap() > 100_000,
        "a loop has loop points"
    );

    // Match a reference: the distance falls as the attempt is iterated toward it.
    let dist = |i: usize| {
        assert!(ok(i), "{}", t(i));
        r[i]["json"]["reference"]["distance_db"]
            .as_f64()
            .unwrap_or_else(|| panic!("no distance in {}", t(i)))
    };
    let (start, middle, last) = (dist(11), dist(13), dist(15));
    println!(
        "reference distance: start {start:.2} dB, after one iteration {middle:.2} dB, after two {last:.2} dB"
    );
    assert!(
        start > 2.0,
        "the first attempt should be clearly off: {start}"
    );
    assert!(middle < start, "iterating improved it: {start} -> {middle}");
    assert!(
        last < 0.3 && last < middle,
        "the final attempt matches the reference: {middle} -> {last}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
