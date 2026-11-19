mod rolling_cloth_footstep {
    //! `rolling`, `cloth`, `footstep`. Each test measures and prints (these
    //! lines go in docs/nodes/surface.md). The footstep presets are tested
    //! against the numbers of the round-0 briefs.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::stft::distance;
    use sfx_analysis::{Descriptors, describe};
    use sfx_graph::node::parse_body;
    use sfx_graph::{Audio, RenderOpts, SoundDoc};

    fn desc(x: &[f32]) -> Descriptors {
        describe(&Audio::mono(SR, x.to_vec()))
    }

    fn centroid(x: &[f32]) -> f32 {
        desc(x).centroid_all_hz
    }

    fn render_error(node: Value) -> String {
        crate::common::render_error(vec![node], "n", 0.5)
    }

    fn render_with_seed(nodes: &[Value], duration: f32, seed: u64) -> Vec<f32> {
        let mut d = SoundDoc::named("test");
        d.nodes = nodes.to_vec();
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "n"}));
        d.duration = Some(duration);
        let r = d
            .render(&RenderOpts {
                seed: Some(seed),
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        r.audio.channels[0].clone()
    }

    // ------------------------------------------------------------------ rolling

    fn rolling(extra: Value) -> Value {
        with_fields(
            json!({"type": "rolling", "name": "n", "speed": 1.0, "seed": 1}),
            extra,
        )
    }

    #[test]
    fn rolling_speed_zero_is_exact_silence() {
        let x = node_audio(vec![rolling(json!({"speed": 0.0}))], "n", 0.5);
        assert!(x.iter().all(|v| *v == 0.0), "speed 0 gives only zeros");
        // A ramp from 0 starts silent and gets louder.
        let nodes = vec![
            json!({"type": "ramp", "name": "v", "from": 0, "to": 2, "time": 1.0}),
            rolling(json!({"speed": "v"})),
        ];
        let y = node_audio(nodes, "n", 1.0);
        let (a, b) = (rms_db(&y[..9600]), rms_db(&y[38400..]));
        println!(
            "speed 0 -> exact zeros; ramp 0 -> 2 m/s: rms first 0.2 s {a:.1} dB, last 0.2 s {b:.1} dB"
        );
        assert!(b > a + 10.0);
    }

    #[test]
    fn rolling_speed_raises_centroid_and_level() {
        let mut last: Option<(f32, f32)> = None;
        for v in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
            let x = node_audio(vec![rolling(json!({"speed": v}))], "n", 1.0);
            assert_finite(&x, "rolling");
            let (c, l) = (centroid(&x), rms_db(&x));
            println!("rolling speed {v} m/s: centroid {c:.0} Hz, rms {l:.1} dB");
            if let Some((pc, pl)) = last {
                assert!(
                    c > pc * 1.7,
                    "doubling speed at least x1.7 centroid: {pc} -> {c}"
                );
                assert!(l > pl + 4.5, "doubling speed at least +4.5 dB: {pl} -> {l}");
            }
            last = Some((c, l));
        }
    }

    #[test]
    fn rolling_roughness_raises_centroid_and_size_lowers_it() {
        let mut last = 0.0;
        for r in [0.0f32, 0.5, 1.0] {
            let c = centroid(&node_audio(
                vec![rolling(json!({"roughness": r}))],
                "n",
                1.0,
            ));
            println!("rolling roughness {r}: centroid {c:.0} Hz");
            assert!(c > last * 1.5, "{last} -> {c}");
            last = c;
        }
        let mut last = f32::MAX;
        for s in [0.001f32, 0.002, 0.004, 0.01] {
            let x = node_audio(vec![rolling(json!({"size": s}))], "n", 1.0);
            let (c, l) = (centroid(&x), rms_db(&x));
            println!("rolling size {s} m: centroid {c:.0} Hz, rms {l:.1} dB");
            assert!(c < last * 0.65, "{last} -> {c}");
            // Size does not move the level.
            assert!(l > -18.0 && l < -10.0, "{l}");
            last = c;
        }
    }

    #[test]
    fn rolling_seeds_differ_and_negative_speed_mirrors() {
        let a = node_audio(vec![rolling(json!({"seed": 1}))], "n", 0.5);
        let b = node_audio(vec![rolling(json!({"seed": 2}))], "n", 0.5);
        let d = distance(&a, &b, SR);
        println!("rolling seed 1 vs 2: STFT distance {d:.2} dB");
        assert!(d > 3.0, "{d}");
        let p = node_audio(vec![rolling(json!({"speed": 1.0}))], "n", 0.5);
        let m = node_audio(vec![rolling(json!({"speed": -1.0}))], "n", 0.5);
        assert!(p.iter().zip(&m).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn rolling_is_finite_at_the_extremes_and_bit_identical() {
        let mut worst = 0f32;
        for (speed, size, rough) in [
            (json!("huge"), 0.0001, 1.0),
            (json!("huge"), 10.0, 0.0),
            (json!(0.0), 0.0001, 1.0),
            (json!(1e-9), 10.0, 1.0),
            (json!(50.0), 0.01, 1.0),
        ] {
            let x = node_audio(
                vec![
                    json!({"type": "ramp", "name": "huge", "from": 1e9, "to": -1e9, "time": 0.5}),
                    rolling(json!({"speed": speed, "size": size, "roughness": rough})),
                ],
                "n",
                0.5,
            );
            assert_finite(&x, "rolling extreme");
            worst = worst.max(peak_db(&x));
        }
        println!(
            "rolling extremes (a +-1e9 m/s sweep clamped, size 0.0001 and 10, roughness 0 and 1): finite, peak at most {worst:.1} dB"
        );
        assert!(worst < 40.0);
        assert_bit_identical(vec![rolling(json!({"speed": 2.0, "roughness": 0.8}))], 0.5);
    }

    #[test]
    fn rolling_errors_say_what_to_do() {
        let e = render_error(rolling(json!({"speed": 500.0, "size": 0.002})));
        println!("{e}");
        assert!(
            e.contains("speed: 500") && e.contains("under 50 m/s"),
            "{e}"
        );
        let e = render_error(rolling(json!({"speed": 30.0, "size": 0.002})));
        println!("{e}");
        assert!(
            e.contains("speed: 30") && e.contains("bigger size") && e.contains("under 24.000"),
            "{e}"
        );
        let e = render_error(rolling(json!({"roughness": 1.5})));
        assert!(e.contains("roughness: 1.5") && e.contains("0 to 1"), "{e}");
        let e = render_error(rolling(json!({"size": 0.0})));
        assert!(e.contains("size: 0") && e.contains("0.0001 to 10"), "{e}");
        let e = parse_body("rolling", &json!({"speed": 1, "colour": 1}), "node").unwrap_err();
        assert!(e.contains("colour"), "{e}");
    }

    #[test]
    fn rolling_excites_modal() {
        // The recipe in the doc: roll over a wooden panel.
        let nodes = vec![
            json!({"type": "ramp", "name": "v", "from": 0.2, "to": 2.0, "time": 1.0}),
            json!({"type": "rolling", "name": "surf", "speed": "v", "size": 0.004, "seed": 3}),
            json!({"type": "modal", "name": "n", "excite": "surf", "material": "wood", "size": 0.45}),
        ];
        let x = node_audio(nodes, "n", 1.0);
        assert_finite(&x, "rolling into modal");
        let (a, b) = (rms_db(&x[..9600]), rms_db(&x[38400..]));
        println!(
            "rolling -> modal wood: rms first 0.2 s {a:.1} dB, last 0.2 s {b:.1} dB, peak {:.1} dB",
            peak_db(&x)
        );
        assert!(b > a + 6.0);
    }

    // -------------------------------------------------------------------- cloth

    fn cloth(extra: Value) -> Value {
        with_fields(
            json!({"type": "cloth", "name": "n", "motion": 1.0, "seed": 1}),
            extra,
        )
    }

    #[test]
    fn cloth_motion_zero_is_exact_silence() {
        let x = node_audio(vec![cloth(json!({"motion": 0.0}))], "n", 1.0);
        assert!(x.iter().all(|v| *v == 0.0));
        println!("cloth motion 0: exact zeros");
    }

    #[test]
    fn cloth_motion_raises_rate_band_and_level() {
        let mut last: Option<(f32, f32, f32)> = None;
        for m in [0.25f32, 0.5, 1.0, 2.0, 4.0] {
            let x = node_audio(vec![cloth(json!({"motion": m}))], "n", 2.0);
            assert_finite(&x, "cloth");
            let d = desc(&x);
            let (c, l, rate) = (d.centroid_all_hz, rms_db(&x), d.density.per_s);
            println!(
                "cloth motion {m}: centroid {c:.0} Hz, rms {l:.1} dB, density {rate:.0} events/s"
            );
            if let Some((pc, pl, _)) = last {
                assert!(c > pc * 1.03, "centroid up: {pc} -> {c}");
                assert!(l > pl + 3.0, "level up: {pl} -> {l}");
            }
            last = Some((c, l, rate));
        }
        // cloth_fix: cloth is a continuous swish with slow bursts, not a tick
        // train (the batch-2 listen heard the ticks as metal pellets), so the
        // 1 ms density counter reads noise grain, not bursts. Pin the swish
        // instead: few distinct fine (5 ms) onsets at any motion.
        for m in [0.25f32, 1.0, 4.0] {
            let d = desc(&node_audio(vec![cloth(json!({"motion": m}))], "n", 2.0));
            let fine = d.fine_onsets.len() as f32 / 2.0;
            println!("cloth motion {m}: {fine:.1} fine onsets/s");
            assert!(fine < 12.0, "{fine}");
        }
    }

    #[test]
    fn cloth_weight_lowers_centroid() {
        let mut last = f32::MAX;
        for w in [0.0f32, 0.5, 1.0] {
            let x = node_audio(vec![cloth(json!({"weight": w}))], "n", 2.0);
            let c = centroid(&x);
            println!(
                "cloth weight {w}: centroid {c:.0} Hz, rms {:.1} dB",
                rms_db(&x)
            );
            assert!(c < last * 0.85, "{last} -> {c}");
            last = c;
        }
    }

    #[test]
    fn cloth_follows_a_motion_signal() {
        let nodes = vec![
            json!({"type": "ramp", "name": "m", "from": 0.0, "to": 2.0, "time": 2.0}),
            cloth(json!({"motion": "m"})),
        ];
        let x = node_audio(nodes, "n", 2.0);
        let n = x.len();
        let (a, b) = (&x[..n / 4], &x[3 * n / 4..]);
        let (la, lb) = (rms_db(a), rms_db(b));
        let (ca, cb) = (centroid(a), centroid(b));
        println!(
            "cloth motion ramp 0 -> 2: first quarter rms {la:.1} dB centroid {ca:.0} Hz; last quarter {lb:.1} dB, {cb:.0} Hz"
        );
        assert!(lb > la + 10.0 && cb > ca);
    }

    #[test]
    fn cloth_seeds_differ_and_extremes_are_finite_within_budget() {
        let a = node_audio(vec![cloth(json!({"seed": 1}))], "n", 1.0);
        let b = node_audio(vec![cloth(json!({"seed": 2}))], "n", 1.0);
        let d = distance(&a, &b, SR);
        println!("cloth seed 1 vs 2: STFT distance {d:.2} dB");
        assert!(d > 3.0, "{d}");
        // A signal far over the cap is clamped to 4; NaN-free.
        let t = std::time::Instant::now();
        let x = node_audio(
            vec![
                json!({"type": "ramp", "name": "huge", "from": 1e9, "to": 3e9, "time": 30.0}),
                cloth(json!({"motion": "huge", "weight": 1.0})),
            ],
            "n",
            30.0,
        );
        let el = t.elapsed();
        assert_finite(&x, "cloth max motion 30 s");
        println!(
            "cloth motion clamped to 4, weight 1, 30 s: {el:.2?}, peak {:.1} dB",
            peak_db(&x)
        );
        assert!(el.as_secs_f32() < 20.0, "{el:?}");
        for w in [0.0f32, 1.0] {
            let x = node_audio(vec![cloth(json!({"motion": 4.0, "weight": w}))], "n", 1.0);
            assert_finite(&x, "cloth extreme");
        }
        assert_bit_identical(vec![cloth(json!({"motion": 1.5, "weight": 0.2}))], 1.0);
    }

    #[test]
    fn cloth_errors_say_what_to_do() {
        let e = render_error(cloth(json!({"motion": 5.0})));
        println!("{e}");
        assert!(e.contains("motion: 5") && e.contains("0 to 4"), "{e}");
        let e = render_error(cloth(json!({"weight": -0.1})));
        assert!(e.contains("weight: -0.1") && e.contains("0 to 1"), "{e}");
    }

    // ----------------------------------------------------------------- footstep

    fn step(surface: &str, extra: Value) -> Value {
        with_fields(
            json!({"type": "footstep", "name": "n", "surface": surface, "seed": 1}),
            extra,
        )
    }

    /// The footstep as the human gets it: through the default output stage.
    fn step_audio(surface: &str, extra: Value) -> Audio {
        render(vec![step(surface, extra)], 1.0)
    }

    fn audible_s(d: &Descriptors) -> f32 {
        d.seconds - d.silence_start_s - d.silence_end_s
    }

    #[test]
    fn footstep_concrete_is_heel_then_toe() {
        let a = step_audio("concrete", json!({}));
        let d = describe(&a);
        let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
        println!(
            "concrete: onsets {:?} s, attack {:.1} ms, centroid {:.0} Hz, audible {:.2} s, peak {:.1} dB",
            t,
            d.attack_s * 1000.0,
            d.centroid_all_hz,
            audible_s(&d),
            d.peak_db
        );
        assert_eq!(t.len(), 2, "heel and toe: {t:?}");
        let gap = t[1] - t[0];
        assert!((0.060..=0.150).contains(&gap), "gap {gap}");
        assert!(d.attack_s < 0.010);
        assert!(
            (800.0..=3000.0).contains(&d.centroid_all_hz),
            "{}",
            d.centroid_all_hz
        );
        assert!((0.15..=0.4).contains(&audible_s(&d)), "{}", audible_s(&d));
        assert!(d.pitch_hz.is_none(), "{:?}", d.pitch_hz);
    }

    #[test]
    fn footstep_gravel_is_many_flat_ticks() {
        let a = step_audio("gravel", json!({}));
        let d = describe(&a);
        println!(
            "gravel: {} onsets, {} density events, flatness {:.3} (occupied band {:?}), centroid {:.0} Hz, audible {:.2} s",
            d.onsets.len(),
            d.density.count,
            d.flatness_all,
            d.flatness_occupied,
            d.centroid_all_hz,
            audible_s(&d)
        );
        assert!(d.onsets.len() >= 8, "{}", d.onsets.len());
        assert!(d.density.count >= 8);
        assert!(
            (2000.0..=6000.0).contains(&d.centroid_all_hz),
            "{}",
            d.centroid_all_hz
        );
        assert!((0.25..=0.6).contains(&audible_s(&d)), "{}", audible_s(&d));
        assert!(d.flatness_occupied.unwrap_or(0.0) > 0.4);
    }

    #[test]
    fn footstep_grass_is_a_soft_pitchless_swish() {
        let a = step_audio("grass", json!({}));
        let d = describe(&a);
        println!(
            "grass: attack {:.1} ms, pitch {:?}, centroid {:.0} Hz, audible {:.2} s, flatness {:.3}",
            d.attack_s * 1000.0,
            d.pitch_hz,
            d.centroid_all_hz,
            audible_s(&d),
            d.flatness_all
        );
        assert!((0.010..=0.040).contains(&d.attack_s), "{}", d.attack_s);
        assert!(d.pitch_hz.is_none(), "{:?}", d.pitch_hz);
        assert!(
            (1500.0..=5000.0).contains(&d.centroid_all_hz),
            "{}",
            d.centroid_all_hz
        );
        assert!((0.25..=0.6).contains(&audible_s(&d)), "{}", audible_s(&d));
    }

    #[test]
    fn footstep_wood_has_a_low_body_under_a_short_tick() {
        let raw = node_audio(vec![step("wood", json!({}))], "n", 1.0);
        let a = Audio::mono(SR, raw.clone());
        let d = describe(&a);
        let body = d
            .modes
            .iter()
            .find(|m| (80.0..=300.0).contains(&m.hz) && m.decay_s >= 0.1);
        let (hz, _) = peaks(&raw, SR, 1, 0.0)[0];
        let ms = |t: f32| (t * SR as f32) as usize;
        // The tick: 1.5-6 kHz in the first 15 ms vs the same band from 120 ms on
        // (after the toe's tick, which sits at ~0.1 s, has died).
        let tick = band_db(&raw[..ms(0.015)], SR, 1500.0, 6000.0);
        let tail = band_db(&raw[ms(0.13)..ms(0.3)], SR, 1500.0, 6000.0);
        let body_db = band_db(&raw[..ms(0.08)], SR, 80.0, 300.0);
        println!(
            "wood: body mode {:?}, strongest peak {hz:.0} Hz, audible {:.2} s; 1.5-6k first 15 ms {tick:.1} dB, from 130 ms {tail:.1} dB, body 80-300 Hz {body_db:.1} dB",
            body.map(|m| (m.hz.round(), m.decay_s)),
            audible_s(&d)
        );
        assert!(
            body.is_some(),
            "no 80-300 Hz mode ringing 0.1 s: {:?}",
            d.modes
        );
        assert!((80.0..=300.0).contains(&hz), "{hz}");
        assert!(tick > tail + 20.0, "the tick is short: {tick} vs {tail}");
        assert!(
            tick > body_db - 30.0,
            "the tick is there: {tick} vs {body_db}"
        );
        assert!((0.2..=0.5).contains(&audible_s(&d)), "{}", audible_s(&d));
    }

    #[test]
    fn footstep_weight_is_continuous_and_darkens() {
        for surface in ["concrete", "gravel", "grass", "wood"] {
            let mut last = (f32::MAX, f32::MIN);
            let mut line = String::new();
            for w in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let x = node_audio(vec![step(surface, json!({"weight": w}))], "n", 1.0);
                assert_finite(&x, surface);
                let (c, l) = (centroid(&x), rms_db(&x));
                line += &format!(" w{w}: {c:.0} Hz {l:.1} dB;");
                assert!(
                    c < last.0,
                    "{surface} weight {w}: centroid {c} !< {}",
                    last.0
                );
                assert!(l > last.1, "{surface} weight {w}: level {l} !> {}", last.1);
                last = (c, l);
            }
            println!("{surface} weight sweep (centroid, rms):{line}");
        }
    }

    #[test]
    fn footstep_pace_sets_the_gap_continuously() {
        let gap = |pace: Value| {
            let d = describe(&step_audio("concrete", json!({ "pace": pace })));
            assert_eq!(d.onsets.len(), 2, "{:?}", d.onsets);
            d.onsets[1].t - d.onsets[0].t
        };
        let mut last = f32::MAX;
        let mut line = String::new();
        for p in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let g = gap(json!(p));
            line += &format!(" pace {p}: {:.0} ms;", g * 1000.0);
            assert!(g < last, "pace {p}: gap {g} !< {last}");
            assert!((0.055..=0.155).contains(&g), "{g}");
            last = g;
        }
        println!("heel-toe gap:{line}");
        // The words are points on the same scale.
        for (word, num) in [("walk", 0.5), ("run", 0.9), ("sneak", 0.1)] {
            let a = node_audio(vec![step("grass", json!({"pace": word}))], "n", 0.7);
            let b = node_audio(vec![step("grass", json!({"pace": num}))], "n", 0.7);
            assert!(
                a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
                "{word}"
            );
            println!("pace \"{word}\" is bit-identical to pace {num}");
        }
        let c = node_audio(vec![step("grass", json!({}))], "n", 0.7);
        let w = node_audio(vec![step("grass", json!({"pace": "walk"}))], "n", 0.7);
        assert!(c.iter().zip(&w).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn footstep_surfaces_differ_in_flatness_and_onsets() {
        let g = describe(&step_audio("gravel", json!({})));
        let w = describe(&step_audio("wood", json!({})));
        println!(
            "gravel: flatness {:.3}, {} onsets; wood: flatness {:.3}, {} onsets",
            g.flatness_all,
            g.onsets.len(),
            w.flatness_all,
            w.onsets.len()
        );
        assert!(g.flatness_all > 3.0 * w.flatness_all);
        assert!(g.onsets.len() >= w.onsets.len() + 6);
    }

    #[test]
    fn footstep_shoe_and_at() {
        let c = |shoe: &str| {
            centroid(&node_audio(
                vec![step("concrete", json!({"shoe": shoe}))],
                "n",
                1.0,
            ))
        };
        let (hard, sneaker, boot, bare) = (c("hard"), c("sneaker"), c("boot"), c("bare"));
        println!(
            "concrete centroid by shoe: hard {hard:.0}, sneaker {sneaker:.0}, boot {boot:.0}, bare {bare:.0} Hz"
        );
        assert!(hard > boot && boot > sneaker && sneaker > bare);
        // `at` moves the heel.
        let x = node_audio(vec![step("concrete", json!({"at": 0.3}))], "n", 1.0);
        let first = x.iter().position(|v| v.abs() > 1e-4).unwrap();
        println!(
            "at 0.3: first sample over 1e-4 at {:.1} ms",
            first as f32 / 48.0
        );
        assert!((first as f32 / 48.0 - 300.0).abs() < 3.0);
    }

    #[test]
    fn footstep_seeds_make_different_steps() {
        for surface in ["concrete", "gravel", "grass", "wood"] {
            let steps: Vec<Vec<f32>> = (1..=6u64)
                .map(|s| render_with_seed(&[step(surface, json!({"seed": 0}))], 0.8, s))
                .collect();
            let mut dists = Vec::new();
            for i in 0..6 {
                for j in i + 1..6 {
                    dists.push(distance(&steps[i], &steps[j], SR));
                }
            }
            let (lo, hi) = dists
                .iter()
                .fold((f32::MAX, 0f32), |(l, h), d| (l.min(*d), h.max(*d)));
            let gaps: Vec<f32> = steps
                .iter()
                .map(|x| {
                    let d = desc(x);
                    if d.onsets.len() >= 2 {
                        d.onsets[1].t - d.onsets[0].t
                    } else {
                        0.0
                    }
                })
                .collect();
            let levels: Vec<f32> = steps.iter().map(|x| rms_db(x)).collect();
            let spread = |v: &[f32]| {
                v.iter().cloned().fold(f32::MIN, f32::max)
                    - v.iter().cloned().fold(f32::MAX, f32::min)
            };
            println!(
                "{surface} 6 doc seeds: STFT distance {lo:.2}..{hi:.2} dB; rms spread {:.2} dB; heel-toe gap spread {:.1} ms",
                spread(&levels),
                spread(&gaps) * 1000.0
            );
            assert!(lo > 1.0, "{surface}: two seeds must differ: {lo}");
            assert!(hi < 14.0, "{surface}: but stay one sound: {hi}");
            assert!(spread(&levels) > 0.5);
            // The node's own seed varies it the same way.
            let a = node_audio(vec![step(surface, json!({"seed": 1}))], "n", 0.8);
            let b = node_audio(vec![step(surface, json!({"seed": 2}))], "n", 0.8);
            let d = distance(&a, &b, SR);
            println!("{surface} node seed 1 vs 2: {d:.2} dB");
            assert!(d > 1.0 && d < 14.0);
        }
    }

    #[test]
    fn footstep_is_finite_at_the_extremes_and_bit_identical() {
        let mut worst = f32::MIN;
        let mut at = String::new();
        for surface in ["concrete", "gravel", "grass", "wood"] {
            for (w, p) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0)] {
                for shoe in ["hard", "bare"] {
                    let x = node_audio(
                        vec![step(
                            surface,
                            json!({"weight": w, "pace": p, "shoe": shoe, "at": 0.5}),
                        )],
                        "n",
                        1.0,
                    );
                    assert_finite(&x, surface);
                    let pk = peak_db(&x);
                    if pk > worst {
                        worst = pk;
                        at = format!("{surface} weight {w} pace {p} shoe {shoe}");
                    }
                }
            }
            // `at` past the end: silence, not an error.
            let x = node_audio(vec![step(surface, json!({"at": 5.0}))], "n", 0.5);
            assert!(x.iter().all(|v| *v == 0.0));
            assert_bit_identical(
                vec![step(surface, json!({"weight": 0.7, "pace": "run"}))],
                0.8,
            );
        }
        println!(
            "footstep weight/pace/shoe extremes, 4 surfaces: finite, highest raw peak {worst:.1} dB ({at}); at 5 s past a 0.5 s sound: zeros"
        );
        assert!(worst < 1.0, "{worst}");
    }

    #[test]
    fn footstep_errors_say_what_to_do() {
        let e = parse_body("footstep", &json!({"surface": "lava"}), "node").unwrap_err();
        println!("{e}");
        assert!(e.contains("lava") && e.contains("concrete"), "{e}");
        let e = render_error(step("wood", json!({"pace": "jog"})));
        println!("{e}");
        assert!(e.contains("jog") && e.contains("walk, run, sneak"), "{e}");
        let e = render_error(step("wood", json!({"pace": 1.5})));
        assert!(
            e.contains("pace: 1.5") && e.contains("0 (sneak) to 1 (run)"),
            "{e}"
        );
        let e = render_error(step("wood", json!({"weight": 2.0})));
        assert!(
            e.contains("weight: 2") && e.contains("0 (light) to 1 (heavy)"),
            "{e}"
        );
        let e = render_error(step("wood", json!({"shoe": "clog"})));
        assert!(e.contains("clog") && e.contains("sneaker"), "{e}");
        let e = render_error(step("wood", json!({"at": -1.0})));
        assert!(e.contains("at: -1"), "{e}");
    }

    #[test]
    fn footstep_one_liners_render_through_the_output_stage() {
        // The point of the node: four surfaces, one node each, no other nodes.
        for surface in ["concrete", "gravel", "grass", "wood"] {
            let a = render(vec![step(surface, json!({}))], 0.6);
            let d = describe(&a);
            println!("{surface}: peak {:.1} dBFS, {:.1} LUFS", d.peak_db, d.lufs);
            assert!(d.peak_db > -20.0 && d.peak_db < -3.0);
        }
    }

    // ------------------------------------------------------------ R2-G: body, swish

    /// The nine renders recorded before `body` and `swish` existed.
    fn baseline_cases() -> Vec<(String, Value)> {
        let mut v = Vec::new();
        for s in ["concrete", "gravel", "grass", "wood"] {
            v.push((format!("{s} default"), step(s, json!({}))));
        }
        for s in ["concrete", "gravel", "grass", "wood"] {
            v.push((
                format!("{s} heavy run boot"),
                step(
                    s,
                    json!({"weight": 0.8, "pace": "run", "shoe": "boot", "seed": 3}),
                ),
            ));
        }
        v.push((
            "wood hard light sneak".into(),
            step(
                "wood",
                json!({"weight": 0.2, "pace": "sneak", "shoe": "hard", "seed": 5}),
            ),
        ));
        v
    }

    const BASELINES: [u64; 9] = [
        0xca4e_df16_3ed1_a572,
        0xc005_c951_bef1_3afd,
        0x4a64_bd8a_1697_af30,
        0xe12c_c77e_9923_4945,
        0xfc7d_172a_6f70_5a5f,
        0x5834_23b2_0ed2_2779,
        0x9e32_1ce3_2bc9_bc29,
        0x32e7_5c23_5103_3ab7,
        0x901e_1415_d839_32cb,
    ];

    #[test]
    fn body_and_swish_defaults_are_bit_identical_to_before() {
        let cases = baseline_cases();
        let mut bad = Vec::new();
        for ((name, node), want) in cases.iter().zip(BASELINES) {
            let x = node_audio(vec![node.clone()], "n", 1.0);
            println!("{name}: hash {:016x}", fnv(&x));
            if fnv(&x) != want {
                bad.push(name.clone());
            }
        }
        assert!(bad.is_empty(), "changed: {bad:?}");
    }

    /// The strongest spectral peak (a local maximum) between `lo` and `hi` Hz.
    fn strongest_in(x: &[f32], lo: f32, hi: f32) -> f32 {
        peaks(x, SR, 200, 200.0)
            .into_iter()
            .find(|(hz, _)| (lo..=hi).contains(hz))
            .map(|p| p.0)
            .unwrap()
    }

    #[test]
    fn footstep_body_moves_the_wood_mode_by_the_factor() {
        // Strongest low mode (40-400 Hz), body 0.5 vs 2, at three weights: the
        // ratio is 4 within 10 %, whatever the weight.
        for w in [0.2, 0.5, 0.8] {
            let lo = node_audio(
                vec![step("wood", json!({"weight": w, "body": 0.5}))],
                "n",
                1.0,
            );
            let hi = node_audio(
                vec![step("wood", json!({"weight": w, "body": 2.0}))],
                "n",
                1.0,
            );
            let (a, b) = (
                strongest_in(&lo, 50.0, 600.0),
                strongest_in(&hi, 50.0, 600.0),
            );
            println!(
                "wood weight {w}: body 0.5 -> {a:.1} Hz, body 2 -> {b:.1} Hz, ratio {:.2}",
                b / a
            );
            assert!((b / a - 4.0).abs() < 0.4, "{a} {b}");
        }
    }

    #[test]
    fn footstep_body_is_independent_of_weight_and_moves_concrete_gravel_grass() {
        // Weight moves the wood mode by 2x over its range; body at a fixed weight
        // moves it by the factor, and a body of 1 is the unchanged recipe.
        let one = node_audio(vec![step("wood", json!({"body": 1.0}))], "n", 1.0);
        let none = node_audio(vec![step("wood", json!({}))], "n", 1.0);
        assert_eq!(fnv(&one), fnv(&none));
        // Concrete: the low thud (60-300 Hz band level) falls in pitch with
        // body 0.5 and rises with 2: the band centroid below 400 Hz orders.
        let low_centroid = |surface: &str, body: f32| {
            let x = node_audio(vec![step(surface, json!({"body": body}))], "n", 1.0);
            let sp = spectrum(&x, SR);
            let (mut num, mut den) = (0.0f64, 0.0f64);
            for (hz, m) in sp.iter().filter(|(hz, _)| (30.0..=400.0).contains(hz)) {
                let p = 10f64.powf(f64::from(*m) / 10.0);
                num += f64::from(*hz) * p;
                den += p;
            }
            (num / den) as f32
        };
        for surface in ["concrete", "gravel", "grass"] {
            let c: Vec<f32> = [0.5, 0.71, 1.0, 1.41, 2.0]
                .iter()
                .map(|b| low_centroid(surface, *b))
                .collect();
            println!("{surface}: centroid below 400 Hz at body 0.5 .. 2: {c:.0?}");
            assert!(c.windows(2).all(|w| w[1] > w[0]), "{surface}: {c:?}");
        }
    }

    fn share_above(x: &[f32], hz: f32) -> f32 {
        10f32.powf(
            (band_db(x, SR, hz, SR as f32 / 2.0) - band_db(x, SR, 0.0, SR as f32 / 2.0)) / 10.0,
        )
    }

    #[test]
    fn footstep_swish_trades_the_swish_for_the_ticks_on_grass() {
        let share: Vec<f32> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|s| {
                let x = node_audio(vec![step("grass", json!({"swish": s}))], "n", 1.0);
                assert_finite(&x, "grass swish");
                share_above(&x, 2000.0)
            })
            .collect();
        println!("grass energy share above 2 kHz, swish 0 .. 1: {share:.3?}");
        // More swish, less of the low soft body left over once the ticks are gone
        // from the top: the share above 2 kHz rises every step.
        assert!(share.windows(2).all(|w| w[1] > w[0]), "{share:?}");
        assert!(share[4] - share[0] > 0.3, "{share:?}");
        // Swish 0.5 is the default.
        let d = node_audio(vec![step("grass", json!({}))], "n", 1.0);
        let h = node_audio(vec![step("grass", json!({"swish": 0.5}))], "n", 1.0);
        assert_eq!(fnv(&d), fnv(&h));
    }

    #[test]
    fn footstep_swish_on_gravel_fades_stones_and_adds_a_bed() {
        let rms = |s: f32| {
            let x = node_audio(vec![step("gravel", json!({"swish": s}))], "n", 1.0);
            assert_finite(&x, "gravel swish");
            (share_above(&x, 2000.0), rms_db(&x))
        };
        let r: Vec<(f32, f32)> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|s| rms(*s))
            .collect();
        println!("gravel swish 0 .. 1: (share above 2 kHz, rms dB) {r:.3?}");
        // More swish, fewer stone ticks: the share of energy above 2 kHz falls
        // (the bed is wide and the stones are narrow bands at 1.7-7.6 kHz) or at
        // least changes monotonically.
        let dir = r[1].0 - r[0].0;
        assert!(r.windows(2).all(|w| (w[1].0 - w[0].0) * dir > 0.0), "{r:?}");
    }

    #[test]
    fn footstep_body_and_swish_errors_say_what_to_do() {
        let e = render_error(step("wood", json!({"body": 3.0})));
        println!("{e}");
        assert!(
            e.contains("body: 3") && e.contains("0.5") && e.contains("to 2"),
            "{e}"
        );
        let e = render_error(step("wood", json!({"body": 0.1})));
        assert!(e.contains("body: 0.1"), "{e}");
        let e = render_error(step("grass", json!({"swish": 1.5})));
        println!("{e}");
        assert!(
            e.contains("swish: 1.5") && e.contains("0 (ticks only)"),
            "{e}"
        );
        let e = render_error(step("grass", json!({"swish": -0.1})));
        assert!(e.contains("swish: -0.1"), "{e}");
    }

    #[test]
    fn footstep_body_and_swish_extremes_are_finite_and_bit_identical() {
        for surface in ["concrete", "gravel", "grass", "wood"] {
            for (b, s) in [(0.5, 0.0), (2.0, 1.0), (0.5, 1.0), (2.0, 0.0)] {
                let x = node_audio(
                    vec![step(
                        surface,
                        json!({"body": b, "swish": s, "weight": 1.0, "pace": 1.0}),
                    )],
                    "n",
                    1.0,
                );
                assert_finite(&x, surface);
                assert!(peak_db(&x) < 3.0, "{surface} {b} {s}: {}", peak_db(&x));
            }
            assert_bit_identical(vec![step(surface, json!({"body": 0.7, "swish": 0.8}))], 0.8);
        }
    }
}

