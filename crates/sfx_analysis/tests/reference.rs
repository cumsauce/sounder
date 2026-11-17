//! Reference compare: alignment, loudness matching, the distance, the
//! words, the difference image and the `reference` check.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::image::MAX_PIXELS;
use sfx_analysis::{
    CheckCx, Column, DiffColumn, Expect, View, compare, listen_png, listen_png_diff, run,
};
use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

/// A struck object: five modes plus a noise tick.
fn struck(modes: Value, lp: Option<f64>, duration: f32) -> Audio {
    let mut d = SoundDoc::named("t");
    let src = if lp.is_some() { "lp" } else { "mixed" };
    d.nodes = vec![
        json!({"type": "impulse", "name": "hit"}),
        json!({"type": "resonators", "name": "body", "input": "hit", "modes": modes}),
        json!({"type": "noise", "name": "n", "seed": 3}),
        json!({"type": "decay", "name": "e", "time": 0.004}),
        json!({"type": "math", "name": "tick", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "gain", "name": "tickg", "input": "tick", "gain": 0.05}),
        json!({"type": "mix", "name": "mixed", "inputs": ["body", "tickg"]}),
    ];
    if let Some(f) = lp {
        d.nodes.push(json!({"type": "filter", "name": "lp", "input": "mixed", "mode": "lp", "freq": f, "q": 0.7}));
    }
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": src, "lufs": -20}));
    d.duration = Some(duration);
    d.render(&RenderOpts::default()).unwrap().audio
}

fn modes() -> Value {
    json!([[300, 0.25, 1.0], [870, 0.2, 0.6], [1700, 0.15, 0.5], [3200, 0.1, 0.4], [5200, 0.08, 0.3]])
}

fn base() -> Audio {
    struck(modes(), None, 0.8)
}

fn scaled(a: &Audio, db: f32) -> Audio {
    let g = 10f32.powf(db / 20.0);
    Audio {
        sample_rate: a.sample_rate,
        channels: a
            .channels
            .iter()
            .map(|c| c.iter().map(|v| v * g).collect())
            .collect(),
    }
}

fn delayed(a: &Audio, secs: f32) -> Audio {
    let n = (secs * a.sample_rate as f32) as usize;
    Audio {
        sample_rate: a.sample_rate,
        channels: a
            .channels
            .iter()
            .map(|c| {
                let mut v = vec![0.0; n];
                v.extend_from_slice(c);
                v
            })
            .collect(),
    }
}

#[test]
fn identical_sounds_are_at_zero_distance_with_no_deviations() {
    let a = base();
    let c = compare(&a, &a);
    assert!(c.distance_db < 0.01, "distance {}", c.distance_db);
    assert!(c.deviations.is_empty(), "{:?}", c.deviations);
    assert!(c.shift_s.abs() < 1e-4 && c.gain_db.abs() < 0.05);
    assert!(c.notes.is_empty(), "{:?}", c.notes);
}

#[test]
fn a_quieter_copy_matches_after_loudness_matching_and_reports_the_gain() {
    let a = base();
    let c = compare(&a, &scaled(&a, -6.0));
    assert!(c.distance_db < 0.2, "distance {}", c.distance_db);
    assert!((c.gain_db - 6.0).abs() < 0.3, "gain {}", c.gain_db);
    assert!(c.deviations.is_empty(), "{:?}", c.deviations);
    assert!(c.text().contains("+6.0 dB") || c.text().contains("+5.9 dB") || c.text().contains("+6.1 dB"));
}

#[test]
fn a_late_copy_is_aligned_by_its_onset() {
    let a = base();
    let late = delayed(&a, 0.1);
    let c = compare(&a, &late);
    assert!((c.shift_s - 0.1).abs() < 0.002, "shift {}", c.shift_s);
    assert!(c.distance_db < 0.5, "distance {}", c.distance_db);
    let early = compare(&late, &a);
    assert!((early.shift_s + 0.1).abs() < 0.002, "shift {}", early.shift_s);
    assert!(early.text().contains("moved later"), "{}", early.text());
}

