//! The descriptor additions read synthetic signals with known answers.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use sfx_analysis::describe;
use sfx_graph::Audio;

const SR: u32 = 48_000;

fn n(secs: f32) -> usize {
    (secs * SR as f32) as usize
}

fn sine(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..n(secs))
        .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
        .collect()
}

/// Deterministic white noise in -1..1 (an LCG; no dependencies).
fn noise(amp: f32, secs: f32, seed: u64) -> Vec<f32> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (0..n(secs))
        .map(|_| {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            amp * ((s >> 40) as f32 / (1u64 << 23) as f32 - 1.0)
        })
        .collect()
}

/// A short decaying 3 kHz burst (about 1 ms) added into `x` at sample `at`.
fn tick(x: &mut [f32], at: usize, amp: f32) {
    for j in 0..120 {
        if let Some(v) = x.get_mut(at + j) {
            let t = j as f32 / SR as f32;
            *v += amp * (std::f32::consts::TAU * 3000.0 * t).sin() * (-t / 0.0003).exp();
        }
    }
}

fn mono(x: Vec<f32>) -> Audio {
    Audio::mono(SR, x)
}

#[test]
fn two_clicks_give_two_rows_at_the_right_times() {
    let mut x = vec![0.0; n(1.0)];
    tick(&mut x, n(0.2), 0.8);
    tick(&mut x, n(0.7), 0.4);
    let d = describe(&mono(x));
    println!("rows: {:?}", d.onset_rows);
    assert_eq!(d.onset_rows.len(), 2, "{:?}", d.onsets);
    for (row, want) in d.onset_rows.iter().zip([0.2, 0.7]) {
        assert!((row.t - want).abs() < 0.025, "row at {} want {want}", row.t);
        assert!(row.attack_ms < 3.0, "attack {} ms", row.attack_ms);
        let d20 = row.decay20_ms.unwrap_or(f32::MAX);
        assert!(d20 < 15.0, "decay-20 {d20} ms");
        assert!(
            (row.centroid_hz - 3000.0).abs() < 600.0,
            "centroid {}",
            row.centroid_hz
        );
    }
    assert!(d.onset_rows[0].strength >= d.onset_rows[1].strength - 0.01);
    let t = d.table();
    println!("{t}");
    assert!(t.contains("onset table:"), "{t}");
}

#[test]
fn a_1khz_sine_puts_everything_in_one_share() {
    let d = describe(&mono(sine(1000.0, 0.5, 1.0)));
    println!("shares {:?}", d.band_shares);
    assert!(d.band_shares[1] > 97.0, "{:?}", d.band_shares);
    assert!(d.band_shares[0] < 1.0 && d.band_shares[2] < 1.0 && d.band_shares[3] < 1.0);
    let total: f32 = d.band_shares.iter().sum();
    assert!((total - 100.0).abs() < 0.1, "shares sum {total}");
    // A tone is narrow: no occupied-band flatness, and the table says why.
    assert!(d.flatness_occupied.is_none(), "{:?}", d.occupied_hz);
    assert!(d.table().contains("too narrow"));
}

#[test]
fn a_chirp_reads_its_start_and_end_pitch() {
    // 400 -> 800 Hz over 2 s (phase integrated).
    let secs = 2.0;
    let mut ph = 0.0f32;
    let x: Vec<f32> = (0..n(secs))
        .map(|i| {
            let t = i as f32 / SR as f32;
            ph += std::f32::consts::TAU * (400.0 + 200.0 * t) / SR as f32;
            0.5 * ph.sin()
        })
        .collect();
    let d = describe(&mono(x));
    // The abrupt stop is a second onset: the first row spans the glide.
    let row = d.onset_rows[0];
    println!(
        "{} rows, start {:?}, end {:?}, tenths {:?}",
        d.onset_rows.len(),
        row.pitch_start_hz,
        row.pitch_end_hz,
        d.pitch_tenths
    );
    let (a, b) = (row.pitch_start_hz.unwrap(), row.pitch_end_hz.unwrap());
    assert!((a - 400.0).abs() / 400.0 < 0.03, "start {a}");
    assert!((b - 800.0).abs() / 800.0 < 0.03, "end {b}");
    // The contour rises tenth by tenth.
    let c: Vec<f32> = d.pitch_tenths.iter().map(|p| p.unwrap()).collect();
    assert!(c.windows(2).all(|w| w[1] > w[0]), "{c:?}");
    assert!(d.table().contains("pitch by tenth"));
    assert!(d.table().contains('>'), "row shows start>end");
}

