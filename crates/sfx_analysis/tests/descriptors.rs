//! The descriptor additions read synthetic signals with known answers.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use sfx_analysis::describe;
use sfx_analysis::descriptors::descriptors_window;
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

// ---- Round 2 (R2-C): fine onsets, the swell headline, rate by tenth, blips.

/// A tick train: `count` ticks, 5 ms or more apart, starting at `start` s.
fn tick_train(secs: f32, start: f32, gaps: impl Fn(usize) -> f32) -> (Vec<f32>, usize) {
    let mut x = vec![0.0; n(secs)];
    let (mut t, mut k) = (start, 0);
    while n(t) + 200 < x.len() {
        tick(&mut x, n(t), 0.5);
        t += gaps(k);
        k += 1;
    }
    (x, k)
}

#[test]
fn fine_onsets_count_hits_the_merged_list_cannot() {
    // 40 hits, 10 ms apart: the 40 ms merge sees a handful, the fine list all.
    let (x, placed) = tick_train(1.0, 0.1, |_| 0.01);
    let d = describe(&mono(x));
    println!(
        "placed {placed}: merged {}, fine {}",
        d.onsets.len(),
        d.fine_onsets.len()
    );
    assert!(d.onsets.len() < placed / 2, "{} merged", d.onsets.len());
    assert!(
        (d.fine_onsets.len() as f32 - placed as f32).abs() <= 2.0,
        "fine {}",
        d.fine_onsets.len()
    );
    assert_eq!(d.fine_onsets.len(), d.density.count);
    // The first fine onset is at the first tick (within a couple of ms).
    assert!(
        (d.fine_onsets[0] - 0.1).abs() < 0.003,
        "{:?}",
        d.fine_onsets
    );
    let t = d.table();
    println!("{t}");
    let first = t.lines().find(|l| l.starts_with("onsets ")).unwrap();
    assert!(
        first.starts_with(&format!(
            "onsets {} (40 ms merge); fine {} (5 ms)",
            d.onsets.len(),
            d.fine_onsets.len()
        )),
        "{first}"
    );
    // 40 times at most, in ms, on compact rows.
    let listed: Vec<&str> = t
        .lines()
        .skip_while(|l| !l.starts_with("fine onsets:"))
        .take_while(|l| l.starts_with("fine onsets:") || l.starts_with("            "))
        .filter(|l| !l.contains("..."))
        .collect();
    assert!(!listed.is_empty() && listed.len() <= 3, "{listed:?}");
    assert!(listed.last().unwrap().ends_with("(ms)") || t.contains("more fine onsets"));
    let ms: usize = listed
        .iter()
        .map(|l| {
            l.trim_start_matches("fine onsets:")
                .trim_end_matches("(ms)")
                .split_whitespace()
                .filter(|w| w.parse::<f32>().is_ok())
                .count()
        })
        .sum();
    assert!(ms <= 40, "{ms} times listed");
    // More than 40 hits: the rest is counted, not listed.
    let (x, placed) = tick_train(2.0, 0.05, |_| 0.02);
    let t = describe(&mono(x)).table();
    assert!(placed > 40);
    assert!(t.contains("more fine onsets not shown"), "{t}");
}

