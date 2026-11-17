//! M7: `modal` shapes (plate bar tube membrane string shell), the round-0
//! fixes (the lowest mode survives a big plate, a unit impulse peaks near
//! 0 dBFS, the size hint comes from the mode table) and `shatter`.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::{Buf, RenderOpts, SoundDoc};

/// An impulse at 10 ms into a `modal` named `m` (so the output stage's 2 ms
/// fade-in does not touch the hit).
fn modal(extra: Value) -> Vec<Value> {
    let mut m = json!({"type": "modal", "name": "m", "excite": "hit"});
    m.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![json!({"type": "impulse", "name": "hit", "at": [0.01]}), m]
}

fn render_error(nodes: Vec<Value>, duration: f32) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "m"}));
    d.duration = Some(duration);
    d.render(&RenderOpts::default()).unwrap_err()
}

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

fn peak_db(x: &[f32]) -> f32 {
    20.0 * x
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()))
        .max(1e-9)
        .log10()
}

/// The size range the error hint gives for a material and shape.
fn hint(material: &str, shape: &str) -> (f32, f32, String) {
    let e = render_error(
        modal(json!({"material": material, "shape": shape, "size": 0.000_001})),
        0.2,
    );
    let tail = e.split("try ").nth(1).unwrap_or_else(|| panic!("{e}"));
    let mut it = tail.split_whitespace();
    let lo: f32 = it.next().unwrap().parse().unwrap();
    assert_eq!(it.next(), Some("to"));
    let hi: f32 = it.next().unwrap().parse().unwrap();
    (lo, hi, e)
}

/// Mode frequencies (Hz, ascending): the spectral peaks within 50 dB of the
/// strongest in `duration` seconds of the node's own samples from the hit (a
/// unit impulse struck into it), one Hann window over them: short for modes
/// that die fast, long for low ones.
fn modes_hz(extra: Value, duration: f32, count: usize) -> Vec<f32> {
    let x = node_audio(modal(extra), "m", 0.01 + duration);
    // From the hit (10 ms in): the Hann window would otherwise be zero there.
    let mut hz: Vec<f32> = peaks(&x[480..], SR, count, 50.0)
        .iter()
        .map(|p| p.0)
        .collect();
    hz.sort_by(f32::total_cmp);
    hz
}

const MATERIALS: [&str; 10] = [
    "stone", "wood", "metal", "glass", "ice", "earth", "granular", "plant", "soft", "snow",
];
const SHAPES: [&str; 6] = ["plate", "bar", "tube", "membrane", "string", "shell"];

#[test]
fn a_unit_impulse_peaks_within_6_db_of_full_scale() {
    let mut worst = 0.0f32;
    let mut lowest = 0.0f32;
    for m in MATERIALS {
        for s in SHAPES {
            let (lo, hi, _) = hint(m, s);
            for size in [lo, (lo * hi).sqrt(), hi] {
                let x = node_audio(
                    modal(json!({"material": m, "shape": s, "size": size})),
                    "m",
                    1.0,
                );
                assert_finite(&x, "m");
                let p = peak_db(&x);
                worst = worst.max(p.abs());
                lowest = lowest.min(p);
                assert!(p.abs() < 6.0, "{m} {s} size {size}: peak {p:.2} dB");
            }
        }
    }
    println!(
        "unit impulse peak, 10 materials x 6 shapes x 3 sizes: worst {worst:.2} dB from 0 dBFS (lowest {lowest:.2})"
    );
}

#[test]
fn the_lowest_mode_falls_with_size_down_to_40_hz_in_wood() {
    // The round-0 sizes: 0.5 gave 425 Hz, 0.8 gave 408 (not 166), 1.4 gave 537
    // (not 54).
    let mut last = f32::MAX;
    for size in [0.5, 0.8, 1.1, 1.4, 1.6] {
        let hz = modes_hz(json!({"material": "wood", "size": size}), 1.5, 32);
        println!("wood {size} m: lowest spectral peak {:.1} Hz", hz[0]);
        assert!(hz[0] < last, "size {size}: {} after {last}", hz[0]);
        last = hz[0];
    }
    assert!(last < 60.0, "{last}");
}

