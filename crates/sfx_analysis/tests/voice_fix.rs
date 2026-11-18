//! voice_fix: the `natural` defaults of `glottis`, `tract` and `bubble`
//! (batch 2 blind labels: creature calls heard as "SAM" / "artificial",
//! drips heard as a UI click / cartoon pop). Each test measures the old
//! output (`natural: false`) against the new default and prints both.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_graph::Audio;

const SR: u32 = 48_000;

/// Off-grid against on-grid energy (dB) above 4 kHz, minus the same ratio
/// under 4 kHz. A bin is off the grid at least 0.3 x f0 from every harmonic.
/// Shimmer noise sits between the harmonics in proportion to them at every
/// frequency, so it cancels; aliasing folds only into the top and stays.
fn alias_excess_db(x: &[f32], f0: f32) -> f32 {
    let s = spectrum(x, SR);
    let mut e = [[0.0f64; 2]; 2]; // [band][on, off]
    for (f, d) in s {
        if f < 0.5 * f0 {
            continue;
        }
        let p = 10f64.powf(f64::from(d) / 10.0);
        let k = (f / f0).round();
        let off = usize::from((f - k * f0).abs() > 0.3 * f0);
        e[usize::from(f > 4000.0)][off] += p;
    }
    let r = |b: [f64; 2]| 10.0 * (b[1].max(1e-30) / b[0].max(1e-30)).log10();
    (r(e[1]) - r(e[0])) as f32
}

fn glottis(f0: f32, natural: bool) -> Value {
    json!({"type": "glottis", "name": "g", "f0": f0, "breath": 0, "natural": natural})
}

#[test]
fn natural_glottis_is_band_limited() {
    let mut res = vec![];
    for natural in [false, true] {
        let x = node_audio([glottis(2900.0, natural)], "g", 1.0);
        assert_finite(&x, "glottis");
        res.push(alias_excess_db(&x[4800..], 2900.0));
    }
    eprintln!(
        "alias excess (off/on grid above 4 kHz minus below) at f0 2900: old {:.1} dB, natural {:.1} dB",
        res[0], res[1]
    );
    assert!(res[1] < res[0] - 12.0, "{res:?}");
}

/// Spread (max - min, cents) of the pitch track over the voiced frames.
fn pitch_spread_cents(x: &[f32]) -> f32 {
    let v: Vec<f32> = sfx_analysis::pitch::yin(x, SR)
        .iter()
        .filter_map(|f| f.hz)
        .collect();
    let lo = v.iter().copied().fold(f32::MAX, f32::min);
    let hi = v.iter().copied().fold(0.0, f32::max);
    1200.0 * (hi / lo).log2()
}

#[test]
fn natural_glottis_wanders_and_breathes_with_the_pulse() {
    let old = node_audio(
        [json!({"type": "glottis", "name": "g", "f0": 140, "natural": false})],
        "g",
        2.0,
    );
    let new = node_audio(
        [json!({"type": "glottis", "name": "g", "f0": 140})],
        "g",
        2.0,
    );
    let (so, sn) = (pitch_spread_cents(&old), pitch_spread_cents(&new));
    eprintln!("pitch spread over 2 s: old {so:.1} cents, natural {sn:.1} cents");
    assert!(so < 6.0, "{so}");
    // Round 2: +-60 cents of drift (round 1 read 25 cents and was heard
    // as identical to the old voice).
    assert!((40.0..200.0).contains(&sn), "{sn}");
    // Still reads its f0 (median).
    let mut v: Vec<f32> = sfx_analysis::pitch::yin(&new, SR)
        .iter()
        .filter_map(|f| f.hz)
        .collect();
    v.sort_by(f32::total_cmp);
    let med = v[v.len() / 2];
    assert!((1200.0 * (med / 140.0).log2()).abs() < 40.0, "{med}");
    // The aspiration is above the voice band.
    let hi = |x: &[f32]| band_db(x, SR, 6000.0, 16000.0) - band_db(x, SR, 100.0, 16000.0);
    eprintln!(
        "share above 6 kHz: old {:.1} dB, natural {:.1} dB",
        hi(&old),
        hi(&new)
    );
    assert!(hi(&new) > hi(&old));
}