#[test]
fn a_crackle_train_reads_its_density() {
    for (spacing, label) in [(240usize, "regular 200/s"), (480, "regular 100/s")] {
        let mut x = vec![0.0; n(1.0)];
        let mut at = 100;
        while at + 200 < x.len() {
            tick(&mut x, at, 0.5);
            at += spacing;
        }
        let want = 48_000.0 / spacing as f32;
        let d = describe(&mono(x));
        println!(
            "{label}: {:.1}/s ({} in {:.3} s), merged onsets {}",
            d.density.per_s,
            d.density.count,
            d.density.span_s,
            d.onsets.len()
        );
        assert!(
            (d.density.per_s - want).abs() / want < 0.1,
            "{label}: {}",
            d.density.per_s
        );
        assert!(d.density.min_spacing_ms >= 3.0 && d.density.min_spacing_ms <= 5.0);
        // The merged list cannot see 200/s.
        assert!(d.table().contains("density:"));
    }
    // Mildly jittered, 100/s on average.
    let mut x = vec![0.0; n(2.0)];
    let mut s = 12345u32;
    let mut at = 500usize;
    let mut placed = 0;
    while at + 200 < x.len() {
        tick(&mut x, at, 0.5);
        placed += 1;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let jitter = (s >> 16) as usize % 241; // 0..5 ms
        at += 480 - 120 + jitter;
    }
    let d = describe(&mono(x));
    println!("jittered: {} events, placed {placed}", d.density.count);
    assert_eq!(d.density.count, placed, "{:?}", d.density);
}

#[test]
fn steady_signals_have_no_density() {
    let d = describe(&mono(sine(120.0, 0.5, 1.0)));
    println!("120 Hz sine: {} events", d.density.count);
    assert!(d.density.count <= 1, "sine: {}", d.density.count);
    let d = describe(&mono(noise(0.3, 2.0, 7)));
    println!("white noise: {} events", d.density.count);
    assert!(d.density.count <= 2, "noise: {}", d.density.count);
}

#[test]
fn occupied_band_flatness_tells_band_limited_noise_from_a_tone() {
    // White noise: flat everywhere.
    let w = describe(&mono(noise(0.3, 2.0, 3)));
    println!(
        "white: full {:.2}, occupied {:?} in {:?}",
        w.flatness_all, w.flatness_occupied, w.occupied_hz
    );
    assert!(w.flatness_all > 0.5);
    assert!(w.flatness_occupied.unwrap() > 0.5);
    // Band-limited "noise" 2-4 kHz: 2 Hz-spaced sines with fixed pseudo-random phases.
    let mut x = vec![0.0f32; n(2.0)];
    let mut s = 99u32;
    let mut f = 2000.0f32;
    while f < 4000.0 {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let phase = (s >> 8) as f32 / (1u32 << 24) as f32 * std::f32::consts::TAU;
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.002 * (std::f32::consts::TAU * f * i as f32 / SR as f32 + phase).sin();
        }
        f += 5.0;
    }
    let b = describe(&mono(x));
    println!(
        "band-limited: full {:.3}, occupied {:?} in {:?}",
        b.flatness_all, b.flatness_occupied, b.occupied_hz
    );
    assert!(b.flatness_all < 0.2, "full {}", b.flatness_all);
    assert!(
        b.flatness_occupied.unwrap() > 0.5,
        "{:?}",
        b.flatness_occupied
    );
    assert!(b.occupied_hz.0 > 1900.0 && b.occupied_hz.1 < 4100.0);
}