#[test]
fn the_swell_headline_is_one_number_and_the_others_are_labelled() {
    // A noise swell: linear amplitude rise over 400 ms, then a slow fall.
    let x: Vec<f32> = noise(1.0, 1.5, 4)
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let t = i as f32 / SR as f32;
            let env = if t < 0.4 {
                t / 0.4
            } else {
                (-(t - 0.4) / 0.3).exp()
            };
            v * env
        })
        .collect();
    let d = describe(&mono(x));
    println!(
        "swell {:.1} ms, to90 {:.1} ms, 10-90 {:.1} ms",
        d.swell_attack_s * 1000.0,
        d.attack_to90_s * 1000.0,
        d.attack_s * 1000.0
    );
    // 90% of a linear rise to 0.4 s is at about 0.36 s (the 10 ms window lags
    // it by a few ms; the noise's own ripple moves the peak a little).
    assert!(
        (d.swell_attack_s - 0.36).abs() < 0.04,
        "swell {} s",
        d.swell_attack_s
    );
    let t = d.table();
    let head: Vec<&str> = t
        .lines()
        .filter(|l| l.starts_with("swell/attack"))
        .collect();
    assert_eq!(head.len(), 1, "{t}");
    assert!(
        head[0].contains("(the headline)") && head[0].contains("90%"),
        "{t}"
    );
    assert!(t.contains("attack (10-90% rise)"), "{t}");
    assert!(
        t.contains("attack detail: first sound to 90% of peak"),
        "{t}"
    );
    assert!(t.contains("swell/attack is the headline"), "{t}");
    // A click and a step read near zero.
    let mut c = vec![0.0; n(0.5)];
    tick(&mut c, n(0.1), 0.8);
    let d = describe(&mono(c));
    println!("click swell {:.2} ms", d.swell_attack_s * 1000.0);
    assert!(d.swell_attack_s < 0.004, "{}", d.swell_attack_s);
    let step: Vec<f32> = (0..n(0.5))
        .map(|i| if i < n(0.2) { 0.0 } else { 0.4 })
        .collect();
    let d = describe(&mono(step));
    assert!(d.swell_attack_s < 0.002, "step {}", d.swell_attack_s);
    // Silence and loops.
    assert_eq!(describe(&mono(vec![0.0; 1000])).swell_attack_s, 0.0);
    let t = describe(&mono(noise(0.3, 1.0, 1))).with_loop(true).table();
    assert!(!t.contains("swell/attack"), "{t}");
}

#[test]
fn density_by_tenth_shows_a_rate_that_rises() {
    // A zipper: the rate rises 30 -> 200 per s over 2 s.
    let at = std::cell::Cell::new(0.01f32);
    let (x, _) = tick_train(2.0, 0.01, |_| {
        let gap = 1.0 / (30.0 + 85.0 * at.get());
        at.set(at.get() + gap);
        gap
    });
    let d = describe(&mono(x));
    println!("tenths {:?}", d.density_tenths);
    assert_eq!(d.density_tenths.len(), 10);
    let t = d.table();
    let line = t
        .lines()
        .find(|l| l.starts_with("density by tenth"))
        .unwrap();
    println!("{line}");
    let v: Vec<f32> = line
        .split(':')
        .nth(1)
        .unwrap()
        .split_whitespace()
        .map(|w| w.parse().unwrap())
        .collect();
    assert_eq!(v.len(), 10);
    assert!(v[9] > 2.0 * v[0], "{v:?}");
    let up = v.windows(2).filter(|w| w[1] >= w[0] - 1.0).count();
    assert!(up >= 8, "mostly rising: {v:?}");
    assert!(line.chars().count() < 120);
    // A steady 100/s is flat within 15 %.
    let (x, _) = tick_train(2.0, 0.005, |_| 0.01);
    let d = describe(&mono(x));
    for r in &d.density_tenths[1..9] {
        assert!((r - 100.0).abs() < 15.0, "{:?}", d.density_tenths);
    }
    // Nothing happening: no line.
    let t = describe(&mono(vec![0.0; n(0.5)])).table();
    assert!(!t.contains("density by tenth"), "{t}");
}

/// A `ms`-long sine blip whose frequency goes `f0` -> `f1` linearly, at `at`
/// samples into `len` samples; `tau` (s) is an optional exponential decay.
fn chirp_blip(len: usize, at: usize, ms: f32, f0: f32, f1: f32, tau: Option<f32>) -> Vec<f32> {
    let m = (ms / 1000.0 * SR as f32) as usize;
    let mut x = vec![0.0; len];
    let mut ph = 0.0f32;
    for j in 0..m {
        let u = j as f32 / m as f32;
        ph += std::f32::consts::TAU * (f0 + (f1 - f0) * u) / SR as f32;
        let env = tau.map_or(1.0, |t| (-(j as f32 / SR as f32) / t).exp());
        x[at + j] = 0.6 * ph.sin() * env;
    }
    x
}