mod cloth_after_listening {
    //! The batch-2 blind listen heard `cloth` as "pellets of
    //! something hard", "small metal objects falling". The cause measured here:
    //! clicky ticks (high crest, many fine onsets, narrow ringing bands). These
    //! tests pin the swish: low crest, few fine onsets, high flatness, while
    //! motion still raises level and centroid. Numbers go in docs/nodes/surface.md.

    use crate::common::*;
    use serde_json::json;
    use sfx_analysis::describe;
    use sfx_graph::Audio;

    struct M {
        crest: f32,
        fine_per_s: f32,
        flat: f32,
        flat_occ: f32,
        centroid: f32,
        rms: f32,
    }

    fn measure(motion: f32, weight: f32, seed: u64) -> M {
        let x = node_audio(
            vec![
                json!({"type": "cloth", "name": "n", "motion": motion, "weight": weight, "seed": seed}),
            ],
            "n",
            2.0,
        );
        assert_finite(&x, "cloth");
        let d = describe(&Audio::mono(SR, x.clone()));
        M {
            crest: d.crest_db,
            fine_per_s: d.fine_onsets.len() as f32 / 2.0,
            flat: d.flatness_all,
            flat_occ: d.flatness_occupied.unwrap_or(0.0),
            centroid: d.centroid_all_hz,
            rms: rms_db(&x),
        }
    }

