mod reverb_ir {
    //! `reverb.model` and `ir`. The first test pins the default `reverb`
    //! (model fdn) bit for bit against a hash recorded before this task changed
    //! the node.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_graph::node::parse_body;

    /// A click and a noise burst, so the tail has broadband content.
    fn source() -> Vec<Value> {
        vec![
            json!({"type": "impulse", "name": "click"}),
            json!({"type": "noise", "name": "n", "color": "white"}),
            json!({"type": "decay", "name": "e", "time": 0.01}),
            json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "mix", "name": "src", "inputs": ["click", "burst"]}),
        ]
    }

    fn with(extra: Value) -> Vec<Value> {
        let mut r = json!({"type": "reverb", "name": "verb", "input": "src"});
        r.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut v = source();
        v.push(r);
        v
    }

    /// (label, render, hash) of the default-model configurations.
    fn default_hashes() -> Vec<(&'static str, u64)> {
        let cases: Vec<(&str, Value)> = vec![
            ("default", json!({})),
            (
                "tuned",
                json!({"size": 0.8, "decay": 2.2, "damp": 0.5, "mix": 0.6, "predelay": 0.03}),
            ),
            (
                "dry_wet_extremes",
                json!({"size": 0.0, "decay": 0.01, "damp": 1.5, "mix": 1.0}),
            ),
        ];
        let mut out = Vec::new();
        for (label, extra) in cases {
            let a = render(with(extra.clone()), 1.5);
            out.push((label, fnv_channels(&a.channels)));
            // The wet port, through a unity gain.
            let mut nodes = with(extra);
            nodes.push(json!({"type": "gain", "name": "tail", "input": "verb.wet", "gain": 1.0}));
            let b = render(nodes, 1.5);
            out.push((label, fnv_channels(&b.channels)));
        }
        out
    }

    /// Recorded on the unmodified `reverb` (commit 95c71e4).
    const DEFAULT_HASHES: [u64; 6] = [
        0x90e7_6a83_e2b7_e1aa,
        0x9ca5_9984_5e99_7545,
        0x3887_84e6_fe14_aaaa,
        0x106a_ebad_0160_d73f,
        0xad3e_056e_3e48_6148,
        0xad3e_056e_3e48_6148,
    ];

    #[test]
    fn default_reverb_is_bit_identical_to_before_this_task() {
        let got = default_hashes();
        println!(
            "hashes: {:016x?}",
            got.iter().map(|g| g.1).collect::<Vec<_>>()
        );
        for ((label, h), want) in got.iter().zip(DEFAULT_HASHES) {
            assert_eq!(*h, want, "{label}: the default model changed");
        }
        // `model: "fdn"` written out is the default.
        let a = render(with(json!({"model": "fdn"})), 1.5);
        let b = render(with(json!({})), 1.5);
        assert_eq!(fnv_channels(&a.channels), fnv_channels(&b.channels));
    }

    // ------------------------------------------------------------ measurement

    /// Schroeder backward integration of `h`, in dB re its total, and the T60
    /// extrapolated from the least-squares line between `hi` and `lo` dB (both
    /// negative, `hi` > `lo`), seconds.
    fn t60(h: &[f32], sr: u32, hi: f32, lo: f32) -> f32 {
        let mut acc = 0.0f64;
        let mut e: Vec<f64> = h
            .iter()
            .rev()
            .map(|v| {
                acc += f64::from(*v) * f64::from(*v);
                acc
            })
            .collect();
        e.reverse();
        let total = e[0].max(1e-30);
        let pts: Vec<(f64, f64)> = e
            .iter()
            .enumerate()
            .map(|(i, v)| {
                (
                    i as f64 / f64::from(sr),
                    10.0 * (v / total).max(1e-30).log10(),
                )
            })
            .filter(|(_, d)| *d <= f64::from(hi) && *d >= f64::from(lo))
            .collect();
        assert!(pts.len() > 10, "decay never reaches {lo} dB");
        let n = pts.len() as f64;
        let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
        let (mx, my) = (sx / n, sy / n);
        let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>()
            / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>();
        (-60.0 / slope) as f32
    }

    /// The wet port of a reverb fed one click, left channel.
    fn wet_ir(extra: Value, duration: f32) -> Vec<f32> {
        let mut nodes = vec![
            json!({"type": "impulse", "name": "src"}),
            json!({"type": "reverb", "name": "verb", "input": "src", "mix": 1.0}),
        ];
        nodes[1]
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        nodes.push(json!({"type": "gain", "name": "tail", "input": "verb.wet", "gain": 1.0}));
        node_audio(nodes, "tail", duration)
    }

    /// `x` with everything outside `lo..hi` Hz removed (an FFT mask).
    fn band_pass(x: &[f32], sr: u32, lo: f32, hi: f32) -> Vec<f32> {
        use realfft::RealFftPlanner;
        let size = x.len().next_power_of_two();
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(size);
        let inv = planner.plan_fft_inverse(size);
        let mut a = vec![0.0f32; size];
        a[..x.len()].copy_from_slice(x);
        let mut f = fwd.make_output_vec();
        fwd.process(&mut a, &mut f).unwrap();
        for (k, z) in f.iter_mut().enumerate() {
            let hz = k as f32 * sr as f32 / size as f32;
            if hz < lo || hz >= hi {
                *z = Default::default();
            }
        }
        let mut out = inv.make_output_vec();
        inv.process(&mut f, &mut out).unwrap();
        out[..x.len()].iter().map(|v| v / size as f32).collect()
    }

    /// (seconds at which 10 % of the energy has arrived, energy centroid in
    /// seconds) of `x[..window]`.
    fn arrival(x: &[f32], sr: u32, window: usize) -> (f32, f32) {
        let e: Vec<f64> = x[..window].iter().map(|v| f64::from(*v).powi(2)).collect();
        let total: f64 = e.iter().sum();
        let mut acc = 0.0;
        let mut onset = 0;
        for (i, v) in e.iter().enumerate() {
            acc += v;
            if acc >= 0.1 * total {
                onset = i;
                break;
            }
        }
        let centroid = e.iter().enumerate().map(|(i, v)| i as f64 * v).sum::<f64>() / total;
        (onset as f32 / sr as f32, (centroid / f64::from(sr)) as f32)
    }

    #[test]
    fn t60_is_within_20_percent_of_decay_for_every_model() {
        for model in ["fdn", "room", "plate", "spring"] {
            for decay in [0.8f32, 1.5, 3.0] {
                // The default damping and none: damping darkens the tail, so it
                // reads shorter than `decay`, which is the low-frequency time.
                for damp in [0.0, 0.3] {
                    let h = wet_ir(json!({"model": model, "decay": decay, "damp": damp}), 6.0);
                    assert_finite(&h, model);
                    let t = t60(&h, SR, -5.0, -35.0);
                    println!(
                        "{model} decay {decay} damp {damp}: T60 {t:.3} s ({:+.1} %)",
                        100.0 * (t / decay - 1.0)
                    );
                    assert!(
                        (t / decay - 1.0).abs() < 0.2,
                        "{model} decay {decay} damp {damp}: T60 {t}"
                    );
                }
            }
        }
    }

    #[test]
    fn spring_four_khz_arrives_after_five_hundred_hz() {
        // size 0.5: a 50 ms loop, so the first pass through the chain is
        // measured in the first 80 ms.
        let h = wet_ir(
            json!({"model": "spring", "size": 0.5, "damp": 0.0, "decay": 1.5}),
            1.0,
        );
        let window = (0.08 * SR as f32) as usize;
        let low = band_pass(&h, SR, 400.0, 600.0);
        let high = band_pass(&h, SR, 3500.0, 4500.0);
        let (lo_on, lo_c) = arrival(&low, SR, window);
        let (hi_on, hi_c) = arrival(&high, SR, window);
        println!(
            "spring first pass: 500 Hz band 10 % at {:.2} ms, centroid {:.2} ms; 4 kHz band 10 % at {:.2} ms, centroid {:.2} ms; 4 kHz is {:.2} ms later (centroid)",
            lo_on * 1e3,
            lo_c * 1e3,
            hi_on * 1e3,
            hi_c * 1e3,
            (hi_c - lo_c) * 1e3
        );
        assert!(hi_c > lo_c + 0.002, "centroid {hi_c} vs {lo_c}");
        assert!(hi_on > lo_on + 0.001, "onset {hi_on} vs {lo_on}");
    }

    #[test]
    fn plate_builds_faster_and_denser_than_fdn() {
        // Echo density: the share of 1 ms windows in the first 60 ms whose peak
        // is within 30 dB of the loudest window.
        let density = |model: &str| {
            let h = wet_ir(json!({"model": model, "damp": 0.0}), 1.0);
            let win = SR as usize / 1000;
            let peaks: Vec<f32> = h[..SR as usize * 6 / 100]
                .chunks(win)
                .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
                .collect();
            let top = peaks.iter().fold(0.0f32, |m, v| m.max(*v));
            let live = peaks.iter().filter(|p| **p > top * 0.0316).count();
            let first = h.iter().position(|v| v.abs() > top * 0.0316).unwrap();
            (live as f32 / peaks.len() as f32, first as f32 / SR as f32)
        };
        let (fdn_d, fdn_t) = density("fdn");
        let (plate_d, plate_t) = density("plate");
        println!(
            "echo density in the first 60 ms: fdn {fdn_d:.2} (first echo {:.1} ms), plate {plate_d:.2} (first echo {:.1} ms)",
            fdn_t * 1e3,
            plate_t * 1e3
        );
        assert!(plate_d >= fdn_d && plate_t <= fdn_t);
    }

    fn ir_node(extra: Value) -> Value {
        let mut n = json!({"type": "ir", "name": "ir1"});
        n.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        n
    }

    fn ir(extra: Value, duration: f32) -> Vec<f32> {
        node_audio(vec![ir_node(extra)], "ir1", duration)
    }

    /// |H(f)| of the response `h`, dB (a direct DFT at one frequency).
    fn gain_db(h: &[f32], sr: u32, hz: f32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in h.iter().enumerate() {
            re += f64::from(*v) * (w * n as f64).cos();
            im -= f64::from(*v) * (w * n as f64).sin();
        }
        (20.0 * (re.hypot(im)).max(1e-12).log10()) as f32
    }

    #[test]
    fn room_ir_energy_decay_matches_sabine() {
        for (size, abs) in [
            (6.0f32, 0.3f32),
            (6.0, 0.1),
            (12.0, 0.2),
            (3.0, 0.5),
            (20.0, 0.05),
        ] {
            let h = ir(
                json!({"model": "room", "size": size, "absorption": abs}),
                12.0,
            );
            assert_finite(&h, "room ir");
            // Sides 1 : 0.8 : 0.4.
            let v = size * size * 0.8 * size * 0.4;
            let surface = 2.0 * (size * size * 0.8 + size * 0.8 * size * 0.4 + size * size * 0.4);
            let sabine = 0.161 * v / (surface * abs);
            let t = t60(&h, SR, -5.0, -35.0);
            println!(
                "ir room size {size} absorption {abs}: Sabine T60 {sabine:.3} s, measured {t:.3} s ({:+.1} %), direct sound {:.6} at sample 0",
                100.0 * (t / sabine - 1.0),
                h[0]
            );
            assert!((t / sabine - 1.0).abs() < 0.25, "{t} vs {sabine}");
            assert!((h[0] - 1.0).abs() < 1e-6, "the direct sound is at sample 0");
        }
    }

    #[test]
    fn room_ir_is_stereo_and_seeded() {
        let nodes = vec![
            ir_node(json!({"model": "room"})),
            json!({"type": "impulse", "name": "src"}),
            json!({"type": "convolve", "name": "wetsig", "input": "src", "ir": "ir1"}),
        ];
        let a = render(&nodes, 0.5);
        assert_eq!(a.channels.len(), 2, "a room is two ears");
        let (l, r) = (&a.channels[0], &a.channels[1]);
        let dot: f64 = l.iter().zip(r).map(|(a, b)| f64::from(a * b)).sum();
        let corr = dot
            / (l.iter().map(|v| f64::from(v * v)).sum::<f64>().sqrt()
                * r.iter().map(|v| f64::from(v * v)).sum::<f64>().sqrt());
        println!("room ir: left/right correlation {corr:.3}");
        assert!(corr < 0.9 && corr > -0.9);
        let one = ir(json!({"model": "room", "seed": 1}), 0.5);
        let again = ir(json!({"model": "room", "seed": 1}), 0.5);
        let other = ir(json!({"model": "room", "seed": 2}), 0.5);
        assert!(
            one.iter()
                .zip(&again)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        assert!(one.iter().zip(&other).any(|(a, b)| a != b));
    }

    #[test]
    fn convolve_with_an_ir_node_returns_the_response() {
        let nodes = vec![
            ir_node(json!({"model": "phone"})),
            json!({"type": "impulse", "name": "src"}),
            json!({"type": "convolve", "name": "c", "input": "src", "ir": "ir1"}),
        ];
        let h = ir(json!({"model": "phone"}), 0.3);
        let c = node_audio(nodes, "c", 0.3);
        let worst = h
            .iter()
            .zip(&c)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        println!("convolve(impulse, ir phone) differs from the ir by at most {worst:.2e}");
        assert!(worst < 1e-4);
    }

    #[test]
    fn phone_ir_passes_300_to_3400_and_cuts_100_and_8000() {
        let h = ir(json!({"model": "phone"}), 0.2);
        assert_finite(&h, "phone");
        let reference = gain_db(&h, SR, 1000.0);
        let rel = |hz| gain_db(&h, SR, hz) - reference;
        let (a, b, c, d) = (rel(300.0), rel(3400.0), rel(100.0), rel(8000.0));
        println!(
            "ir phone re 1 kHz ({reference:.2} dB): 300 Hz {a:.2} dB, 3.4 kHz {b:.2} dB, 100 Hz {c:.1} dB, 8 kHz {d:.1} dB"
        );
        assert!(a > -3.0 && b > -3.0, "{a} {b}");
        assert!(c < -30.0 && d < -30.0, "{c} {d}");
    }

    #[test]
    fn cabinet_ir_has_a_low_resonance_and_rolls_off() {
        let h = ir(json!({"model": "cabinet", "size": 0.6}), 0.3);
        assert_finite(&h, "cabinet");
        let low: Vec<(f32, f32)> = (30..400)
            .map(|hz| (hz as f32, gain_db(&h, SR, hz as f32)))
            .collect();
        let top = low
            .iter()
            .fold((0.0, f32::MIN), |m, p| if p.1 > m.1 { *p } else { m });
        let reference = gain_db(&h, SR, 1000.0);
        let (g40, g10k) = (gain_db(&h, SR, 40.0), gain_db(&h, SR, 10_000.0));
        println!(
            "ir cabinet: resonance {:.0} Hz at {:.1} dB re 1 kHz (expected {:.0} Hz); 40 Hz {:.1} dB, 10 kHz {:.1} dB",
            top.0,
            top.1 - reference,
            70.0 / 0.6,
            g40 - reference,
            g10k - reference
        );
        assert!((top.0 - 70.0 / 0.6).abs() < 0.2 * 70.0 / 0.6, "{}", top.0);
        assert!(g40 - reference < -10.0 && g10k - reference < -20.0);
    }

    #[test]
    fn tube_ir_has_modes_at_k_c_over_2l() {
        let h = ir(
            json!({"model": "tube", "size": 1.0, "absorption": 0.05}),
            1.0,
        );
        assert_finite(&h, "tube");
        let f1 = 343.0 / 2.0;
        let p = peaks(&h, SR, 12, 60.0);
        let near = |want: f32| {
            p.iter()
                .map(|q| q.0)
                .find(|hz| (hz - want).abs() < 0.01 * want)
        };
        let found: Vec<Option<f32>> = (1..=4).map(|k| near(k as f32 * f1)).collect();
        println!(
            "ir tube 1 m: modes wanted {:.1} {:.1} {:.1} {:.1} Hz, found {found:.1?}",
            f1,
            2.0 * f1,
            3.0 * f1,
            4.0 * f1
        );
        assert!(found.iter().all(Option::is_some));
    }

    #[test]
    fn box_ir_has_the_modes_of_all_three_axes() {
        let h = ir(
            json!({"model": "box", "size": 0.5, "absorption": 0.05}),
            1.0,
        );
        assert_finite(&h, "box");
        let p = peaks(&h, SR, 40, 40.0);
        // 0.5 m, 0.4 m and 0.2 m axes: first modes 343, 428.75 and 857.5 Hz.
        let wanted = [343.0f32, 428.75, 857.5];
        let found: Vec<Option<f32>> = wanted
            .iter()
            .map(|w| p.iter().map(|q| q.0).find(|hz| (hz - w).abs() < 0.01 * w))
            .collect();
        println!("ir box 0.5 m: modes wanted {wanted:?} Hz, found {found:.1?}");
        assert!(found.iter().all(Option::is_some));
    }

    #[test]
    fn room_reverb_first_reflection_arrives_when_the_geometry_says() {
        // size 0.5: sides 11.5 x 9.2 x 4.6 m. The nearest first-order image to
        // the left ear, from the geometry alone.
        let l = 3.0 + 17.0 * 0.5f32;
        let dims = [l, l * 0.8, l * 0.4];
        let src = [dims[0] * 0.3, dims[1] * 0.45, dims[2] * 0.4];
        let ear = |side: f32| [dims[0] * 0.7, dims[1] * 0.5 + side * 0.09, dims[2] * 0.4];
        let dist =
            |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt();
        let r0 = dist(src, ear(-1.0)).min(dist(src, ear(1.0)));
        let mut first = f32::MAX;
        for axis in 0..3 {
            for wall in [0.0, dims[axis]] {
                let mut img = src;
                img[axis] = 2.0 * wall - src[axis];
                first = first.min(dist(img, ear(-1.0)));
            }
        }
        let want = (first - r0) / 343.0;
        let h = wet_ir(
            json!({"model": "room", "er_mix": 1.0, "predelay": 0.0}),
            0.5,
        );
        let got = h.iter().position(|v| v.abs() > 1e-9).unwrap() as f32 / SR as f32;
        println!(
            "room reverb, left ear: first reflection expected {:.3} ms, found {:.3} ms",
            want * 1e3,
            got * 1e3
        );
        assert!((got - want).abs() < 2.0 / SR as f32);
        // The early part carries energy the fdn alone has not built yet.
        let fdn = wet_ir(json!({"model": "fdn"}), 0.5);
        let early = |x: &[f32]| rms_db(&x[..SR as usize / 20]);
        println!(
            "first 50 ms rms: room er_mix 1 {:.1} dB, fdn {:.1} dB",
            early(&h),
            early(&fdn)
        );
        assert!(early(&h) > early(&fdn));
    }

    #[test]
    fn width_scales_the_side_signal() {
        let lr = |w: Option<f32>| {
            let extra = match w {
                Some(w) => json!({"model": "plate", "width": w}),
                None => json!({"model": "plate"}),
            };
            let mut nodes = source();
            nodes.push(json!({"type": "reverb", "name": "verb", "input": "src", "mix": 1.0}));
            nodes[5]
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let a = render(&nodes, 0.6);
            (a.channels[0].clone(), a.channels[1].clone())
        };
        let side_rms = |(l, r): &(Vec<f32>, Vec<f32>)| {
            let s: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect();
            rms_db(&s)
        };
        let (mono, own, wide) = (lr(Some(0.0)), lr(None), lr(Some(2.0)));
        println!(
            "plate side rms: width 0 {:.1} dB, own {:.1} dB, width 2 {:.1} dB",
            side_rms(&mono),
            side_rms(&own),
            side_rms(&wide)
        );
        assert!(
            mono.0
                .iter()
                .zip(&mono.1)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert!((side_rms(&wide) - side_rms(&own) - 6.02).abs() < 0.3);
    }

    #[test]
    fn every_model_is_bit_identical() {
        for model in ["fdn", "room", "plate", "spring"] {
            assert_bit_identical(with(json!({"model": model, "width": 1.5})), 0.8);
        }
        for model in ["room", "tube", "box", "cabinet", "phone"] {
            let mut nodes = source();
            nodes.push(ir_node(json!({"model": model})));
            nodes.push(json!({"type": "convolve", "name": "c", "input": "src", "ir": "ir1"}));
            assert_bit_identical(nodes, 0.8);
        }
    }

    #[test]
    fn every_model_is_finite_at_the_extremes() {
        let loud = vec![json!({"type": "osc", "name": "src", "freq": 220, "amp": 1.0e3})];
        for model in ["fdn", "room", "plate", "spring"] {
            for size in [0.0, 1.0] {
                for decay in [0.0, 1.0e6] {
                    for damp in [0.0, 1.0] {
                        for (predelay, width) in [(0.0, 0.0), (1.0e9, 2.0)] {
                            let mut nodes = loud.clone();
                            nodes.push(json!({"type": "reverb", "name": "verb", "input": "src",
                            "model": model, "size": size, "decay": decay, "damp": damp,
                            "predelay": predelay, "width": width, "mix": 1.0}));
                            let x = node_audio(&nodes, "verb", 0.6);
                            assert_finite(&x, &format!("{model} {size} {decay} {damp}"));
                            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                            assert!(
                                peak < 1.0e5,
                                "{model} size {size} decay {decay}: peak {peak}"
                            );
                        }
                    }
                }
            }
        }
        for model in ["room", "tube", "box", "cabinet", "phone"] {
            for size in [0.05, 100.0] {
                for absorption in [0.005, 1.0] {
                    let mut extra = json!({"model": model, "length": 30.0});
                    if model != "phone" {
                        extra["size"] = json!(size);
                    }
                    if matches!(model, "room" | "tube" | "box") {
                        extra["absorption"] = json!(absorption);
                    }
                    let h = ir(extra, 2.0);
                    assert_finite(&h, &format!("ir {model} {size} {absorption}"));
                    let peak = h.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                    assert!(peak < 100.0, "ir {model} {size} {absorption}: peak {peak}");
                }
            }
        }
        println!(
            "extremes: reverb 4 models x size 0/1 x decay 0/1e6 x damp 0/1 x predelay 0/1e9 at input 1e3, and ir 5 models x size 0.05/100 x absorption 0.005/1 x length 30: all finite and bounded"
        );
    }

    fn render_error(nodes: Vec<Value>) -> String {
        crate::common::render_error(nodes, "last", 0.1)
    }

    #[test]
    fn errors_say_the_field_value_and_fix() {
        let verb = |extra: Value| {
            let mut v = with(extra);
            v.last_mut().unwrap()["name"] = json!("last");
            render_error(v)
        };
        let e = verb(json!({"er_mix": 0.5}));
        println!("{e}");
        assert!(
            e.contains("er_mix: 0.5") && e.contains("\"model\": \"room\""),
            "{e}"
        );
        let e = verb(json!({"model": "room", "er_mix": 1.5}));
        println!("{e}");
        assert!(e.contains("er_mix: 1.5: give a number from 0 to 1"), "{e}");
        let e = verb(json!({"width": 3}));
        println!("{e}");
        assert!(
            e.contains("width: 3: give a number from 0 (mono) to 2"),
            "{e}"
        );
        let e = verb(json!({"model": "hall"}));
        println!("{e}");
        assert!(e.contains("hall") && e.contains("spring"), "{e}");
        let ir_err = |extra: Value| {
            let mut n = ir_node(extra);
            n["name"] = json!("last");
            render_error(vec![n])
        };
        let e = ir_err(json!({"size": 500}));
        println!("{e}");
        assert!(
            e.contains("size: 500: give a number from 0.05 to 100"),
            "{e}"
        );
        let e = ir_err(json!({"absorption": 0}));
        println!("{e}");
        assert!(
            e.contains("absorption: 0: give a number from 0.005 to 1"),
            "{e}"
        );
        let e = ir_err(json!({"length": 60}));
        println!("{e}");
        assert!(
            e.contains("length: 60: give a number from 0.01 to 30"),
            "{e}"
        );
        let e = ir_err(json!({"model": "phone", "size": 2}));
        println!("{e}");
        assert!(e.contains("size: 2: a phone has no size"), "{e}");
        let e = ir_err(json!({"model": "cabinet", "absorption": 0.2}));
        println!("{e}");
        assert!(
            e.contains("absorption: 0.2: only room, tube and box"),
            "{e}"
        );
        let e = ir_err(json!({"model": "garage"}));
        println!("{e}");
        assert!(e.contains("garage") && e.contains("cabinet"), "{e}");
    }

    #[test]
    fn ir_is_registered_and_reverb_documents_its_models() {
        let ex = sfx_graph::example("ir").expect("registered");
        let mut body = ex.clone();
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body("ir", &body, "ir1").unwrap();
        assert!(node.causal() && node.random());
        let phone = parse_body("ir", &json!({"model": "phone"}), "ir1").unwrap();
        assert!(!phone.random(), "only the room reads the seed");
        let text = sfx_graph::describe(Some("ir")).unwrap();
        assert!(text.contains("impulse response") && text.contains("Sabine"));
        let text = sfx_graph::describe(Some("reverb")).unwrap();
        assert!(text.contains("spring") && text.contains("er_mix") && text.contains("width"));
        assert!(sfx_graph::describe(None).unwrap().contains("- ir:"));
    }

    /// The recipe in docs/nodes/space.md, as written there.
    fn recipe() -> Vec<Value> {
        vec![
            json!({"type": "noise", "name": "n", "color": "white"}),
            json!({"type": "decay", "name": "e", "time": 0.004}),
            json!({"type": "math", "name": "snap", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "ir", "name": "hall", "model": "room", "size": 14, "absorption": 0.15}),
            json!({"type": "convolve", "name": "inroom", "input": "snap", "ir": "hall", "mix": 1.0}),
            json!({"type": "reverb", "name": "boing", "input": "snap", "model": "spring", "size": 0.4, "decay": 1.2, "mix": 1.0}),
            json!({"type": "mix", "name": "both", "inputs": ["inroom", "boing"], "to": "stereo"}),
        ]
    }

    #[test]
    fn recipe_renders() {
        let a = render(recipe(), 2.0);
        assert_eq!(a.len(), 2 * SR as usize);
        let l = &a.channels[0];
        println!("recipe: {} samples, left rms {:.2} dB", a.len(), rms_db(l));
        assert_finite(l, "recipe");
        assert!(rms_db(l) > -80.0);
    }
}

mod motion {
    //! `doppler`, `haas`, `decorrelate`. Each test measures and prints the
    //! numbers that go in docs/nodes/motion.md.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_graph::node::parse_body;
    use sfx_graph::{Buf, RenderOpts, SoundDoc};

    fn opts() -> RenderOpts {
        RenderOpts {
            cached: false,
            ..RenderOpts::default()
        }
    }

    /// Both channels of node `name` (a mono node returns the same twice), before
    /// any output stage.
    fn lr(nodes: &[Value], name: &str, duration: f32) -> (Vec<f32>, Vec<f32>) {
        let mut d = SoundDoc::named("test");
        d.nodes = nodes.to_vec();
        d.nodes
            .push(json!({"type": "output", "name": "_out", "input": name}));
        d.duration = Some(duration);
        let r = d.render(&opts()).unwrap_or_else(|e| panic!("{e}"));
        match &*r.eval.buf(name).unwrap() {
            Buf::Audio(x) => (x.clone(), x.clone()),
            Buf::Stereo(x) => (
                x.iter().map(|s| s[0]).collect(),
                x.iter().map(|s| s[1]).collect(),
            ),
            other => panic!("{}", other.kind()),
        }
    }

    fn render_error(nodes: Vec<Value>) -> String {
        crate::common::render_error(nodes, "m", 0.5)
    }

    fn node(extra: Value, ty: &str) -> Vec<Value> {
        let mut m = json!({"type": ty, "name": "m", "input": "src"});
        m.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![sine_src(1000.0, 0.5), m]
    }

    fn secs(x: &[f32], a: f32, b: f32) -> &[f32] {
        &x[(a * SR as f32) as usize..(b * SR as f32) as usize]
    }

    // ---------------------------------------------------------------- doppler

    #[test]
    fn doppler_bends_the_pitch_by_the_radial_speed() {
        // 34.3 m/s is a tenth of the speed of sound: 1000 / 0.9 = 1111.1 Hz
        // approaching, 1000 / 1.1 = 909.1 Hz receding.
        let nodes = node(
            json!({"speed": 34.3, "closest": 1, "at": 1.0, "pan": false, "air": false}),
            "doppler",
        );
        let (x, _) = lr(&nodes, "m", 2.0);
        assert_finite(&x, "doppler");
        let before = peak_hz(secs(&x, 0.1, 0.4), SR);
        let after = peak_hz(secs(&x, 1.6, 1.9), SR);
        println!(
            "1 kHz at 34.3 m/s, closest 1 m: {before:.2} Hz before (expect 1111.1), {after:.2} Hz after (expect 909.1)"
        );
        assert!((before / 1111.1 - 1.0).abs() < 0.01, "{before}");
        assert!((after / 909.1 - 1.0).abs() < 0.01, "{after}");
    }

    #[test]
    fn doppler_level_peaks_at_the_closest_approach() {
        // A constant input shows the gain curve itself: 1 / max(r, 1).
        for (closest, want) in [(1.0, 1.0), (5.0, 0.2), (0.2, 1.0)] {
            let nodes = vec![
                json!({"type": "doppler", "name": "m", "input": 1.0, "speed": 34.3, "closest": closest, "at": 1.0, "pan": false, "air": false}),
            ];
            let (x, _) = lr(&nodes, "m", 2.0);
            let (i, peak) = x
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(i, v)| (i, *v))
                .unwrap();
            let t = i as f32 / SR as f32;
            println!(
                "closest {closest} m: peak {peak:.4} at {:.2} ms from `at`",
                (t - 1.0) * 1000.0
            );
            // Inside 1 m the level is flat (1 / max(r, 1)), so only the value
            // is checked there.
            assert!(closest < 1.0 || (t - 1.0).abs() < 0.005, "peak at {t}");
            assert!((peak - want).abs() < 2e-3, "{peak} vs {want}");
        }
        // The same on a sine, through a windowed RMS (10 ms, hop 1 ms): the peak
        // of a sine's envelope is flat to 0.1 dB within 5 ms, so it is only
        // required to land within 15 ms.
        let nodes = node(
            json!({"speed": 34.3, "closest": 1, "at": 1.0, "pan": false, "air": false}),
            "doppler",
        );
        let (x, _) = lr(&nodes, "m", 2.0);
        let w = SR as usize / 100;
        let (best, _) = (0..x.len() - w)
            .step_by(SR as usize / 1000)
            .map(|s| (s, rms_db(&x[s..s + w])))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let t = (best + w / 2) as f32 / SR as f32;
        println!(
            "sine: loudest 10 ms window centred at {:.1} ms from `at`",
            (t - 1.0) * 1000.0
        );
        assert!((t - 1.0).abs() < 0.015);
    }

    #[test]
    fn doppler_pan_crosses_the_centre_at_the_closest_approach() {
        let nodes = vec![
            json!({"type": "doppler", "name": "m", "input": 1.0, "speed": 30, "closest": 3, "at": 0.8, "air": false}),
        ];
        let (l, r) = lr(&nodes, "m", 1.6);
        // Skip the silence before the first sound arrives.
        let start = (0.1 * SR as f32) as usize;
        let cross = (start..l.len())
            .find(|&i| l[i] <= r[i])
            .expect("L falls below R");
        let t = cross as f32 / SR as f32;
        println!(
            "pan crosses the centre at {:.2} ms from `at`; start L {:.3} R {:.3}; end L {:.3} R {:.3}",
            (t - 0.8) * 1000.0,
            l[start],
            r[start],
            l[l.len() - 1],
            r[r.len() - 1]
        );
        assert!((t - 0.8).abs() < 0.001, "{t}");
        assert!(l[start] > 3.0 * r[start], "starts on the left");
        assert!(r[r.len() - 1] > 3.0 * l[l.len() - 1], "ends on the right");
        // Equal power: the pan law's L^2 + R^2 is the gain squared.
        let i = cross;
        assert!((l[i] - r[i]).abs() < 5e-3);
    }

    #[test]
    fn doppler_air_takes_the_highs_off_a_far_pass() {
        let noise = json!({"type": "noise", "name": "src", "amp": 0.3});
        let run = |air: bool| {
            let m = json!({"type": "doppler", "name": "m", "input": "src", "speed": 30, "closest": 200, "at": 1.0, "pan": false, "air": air});
            lr(&[noise.clone(), m], "m", 2.0).0
        };
        let (on, off) = (run(true), run(false));
        let hi = |x: &[f32]| band_db(secs(x, 0.8, 1.2), SR, 8000.0, 16000.0);
        let lo = |x: &[f32]| band_db(secs(x, 0.8, 1.2), SR, 200.0, 800.0);
        println!(
            "closest 200 m, around `at`: 8-16 kHz {:.1} dB with air, {:.1} dB without ({:.1} dB lower); 200-800 Hz {:.1} vs {:.1} dB",
            hi(&on),
            hi(&off),
            hi(&off) - hi(&on),
            lo(&on),
            lo(&off)
        );
        assert!(hi(&off) - hi(&on) > 6.0);
        assert!((lo(&off) - lo(&on)).abs() < 1.0);
    }

    #[test]
    fn doppler_follows_a_distance_signal() {
        // A constant 343 m is exactly one second late and 1 / 343 as loud.
        let nodes = vec![
            json!({"type": "impulse", "name": "src", "at": 0.1}),
            json!({"type": "doppler", "name": "m", "input": "src", "distance": 343, "air": false}),
        ];
        let (x, y) = lr(&nodes, "m", 1.5);
        assert_eq!(x, y, "mono out");
        let (i, v) = x
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, v)| (i, *v))
            .unwrap();
        println!(
            "distance 343 m: impulse at 0.1 s arrives at sample {i} (expected {}), height {v:.5} (expected {:.5})",
            SR + SR / 10,
            1.0 / 343.0
        );
        assert!(i.abs_diff((SR + SR / 10) as usize) <= 1);
        assert!((v - 1.0 / 343.0).abs() < 2e-4);
        // The distance is as heard at each moment, so a ramp 100 -> 0 m over 1 s
        // (100 m/s) moves the heard phase at 1 + 100 / 343: 1291.5 Hz.
        let nodes = vec![
            sine_src(1000.0, 0.5),
            json!({"type": "ramp", "name": "d", "from": 100, "to": 0, "time": 1.0, "curve": "linear"}),
            json!({"type": "doppler", "name": "m", "input": "src", "distance": "d", "air": false}),
        ];
        let (x, _) = lr(&nodes, "m", 1.0);
        let p = peak_hz(secs(&x, 0.3, 0.7), SR);
        println!("distance ramp 100 -> 0 m in 1 s: {p:.1} Hz (expect 1291.5)");
        assert!((p / 1291.5 - 1.0).abs() < 0.01, "{p}");
    }

    #[test]
    fn doppler_is_bit_identical() {
        assert_bit_identical(
            node(json!({"speed": 77, "closest": 2.5, "at": 0.2}), "doppler"),
            0.4,
        );
        let mut d = node(json!({"distance": "dist"}), "doppler");
        d.insert(
            1,
            json!({"type": "ramp", "name": "dist", "from": 50, "to": 2, "time": 0.3}),
        );
        assert_bit_identical(d, 0.4);
    }

    #[test]
    fn doppler_is_finite_at_the_extremes() {
        for speed in [0.0, 1e-6, 300.0] {
            for closest in [1e-6, 1.0, 5000.0] {
                for at in [0.0, 0.1, 0.5] {
                    for pan in [true, false] {
                        let nodes = vec![
                            json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6}),
                            json!({"type": "doppler", "name": "m", "input": "src", "speed": speed, "closest": closest, "at": at, "pan": pan}),
                        ];
                        let (l, r) = lr(&nodes, "m", 0.5);
                        assert_finite(&l, &format!("speed {speed} closest {closest} at {at}"));
                        assert_finite(&r, "r");
                        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
                        assert!(peak <= 1.3e6, "{peak}");
                    }
                }
            }
        }
        // A distance signal far out of range is clamped.
        let nodes = vec![
            sine_src(1000.0, 0.5),
            json!({"type": "ramp", "name": "d", "from": -1.0e9, "to": 1.0e9, "time": 0.2, "curve": "linear"}),
            json!({"type": "doppler", "name": "m", "input": "src", "distance": "d"}),
        ];
        assert_finite(&lr(&nodes, "m", 0.4).0, "distance far out of range");
        println!(
            "extremes: speed 0..300 x closest 1e-6..5000 x at 0..0.5 x pan, input 1e6, and a +-1e9 m distance sweep: all finite"
        );
    }

    #[test]
    fn doppler_errors_say_the_field_value_and_fix() {
        let cases = [
            (
                json!({"speed": 400}),
                "speed: 400: give a number from 0 to 300 m/s",
            ),
            (json!({"closest": 0}), "closest: 0: give a distance above 0"),
            (json!({"closest": 6000}), "closest: 6000"),
            (
                json!({"at": -1}),
                "at: -1: give a time in seconds, 0 or more",
            ),
            (
                json!({"distance": 100, "speed": 30}),
                "speed: not used with `distance`",
            ),
            (
                json!({"distance": 100, "pan": true}),
                "pan: true needs a bearing",
            ),
            (
                json!({"distance": 99999}),
                "distance: 99999: give a number from 0 to",
            ),
            (json!({"spd": 5}), "spd"),
        ];
        for (extra, want) in cases {
            let e = render_error(node(extra, "doppler"));
            println!("{e}");
            assert!(e.contains(want), "wanted `{want}` in {e}");
        }
        // A start so far away the delay line would pass the cap.
        let nodes = vec![
            sine_src(1000.0, 0.5),
            json!({"type": "doppler", "name": "m", "input": "src", "speed": 300, "at": 60}),
        ];
        let e = render_error(nodes);
        println!("{e}");
        assert!(
            e.contains("at: 60") && e.contains("smaller at or speed"),
            "{e}"
        );
        // Stereo into a panned doppler.
        let e = render_error(vec![
            sine_src(1000.0, 0.5),
            json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
            json!({"type": "doppler", "name": "m", "input": "st"}),
        ]);
        println!("{e}");
        assert!(e.contains("takes mono") && e.contains("pan false"), "{e}");
    }

    // ------------------------------------------------------------------- haas

    #[test]
    fn haas_delays_one_side_by_the_stated_time() {
        let imp = json!({"type": "impulse", "name": "src", "at": 0.1});
        for (side, delay) in [("right", 0.012f32), ("left", 0.025)] {
            let nodes = vec![
                imp.clone(),
                json!({"type": "haas", "name": "m", "input": "src", "delay": delay, "side": side}),
            ];
            let (l, r) = lr(&nodes, "m", 0.3);
            let at = |x: &[f32]| x.iter().position(|v| v.abs() > 0.5).unwrap();
            let (il, ir) = (at(&l), at(&r));
            println!(
                "side {side}, delay {delay}: left impulse at sample {il}, right at {ir} (difference {} samples, expected {})",
                ir as i64 - il as i64,
                (delay * SR as f32).round()
            );
            let diff = ir as i64 - il as i64;
            let want = (delay * SR as f32).round() as i64;
            assert_eq!(il.min(ir), (0.1 * SR as f32) as usize);
            assert_eq!(diff.abs(), want);
            assert_eq!(diff > 0, side == "right");
            // The undelayed channel is the input bit for bit.
            let src = node_audio(&nodes[..1], "src", 0.3);
            let early = if side == "right" { &l } else { &r };
            assert!(
                early
                    .iter()
                    .zip(&src)
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            );
            assert!((l.iter().chain(&r).map(|v| v.abs()).sum::<f32>() - 2.0).abs() < 1e-3);
        }
        // delay 0: two identical channels.
        let nodes = vec![
            imp.clone(),
            json!({"type": "haas", "name": "m", "input": "src", "delay": 0}),
        ];
        let (l, r) = lr(&nodes, "m", 0.3);
        assert!(l.iter().zip(&r).all(|(a, b)| a.to_bits() == b.to_bits()));
        // A fractional delay keeps the level of a 1 kHz sine.
        let nodes = node(json!({"delay": 0.0123456}), "haas");
        let (l, r) = lr(&nodes, "m", 1.0);
        let (a, b) = (rms_db(secs(&l, 0.2, 0.9)), rms_db(secs(&r, 0.2, 0.9)));
        println!("fractional delay 12.3456 ms: left {a:.3} dB, right {b:.3} dB rms");
        assert!((a - b).abs() < 0.05);
    }

    #[test]
    fn haas_is_bit_identical_finite_and_errors() {
        assert_bit_identical(node(json!({"delay": 0.0217, "side": "left"}), "haas"), 0.3);
        let nodes = vec![
            json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6}),
            json!({"type": "ramp", "name": "d", "from": -5, "to": 5, "time": 0.2, "curve": "linear"}),
            json!({"type": "haas", "name": "m", "input": "src", "delay": "d"}),
        ];
        let (l, r) = lr(&nodes, "m", 0.3);
        assert_finite(&l, "l");
        assert_finite(&r, "r");
        for extra in [json!({"delay": 0.05}), json!({"delay": 0})] {
            let (l, r) = lr(&node(extra, "haas"), "m", 0.2);
            assert_finite(&l, "l");
            assert_finite(&r, "r");
        }
        let e = render_error(node(json!({"delay": 0.2}), "haas"));
        println!("{e}");
        assert!(
            e.contains("delay: 0.2: give a number from 0 to 0.05 seconds"),
            "{e}"
        );
        let e = render_error(node(json!({"side": "middle"}), "haas"));
        println!("{e}");
        assert!(
            e.contains("middle") && e.contains("left") && e.contains("right"),
            "{e}"
        );
        let e = render_error(vec![
            sine_src(1000.0, 0.5),
            json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
            json!({"type": "haas", "name": "m", "input": "st"}),
        ]);
        assert!(e.contains("input") && e.contains("stereo"), "{e}");
    }

    // ------------------------------------------------------------ decorrelate

    fn correlation(l: &[f32], r: &[f32]) -> f32 {
        let (mut lr_, mut ll, mut rr) = (0.0f64, 0.0f64, 0.0f64);
        for (a, b) in l.iter().zip(r) {
            lr_ += f64::from(*a) * f64::from(*b);
            ll += f64::from(*a) * f64::from(*a);
            rr += f64::from(*b) * f64::from(*b);
        }
        (lr_ / (ll * rr).sqrt().max(1e-30)) as f32
    }

    #[test]
    fn decorrelate_splits_noise_into_uncorrelated_flat_channels() {
        let nodes = vec![
            json!({"type": "noise", "name": "src", "amp": 0.3}),
            json!({"type": "decorrelate", "name": "m", "input": "src", "amount": 1}),
        ];
        let secs_n = 4.0;
        let src = node_audio(&nodes[..1], "src", secs_n);
        let (l, r) = lr(&nodes, "m", secs_n);
        assert_finite(&l, "l");
        assert_finite(&r, "r");
        // The kernels add up to ~20 ms of delay; compare the settled part.
        let skip = SR as usize / 10;
        let (cl, cr) = (&l[skip..], &r[skip..]);
        let rho = correlation(cl, cr);
        let rho_in = correlation(&src[skip..], cl);
        println!(
            "noise: L/R correlation {rho:.3}; input/L correlation {rho_in:.3}; rms in {:.2} L {:.2} R {:.2} dB",
            rms_db(&src[skip..]),
            rms_db(cl),
            rms_db(cr)
        );
        assert!(rho.abs() < 0.3, "{rho}");
        // Octave bands: each channel against the input, and the power-preserving
        // mono sum (L + R) / sqrt 2 against the input.
        let mono: Vec<f32> = l
            .iter()
            .zip(&r)
            .map(|(a, b)| (a + b) * std::f32::consts::FRAC_1_SQRT_2)
            .collect();
        let mut worst = (0.0f32, 0.0f32, 0.0f32);
        let mut lo = 62.5f32;
        while lo < 16_000.0 {
            let hi = lo * 2.0;
            let (a, b, c, m) = (
                band_db(&src[skip..], SR, lo, hi),
                band_db(cl, SR, lo, hi),
                band_db(cr, SR, lo, hi),
                band_db(&mono[skip..], SR, lo, hi),
            );
            println!(
                "  {lo:>7.1}-{hi:>7.1} Hz: input {a:.2}  L {:+.2}  R {:+.2}  mono {:+.2} dB",
                b - a,
                c - a,
                m - a
            );
            worst.0 = worst.0.max((b - a).abs()).max((c - a).abs());
            worst.1 = worst.1.max((m - a).abs());
            assert!(
                (b - a).abs() < 1.0 && (c - a).abs() < 1.0,
                "{lo}: {b} {c} vs {a}"
            );
            assert!((m - a).abs() < 3.0, "{lo}: mono {m} vs {a}");
            lo = hi;
        }
        println!(
            "largest band deviation: channels {:.2} dB, mono sum {:.2} dB",
            worst.0, worst.1
        );
    }

    /// |H|^2 per bin of an impulse response, zero-padded to 2^16 points (a
    /// rectangular window: the response itself, no taper).
    fn power_response(h: &[f32]) -> Vec<f32> {
        use realfft::RealFftPlanner;
        let size = 1 << 16;
        let mut input = vec![0.0f32; size];
        input[..h.len()].copy_from_slice(h);
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let mut out = fft.make_output_vec();
        fft.process(&mut input, &mut out).unwrap();
        out.iter().map(|z| z.norm_sqr()).collect()
    }

    #[test]
    fn decorrelate_kernels_are_flat_all_pass_filters() {
        // An impulse through the node is the pair of impulse responses.
        let nodes = vec![
            json!({"type": "impulse", "name": "src", "at": 0.0}),
            json!({"type": "decorrelate", "name": "m", "input": "src", "seed": 5}),
        ];
        let (l, r) = lr(&nodes, "m", 0.1);
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let bin_hz = SR as f32 / (1 << 16) as f32;
        let (mut band_min, mut band_max) = (f32::MAX, f32::MIN);
        let (mut bin_min, mut bin_max) = (f32::MAX, f32::MIN);
        for h in [&l, &r] {
            let p = power_response(h);
            // Per bin from 100 Hz to 16 kHz.
            for v in &p[(100.0 / bin_hz) as usize..(16_000.0 / bin_hz) as usize] {
                let d = 10.0 * v.max(1e-12).log10();
                bin_min = bin_min.min(d);
                bin_max = bin_max.max(d);
            }
            // Mean power per third octave.
            let k = 2f32.powf(1.0 / 3.0);
            let mut lo = 100.0f32;
            while lo * k < 16_000.0 {
                let (a, b) = ((lo / bin_hz) as usize, (lo * k / bin_hz) as usize);
                let m = p[a..b].iter().sum::<f32>() / (b - a) as f32;
                let d = 10.0 * m.log10();
                band_min = band_min.min(d);
                band_max = band_max.max(d);
                lo *= k;
            }
        }
        println!(
            "impulse response energy L {:.4} R {:.4}; |H| per bin 100 Hz - 16 kHz {bin_min:+.2} .. {bin_max:+.2} dB; per third octave {band_min:+.2} .. {band_max:+.2} dB",
            energy(&l),
            energy(&r)
        );
        assert!((energy(&l) - 1.0).abs() < 0.02 && (energy(&r) - 1.0).abs() < 0.02);
        assert!(band_min > -0.5 && band_max < 0.5);
        assert!(bin_min > -3.0 && bin_max < 3.0);
        // The two channels differ and so do two seeds.
        assert_ne!(l, r);
        let other = vec![
            nodes[0].clone(),
            json!({"type": "decorrelate", "name": "m", "input": "src", "seed": 6}),
        ];
        let (l2, _) = lr(&other, "m", 0.1);
        assert_ne!(l, l2);
    }

    #[test]
    fn decorrelate_amount_zero_is_two_copies_and_one_pulls_apart_smoothly() {
        let nodes = node(json!({"amount": 0}), "decorrelate");
        let src = node_audio(&nodes[..1], "src", 0.3);
        let (l, r) = lr(&nodes, "m", 0.3);
        assert!(l.iter().zip(&src).all(|(a, b)| a.to_bits() == b.to_bits()));
        assert!(r.iter().zip(&src).all(|(a, b)| a.to_bits() == b.to_bits()));
        // Brown noise keeps its energy low, where the filters' group delay is
        // small, so the amount shows in the correlation.
        let noise = json!({"type": "noise", "name": "src", "color": "brown", "amp": 0.3});
        let mut last = 1.1f32;
        for amount in [0.02, 0.1, 0.25, 0.5, 1.0] {
            let n = vec![
                noise.clone(),
                json!({"type": "decorrelate", "name": "m", "input": "src", "amount": amount}),
            ];
            let (l, r) = lr(&n, "m", 4.0);
            let rho = correlation(&l[4800..], &r[4800..]);
            println!("brown noise, amount {amount}: L/R correlation {rho:.3}");
            assert!(rho < last + 0.02, "correlation falls as amount grows");
            last = rho;
        }
        assert!(last.abs() < 0.3, "{last}");
    }

    #[test]
    fn decorrelate_is_bit_identical_finite_and_errors() {
        assert_bit_identical(
            vec![
                json!({"type": "noise", "name": "src", "amp": 0.3}),
                json!({"type": "decorrelate", "name": "m", "input": "src", "seed": 3, "amount": 0.7}),
            ],
            0.3,
        );
        for amount in [0.0, 1e-6, 1.0] {
            let nodes = vec![
                json!({"type": "osc", "name": "src", "freq": 1000, "amp": 1.0e6}),
                json!({"type": "decorrelate", "name": "m", "input": "src", "amount": amount}),
            ];
            let (l, r) = lr(&nodes, "m", 0.2);
            assert_finite(&l, "l");
            assert_finite(&r, "r");
            let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak < 6.0e6, "{peak}");
        }
        let e = render_error(node(json!({"amount": 1.5}), "decorrelate"));
        println!("{e}");
        assert!(e.contains("amount: 1.5: give a number from 0 to 1"), "{e}");
        let e = render_error(vec![
            sine_src(1000.0, 0.5),
            json!({"type": "pan", "name": "st", "input": "src", "pan": 0.0}),
            json!({"type": "decorrelate", "name": "m", "input": "st"}),
        ]);
        assert!(e.contains("input") && e.contains("mix node"), "{e}");
    }

    // ----------------------------------------------------------- registration

    #[test]
    fn motion_nodes_are_registered_with_flags() {
        for (ty, random) in [("doppler", false), ("haas", false), ("decorrelate", true)] {
            let mut body = sfx_graph::example(ty).expect("registered");
            body.as_object_mut().unwrap().remove("type");
            body.as_object_mut().unwrap().remove("name");
            let n = parse_body(ty, &body, "x").unwrap();
            assert!(n.causal(), "{ty} is causal");
            assert_eq!(n.random(), random, "{ty} random");
            let text = sfx_graph::describe(Some(ty)).unwrap();
            assert!(text.contains(ty));
            assert!(
                sfx_graph::describe(None)
                    .unwrap()
                    .contains(&format!("- {ty}:"))
            );
        }
    }

    /// The recipe in docs/nodes/motion.md, as written there.
    fn recipe() -> Vec<Value> {
        vec![
            json!({"type": "noise", "name": "engine", "color": "brown", "amp": 0.5}),
            json!({"type": "filter", "name": "growl", "input": "engine", "mode": "bp", "freq": 300, "q": 2}),
            json!({"type": "doppler", "name": "pass", "input": "growl", "speed": 25, "closest": 4, "at": 2.0}),
            json!({"type": "decorrelate", "name": "wide", "input": "growl", "amount": 0.6, "seed": 2}),
        ]
    }

    #[test]
    fn motion_recipe_renders() {
        let (l, r) = lr(&recipe(), "pass", 4.0);
        assert_finite(&l, "pass");
        let early = rms_db(secs(&l, 0.0, 0.5)) - rms_db(secs(&r, 0.0, 0.5));
        let late = rms_db(secs(&l, 3.5, 4.0)) - rms_db(secs(&r, 3.5, 4.0));
        println!(
            "recipe: L minus R rms {early:+.1} dB in the first 0.5 s, {late:+.1} dB in the last 0.5 s; peak rms {:.1} dB",
            rms_db(secs(&l, 1.9, 2.1))
        );
        assert!(early > 3.0 && late < -3.0);
        let (a, b) = lr(&recipe(), "wide", 1.0);
        assert!(correlation(&a, &b).abs() < 0.6);
    }
}

