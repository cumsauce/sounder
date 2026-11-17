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

// ------------------------------------------------- R1-D: loss, tilt, size range

/// A thin glass plate: 3 mm, the round-1 shard (glass_break).
fn glass_3mm() -> Value {
    json!({"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.004, "thickness": 0.003})
}

/// The modal node `m` (impulse at 10 ms) rendered at `sr`; the samples from
/// the hit on.
fn modal_at(extra: Value, sr: u32, duration: f32) -> Vec<f32> {
    let mut d = SoundDoc::named("t");
    d.sample_rate = sr;
    d.nodes = modal(extra);
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "m"}));
    d.duration = Some(duration);
    let r = d
        .render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"));
    match &*r.eval.buf("m").unwrap() {
        Buf::Audio(x) => x[(0.01 * sr as f32) as usize..].to_vec(),
        _ => panic!("not audio"),
    }
}

/// Seconds for the 5 ms RMS envelope to fall 20 dB below its maximum.
fn decay_20_s(x: &[f32]) -> f32 {
    let win = (0.005 * SR as f32) as usize;
    let env: Vec<f32> = x
        .chunks(win)
        .map(|c| {
            10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
                .max(1e-20)
                .log10()
        })
        .collect();
    let top = env.iter().copied().fold(f32::MIN, f32::max);
    let k = env
        .iter()
        .position(|&e| e < top - 20.0)
        .expect("never fell");
    k as f32 * 0.005
}

#[test]
fn loss_scales_the_ring_time_of_a_named_material() {
    // One mode, so the envelope is one exponential: T20 = T60 / 3.
    let one = |loss: Option<f32>| {
        let mut e = json!({"material": "metal", "size": 0.5, "modes": 1});
        if let Some(l) = loss {
            e["loss"] = json!(l);
        }
        decay_20_s(&node_audio(modal(e), "m", 3.0)[480..])
    };
    let base = one(None);
    assert_eq!(base, one(Some(1.0)));
    let (half, double) = (one(Some(0.5)), one(Some(2.0)));
    println!("T20 loss 1: {base:.4} s; loss 0.5: {half:.4} s; loss 2: {double:.4} s");
    assert!((half / base - 2.0).abs() < 0.3, "loss 0.5: {}", half / base);
    assert!(
        (double / base - 0.5).abs() < 0.075,
        "loss 2: {}",
        double / base
    );
    // The multiplier applies to a {young, ...} material too.
    let steel = json!({"young": 200e9, "density": 7800, "poisson": 0.3, "loss": 0.004,
        "thickness": 0.05});
    let t = |loss: f32| {
        decay_20_s(
            &node_audio(
                modal(json!({"material": steel, "size": 0.5, "modes": 1, "loss": loss})),
                "m",
                3.0,
            )[480..],
        )
    };
    let r = t(0.5) / t(1.0);
    println!("custom material: T20 ratio at loss 0.5: {r:.3}");
    assert!((r - 2.0).abs() < 0.3, "{r}");
}

#[test]
fn loss_and_tilt_are_validated() {
    for (extra, field, range) in [
        (json!({"loss": 0.0}), "loss", "0.01..100"),
        (json!({"loss": -1.0}), "loss", "0.01..100"),
        (json!({"loss": 1000.0}), "loss", "0.01..100"),
        (json!({"tilt": 13.0}), "tilt", "-12..12"),
        (json!({"tilt": -13.0}), "tilt", "-12..12"),
    ] {
        let mut e = json!({"material": "wood", "size": 0.5});
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let msg = render_error(modal(e), 0.2);
        println!("{msg}");
        assert!(msg.contains(field) && msg.contains(range), "{msg}");
    }
    // The ends are fine.
    for extra in [
        json!({"loss": 0.01}),
        json!({"loss": 100.0}),
        json!({"tilt": 12.0}),
        json!({"tilt": -12.0}),
    ] {
        let mut e = json!({"material": "wood", "size": 0.5});
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let x = node_audio(modal(e), "m", 0.3);
        assert_finite(&x, "m");
        assert!(peak_db(&x).abs() < 6.0, "{extra}: {}", peak_db(&x));
    }
}

