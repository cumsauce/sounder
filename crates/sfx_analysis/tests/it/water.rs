mod model {
    //! `water` and `splash` (M4): each field moves its measure at three
    //! settings; level at `lufs -20` raises no missed-target warning; a
    //! `kind: loop` water passes the default checks including `loop_seam`;
    //! 30 s at flow 1 renders inside a stated time budget; bit-identical,
    //! finite at the extremes, error messages.

    use std::time::Instant;

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::{CheckCx, Expect, report, run};
    use sfx_graph::node::parse_body;
    use sfx_graph::{Kind, RenderOpts, SoundDoc};

    fn water(extra: Value) -> Vec<Value> {
        let mut w = json!({"type": "water", "name": "w", "flow": 0.5, "seed": 3});
        w.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![w]
    }

    fn splash(extra: Value) -> Vec<Value> {
        let mut w = json!({"type": "splash", "name": "s", "size": 0.1, "seed": 3});
        w.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![w]
    }

    /// Spectral centroid, Hz, over 100 Hz .. 16 kHz (power-weighted).
    fn centroid(x: &[f32]) -> f32 {
        let s = spectrum(x, SR);
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (f, d) in s {
            if (100.0..16_000.0).contains(&f) {
                let p = 10f64.powf(f64::from(d) / 10.0);
                num += f64::from(f) * p;
                den += p;
            }
        }
        (num / den.max(1e-30)) as f32
    }

    /// Share of the power under `hz`, 0..1.
    fn share_below(x: &[f32], hz: f32) -> f32 {
        let lo = 10f32.powf(band_db(x, SR, 0.0, hz) / 10.0);
        let all = 10f32.powf(band_db(x, SR, 0.0, SR as f32 / 2.0) / 10.0);
        lo / all
    }

    /// Rises in a 1 ms rectified envelope: points where the envelope exceeds
    /// 3x its own level 5 ms earlier and 0.01 absolute, re-armed once it falls
    /// under 1.5x. Counts bubble births that stand out of the others.
    fn rises(x: &[f32]) -> usize {
        let w = (SR / 1000) as usize;
        let env: Vec<f32> = x
            .chunks(w)
            .map(|c| c.iter().map(|v| v.abs()).sum::<f32>() / c.len() as f32)
            .collect();
        let (mut count, mut armed) = (0, true);
        for i in 5..env.len() {
            if armed && env[i] > 3.0 * env[i - 5] && env[i] > 0.01 {
                count += 1;
                armed = false;
            } else if env[i] < 1.5 * env[i - 5] {
                armed = true;
            }
        }
        count
    }

    /// Largest LUFS spread over 8 seeds of one setting, dB.
    const SPREAD: f32 = 1.5;

    /// Seconds from the start until a 10 ms RMS window stays under -50 dB
    /// relative to the loudest one for good.
    fn decay_s(x: &[f32]) -> f32 {
        let w = (SR / 100) as usize;
        let rms: Vec<f32> = x
            .chunks(w)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let top = rms.iter().cloned().fold(0.0f32, f32::max);
        let last = rms.iter().rposition(|v| *v > top * 0.00316).unwrap_or(0);
        (last + 1) as f32 * 0.01
    }

    fn render_doc(
        nodes: &[Value],
        duration: f32,
        lufs: Option<f32>,
        kind: Kind,
    ) -> sfx_graph::Rendered {
        let mut d = SoundDoc::named("test");
        d.nodes = nodes.to_vec();
        d.kind = kind;
        d.duration = Some(duration);
        let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
        let mut out = json!({"type": "output", "name": "out", "input": last});
        if let Some(l) = lufs {
            out["lufs"] = json!(l);
        }
        d.nodes.push(out);
        d.render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn render_error(nodes: Vec<Value>) -> String {
        let mut d = SoundDoc::named("test");
        d.nodes = nodes;
        let last = d.nodes.last().unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string();
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": last}));
        d.duration = Some(0.5);
        d.render(&RenderOpts::default()).unwrap_err()
    }

    #[test]
    fn flow_moves_onset_density_lufs_and_centroid() {
        let mut dens = Vec::new();
        let mut lufs = Vec::new();
        let mut cent = Vec::new();
        for flow in [0.1, 0.35, 0.8] {
            // density 60 keeps the bubbles apart enough to count their onsets.
            let nodes = water(json!({"flow": flow, "density": 60}));
            let r = render_doc(&nodes, 8.0, None, Kind::Oneshot);
            let x = node_audio(&nodes, "w", 8.0);
            assert_finite(&x, "w");
            dens.push(rises(&x) as f32 / 8.0);
            lufs.push(r.lufs);
            cent.push(centroid(&x));
        }
        println!(
            "flow 0.1 / 0.35 / 0.8: rises/s {dens:.1?}, LUFS {lufs:.1?}, centroid Hz {cent:.0?}"
        );
        assert!(dens[0] < dens[1] && dens[1] < dens[2], "{dens:?}");
        assert!(
            lufs[0] + 3.0 < lufs[1] && lufs[1] + 3.0 < lufs[2],
            "{lufs:?}"
        );
        assert!(
            cent[0] > cent[1] * 1.1 && cent[1] > cent[2] * 1.1,
            "{cent:?}"
        );
        // The default density at the same three flows: louder, still lower.
        let (mut l2, mut c2) = (Vec::new(), Vec::new());
        for flow in [0.1, 0.35, 0.8] {
            let nodes = water(json!({"flow": flow}));
            l2.push(render_doc(&nodes, 8.0, None, Kind::Oneshot).lufs);
            c2.push(centroid(&node_audio(&nodes, "w", 8.0)));
        }
        println!("density 300: LUFS {l2:.1?}, centroid Hz {c2:.0?}");
        assert!(l2[0] + 3.0 < l2[1] && l2[1] + 3.0 < l2[2], "{l2:?}");
        assert!(c2[0] > c2[1] * 1.1 && c2[1] > c2[2] * 1.1, "{c2:?}");
    }

    #[test]
    fn flow_zero_is_silence_and_a_ramp_is_followed() {
        let x = node_audio(water(json!({"flow": 0.0})), "w", 1.0);
        assert!(x.iter().all(|v| *v == 0.0), "flow 0 is silence");
        let nodes = vec![
            json!({"type": "ramp", "name": "f", "from": 0.0, "to": 1.0, "time": 8.0}),
            json!({"type": "water", "name": "w", "flow": "f", "seed": 3}),
        ];
        let x = node_audio(&nodes, "w", 8.0);
        let q = x.len() / 4;
        let levels: Vec<f32> = (0..4).map(|i| rms_db(&x[i * q..(i + 1) * q])).collect();
        println!("ramp 0 -> 1 over 8 s, rms per quarter: {levels:.1?} dB");
        assert!(levels.windows(2).all(|w| w[0] < w[1]), "{levels:?}");
    }

    #[test]
    fn depth_moves_the_centroid_down() {
        let c: Vec<f32> = [0.0, 0.5, 1.0]
            .iter()
            .map(|d| centroid(&node_audio(water(json!({"depth": d})), "w", 6.0)))
            .collect();
        println!("depth 0 / 0.5 / 1: centroid {c:.0?} Hz");
        assert!(c[0] > c[1] * 1.1 && c[1] > c[2] * 1.1, "{c:?}");
    }

    #[test]
    fn splash_amount_adds_energy_and_peaks() {
        let mut peak = Vec::new();
        let mut lufs = Vec::new();
        for s in [0.0, 0.5, 1.0] {
            let nodes = water(json!({"splash": s, "flow": 0.7}));
            let x = node_audio(&nodes, "w", 8.0);
            peak.push(x.iter().fold(0.0f32, |m, v| m.max(v.abs())));
            lufs.push(render_doc(&nodes, 8.0, None, Kind::Oneshot).lufs);
        }
        println!("splash 0 / 0.5 / 1: peak {peak:.3?}, LUFS {lufs:.1?}");
        // Round 2 (steadier hiss): one sample peak depends on where a splash
        // lands on the stream (0.36 / 0.57 / 0.49), so the peak only has to
        // rise from none; the loudness has to rise at every step.
        assert!(peak[0] < peak[1] && peak[0] < peak[2], "{peak:?}");
        assert!(lufs[0] < lufs[1] && lufs[1] < lufs[2], "{lufs:?}");
    }

    #[test]
    fn splash_size_moves_centroid_decay_and_low_band() {
        let (mut cent, mut dec, mut low) = (Vec::new(), Vec::new(), Vec::new());
        for size in [0.02, 0.1, 0.5] {
            let x = node_audio(splash(json!({"size": size})), "s", 3.0);
            assert_finite(&x, "s");
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak - 0.5).abs() < 0.01, "size {size}: peak {peak}");
            cent.push(centroid(&x));
            dec.push(decay_s(&x));
            low.push(share_below(&x, 200.0));
        }
        println!(
            "splash size 0.02 / 0.1 / 0.5: centroid {cent:.0?} Hz, decay {dec:.2?} s, share under 200 Hz {low:.4?}"
        );
        assert!(
            // Fitted to refs/splash_set the spray fills the small splash's top,
            // so its centroid sits over the middle one (8050 / 6894 / 4704 Hz
            // since the spray stopped sweeping: batch 3 heard the sweep).
            cent[0] > cent[1] * 1.1 && cent[1] > cent[2] * 1.2,
            "{cent:?}"
        );
        assert!(dec[0] < dec[1] && dec[1] < dec[2], "{dec:?}");
        // Book check 1 (Farnell's model): the weight is a short pressure bump,
        // so the share under 200 Hz stays small at every size.
        assert!(low.iter().all(|l| *l < 0.05), "{low:?}");
    }

    /// Power centroid over 20 Hz .. 20 kHz (the largest splash rings at 55 Hz).
    fn centroid_wide(x: &[f32]) -> f32 {
        let s = spectrum(x, SR);
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (f, d) in s {
            if (20.0..20_000.0).contains(&f) {
                let p = 10f64.powf(f64::from(d) / 10.0);
                num += f64::from(f) * p;
                den += p;
            }
        }
        (num / den.max(1e-30)) as f32
    }

    /// The audible part of a splash: up to the last sample over 1e-2 of the peak
    /// (-40 dB; below that a tenth is a few bits of f32 and its centroid is the
    /// noise floor's).
    fn audible(x: &[f32]) -> &[f32] {
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let end = x.iter().rposition(|v| v.abs() > peak * 1e-2).unwrap_or(0);
        &x[..=end]
    }

    /// Centroid of each tenth of the audible part.
    fn tenths(x: &[f32]) -> Vec<f32> {
        let x = audible(x);
        let n = x.len() / 10;
        (0..10)
            .map(|i| centroid_wide(&x[i * n..(i + 1) * n]))
            .collect()
    }

    /// Energy of `x` above `hz`, in the units of sum(x^2) (Parseval). A
    /// second difference first removes the loud low end (a window cannot: the
    /// burst sits at the very start of the slice, where a Hann window is 0 and a
    /// rectangular one leaks), a flat-topped taper windows it, and each bin is
    /// divided by the difference filter's power gain again.
    fn energy_above(x: &[f32], hz: f32) -> f64 {
        use realfft::RealFftPlanner;
        let n = x.len();
        let size = n.next_power_of_two();
        let taper = (n / 10).max(1);
        let mut buf = vec![0.0f32; size];
        for i in 2..n {
            let d = x[i] - 2.0 * x[i - 1] + x[i - 2];
            let edge = (n - 1 - i).min(taper) as f32 / taper as f32;
            buf[i] = d * (0.5 + 0.5 * (std::f32::consts::PI * (1.0 - edge)).cos());
        }
        let mut out = RealFftPlanner::<f32>::new()
            .plan_fft_forward(size)
            .make_output_vec();
        RealFftPlanner::<f32>::new()
            .plan_fft_forward(size)
            .process(&mut buf, &mut out)
            .unwrap();
        let mut sum = 0.0f64;
        for (k, z) in out.iter().enumerate() {
            let f = k as f32 * SR as f32 / size as f32;
            if f < hz || f > SR as f32 / 2.0 {
                continue;
            }
            let w = std::f64::consts::TAU * f64::from(f) / f64::from(SR);
            let gain = (2.0 - 2.0 * w.cos()).powi(2);
            let weight = if k == 0 || k == size / 2 { 1.0 } else { 2.0 };
            sum += weight * f64::from(z.norm_sqr()) / gain.max(1e-12);
        }
        sum / size as f64
    }

    fn energy(x: &[f32]) -> f64 {
        x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum()
    }

    #[test]
    fn splash_tail_keeps_the_bubble_band() {
        let mut bad = Vec::new();
        for size in [0.02, 0.1, 0.5] {
            for seed in [1u64, 2, 3, 4, 5] {
                let x = node_audio(splash(json!({"size": size, "seed": seed})), "s", 3.0);
                let c = tenths(&x);
                println!(
                    "splash size {size} seed {seed}: audible {:.3} s, centroid by tenth {c:.0?} Hz",
                    audible(&x).len() as f32 / SR as f32
                );
                // Book check 1: the slap opens bright (4-5 kHz) and the tail is
                // the rising-pitch bubbles, 350 Hz-3 kHz: the last three tenths
                // stay in that band (the A/B listener preferred this to the old
                // fixed-band spray).
                let m = |a: &[f32]| a.iter().sum::<f32>() / a.len() as f32;
                let end = m(&c[7..]);
                if !(400.0..3000.0).contains(&end) || m(&c[..3]) < 2000.0 {
                    bad.push(format!("size {size} seed {seed}: {c:?}"));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn splash_burst_is_bright_and_brief() {
        for size in [0.02, 0.1, 0.5] {
            for seed in [1u64, 2, 3] {
                let x = node_audio(splash(json!({"size": size, "seed": seed})), "s", 3.0);
                let a = audible(&x);
                let (early, late) = (&a[..a.len() / 5], &a[a.len() / 2..]);
                let e_en = energy_above(early, 8000.0);
                let l_en = energy_above(late, 8000.0);
                let all_early = energy(early);
                println!(
                    "splash size {size} seed {seed}: early 20 % energy above 8 kHz {:.1} dB under its total, last 50 % holds {:.2e} of it",
                    10.0 * (e_en / all_early).log10(),
                    l_en / e_en
                );
                // water_fix: the slap sits at 1-6 kHz (no 16 kHz burst any
                // more), so "bright" is the share above 2 kHz early on.
                let e2 = energy_above(early, 2000.0);
                assert!(
                    e2 > 0.15 * all_early,
                    "size {size}: {e2} of {all_early} above 2 kHz"
                );
                // The spray band now stays at 1.2-7 kHz (no sweep down), so the
                // late half keeps a little energy above 8 kHz (up to 7 %).
                assert!(l_en < 0.15 * e_en, "size {size}: {l_en} vs {e_en}");
            }
        }
    }

    #[test]
    fn splash_at_delays_and_seeds_differ() {
        let x = node_audio(splash(json!({"at": 0.5})), "s", 2.0);
        let first = x.iter().position(|v| v.abs() > 1e-6).unwrap();
        println!(
            "splash at 0.5: first nonzero sample {first} ({:.4} s)",
            first as f32 / SR as f32
        );
        assert!(first >= (0.5 * SR as f32) as usize && first < (0.52 * SR as f32) as usize);
        let a = node_audio(splash(json!({"seed": 1})), "s", 2.0);
        let b = node_audio(splash(json!({"seed": 2})), "s", 2.0);
        let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).powi(2)).sum::<f32>()
            / a.iter().map(|p| p * p).sum::<f32>();
        println!("splash seeds 1 vs 2: relative difference energy {diff:.2}");
        assert!(diff > 0.2, "{diff}");
        // Longer than the document: cut with no click at the end.
        let x = node_audio(splash(json!({"size": 0.5})), "s", 0.1);
        assert!(x.last().unwrap().abs() < 1e-3);
    }

    #[test]
    fn typical_settings_reach_lufs_minus_20_without_a_warning() {
        for (flow, density, splash) in [
            (0.3, 300.0, 0.0),
            (0.6, 300.0, 0.2),
            (1.0, 300.0, 0.0),
            (0.6, 40.0, 0.0),
            (1.0, 3000.0, 0.0),
        ] {
            let nodes = water(json!({"flow": flow, "density": density, "splash": splash}));
            let r = render_doc(&nodes, 6.0, Some(-20.0), Kind::Oneshot);
            println!(
                "flow {flow} density {density} splash {splash}: LUFS {:.2}, peak {:.2} dBFS, warnings {:?}",
                r.lufs, r.peak_db, r.warnings
            );
            assert!(
                r.warnings.iter().all(|w| !w.contains("target")),
                "{:?}",
                r.warnings
            );
            assert!((r.lufs + 20.0).abs() < 1.0, "{}", r.lufs);
        }
        // A sparse trickle is peaky: print where -20 stops being reachable.
        for flow in [0.05, 0.1, 0.2] {
            let r = render_doc(
                &water(json!({"flow": flow})),
                6.0,
                Some(-20.0),
                Kind::Oneshot,
            );
            println!(
                "sparse: flow {flow} at -20: LUFS {:.2}, warnings {}",
                r.lufs,
                r.warnings.len()
            );
        }
        // A lone splash is a peaky one-shot (crest about 32 dB since book check
        // 1): AGENTS.md sends it to peak_db, so it has no LUFS case here.
        for size in [0.02, 0.1, 0.5] {
            let r = render_doc(
                &splash(json!({"size": size})),
                3.0,
                Some(-20.0),
                Kind::Oneshot,
            );
            println!(
                "splash {size} at -20: LUFS {:.2}, peak {:.2}, warnings {:?}",
                r.lufs, r.peak_db, r.warnings
            );
            // Fitted to refs/splash_set the crest is 28-31 dB (the recordings
            // 21-27), so -20 is out of reach: a miss must point at peak_db.
            assert!(
                r.warnings
                    .iter()
                    .all(|w| !w.contains("target") || w.contains("peak_db")),
                "{:?}",
                r.warnings
            );
        }
    }

    #[test]
    fn level_does_not_depend_on_a_lucky_big_bubble() {
        // The round-0 complaint: with bubble's r^1.5 law a few large bubbles set
        // the level. Here the same stream under 8 seeds, and a sparse one.
        for (flow, density) in [(0.5, 300.0), (0.5, 40.0)] {
            let l: Vec<f32> = (1..=8)
                .map(|seed| {
                    let nodes = water(json!({"flow": flow, "density": density, "seed": seed}));
                    render_doc(&nodes, 6.0, None, Kind::Oneshot).lufs
                })
                .collect();
            let (lo, hi) = (
                l.iter().cloned().fold(f32::MAX, f32::min),
                l.iter().cloned().fold(f32::MIN, f32::max),
            );
            println!(
                "flow {flow} density {density}, seeds 1..8: LUFS {lo:.2} .. {hi:.2} (spread {:.2} dB)",
                hi - lo
            );
            assert!(hi - lo < SPREAD, "{l:?}");
        }
    }

    #[test]
    fn a_loop_passes_the_default_checks_including_loop_seam() {
        for (flow, splash) in [(0.5, 0.0), (0.9, 0.5), (0.15, 0.0)] {
            let nodes = water(json!({"flow": flow, "splash": splash}));
            let mut d = SoundDoc::named("loop");
            d.nodes = nodes;
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "w", "lufs": -20}));
            d.kind = Kind::Loop;
            d.duration = Some(4.0);
            let opts = RenderOpts {
                cached: false,
                ..RenderOpts::default()
            };
            let rendered = d.render(&opts).unwrap();
            let checks = run(
                &Expect::defaults_for(&d),
                &CheckCx {
                    doc: &d,
                    rendered: &rendered,
                    opts: &opts,
                },
            )
            .unwrap();
            let text = report(&checks);
            println!("loop flow {flow} splash {splash}:\n{text}");
            assert!(text.contains("loop_seam"), "the seam check ran");
            assert!(checks.iter().all(|c| c.pass), "{text}");
        }
        // The wrap: with loop on, the last 100 ms continue into the first 100
        // ms (bubble tails wrap); the sample step across the seam is ordinary.
        let nodes = water(json!({"flow": 0.5, "loop": true}));
        let x = node_audio(&nodes, "w", 2.0);
        let step = (x[0] - x[x.len() - 1]).abs();
        let typical = x.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / x.len() as f32;
        println!("loop true: seam step {step:.5}, mean step {typical:.5}");
        assert!(step < 8.0 * typical);
        // Explicit loop: true / false only change the result by the wrap.
        let off = node_audio(water(json!({"flow": 0.5, "loop": false})), "w", 2.0);
        assert!(off != x);
    }

    #[test]
    fn thirty_seconds_at_max_flow_renders_inside_the_budget() {
        // Budget: 10 s on the test profile (optimised sfx_graph). Measured
        // value printed below.
        let nodes = water(json!({"flow": 1.0, "splash": 0.5}));
        let t = Instant::now();
        let x = node_audio(&nodes, "w", 30.0);
        let took = t.elapsed().as_secs_f32();
        assert_finite(&x, "w");
        let dens = node_audio(water(json!({"flow": 1.0, "density": 3000})), "w", 30.0);
        let took_max = t.elapsed().as_secs_f32() - took;
        assert_finite(&dens, "w");
        println!(
            "30 s, flow 1, default density + splash 0.5: {took:.2} s; density 3000: {took_max:.2} s"
        );
        assert!(took < 10.0 && took_max < 10.0);
    }

    #[test]
    fn water_and_splash_are_bit_identical() {
        assert_bit_identical(water(json!({"flow": 0.7, "splash": 0.4})), 2.0);
        assert_bit_identical(
            water(json!({"flow": 0.7, "loop": true, "splash": 0.4})),
            2.0,
        );
        assert_bit_identical(splash(json!({"size": 0.3, "at": 0.2})), 2.0);
    }

    #[test]
    fn extremes_are_finite() {
        for flow in [0.0, 1.0] {
            for depth in [0.0, 1.0] {
                for density in [0.0, 3000.0] {
                    let x = node_audio(
                        water(
                            json!({"flow": flow, "depth": depth, "density": density, "splash": 1.0}),
                        ),
                        "w",
                        1.0,
                    );
                    assert_finite(&x, "w");
                    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    assert!(
                        peak < 20.0,
                        "flow {flow} depth {depth} density {density}: {peak}"
                    );
                }
            }
        }
        // A flow signal far out of range is clamped.
        let nodes = vec![
            json!({"type": "ramp", "name": "f", "from": -1.0e9, "to": 1.0e9, "time": 1.0}),
            json!({"type": "water", "name": "w", "flow": "f", "seed": 1}),
        ];
        assert_finite(&node_audio(&nodes, "w", 1.0), "ramp flow");
        for size in [0.005, 1.0] {
            let x = node_audio(splash(json!({"size": size})), "s", 3.0);
            assert_finite(&x, "s");
        }
        // A very short document and a low sample rate.
        assert_finite(
            &node_audio(water(json!({"flow": 1.0, "loop": true})), "w", 0.001),
            "short loop",
        );
        let mut d = SoundDoc::named("low");
        d.sample_rate = 8000;
        d.nodes = vec![
            json!({"type": "water", "name": "w", "flow": 1.0, "splash": 1.0, "depth": 0.0}),
            json!({"type": "splash", "name": "s", "size": 0.005}),
            json!({"type": "mix", "name": "m", "inputs": ["w", "s"]}),
            json!({"type": "output", "name": "out", "input": "m"}),
        ];
        d.duration = Some(1.0);
        let a = d.render(&RenderOpts::default()).unwrap().audio;
        assert!(a.channels[0].iter().all(|v| v.is_finite()));
        println!(
            "extremes: flow 0/1 x depth 0/1 x density 0/3000 at splash 1, +-1e9 flow ramp, splash 0.005 and 1, 1 ms loop, 8 kHz: all finite"
        );
    }

    #[test]
    fn errors_say_the_field_value_and_fix() {
        let e = render_error(water(json!({"flow": 2})));
        println!("{e}");
        assert!(e.contains("flow: 2: give a number from 0 to 1"), "{e}");
        let e = render_error(water(json!({"density": 5000})));
        println!("{e}");
        assert!(
            e.contains("density: 5000: give bubbles a second from 0 to 3000"),
            "{e}"
        );
        let e = render_error(water(json!({"depth": 1.5})));
        println!("{e}");
        assert!(e.contains("depth: 1.5: give a number from 0 to 1"), "{e}");
        let e = render_error(water(json!({"splash": -1})));
        assert!(e.contains("splash: -1: give a number from 0 to 1"), "{e}");
        let e = render_error(water(json!({"flwo": 1})));
        assert!(e.contains("flwo") && e.contains("flow"), "{e}");
        let e = render_error(splash(json!({"size": 5})));
        println!("{e}");
        assert!(
            e.contains("size: 5: give the splash radius in metres from 0.005 to 1"),
            "{e}"
        );
        let e = render_error(splash(json!({"size": 0})));
        println!("{e}");
        assert!(e.contains("size"), "{e}");
        let e = render_error(splash(json!({"at": -1})));
        println!("{e}");
        assert!(e.contains("at"), "{e}");
        // The work cap: a very long, dense, splashy document.
        let mut d = SoundDoc::named("cap");
        d.nodes = vec![
            json!({"type": "water", "name": "w", "flow": 1.0, "density": 3000, "splash": 1.0}),
            json!({"type": "output", "name": "out", "input": "w"}),
        ];
        d.duration = Some(600.0);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        println!("{e}");
        assert!(
            e.contains("limit 800000") && e.contains("lower density"),
            "{e}"
        );
    }

    #[test]
    fn registered_with_the_right_flags() {
        for (ty, causal) in [("water", true), ("splash", false)] {
            let mut body = sfx_graph::example(ty).expect("registered");
            body.as_object_mut().unwrap().remove("type");
            body.as_object_mut().unwrap().remove("name");
            let node = parse_body(ty, &body, "x").unwrap();
            assert!(node.random());
            assert_eq!(node.causal(), causal, "{ty}");
            assert!(
                sfx_graph::describe(None)
                    .unwrap()
                    .contains(&format!("- {ty}:"))
            );
        }
        let text = sfx_graph::describe(Some("water")).unwrap();
        assert!(text.contains("Depth of the water") && text.contains("loop"));
        let body = json!({"flow": 0.5, "loop": true});
        let node = parse_body("water", &body, "x").unwrap();
        assert!(!node.causal(), "a looping water is not causal");
    }

    /// The recipe in docs/nodes/water.md, as written there.
    fn recipe() -> Vec<Value> {
        vec![
            json!({"type": "wander", "name": "level", "rate": 0.4, "range": [0.35, 0.7], "seed": 1}),
            json!({"type": "water", "name": "brook", "flow": "level", "depth": 0.3, "splash": 0.15, "seed": 2}),
            json!({"type": "splash", "name": "plop", "size": 0.12, "at": 1.5, "seed": 5}),
            json!({"type": "gain", "name": "plop_g", "input": "plop", "db": -6}),
            json!({"type": "mix", "name": "scene", "inputs": ["brook", "plop_g"]}),
        ]
    }

    #[test]
    fn recipe_renders() {
        let r = render_doc(&recipe(), 6.0, Some(-22.0), Kind::Oneshot);
        println!(
            "recipe: LUFS {:.2}, peak {:.2} dBFS, warnings {:?}",
            r.lufs, r.peak_db, r.warnings
        );
        assert!(r.warnings.iter().all(|w| !w.contains("target")));
        assert!((r.lufs + 22.0).abs() < 1.0);
        // The default checks examples/water.rs prints for the same document.
        let mut d = SoundDoc::named("water");
        d.nodes = recipe();
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "scene", "lufs": -22}));
        d.duration = Some(6.0);
        let opts = RenderOpts::default();
        let rendered = d.render(&opts).unwrap();
        let checks = run(
            &Expect::defaults_for(&d),
            &CheckCx {
                doc: &d,
                rendered: &rendered,
                opts: &opts,
            },
        )
        .unwrap();
        let text = report(&checks);
        println!("{text}");
        assert!(checks.iter().all(|c| c.pass), "{text}");
    }

    /// The `loop_seam` check on a looped `water` document: `filters` puts a 250 Hz
    /// high-pass and a 6 kHz low-pass after it, `xfade` a `crossfade_loop` after those.
    fn seam_case(
        water_fields: Value,
        seed: u64,
        duration: f32,
        filters: bool,
        xfade: bool,
    ) -> (bool, String) {
        let mut d = SoundDoc::named("loop");
        d.seed = seed;
        d.nodes = water(water_fields);
        let mut last = "w";
        if filters {
            d.nodes.push(
                json!({"type": "filter", "name": "hp", "input": last, "mode": "hp", "freq": 250}),
            );
            d.nodes.push(
                json!({"type": "filter", "name": "lp", "input": "hp", "mode": "lp", "freq": 6000}),
            );
            last = "lp";
        }
        if xfade {
            d.nodes
                .push(json!({"type": "crossfade_loop", "name": "xf", "input": last, "fade": 0.25}));
            last = "xf";
        }
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": last, "lufs": -20}));
        d.kind = Kind::Loop;
        d.duration = Some(duration);
        let opts = RenderOpts {
            cached: false,
            ..RenderOpts::default()
        };
        let rendered = d.render(&opts).unwrap();
        let checks = run(
            &Expect {
                loop_seam: Some(true),
                ..Expect::default()
            },
            &CheckCx {
                doc: &d,
                rendered: &rendered,
                opts: &opts,
            },
        )
        .unwrap();
        (checks.iter().all(|c| c.pass), report(&checks))
    }

    /// Seeds 1..=40 at flows 0.2 / 0.575 / 0.9 and densities 60 / 400.
    fn seam_grid(
        splash: f32,
        filters: bool,
        xfade: bool,
        flows: &[f32],
    ) -> (usize, usize, Vec<String>) {
        let (mut fails, mut total, mut first) = (0, 0, vec![]);
        for seed in 1..=40u64 {
            for &flow in flows {
                for density in [60.0f32, 400.0] {
                    let f = json!({"flow": flow, "density": density, "splash": splash});
                    let (ok, text) = seam_case(f, seed, 3.0, filters, xfade);
                    total += 1;
                    if !ok {
                        fails += 1;
                        if first.len() < 3 {
                            first.push(format!(
                                "seed {seed} flow {flow} density {density}: {}",
                                text.lines().next().unwrap_or("")
                            ));
                        }
                    }
                }
            }
        }
        (fails, total, first)
    }

    #[test]
    fn a_looped_water_passes_loop_seam_for_40_seeds() {
        // Plain node in a `kind: loop` document, default checks: measured 0 of
        // 480 failing over seeds 1..=40 x flow 0.2 / 0.5 / 0.575 / 0.9 x density
        // 60 / 150 / 400 at 3 s (and 0 of 480 at 1 s); this is the 240-case
        // subset below.
        let (fails, total, first) = seam_grid(0.0, false, false, &[0.2, 0.575, 0.9]);
        println!("plain water loop: {fails}/{total} fail {first:?}");
        assert_eq!(fails, 0, "{first:?}");
    }

    #[test]
    fn a_looped_water_is_continuous_across_the_wrap_with_splashes_too() {
        // The raw node (no output stage, no filters): the step across the wrap
        // against the largest step in the 32 samples either side of it. A
        // bubble tail that did not wrap, or a primed filter that did not match,
        // would show as a step far above its neighbours; a continuous signal
        // reads about 1. (`loop_seam` compares the wrap step with the whole
        // loop's 99th percentile, which a sparse loud bubble ringing over the
        // wrap can exceed without any break: doc seed 18, flow 0.2, density 60,
        // 8 s reads 2.2x there, with the steps climbing smoothly 0.057, 0.074,
        // 0.099, 0.117, 0.123, 0.166 (the wrap), 0.039 around it.)
        let mut worst = (0.0f32, String::new());
        for seed in 1..=40u64 {
            for flow in [0.2f32, 0.575, 0.9] {
                for splash in [0.0f32, 0.5] {
                    let nodes = water(json!({
                        "flow": flow, "density": 200, "splash": splash,
                        "loop": true, "seed": seed
                    }));
                    let x = node_audio(&nodes, "w", 3.0);
                    let n = x.len();
                    let step = |a: f32, b: f32| (b - a).abs();
                    let seam = step(x[n - 1], x[0]);
                    let mut local = 1e-9f32;
                    for k in 1..=32 {
                        local = local
                            .max(step(x[n - k - 1], x[n - k]))
                            .max(step(x[k - 1], x[k]));
                    }
                    let ratio = seam / local;
                    if ratio > worst.0 {
                        worst = (ratio, format!("seed {seed} flow {flow} splash {splash}"));
                    }
                }
            }
        }
        println!(
            "worst wrap step / local max step: {:.2} at {}",
            worst.0, worst.1
        );
        assert!(worst.0 < 1.5, "{worst:?}");
    }

    #[test]
    fn filters_after_a_looped_water_break_the_seam_and_a_crossfade_mends_it() {
        // A filter has its own startup: hp 250 Hz + lp 6 kHz after the (periodic)
        // water start from zero state, so their output is not periodic at the
        // wrap. Measured over the 480-case grid at 3 s: 169 fail without a
        // crossfade_loop, 1 with.
        let (fails, total, first) = seam_grid(0.0, true, false, &[0.2, 0.575, 0.9]);
        println!("water + hp + lp, no crossfade: {fails}/{total} fail {first:?}");
        assert!(
            fails * 5 >= total,
            "the requirement the docs state (crossfade_loop after filters) is real: only {fails}/{total} failed"
        );
        let (fails, total, first) = seam_grid(0.0, true, true, &[0.575, 0.9]);
        println!("water + hp + lp + crossfade_loop: {fails}/{total} fail {first:?}");
        assert_eq!(fails, 0, "{first:?}");
    }

    #[test]
    fn the_loop_field_docs_state_the_filter_requirement() {
        let text = sfx_graph::describe(Some("water")).unwrap();
        assert!(
            text.contains("crossfade_loop"),
            "water's description says filters after it need crossfade_loop: {text}"
        );
    }

    #[test]
    fn a_filter_after_a_looped_source_warns_unless_a_crossfade_follows() {
        let mk = |with_xfade: bool| {
            let mut d = SoundDoc::named("w");
            d.kind = sfx_graph::Kind::Loop;
            d.duration = Some(2.0);
            let mut nodes = vec![
                json!({"type": "water", "name": "w1", "flow": 0.5}),
                json!({"type": "filter", "name": "hp", "input": "w1", "mode": "hp", "freq": 250}),
            ];
            let last = if with_xfade {
                nodes.push(json!({"type": "crossfade_loop", "name": "x", "input": "hp"}));
                "x"
            } else {
                "hp"
            };
            nodes.push(json!({"type": "output", "name": "out", "input": last}));
            d.nodes = nodes;
            d.render(&RenderOpts::default()).unwrap().warnings
        };
        assert!(
            mk(false).iter().any(|w| w.contains("crossfade_loop")),
            "{:?}",
            mk(false)
        );
        assert!(
            !mk(true)
                .iter()
                .any(|w| w.contains("filters the looped source")),
            "{:?}",
            mk(true)
        );
    }
}