#[test]
fn the_size_hint_names_a_range_that_renders_and_comes_from_the_table() {
    for m in MATERIALS {
        for s in SHAPES {
            let (lo, hi, e) = hint(m, s);
            assert!(lo < hi && lo > 0.0, "{m} {s}: {e}");
            for size in [lo, hi] {
                let x = node_audio(
                    modal(json!({"material": m, "shape": s, "size": size})),
                    "m",
                    0.3,
                );
                assert!(x.iter().any(|v| v.abs() > 0.01), "{m} {s} size {size}");
            }
            // Just outside the small end the same error comes back.
            let e2 = render_error(
                modal(json!({"material": m, "shape": s, "size": lo * 0.4})),
                0.2,
            );
            assert!(e2.contains("try"), "{m} {s} at {}: {e2}", lo * 0.4);
        }
    }
    let (lo, hi, e) = hint("metal", "plate");
    println!("metal plate hint: {lo} to {hi} m; message: {e}");
    let (lo2, hi2, _) = hint("wood", "plate");
    println!("wood plate hint: {lo2} to {hi2} m");
    assert!(e.contains("metal"));
    assert!(
        lo > 0.1 && hi > 1.0,
        "the old \"0.05 to 5 m\" for all is gone"
    );
}

/// First four measured ratios against the table, per shape.
fn check_ratios(shape: &str, extra: Value, table: [f32; 4], tol: f32) {
    // Steel's constants with a tenth of its loss, so every mode of the table
    // is still ringing at the end of a 1.5 s window and shows as a clean line.
    let steel = json!({"young": 200e9, "density": 7800, "poisson": 0.3, "loss": 0.0004,
        "thickness": 0.05});
    let mut e = json!({"material": steel, "shape": shape});
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let hz = modes_hz(e, 1.5, 40);
    let f1 = hz[0];
    let mut line = format!("{shape}: f1 {f1:.1} Hz, ratios");
    let mut worst = 0.0f32;
    for r in table {
        let want = f1 * r;
        let got = hz
            .iter()
            .copied()
            .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()))
            .unwrap();
        let err = (got / want - 1.0).abs();
        worst = worst.max(err);
        line += &format!(" {:.3} (table {r:.3})", got / f1);
    }
    println!("{line}; worst error {:.2} %", worst * 100.0);
    assert!(worst < tol, "{shape}: {line}");
}

#[test]
fn bar_ratios_match_the_free_bar_table() {
    check_ratios(
        "bar",
        json!({"size": 0.5, "strike_position": 0.3}),
        [1.0, 2.757, 5.404, 8.933],
        0.02,
    );
}

#[test]
fn membrane_ratios_match_the_ideal_membrane_table() {
    check_ratios(
        "membrane",
        json!({"size": 0.5, "strike_position": 0.5}),
        [1.0, 1.593, 2.136, 2.296],
        0.02,
    );
}

#[test]
fn string_is_harmonic_and_stiffness_stretches_it() {
    check_ratios(
        "string",
        json!({"size": 0.5, "strike_position": 0.17}),
        [1.0, 2.0, 3.0, 4.0],
        0.02,
    );
    // B = 0.01: f_n = n f1 sqrt(1 + B n^2) -> 1, 2.0396, 3.1 ..., ratios.
    let b = 0.01f32;
    let table = [1.0, 2.0, 3.0, 4.0].map(|n: f32| n * ((1.0 + b * n * n) / (1.0 + b)).sqrt());
    check_ratios(
        "string",
        json!({"size": 0.5, "strike_position": 0.17, "stiffness": b}),
        table,
        0.02,
    );
}

#[test]
fn tube_ratios_match_the_thin_ring_table() {
    check_ratios(
        "tube",
        json!({"size": 0.5, "strike_position": 0.1}),
        [1.0, 2.828, 5.423, 8.771],
        0.02,
    );
}

#[test]
fn shell_ratios_match_the_bell_series() {
    // hum, prime, tierce, quint relative to the hum: 1, 2, 2.4, 3.
    check_ratios("shell", json!({"size": 0.4}), [1.0, 2.0, 2.4, 3.0], 0.02);
}

#[test]
fn the_default_plate_is_the_plate() {
    // FNV-1a of the f32 bits of the modal node (impulse at 0, 1 s, seed 2),
    // recorded after the round-0 fixes, which changed them on purpose (the
    // level, and which modes a crowded table keeps). Before the fixes the same
    // renders hashed c83f8e64f34833e7 (wood), b1cf948b0feb0de5 (metal) and
    // 25e7e9e77b87ddcc (glass). The explicit shape and the missing one must
    // agree bit for bit.
    let render = |extra: Value| {
        let mut e = json!({"seed": 2});
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut nodes = modal(e);
        nodes[0] = json!({"type": "impulse", "name": "hit", "at": [0.0]});
        node_audio(nodes, "m", 1.0)
    };
    for (m, size, pos) in [
        ("wood", 0.5, 0.3),
        ("metal", 0.25, 0.5),
        ("glass", 1.4, 0.3),
    ] {
        let base = json!({"material": m, "size": size, "strike_position": pos});
        let mut with = base.clone();
        with["shape"] = json!("plate");
        let a = render(base);
        let b = render(with);
        println!("{m} {size} {pos}: hash {:016x}", fnv(&a));
        assert_eq!(a, b, "{m}: shape plate differs from the default");
    }
    let hashes = [
        ("wood", 0.5, 0.3, HASH_WOOD),
        ("metal", 0.25, 0.5, HASH_METAL),
        ("glass", 1.4, 0.3, HASH_GLASS),
    ];
    for (m, size, pos, want) in hashes {
        let x = render(json!({"material": m, "size": size, "strike_position": pos}));
        assert_eq!(fnv(&x), want, "{m} {size} {pos}");
    }
}

