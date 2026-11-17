//! R3-F: `thunder.rumble`.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};

fn fnv(x: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

fn thunder(base: Value, extra: Value, secs: f32) -> Vec<f32> {
    let mut n = json!({"type": "thunder", "name": "t"});
    let o = n.as_object_mut().unwrap();
    o.extend(base.as_object().unwrap().clone());
    o.extend(extra.as_object().unwrap().clone());
    node_audio(vec![n], "t", secs)
}

fn flash() -> Value {
    json!({"distance": 1500, "seed": 3})
}

/// Hashes taken from the code before `rumble` existed.
#[test]
fn defaults_are_bit_identical() {
    let cases: [(Value, f32, u64); 4] = [
        (flash(), 16.0, 0xebec_a2f8_5b5e_d486),
        (json!({"distance": 300, "seed": 5}), 4.0, HASH_300),
        (
            json!({"distance": 800, "seed": 2, "exposure": 0.5, "brightness": 0.8}),
            8.0,
            HASH_800,
        ),
        (
            json!({"distance": 500, "seed": 7, "length": 1.5, "at": 0.2}),
            8.0,
            HASH_LEN,
        ),
    ];
    for (base, secs, want) in cases {
        let none = fnv(&thunder(base.clone(), json!({}), secs));
        let zero = fnv(&thunder(base.clone(), json!({"rumble": 0}), secs));
        assert_eq!(none, want, "{base}: {none:016x}");
        assert_eq!(zero, want, "{base} rumble 0");
    }
}

const HASH_300: u64 = 0x5af5_d872_afe9_e8b6;
const HASH_800: u64 = 0x4b63_e5a6_60da_b7e0;
const HASH_LEN: u64 = 0xc595_7d04_50da_faa0;

/// First sample above 1e-4 and the last above 1e-3, in samples.
fn span(x: &[f32]) -> (usize, usize) {
    (
        x.iter().position(|v| v.abs() > 1e-4).unwrap(),
        x.iter().rposition(|v| v.abs() > 1e-3).unwrap(),
    )
}

/// Min and max of the 400 ms rms (dB) in windows between 40 % and 80 % of
/// the audible span (hop 100 ms).
fn mid_spread(x: &[f32]) -> (f32, f32) {
    let (a, e) = span(x);
    let (lo, hi) = (a + (e - a) * 4 / 10, a + (e - a) * 8 / 10);
    let w = SR as usize * 4 / 10;
    let mut s = lo;
    let (mut mn, mut mx) = (f32::MAX, f32::MIN);
    while s + w <= hi {
        let r = rms_db(&x[s..s + w]);
        mn = mn.min(r);
        mx = mx.max(r);
        s += SR as usize / 10;
    }
    (mn, mx)
}

#[test]
fn rumble_makes_the_tail_continuous() {
    let mut worse = 0;
    for seed in [1u64, 3, 5, 8] {
        let base = json!({"distance": 1500, "seed": seed});
        let plain = thunder(base.clone(), json!({}), 16.0);
        let bed = thunder(base.clone(), json!({"rumble": 0.6}), 16.0);
        let (pa, pb) = mid_spread(&plain);
        let (ba, bb) = mid_spread(&bed);
        eprintln!(
            "seed {seed}: default {pa:.1}..{pb:.1} ({:.1} dB), rumble 0.6 {ba:.1}..{bb:.1} ({:.1} dB)",
            pb - pa,
            bb - ba
        );
        assert!(bb - ba < 6.0, "seed {seed}: spread {:.1} dB", bb - ba);
        if pb - pa > bb - ba {
            worse += 1;
        }
    }
    assert!(
        worse == 4,
        "the default varies more than the bed in every seed"
    );
}

#[test]
fn rumble_stays_low_and_scales() {
    let base = flash();
    let plain = thunder(base.clone(), json!({}), 16.0);
    let mut prev = rms_db(&plain);
    for r in [0.3f32, 0.6, 1.0] {
        let x = thunder(base.clone(), json!({"rumble": r}), 16.0);
        let level = rms_db(&x);
        let sp = peaks(&x, SR, 1, 60.0)[0].0;
        let (lo, hi) = (
            band_db(&x, SR, 25.0, 90.0),
            band_db(&x, SR, 2000.0, 20000.0),
        );
        eprintln!(
            "rumble {r}: rms {level:.1} dB, peak bin {sp:.0} Hz, 25-90 Hz {lo:.1}, >2k {hi:.1}"
        );
        assert!(level > prev, "louder with more rumble: {level} vs {prev}");
        prev = level;
        assert!(lo > hi + 20.0, "low band dominates");
        // The spectral centroid, from the power spectrum.
        let s = spectrum(&x, SR);
        let (num, den) = s.iter().fold((0.0f64, 0.0f64), |(n, d), (hz, db)| {
            let p = 10f64.powf(f64::from(*db) / 10.0);
            (n + f64::from(*hz) * p, d + p)
        });
        let c = num / den;
        eprintln!("  centroid {c:.1} Hz");
        assert!(c < 100.0, "centroid {c}");
    }
}

#[test]
fn rumble_is_seeded_and_finite() {
    let a = thunder(flash(), json!({"rumble": 0.6}), 16.0);
    let b = thunder(flash(), json!({"rumble": 0.6}), 16.0);
    assert_eq!(fnv(&a), fnv(&b));
    let c = thunder(
        json!({"distance": 1500, "seed": 4}),
        json!({"rumble": 0.6}),
        16.0,
    );
    assert_ne!(fnv(&a), fnv(&c));
    // Before the sound arrives nothing is added.
    assert!(rms_db(&a[..SR as usize * 4]) < -80.0);
    assert_bit_identical(
        vec![json!({"type": "thunder", "name": "t", "distance": 300, "rumble": 0.8, "seed": 2})],
        4.0,
    );
    for extra in [
        json!({"rumble": 1, "distance": 0}),
        json!({"rumble": 1, "distance": 30000, "exposure": 0}),
        json!({"rumble": 1, "length": 3, "brightness": 1}),
        json!({"rumble": 1, "length": 0.5, "brightness": 0, "amp": 4}),
        json!({"rumble": 1, "at": 50}),
        json!({"rumble": 0.01, "celsius": -60, "humidity": 0}),
    ] {
        let x = thunder(json!({}), extra.clone(), 3.0);
        assert_finite(&x, &extra.to_string());
    }
}

#[test]
fn rumble_out_of_range_is_an_error() {
    for r in [json!(-0.1), json!(1.5)] {
        let d = {
            let mut d = sfx_graph::SoundDoc::named("t");
            d.nodes = vec![
                json!({"type": "thunder", "name": "t", "rumble": r}),
                json!({"type": "output", "name": "o", "input": "t"}),
            ];
            d.duration = Some(1.0);
            d
        };
        let e = d.render(&sfx_graph::RenderOpts::default()).err().unwrap();
        assert!(e.to_string().contains("rumble"), "{e}");
    }
}