#[test]
fn a_darker_sound_is_quieter_in_the_high_bands_and_the_words_name_them() {
    let bright = base();
    let dark = struck(modes(), Some(500.0), 0.8);
    let c = compare(&dark, &bright);
    assert!(c.distance_db > 3.0, "distance {}", c.distance_db);
    assert!(!c.deviations.is_empty());
    let high_quieter = c
        .deviations
        .iter()
        .filter(|d| d.band >= 5 && d.diff_db < 0.0)
        .count();
    assert!(high_quieter >= 2, "{:#?}", c.deviations);
    assert!(c.deviations[0].band >= 5, "{:#?}", c.deviations);
    let text = c.text();
    assert!(
        text.contains("0-20% of the sound:") && text.contains("kHz is") && text.contains("quieter than the reference"),
        "{text}"
    );
    assert!(text.contains("centroid") && text.contains("darker"), "{text}");
    assert!(c.deviations.len() <= 5);
    // The brighter sound against the dark reference is the mirror image.
    let m = compare(&bright, &dark);
    assert!(m.text().contains("louder than the reference"), "{}", m.text());
}

#[test]
fn a_longer_ringing_reference_is_reported_as_longer() {
    let short = struck(
        json!([[300, 0.1, 1.0], [870, 0.08, 0.6], [1700, 0.06, 0.5]]),
        None,
        1.0,
    );
    let long = struck(
        json!([[300, 0.6, 1.0], [870, 0.5, 0.6], [1700, 0.4, 0.5]]),
        None,
        1.0,
    );
    let c = compare(&short, &long);
    let text = c.text();
    assert!(
        c.notes.iter().any(|n| n.contains("the reference is") && n.contains("longer")),
        "{text}"
    );
    assert!(text.contains("decay"), "{text}");
    assert!(c.distance_db > 2.0, "distance {}", c.distance_db);
}

#[test]
fn another_sample_rate_and_mono_against_stereo_are_handled() {
    let a = base();
    // The same sound at 44.1 kHz, as a stereo file.
    let r = sfx_analysis::reference::resample(&a, 44_100);
    assert_eq!(r.sample_rate, 44_100);
    assert!((r.len() as f32 - a.len() as f32 * 44_100.0 / 48_000.0).abs() < 2.0);
    let stereo = Audio {
        sample_rate: 44_100,
        channels: vec![r.channels[0].clone(), r.channels[0].clone()],
    };
    let c = compare(&a, &stereo);
    // Not zero: the 44.1 kHz copy has nothing above 22 kHz, where the
    // original still has a little noise (the note says so).
    let far = compare(&a, &struck(modes(), Some(500.0), 0.8)).distance_db;
    assert!(
        c.distance_db < 4.0 && c.distance_db < 0.5 * far,
        "distance {} vs {far} for a darker sound",
        c.distance_db
    );
    assert!(c.notes.iter().any(|n| n.contains("nothing above 22.0 kHz")), "{:?}", c.notes);
    assert!(c.notes.iter().any(|n| n.contains("resampled")), "{:?}", c.notes);
    assert!(c.notes.iter().any(|n| n.contains("mono")), "{:?}", c.notes);
}