const HASH_WOOD: u64 = 0x51c2_6165_48ef_09c9;
const HASH_METAL: u64 = 0x6923_f210_cb19_dba8;
const HASH_GLASS: u64 = 0xdffb_e591_3f62_39bb;

#[test]
fn every_shape_is_finite_deterministic_and_bounded_at_the_extremes() {
    for s in SHAPES {
        for (size, modes, pos) in [
            (0.05, 1, 0.0),
            (0.3, 256, 1.0),
            (3.0, 7, 0.5),
            (400.0, 48, 0.3),
        ] {
            let mut d = SoundDoc::named("t");
            d.nodes = modal(json!({"material": "metal", "shape": s, "size": size,
                "modes": modes, "strike_position": pos}));
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "m"}));
            d.duration = Some(0.5);
            match d.render(&RenderOpts::default()) {
                Ok(_) => {
                    let x = node_audio(
                        modal(json!({"material": "metal", "shape": s, "size": size,
                            "modes": modes, "strike_position": pos})),
                        "m",
                        0.5,
                    );
                    assert_finite(&x, s);
                    assert!(peak_db(&x) < 6.0, "{s} {size}: {}", peak_db(&x));
                }
                Err(e) => assert!(e.contains("try"), "{s} {size}: {e}"),
            }
        }
    }
    assert_bit_identical(
        modal(json!({"material": "glass", "shape": "membrane", "size": 0.3, "seed": 5})),
        0.5,
    );
    // The mistuning is the seed's.
    let a = node_audio(
        modal(json!({"material": "wood", "shape": "bar", "size": 0.4})),
        "m",
        0.3,
    );
    let b = node_audio(
        modal(json!({"material": "wood", "shape": "bar", "size": 0.4, "seed": 3})),
        "m",
        0.3,
    );
    assert_ne!(a, b);
}

#[test]
fn modal_errors_say_what_to_do() {
    let e = render_error(
        modal(json!({"material": "wood", "size": 0.5, "stiffness": 0.1})),
        0.2,
    );
    println!("{e}");
    assert!(e.contains("stiffness") && e.contains("string"), "{e}");
    let e = render_error(
        modal(json!({"material": "wood", "shape": "string", "size": 0.5, "stiffness": 2})),
        0.2,
    );
    assert!(e.contains("stiffness") && e.contains("0..1"), "{e}");
    let mut d = SoundDoc::named("t");
    d.nodes = modal(json!({"material": "wood", "shape": "cube", "size": 0.5}));
    d.duration = Some(0.2);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("{e}");
    assert!(e.contains("cube") && e.contains("membrane"), "{e}");
}

// ------------------------------------------------------------- shatter

fn shatter_events(extra: Value, duration: f32) -> Vec<(f64, f32)> {
    let mut s = json!({"type": "shatter", "name": "sh", "pieces": 10});
    s.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        s,
        json!({"type": "osc", "name": "o", "freq": 100}),
        json!({"type": "output", "name": "out", "input": "o"}),
    ];
    d.duration = Some(duration);
    let r = d.render(&RenderOpts::default()).unwrap();
    match &*r.eval.buf("sh").unwrap() {
        Buf::Events(e) => {
            for (i, ev) in e.iter().enumerate() {
                assert_eq!(ev.id as usize, i);
            }
            assert!(e.windows(2).all(|w| w[0].t <= w[1].t));
            e.iter().map(|e| (e.t, e.value)).collect()
        }
        other => panic!("{}", other.kind()),
    }
}

