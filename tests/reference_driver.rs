//! `listen reference:` / `diff` and the `reference` check through the driver
//! script (`mcp_driver.py --calls tests/calls/reference.json` against
//! `sounder --headless`): a sound is built and exported, then compared with
//! its own WAV (distance ~0), darkened and compared again.

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
            eprintln!("python3 not available ({e}): skipping the driver test");
            return None;
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
fn listen_reference_diff_and_the_reference_check_through_the_driver() {
    let dir = temp_dir("reference");
    let out_dir = dir.join("exported");
    let mut calls: Value = serde_json::from_slice(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/calls/reference.json"))
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
    assert!(ok(1), "{}", t(1));
    assert!(ok(2) && out_dir.join("struck.wav").is_file(), "{}", t(2));
    // Against its own export: about zero, an image, and the descriptor table.
    assert!(ok(3), "{}", t(3));
    assert_eq!(r[3]["n_images"], 1);
    assert!(
        t(3).contains("reference distance 0.") && t(3).contains("bands dB"),
        "{}",
        t(3)
    );
    let near = r[3]["json"]["reference"]["distance_db"].as_f64().unwrap();
    assert!(near < 0.3, "distance to its own export {near}");
    // Darkened: far, and the words name the quieter high bands.
    assert!(ok(5), "{}", t(5));
    let far = r[5]["json"]["reference"]["distance_db"].as_f64().unwrap();
    assert!(far > 3.0, "darkened distance {far}");
    assert!(
        t(5).contains("kHz is") && t(5).contains("quieter than the reference"),
        "{}",
        t(5)
    );
    // The check fails with the same words; a missing file is an error.
    assert!(!ok(6), "{}", t(6));
    assert!(
        t(6).contains("FAIL reference") && t(6).contains("max 1"),
        "{}",
        t(6)
    );
    assert!(!ok(7) && t(7).contains("missing.wav"), "{}", t(7));
    // diff compares with the last listen of the same request (call 5's
    // render, unchanged: before / now / difference), then after an edit.
    assert!(ok(8) && r[8]["n_images"] == 1, "{}", t(8));
    assert!(!t(8).contains("no earlier render"), "{}", t(8));
    assert!(ok(10) && r[10]["n_images"] == 1, "{}", t(10));
    let _ = std::fs::remove_dir_all(&dir);
}
