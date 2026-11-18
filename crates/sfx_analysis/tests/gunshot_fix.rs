//! gunshot_fix: suppressed shots were heard as "hitting a metal sheet" and
//! "lo-fi slam, no high end or detail". Measured here: no pitched thump
//! (flatness up), energy above 4 kHz (the action clicks), the action's two
//! onsets, and distance darkening at 50-120 m.

mod common;

use common::*;
use serde_json::json;
use sfx_analysis::measure::measure;
use sfx_graph::{Audio, Measure};

fn shot(supp: bool, d: f32) -> Vec<f32> {
    node_audio(
        [
            json!({"type": "gunshot", "name": "x", "caliber": 9, "suppressed": supp, "distance": d, "seed": 1}),
        ],
        "x",
        4.0,
    )
}

fn m(x: &[f32], k: Measure) -> f32 {
    measure(&Audio::mono(SR, x.to_vec()), k)
}

/// dB of the band against the whole signal.
fn share_db(x: &[f32], lo: f32, hi: f32) -> f32 {
    let p = x.iter().fold(1e-9f32, |a, v| a.max(v.abs()));
    let x: Vec<f32> = x.iter().map(|v| v / p).collect();
    let x = x.as_slice();
    band_db(x, SR, lo, hi) - band_db(x, SR, 0.0, 24_000.0)
}

#[test]
fn suppressed_is_atonal_with_high_detail() {
    let x = shot(true, 0.0);
    assert_finite(&x, "suppressed");
    let (c, f) = (m(&x, Measure::Centroid), m(&x, Measure::Flatness));
    let (lo, hi, top) = (
        share_db(&x, 0.0, 150.0),
        share_db(&x, 4000.0, 12_000.0),
        share_db(&x, 8000.0, 12_000.0),
    );
    println!(
        "suppressed 9 mm: centroid {c:.0} Hz, flatness {f:.3}, <150 Hz {lo:.1} dB, 4-12 kHz {hi:.1} dB, 8-12 kHz {top:.1} dB"
    );
    assert!(c > 1500.0, "centroid {c}");
    assert!(hi > -12.0, "4-12 kHz share {hi} dB");
    assert!(top > -22.0, "8-12 kHz share {top} dB");
    // Round 2: an 80-400 Hz noise body (heard as "a small hit" without it)
    // spills under 150 Hz (-12.7 dB); the old pitched thump read -7.0 dB.
    assert!(lo < -10.0, "no sub thump: {lo} dB");
    assert!(c < 3500.0, "a body under the clicks: centroid {c}");
}

#[test]
fn suppressed_has_the_action_after_the_puff() {
    let x = shot(true, 0.0);
    // Peak 10 ms-windowed energy after 30 ms (the slide's return) is within
    // 20 dB of the shot's peak.
    let w = (0.002 * SR as f32) as usize;
    let p = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let late = x[(0.03 * SR as f32) as usize..(0.08 * SR as f32) as usize]
        .chunks(w)
        .map(|c| c.iter().fold(0.0f32, |a, v| a.max(v.abs())))
        .fold(0.0f32, f32::max);
    let db = 20.0 * (late / p).log10();
    println!("suppressed: action return peak {db:.1} dB under the shot");
    assert!(db > -20.0);
}

#[test]
fn distance_keeps_the_high_end_at_50_to_120_m() {
    for d in [0.0f32, 50.0, 120.0, 1000.0] {
        for supp in [false, true] {
            let x = shot(supp, d);
            let c = m(&x, Measure::Centroid);
            let hi = share_db(&x, 4000.0, 12_000.0);
            println!("suppressed {supp} distance {d} m: centroid {c:.0} Hz, 4-12 kHz {hi:.1} dB");
            if d > 0.0 && d <= 120.0 {
                assert!(hi > -18.0, "distance {d}: 4-12 kHz {hi}");
            }
        }
    }
}