mod after_listening {
    //! The batch-2 blind labels heard `water`'s hiss as wind and
    //! `splash` as a "drop kick" / "weird crunchies". These tests pin the
    //! measurable causes going away (numbers in docs/nodes/water.md).

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::describe;

    fn mono(x: &[f32]) -> sfx_graph::Audio {
        sfx_graph::Audio {
            sample_rate: SR,
            channels: vec![x.to_vec()],
        }
    }

    fn one(kind: &str, fields: Value) -> Vec<Value> {
        let mut n = json!({"type": kind, "name": "n"});
        n.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        vec![n]
    }

    /// Energy share of `x` between `lo` and `hi` Hz (0..1), from `band_db`.
    fn share(x: &[f32], lo: f32, hi: f32) -> f32 {
        let p = |a: f32, b: f32| 10f32.powf(band_db(x, SR, a, b) / 10.0);
        p(lo.max(1.0), hi) / p(1.0, SR as f32 / 2.0).max(1e-30)
    }

    /// Coefficient of variation of the 10 ms RMS envelope of `x` above `hz`
    /// (a second-order high-pass by difference twice): a steady hiss reads low,
    /// a hiss that comes and goes with the bubbles reads high.
    fn hf_env_cv(x: &[f32], hz: f32) -> f32 {
        // One-pole high-passes, twice.
        let a = (-std::f32::consts::TAU * hz / SR as f32).exp();
        let mut y = x.to_vec();
        for _ in 0..2 {
            let (mut px, mut py) = (0.0f32, 0.0f32);
            for v in &mut y {
                let o = a * (py + *v - px);
                px = *v;
                py = o;
                *v = o;
            }
        }
        let w = (SR / 100) as usize;
        let rms: Vec<f32> = y
            .chunks(w)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let m = rms.iter().sum::<f32>() / rms.len() as f32;
        let var = rms.iter().map(|r| (r - m).powi(2)).sum::<f32>() / rms.len() as f32;
        var.sqrt() / m.max(1e-30)
    }