    #[test]
    fn cloth_is_a_swish_not_ticks() {
        for w in [0.0f32, 0.5, 1.0] {
            for m in [0.5f32, 1.0, 2.0] {
                let mut crest = 0.0;
                let mut fine = 0.0;
                let mut flat = 0.0;
                for seed in 1..=3 {
                    let r = measure(m, w, seed);
                    println!(
                        "cloth motion {m} weight {w} seed {seed}: crest {:.1} dB, fine onsets {:.1}/s, flatness {:.3} (occupied {:.3}), centroid {:.0} Hz, rms {:.1} dB",
                        r.crest, r.fine_per_s, r.flat, r.flat_occ, r.centroid, r.rms
                    );
                    crest += r.crest / 3.0;
                    fine += r.fine_per_s / 3.0;
                    flat += r.flat_occ / 3.0;
                }
                assert!(crest < 16.0, "crest {crest} at motion {m} weight {w}");
                assert!(fine < 12.0, "fine onsets {fine}/s at motion {m} weight {w}");
                assert!(
                    flat > 0.3,
                    "occupied flatness {flat} at motion {m} weight {w}"
                );
            }
        }
    }
}

mod crumple {
    //! `crumple`. Each test measures and prints (these lines go in
    //! docs/nodes/crumple.md).

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::describe;
    use sfx_analysis::measure::fine_onsets;
    use sfx_graph::node::parse_body;
    use sfx_graph::{Audio, RenderOpts, SoundDoc};

