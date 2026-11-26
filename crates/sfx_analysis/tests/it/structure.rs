//! `structure` (a statistically dense struck body) and `rumble` (the
//! pitchless low roll after a heavy event). Figures in
//! `docs/nodes/structure.md` come from the lines these tests print
//! (`-- --nocapture`).

use serde_json::{Value, json};

use crate::common::*;

/// Frame length of the band-over-time measurements, seconds.
const FRAME: f32 = 0.05;

/// Power (linear, `band_db` scaling) of `lo..hi` Hz in each `FRAME` of `x`.
fn band_track(x: &[f32], lo: f32, hi: f32) -> Vec<f32> {
    let n = (FRAME * SR as f32) as usize;
    x.chunks(n)
        .filter(|c| c.len() == n)
        .map(|c| 10f32.powf(band_db(c, SR, lo, hi) / 10.0))
        .collect()
}

/// Seconds from a band's loudest frame until it last sits within `drop_db`
/// of that peak (the band's audible length at that depth).
fn band_length(x: &[f32], lo: f32, hi: f32, drop_db: f32) -> f32 {
    let t = band_track(x, lo, hi);
    let (peak_i, peak) = t
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |m, (i, p)| if *p > m.1 { (i, *p) } else { m });
    let floor = peak * 10f32.powf(-drop_db / 10.0);
    let last = t.iter().rposition(|p| *p >= floor).unwrap_or(peak_i);
    (last - peak_i) as f32 * FRAME
}

/// The frequency under which 99 % of the power of `x[a..b]` lies: the
/// effective low-pass of that stretch.
fn rolloff(x: &[f32], a: f32, b: f32) -> f32 {
    let (i, j) = (
        (a * SR as f32) as usize,
        ((b * SR as f32) as usize).min(x.len()),
    );
    let s = spectrum(&x[i.min(j)..j], SR);
    let p: Vec<f32> = s.iter().map(|(_, d)| 10f32.powf(d / 10.0)).collect();
    let total: f32 = p.iter().sum();
    let mut acc = 0.0;
    for ((f, _), v) in s.iter().zip(&p) {
        acc += v;
        if acc >= 0.99 * total {
            return *f;
        }
    }
    s.last().map_or(0.0, |v| v.0)
}

/// Share of the energy of `x` under `hz`.
fn share_under(x: &[f32], hz: f32) -> f32 {
    let lo = 10f32.powf(band_db(x, SR, 0.0, hz) / 10.0);
    let all = 10f32.powf(band_db(x, SR, 0.0, SR as f32) / 10.0);
    lo / all.max(1e-30)
}

fn impulse() -> Value {
    json!({"type": "impulse", "name": "hit", "at": 0.01})
}

fn structure(extra: Value) -> Value {
    with_fields(
        json!({"type": "structure", "name": "s", "excite": "hit", "material": "metal",
            "size": 10.0, "thickness": 0.003}),
        extra,
    )
}

#[test]
fn structure_is_finite_across_its_range() {
    for (size, h) in [
        (0.3, 0.0005),
        (1.0, 0.002),
        (10.0, 0.003),
        (40.0, 0.002),
        (80.0, 0.05),
        (200.0, 0.5),
    ] {
        for shape in ["plate", "shell"] {
            for material in ["metal", "wood", "glass", "stone"] {
                let x = node_audio(
                    [
                        impulse(),
                        structure(json!({"size": size, "thickness": h, "shape": shape,
                            "material": material})),
                    ],
                    "s",
                    1.0,
                );
                assert_finite(&x, &format!("{shape} {material} {size} m {h} m"));
                assert!(peak(&x) > 0.1, "{shape} {material} {size} m: silent");
            }
        }
    }
    // Extremes of loss, edge loss, tilt and low_modes.
    for extra in [
        json!({"loss": 0.01, "edge_loss": 0.0}),
        json!({"loss": 100.0, "edge_loss": 1.0}),
        json!({"tilt": -12.0, "low_modes": 0}),
        json!({"tilt": 12.0, "low_modes": 96, "shape": "shell", "size": 0.5}),
    ] {
        let x = node_audio([impulse(), structure(extra.clone())], "s", 1.0);
        assert_finite(&x, &extra.to_string());
    }
}