fn vowel(natural: bool) -> Vec<Value> {
    vec![
        json!({"type": "glottis", "name": "g", "f0": 120, "natural": natural}),
        json!({"type": "tract", "name": "t", "input": "g", "vowel": "a", "natural": natural}),
        json!({"type": "output", "name": "out", "input": "t", "lufs": -20}),
    ]
}

fn centroid_range(a: &Audio) -> f32 {
    let d = describe(a);
    let c: Vec<f32> = d.centroid_tenths.iter().flatten().copied().collect();
    let lo = c.iter().copied().fold(f32::MAX, f32::min);
    let hi = c.iter().copied().fold(0.0, f32::max);
    hi / lo
}

#[test]
fn natural_tract_moves_and_is_not_razor_sharp() {
    let old = render(vowel(false), 3.0);
    let new = render(vowel(true), 3.0);
    let (ro, rn) = (centroid_range(&old), centroid_range(&new));
    eprintln!("centroid by tenth max/min: old {ro:.3}, natural {rn:.3}");
    assert!(rn > ro, "{ro} {rn}");
    let (dold, dnew) = (describe(&old), describe(&new));
    eprintln!(
        "voiced {:.2} -> {:.2}, harmonicity {:.2} -> {:.2}, flatness {:.3} -> {:.3}",
        dold.voiced,
        dnew.voiced,
        dold.harmonicity,
        dnew.harmonicity,
        dold.flatness_all,
        dnew.flatness_all
    );
    assert!(dnew.voiced > 0.8, "still a voice");
}

fn drip(natural: bool) -> Vec<Value> {
    vec![
        json!({"type": "bubble", "name": "b", "radius_mm": 2.5, "at": 0.01, "natural": natural, "seed": 1}),
    ]
}

#[test]
fn natural_bubble_has_an_impact_and_a_shorter_ring() {
    let old = node_audio(drip(false), "b", 0.5);
    let new = node_audio(drip(true), "b", 0.5);
    assert_finite(&new, "bubble");
    let s = (0.01 * SR as f32) as usize;
    let w = (0.004 * SR as f32) as usize;
    let hi = |x: &[f32]| band_db(&x[s..s + w], SR, 6000.0, 20000.0);
    eprintln!(
        "first 4 ms above 6 kHz: old {:.1} dB, natural {:.1} dB",
        hi(&old),
        hi(&new)
    );
    assert!(hi(&new) > hi(&old) + 20.0);
    let d = |x: &[f32]| describe(&Audio::mono(SR, x.to_vec()));
    let (dold, dnew) = (d(&old), d(&new));
    eprintln!(
        "decay -20 dB: old {:?}, natural {:?}; flatness {:.4} -> {:.4}; pitch {:?} -> {:?}",
        dold.decay20_s,
        dnew.decay20_s,
        dold.flatness_all,
        dnew.flatness_all,
        dold.pitch_hz,
        dnew.pitch_hz
    );
    assert!(dnew.decay20_s.unwrap() < dold.decay20_s.unwrap());
    assert!(dnew.flatness_all > dold.flatness_all);
    // The ring is still the Minnaert pitch: the strongest bin under 4 kHz.
    let p = peaks(&new, SR, 1, 0.0)[0].0;
    assert!((p - 3290.0 / 2.5).abs() < 0.1 * 1316.0, "{p}");
}

#[test]
fn natural_voices_are_deterministic() {
    let mut nodes = vowel(true);
    nodes.extend(drip(true));
    nodes[2] = json!({"type": "mix", "name": "m", "inputs": ["t", "b"]});
    nodes.push(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
    assert_bit_identical(nodes, 1.0);
}