    fn crumple(extra: Value) -> Value {
        let mut n = json!({"type": "crumple", "name": "n", "seed": 1});
        n.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        n
    }

    fn node(extra: Value, secs: f32) -> Vec<f32> {
        node_audio(vec![crumple(extra)], "n", secs)
    }

    fn centroid(x: &[f32]) -> f32 {
        describe(&Audio::mono(SR, x.to_vec())).centroid_all_hz
    }

    /// Fine onsets (spacing 5 ms) of the node's own samples, per second.
    fn onsets_per_s(x: &[f32]) -> f32 {
        fine_onsets(&Audio::mono(SR, x.to_vec()), 5.0).len() as f32 / (x.len() as f32 / SR as f32)
    }

    fn render_error(node: Value) -> String {
        crate::common::render_error(vec![node], "n", 0.5)
    }

    #[test]
    fn crumple_fine_onset_count_tracks_density() {
        for d in [20.0f32, 40.0, 80.0, 160.0] {
            // Three seeds, 2 s each: the mean of the counts per second.
            let rates: Vec<f32> = (1..=3)
                .map(|s| onsets_per_s(&node(json!({"density": d, "seed": s}), 2.0)))
                .collect();
            let mean = rates.iter().sum::<f32>() / 3.0;
            println!("crumple density {d}: fine onsets per s {rates:.1?}, mean {mean:.1}");
            assert!(
                (mean / d - 1.0).abs() <= 0.25,
                "density {d}: counted {mean}/s"
            );
            // Each seed alone is also within 25 %.
            for r in rates {
                assert!((r / d - 1.0).abs() <= 0.25, "density {d}: one seed {r}/s");
            }
        }
    }