#[test]
fn structure_peaks_at_0_dbfs_for_a_unit_impulse() {
    let x = node_audio([impulse(), structure(json!({}))], "s", 2.0);
    let p = peak(&x);
    println!("unit impulse peak {p:.3}");
    assert!((p - 1.0).abs() < 0.01, "{p}");
}

#[test]
fn bigger_structures_are_lower_and_ring_longer_in_the_lows() {
    let mut last: Option<(f32, f32)> = None;
    for size in [0.5, 2.0, 8.0, 30.0] {
        let x = node_audio([impulse(), structure(json!({"size": size}))], "s", 6.0);
        let c = stft_centroid(&x);
        let low = band_length(&x, 60.0, 250.0, 30.0);
        println!("size {size} m: centroid {c:.0} Hz, 60-250 Hz lasts {low:.2} s to -30 dB");
        if let Some((pc, pl)) = last {
            assert!(c < pc, "size {size}: centroid {c} not below {pc}");
            assert!(
                low > pl,
                "size {size}: low band {low} s not longer than {pl}"
            );
        }
        last = Some((c, low));
    }
}

#[test]
fn high_bands_decay_faster_than_low_bands() {
    let x = node_audio(
        [
            impulse(),
            structure(json!({"size": 20.0, "thickness": 0.002})),
        ],
        "s",
        6.0,
    );
    let bands = [
        (60.0, 250.0),
        (250.0, 1000.0),
        (1000.0, 4000.0),
        (4000.0, 16000.0),
    ];
    let lengths: Vec<f32> = bands
        .iter()
        .map(|(lo, hi)| band_length(&x, *lo, *hi, 30.0))
        .collect();
    println!("band lengths to -30 dB (60-250, 250-1k, 1-4k, 4-16k Hz): {lengths:.2?} s");
    for w in lengths.windows(2) {
        assert!(w[1] < w[0], "{lengths:?}");
    }
    // About 1 / f: each two octaves up rings at most half as long.
    assert!(lengths[0] > 4.0 * lengths[2], "{lengths:?}");
}

/// The strongest bin of the 0.6 s after `at`, in `lo..hi` Hz, over the
/// median of the bins within a sixth of an octave of it, dB: a ringing
/// partial stands far over its neighbours, noise only by chance (about
/// 10-15 dB for the few thousand bins measured here).
fn partial_over_floor(x: &[f32], at: f32, lo: f32, hi: f32) -> f32 {
    let i = (at * SR as f32) as usize;
    let j = (i + (0.6 * SR as f32) as usize).min(x.len());
    let s = spectrum(&x[i..j], SR);
    let span = 2f32.powf(1.0 / 12.0);
    let mut worst = f32::MIN;
    for (f, l) in s.iter().filter(|(f, _)| (lo..hi).contains(f)) {
        let mut near: Vec<f32> = s
            .iter()
            .filter(|(g, _)| *g >= f / span && *g <= f * span)
            .map(|v| v.1)
            .collect();
        if near.len() < 9 {
            continue;
        }
        near.sort_by(f32::total_cmp);
        worst = worst.max(l - near[near.len() / 2]);
    }
    worst
}

#[test]
fn large_structures_do_not_ring_partials() {
    for (shape, size) in [("plate", 20.0), ("shell", 4.0), ("plate", 60.0)] {
        let x = node_audio(
            [
                impulse(),
                structure(
                    json!({"shape": shape, "size": size, "thickness": 0.002, "length": if shape == "shell" { json!(40.0) } else { Value::Null }}),
                ),
            ],
            "s",
            2.0,
        );
        let over = partial_over_floor(&x, 0.6, 100.0, 8000.0);
        println!("{shape} {size} m: strongest peak 0.6-1.2 s is {over:.1} dB over the median");
        // Noise of this length peaks about 12-15 dB over its median by chance;
        // a ringing partial stands 30 dB or more over it.
        assert!(over < 22.0, "{shape} {size}: {over} dB");
    }
    // The measure does see partials: a ringing `modal` plate stands far over.
    let m = node_audio(
        [
            impulse(),
            json!({"type": "modal", "name": "m", "excite": "hit", "material": "metal", "size": 1.0, "loss": 0.3}),
        ],
        "m",
        2.0,
    );
    let over = partial_over_floor(&m, 0.6, 100.0, 8000.0);
    println!("modal metal plate 1 m: strongest peak {over:.1} dB over the median");
    assert!(over > 30.0, "{over}");
}

