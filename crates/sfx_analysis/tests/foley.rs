//! The foley physical-model nodes: bubble, air, glottis, tract, syrinx,
//! thunder. Numbers in comments are what these tests measured.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::stft::stft;
use sfx_analysis::{describe, pitch};
use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

const SR: u32 = 48_000;

/// Renders `nodes` (the last one named `last`) through a bare output stage.
fn render(nodes: Vec<Value>, last: &str, secs: f32) -> Vec<f32> {
    let mut nodes = nodes;
    nodes.push(json!({"type": "output", "name": "out", "input": last,
        "fade_in": 0, "fade_out": 0, "ceiling_db": 12}));
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(secs);
    d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap_or_else(|e| panic!("{e}"))
    .audio
    .mix_mono()
}

/// Frequency of the strongest bin of the mean spectrum between `lo` and `hi`.
fn peak_hz(x: &[f32], lo: f32, hi: f32) -> f32 {
    let s = stft(x, SR, 8192, 4096);
    let mean = s.mean(0..s.frames.len());
    let (a, b) = (s.bin(lo), s.bin(hi));
    let best = (a..=b)
        .max_by(|i, j| mean[*i].total_cmp(&mean[*j]))
        .unwrap();
    s.hz(best)
}

fn rms_db(x: &[f32]) -> f32 {
    10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32)
        .max(1e-12)
        .log10()
}