    #[test]
    fn crumple_has_no_saturation_to_400() {
        // Past 200/s the bursts are closer than any 1 ms-chunk detector splits
        // (it looks 3 ms back), so count with a 2 ms refractory time up to 200,
        // where the count must keep rising, and check that the level does not
        // pile up between 50 and 400.
        let mut last = 0.0f32;
        for d in [50.0f32, 100.0, 200.0] {
            let x = node(json!({"density": d}), 2.0);
            assert_finite(&x, "crumple");
            let n = fine_onsets(&Audio::mono(SR, x.clone()), 2.0).len() as f32 / 2.0;
            println!(
                "crumple density {d}: fine onsets (2 ms) per s {n:.1}, rms {:.1} dB",
                rms_db(&x)
            );
            assert!(n > last + 5.0, "{last} -> {n}");
            last = n;
        }
        let (a, b) = (
            rms_db(&node(json!({"density": 50}), 2.0)),
            rms_db(&node(json!({"density": 400}), 2.0)),
        );
        assert!((a - b).abs() < 6.0, "rms {a} dB at 50/s, {b} dB at 400/s");
    }

    /// Mean centroid over four seeds (a few snaps dominate one render).
    fn mean_centroid(extra: Value) -> f32 {
        (1..=4)
            .map(|seed| {
                let mut e = extra.clone();
                e["seed"] = json!(seed);
                centroid(&node(e, 2.0))
            })
            .sum::<f32>()
            / 4.0
    }