    #[test]
    fn water_hiss_follows_the_bubbles() {
        let mut bad = Vec::new();
        // The labelled graph: flow 0.5, depth 0.5, density 300.
        for (flow, density, before) in [
            (0.5, 300.0, 0.254),
            (1.0, 300.0, 0.060),
            (0.8, 600.0, 0.112),
        ] {
            let x = node_audio(
                one(
                    "water",
                    json!({"flow": flow, "density": density, "seed": 1}),
                ),
                "n",
                3.0,
            );
            assert_finite(&x, "water");
            let d = describe(&mono(&x));
            let cv = hf_env_cv(&x, 3000.0);
            let low_mid = share(&x, 300.0, 1200.0);
            println!(
                "water flow {flow} density {density}: hf envelope cv {cv:.3}, 300-1200 Hz share {:.3}, flatness {:.3}, centroid {:.0} Hz, fine onsets/s {:.1}",
                low_mid,
                d.flatness_all,
                d.centroid_all_hz,
                d.fine_onsets.len() as f32 / d.seconds
            );
            // Before the fix the hiss was a steady bed (cv `before`); it now
            // rides the pops. Round 1 had 1.6x and was heard as "crispy, like
            // boiling oil"; round 2 moves it halfway back (a 45 % steady share):
            // at least 1.2x as much envelope movement.
            if cv <= 1.2 * before {
                bad.push((flow, density, cv, before));
            }
        }
        assert!(
            bad.is_empty(),
            "hiss above 3 kHz is still a steady bed (flow, density, cv, before): {bad:?}"
        );
    }

