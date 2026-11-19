mod additions {
    //! The descriptor additions read synthetic signals with known answers.

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

    /// A grainy scrape: noise whose level wobbles in 2-12 ms grains, like
    /// the `friction` node's output.
    fn grainy_noise(amp: f32, secs: f32, seed: u64) -> Vec<f32> {
        let mut x = noise(1.0, secs, seed);
        let mut s = (seed as u32).wrapping_mul(2654435761).wrapping_add(7);
        let mut i = 0;
        while i < x.len() {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let len = n(0.002) + (s >> 16) as usize % n(0.010);
            let g = 0.3 + 0.7 * ((s >> 8) & 0xff) as f32 / 255.0;
            for v in x[i..(i + len).min(n(secs))].iter_mut() {
                *v *= g * amp;
            }
            i += len;
        }
        x
    }

    #[test]
    fn a_noise_scrape_is_not_a_stream_of_events() {
        // A 0.6 s scrape, then one hit and its ring: three onsets, not 176/s.
        let mut x = vec![0.0; n(1.2)];
        for (i, v) in grainy_noise(0.2, 0.6, 5).into_iter().enumerate() {
            x[i] = v;
        }
        tick(&mut x, n(0.8), 0.8);
        let d = describe(&mono(x));
        println!(
            "scrape: {:.1}/s ({} counted)",
            d.density.per_s, d.density.count
        );
        assert!(d.density.per_s < 10.0, "scrape + hit: {:?}", d.density);
        // Pink-ish (smoothed) noise alone.
        let mut p = noise(0.3, 0.6, 9);
        let mut lp = 0.0;
        for v in p.iter_mut() {
            lp += 0.3 * (*v - lp);
            *v = lp * 3.0;
        }
        let d = describe(&mono(p));
        println!("lp noise: {:.1}/s", d.density.per_s);
        assert!(d.density.per_s < 10.0, "lp noise: {:?}", d.density);
    }

    #[test]
    fn a_tick_train_over_a_quieter_bed_keeps_its_rate() {
        for (spacing, bed) in [(480usize, 0.03f32), (480, 0.06)] {
            let mut x = noise(bed, 1.0, 11);
            let mut at = 100;
            while at + 200 < x.len() {
                tick(&mut x, at, 0.5);
                at += spacing;
            }
            let want = 48_000.0 / spacing as f32;
            let d = describe(&mono(x));
            println!("bed {bed}: {:.1}/s", d.density.per_s);
            assert!(
                (d.density.per_s - want).abs() / want < 0.15,
                "bed {bed}: {:?}",
                d.density
            );
        }
    }

    #[test]
    fn steady_signals_have_no_density() {
        let d = describe(&mono(sine(120.0, 0.5, 1.0)));
        println!("120 Hz sine: {} events", d.density.count);
        assert!(d.density.count <= 1, "sine: {}", d.density.count);
        let d = describe(&mono(noise(0.3, 2.0, 7)));
        println!("white noise: {} events", d.density.count);
        assert!(d.density.count <= 2, "noise: {}", d.density.count);
        assert!(d.density.per_s < 10.0, "noise rate: {:?}", d.density);
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
}

mod known_signals {
    //! The senses read known signals correctly.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::checks::{Expect, clicks};
    use sfx_analysis::{CheckCx, Column, View, describe, listen_png, run};
    use sfx_graph::{Audio, Kind, RenderOpts, SoundDoc, db_to_gain, loudness};

    const SR: u32 = 48_000;

    fn sine(hz: f32, amp: f32, secs: f32) -> Vec<f32> {
        (0..(secs * SR as f32) as usize)
            .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / SR as f32).sin())
            .collect()
    }

    fn check(d: &SoundDoc, e: Expect) -> Vec<sfx_analysis::CheckResult> {
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
        run(
            &e,
            &CheckCx {
                doc: d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn expect(v: Value) -> Expect {
        serde_json::from_value(v).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_1khz_sine_reads_1000hz_and_a_1khz_centroid() {
        let d = describe(&Audio::mono(SR, sine(1000.0, 0.5, 1.0)));
        let p = d.pitch_hz.unwrap_or_else(|| panic!("no pitch"));
        let cents = 1200.0 * (p / 1000.0).log2();
        assert!(cents.abs() <= 5.0, "pitch {p} Hz ({cents} cents)");
        assert!(d.voiced > 0.9, "voiced {}", d.voiced);
        assert!(d.harmonicity > 0.9, "harmonicity {}", d.harmonicity);
        let c = d.centroid_hz.middle;
        assert!((c - 1000.0).abs() < 50.0, "centroid {c}");
        // The 1k band holds it at about -9 dB (amplitude 0.5 = -6 dBFS peak, -9 dB power).
        assert!(
            (d.bands_total[4] + 9.0).abs() < 1.0,
            "1k band {}",
            d.bands_total[4]
        );
        // The abrupt start and end leak a little broadband energy.
        assert!(d.bands_total[2] < -40.0, "250 band {}", d.bands_total[2]);
        assert!(d.flatness.middle < 0.05, "flatness {}", d.flatness.middle);
        // The strongest mode is the sine itself.
        assert!(
            d.modes.iter().any(|m| (m.hz - 1000.0).abs() < 5.0),
            "{:?}",
            d.modes
        );
    }

    #[test]
    fn a_minus_18_dbfs_sine_reads_the_right_lufs() {
        // BS.1770: a 997 Hz sine at 0 dBFS peak reads -3.01 LUFS.
        let x = sine(997.0, db_to_gain(-18.0), 3.0);
        let l = loudness::integrated(std::slice::from_ref(&x), SR);
        assert!((l - (-21.01)).abs() <= 0.2, "{l} LUFS");
        // At another sample rate the K-weighting is re-derived.
        let x44: Vec<f32> = (0..(3 * 44_100))
            .map(|i| {
                db_to_gain(-18.0) * (std::f32::consts::TAU * 997.0 * i as f32 / 44_100.0).sin()
            })
            .collect();
        let l44 = loudness::integrated(&[x44], 44_100);
        assert!((l44 - (-21.01)).abs() <= 0.2, "{l44} LUFS at 44.1 kHz");
        // Stereo: both channels count (+3 dB).
        let ls = loudness::integrated(&[x.clone(), x], SR);
        assert!((ls - (-18.0)).abs() <= 0.2, "{ls} LUFS stereo");
    }

    #[test]
    fn white_noise_is_flat() {
        let d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n", "amp": 0.3}),
                json!({"type": "output", "name": "out", "input": "n"}),
            ],
            Some(2.0),
        );
        let r = d.render(&RenderOpts::default()).unwrap();
        let desc = describe(&r.audio);
        for f in [desc.flatness.start, desc.flatness.middle, desc.flatness.end] {
            assert!(f > 0.9, "flatness {f}");
        }
        assert!(
            desc.pitch_hz.is_none() || desc.voiced < 0.2,
            "noise has no pitch"
        );
        assert!(desc.harmonicity < 0.5);
    }

    #[test]
    fn true_peak_sees_between_samples() {
        // A sine at a quarter of the rate, 45 degrees off: samples at +-0.707,
        // the waveform peaks at 1.
        let x: Vec<f32> = (0..4800)
            .map(|i| (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin())
            .collect();
        let sample_peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let tp = loudness::true_peak(&[x]);
        assert!((sample_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!(tp > 0.95, "true peak {tp}");
    }

    #[test]
    fn silent_edges_fails_on_a_buffer_cut_mid_wave() {
        let cut = bare_doc(
            vec![
                json!({"type": "osc", "name": "o", "freq": 440, "phase": 0.25, "amp": 0.5}),
                json!({"type": "output", "name": "out", "input": "o", "fade_in": 0, "fade_out": 0}),
            ],
            Some(0.1013),
        );
        let r = check(&cut, expect(json!({"silent_edges": true})));
        assert!(!r[0].pass, "{:?}", r[0]);
        assert!(
            r[0].detail.contains("fade_in") && r[0].detail.contains("fade_out"),
            "{}",
            r[0].detail
        );
        // The output's default fades fix it.
        let faded = bare_doc(
            vec![
                json!({"type": "osc", "name": "o", "freq": 440, "phase": 0.25, "amp": 0.5}),
                json!({"type": "output", "name": "out", "input": "o"}),
            ],
            Some(0.1013),
        );
        let r = check(&faded, expect(json!({"silent_edges": true})));
        assert!(r[0].pass, "{:?}", r[0]);
    }

    #[test]
    fn the_click_detector_finds_a_step() {
        let mut x = sine(200.0, 0.3, 0.5);
        assert!(clicks(&x, SR).is_empty());
        x[12_000] += 0.5;
        let t = clicks(&x, SR);
        assert!(!t.is_empty() && (t[0] - 0.25).abs() < 0.001, "{t:?}");
    }

    #[test]
    fn loop_seam_passes_a_periodic_loop_and_fails_a_broken_one() {
        let mk = |hz: f32| {
            let mut d = bare_doc(
                vec![
                    json!({"type": "osc", "name": "o", "freq": hz, "amp": 0.5}),
                    json!({"type": "output", "name": "out", "input": "o"}),
                ],
                Some(1.0),
            );
            d.kind = Kind::Loop;
            d
        };
        let r = check(&mk(220.0), expect(json!({"loop_seam": true})));
        assert!(r[0].pass, "{:?}", r[0]);
        // Half a cycle off: no jump but a kink. A quarter off: a jump.
        for hz in [220.5, 220.25] {
            let r = check(&mk(hz), expect(json!({"loop_seam": true})));
            assert!(!r[0].pass, "{hz}: {:?}", r[0]);
        }
    }

    #[test]
    fn pitch_bands_onsets_and_duration_checks() {
        let d = bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [0.0, 0.3, 0.6]}),
                json!({"type": "resonators", "name": "r", "input": "i", "modes": [[440, 0.2, 0.5]]}),
                json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
            ],
            None,
        );
        let r = check(
            &d,
            expect(json!({
                "pitch": {"hz": 440, "tol_cents": 10},
                "bands": [{"band": "500", "min_db": -40}, {"band": "4k", "max_db": -60}, {"band": 440, "segment": 0, "min_db": -40}],
                "onsets": {"min": 3, "max": 3},
                "duration": {"min": 0.7, "max": 1.0},
                "lufs": {"min": -21, "max": -19},
            })),
        );
        for c in &r {
            assert!(c.pass, "{c:?}");
        }
        let wrong = check(&d, expect(json!({"pitch": {"hz": 500}})));
        assert!(
            !wrong[0].pass && wrong[0].detail.contains("scale the frequencies"),
            "{:?}",
            wrong[0]
        );
    }

    #[test]
    fn defaults_and_variations_and_rebuild() {
        let d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n", "color": "pink"}),
                json!({"type": "wander", "name": "w", "rate": 12, "range": [400, 2500]}),
                json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": "w", "q": 3}),
                json!({"type": "decay", "name": "e", "time": 0.25}),
                json!({"type": "math", "name": "m", "op": "mul", "a": "f", "b": "e"}),
                json!({"type": "output", "name": "out", "input": "m", "lufs": -20}),
            ],
            None,
        );
        for c in check(&d, Expect::defaults()) {
            assert!(c.pass, "{c:?}");
        }
        let v = check(
            &d,
            expect(json!({"variations": {"seeds": 4, "lufs_spread": 1.5}})),
        );
        assert!(v[0].pass, "{:?}", v[0]);
        // A sound with no randomness has identical seeds: fails min_distance.
        let fixed = bare_doc(
            vec![
                json!({"type": "osc", "name": "o", "freq": 300}),
                json!({"type": "decay", "name": "e", "time": 0.2}),
                json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
                json!({"type": "output", "name": "out", "input": "m"}),
            ],
            None,
        );
        let v = check(&fixed, expect(json!({"variations": true})));
        assert!(
            !v[0].pass && v[0].detail.contains("sound alike"),
            "{:?}",
            v[0]
        );
    }

    #[test]
    fn unknown_checks_are_rejected() {
        let e = serde_json::from_value::<Expect>(json!({"peek_db": {"max": 0}})).unwrap_err();
        assert!(e.to_string().contains("peek_db"), "{e}");
    }

    #[test]
    fn the_listen_image_fits_the_budget() {
        let a = Audio::mono(SR, sine(440.0, 0.5, 0.5));
        let b = Audio {
            sample_rate: SR,
            channels: vec![sine(220.0, 0.3, 1.5), sine(330.0, 0.3, 1.5)],
        };
        for cols in [1usize, 4] {
            let columns: Vec<Column> = (0..cols)
                .map(|k| Column {
                    label: format!("seed {k}"),
                    audio: if k % 2 == 0 { &a } else { &b },
                })
                .collect();
            let png = listen_png(&columns, View::default()).unwrap();
            assert_eq!(&png[1..4], b"PNG");
            let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]) as usize;
            let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]) as usize;
            assert!(
                w * h <= sfx_analysis::image::MAX_PIXELS,
                "{cols} columns: {w}x{h}"
            );
        }
        // Zoom.
        let png = listen_png(
            &[Column {
                label: "zoom".into(),
                audio: &a,
            }],
            View {
                t: Some([0.1, 0.2]),
                hz: Some([200.0, 2000.0]),
            },
        )
        .unwrap();
        assert!(png.len() > 1000);
    }

    fn knob(
        name: &str,
        claim: (sfx_graph::Measure, i8),
        map: Vec<sfx_graph::MapEntry>,
    ) -> sfx_graph::Macro {
        sfx_graph::Macro {
            name: name.into(),
            value: 0.5,
            default: Some(0.5),
            doc: String::new(),
            claim: Some(sfx_graph::Claim(claim.0, claim.1)),
            game: false,
            map,
        }
    }

    fn entry(node: &str, field: &str, from: f64, to: f64, exp: bool) -> sfx_graph::MapEntry {
        sfx_graph::MapEntry {
            node: node.into(),
            field: field.into(),
            from,
            to,
            curve: if exp {
                sfx_graph::MapCurve::Exp
            } else {
                sfx_graph::MapCurve::Linear
            },
            param: None,
            col: None,
            index: None,
            key: None,
        }
    }

    #[test]
    fn the_macros_check_keeps_names_honest() {
        use sfx_graph::Measure;
        let mut d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n", "color": "white", "amp": 0.3}),
                json!({"type": "decay", "name": "e", "time": 0.3}),
                json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
                json!({"type": "filter", "name": "f", "input": "m", "mode": "lp", "freq": 1000, "q": 1}),
                json!({"type": "filter", "name": "h", "input": "f", "mode": "hp", "freq": 100}),
                json!({"type": "output", "name": "out", "input": "h", "lufs": -20}),
            ],
            None,
        );
        d.macros = vec![
            // Opening the filter raises the centroid: an honest name.
            knob(
                "brightness",
                (Measure::Centroid, 1),
                vec![entry("f", "freq", 300.0, 8000.0, true)],
            ),
            // A high-pass opened up raises the centroid, but claimed "weight"
            // (centroid falls): the check fails.
            knob(
                "weight",
                (Measure::Centroid, -1),
                vec![entry("h", "freq", 100.0, 6000.0, true)],
            ),
            knob(
                "length",
                (Measure::Decay, 1),
                vec![entry("e", "time", 0.05, 0.5, true)],
            ),
            // Claimed to change decay but wired to the filter only: fails.
            knob(
                "ring",
                (Measure::Decay, 1),
                vec![entry("f", "q", 0.7, 0.8, false)],
            ),
        ];
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap();
        let res = run(
            &Expect::defaults_for(&d),
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap();
        let by = |n: &str| {
            res.iter()
                .find(|c| c.name == format!("macro {n}"))
                .unwrap_or_else(|| panic!("{n}: {res:?}"))
        };
        assert!(by("brightness").pass, "{:?}", by("brightness"));
        assert!(by("length").pass, "{:?}", by("length"));
        let w = by("weight");
        assert!(
            !w.pass && w.detail.contains("does not fall") && w.detail.contains("rename the macro"),
            "{w:?}"
        );
        assert!(!by("ring").pass, "{:?}", by("ring"));
        // A sound with no macros gets no macro check by default.
        d.macros.clear();
        assert!(Expect::defaults_for(&d).macros.is_none());
    }
}