mod camera {
    //! `camera`: distance, air, delay, ground reflection, space presets.

    use crate::common::*;
    use serde_json::{Value, json};

    fn src() -> Vec<Value> {
        vec![
            json!({"type": "noise", "name": "n", "color": "white", "seed": 4}),
            json!({"type": "decay", "name": "e", "time": 0.05}),
            json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "e"}),
        ]
    }

    fn cam(extra: Value) -> Vec<Value> {
        let mut v = src();
        v.push(with_fields(
            json!({"type": "camera", "name": "cam", "input": "src"}),
            extra,
        ));
        v
    }

    #[test]
    fn off_is_bit_identical_passthrough() {
        let a = node_audio(cam(json!({})), "cam", 0.5);
        let b = node_audio(src(), "src", 0.5);
        assert_eq!(fnv(&a), fnv(&b));
    }

    #[test]
    fn inverse_law_falls_6_db_per_doubling_and_toggles_off() {
        let near = rms_db(&node_audio(
            cam(json!({"distance": 2, "air": false})),
            "cam",
            0.5,
        ));
        let far = rms_db(&node_audio(
            cam(json!({"distance": 4, "air": false})),
            "cam",
            0.5,
        ));
        assert!((near - far - 6.02).abs() < 0.1, "{near} {far}");
        let dry = rms_db(&node_audio(src(), "src", 0.5));
        let off = rms_db(&node_audio(
            cam(json!({"distance": 40, "air": false, "inverse_law": false})),
            "cam",
            0.5,
        ));
        assert!((off - dry).abs() < 0.01, "{off} {dry}");
    }

    #[test]
    fn air_takes_the_highs_with_distance() {
        let x = node_audio(src(), "src", 0.5);
        let y = node_audio(
            cam(json!({"distance": 200, "inverse_law": false})),
            "cam",
            0.5,
        );
        let lo = band_db(&x, SR, 200.0, 1000.0) - band_db(&y, SR, 200.0, 1000.0);
        let hi = band_db(&x, SR, 8000.0, 12000.0) - band_db(&y, SR, 8000.0, 12000.0);
        // ISO 9613-1 at 10 kHz, 50 %: about 0.15 dB/m, so 200 m takes tens of dB.
        assert!(lo < 1.0 && hi > 15.0, "lo {lo} hi {hi}");
    }

    #[test]
    fn delay_shifts_by_distance_over_c() {
        let v = vec![
            json!({"type": "impulse", "name": "src", "at": 0.01}),
            json!({"type": "camera", "name": "cam", "input": "src", "distance": 34.3, "air": false, "inverse_law": false, "delay": true}),
        ];
        let y = node_audio(v, "cam", 0.3);
        let i = y
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        assert!((i as i64 - (0.11 * SR as f32) as i64).abs() <= 2, "{i}");
    }

    #[test]
    fn ground_reflection_arrives_at_the_path_difference() {
        let v = vec![
            json!({"type": "impulse", "name": "src", "at": 0.01}),
            json!({"type": "camera", "name": "cam", "input": "src", "distance": 10, "air": false,
                   "source_height": 1.0, "camera_height": 1.0, "ground": 1.0}),
        ];
        let y = node_audio(v, "cam", 0.2);
        let dt = ((10f32.powi(2) + 4.0).sqrt() - 10.0) / 343.0;
        let at = ((0.01 + dt) * SR as f32).round() as usize;
        let around = y[at - 2..at + 3].iter().fold(0f32, |m, s| m.max(s.abs()));
        let d = y[(0.01 * SR as f32) as usize];
        assert!(around > 0.8 * d && around < d, "refl {around} direct {d}");
    }

    #[test]
    fn spaces_are_stereo_and_rank_by_tail() {
        let tail = |space: &str| {
            let a = render(cam(json!({"space": space})), 3.0);
            assert_eq!(a.channels.len(), 2, "{space}");
            let x = &a.channels[0];
            rms_db(&x[(1.0 * SR as f32) as usize..])
        };
        let (room, hall) = (tail("room"), tail("hall"));
        assert!(hall > room + 10.0, "room {room} hall {hall}");
        for s in ["outdoor", "street"] {
            assert_finite(&render(cam(json!({"space": s})), 2.0).channels[0], s);
        }
    }

    #[test]
    fn farther_camera_hears_more_room() {
        let ratio = |d: f32| {
            let dry = node_audio(cam(json!({"distance": d, "air": false})), "cam", 2.0);
            let all = node_audio(
                cam(json!({"distance": d, "air": false, "space": "room"})),
                "cam",
                2.0,
            );
            let wet: Vec<f32> = all.iter().zip(&dry).map(|(a, b)| a - b).collect();
            rms_db(&dry) - rms_db(&wet)
        };
        let (near, far) = (ratio(1.0), ratio(8.0));
        assert!((near - far - 18.06).abs() < 0.5, "{near} {far}");
    }

    #[test]
    fn moving_distance_and_errors() {
        let mut v = src();
        v.push(json!({"type": "ramp", "name": "d", "from": 5, "to": 50, "time": 0.5}));
        v.push(json!({"type": "camera", "name": "cam", "input": "src", "distance": "d", "delay": true}));
        assert_finite(&node_audio(v, "cam", 0.6), "moving");
        assert!(render_error(cam(json!({"air": false})), "cam", 0.3).contains("need a distance"));
        assert!(render_error(cam(json!({"space_db": 3})), "cam", 0.3).contains("need a space"));
    }
}
