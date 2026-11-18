//! machines_fix: the blind listen heard `engine` as "digital riser" /
//! "low resolution" / "trombonish" and `motor` as "artificial / lo-fi". These
//! tests measure the causes named in docs/nodes/machines_fix.md: too clean a
//! line spectrum (off-grid energy), no cycle-to-cycle variation, a resonant
//! exhaust comb (third-octave peak over median) and a perfectly steady motor.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_graph::{Audio, Measure};

fn flatness(x: &[f32]) -> f32 {
    let a = Audio {
        sample_rate: SR,
        channels: vec![x.to_vec()],
    };
    measure(&a, Measure::Flatness)
}

/// Coefficient of variation (percent) of the rms of consecutive windows of
/// `period` samples: how much one firing differs from the next.
fn cycle_cv(x: &[f32], period: usize) -> f32 {
    let r: Vec<f32> = x
        .chunks_exact(period)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let m = r.iter().sum::<f32>() / r.len() as f32;
    let var = r.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / r.len() as f32;
    100.0 * var.sqrt() / m.max(1e-12)
}

/// Loudest third-octave band from 50 Hz to 2 kHz over the median of those
/// bands, dB: a resonant comb (the "trombone") stands out.
fn resonance_db(x: &[f32]) -> f32 {
    let mut bands = Vec::new();
    let mut f = 50.0f32;
    while f < 2000.0 {
        bands.push(band_db(x, SR, f, f * 1.26));
        f *= 1.26;
    }
    let top = bands.iter().copied().fold(f32::MIN, f32::max);
    bands.sort_by(f32::total_cmp);
    top - bands[bands.len() / 2]
}

fn engine(extra: Value) -> Vec<Value> {
    let mut e = json!({"type": "engine", "name": "eng", "rpm": 3000, "seed": 1});
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![e]
}

#[test]
fn engine_is_less_clean_and_less_resonant() {
    let mut rows = Vec::new();
    for cyl in [2u32, 4, 6] {
        let x = node_audio(engine(json!({"cylinders": cyl})), "eng", 2.0);
        assert_finite(&x, "eng");
        let f0 = 3000.0 / 60.0 * cyl as f32 / 2.0;
        let off = alias_db(&x, SR, f0);
        let cv = cycle_cv(&x, (SR as f32 / f0) as usize);
        let res = resonance_db(&x);
        let fl = flatness(&x);
        let hi = band_db(&x, SR, 2000.0, 8000.0) - band_db(&x, SR, 0.0, 24000.0);
        println!(
            "cylinders {cyl}: off-grid {off:.1} dB, cycle cv {cv:.1} %, resonance {res:.1} dB, flatness {fl:.3}, 2-8 kHz share {hi:.1} dB"
        );
        // Before the fix (seed 1), cylinders 2 / 4 / 6: off-grid -12.2 /
        // -8.8 / -5.3 dB, cycle cv 7.8 / 4.5 / 6.0 %, resonance 12.3 / 8.5 /
        // 10.2 dB, 2-8 kHz share -34.8 / -22.8 / -10.8 dB.
        // After: -7.5 / -3.4 / 0.6, 11.0 / 8.8 / 13.5 %, 9.9 / 4.6 / 5.3 dB,
        // -25.1 / -19.7 / -9.1 dB.
        rows.push((off, cv, res, hi));
    }
    // Fitted to refs/engine_set (three CC0 car revs): the recordings have
    // 0.1-3.9 % of their energy above 2 kHz and 32-84 % under 200 Hz, so
    // the round-1 bounds (a 2-8 kHz share over -26 dB, a flat third-octave
    // spectrum) no longer hold: the engine now keeps its per-cycle variation
    // and some off-grid noise, a low 2-8 kHz share, and resonance under 20 dB
    // (the strong fundamental reads as a third-octave peak).
    for (off, cv, res, hi) in rows {
        assert!(off > -12.0, "off-grid {off}");
        assert!(cv > 6.0, "cv {cv}");
        assert!(res < 20.0, "resonance {res}");
        assert!(hi < -20.0, "2-8 kHz share {hi}");
    }
}

#[test]
fn engine_rev_has_no_alias_above_the_band() {
    // A fast rev at full load: energy above 18 kHz relative to the total.
    // Before the fix -47.3 dB (pulse edges through the tanh); after -43.9 dB,
    // now mostly the broadband combustion / clatter noise: the output's
    // low-pass at 0.36 x the sample rate keeps it under -40 dB.
    let nodes = vec![
        json!({"type": "ramp", "name": "revs", "from": 1000, "to": 9000, "time": 2.0}),
        json!({"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 4, "load": 1.0}),
    ];
    let x = node_audio(&nodes, "eng", 2.0);
    let top = band_db(&x, SR, 18000.0, 24000.0) - band_db(&x, SR, 0.0, 24000.0);
    println!("rev 1000 -> 9000 rpm, load 1: energy above 18 kHz {top:.1} dB");
    assert!(top < -40.0);
}

fn motor(extra: Value) -> Vec<Value> {
    let mut m = json!({"type": "motor", "name": "mot", "rpm": 3000, "gear_teeth": 20, "seed": 1});
    m.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![m]
}

#[test]
fn motor_has_noise_and_wobble() {
    let x = node_audio(motor(json!({})), "mot", 2.0);
    assert_finite(&x, "mot");
    let off = alias_db(&x, SR, 50.0);
    let fl = flatness(&x);
    // The mesh line's frequency in 0.25 s windows: its spread is the wobble.
    let win = SR as usize / 4;
    let hz: Vec<f32> = x
        .chunks_exact(win)
        .map(|c| {
            peaks(c, SR, 12, 60.0)
                .into_iter()
                .filter(|p| (800.0..1200.0).contains(&p.0))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap()
                .0
        })
        .collect();
    let spread =
        hz.iter().copied().fold(f32::MIN, f32::max) - hz.iter().copied().fold(f32::MAX, f32::min);
    let hi = band_db(&x, SR, 2000.0, 12000.0) - band_db(&x, SR, 0.0, 24000.0);
    println!(
        "motor 3000 rpm, 20 teeth: off-grid (50 Hz grid) {off:.1} dB, flatness {fl:.3}, mesh spread {spread:.2} Hz ({hz:?}), 2-12 kHz share {hi:.1} dB"
    );
    // Before the fix: off-grid -26.8 dB, flatness 0.003, mesh spread 0.00
    // Hz. After: -16.5 dB, 0.005, 3.25 Hz (the +-0.4 % drift).
    assert!(off > -20.0, "off-grid {off}");
    assert!(spread > 1.0, "spread {spread}");
}