#[test]
fn the_whole_sound_centroid_and_its_tenths_follow_the_content() {
    let mut x = sine(500.0, 0.5, 1.0);
    x.extend(sine(4000.0, 0.5, 1.0));
    let d = describe(&mono(x));
    println!(
        "all {:.0} Hz, rolloff {:.0}, tenths {:?}",
        d.centroid_all_hz, d.rolloff_all_hz, d.centroid_tenths
    );
    // Energy-weighted: two equal halves put the centroid between, rolloff at the top tone.
    assert!(d.centroid_all_hz > 1500.0 && d.centroid_all_hz < 3000.0);
    assert!((d.rolloff_all_hz - 4000.0).abs() < 300.0);
    assert_eq!(d.centroid_tenths.len(), 10);
    let first = d.centroid_tenths[1].unwrap();
    let last = d.centroid_tenths[8].unwrap();
    assert!((first - 500.0).abs() < 60.0, "first {first}");
    assert!((last - 4000.0).abs() < 200.0, "last {last}");
    assert!(d.table().contains("centroid by tenth"));
}

#[test]
fn inharmonic_partials_are_named_and_harmonic_ones_are_not() {
    let decaying = |hz: f32, amp: f32, tau: f32| -> Vec<f32> {
        (0..n(1.5))
            .map(|i| {
                let t = i as f32 / SR as f32;
                amp * (-t / tau).exp() * (std::f32::consts::TAU * hz * t).sin()
            })
            .collect()
    };
    let mix = |parts: &[(f32, f32)]| -> Vec<f32> {
        let mut x = vec![0.0f32; n(1.5)];
        for (hz, amp) in parts {
            for (a, b) in x.iter_mut().zip(decaying(*hz, *amp, 0.3)) {
                *a += b;
            }
        }
        x
    };
    let glass = mix(&[
        (919.0, 0.3),
        (1180.0, 0.2),
        (1690.0, 0.2),
        (2180.0, 0.15),
        (2850.0, 0.1),
    ]);
    let d = describe(&mono(glass));
    println!(
        "glass: pitch {:?} harmonicity {:.2} partials {:?}",
        d.pitch_hz, d.harmonicity, d.inharmonic_partials
    );
    assert!(
        d.inharmonic_partials.len() >= 4,
        "{:?}",
        d.inharmonic_partials
    );
    assert!(
        d.inharmonic_partials
            .iter()
            .any(|f| (f - 919.0).abs() < 15.0)
    );
    assert!(d.table().contains("inharmonic: partials at"));

    let harm = mix(&[
        (200.0, 0.3),
        (400.0, 0.2),
        (600.0, 0.2),
        (800.0, 0.15),
        (1000.0, 0.1),
    ]);
    let d = describe(&mono(harm));
    println!(
        "harmonic: pitch {:?} harmonicity {:.2} partials {:?}",
        d.pitch_hz, d.harmonicity, d.inharmonic_partials
    );
    assert!(
        d.inharmonic_partials.is_empty(),
        "{:?}",
        d.inharmonic_partials
    );
    assert!(!d.table().contains("inharmonic:"));
}

#[test]
fn attack_has_two_values_that_say_which_is_which() {
    // Linear 20 ms rise to 1.0, then a slow decay.
    let x: Vec<f32> = sine(1000.0, 1.0, 1.0)
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let t = i as f32 / SR as f32;
            v * (t / 0.02).min(1.0) * (-(t - 0.02).max(0.0) / 0.2).exp()
        })
        .collect();
    let d = describe(&mono(x));
    println!(
        "to90 {:.2} ms, to peak {:.2} ms, 10-90 {:.2} ms",
        d.attack_to90_s * 1000.0,
        d.attack_to_peak_s * 1000.0,
        d.attack_s * 1000.0
    );
    assert!(
        (d.attack_to_peak_s - 0.020).abs() < 0.002,
        "{}",
        d.attack_to_peak_s
    );
    assert!(
        (d.attack_to90_s - 0.018).abs() < 0.002,
        "{}",
        d.attack_to90_s
    );
    assert!(d.attack_to90_s <= d.attack_to_peak_s);
    let t = d.table();
    assert!(
        t.contains("attack detail: first sound to 90% of peak"),
        "{t}"
    );
    // A click is near-instant by both measures.
    let mut c = vec![0.0; n(0.5)];
    c[n(0.1)] = 1.0;
    let d = describe(&mono(c));
    assert!(d.attack_to_peak_s < 0.001 && d.attack_to90_s < 0.001);
}