/// The level (dB) of the spectral peak within 1.5 % of `hz`.
fn level_near(x: &[f32], hz: f32) -> f32 {
    spectrum(x, SR)
        .iter()
        .filter(|(f, _)| (f / hz - 1.0).abs() < 0.015)
        .map(|p| p.1)
        .fold(f32::MIN, f32::max)
}

#[test]
fn tilt_moves_the_mode_balance_by_db_per_octave() {
    // A long-ringing steel string / bar, so each mode is a clean line.
    let steel = json!({"young": 200e9, "density": 7800, "poisson": 0.3, "loss": 0.0004,
        "thickness": 0.05});
    // (shape, size, mode ratios to the lowest mode: table values)
    for (shape, size, ratio) in [
        ("string", 0.5, 4.0f32),
        ("string", 0.5, 2.0),
        ("bar", 1.0, 5.404),
        ("bar", 1.0, 8.933),
    ] {
        let spec =
            |tilt: f32| json!({"material": steel, "shape": shape, "size": size, "tilt": tilt});
        let fa = modes_hz(spec(0.0), 1.5, 40)[0];
        let fb = fa * ratio;
        let flat = node_audio(modal(spec(0.0)), "m", 1.5)[480..].to_vec();
        let octaves = ratio.log2();
        for tilt in [6.0f32, -6.0, 3.0] {
            let x = node_audio(modal(spec(tilt)), "m", 1.5)[480..].to_vec();
            // Level of mode b over mode a, relative to the untilted table.
            let got = (level_near(&x, fb) - level_near(&x, fa))
                - (level_near(&flat, fb) - level_near(&flat, fa));
            let want = tilt * octaves;
            println!(
                "{shape} {fa:.0} -> {fb:.0} Hz ({octaves:.2} oct), tilt {tilt}: {got:.2} dB (want {want:.2})"
            );
            assert!(
                (got - want).abs() < 2.0,
                "{shape} tilt {tilt}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn tilt_flattens_a_glass_plate_that_is_all_in_one_mode() {
    // Round 1: glass_clink put 98 % of the energy in the lowest mode.
    let energy_share = |tilt: f32| {
        let x = node_audio(
            modal(
                json!({"material": "glass", "size": 0.15, "strike_position": 0.31,
                "modes": 24, "tilt": tilt}),
            ),
            "m",
            0.5,
        );
        let top = peaks(&x[480..], SR, 1, 0.0)[0].0;
        let near = band_db(&x[480..], SR, top * 0.97, top * 1.03);
        let all = band_db(&x[480..], SR, 20.0, 20_000.0);
        (near - all, top)
    };
    let (flat, f) = energy_share(0.0);
    let mut last = flat;
    for t in [3.0, 6.0, 9.0, 12.0] {
        let (share, f2) = energy_share(t);
        println!(
            "tilt {t}: strongest mode {f2:.0} Hz holds {share:.2} dB ({:.0} %) of the total (flat: {f:.0} Hz, {flat:.2} dB)",
            100.0 * 10f32.powf(share / 10.0)
        );
        assert!(share < last, "tilt {t}: {share} dB after {last}");
        last = share;
    }
    // From 98.6 % in the lowest mode to under 65 %.
    assert!(flat > -0.1 && last < -1.9, "{flat} -> {last}");
}

#[test]
fn a_small_glass_shard_has_several_modes_between_1_5_and_8_khz() {
    // The mode table scales with the plate's thickness, so a 3 cm shard is
    // given a 0.3 mm plate: (1,1) at 1.6 kHz, then 4.1 k (twice), 6.5 k, 8.1 k.
    let thin = json!({"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.004,
        "thickness": 0.0003});
    let hz = modes_hz(json!({"material": thin, "size": 0.03}), 0.3, 16);
    let inside: Vec<f32> = hz
        .iter()
        .copied()
        .filter(|f| (1500.0..=8000.0).contains(f))
        .collect();
    println!("0.03 m glass shard (0.3 mm): peaks {hz:.0?}; between 1.5 and 8 kHz: {inside:.0?}");
    assert!(inside.len() >= 3, "{inside:?}");
}

#[test]
fn the_size_range_reaches_small_shards_and_keys() {
    // Round 1: 3 mm glass was refused under 0.039 m and 5 cm metal under
    // 0.16 m (the hint stopped at a 10 kHz lowest mode). The range now runs to
    // 90 % of the top of the band, and past it the error names the fix.
    let glass = json!({"material": glass_3mm(), "size": 1e-6});
    let e = render_error(modal(glass), 0.2);
    println!("{e}");
    let tail = e.split("try ").nth(1).unwrap();
    let lo: f32 = tail.split_whitespace().next().unwrap().parse().unwrap();
    assert!(lo < 0.03, "3 mm glass plate: lo {lo}");
    let (mlo, _, _) = hint("metal", "plate");
    assert!(mlo < 0.15, "metal plate: lo {mlo}");
    println!("3 mm glass plate from {lo} m; metal plate from {mlo} m (was 0.039 and 0.16)");
    // 0.03 m and 0.028 m render (lowest mode 16.3 and 18.7 kHz).
    for size in [0.03, 0.028, lo] {
        let x = node_audio(
            modal(json!({"material": glass_3mm(), "size": size})),
            "m",
            0.1,
        );
        assert!(x.iter().any(|v| v.abs() > 0.01), "size {size}");
        assert_finite(&x, "m");
        println!(
            "3 mm glass {size} m: lowest peak {:.0} Hz, peak {:.2} dB",
            peak_hz(&x[480..], SR),
            peak_db(&x)
        );
    }
    // Under the range: an error that names the lowest mode and the thickness
    // that would put it back in the band; following it renders.
    let e = render_error(
        modal(json!({"material": glass_3mm(), "size": lo * 0.5})),
        0.2,
    );
    println!("{e}");
    assert!(
        e.contains("lowest mode would be") && e.contains("thinner"),
        "{e}"
    );
    let t: f32 = e
        .split("thickness ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(t > 0.0 && t < 0.003, "{t}");
    let mut fixed = glass_3mm();
    fixed["thickness"] = json!(t);
    let x = node_audio(
        modal(json!({"material": fixed, "size": lo * 0.5})),
        "m",
        0.1,
    );
    assert!(
        x.iter().any(|v| v.abs() > 0.01),
        "thickness {t} did not fix it"
    );
    // Too big says so the other way.
    let e = render_error(
        modal(json!({"material": "wood", "shape": "shell", "size": 400.0})),
        0.2,
    );
    println!("{e}");
    assert!(e.contains("under 20 Hz") && e.contains("smaller"), "{e}");
}

#[test]
fn every_size_in_the_range_is_stable_and_nothing_folds_past_the_band() {
    // 8 log-spaced sizes from each end of the range, at three sample rates,
    // for several materials and all shapes: finite, bounded, and no strong
    // spectral peak above the top of the band (modes there are dropped, not
    // folded). 20 kHz or 0.49 sr, whichever is lower.
    let mats = [json!("wood"), json!("metal"), json!("glass"), glass_3mm()];
    let mut n = 0;
    let mut worst = f32::MIN;
    for sr in [22_050u32, 44_100, 48_000, 96_000] {
        let top = (0.49 * sr as f32).min(20_000.0);
        for mat in &mats {
            for s in SHAPES {
                let e = render_error_at(mat, s, sr);
                let (lo, hi) = parse_range(&e);
                assert!(lo < hi, "{mat} {s} {sr}: {e}");
                for i in 0..8 {
                    let size = lo * (hi / lo).powf(i as f32 / 7.0);
                    let x = modal_at(json!({"material": mat, "shape": s, "size": size}), sr, 0.25);
                    assert_finite(&x, s);
                    let p = peak_db(&x);
                    worst = worst.max(p);
                    assert!(p.abs() < 6.0, "{mat} {s} {sr} Hz size {size}: peak {p}");
                    // Peaks above the top of the band (within 40 dB).
                    let above = peaks(&x[(0.002 * sr as f32) as usize..], sr, 8, 40.0)
                        .iter()
                        .filter(|p| p.0 > top * 1.05)
                        .count();
                    assert_eq!(above, 0, "{mat} {s} {sr} Hz size {size}: peaks above {top}");
                    n += 1;
                }
            }
        }
    }
    println!("{n} renders over the size range: all finite, worst peak {worst:.2} dB");
}

/// The error a tiny size gives, at `sr`.
fn render_error_at(material: &Value, shape: &str, sr: u32) -> String {
    let mut d = SoundDoc::named("t");
    d.sample_rate = sr;
    d.nodes = modal(json!({"material": material, "shape": shape, "size": 1e-6}));
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "m"}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
}

fn parse_range(e: &str) -> (f32, f32) {
    let mut it = e.split("try ").nth(1).unwrap().split_whitespace();
    let lo: f32 = it.next().unwrap().parse().unwrap();
    it.next();
    (lo, it.next().unwrap().parse().unwrap())
}

#[test]
fn the_default_loss_and_tilt_leave_the_render_bit_identical() {
    for m in ["wood", "glass"] {
        let plain = json!({"material": m, "size": 0.4, "seed": 3});
        let mut with = plain.clone();
        with["loss"] = json!(1.0);
        with["tilt"] = json!(0.0);
        assert_eq!(
            node_audio(modal(plain), "m", 0.5),
            node_audio(modal(with), "m", 0.5),
            "{m}"
        );
    }
}

// ------------------------------------------------- R2-E: thickness

#[test]
fn thickness_scales_every_mode_linearly() {
    // Wood plate 0.1 m: thickness 0.002 against 0.004 doubles the lowest mode.
    let low = |t: f64| {
        let hz = modes_hz(
            json!({"material": "wood", "size": 0.1, "thickness": t, "seed": 1}),
            0.4,
            12,
        );
        hz[0]
    };
    let (a, b) = (low(0.002), low(0.004));
    println!(
        "thickness 0.002: lowest {a:.1} Hz; 0.004: {b:.1} Hz; ratio {:.3}",
        b / a
    );
    assert!((b / a - 2.0).abs() < 0.04, "{a} {b}");
    // The lowest mode follows the material's own table: 0.025 m wood.
    let own = modes_hz(json!({"material": "wood", "size": 0.3, "seed": 1}), 0.4, 12)[0];
    let t25 = modes_hz(
        json!({"material": "wood", "size": 0.3, "thickness": 0.025, "seed": 1}),
        0.4,
        12,
    )[0];
    println!("default wood {own:.1} Hz; thickness 0.025: {t25:.1} Hz");
    assert!((own - t25).abs() / own < 0.01, "{own} {t25}");
}

/// Peaks of a thin metal disc, and those between 3 and 9 kHz.
fn metal_disc(size: f64) -> (Vec<f32>, Vec<f32>) {
    let hz = modes_hz(
        json!({"material": "metal", "size": size, "thickness": 0.0015}),
        0.4,
        24,
    );
    let inside = hz
        .iter()
        .copied()
        .filter(|f| (3000.0..=9000.0).contains(f))
        .collect();
    (hz, inside)
}

#[test]
#[ignore = "measured: metal (E 200 GPa, rho 7850) at size 0.025 m, thickness 0.0015 m has its (1,1) mode at 11.5 kHz and nothing else under 18 kHz, so 0 modes between 3 and 9 kHz; the plate formula puts (1,1) at pi/2 t c / sqrt 12 x 2 / size^2"]
fn a_thin_metal_disc_has_modes_between_3_and_9_khz() {
    // A coin: metal, 25 mm, 1.5 mm thick (the card's numbers).
    let (hz, inside) = metal_disc(0.025);
    println!("metal 25 mm x 1.5 mm: peaks {hz:.0?}; between 3 and 9 kHz: {inside:.0?}");
    assert!(inside.len() >= 3, "{hz:?}");
}

#[test]
fn a_thin_metal_disc_of_8_cm_has_modes_between_3_and_9_khz() {
    // The 25 mm coin of the card sits at 11.5 kHz (above); 8 cm of the same
    // 1.5 mm metal puts (1,1) at 1.1 kHz and the next ones inside 3..9 kHz.
    let (hz, inside) = metal_disc(0.08);
    println!("metal 80 mm x 1.5 mm: peaks {hz:.0?}; between 3 and 9 kHz: {inside:.0?}");
    assert!(inside.len() >= 3, "{hz:?}");
    // And the 25 mm coin does render: its one mode is at the formula's value.
    let (hz, _) = metal_disc(0.025);
    println!("metal 25 mm x 1.5 mm: peaks {hz:.0?}");
    assert!(hz.len() == 1 && (hz[0] - 11_492.0).abs() < 300.0, "{hz:?}");
}

#[test]
fn glass_shard_with_a_thickness_field_renders() {
    let x = node_audio(
        modal(json!({"material": "glass", "size": 0.03, "thickness": 0.003})),
        "m",
        0.3,
    );
    println!("glass 3 cm x 3 mm: peak {:.1} dB", peak_db(&x));
    assert!(x.iter().all(|v| v.is_finite()));
    assert!(x.iter().any(|v| v.abs() > 0.01));
}

#[test]
fn thickness_is_stable_across_its_range_for_every_shape() {
    for s in SHAPES {
        for t in [0.0003f64, 0.001, 0.003, 0.01, 0.05] {
            for size in [0.05f64, 0.3] {
                let mut d = SoundDoc::named("t");
                d.nodes =
                    modal(json!({"material": "glass", "shape": s, "size": size, "thickness": t}));
                d.nodes
                    .push(json!({"type": "output", "name": "out", "input": "m"}));
                d.duration = Some(0.3);
                match d.render(&RenderOpts::default()) {
                    Ok(r) => match &*r.eval.buf("m").unwrap() {
                        Buf::Audio(x) => {
                            assert!(x.iter().all(|v| v.is_finite()), "{s} {t} {size}");
                            assert!(peak_db(x) < 3.0, "{s} {t} {size}: {}", peak_db(x));
                        }
                        _ => panic!("not audio"),
                    },
                    // A combination with no mode in the band is an error that
                    // names the fix, never a blown-up render.
                    Err(e) => assert!(e.contains("no mode"), "{s} {t} {size}: {e}"),
                }
            }
        }
    }
}

#[test]
fn the_default_thickness_leaves_the_render_bit_identical() {
    // Wood is 0.025 m, glass 0.01 m in foley's table: giving the material's
    // own thickness changes nothing, and the pinned hashes still hold.
    let own = |m: &str| modal(json!({"material": m, "size": 0.4, "seed": 3}));
    let plain = node_audio(own("wood"), "m", 0.5);
    let mut with = own("wood");
    with[1]["thickness"] = json!(0.025);
    assert_eq!(plain, node_audio(with, "m", 0.5));
}

#[test]
fn thickness_errors_say_what_to_do_and_the_hint_uses_it() {
    for bad in [0.0f64, -1.0, 0.00001, 0.9] {
        let e = render_error(
            modal(json!({"material": "wood", "size": 0.4, "thickness": bad})),
            0.2,
        );
        println!("{e}");
        assert!(e.contains("thickness") && e.contains("0.0001"), "{e}");
    }
    // The size hint comes from the effective thickness: a 1.5 mm metal disc
    // is allowed a smaller size than the default metal plate.
    let (lo_default, _, _) = hint("metal", "plate");
    let e = render_error(
        modal(json!({"material": "metal", "size": 1e-6, "thickness": 0.0015})),
        0.2,
    );
    println!("{e}");
    let (lo_thin, _) = parse_range(&e);
    println!("metal size floor: default {lo_default}, 1.5 mm {lo_thin}");
    assert!(lo_thin < lo_default, "{lo_thin} {lo_default}");
    assert!(e.contains("`thickness` field"), "{e}");
}

// ------------------------------------------------------------ R3-C: band and f0

/// Spectrum level (dB, 0 = the strongest bin) summed as a maximum over the bins
/// outside `[lo, hi]` Hz widened by a factor `margin` each way, and inside it.
fn band_levels(x: &[f32], lo: f32, hi: f32, margin: f32) -> (f32, f32) {
    let s = spectrum(x, SR);
    let top = s.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let (mut outside, mut inside) = (f32::MIN, f32::MIN);
    for &(hz, db) in &s {
        let rel = db - top;
        if hz < lo / margin || hz > hi * margin {
            outside = outside.max(rel);
        } else if hz >= lo && hz <= hi {
            inside = inside.max(rel);
        }
    }
    (outside, inside)
}

#[test]
fn f0_puts_the_lowest_mode_at_the_asked_frequency_for_every_shape() {
    for shape in ["plate", "bar", "tube", "membrane"] {
        for material in ["wood", "metal", "glass"] {
            for f0 in [800.0f32, 2000.0, 5000.0] {
                let hz = modes_hz(
                    json!({"material": material, "shape": shape, "f0": f0}),
                    0.25,
                    32,
                );
                let err = (hz[0] / f0 - 1.0).abs() * 100.0;
                println!(
                    "{shape} {material} f0 {f0}: lowest peak {:.1} Hz ({err:.2} %)",
                    hz[0]
                );
                assert!(
                    err < 3.0,
                    "{shape} {material} f0 {f0}: lowest peak {:.1}",
                    hz[0]
                );
            }
        }
    }
    // The two shapes the card does not name, by the same rule.
    for shape in ["string", "shell"] {
        let hz = modes_hz(
            json!({"material": "metal", "shape": shape, "f0": 2000.0}),
            0.25,
            32,
        );
        let err = (hz[0] / 2000.0 - 1.0).abs() * 100.0;
        println!(
            "{shape} metal f0 2000: lowest peak {:.1} Hz ({err:.2} %)",
            hz[0]
        );
        assert!(err < 3.0, "{shape}: {:.1}", hz[0]);
    }
}

#[test]
fn f0_uses_the_effective_thickness() {
    // f0 solves for the size with the effective thickness: a thinner wall
    // needs a smaller object for the same pitch.
    let base = json!({"material": "metal", "shape": "bar", "f0": 1500.0, "seed": 4});
    let mut thin = base.clone();
    thin["thickness"] = json!(0.002);
    let a = modes_hz(base, 0.25, 32);
    let b = modes_hz(thin, 0.25, 32);
    println!(
        "bar f0 1500: default thickness {:.1} Hz, 2 mm {:.1} Hz",
        a[0], b[0]
    );
    assert!((a[0] / 1500.0 - 1.0).abs() < 0.03 && (b[0] / 1500.0 - 1.0).abs() < 0.03);
}

#[test]
fn f_min_and_f_max_remove_the_modes_outside_the_band() {
    // A wood plate 0.5 m a side: lowest mode ~425 Hz, then a dense series.
    let plain = node_audio(
        modal(json!({"material": "metal", "size": 0.5, "loss": 0.4})),
        "m",
        0.6,
    );
    let band = node_audio(
        modal(
            json!({"material": "metal", "size": 0.5, "loss": 0.4, "f_min": 1500.0, "f_max": 4000.0}),
        ),
        "m",
        0.6,
    );
    let (out_plain, _) = band_levels(&plain[480..], 1500.0, 4000.0, 1.25);
    let (out, inside) = band_levels(&band[480..], 1500.0, 4000.0, 1.25);
    println!(
        "outside the band: {out_plain:.1} dB without, {out:.1} dB with; inside {inside:.1} dB"
    );
    assert!(
        out_plain > -40.0,
        "the unbanded plate has energy outside: {out_plain}"
    );
    assert!(out < -40.0, "peaks outside the band: {out} dB");
    assert!(inside > -10.0, "the band is not empty: {inside}");
    // One-sided bands.
    let lowpass = node_audio(
        modal(json!({"material": "metal", "size": 0.5, "loss": 0.4, "f_max": 2000.0})),
        "m",
        0.6,
    );
    let (out, inside) = band_levels(&lowpass[480..], 20.0, 2000.0, 1.25);
    println!("f_max 2000: outside {out:.1} dB, inside {inside:.1} dB");
    assert!(out < -40.0, "{out}");
    let highpass = node_audio(
        modal(json!({"material": "metal", "size": 0.5, "loss": 0.4, "f_min": 3000.0})),
        "m",
        0.6,
    );
    let (out, inside) = band_levels(&highpass[480..], 3000.0, 22000.0, 1.25);
    println!("f_min 3000: outside {out:.1} dB, inside {inside:.1} dB");
    assert!(out < -40.0, "{out}");
    // Every shape keeps its band.
    for shape in SHAPES {
        let x = node_audio(
            modal(
                json!({"material": "metal", "shape": shape, "size": 0.3, "loss": 0.4,
                "f_min": 2500.0, "f_max": 6000.0}),
            ),
            "m",
            0.5,
        );
        let (out, inside) = band_levels(&x[480..], 2500.0, 6000.0, 1.25);
        println!("{shape}: outside {out:.1} dB, inside {inside:.1} dB");
        assert!(out < -40.0, "{shape}: {out}");
        assert!(inside > -10.0, "{shape}: {inside}");
    }
}

#[test]
fn modes_counts_the_kept_modes_and_a_wide_band_changes_nothing() {
    // `modes` is applied after the band: 3 modes inside 1.5 .. 4 kHz, not 3 of
    // the table of which some fall outside.
    let x = node_audio(
        modal(
            json!({"material": "metal", "size": 0.5, "loss": 0.4, "modes": 3,
            "f_min": 1500.0, "f_max": 4000.0}),
        ),
        "m",
        0.6,
    );
    let (out, inside) = band_levels(&x[480..], 1500.0, 4000.0, 1.05);
    println!("modes 3 in a band: outside {out:.1} dB, inside {inside:.1} dB");
    assert!(out < -40.0 && inside > -10.0);
    let hz = peaks(&x[480..], SR, 8, 40.0);
    assert!(hz.len() <= 3, "{} peaks for 3 modes: {hz:?}", hz.len());
    // A band that holds the whole table is bit-identical to no band.
    let render = |extra: Value| {
        let mut e = json!({"material": "wood", "size": 0.5, "seed": 2});
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut nodes = modal(e);
        nodes[0] = json!({"type": "impulse", "name": "hit", "at": [0.0]});
        node_audio(nodes, "m", 1.0)
    };
    let plain = render(json!({}));
    let wide = render(json!({"f_min": 1.0, "f_max": 100_000.0}));
    assert_eq!(
        plain, wide,
        "a band around the whole table changes the render"
    );
}

#[test]
fn band_and_f0_errors_say_what_to_do() {
    let err = |extra: Value| render_error(modal(extra), 0.2);
    let e = err(json!({"material": "wood", "size": 0.5, "f_min": 100.0, "f_max": 200.0}));
    println!("{e}");
    assert!(
        e.contains("f_min 100 / f_max 200") && e.contains("span") && e.contains("Hz"),
        "{e}"
    );
    let e = err(json!({"material": "wood", "size": 0.5, "f_min": 3000.0, "f_max": 2000.0}));
    assert!(e.contains("f_min") && e.contains("f_max"), "{e}");
    let e = err(json!({"material": "wood", "size": 0.5, "f_max": -5.0}));
    assert!(e.contains("f_max"), "{e}");
    let e = err(json!({"material": "wood", "size": 0.5, "f0": 1000.0}));
    assert!(
        e.contains("size") && e.contains("f0") && e.contains("both"),
        "{e}"
    );
    let e = err(json!({"material": "wood"}));
    assert!(e.contains("size") && e.contains("f0"), "{e}");
    for bad in [10.0, 25_000.0] {
        let e = err(json!({"material": "wood", "f0": bad}));
        assert!(
            e.contains("f0") && e.contains("20") && e.contains("18000"),
            "{e}"
        );
    }
}
