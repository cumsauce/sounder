//! R4-B: per-onset detail in `listen` (-60 dB column, `n/a` pitch, glide,
//! ignored slow modes, inharmonicity from partial ratios, close pairs).

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use sfx_analysis::describe;
use sfx_graph::Audio;

const SR: u32 = 48_000;

fn n(secs: f32) -> usize {
    (secs * SR as f32) as usize
}

fn mono(x: Vec<f32>) -> Audio {
    Audio::mono(SR, x)
}

/// Deterministic white noise in -1..1 (an LCG).
fn noise(len: usize, seed: u64) -> Vec<f32> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (0..len)
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 40) as f32 / (1u64 << 23) as f32 - 1.0
        })
        .collect()
}

/// A hit at `at` seconds: partials (hz, amp) all decaying with `tau`.
fn add_hit(x: &mut [f32], at: f32, parts: &[(f32, f32)], tau: f32) {
    let a = n(at);
    for j in 0..x.len().saturating_sub(a) {
        let t = j as f32 / SR as f32;
        let e = (-t / tau).exp();
        if e < 1e-5 {
            break;
        }
        for (hz, amp) in parts {
            x[a + j] += amp * e * (std::f32::consts::TAU * hz * t).sin();
        }
    }
}

fn tick(x: &mut [f32], at: usize, amp: f32) {
    for j in 0..120 {
        if let Some(v) = x.get_mut(at + j) {
            let t = j as f32 / SR as f32;
            *v += amp * (std::f32::consts::TAU * 3000.0 * t).sin() * (-t / 0.0003).exp();
        }
    }
}

#[test]
fn inharmonic_metal_hits_read_na_pitch_and_over_100_cents() {
    // Clink-like partial sets: 1.5k / 1.93k / 3.9k / 5.3k, and another one.
    for parts in [
        [(1500.0, 0.3), (1930.0, 0.25), (3900.0, 0.2), (5300.0, 0.15)],
        [(2100.0, 0.3), (2740.0, 0.25), (5600.0, 0.2), (7500.0, 0.15)],
    ] {
        let mut x = vec![0.0; n(1.0)];
        add_hit(&mut x, 0.1, &parts, 0.12);
        let d = describe(&mono(x));
        let t = d.table();
        println!("{t}");
        let c = d.inharmonicity_cents.expect("partials found");
        assert!(c > 100.0, "cents {c}");
        assert!(!d.inharmonic_partials.is_empty());
        assert!(t.contains("inharmonic: partials at"), "{t}");
        let row = d.onset_rows[0];
        assert!(row.pitch_na, "{row:?}");
        assert!(
            t.lines()
                .any(|l| l.trim_start().starts_with("0.0") && l.trim_end().ends_with("n/a")),
            "{t}"
        );
    }
}

#[test]
fn a_harmonic_hit_keeps_its_pitch_and_stays_under_100_cents() {
    let mut x = vec![0.0; n(1.0)];
    add_hit(
        &mut x,
        0.1,
        &[(300.0, 0.3), (600.0, 0.2), (900.0, 0.15), (1200.0, 0.1)],
        0.2,
    );
    let d = describe(&mono(x));
    let t = d.table();
    println!("{t}");
    assert!(d.inharmonicity_cents.is_none_or(|c| c < 100.0));
    assert!(!d.onset_rows[0].pitch_na);
    assert!(!t.contains("inharmonic:"), "{t}");
}

/// A `ms`-long sine blip, frequency `f0` -> `f1` linearly, `tau` decay.
fn chirp_blip(len: usize, at: usize, ms: f32, f0: f32, f1: f32, tau: f32) -> Vec<f32> {
    let m = (ms / 1000.0 * SR as f32) as usize;
    let mut x = vec![0.0; len];
    let mut ph = 0.0f32;
    for j in 0..m {
        let u = j as f32 / m as f32;
        ph += std::f32::consts::TAU * (f0 + (f1 - f0) * u) / SR as f32;
        x[at + j] = 0.6 * ph.sin() * (-(j as f32 / SR as f32) / tau).exp();
    }
    x
}

#[test]
fn a_40_ms_bubble_sweeping_up_reads_glide_up() {
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        40.0,
        1600.0,
        3400.0,
        0.03,
    )));
    let row = d.onset_rows[0];
    println!("{row:?}");
    let (a, b) = row.glide_hz.expect("glide");
    assert!(b > a * 1.4, "{a} {b}");
    assert!((a - 1600.0).abs() / 1600.0 < 0.25, "start {a}");
    assert!(b > 2800.0, "end {b}");
    let t = d.table();
    assert!(t.contains("glide up"), "{t}");
    // Falling.
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        40.0,
        3400.0,
        1600.0,
        0.03,
    )));
    assert!(d.table().contains("glide down"), "{}", d.table());
}