#[test]
fn the_image_has_three_columns_inside_the_pixel_budget() {
    let a = base();
    let dark = struck(modes(), Some(500.0), 0.8);
    let c = compare(&dark, &a);
    let cols = [
        Column {
            label: "sound".into(),
            audio: &dark,
        },
        Column {
            label: "reference".into(),
            audio: &c.aligned,
        },
    ];
    let d = DiffColumn {
        label: "sound minus reference (dB)".into(),
        a: &c.padded,
        b: &c.aligned,
    };
    let png = listen_png_diff(&cols, Some(&d), View::default()).unwrap();
    let (w, h) = png_size(&png);
    assert!(w * h <= MAX_PIXELS, "{w}x{h}");
    assert!(w % 3 == 0 && w / 3 >= 240, "three columns of {}", w / 3);
    let one = png_size(&listen_png(&cols[..1], View::default()).unwrap());
    assert!(one.0 < w, "one column {} px, three {w} px", one.0);
    // Blue (quieter) and red (louder) both show up in the difference column.
    let px = decode(&png);
    let third = (w / 3 * 2)..w;
    let (mut blue, mut red) = (0, 0);
    for y in 0..h {
        for x in third.clone() {
            let i = (y * w + x) * 4;
            let (r, g, b) = (px[i], px[i + 1], px[i + 2]);
            if b > 200 && r < 80 && g < 150 {
                blue += 1;
            }
            if r > 200 && g < 120 && b < 80 {
                red += 1;
            }
        }
    }
    assert!(blue > 500 && red > 50, "blue {blue}, red {red}");
}

fn decode(png: &[u8]) -> Vec<u8> {
    let mut r = png::Decoder::new(std::io::BufReader::new(std::io::Cursor::new(png))).read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    r.next_frame(&mut buf).unwrap();
    buf
}

fn png_size(png: &[u8]) -> (usize, usize) {
    let r = png::Decoder::new(std::io::BufReader::new(std::io::Cursor::new(png))).read_info().unwrap();
    let i = r.info();
    (i.width as usize, i.height as usize)
}

fn check_reference(doc: &SoundDoc, dir: &std::path::Path, file: &str, max: f32) -> Vec<sfx_analysis::CheckResult> {
    let opts = RenderOpts {
        base_dir: Some(dir.to_path_buf()),
        ..RenderOpts::default()
    };
    let r = doc.render(&opts).unwrap();
    let e: Expect = serde_json::from_value(json!({"reference": {"file": file, "max_distance": max}})).unwrap();
    run(&e, &CheckCx { doc, rendered: &r, opts: &opts }).unwrap_or_else(|e| vec![sfx_analysis::CheckResult {
        name: "error".into(),
        pass: false,
        detail: e,
    }])
}

#[test]
fn the_reference_check_passes_close_and_fails_far_with_words() {
    let dir = std::env::temp_dir().join(format!("sounder_reftest_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dark = struck(modes(), Some(500.0), 0.8);
    wav::write(&dir.join("dark.wav"), &dark, wav::Bits::Float32).unwrap();
    let mut doc = SoundDoc::named("t");
    doc.nodes = vec![
        json!({"type": "impulse", "name": "hit"}),
        json!({"type": "resonators", "name": "body", "input": "hit", "modes": modes()}),
        json!({"type": "output", "name": "out", "input": "body", "lufs": -20}),
    ];
    doc.duration = Some(0.8);
    // Against a file of its own render: relative path, resolved on base_dir.
    let own = doc.render(&RenderOpts::default()).unwrap().audio;
    wav::write(&dir.join("own.wav"), &own, wav::Bits::Float32).unwrap();
    let ok = check_reference(&doc, &dir, "own.wav", 1.0);
    assert!(ok[0].pass, "{}", ok[0].detail);
    assert!(ok[0].detail.contains("own.wav"), "{}", ok[0].detail);
    // Against the dark one: far, and the FAIL says where.
    let bad = check_reference(&doc, &dir, "dark.wav", 1.0);
    assert!(!bad[0].pass, "{}", bad[0].detail);
    assert!(
        bad[0].detail.contains("max 1") && bad[0].detail.contains("louder than the reference"),
        "{}",
        bad[0].detail
    );
    // A missing file is an error naming it.
    let missing = check_reference(&doc, &dir, "nope.wav", 1.0);
    assert!(!missing[0].pass && missing[0].detail.contains("nope.wav"), "{}", missing[0].detail);
    let _ = std::fs::remove_dir_all(&dir);
}
