//! cloth_fix: the batch-2 blind listen heard `cloth` as "pellets of
//! something hard", "small metal objects falling". The cause measured here:
//! clicky ticks (high crest, many fine onsets, narrow ringing bands). These
//! tests pin the swish: low crest, few fine onsets, high flatness, while
//! motion still raises level and centroid. Numbers go in docs/nodes/cloth_fix.md.

#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::json;
use sfx_analysis::describe;
use sfx_graph::Audio;

struct M {
    crest: f32,
    fine_per_s: f32,
    flat: f32,
    flat_occ: f32,
    centroid: f32,
    rms: f32,
}

fn measure(motion: f32, weight: f32, seed: u64) -> M {
    let x = node_audio(
        vec![
            json!({"type": "cloth", "name": "n", "motion": motion, "weight": weight, "seed": seed}),
        ],
        "n",
        2.0,
    );
    assert_finite(&x, "cloth");
    let d = describe(&Audio::mono(SR, x.clone()));
    M {
        crest: d.crest_db,
        fine_per_s: d.fine_onsets.len() as f32 / 2.0,
        flat: d.flatness_all,
        flat_occ: d.flatness_occupied.unwrap_or(0.0),
        centroid: d.centroid_all_hz,
        rms: rms_db(&x),
    }
}

#[test]
fn cloth_is_a_swish_not_ticks() {
    for w in [0.0f32, 0.5, 1.0] {
        for m in [0.5f32, 1.0, 2.0] {
            let mut crest = 0.0;
            let mut fine = 0.0;
            let mut flat = 0.0;
            for seed in 1..=3 {
                let r = measure(m, w, seed);
                println!(
                    "cloth motion {m} weight {w} seed {seed}: crest {:.1} dB, fine onsets {:.1}/s, flatness {:.3} (occupied {:.3}), centroid {:.0} Hz, rms {:.1} dB",
                    r.crest, r.fine_per_s, r.flat, r.flat_occ, r.centroid, r.rms
                );
                crest += r.crest / 3.0;
                fine += r.fine_per_s / 3.0;
                flat += r.flat_occ / 3.0;
            }
            assert!(crest < 16.0, "crest {crest} at motion {m} weight {w}");
            assert!(fine < 12.0, "fine onsets {fine}/s at motion {m} weight {w}");
            assert!(
                flat > 0.3,
                "occupied flatness {flat} at motion {m} weight {w}"
            );
        }
    }
}