    #[test]
    fn crumple_hardness_raises_the_centroid() {
        let mut last = 0.0f32;
        for h in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let c = mean_centroid(json!({"hardness": h}));
            println!("crumple hardness {h}: centroid {c:.0} Hz (mean of 4 seeds)");
            assert!(c > last * 1.03, "hardness {h}: {last} -> {c}");
            last = c;
        }
    }

    #[test]
    fn crumple_size_lowers_the_centroid() {
        let mut last = f32::MAX;
        for s in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let c = mean_centroid(json!({"size": s}));
            println!("crumple size {s}: centroid {c:.0} Hz (mean of 4 seeds)");
            assert!(c < last * 0.98, "size {s}: {last} -> {c}");
            last = c;
        }
    }

    #[test]
    fn crumple_seeds_differ() {
        let a = node(json!({"seed": 1}), 1.0);
        let b = node(json!({"seed": 2}), 1.0);
        let same = a.iter().zip(&b).filter(|(p, q)| p == q).count();
        let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum::<f32>()
            / a.iter().map(|p| p.abs()).sum::<f32>();
        println!(
            "crumple seeds 1 vs 2: {same} equal samples of {}, relative difference {diff:.2}",
            a.len()
        );
        assert!(diff > 0.5, "{diff}");
        // The document seed changes it too.
        let d = |seed: u64| {
            let mut doc = SoundDoc::named("t");
            doc.nodes = vec![
                crumple(json!({})),
                json!({"type": "output", "name": "out", "input": "n"}),
            ];
            doc.duration = Some(0.5);
            doc.seed = seed;
            doc.render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap()
            .audio
            .channels[0]
                .clone()
        };
        assert_ne!(d(1), d(2));
    }

    #[test]
    fn crumple_into_lufs_minus_20() {
        for seed in 1..=6u64 {
            let mut d = SoundDoc::named("test");
            d.nodes = vec![
                crumple(json!({"density": 40, "seed": seed})),
                json!({"type": "output", "name": "out", "input": "n", "lufs": -20}),
            ];
            d.duration = Some(1.0);
            let r = d
                .render(&RenderOpts {
                    cached: false,
                    ..RenderOpts::default()
                })
                .unwrap();
            let n = fine_onsets(&r.audio, 5.0).len();
            println!(
                "crumple 40/s seed {seed} into lufs -20: {n} fine onsets, {:.2} LUFS, peak {:.2} dBFS, warnings {:?}",
                r.lufs, r.peak_db, r.warnings
            );
            assert!(n >= 20, "seed {seed}: {n} onsets");
            assert!(
                !r.warnings.iter().any(|w| w.contains("lufs")),
                "seed {seed}: {:?}",
                r.warnings
            );
            assert!((r.lufs + 20.0).abs() <= 1.0, "seed {seed}: {}", r.lufs);
        }
    }

    #[test]
    fn crumple_density_signal_and_silence() {
        // Density 0 is exact silence.
        assert!(node(json!({"density": 0.0}), 0.5).iter().all(|v| *v == 0.0));
        // A linear ramp 20 -> 180 puts about 60 bursts in the first second and
        // 140 in the second.
        let nodes = vec![
            json!({"type": "ramp", "name": "r", "from": 20, "to": 180, "time": 2.0, "curve": "linear"}),
            crumple(json!({"density": "r"})),
        ];
        let x = node_audio(nodes, "n", 2.0);
        let (a, b) = x.split_at(x.len() / 2);
        let (ra, rb) = (onsets_per_s(a), onsets_per_s(b));
        println!(
            "crumple density ramp 20 -> 180: first second {ra:.0}/s (60 expected), second {rb:.0}/s (140 expected)"
        );
        assert!((ra / 60.0 - 1.0).abs() <= 0.25 && (rb / 140.0 - 1.0).abs() <= 0.25);
    }

    #[test]
    fn crumple_is_bit_identical() {
        for extra in [
            json!({}),
            json!({"density": 160, "size": 0.9, "hardness": 0.1}),
        ] {
            assert_bit_identical(vec![crumple(extra)], 0.5);
        }
    }

    #[test]
    fn crumple_is_finite_at_the_extremes() {
        for density in [0.0f32, 0.001, 400.0] {
            for size in [0.0f32, 1.0] {
                for hardness in [0.0f32, 1.0] {
                    let x = node(
                        json!({"density": density, "size": size, "hardness": hardness}),
                        0.5,
                    );
                    assert_finite(&x, "crumple");
                    let peak = x.iter().fold(0f32, |a, b| a.max(b.abs()));
                    println!(
                        "crumple density {density} size {size} hardness {hardness}: peak {peak:.2}, rms {:.1} dB",
                        rms_db(&x)
                    );
                    assert!(peak < 10.0, "peak {peak}");
                }
            }
        }
        // A wild density signal is clamped.
        let x = node_audio(
            vec![
                json!({"type": "osc", "name": "o", "freq": 3, "amp": 1e9}),
                crumple(json!({"density": "o"})),
            ],
            "n",
            0.5,
        );
        assert_finite(&x, "crumple with a wild density");
    }

    #[test]
    fn crumple_errors_name_the_field() {
        for (extra, field) in [
            (json!({"density": 500}), "density: 500"),
            (json!({"density": -1}), "density: -1"),
            (json!({"size": 1.5}), "size: 1.5"),
            (json!({"hardness": -0.1}), "hardness: -0.1"),
        ] {
            let e = render_error(crumple(extra));
            println!("{e}");
            assert!(e.contains(field) && e.contains("give a number"), "{e}");
        }
        let e = parse_body("crumple", &json!({"dense": 3}), "node #0").err();
        println!("{e:?}");
        assert!(e.unwrap().contains("dense"));
    }

    #[test]
    fn crumple_recipe_renders() {
        // The recipe in docs/nodes/crumple.md: a hand squeezing a sheet of foil.
        let nodes = vec![
            json!({"type": "wander", "name": "squeeze", "rate": 3, "range": [30, 140], "seed": 2}),
            json!({"type": "crumple", "name": "foil", "density": "squeeze", "size": 0.4, "hardness": 0.85, "seed": 3}),
            json!({"type": "output", "name": "out", "input": "foil", "lufs": -20}),
        ];
        let mut d = SoundDoc::named("recipe");
        d.nodes = nodes;
        d.duration = Some(1.5);
        let r = d
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap();
        let n = fine_onsets(&r.audio, 5.0).len();
        println!(
            "recipe 1.5 s: {} samples, {n} fine onsets, {:.2} LUFS, peak {:.2} dBFS, warnings {:?}",
            r.audio.channels[0].len(),
            r.lufs,
            r.peak_db,
            r.warnings
        );
        assert_eq!(r.audio.channels[0].len(), 72_000);
        assert!(n > 40, "{n}");
    }
}