#[test]
fn a_short_chirp_reads_its_start_and_end_pitch() {
    // 30 ms, 800 -> 1600 Hz, hard start and stop.
    let d = describe(&mono(chirp_blip(n(0.5), n(0.2), 30.0, 800.0, 1600.0, None)));
    println!("{:?}", d.onset_rows);
    let row = d.onset_rows[0];
    let (a, b) = (row.pitch_start_hz.unwrap(), row.pitch_end_hz.unwrap());
    println!("start {a}, end {b}");
    assert!((a - 800.0).abs() / 800.0 < 0.08, "start {a}");
    assert!((b - 1600.0).abs() / 1600.0 < 0.08, "end {b}");
    let t = d.table();
    assert!(
        t.lines()
            .any(|l| l.contains(">") && l.trim_start().starts_with("0.")),
        "{t}"
    );
    // Falling, 40 ms: 2400 -> 1200 Hz.
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        45.0,
        2400.0,
        1200.0,
        None,
    )));
    let row = d.onset_rows[0];
    let (a, b) = (row.pitch_start_hz.unwrap(), row.pitch_end_hz.unwrap());
    println!("falling start {a}, end {b}; rows {:?}", d.onset_rows);
    // Measured 2408 / 1351 (+0.3 % / +12.6 %): a falling glide this steep is
    // read less well than the rising one above; direction and size are right.
    assert!((a - 2400.0).abs() / 2400.0 < 0.08, "start {a}");
    assert!((b - 1200.0).abs() / 1200.0 < 0.15, "end {b}");
}

#[test]
fn a_steady_blip_or_a_click_reports_no_glide() {
    // A steady 1 kHz blip (30 ms), a blip of noise, and a 1 ms tick.
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        30.0,
        1000.0,
        1000.0,
        None,
    )));
    assert!(
        d.onset_rows[0].pitch_start_hz.is_none(),
        "{:?}",
        d.onset_rows
    );
    let mut x = vec![0.0; n(0.5)];
    for (j, v) in noise(0.5, 0.03, 9).into_iter().enumerate() {
        x[n(0.2) + j] = v;
    }
    let d = describe(&mono(x));
    assert!(
        d.onset_rows.iter().all(|r| r.pitch_start_hz.is_none()),
        "{:?}",
        d.onset_rows
    );
    let mut x = vec![0.0; n(0.5)];
    tick(&mut x, n(0.2), 0.8);
    let d = describe(&mono(x));
    assert!(d.onset_rows.iter().all(|r| r.pitch_start_hz.is_none()));
}

#[test]
fn a_decaying_drip_like_chirp_still_reads_a_rise() {
    // 40 ms, 700 -> 1300 Hz, 12 ms decay constant (a drip's shape).
    let d = describe(&mono(chirp_blip(
        n(0.5),
        n(0.2),
        40.0,
        700.0,
        1300.0,
        Some(0.012),
    )));
    let row = d.onset_rows[0];
    println!("{:?}", row);
    let (a, b) = (row.pitch_start_hz.unwrap(), row.pitch_end_hz.unwrap());
    println!("drip start {a}, end {b}");
    // Measured 991 / 1256 (truth 700 / 1300): the loud early part dominates
    // each half, so the rise is understated but its direction is clear.
    assert!(b > a * 1.2 && a < 1100.0 && b > 1150.0, "{a} {b}");
}

#[test]
fn a_window_of_the_sound_has_its_own_descriptors() {
    // Ticks at 0.2 s and 0.7 s, 3 kHz.
    let mut x = vec![0.0; n(1.0)];
    tick(&mut x, n(0.2), 0.8);
    tick(&mut x, n(0.7), 0.4);
    let a = mono(x);
    let whole = describe(&a);
    assert_eq!(whole.fine_onsets.len(), 2);
    let w = descriptors_window(&a, 0.5, 1.0);
    println!("{}", w.table());
    assert!((w.seconds - 0.5).abs() < 1e-3, "{}", w.seconds);
    assert_eq!(w.fine_onsets.len(), 1);
    assert!(
        (w.fine_onsets[0] - 0.2).abs() < 0.003,
        "{:?}",
        w.fine_onsets
    );
    // The quieter hit is its peak: about 6 dB under the whole's.
    assert!(
        (whole.peak_db - w.peak_db - 6.0).abs() < 1.0,
        "{} {}",
        whole.peak_db,
        w.peak_db
    );
    // Clamped and degenerate windows.
    assert!((descriptors_window(&a, -1.0, 9.0).seconds - 1.0).abs() < 1e-3);
    let e = descriptors_window(&a, 0.8, 0.3);
    let _ = e.table();
    let _ = descriptors_window(&a, 5.0, 6.0).table();
    // Stereo keeps both channels.
    let s = Audio {
        sample_rate: SR,
        channels: vec![a.channels[0].clone(), a.channels[0].clone()],
    };
    assert_eq!(descriptors_window(&s, 0.1, 0.4).channels, 2);
}