/// Least-squares slope of log N(>v) against log v over `v` in `lo..hi`.
fn ccdf_slope(values: &[f32], lo: f32, hi: f32) -> f32 {
    let mut v: Vec<f32> = values.to_vec();
    v.sort_by(|a, b| b.total_cmp(a));
    let pts: Vec<(f32, f32)> = v
        .iter()
        .enumerate()
        .filter(|(_, x)| **x >= lo && **x <= hi)
        .map(|(rank, x)| (x.ln(), ((rank + 1) as f32).ln()))
        .collect();
    let n = pts.len() as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let num: f32 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let den: f32 = pts.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    num / den
}

#[test]
fn shatter_makes_pieces_events_with_a_power_law_of_sizes() {
    for (exponent, seed) in [(2.0f32, 1u64), (3.0, 2), (1.2, 3)] {
        let ev = shatter_events(
            json!({"pieces": 5000, "spread": 0.5, "exponent": exponent, "seed": seed}),
            2.0,
        );
        assert_eq!(ev.len(), 5000);
        let values: Vec<f32> = ev.iter().map(|e| e.1).collect();
        // The cap at 1 / 100 and the crack at 1 bend the ends: fit between.
        let slope = ccdf_slope(&values, 0.02, 0.5);
        println!(
            "exponent {exponent}: {} events, log-log slope of N(>size) {slope:.3} (want {:.3}), first event t {:.3} value {}",
            ev.len(),
            -exponent,
            ev[0].0,
            ev[0].1
        );
        assert!(
            (slope / -exponent - 1.0).abs() < 0.2,
            "exponent {exponent}: slope {slope}"
        );
    }
}

#[test]
fn shatter_times_crowd_early_and_start_with_the_crack() {
    let ev = shatter_events(
        json!({"pieces": 3000, "spread": 0.6, "at": 0.1, "seed": 4}),
        2.0,
    );
    let early = ev.iter().filter(|e| e.0 < 0.1 + 0.2).count() as f32 / ev.len() as f32;
    let late = ev.iter().filter(|e| e.0 >= 0.1 + 0.4).count() as f32 / ev.len() as f32;
    println!(
        "first event t {:.3} value {:.2}; share in the first third of the spread {early:.3}, in the last third {late:.3}",
        ev[0].0, ev[0].1
    );
    assert!((ev[0].0 - 0.1).abs() < 1e-6 && ev[0].1 == 1.0);
    assert!(early > 0.7 && late < 0.1, "{early} {late}");
    assert!(ev.iter().all(|e| e.0 >= 0.1 && e.0 <= 0.7));
    // Same seed, same events; another seed, other sizes.
    let a = shatter_events(json!({"pieces": 50, "seed": 4}), 1.0);
    assert_eq!(a, shatter_events(json!({"pieces": 50, "seed": 4}), 1.0));
    assert_ne!(a, shatter_events(json!({"pieces": 50, "seed": 5}), 1.0));
    assert_eq!(shatter_events(json!({"pieces": 1}), 1.0), vec![(0.0, 1.0)]);
}

#[test]
fn shatter_errors_say_what_to_do() {
    for (extra, want) in [
        (json!({"pieces": 0}), "pieces"),
        (json!({"pieces": 1_000_000}), "200000"),
        (json!({"pieces": 5, "exponent": 9}), "0.5..6"),
        (json!({"pieces": 5, "spread": -1}), "spread"),
    ] {
        let mut s = json!({"type": "shatter", "name": "sh"});
        s.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut d = SoundDoc::named("t");
        d.nodes = vec![
            s,
            json!({"type": "osc", "name": "o", "freq": 100}),
            json!({"type": "output", "name": "out", "input": "o"}),
        ];
        d.duration = Some(1.0);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        println!("{e}");
        assert!(e.contains(want), "{want}: {e}");
    }
}

#[test]
fn shatter_drives_voices_of_small_plates() {
    let nodes = vec![
        json!({"type": "shatter", "name": "sh", "pieces": 60, "spread": 0.5, "seed": 2}),
        json!({"type": "voices", "name": "v", "events": "sh", "length": 0.3, "max": 100,
            "map": {"size": "0.08 + (1 - $value) * 0.3"},
            "patch": [
                {"type": "impulse", "name": "tap", "at": [0.0]},
                {"type": "gain", "name": "hit", "input": "tap", "gain": "$value"},
                {"type": "modal", "name": "piece", "excite": "hit", "material": "glass",
                 "size": "$size", "modes": 12}]}),
    ];
    let x = node_audio(&nodes, "v", 1.2);
    assert_finite(&x, "v");
    println!(
        "shatter -> voices -> glass modal: peak {:.1} dB",
        peak_db(&x)
    );
    assert!(peak_db(&x) > -20.0);
    assert_bit_identical(&nodes, 1.2);
}