fn median_pitch(x: &[f32]) -> f32 {
    let mut v: Vec<f32> = pitch::yin(x, SR).iter().filter_map(|f| f.hz).collect();
    assert!(!v.is_empty(), "no voiced frames");
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

#[test]
fn bubble_pitch_follows_minnaert() {
    let mut last = 0.0;
    for (mm, expect) in [(2.0f32, 1643.0f32), (4.0, 821.0)] {
        let x = render(
            vec![json!({"type": "bubble", "name": "b", "radius_mm": mm, "rise": 0})],
            "b",
            0.3,
        );
        let hz = peak_hz(&x, 200.0, 8000.0);
        eprintln!("bubble {mm} mm: peak {hz:.0} Hz (Minnaert {expect})");
        assert!((hz - expect).abs() < 0.06 * expect, "{mm} mm: {hz}");
        if last > 0.0 {
            assert!(hz < last, "larger rings lower");
        }
        last = hz;
    }
}

/// Spectral centroid of the whole signal, Hz.
fn centroid(x: &[f32]) -> f32 {
    let s = stft(x, SR, 2048, 1024);
    let mean = s.mean(0..s.frames.len());
    let total: f32 = mean.iter().sum();
    mean.iter()
        .enumerate()
        .map(|(b, p)| s.hz(b) * p)
        .sum::<f32>()
        / total.max(1e-20)
}

#[test]
fn air_loses_highs_and_level_monotonically() {
    // Measured (white noise in): centroid 11.9 kHz at 1 m and 10 m (air
    // takes under 6 dB at 20 kHz there), 4.3 kHz at 50 m, 3.2 kHz at 100 m,
    // 1.9 kHz at 200 m; level -20 dB per decade of distance plus the filter.
    let mut prev: Option<(f32, f32)> = None;
    let mut first = (0.0, 0.0);
    for d in [1.0f32, 10.0, 50.0, 100.0, 200.0] {
        let x = render(
            vec![
                json!({"type": "noise", "name": "n", "seed": 5, "amp": 0.5}),
                json!({"type": "air", "name": "a", "input": "n", "distance": d}),
            ],
            "a",
            1.0,
        );
        let (c, l) = (centroid(&x), rms_db(&x));
        eprintln!("air {d} m: centroid {c:.0} Hz, rms {l:.1} dB");
        if let Some((pc, pl)) = prev {
            assert!(
                c <= pc + 1.0,
                "centroid never rises with distance: {c} vs {pc}"
            );
            assert!(l < pl, "level falls with distance: {l} vs {pl}");
        } else {
            first = (c, l);
        }
        prev = Some((c, l));
    }
    let (c, l) = prev.unwrap();
    assert!(
        c < 0.4 * first.0,
        "200 m is much duller than 1 m: {c} vs {}",
        first.0
    );
    assert!(l < first.1 - 40.0);
}

#[test]
fn glottis_reads_its_f0() {
    for f0 in [120.0f32, 220.0] {
        let x = render(
            vec![json!({"type": "glottis", "name": "g", "f0": f0})],
            "g",
            0.6,
        );
        let hz = median_pitch(&x);
        let cents = pitch::cents(hz, f0).abs();
        eprintln!("glottis {f0} Hz: YIN {hz:.1} Hz ({cents:.1} cents)");
        assert!(cents < 25.0, "{f0}: {hz}");
    }
}

#[test]
fn tract_moves_the_dominant_formant() {
    let mut peaks = Vec::new();
    for v in ["a", "i", "u"] {
        let x = render(
            vec![
                json!({"type": "glottis", "name": "g", "f0": 110, "breath": 0}),
                json!({"type": "tract", "name": "t", "input": "g", "vowel": v}),
            ],
            "t",
            0.6,
        );
        let hz = peak_hz(&x, 150.0, 1500.0);
        eprintln!(
            "tract {v}: dominant peak {hz:.0} Hz, peak {:.2}",
            x.iter().fold(0f32, |m, s| m.max(s.abs()))
        );
        peaks.push(hz);
    }
    // /a/ F1 730, /i/ F1 270, /u/ F1 300.
    assert!((peaks[0] - 730.0).abs() < 130.0, "{peaks:?}");
    assert!((peaks[1] - 270.0).abs() < 90.0, "{peaks:?}");
    assert!((peaks[2] - 300.0).abs() < 90.0, "{peaks:?}");
    assert!(peaks[0] > peaks[1] + 300.0);
}

#[test]
fn syrinx_species_and_whistle() {
    let a = render(
        vec![json!({"type": "syrinx", "name": "s", "species": "robin", "seed": 2})],
        "s",
        2.0,
    );
    assert!(a.iter().all(|v| v.is_finite()));
    let d = describe(&sfx_graph::Audio::mono(SR, a.clone()));
    eprintln!(
        "robin: peak {:.1} dB, pitch {:?}, centroid {:.0}",
        d.peak_db, d.pitch_hz, d.centroid_hz.middle
    );
    assert!(d.peak_db > -40.0);
    // A whistle follows its contour: 3 kHz glide down to 2 kHz.
    let w = render(
        vec![
            json!({"type": "ramp", "name": "r", "from": 3000, "to": 2000, "time": 0.4}),
            json!({"type": "syrinx", "name": "s", "pitch": "r", "amp": 0.5}),
        ],
        "s",
        0.4,
    );
    let hz = median_pitch(&w);
    eprintln!("whistle median {hz:.0}");
    assert!((2000.0..=3000.0).contains(&hz), "{hz}");
}

#[test]
fn thunder_is_low_long_and_decays() {
    let x = render(
        vec![json!({"type": "thunder", "name": "t", "distance": 1500, "seed": 3})],
        "t",
        14.0,
    );
    let n = |secs: f32| (secs * SR as f32) as usize;
    assert!(x.iter().all(|v| v.is_finite()));
    // Silent until the sound arrives: 1500 m / 343 m/s = 4.37 s.
    assert!(rms_db(&x[..n(4.0)]) < -80.0, "quiet before arrival");
    assert!(rms_db(&x[n(4.4)..n(8.0)]) > -45.0);
    // Low: well under a few hundred Hz after the crack.
    let body = &x[n(4.4)..n(12.0)];
    let c = centroid(body);
    let d = describe(&Audio::mono(SR, x.clone()));
    eprintln!(
        "thunder: centroid {c:.0} Hz, peak {:.1} dB, audible {:.1}-{:.1} s, decay20 {:?}",
        d.peak_db,
        d.silence_start_s,
        14.0 - d.silence_end_s,
        d.decay20_s
    );
    assert!(c < 300.0, "centroid {c}");
    // Long: audible for many seconds (the rumble), and decaying.
    let thirds = [n(4.4), n(7.0), n(9.5), n(12.0)];
    let e: Vec<f32> = thirds.windows(2).map(|w| rms_db(&x[w[0]..w[1]])).collect();
    eprintln!("thunder rms by 2.5 s: {e:?}");
    assert!(e[2] < e[0] - 6.0, "decays: {e:?}");
    assert!(d.decay20_s.is_some_and(|t| t > 1.0), "long decay");
    // A nearer flash is brighter.
    let near = render(
        vec![json!({"type": "thunder", "name": "t", "distance": 150, "seed": 3})],
        "t",
        6.0,
    );
    let cn = centroid(&near);
    let pk = describe(&Audio::mono(SR, near.clone())).peak_db;
    eprintln!("thunder 150 m centroid {cn:.0}, peak {pk:.1} dB");
    assert!(cn > c, "{cn} vs {c}");
}

fn all_foley() -> Vec<Value> {
    vec![
        json!({"type": "bubble", "name": "b", "radius_mm": 2.5, "jitter": 0.3, "seed": 2, "at": 0.1}),
        json!({"type": "noise", "name": "n", "seed": 1}),
        json!({"type": "air", "name": "a", "input": "n", "distance": 60}),
        json!({"type": "glottis", "name": "g", "f0": 150, "vibrato_cents": 30, "seed": 3}),
        json!({"type": "tract", "name": "t", "input": "g", "vowel": "o", "vowel_to": "e", "morph": 0.4}),
        json!({"type": "syrinx", "name": "s", "species": "cardinal", "seed": 4}),
        json!({"type": "thunder", "name": "th", "distance": 300, "seed": 5}),
        json!({"type": "mix", "name": "all", "inputs": ["b", "a", "t", "s", "th"]}),
    ]
}

fn bytes(nodes: Vec<Value>, threads: usize) -> Vec<u8> {
    let mut nodes = nodes;
    nodes.push(json!({"type": "output", "name": "out", "input": "all", "lufs": -20}));
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = Some(3.0);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    pool.install(|| {
        let r = d
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        wav::to_bytes16(&r.audio)
    })
}

#[test]
fn foley_nodes_are_deterministic_at_any_thread_count() {
    let one = bytes(all_foley(), 1);
    assert!(one.len() > 1000);
    assert_eq!(one, bytes(all_foley(), 8));
    // And the seed matters.
    let mut other = all_foley();
    other[5]["seed"] = json!(9);
    assert_ne!(one, bytes(other, 1));
}

#[test]
fn foley_nodes_are_finite_on_edge_values() {
    let cases = [
        json!({"type": "bubble", "name": "x", "radius_mm": 0.1}),
        json!({"type": "bubble", "name": "x", "radius_mm": 80}),
        json!({"type": "thunder", "name": "x", "distance": 0}),
        json!({"type": "thunder", "name": "x", "distance": 30000, "exposure": 0}),
        json!({"type": "glottis", "name": "x", "f0": 0}),
        json!({"type": "glottis", "name": "x", "f0": 30000, "tension": 5, "breath": 3}),
        json!({"type": "syrinx", "name": "x", "pitch": 0}),
        json!({"type": "syrinx", "name": "x", "pitch": 40000}),
        json!({"type": "syrinx", "name": "x", "species": "dove", "rate": 8}),
    ];
    for c in cases {
        let x = render(vec![c.clone()], "x", 1.0);
        assert!(x.iter().all(|v| v.is_finite()), "{c}");
    }
    for d in [0.0f32, 0.001, 1e6] {
        let x = render(
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "air", "name": "x", "input": "n", "distance": d, "humidity": 0}),
            ],
            "x",
            0.5,
        );
        assert!(x.iter().all(|v| v.is_finite()), "air {d}");
    }
    for scale in [0.1f32, 1.0, 9.0] {
        let x = render(
            vec![
                json!({"type": "glottis", "name": "g", "f0": 200}),
                json!({"type": "tract", "name": "x", "input": "g", "scale": scale}),
            ],
            "x",
            0.5,
        );
        assert!(x.iter().all(|v| v.is_finite()), "tract {scale}");
    }
}

#[test]
fn foley_nodes_refuse_bad_fields_helpfully() {
    let mut d = SoundDoc::named("t");
    for bad in [
        json!({"type": "bubble", "name": "x", "radius_mm": 0}),
        json!({"type": "syrinx", "name": "x", "species": "dodo"}),
        json!({"type": "syrinx", "name": "x"}),
        json!({"type": "tract", "name": "x", "input": "x", "vowel": "q"}),
    ] {
        d.nodes = vec![
            bad.clone(),
            json!({"type": "output", "name": "out", "input": "x"}),
        ];
        assert!(d.render(&RenderOpts::default()).is_err(), "{bad}");
    }
}