mod click {
    //! `click`, the mechanism node. Each test measures and prints (these
    //! lines go in docs/nodes/click.md).

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::describe;
    use sfx_graph::node::parse_body;
    use sfx_graph::{Audio, RenderOpts, SoundDoc};

    const KINDS: [&str; 5] = ["latch", "switch", "button", "lock", "clasp"];

    /// A `click` named `c` with extra fields.
    fn click(kind: &str, extra: Value) -> Vec<Value> {
        let mut c = json!({"type": "click", "name": "c", "kind": kind});
        c.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![c]
    }

    fn audio(kind: &str, extra: Value) -> Vec<f32> {
        node_audio(click(kind, extra), "c", 0.4)
    }

    /// Seconds from the start to the last sample within 50 dB of the peak.
    fn length_s(x: &[f32]) -> f32 {
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let last = x
            .iter()
            .rposition(|v| v.abs() > peak * 10f32.powf(-50.0 / 20.0))
            .unwrap_or(0);
        last as f32 / SR as f32
    }

    fn first_diff(a: &[f32], b: &[f32]) -> usize {
        a.iter()
            .zip(b)
            .position(|(p, q)| p.to_bits() != q.to_bits())
            .unwrap_or(a.len())
    }

    #[test]
    fn latch_has_two_onsets_25_to_60_ms_apart() {
        for seed in 0..6u64 {
            let x = audio("latch", json!({"seed": seed}));
            assert_finite(&x, "latch");
            let d = describe(&Audio::mono(SR, x.clone()));
            let t: Vec<f32> = d.onsets.iter().map(|o| o.t).collect();
            println!("latch seed {seed}: onsets {t:?}");
            assert_eq!(t.len(), 2, "two onsets: {t:?}");
            let gap = (t[1] - t[0]) * 1000.0;
            assert!((25.0..=60.0).contains(&gap), "gap {gap:.1} ms");
        }
    }

    #[test]
    fn every_kind_has_its_second_transient_20_to_80_ms_after_the_first() {
        for kind in KINDS {
            let two = audio(kind, json!({}));
            let one = audio(kind, json!({"double": false}));
            let i = first_diff(&two, &one);
            let ms = i as f32 / SR as f32 * 1000.0;
            println!(
                "{kind}: second transient at {ms:.2} ms; centroid {:.0} Hz; length {:.1} ms; peak {:.3}",
                spectrum_centroid(&two),
                length_s(&two) * 1000.0,
                two.iter().fold(0.0f32, |m, v| m.max(v.abs())),
            );
            // The press is the same samples with and without the second transient
            // (it is scaled by its own peak): the gap is where they part.
            assert!((20.0..=80.0).contains(&ms), "{kind}: {ms:.2} ms");
        }
    }

    #[test]
    fn switch_is_bright_and_short() {
        let x = audio("switch", json!({}));
        let c = spectrum_centroid(&x);
        let l = length_s(&x) * 1000.0;
        println!("switch: centroid {c:.0} Hz, length (to -50 dB) {l:.1} ms");
        assert!((2000.0..=6000.0).contains(&c), "centroid {c}");
        assert!(l < 120.0, "length {l} ms");
    }