mod synthesis {
    //! The senses for synthesized sources (aliasing, inharmonicity,
    //! modulation, roughness) and the sturdier `decay` / `swing` measures.

    use serde_json::{Value, json};
    use sfx_analysis::measure::{
        aliasing_db, amplitude_modulation, extra, inharmonicity_cents, measure, pitch_modulation,
        roughness_pct,
    };
    use sfx_analysis::{CheckCx, CheckResult, Expect, run};
    use sfx_graph::{Audio, Measure, RenderOpts, SoundDoc};

    fn doc_at(nodes: Vec<Value>, secs: f32, sr: u32) -> SoundDoc {
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "src"}));
        d.duration = Some(secs);
        d.sample_rate = sr;
        d
    }

    fn check(d: &SoundDoc, e: Value) -> Vec<CheckResult> {
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
        let e: Expect = serde_json::from_value(e).unwrap_or_else(|e| panic!("{e}"));
        run(
            &e,
            &CheckCx {
                doc: d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn render(d: &SoundDoc) -> Audio {
        d.render(&RenderOpts::default()).unwrap().audio
    }

    fn mono(sr: u32, secs: f32, f: impl Fn(f32) -> f32) -> Audio {
        Audio::mono(
            sr,
            (0..(secs * sr as f32) as usize)
                .map(|i| f(i as f32 / sr as f32))
                .collect(),
        )
    }

    const TAU: f32 = std::f32::consts::TAU;

    /// The `decay` formula before S8a (10 ms frames, last frame within 6 dB of
    /// the peak to the last within 40 dB).
    fn old_decay(audio: &Audio) -> f32 {
        let x = audio.mix_mono();
        let fr = (audio.sample_rate / 100).max(1) as usize;
        let lev: Vec<f32> = x
            .chunks(fr)
            .map(|c| {
                10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
                    .max(1e-20)
                    .log10()
            })
            .collect();
        let top = lev.iter().copied().reduce(f32::max).unwrap();
        let last = |drop: f32| lev.iter().rposition(|v| *v > top - drop).unwrap_or(0);
        (last(40.0).saturating_sub(last(6.0))) as f32 / 100.0
    }

    fn old_swing(audio: &Audio) -> f32 {
        let lc: Vec<f32> =
            sfx_graph::loudness::curve(&audio.channels, audio.sample_rate, 0.2, 0.05)
                .into_iter()
                .filter(|v| *v > -70.0)
                .collect();
        if lc.len() < 3 {
            return 0.0;
        }
        let mut s = lc;
        s.sort_by(f32::total_cmp);
        s[s.len() * 9 / 10] - s[s.len() / 10]
    }

    fn bounce(restitution: f32, first: f32) -> Audio {
        let d = SoundDoc {
            nodes: vec![
                json!({"type": "bounce", "name": "ev", "first": first, "restitution": restitution}),
                json!({"type": "osc", "name": "o", "freq": 180, "amp": 0.8}),
                json!({"type": "decay", "name": "e", "time": 0.04}),
                json!({"type": "math", "name": "thump", "op": "mul", "a": "o", "b": "e"}),
                json!({"type": "grains", "name": "src", "source": "thump", "events": "ev", "size": 0.15}),
                json!({"type": "output", "name": "out", "input": "src"}),
            ],
            duration: Some(3.0),
            ..SoundDoc::named("t")
        };
        render(&d)
    }

    /// White noise rising by `depth` dB over its `secs` (a crescendo / swell).
    fn swell(depth: f32, secs: f32) -> Audio {
        let d = SoundDoc {
            nodes: vec![
                json!({"type": "noise", "name": "n", "color": "white", "amp": 0.5}),
                json!({"type": "expr", "name": "src", "inputs": {"a": "n"},
                   "out": format!("a * pow(10, 0 - {depth} * (1 - t / {secs}) / 20)")}),
                json!({"type": "output", "name": "out", "input": "src", "fade_in": 0, "fade_out": 0}),
            ],
            duration: Some(secs),
            ..SoundDoc::named("t")
        };
        render(&d)
    }

    /// A click: noise times a decay of `time` seconds.
    fn click(time: f32, secs: f32) -> Audio {
        let d = SoundDoc {
            nodes: vec![
                json!({"type": "noise", "name": "n", "color": "white", "amp": 0.5}),
                json!({"type": "decay", "name": "e", "time": time}),
                json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "e"}),
                json!({"type": "output", "name": "out", "input": "src"}),
            ],
            duration: Some(secs),
            ..SoundDoc::named("t")
        };
        render(&d)
    }

    fn line<'a>(res: &'a [CheckResult], name: &str) -> &'a CheckResult {
        res.iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name}: {res:?}"))
    }

    fn naive_saw(hz: f32, sr: u32) -> SoundDoc {
        doc_at(
            vec![json!({"type": "expr", "name": "src", "inputs": {},
                    "out": format!("0.5 * (2 * fract(t * {hz}) - 1)")})],
            1.0,
            sr,
        )
    }

    fn osc_saw(hz: f32, sr: u32) -> SoundDoc {
        doc_at(
            vec![json!({"type": "osc", "name": "src", "shape": "saw", "freq": hz, "amp": 0.5})],
            1.0,
            sr,
        )
    }

    #[test]
    fn a_naive_saw_at_3khz_fails_aliasing_and_the_osc_saw_passes() {
        let (bad, good) = (naive_saw(3000.0, 44_100), osc_saw(3000.0, 44_100));
        let (vb, vg) = (
            aliasing_db(&render(&bad)).unwrap(),
            aliasing_db(&render(&good)).unwrap(),
        );
        println!(
            "3 kHz saw at 44.1 kHz: naive (expr) {vb:.1} dB, osc {vg:.1} dB off the harmonic grid above 4 kHz"
        );
        assert!(vb > -15.0 && vg < -20.0 && vb - vg > 10.0, "{vb} {vg}");
        let e = json!({"aliasing": {"max_db": -20}});
        let r = check(&bad, e.clone());
        let l = line(&r, "aliasing");
        println!("{}", l.detail);
        assert!(!l.pass && l.detail.contains("likely aliasing"), "{l:?}");
        let r = check(&good, e);
        assert!(line(&r, "aliasing").pass, "{r:?}");
        // At musical pitches the default (-35 dB) separates them too.
        let (vb, vg) = (
            aliasing_db(&render(&naive_saw(440.0, 48_000))).unwrap(),
            aliasing_db(&render(&osc_saw(440.0, 48_000))).unwrap(),
        );
        println!("440 Hz saw at 48 kHz: naive {vb:.1} dB, osc {vg:.1} dB");
        assert!(vb > -30.0 && vg < -35.0, "{vb} {vg}");
        let r = check(&naive_saw(440.0, 48_000), json!({"aliasing": true}));
        assert!(!line(&r, "aliasing").pass);
        let r = check(&osc_saw(440.0, 48_000), json!({"aliasing": true}));
        assert!(line(&r, "aliasing").pass, "{r:?}");
    }

    #[test]
    fn aliasing_names_the_shape_node_to_oversample() {
        let d = doc_at(
            vec![
                json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.9}),
                json!({"type": "shape", "name": "src", "input": "tone", "mode": "fold", "drive": 6}),
            ],
            1.0,
            44_100,
        );
        let v = aliasing_db(&render(&d)).unwrap();
        println!("1 kHz sine folded (drive 6) at 44.1 kHz: {v:.1} dB");
        let r = check(&d, json!({"aliasing": {"max_db": -60}}));
        let l = line(&r, "aliasing");
        println!("{}", l.detail);
        assert!(
            !l.pass && l.detail.contains("raise `oversample` on shape `src`"),
            "{l:?}"
        );
        // A pure sine has none.
        let sine = mono(48_000, 1.0, |t| 0.5 * (TAU * 1000.0 * t).sin());
        let v = aliasing_db(&sine).unwrap();
        println!("pure 1 kHz sine: {v:.1} dB");
        assert!(v < -55.0, "{v}");
        // A clean sweep reads clean (only its steadiest frames count).
        let sweep = mono(48_000, 1.0, |t| {
            0.5 * (TAU * (200.0 * t + 400.0 * t * t)).sin()
        });
        println!("sweep: {:?}", aliasing_db(&sweep));
        assert!(aliasing_db(&sweep).is_none_or(|v| v < -55.0));
        // White noise has no pitch to measure.
        let noise = render(&doc_at(
            vec![json!({"type": "noise", "name": "src", "color": "white", "amp": 0.3})],
            1.0,
            48_000,
        ));
        println!("noise: {:?}", aliasing_db(&noise));
        assert!(aliasing_db(&noise).is_none());
    }

    #[test]
    fn inharmonicity_tells_a_harmonic_tone_from_a_bell() {
        let sr = 48_000;
        let harmonic = mono(sr, 1.0, |t| {
            (1..=6)
                .map(|k| (TAU * 200.0 * k as f32 * t).sin() / k as f32)
                .sum::<f32>()
                * 0.3
        });
        let bell = mono(sr, 1.0, |t| {
            [1.0f32, 2.1, 3.3]
                .iter()
                .map(|k| (TAU * 200.0 * k * t).sin() * 0.3)
                .sum::<f32>()
        });
        let (h, b) = (
            inharmonicity_cents(&harmonic).unwrap(),
            inharmonicity_cents(&bell).unwrap(),
        );
        println!(
            "inharmonicity: 6 harmonics of 200 Hz {h:.2} cents; partials 1, 2.1, 3.3 x 200 Hz {b:.1} cents"
        );
        assert!(h < 2.0, "{h}");
        assert!(b > 50.0, "{b}");
        // A noisy sound has no clear partials.
        let noise = render(&doc_at(
            vec![json!({"type": "noise", "name": "src", "color": "white", "amp": 0.3})],
            1.0,
            sr,
        ));
        println!(
            "inharmonicity of white noise: {:?}",
            inharmonicity_cents(&noise)
        );
        // Through the check, on a rendered sound.
        let d = doc_at(
            vec![json!({"type": "resonators", "name": "src", "input": "hit",
                    "modes": [[300, 0.8, 1.0], [630, 0.6, 0.8], [990, 0.5, 0.6]]})],
            1.5,
            48_000,
        );
        let mut d = d;
        d.nodes.insert(0, json!({"type": "impulse", "name": "hit"}));
        let r = check(&d, json!({"inharmonicity": {"min": 50}}));
        println!("{}", line(&r, "inharmonicity").detail);
        assert!(line(&r, "inharmonicity").pass, "{r:?}");
        let r = check(&d, json!({"inharmonicity": {"max": 10}}));
        let l = line(&r, "inharmonicity");
        println!("{}", l.detail);
        assert!(
            !l.pass && l.detail.contains("off integer multiples"),
            "{l:?}"
        );
    }

    #[test]
    fn a_6hz_tremolo_of_6db_reads_6hz_and_6db() {
        let sr = 48_000;
        // 6 dB peak-to-peak, sinusoidal in dB (+-3 dB).
        let trem = mono(sr, 2.0, |t| {
            (TAU * 1000.0 * t).sin() * 10f32.powf(3.0 * (TAU * 6.0 * t).sin() / 20.0) * 0.2
        });
        let m = amplitude_modulation(&trem);
        println!("tremolo 6 Hz, +-3 dB: {m:?}");
        assert!(
            (m[0].rate_hz - 6.0).abs() < 0.2 && (m[0].depth - 6.0).abs() < 1.0,
            "{m:?}"
        );
        // A linear tremolo whose gain runs 0.5 .. 1 (6.02 dB between the extremes).
        let lin = mono(sr, 2.0, |t| {
            (TAU * 1000.0 * t).sin() * (0.75 + 0.25 * (TAU * 6.0 * t).sin()) * 0.5
        });
        let m = amplitude_modulation(&lin);
        println!("tremolo 6 Hz, gain 0.5..1: {m:?}");
        assert!(
            (m[0].rate_hz - 6.0).abs() < 0.2 && (m[0].depth - 6.0).abs() < 1.0,
            "{m:?}"
        );
        // Two modulators: both rates are listed, strongest first.
        let two = mono(sr, 3.0, |t| {
            (TAU * 800.0 * t).sin()
                * 10f32.powf((3.0 * (TAU * 4.0 * t).sin() + 1.5 * (TAU * 17.0 * t).sin()) / 20.0)
                * 0.2
        });
        let m = amplitude_modulation(&two);
        println!("4 Hz +-3 dB and 17 Hz +-1.5 dB: {m:?}");
        assert!(
            m.len() >= 2 && (m[0].rate_hz - 4.0).abs() < 0.2 && (m[1].rate_hz - 17.0).abs() < 0.3
        );
        assert!((m[1].depth - 3.0).abs() < 0.7, "{m:?}");
        // A steady tone has none; an exponential decay is detrended away.
        let steady = mono(sr, 2.0, |t| (TAU * 1000.0 * t).sin() * 0.3);
        assert!(amplitude_modulation(&steady).is_empty());
        let decay = mono(sr, 2.0, |t| (TAU * 1000.0 * t).sin() * (-t * 3.0).exp());
        println!("decaying tone: {:?}", amplitude_modulation(&decay));
        assert!(amplitude_modulation(&decay).iter().all(|p| p.depth < 1.0));
        // Through the check on a rendered sound.
        let d = doc_at(
            vec![
                json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.4}),
                json!({"type": "lfo", "name": "lf", "freq": 6, "range": [0.5, 1.0]}),
                json!({"type": "math", "name": "src", "op": "mul", "a": "tone", "b": "lf"}),
            ],
            2.0,
            48_000,
        );
        let r = check(&d, json!({"modulation": {"rate_hz": 6.0, "tol_hz": 0.3}}));
        println!("{}", line(&r, "modulation").detail);
        let r = check(&d, json!({"modulation": {"rate_hz": 9.0}}));
        let l = line(&r, "modulation");
        println!("{}", l.detail);
        assert!(!l.pass && l.detail.contains("no rate within"), "{l:?}");
    }

    #[test]
    fn a_5hz_vibrato_reads_5hz_in_the_pitch_track() {
        let sr = 48_000;
        let mut ph = 0.0f32;
        let vib = Audio::mono(
            sr,
            (0..2 * sr as usize)
                .map(|i| {
                    let t = i as f32 / sr as f32;
                    let f = 440.0 * 2f32.powf(30.0 * (TAU * 5.0 * t).sin() / 1200.0);
                    ph += TAU * f / sr as f32;
                    ph.sin() * 0.5
                })
                .collect(),
        );
        let m = pitch_modulation(&vib);
        println!("vibrato 5 Hz +-30 cents: {m:?}");
        assert!(
            (m[0].rate_hz - 5.0).abs() < 0.3 && (m[0].depth - 60.0).abs() < 8.0,
            "{m:?}"
        );
        let steady = mono(sr, 2.0, |t| (TAU * 440.0 * t).sin() * 0.5);
        assert!(pitch_modulation(&steady).is_empty());
    }

    #[test]
    fn two_sines_40hz_apart_are_rougher_than_two_400hz_apart() {
        let sr = 48_000;
        let pair = |a: f32, b: f32| {
            mono(sr, 1.0, |t| {
                ((TAU * a * t).sin() + (TAU * b * t).sin()) * 0.25
            })
        };
        let near = roughness_pct(&pair(1000.0, 1040.0));
        let far = roughness_pct(&pair(1000.0, 1400.0));
        let low_near = roughness_pct(&pair(400.0, 440.0));
        let one = roughness_pct(&pair(1000.0, 1000.0));
        println!(
            "roughness: 40 Hz apart {near:.2} %, {low_near:.2} % (at 400 Hz); 400 Hz apart {far:.3} %; one tone {one:.3} %"
        );
        assert!(near > 5.0 && near > 20.0 * far.max(0.01), "{near} {far}");
        let d = doc_at(
            vec![
                json!({"type": "osc", "name": "a", "freq": 1000, "amp": 0.3}),
                json!({"type": "osc", "name": "b", "freq": 1040, "amp": 0.3}),
                json!({"type": "mix", "name": "src", "inputs": ["a", "b"]}),
            ],
            1.0,
            48_000,
        );
        let r = check(&d, json!({"roughness": {"min": 5}}));
        println!("{}", line(&r, "roughness").detail);
        assert!(line(&r, "roughness").pass);
        let r = check(&d, json!({"roughness": {"max": 2}}));
        assert!(!line(&r, "roughness").pass);
    }

    #[test]
    fn the_extra_senses_are_deterministic_finite_and_survive_degenerate_input() {
        let sr = 48_000;
        let a = mono(sr, 1.0, |t| {
            (TAU * 220.0 * t).sin() * (1.0 + 0.5 * (TAU * 7.0 * t).sin()) * 0.3
        });
        let (p, q) = (extra(&a), extra(&a));
        assert_eq!(format!("{p:?}"), format!("{q:?}"));
        for line in p.lines() {
            println!("{line}");
        }
        for audio in [
            Audio::mono(sr, vec![]),
            Audio::mono(sr, vec![0.0; 10]),
            Audio::mono(sr, vec![0.0; 48_000]),
            Audio::mono(sr, vec![1.0; 5000]),
            Audio::mono(sr, vec![f32::MAX / 4.0; 3000]),
            Audio::mono(8000, vec![0.1; 40_000]),
        ] {
            let e = extra(&audio);
            assert!(e.roughness_pct.is_finite());
            assert!(e.aliasing_db.is_none_or(f32::is_finite));
            assert!(e.inharmonicity_cents.is_none_or(f32::is_finite));
            for m in e.amp_mod.iter().chain(&e.pitch_mod) {
                assert!(m.rate_hz.is_finite() && m.depth.is_finite());
            }
            for m in [Measure::Decay, Measure::Swing] {
                assert!(measure(&audio, m).is_finite());
            }
        }
    }

    #[test]
    fn a_bad_modulation_mode_is_an_error_that_says_what_to_use() {
        let d = osc_saw(440.0, 48_000);
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap();
        let e: Expect =
            serde_json::from_value(json!({"modulation": {"rate_hz": 5, "mode": "phase"}})).unwrap();
        let err = run(
            &e,
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_err();
        println!("{err}");
        assert!(err.contains("\"amplitude\" or \"pitch\""), "{err}");
    }

    // ---- decay and swing ------------------------------------------------------

    #[test]
    fn decay_and_swing_keep_their_values_on_sounds_that_already_worked() {
        // One-hit sounds of 0.5 s and longer (decaying noise at several
        // lengths, a pitched ring) read exactly as before.
        for (time, secs) in [(0.05f32, 0.6f32), (0.1, 1.0), (0.3, 2.0), (0.8, 4.0)] {
            let a = click(time, secs);
            let (n, o) = (measure(&a, Measure::Decay), old_decay(&a));
            println!("decay noise x decay({time}) over {secs} s: new {n:.3} old {o:.3}");
            assert_eq!(n.to_bits(), o.to_bits(), "{time} {secs}");
        }
        let ring = mono(48_000, 2.0, |t| {
            (TAU * 600.0 * t).sin() * (-t * 6.0).exp() * 0.5
        });
        assert_eq!(
            measure(&ring, Measure::Decay).to_bits(),
            old_decay(&ring).to_bits()
        );
        // Swing: identical from 0.6 s up.
        for secs in [0.6f32, 1.0, 2.5] {
            for depth in [0.0f32, 9.0, 24.0] {
                let a = swell(depth, secs);
                let (n, o) = (measure(&a, Measure::Swing), old_swing(&a));
                assert_eq!(n.to_bits(), o.to_bits(), "{secs} {depth}: {n} {o}");
            }
        }
    }

    #[test]
    fn decay_of_a_bouncing_ball_grows_with_restitution_and_with_the_first_gap() {
        let by_r: Vec<(f32, f32)> = [0.5f32, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8]
            .iter()
            .map(|r| {
                let a = bounce(*r, 0.3);
                (measure(&a, Measure::Decay), old_decay(&a))
            })
            .collect();
        println!("bounce restitution 0.50..0.80 (new, old): {by_r:?}");
        assert!(by_r.windows(2).all(|w| w[1].0 > w[0].0), "{by_r:?}");
        assert!(
            by_r.windows(2).any(|w| w[1].1 <= w[0].1),
            "the old measure was monotone here; the test no longer shows the fix"
        );
        let by_f: Vec<f32> = [0.15f32, 0.2, 0.25, 0.3, 0.35, 0.4]
            .iter()
            .map(|f| measure(&bounce(0.6, *f), Measure::Decay))
            .collect();
        println!("bounce first gap 0.15..0.40: {by_f:?}");
        assert!(by_f.windows(2).all(|w| w[1] > w[0]), "{by_f:?}");
    }

    #[test]
    fn decay_and_swing_resolve_short_sounds() {
        // A 0.1 s click sweeping its decay time: decay never falls and ends
        // higher; the 10 ms frames of before read most of these as 0 or 0.01.
        let v: Vec<f32> = [0.004f32, 0.008, 0.012, 0.016, 0.02, 0.03]
            .iter()
            .map(|t| measure(&click(*t, 0.3), Measure::Decay))
            .collect();
        let o: Vec<f32> = [0.004f32, 0.008, 0.012, 0.016, 0.02, 0.03]
            .iter()
            .map(|t| old_decay(&click(*t, 0.3)))
            .collect();
        println!("decay of a 0.3 s click, decay time 4..30 ms: new {v:?} old {o:?}");
        assert!(
            v.windows(2).all(|w| w[1] >= w[0]) && v[5] > v[1] * 2.0,
            "{v:?}"
        );
        // Swing of a 0.1 s swell: 4 distinct growing values where the old
        // measure had under 3 windows and read 0.
        let s: Vec<f32> = [0.0f32, 6.0, 12.0, 18.0, 24.0]
            .iter()
            .map(|d| measure(&swell(*d, 0.1), Measure::Swing))
            .collect();
        let o: Vec<f32> = [0.0f32, 6.0, 12.0, 18.0, 24.0]
            .iter()
            .map(|d| old_swing(&swell(*d, 0.1)))
            .collect();
        println!("swing of a 0.1 s swell of 0..24 dB: new {s:?} old {o:?}");
        assert!(s.windows(2).all(|w| w[1] > w[0] + 2.0), "{s:?}");
        assert!(o.iter().all(|v| *v == 0.0));
    }
}

mod onset_detail {
    //! Per-onset detail in `listen` (-60 dB column, `n/a` pitch, glide,
    //! ignored slow modes, inharmonicity from partial ratios, close pairs).

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
}
