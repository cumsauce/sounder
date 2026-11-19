//! The shared test helpers measure known signals correctly.

use crate::common::*;
use serde_json::json;
use sfx_graph::dsp::Oversampler;

fn tone_nodes() -> Vec<serde_json::Value> {
    vec![
        json!({"type": "osc", "name": "tone", "shape": "sine", "freq": 1000}),
        json!({"type": "gain", "name": "amp", "input": "tone", "gain": 0.5}),
    ]
}

#[test]
fn render_and_node_audio_return_the_sound() {
    let nodes = tone_nodes();
    let a = render(&nodes, 0.5);
    assert_eq!(a.sample_rate, SR);
    assert_eq!(a.len(), (0.5 * SR as f32) as usize);
    let x = node_audio(&nodes, "amp", 0.5);
    assert_eq!(x.len(), a.len());
    assert_finite(&x, "amp");
    let peak = peak_hz(&x, SR);
    println!(
        "osc 1000 Hz at gain 0.5: peak {peak:.2} Hz, rms {:.2} dB",
        rms_db(&x)
    );
    assert!((peak - 1000.0).abs() < 1.0);
    assert!((rms_db(&x) - (-9.03)).abs() < 0.1);
    // A node of its own can be read before the output stage.
    let tone = node_audio(&nodes, "tone", 0.5);
    assert!((rms_db(&tone) - (-3.01)).abs() < 0.1);
}

#[test]
fn bit_identical_passes_for_a_seeded_noise() {
    assert_bit_identical(
        [
            json!({"type": "noise", "name": "n", "color": "pink"}),
            json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": 800, "q": 3}),
        ],
        0.3,
    );
}

#[test]
#[should_panic(expected = "unknown")]
fn render_panics_with_the_graph_error() {
    render([json!({"type": "osc", "name": "o", "frq": 100})], 0.1);
}

#[test]
fn spectrum_and_peaks_find_the_partials() {
    let x: Vec<f32> = sine_samples(700.0, 0.5, 1.0)
        .iter()
        .zip(sine_samples(1300.0, 0.25, 1.0))
        .map(|(a, b)| a + b)
        .collect();
    let s = spectrum(&x, SR);
    assert!(s.len() > 1000 && s[0].0 == 0.0 && (s.last().unwrap().0 - 24_000.0).abs() < 1.0);
    let p = peaks(&x, SR, 2, 40.0);
    println!("peaks {p:?}");
    assert!((p[0].0 - 700.0).abs() < 1.0 && (p[1].0 - 1300.0).abs() < 1.0);
    assert!(
        (p[0].1 - p[1].1 - 6.0).abs() < 0.5,
        "amplitude ratio 2 is 6 dB"
    );
    // The stft convention: amplitude 0.5 sums to -9 dB.
    let l = band_db(&x, SR, 600.0, 800.0);
    println!("700 Hz band: {l:.2} dB");
    assert!((l - (-9.03)).abs() < 0.2);
    assert!(band_db(&x, SR, 2000.0, 8000.0) < -80.0);
}

#[test]
fn rms_db_reads_levels() {
    let x = sine_samples(440.0, 1.0, 1.0);
    assert!((rms_db(&x) - (-3.01)).abs() < 0.01);
    assert_eq!(rms_db(&[]), -120.0);
    assert_eq!(rms_db(&vec![0.0; 100]), -120.0);
}

#[test]
fn alias_db_tells_a_clean_tone_from_a_folded_one() {
    let clean = sine_samples(5000.0, 0.8, 1.0);
    let c = alias_db(&clean, SR, 5000.0);
    let clip = |v: f32| (v * 8.0).clamp(-1.0, 1.0);
    let hard: Vec<f32> = sine_samples(5000.0, 1.0, 1.0)
        .iter()
        .map(|v| clip(*v))
        .collect();
    let mut os = Oversampler::new(8).unwrap();
    let soft: Vec<f32> = sine_samples(5000.0, 1.0, 1.0)
        .iter()
        .map(|v| os.process(*v, clip))
        .collect();
    let (h, o) = (alias_db(&hard, SR, 5000.0), alias_db(&soft, SR, 5000.0));
    println!("off-grid energy: clean {c:.1} dB, hard clip {h:.1} dB, clip at 8x {o:.1} dB");
    assert!(c < -55.0);
    assert!(h > -20.0 && h < -5.0);
    assert!(o < h - 20.0);
}

#[test]
#[should_panic(expected = "sample 2 is NaN")]
fn assert_finite_names_the_bad_sample() {
    assert_finite(&[0.0, 1.0, f32::NAN], "test");
}