#[test]
fn a_steady_blip_a_click_and_a_noise_burst_have_no_glide() {
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        40.0,
        2000.0,
        2000.0,
        0.03,
    )));
    assert!(d.onset_rows[0].glide_hz.is_none(), "{:?}", d.onset_rows);
    let mut x = vec![0.0; n(0.5)];
    tick(&mut x, n(0.2), 0.8);
    assert!(
        describe(&mono(x))
            .onset_rows
            .iter()
            .all(|r| r.glide_hz.is_none())
    );
    let mut x = vec![0.0; n(0.5)];
    for (j, v) in noise(n(0.03), 9).into_iter().enumerate() {
        x[n(0.2) + j] = 0.5 * v;
    }
    let d = describe(&mono(x));
    assert!(
        d.onset_rows.iter().all(|r| r.glide_hz.is_none()),
        "{:?}",
        d.onset_rows
    );
}

#[test]
fn per_onset_minus_60_db_reads_the_true_decays() {
    // Three knocks, T60 = 6.9 tau: 138, 276, 207 ms.
    let taus = [0.02f32, 0.04, 0.03];
    let mut x = vec![0.0; n(2.4)];
    for (i, tau) in taus.iter().enumerate() {
        add_hit(&mut x, 0.2 + 0.7 * i as f32, &[(440.0, 0.5)], *tau);
    }
    let d = describe(&mono(x));
    let t = d.table();
    println!("{t}");
    assert_eq!(d.onset_rows.len(), 3, "{:?}", d.onsets);
    for (row, tau) in d.onset_rows.iter().zip(taus) {
        let want = tau * 6.908 * 1000.0;
        assert!(!row.decay60_open, "{row:?}");
        assert!(
            (row.decay60_ms - want).abs() / want < 0.2,
            "got {} want {want}",
            row.decay60_ms
        );
    }
    assert!(t.contains("-60 dB ms"));
}

#[test]
fn a_hit_still_sounding_at_the_next_onset_reads_open() {
    let mut x = vec![0.0; n(1.2)];
    add_hit(&mut x, 0.1, &[(440.0, 0.5)], 0.5);
    add_hit(&mut x, 0.4, &[(660.0, 0.5)], 0.5);
    let d = describe(&mono(x));
    let t = d.table();
    println!("{t}");
    assert!(d.onset_rows[0].decay60_open, "{:?}", d.onset_rows);
    assert!(
        t.lines().any(|l| l.contains(" >")),
        "an open -60 column reads >N: {t}"
    );
}

#[test]
fn a_short_noise_tick_footstep_has_no_sustained_modes() {
    let mut x = vec![0.0; n(0.3)];
    for (at, seed) in [(0.03f32, 1u64), (0.15, 2)] {
        let a = n(at);
        for (j, v) in noise(n(0.12), seed).into_iter().enumerate() {
            let t = j as f32 / SR as f32;
            if a + j < x.len() {
                x[a + j] += 0.4 * v * (-t / 0.02).exp();
            }
        }
    }
    let d = describe(&mono(x));
    let t = d.table();
    println!("{t}");
    assert!(!t.contains("sustained"), "{t}");
    for l in t.lines().filter(|l| l.starts_with("modes:")) {
        assert!(!l.contains("600"), "{l}");
        assert!(
            !d.modes.iter().any(|m| m.decay_s > 1.5)
                || l.contains("ignored: slower than the sound"),
            "{l}"
        );
    }
}

#[test]
fn two_clicks_12_ms_apart_report_one_close_pair() {
    let mut x = vec![0.0; n(0.6)];
    tick(&mut x, n(0.2), 0.8);
    tick(&mut x, n(0.212), 0.8);
    let d = describe(&mono(x));
    let t = d.table();
    println!("{t}");
    assert_eq!(
        d.close_pairs.len(),
        1,
        "{:?} {:?}",
        d.close_pairs,
        d.fine_onsets
    );
    assert!(
        t.contains("closely spaced events: 1 pairs within 20 ms (at 200"),
        "{t}"
    );
    // 100 ms apart: no such line.
    let mut x = vec![0.0; n(0.6)];
    tick(&mut x, n(0.2), 0.8);
    tick(&mut x, n(0.3), 0.8);
    let t = describe(&mono(x)).table();
    assert!(!t.contains("closely spaced"), "{t}");
}