#[test]
fn structure_seeds_differ_and_repeat() {
    let a = node_audio([impulse(), structure(json!({"seed": 1}))], "s", 1.0);
    let b = node_audio([impulse(), structure(json!({"seed": 2}))], "s", 1.0);
    let a2 = node_audio([impulse(), structure(json!({"seed": 1}))], "s", 1.0);
    assert_eq!(fnv(&a), fnv(&a2));
    assert_ne!(fnv(&a), fnv(&b));
    // Different takes, not a shifted copy: they hardly correlate.
    let dot: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
    let r = dot / (rms(&a) * rms(&b) * a.len() as f32);
    println!("seed 1 vs 2 correlation {r:.3}");
    assert!(r.abs() < 0.2, "{r}");
    assert_bit_identical([impulse(), structure(json!({}))], 1.0);
}

#[test]
fn structure_takes_a_continuous_excitation() {
    // A two-second scrape: noise through friction-like gating.
    let x = node_audio(
        [
            json!({"type": "noise", "name": "n"}),
            json!({"type": "env", "name": "e", "points": [[0.0, 0.0], [0.1, 0.3], [2.0, 0.3], [2.1, 0.0]]}),
            json!({"type": "gain", "name": "hit", "input": "n", "gain": "e"}),
            structure(json!({"size": 6.0})),
        ],
        "s",
        3.0,
    );
    assert_finite(&x, "scrape");
    let mid = rms_db(&x[(1.0 * SR as f32) as usize..(1.5 * SR as f32) as usize]);
    println!("scrape level 1.0-1.5 s: {mid:.1} dB");
    assert!(mid > -60.0);
}

#[test]
fn structure_errors_name_the_range() {
    let e = render_error(vec![impulse(), structure(json!({"size": 500.0}))], "s", 0.5);
    assert!(e.contains("size") && e.contains("200"), "{e}");
    let e = render_error(vec![impulse(), structure(json!({"length": 3.0}))], "s", 0.5);
    assert!(e.contains("shell"), "{e}");
}

fn rumble(extra: Value) -> Value {
    with_fields(json!({"type": "rumble", "name": "r"}), extra)
}

#[test]
fn rumble_stays_under_its_top() {
    for top in [40.0, 120.0, 400.0] {
        for depth in [0.0, 1.0] {
            let x = node_audio(
                [rumble(
                    json!({"top": top, "flutter_depth": depth, "flutter_rate": 20}),
                )],
                "r",
                3.0,
            );
            assert_finite(&x, "rumble");
            let over = 1.0 - share_under(&x, 2.0 * top);
            println!(
                "top {top} Hz depth {depth}: {:.2} % of the energy above 2 x top",
                100.0 * over
            );
            assert!(over < 0.05, "top {top}: {over}");
        }
    }
}

#[test]
fn rumble_has_no_pitch() {
    let x = node_audio([rumble(json!({"decay": 10.0}))], "r", 3.0);
    let over = partial_over_floor(&x, 0.5, 25.0, 120.0);
    println!("rumble: strongest peak {over:.1} dB over the median in 25-120 Hz");
    assert!(over < 22.0, "{over}");
}

#[test]
fn rumble_decays_and_follows_energy() {
    let one = node_audio([rumble(json!({}))], "r", 6.0);
    let four = node_audio([rumble(json!({"energy": 4.0}))], "r", 6.0);
    let l1 = band_length(&one, 20.0, 200.0, 40.0);
    let l4 = band_length(&four, 20.0, 200.0, 40.0);
    let g = rms_db(&four) - rms_db(&one);
    println!(
        "rumble lasts {l1:.2} s (energy 1) / {l4:.2} s (energy 4) to -40 dB; energy 4 is {g:+.1} dB"
    );
    assert!(l1 > 1.5 && l4 > l1);
    assert!(g > 3.0);
    // Following a signal: silent before it, rumbling after it.
    let f = node_audio(
        [
            json!({"type": "decay", "name": "kick", "time": 0.3, "at": 1.0}),
            rumble(json!({"follow": "kick"})),
        ],
        "r",
        3.0,
    );
    let before = rms_db(&f[..(0.9 * SR as f32) as usize]);
    let after = rms_db(&f[(1.1 * SR as f32) as usize..(1.6 * SR as f32) as usize]);
    println!("follow: {before:.1} dB before, {after:.1} dB after");
    assert!(before < -100.0 && after > -60.0);
}