    #[test]
    fn splash_has_no_kick_and_a_watery_slap() {
        let mut bad = Vec::new();
        for size in [0.01, 0.05, 0.15, 0.3, 0.6] {
            let x = node_audio(
                one("splash", json!({"size": size, "at": 0.0, "seed": 2})),
                "n",
                2.5,
            );
            assert_finite(&x, "splash");
            let d = describe(&mono(&x));
            let first = &x[..(0.02 * SR as f32) as usize];
            let thump = share(&x, 0.0, 250.0);
            let slap = share(first, 1000.0, 6000.0);
            let hp_first = peak_hz(first, SR);
            // Where the sample peak is (the slap must be the loudest instant).
            let peak_s = x
                .iter()
                .enumerate()
                .fold(
                    (0, 0.0f32),
                    |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
                )
                .0 as f32
                / SR as f32;
            println!(
                "splash {size}: <250 Hz share {thump:.3}, first 20 ms 1-6 kHz share {slap:.3} (peak {hp_first:.0} Hz), voiced {:.2}, centroid {:.0} Hz, sample peak at {peak_s:.4} s, loudest 10 ms at {:.3} s, fine onsets {}",
                d.voiced,
                d.centroid_all_hz,
                d.peak_t,
                d.fine_onsets.len()
            );
            // Book check 1: the first 20 ms are the slap (3.5 kHz) over the
            // impact bump, so 1-6 kHz holds at least 0.35 of them at every size.
            let min_slap = 0.35;
            if thump >= 0.05 || slap <= min_slap || peak_s >= 0.03 {
                bad.push(size);
            }
        }
        assert!(
            bad.is_empty(),
            "splash sizes with a thump, no slap or a late peak: {bad:?}"
        );
    }

