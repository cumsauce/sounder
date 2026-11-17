//! Phase 4 (source material): files resolve against the project, then the
//! sources folders; `convolve` resamples an impulse response recorded at
//! another rate; editing a WAV a `grains` / `cloud` node reads re-renders.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sfx_graph::node::resolve_file;
use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sounder_sources_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tone(sr: u32, hz: f32, secs: f32, amp: f32) -> Audio {
    let n = (sr as f32 * secs) as usize;
    Audio {
        sample_rate: sr,
        channels: vec![
            (0..n)
                .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / sr as f32).sin())
                .collect(),
        ],
    }
}

fn render(
    nodes: Vec<Value>,
    out: &str,
    secs: f32,
    base: Option<&Path>,
    sources: &[PathBuf],
) -> Audio {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": out, "fade_in": 0.0005}));
    d.duration = Some(secs);
    d.render(&RenderOpts {
        base_dir: base.map(Path::to_path_buf),
        sources: sources.to_vec(),
        ..RenderOpts::default()
    })
    .unwrap_or_else(|e| panic!("{e}"))
    .audio
}

#[test]
fn files_resolve_in_the_project_then_the_sources_folders() {
    let root = temp("resolve");
    let (project, src_a, src_b) = (root.join("p"), root.join("a"), root.join("b"));
    for d in [&project, &src_a, &src_b.join("hits")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let t = tone(48_000, 440.0, 0.1, 0.5);
    wav::write(&project.join("here.wav"), &t, wav::Bits::Int16).unwrap();
    wav::write(&src_a.join("here.wav"), &t, wav::Bits::Int16).unwrap();
    wav::write(&src_b.join("hits").join("knock.wav"), &t, wav::Bits::Int16).unwrap();
    let sources = [src_a.clone(), src_b.clone()];
    // The project wins over a sources folder.
    assert_eq!(
        resolve_file("here.wav", Some(&project), &sources),
        project.join("here.wav")
    );
    // A path as written inside a sources folder.
    assert_eq!(
        resolve_file("hits/knock.wav", Some(&project), &sources),
        src_b.join("hits").join("knock.wav")
    );
    // A missing file names the project path (the error says where it looked first).
    assert_eq!(
        resolve_file("nope.wav", Some(&project), &sources),
        project.join("nope.wav")
    );
    // Absolute paths are as given.
    let abs = src_a.join("here.wav");
    assert_eq!(
        resolve_file(abs.to_str().unwrap(), Some(&project), &sources),
        abs
    );
    // A `sample` finds a file that only a sources folder holds.
    let a = render(
        vec![json!({"type": "sample", "name": "s", "file": "hits/knock.wav"})],
        "s",
        0.1,
        Some(&project),
        &sources,
    );
    let peak = a.channels[0].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.4, "the sample read silence: peak {peak}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn convolve_resamples_an_impulse_response_at_another_rate() {
    let dir = temp("convolve");
    // A 44.1 kHz response: a single click 10 ms in (441 samples).
    let mut ir = vec![0.0f32; 4410];
    ir[441] = 1.0;
    wav::write(
        &dir.join("ir.wav"),
        &Audio {
            sample_rate: 44_100,
            channels: vec![ir],
        },
        wav::Bits::Float32,
    )
    .unwrap();
    let nodes = vec![
        json!({"type": "impulse", "name": "hit", "at": 0.005}),
        json!({"type": "convolve", "name": "wet", "input": "hit", "ir": "ir.wav"}),
    ];
    let a = render(nodes, "wet", 0.1, Some(&dir), &[]);
    let x = &a.channels[0];
    let (i, _) =
        x.iter().enumerate().fold(
            (0, 0.0f32),
            |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
        );
    // 5 ms + 10 ms at 48 kHz = sample 720.
    println!("convolved click at sample {i} (want 720)");
    assert!((i as i64 - 720).abs() <= 1, "click at {i}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn editing_a_grains_source_re_renders() {
    let dir = temp("grains");
    let nodes = || {
        vec![
            json!({"type": "pattern", "name": "ev", "times": [0.01]}),
            json!({"type": "grains", "name": "g", "source": "src.wav", "events": "ev", "size": 0.05}),
        ]
    };
    wav::write(
        &dir.join("src.wav"),
        &tone(48_000, 300.0, 0.2, 0.5),
        wav::Bits::Float32,
    )
    .unwrap();
    // Cached renders (the default), so only the key can tell the files apart.
    let first = render(nodes(), "g", 0.1, Some(&dir), &[]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    wav::write(
        &dir.join("src.wav"),
        &tone(48_000, 900.0, 0.2, 0.5),
        wav::Bits::Float32,
    )
    .unwrap();
    let second = render(nodes(), "g", 0.1, Some(&dir), &[]);
    assert_ne!(
        first.channels, second.channels,
        "the edited source did not re-render"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