#[test]
fn a_loop_gets_steadiness_and_no_attack_lines() {
    let steady = describe(&mono(noise(0.3, 3.0, 5))).with_loop(true);
    let s = steady.steadiness.unwrap();
    println!("steady noise: {:.2} LU spread", s.spread_lu);
    assert!(s.spread_lu < 1.0, "{s:?}");
    let t = steady.table();
    assert!(t.contains("loop: n/a"), "{t}");
    assert!(!t.contains("peak at"), "{t}");
    assert!(!t.contains("decay -20"), "{t}");
    assert!(!t.contains("attack detail"), "{t}");
    assert!(t.contains("steadiness:"), "{t}");
    // A decaying sound is not steady.
    let x: Vec<f32> = noise(0.5, 3.0, 5)
        .into_iter()
        .enumerate()
        .map(|(i, v)| v * (-(i as f32) / SR as f32 / 0.5).exp())
        .collect();
    let d = describe(&mono(x));
    println!("decaying: {:.1} LU", d.steadiness.unwrap().spread_lu);
    assert!(d.steadiness.unwrap().spread_lu > 6.0);
    // Not a loop: the usual lines stay.
    let t = d.table();
    assert!(t.contains("attack ") && t.contains("peak at"), "{t}");
}

#[test]
fn silence_is_worded_as_where_it_starts() {
    let mut x = sine(500.0, 0.5, 0.2);
    x.extend(vec![0.0; n(0.3)]);
    let d = describe(&mono(x));
    let t = d.table();
    println!("{}", t.lines().find(|l| l.starts_with("time:")).unwrap());
    assert!(
        t.contains("trailing silence from 0.200 s (0.300 s long)"),
        "{t}"
    );
    assert!(t.contains("no leading silence"), "{t}");
    assert!(!t.contains("silence start"), "{t}");

    let mut x = vec![0.0; n(0.1)];
    x.extend(sine(500.0, 0.5, 0.4));
    let t = describe(&mono(x)).table();
    assert!(t.contains("leading silence 0.100 s"), "{t}");
    assert!(t.contains("no trailing silence"), "{t}");

    let t = describe(&mono(vec![0.0; n(0.2)])).table();
    assert!(t.contains("all silent"), "{t}");
}

#[test]
fn the_onset_table_is_capped_and_every_new_line_is_short() {
    let mut x = vec![0.0; n(5.0)];
    for k in 0..40 {
        tick(&mut x, n(0.1) + k * n(0.12), 0.5);
    }
    let d = describe(&mono(x));
    assert!(d.onsets.len() > 24, "{} onsets", d.onsets.len());
    assert_eq!(d.onset_rows.len(), 24);
    let t = d.table();
    assert!(t.contains("more onsets not shown"), "{t}");
    let rows = t
        .lines()
        .skip_while(|l| !l.starts_with("onset table:"))
        .count();
    assert!(rows <= 26, "{rows} lines in the onset table");
    for l in t.lines() {
        assert!(l.chars().count() < 230, "long line ({}): {l}", l.len());
    }
}

#[test]
fn describe_is_deterministic_and_survives_degenerate_input() {
    let mut x = noise(0.2, 1.0, 2);
    tick(&mut x, 5000, 0.7);
    let a = describe(&mono(x.clone())).table();
    let b = describe(&mono(x)).table();
    assert_eq!(a, b);
    for len in [0usize, 1, 10, 100, 3000] {
        let d = describe(&mono(vec![0.1; len]));
        let _ = d.table();
        let d = describe(&mono(vec![0.0; len]));
        let _ = d.table();
    }
    // Stereo goes through the mono mix.
    let l = sine(300.0, 0.4, 0.5);
    let d = describe(&Audio {
        sample_rate: SR,
        channels: vec![l.clone(), l],
    });
    assert!(d.centroid_all_hz > 200.0 && d.centroid_all_hz < 500.0);
}