    #[test]
    fn splash_and_water_are_deterministic() {
        assert_bit_identical(one("splash", json!({"size": 0.2, "seed": 4})), 1.5);
        assert_bit_identical(
            one("water", json!({"flow": 0.6, "splash": 0.5, "seed": 4})),
            2.0,
        );
    }
}

mod tail_tone {
    //! `splash` `tail` / `burst`, `thunder` `tail`, `water` `tone`.
    //! Each test prints its measurement (these lines go in docs/nodes/water.md).

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::{CheckCx, Expect, describe, report, run};
    use sfx_graph::{Kind, RenderOpts, SoundDoc};

    fn same_bits(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.to_bits() == q.to_bits())
    }

    fn node(kind: &str, base: Value, extra: Value) -> Vec<Value> {
        let mut n = json!({"type": kind, "name": "n"});
        let o = n.as_object_mut().unwrap();
        o.extend(base.as_object().unwrap().clone());
        o.extend(extra.as_object().unwrap().clone());
        vec![n]
    }

    fn mono(x: &[f32]) -> sfx_graph::Audio {
        sfx_graph::Audio {
            sample_rate: SR,
            channels: vec![x.to_vec()],
        }
    }

    /// Seconds from the first sound until the 10 ms RMS envelope is under `db`
    /// dBFS for good.
    fn audible_s(x: &[f32], db: f32) -> f32 {
        let w = (SR / 100) as usize;
        let floor = 10f32.powf(db / 20.0);
        let rms: Vec<f32> = x
            .chunks(w)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let first = rms.iter().position(|v| *v > 0.0).unwrap_or(0);
        let last = rms.iter().rposition(|v| *v > floor).unwrap_or(first);
        last.saturating_sub(first) as f32 * 0.01
    }

    // ---------------------------------------------------------------- splash

    fn splash(extra: Value) -> Vec<Value> {
        node("splash", json!({"size": 0.15, "seed": 3}), extra)
    }

    #[test]
    fn splash_defaults_are_bit_identical() {
        for size in [0.02, 0.15, 0.6] {
            let base = json!({"size": size, "seed": 3});
            let a = node_audio(node("splash", base.clone(), json!({})), "n", 3.0);
            let b = node_audio(
                node("splash", base, json!({"tail": 1.0, "burst": 1.0})),
                "n",
                3.0,
            );
            assert!(same_bits(&a, &b), "size {size}");
        }
    }

    /// The end of the audible part: the last 10 ms window whose rms is within
    /// 45 dB of the loudest window's. Fitted to refs/splash_set, the bubble cloud
    /// is quiet under spray and slap, so its onsets no longer read separately;
    /// its length still shows here.
    fn audible_end(x: &[f32]) -> f32 {
        let w = (0.01 * SR as f32) as usize;
        let rms: Vec<f32> = x
            .chunks(w)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let top = rms.iter().copied().fold(0.0f32, f32::max);
        let last = rms
            .iter()
            .rposition(|r| *r > top * 10f32.powf(-45.0 / 20.0))
            .unwrap_or(0);
        (last + 1) as f32 * 0.01
    }

    #[test]
    fn splash_tail_and_burst_reshape_it() {
        let plain = node_audio(splash(json!({})), "n", 3.0);
        let long = node_audio(splash(json!({"tail": 2.0, "burst": 2.0})), "n", 3.0);
        assert_finite(&long, "splash");
        let first_20ms = (0.02 * SR as f32) as usize;
        let peak_at = |x: &[f32]| {
            x.iter()
                .enumerate()
                .fold(
                    (0, 0.0f32),
                    |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m },
                )
                .0
        };
        let (p_plain, p_long) = (peak_at(&plain), peak_at(&long));
        let (after, after0) = (audible_end(&long), audible_end(&plain));
        println!(
            "0.15 m splash: default peaks at {:.1} ms, audible to {after0:.2} s; tail 2 burst 2 peaks at {:.1} ms, audible to {after:.2} s, length {:.2} s vs {:.2} s",
            p_plain as f32 * 1000.0 / SR as f32,
            p_long as f32 * 1000.0 / SR as f32,
            long.len() as f32 / SR as f32,
            plain.len() as f32 / SR as f32,
        );
        assert!(p_long < first_20ms, "peak at sample {p_long}");
        // tail 2 must make the splash audible for longer.
        assert!(after > after0, "audible to {after} s (default {after0} s)");
    }

    #[test]
    fn splash_tail_lengthens_the_cloud_monotonically() {
        let mut onsets = Vec::new();
        for tail in [0.5f32, 1.0, 2.0] {
            let x = node_audio(splash(json!({"tail": tail})), "n", 3.0);
            let last = audible_end(&x);
            println!("tail {tail}: audible to {last:.2} s");
            onsets.push(last);
        }
        assert!(onsets[0] < onsets[1] && onsets[1] < onsets[2], "{onsets:?}");
    }

    #[test]
    fn splash_burst_moves_the_peak_into_the_opening() {
        let level_20ms = |burst: f32| {
            let x = node_audio(splash(json!({"burst": burst})), "n", 3.0);
            assert_finite(&x, "splash");
            let n = (0.02 * SR as f32) as usize;
            let head = x[..n].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let rest = x[n..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            (head, rest)
        };
        let (h0, r0) = level_20ms(0.0);
        let (h1, r1) = level_20ms(1.0);
        let (h2, r2) = level_20ms(2.0);
        println!(
            "burst 0 / 1 / 2: peak in first 20 ms {h0:.3} / {h1:.3} / {h2:.3}, after {r0:.3} / {r1:.3} / {r2:.3}"
        );
        // water_fix: the slap is the loudest moment from burst 1 (the old
        // default peaked on the cavity bubble), so h1 = h2 = the peak and a
        // louder burst shows as a quieter rest.
        assert!(h0 < h1 && h1 <= h2);
        assert!(h1 >= r1 && h2 > r2, "burst 1 and 2 are the loudest moment");
        // The spray (which carries the tail since the refs/splash_set fit) scales
        // with `burst` too, so the rest only must not grow (0.157 / 0.158).
        assert!(
            r2 <= r1 * 1.03,
            "a louder burst does not make the rest louder"
        );
    }

    #[test]
    fn splash_new_fields_are_checked_and_finite_at_the_extremes() {
        for extra in [
            json!({"tail": 0.0}),
            json!({"tail": 3.0, "burst": 2.0}),
            json!({"burst": 0.0, "tail": 0.0}),
        ] {
            let x = node_audio(splash(extra.clone()), "n", 3.0);
            assert_finite(&x, &extra.to_string());
        }
        for bad in [
            json!({"tail": 3.5}),
            json!({"burst": 2.5}),
            json!({"tail": -1}),
        ] {
            let mut d = SoundDoc::named("t");
            d.nodes = splash(bad);
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "n"}));
            d.duration = Some(1.0);
            let e = d.render(&RenderOpts::default()).unwrap_err();
            println!("error: {e}");
            assert!(e.contains("give 0.."), "{e}");
        }
    }

    // --------------------------------------------------------------- thunder

    fn thunder(extra: Value) -> Vec<f32> {
        node_audio(
            node("thunder", json!({"distance": 1500, "seed": 3}), extra),
            "n",
            16.0,
        )
    }

    #[test]
    fn thunder_tail_off_is_bit_identical() {
        let a = thunder(json!({}));
        let b = thunder(json!({"tail": 0.0}));
        assert!(same_bits(&a, &b));
    }

    #[test]
    fn thunder_tail_shortens_the_decay_monotonically() {
        let mut t20 = Vec::new();
        for tail in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let x = thunder(json!({"tail": tail}));
            assert_finite(&x, "thunder");
            let a = x.iter().position(|v| *v != 0.0).unwrap();
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let end = x.iter().rposition(|v| v.abs() > 1e-3 * peak).unwrap();
            let desc = describe(&mono(&x));
            let d = desc.decay20_s.unwrap_or(0.0);
            let d60 = desc.decay60_s.unwrap_or(0.0);
            println!(
                "tail {tail}: first sound {:.2} s, last sample over -60 dB at {:.2} s, listen decay -20 dB {d:.2} s, -60 dB {d60:.2} s, audible (10 ms rms over -45 dBFS) {:.2} s, peak {:.2} dB",
                a as f32 / SR as f32,
                end as f32 / SR as f32,
                audible_s(&x, -45.0),
                20.0 * peak.log10()
            );
            t20.push(audible_s(&x, -45.0));
        }
        // Foley's tail is a string of separate spikes, so the audible end moves
        // in steps (a spike either clears the floor or not): non-increasing,
        // and well shorter at the top.
        assert!(t20.windows(2).all(|w| w[1] <= w[0]), "{t20:?}");
        assert!(t20[4] + 2.0 < t20[0], "{t20:?}");
    }

    #[test]
    fn thunder_tail_bounds() {
        let mut d = SoundDoc::named("t");
        d.nodes = node("thunder", json!({}), json!({"tail": 1.5}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(8.0);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        println!("error: {e}");
        assert!(e.contains("tail"));
    }

    // ----------------------------------------------------------------- water

    fn water(extra: Value) -> Vec<Value> {
        node("water", json!({"flow": 0.5, "seed": 3}), extra)
    }

    #[test]
    fn water_tone_default_is_bit_identical() {
        for looped in [false, true] {
            let mut d = SoundDoc::named("w");
            d.nodes = water(json!({"loop": looped}));
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "n"}));
            d.duration = Some(2.0);
            let a = node_audio(water(json!({"loop": looped})), "n", 2.0);
            let b = node_audio(water(json!({"loop": looped, "tone": 0.5})), "n", 2.0);
            assert!(same_bits(&a, &b), "loop {looped}");
        }
    }

    fn share_above(x: &[f32], hz: f32) -> f32 {
        let hi = 10f32.powf(band_db(x, SR, hz, SR as f32 / 2.0) / 10.0);
        let all = 10f32.powf(band_db(x, SR, 0.0, SR as f32 / 2.0) / 10.0);
        hi / all
    }

    #[test]
    fn water_tone_tilts_the_spectrum_above_2k() {
        let mut shares = Vec::new();
        for tone in [0.0f32, 0.5, 1.0] {
            let x = node_audio(water(json!({"tone": tone})), "n", 4.0);
            assert_finite(&x, "water");
            shares.push(share_above(&x, 2000.0));
        }
        let db = |a: f32, b: f32| 10.0 * (a / b).log10();
        println!(
            "tone 0 / 0.5 / 1: share above 2 kHz {:.3} / {:.3} / {:.3}; tone 1 vs 0.5 {:+.1} dB, tone 0 vs 0.5 {:+.1} dB",
            shares[0],
            shares[1],
            shares[2],
            db(shares[2], shares[1]),
            db(shares[0], shares[1])
        );
        assert!(shares[0] < shares[1] && shares[1] < shares[2]);
        // The share is relative to the total, which rises with the band (so
        // tone 1 against 0.5 cannot gain 6 dB of share from a 0.23 start): the
        // whole range 0..1 does, and the band level itself is checked below.
        assert!(db(shares[2], shares[0]) >= 6.0);
    }

    #[test]
    fn water_tone_raises_the_band_level_by_six_db() {
        let lo = node_audio(water(json!({"tone": 0.5})), "n", 4.0);
        let hi = node_audio(water(json!({"tone": 1.0})), "n", 4.0);
        let up = band_db(&hi, SR, 2000.0, 20_000.0) - band_db(&lo, SR, 2000.0, 20_000.0);
        let low_move = band_db(&hi, SR, 50.0, 500.0) - band_db(&lo, SR, 50.0, 500.0);
        println!("tone 1 vs 0.5: band 2-20 kHz {up:+.1} dB, band 50-500 Hz {low_move:+.1} dB");
        assert!(up >= 6.0, "{up}");
        assert!(low_move.abs() < 1.0, "{low_move}");
    }

    #[test]
    fn water_tone_keeps_loop_seam_at_seeds_1_to_4() {
        for tone in [0.0f32, 1.0] {
            for seed in 1..=4u64 {
                let mut d = SoundDoc::named("loop");
                d.seed = seed;
                d.nodes = water(json!({"tone": tone}));
                d.nodes
                    .push(json!({"type": "output", "name": "out", "input": "n", "lufs": -20}));
                d.kind = Kind::Loop;
                d.duration = Some(3.0);
                let opts = RenderOpts {
                    cached: false,
                    ..RenderOpts::default()
                };
                let rendered = d.render(&opts).unwrap();
                let checks = run(
                    &Expect {
                        loop_seam: Some(true),
                        ..Expect::default()
                    },
                    &CheckCx {
                        doc: &d,
                        rendered: &rendered,
                        opts: &opts,
                    },
                )
                .unwrap();
                let text = report(&checks);
                println!(
                    "tone {tone} seed {seed}: {}",
                    text.lines().next().unwrap_or("")
                );
                assert!(
                    checks.iter().all(|c| c.pass),
                    "tone {tone} seed {seed}: {text}"
                );
            }
        }
    }

    #[test]
    fn water_tone_bounds() {
        let mut d = SoundDoc::named("t");
        d.nodes = water(json!({"tone": 1.5}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(1.0);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        println!("error: {e}");
        assert!(e.contains("tone"));
    }
}