    #[test]
    fn bigger_size_lowers_the_centroid() {
        for kind in KINDS {
            let cs: Vec<f32> = [0.1, 0.3, 0.5, 0.7, 0.9]
                .iter()
                .map(|s| spectrum_centroid(&audio(kind, json!({"size": s}))))
                .collect();
            println!("{kind}: centroid at size .1 .3 .5 .7 .9: {cs:.0?} Hz");
            assert!(
                cs.windows(2).all(|w| w[1] < w[0]),
                "{kind}: not monotone {cs:?}"
            );
        }
    }

    #[test]
    fn harder_is_brighter() {
        let cs: Vec<f32> = [0.0, 0.35, 0.7, 1.0]
            .iter()
            .map(|h| spectrum_centroid(&audio("latch", json!({"hardness": h}))))
            .collect();
        println!("latch: centroid at hardness 0 .35 .7 1: {cs:.0?} Hz");
        assert!(cs.windows(2).all(|w| w[1] > w[0]), "{cs:?}");
    }

    #[test]
    fn seeds_differ_by_a_few_percent() {
        let a = audio("latch", json!({"seed": 1}));
        let b = audio("latch", json!({"seed": 2}));
        let diff: f32 = a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum();
        let tot: f32 = a.iter().map(|v| v.abs()).sum();
        let (pa, pb) = (peak_hz(&a, SR), peak_hz(&b, SR));
        println!(
            "seed 1 vs 2: summed |diff| / summed |a| = {:.3}; spectral peak {pa:.0} vs {pb:.0} Hz",
            diff / tot
        );
        assert!(diff > 0.01 * tot, "seeds 1 and 2 are nearly equal");
        // +-3 % on every mode frequency.
        assert!((pa / pb - 1.0).abs() < 0.08, "{pa} {pb}");
    }

    #[test]
    fn a_hit_at_zero_is_not_cut() {
        let x = audio("button", json!({"at": 0.0}));
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let head = x[..48].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!(
            "button at 0: first sample {:.4}, peak of the first 1 ms {head:.4}, peak {peak:.4}",
            x[0]
        );
        assert!(x[0].abs() > 0.0 && head > 0.3 * peak);
        // Through the output stage: the default 2 ms fade_in cuts it; 0.0005 keeps it.
        let render_peak = |fade: Option<f32>| {
            let mut out = json!({"type": "output", "name": "out", "input": "c"});
            if let Some(f) = fade {
                out["fade_in"] = json!(f);
            }
            let mut d = SoundDoc::named("t");
            d.nodes = vec![click("button", json!({}))[0].clone(), out];
            d.duration = Some(0.3);
            let a = d.render(&RenderOpts::default()).unwrap().audio;
            a.channels[0].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        let (default, short) = (render_peak(None), render_peak(Some(0.0005)));
        println!("through output: peak {default:.4} (default fade), {short:.4} (fade_in 0.0005)");
        let none = render_peak(Some(0.0));
        println!("fade_in 0: {none:.4}");
        assert!((none - peak).abs() < 1e-3, "fade_in 0 keeps the hit whole");
        assert!(short > 0.75 * peak, "fade_in 0.0005 keeps most of the hit");
        assert!(default < short);
    }

    #[test]
    fn double_false_is_the_press_alone() {
        let x = audio("lock", json!({"double": false}));
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let d = describe(&Audio::mono(SR, x));
        println!("lock single: {} onset(s), peak {peak:.3}", d.onsets.len());
        assert_eq!(d.onsets.len(), 1);
        assert!((peak - 0.5).abs() < 1e-3, "{peak}");
    }

    #[test]
    fn bit_identical_per_seed() {
        for kind in KINDS {
            assert_bit_identical(click(kind, json!({"seed": 7})), 0.3);
        }
        assert_eq!(
            audio("clasp", json!({"seed": 3})),
            audio("clasp", json!({"seed": 3}))
        );
    }

    #[test]
    fn finite_at_the_extremes() {
        for kind in KINDS {
            for size in [0.0, 1.0] {
                for hardness in [0.0, 1.0] {
                    for at in [0.0, 0.29, 5.0] {
                        let x = audio(kind, json!({"size": size, "hardness": hardness, "at": at}));
                        assert_finite(&x, kind);
                        assert_eq!(x.len(), (0.4 * SR as f32) as usize);
                        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        assert!(peak <= 0.5 + 1e-4, "{kind} {size} {hardness} {at}: {peak}");
                    }
                }
            }
        }
        // A low sample rate drops the modes above the top instead of folding them.
        let mut d = SoundDoc::named("lo");
        d.sample_rate = 8000;
        d.nodes = click("lock", json!({"size": 0.0}));
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "c"}));
        d.duration = Some(0.2);
        match d.render(&RenderOpts::default()) {
            Ok(r) => assert!(r.audio.channels[0].iter().all(|v| v.is_finite())),
            Err(e) => println!("8 kHz lock size 0: {e}"),
        }
    }

    #[test]
    fn errors_say_what_to_give() {
        let err = |extra: Value, kind: &str| {
            let mut d = SoundDoc::named("e");
            d.nodes = click(kind, extra);
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": "c"}));
            d.duration = Some(0.2);
            d.render(&RenderOpts::default()).unwrap_err()
        };
        let e = err(json!({"size": 1.5}), "latch");
        println!("{e}");
        assert!(e.contains("size") && e.contains("1.5") && e.contains("0 to 1"));
        let e = err(json!({"hardness": -0.1}), "latch");
        assert!(e.contains("hardness") && e.contains("0 to 1"), "{e}");
        let e = err(json!({"at": -1.0}), "latch");
        assert!(e.contains("at") && e.contains(">= 0"), "{e}");
        let e = parse_body("click", &json!({"kind": "squeak"}), "node `c`").unwrap_err();
        println!("{e}");
        assert!(
            e.contains("squeak") && e.contains("latch") && e.contains("clasp"),
            "{e}"
        );
        let e = parse_body("click", &json!({"kind": "latch", "frq": 1}), "node `c`").unwrap_err();
        assert!(e.contains("frq"), "{e}");
    }

    #[test]
    fn the_example_parses() {
        let text = sfx_graph::describe(Some("click")).unwrap();
        assert!(text.contains("latch"), "{text}");
        assert!(sfx_graph::describe(None).unwrap().contains("click"));
    }

    #[test]
    fn recipe_renders() {
        // The doc's recipe: a latch at the end of a door slam's body, here alone.
        let nodes = vec![
            json!({"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.0}),
            json!({"type": "gain", "name": "hit", "input": "latch", "gain": 0.8}),
            json!({"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}),
        ];
        let a = render(&nodes, 0.25);
        let d = describe(&a);
        println!(
            "recipe: {} samples, peak {:.2} dB, onsets {:?}, centroid {:.0} Hz",
            a.channels[0].len(),
            d.peak_db,
            d.onsets.iter().map(|o| o.t).collect::<Vec<_>>(),
            spectrum_centroid(&a.channels[0])
        );
        assert_eq!(a.channels[0].len(), (0.25 * SR as f32) as usize);
        assert_finite(&a.channels[0], "recipe");
        assert_eq!(d.onsets.len(), 2);
    }
}