#[test]
fn rumble_seeds_differ() {
    let a = node_audio([rumble(json!({"seed": 1}))], "r", 1.0);
    let b = node_audio([rumble(json!({"seed": 2}))], "r", 1.0);
    assert_ne!(fnv(&a), fnv(&b));
}

/// A heavy metal hit, tuned against the recordings (`docs/nodes/structure.md`,
/// "Against the recordings"): an impulse and 0.28 s of crushing noise
/// strike a 4 m x 40 m, 2 mm steel shell with frames every 3 m, and a
/// rumble runs under it.
fn heavy_hit() -> Vec<Value> {
    vec![
        json!({"type": "impulse", "name": "click", "at": 0.01}),
        json!({"type": "noise", "name": "n"}),
        json!({"type": "decay", "name": "d", "time": 0.28, "at": 0.01}),
        json!({"type": "gain", "name": "crush", "input": "n", "gain": "d"}),
        json!({"type": "mix", "name": "hit", "inputs": ["click", {"input": "crush", "gain": 0.07}]}),
        json!({"type": "structure", "name": "s", "excite": "hit", "material": "metal", "shape": "shell",
            "size": 4.0, "length": 40.0, "thickness": 0.002, "loss": 0.07, "panel": 3.0,
            "edge_loss": 0.075, "tilt": -2.5}),
        json!({"type": "rumble", "name": "r", "top": 115, "flutter_rate": 16, "flutter_depth": 0.6,
            "onset": 0.03, "decay": 2.5, "at": 0.01}),
        json!({"type": "mix", "name": "all", "inputs": ["s", {"input": "r", "gain": 1.8}]}),
    ]
}

#[test]
fn a_heavy_hit_has_the_shape_of_the_recordings() {
    let t = std::time::Instant::now();
    let x = node_audio(heavy_hit(), "all", 3.0);
    let ms = t.elapsed().as_millis();
    assert_finite(&x, "heavy hit");
    let roll: Vec<f32> = [(0.0, 0.1), (0.1, 0.2), (0.3, 0.4), (0.6, 0.7), (1.1, 1.3)]
        .iter()
        .map(|(a, b)| rolloff(&x, 0.01 + a, 0.01 + b))
        .collect();
    let under = share_under(&x, 200.0);
    let low = band_length(&x, 20.0, 120.0, 40.0);
    let mid = band_length(&x, 500.0, 2000.0, 40.0);
    let high = band_length(&x, 4000.0, 16000.0, 40.0);
    println!(
        "heavy hit ({ms} ms to render 3 s): 99 % roll-off {roll:.0?} Hz over 0-0.1, 0.1-0.2, 0.3-0.4, 0.6-0.7, 1.1-1.3 s; {:.0} % of the power under 200 Hz; to -40 dB: 20-120 Hz {low:.2} s, 0.5-2 kHz {mid:.2} s, 4-16 kHz {high:.2} s",
        100.0 * under
    );
    // The recordings (el_midair_hit, el_ground_slam, el_wing_snap, measured
    // the same way): roll-off 3.8-10.6 kHz in the first 0.1 s falling to
    // 80-220 Hz by 1.1 s; 85-99 % of the power under 200 Hz; 20-120 Hz
    // lasts 0.7-2.05 s to -40 dB, 0.5-2 kHz 0.35-0.85 s, 4-16 kHz
    // 0.4-0.6 s.
    assert!(roll[0] > 6000.0, "{roll:?}");
    assert!(roll[4] < 500.0, "{roll:?}");
    assert!(roll.windows(2).all(|w| w[1] < w[0]), "{roll:?}");
    assert!((0.85..0.995).contains(&under), "{under}");
    assert!(low >= 1.5, "{low}");
    assert!((0.6..1.5).contains(&mid), "{mid}");
    assert!(high < 0.6 && high < mid && mid < low, "{high} {mid} {low}");
}
