mod measures {
    //! The `density`, `attack` and `spacing` macro measures, `onsets`
    //! with `spacing_ms`, and the minimum effect of a macro sweep.

    use serde_json::{Value, json};
    use sfx_analysis::measure::measure;
    use sfx_analysis::{CheckCx, CheckResult, Expect, describe, run};
    use sfx_graph::{
        Audio, Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc,
    };

    const SR: u32 = 48_000;

    /// Deterministic white noise, -1..1.
    struct Lcg(u32);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
        }
    }

    /// A noise burst `ms` long at each time in `times` (seconds), with the
    /// matching amplitude, in `secs` of silence.
    fn clicks(times: &[(f32, f32)], ms: f32, secs: f32) -> Audio {
        let mut x = vec![0.0f32; (secs * SR as f32) as usize];
        let mut rng = Lcg(7);
        let len = (ms * 0.001 * SR as f32) as usize;
        for (t, amp) in times {
            let at = (t * SR as f32) as usize;
            for i in 0..len {
                if let Some(v) = x.get_mut(at + i) {
                    // A fast decay inside the burst.
                    *v += amp * rng.next() * (1.0 - i as f32 / len as f32);
                }
            }
        }
        Audio::mono(SR, x)
    }

    fn train(rate: f32, secs: f32) -> Audio {
        let n = (rate * secs) as usize;
        let t: Vec<(f32, f32)> = (0..n)
            .map(|i| (0.01 + i as f32 / rate, 0.5))
            .filter(|(t, _)| *t < secs - 0.01)
            .collect();
        clicks(&t, 1.5, secs)
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

    /// An impulse train at `rate` per second through a short resonator.
    fn click_doc(rate: f32, secs: f32) -> SoundDoc {
        let mut d = SoundDoc::named("t");
        let n = (rate * secs) as usize - 1;
        let at: Vec<f32> = (0..n).map(|i| 0.01 + i as f32 / rate).collect();
        d.nodes = vec![
            json!({"type": "impulse", "name": "i", "at": at}),
            json!({"type": "resonators", "name": "r", "input": "i", "modes": [[3000, 0.0008, 0.8]]}),
            json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
        ];
        d.duration = Some(secs);
        d
    }

    // --- density ---------------------------------------------------------------

    #[test]
    fn density_rises_with_the_rate_of_a_click_train_and_matches_the_descriptors() {
        let rates = [5.0f32, 10.0, 20.0, 40.0, 80.0, 160.0];
        let v: Vec<f32> = rates
            .iter()
            .map(|r| measure(&train(*r, 1.5), Measure::Density))
            .collect();
        println!("density of trains at {rates:?}/s: {v:?}");
        assert!(v.windows(2).all(|w| w[1] > w[0] * 1.5), "{v:?}");
        for (r, m) in rates.iter().zip(&v) {
            // The span runs from the first click to the end of the last, so the
            // count over the span reads a little over the rate.
            assert!((m / r - 1.0).abs() < 0.2, "rate {r}: measured {m}");
        }
        // The same number the descriptor table prints.
        for r in [10.0f32, 80.0] {
            let a = train(r, 1.5);
            let d = describe(&a);
            let m = measure(&a, Measure::Density);
            println!("rate {r}: measure {m}, descriptors {}", d.density.per_s);
            assert!((m - d.density.per_s).abs() < 0.05 * d.density.per_s, "{m}");
        }
        assert_eq!(
            measure(&Audio::mono(SR, vec![0.0; 4800]), Measure::Density),
            0.0
        );
    }

    // --- attack ----------------------------------------------------------------

    #[test]
    fn attack_rises_as_a_swell_lengthens_its_rise() {
        let rises = [0.005f32, 0.01, 0.02, 0.04, 0.08];
        let v: Vec<f32> = rises
            .iter()
            .map(|rise| {
                let n = (0.4 * SR as f32) as usize;
                let x: Vec<f32> = (0..n)
                    .map(|i| {
                        let t = i as f32 / SR as f32;
                        let env = if t < *rise {
                            t / rise
                        } else {
                            (-(t - rise) * 12.0).exp()
                        };
                        env * (std::f32::consts::TAU * 3000.0 * t).sin() * 0.5
                    })
                    .collect();
                measure(&Audio::mono(SR, x), Measure::Attack)
            })
            .collect();
        println!("attack (ms) of rises {rises:?} s: {v:?}");
        assert!(v.windows(2).all(|w| w[1] > w[0] * 1.5), "{v:?}");
        for (rise, a) in rises.iter().zip(&v) {
            let want = rise * 1000.0 * 0.9;
            assert!((a - want).abs() < 1.5, "rise {rise}: {a} ms, want {want}");
        }
        assert_eq!(
            measure(&Audio::mono(SR, vec![0.0; 4800]), Measure::Attack),
            0.0
        );
    }

    // --- spacing ---------------------------------------------------------------

    /// Hits at `first`, `first*r`, ... apart, each `r` times the last.
    fn bounce(first: f32, r: f32, hits: usize) -> Audio {
        let (mut t, mut gap, mut amp) = (0.02f32, first, 0.8f32);
        let mut ev = Vec::new();
        for _ in 0..hits {
            ev.push((t, amp));
            t += gap;
            gap *= r;
            amp *= r;
        }
        clicks(&ev, 3.0, t + 0.2)
    }

    #[test]
    fn spacing_rises_as_the_first_gap_of_a_bounce_grows() {
        let firsts = [0.1f32, 0.15, 0.2, 0.3, 0.4];
        let v: Vec<f32> = firsts
            .iter()
            .map(|f| measure(&bounce(*f, 0.7, 5), Measure::Spacing))
            .collect();
        println!("spacing (s) of bounces with first gap {firsts:?}: {v:?}");
        assert!(v.windows(2).all(|w| w[1] > w[0] * 1.2), "{v:?}");
        // Gaps f, .7f, .49f, .34f: the median (upper middle of four) is .7 f.
        for (f, s) in firsts.iter().zip(&v) {
            assert!((s - f * 0.7).abs() < 0.012, "first {f}: {s}");
        }
        // A lone hit has no spacing.
        assert_eq!(measure(&bounce(0.2, 0.7, 1), Measure::Spacing), 0.0);
    }

    // --- onsets with spacing_ms ------------------------------------------------

    #[test]
    fn onsets_spacing_ms_counts_a_dense_click_train() {
        let d = click_doc(200.0, 1.0);
        let n = 199.0f32;
        let fine = check(
            &d,
            json!({"onsets": {"min": 0, "max": 100000, "spacing_ms": 5}}),
        );
        let default = check(&d, json!({"onsets": {"min": 0, "max": 100000}}));
        println!("fine: {}\ndefault: {}", fine[0].detail, default[0].detail);
        let counted: f32 = fine[0]
            .detail
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((counted / n - 1.0).abs() < 0.1, "{counted} of {n}");
        // The default merges them: far fewer than there are clicks.
        let merged: f32 = default[0]
            .detail
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(merged < 0.5 * n, "{merged}");
        // min / max apply to the fine count.
        let ok = check(
            &d,
            json!({"onsets": {"min": 180, "max": 220, "spacing_ms": 5}}),
        );
        assert!(ok[0].pass, "{:?}", ok[0]);
        let bad = check(&d, json!({"onsets": {"max": 50, "spacing_ms": 5}}));
        assert!(!bad[0].pass, "{:?}", bad[0]);
        // The default fail text points at spacing_ms.
        let hint = check(&d, json!({"onsets": {"min": 150}}));
        assert!(
            !hint[0].pass && hint[0].detail.contains("spacing_ms"),
            "{:?}",
            hint[0]
        );
    }

    #[test]
    fn onsets_without_spacing_ms_is_unchanged() {
        let mut d = SoundDoc::named("t");
        d.nodes = vec![
            json!({"type": "impulse", "name": "i", "at": [0.0, 0.3, 0.6]}),
            json!({"type": "resonators", "name": "r", "input": "i", "modes": [[440, 0.2, 0.5]]}),
            json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
        ];
        let a = check(&d, json!({"onsets": {"min": 3, "max": 3}}));
        let b = check(
            &d,
            json!({"onsets": {"min": 3, "max": 3, "spacing_ms": 40}}),
        );
        assert!(a[0].pass && b[0].pass, "{:?} {:?}", a[0], b[0]);
        assert_eq!(a[0].detail, "3 onsets (3..3)");
        assert_eq!(b[0].detail, a[0].detail);
        // 0.3 s apart survives a 500 ms merge distance as 1 onset.
        let c = check(
            &d,
            json!({"onsets": {"min": 1, "max": 1, "spacing_ms": 700}}),
        );
        assert!(c[0].pass, "{:?}", c[0]);
        // Unknown fields and a bad spacing are errors that say what to do.
        let e: Result<Expect, _> = serde_json::from_value(json!({"onsets": {"gap": 5}}));
        assert!(e.is_err());
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap();
        let e: Expect = serde_json::from_value(json!({"onsets": {"spacing_ms": 0}})).unwrap();
        let err = run(
            &e,
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_err();
        assert!(err.contains("spacing_ms"), "{err}");
    }

    // --- the sweep's minimum effect and the new claims --------------------------

    fn knob(name: &str, claim: Claim, node: &str, field: &str, from: f64, to: f64) -> Macro {
        Macro {
            name: name.into(),
            value: 0.5,
            default: None,
            doc: String::new(),
            claim: Some(claim),
            game: false,
            map: vec![MapEntry {
                node: node.into(),
                field: field.into(),
                param: None,
                from,
                to,
                curve: MapCurve::Linear,
                col: None,
                index: None,
                key: None,
            }],
        }
    }

    fn with_macro(mut d: SoundDoc, m: Macro) -> SoundDoc {
        d.apply(
            vec![],
            DocEdit {
                macros: Some(vec![m]),
                ..DocEdit::default()
            },
            &RenderOpts::default(),
        )
        .unwrap();
        d
    }

    fn macro_line(d: &SoundDoc, name: &str) -> CheckResult {
        let res = check(d, json!({"macros": true}));
        res.into_iter()
            .find(|c| c.name == format!("macro {name}"))
            .unwrap_or_else(|| panic!("no line for {name}"))
    }

    fn tone_doc(freq: f64) -> SoundDoc {
        let mut d = SoundDoc::named("t");
        d.duration = Some(0.3);
        d.nodes = vec![
            json!({"type": "osc", "name": "o", "freq": freq, "amp": 0.5}),
            json!({"type": "output", "name": "out", "input": "o", "lufs": -20}),
        ];
        d
    }

    #[test]
    fn a_knob_that_barely_moves_its_claim_fails_and_a_real_one_passes() {
        // 2000 -> 2020 Hz: a rising centroid, 1 % of the value.
        let d = with_macro(
            tone_doc(2000.0),
            knob(
                "bright",
                Claim(Measure::Centroid, 1),
                "o",
                "freq",
                2000.0,
                2020.0,
            ),
        );
        let l = macro_line(&d, "bright");
        println!("{}", l.detail);
        assert!(
            !l.pass && l.detail.contains("barely moves") && l.detail.contains("widen"),
            "{l:?}"
        );
        // 2000 -> 3000 Hz is a real move.
        let d = with_macro(
            tone_doc(2000.0),
            knob(
                "bright",
                Claim(Measure::Centroid, 1),
                "o",
                "freq",
                2000.0,
                3000.0,
            ),
        );
        let l = macro_line(&d, "bright");
        println!("{}", l.detail);
        assert!(l.pass, "{l:?}");
    }

    #[test]
    fn the_new_claims_are_swept_by_verify() {
        // A click train whose rate the knob sets: a density claim holds.
        let mut d = SoundDoc::named("t");
        d.duration = Some(1.0);
        d.nodes = vec![
            json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 9, "start": 0.01}),
            json!({"type": "voices", "name": "v", "events": "ev", "length": 0.01, "max": 400,
               "patch": [
                   {"type": "noise", "name": "n"},
                   {"type": "decay", "name": "e", "time": 0.002},
                   {"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}]}),
            json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
        ];
        let d = with_macro(
            d,
            knob("rate", Claim(Measure::Density, 1), "ev", "every", 0.1, 0.02),
        );
        // `every` falls as the knob rises, and the count stays 9: the rate rises
        // only if the span shrinks, which it does.
        let l = macro_line(&d, "rate");
        println!("{}", l.detail);
        assert!(l.pass, "{l:?}");
        // Naming it the wrong way round fails and says so.
        let mut wrong = d.clone();
        wrong.macros[0].claim = Some(Claim(Measure::Density, -1));
        let l = macro_line(&wrong, "rate");
        println!("{}", l.detail);
        assert!(!l.pass && l.detail.contains("density"), "{l:?}");
        // The claim names parse from JSON.
        for (name, m) in [
            ("density", Measure::Density),
            ("attack", Measure::Attack),
            ("spacing", Measure::Spacing),
        ] {
            let c: Claim = serde_json::from_value(json!([name, 1])).unwrap();
            assert_eq!(c, Claim(m, 1));
            assert_eq!(m.name(), name);
        }
    }
}

mod density {
    //! The `density` measure stays proportional to the event rate of dense
    //! tick trains (10 to 500 per second), and the descriptors say how many events
    //! are strong, how many weak, and where the last one is.
    //!
    //! Three kinds of tick, all with a 3 ms decay (time constant) and a random
    //! amplitude in 0.4..1:
    //!
    //! - **A**: a decaying 3 kHz sine with a random phase (a coherent carrier: the
    //!   hardest, ticks cancel each other when their phases oppose);
    //! - **B**: a decaying sine whose frequency is random in 2..5 kHz (an impulse
    //!   into a resonator, a different one each time);
    //! - **C**: a noise burst band-passed at 3.5 kHz (Q 1.5) (rain, crackle).

    use sfx_analysis::describe;
    use sfx_analysis::measure::{RateMethod, event_rate, fine_events, measure};
    use sfx_graph::{Audio, Measure};

    const SR: u32 = 48_000;
    const SECS: f32 = 3.0;
    const RATES: [f32; 6] = [20.0, 40.0, 80.0, 160.0, 320.0, 500.0];
    const SEEDS: u64 = 6;

    struct Rng(u64);
    impl Rng {
        /// Uniform in [0, 1).
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32) / (1u64 << 24) as f32
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Tick {
        A,
        B,
        C,
    }

    fn bandpass(x: &[f32], hz: f32, q: f32) -> Vec<f32> {
        let w0 = std::f32::consts::TAU * hz / SR as f32;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b2) = (alpha / a0, -alpha / a0);
        let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha) / a0);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        x.iter()
            .map(|&xn| {
                let yn = b0 * xn + b2 * x2 - a1 * y1 - a2 * y2;
                (x2, x1, y2, y1) = (x1, xn, y1, yn);
                yn
            })
            .collect()
    }

    /// Ticks of kind `kind` at `times` (seconds) in `SECS` of silence.
    fn ticks(times: &[f32], kind: Tick, rng: &mut Rng) -> Audio {
        let tau = 0.003f32;
        let mut x = vec![0.0f32; (SECS * SR as f32) as usize];
        for t in times {
            let at = (t * SR as f32) as usize;
            let amp = 0.4 + 0.6 * rng.next();
            let phase = rng.next() * std::f32::consts::TAU;
            let hz = match kind {
                Tick::A => 3000.0,
                _ => 2000.0 + 3000.0 * rng.next(),
            };
            for j in 0..(tau * 8.0 * SR as f32) as usize {
                let Some(v) = x.get_mut(at + j) else { break };
                let s = j as f32 / SR as f32;
                let env = amp * (-s / tau).exp();
                *v += match kind {
                    Tick::C => 0.5 * env * (rng.next() * 2.0 - 1.0),
                    _ => 0.2 * env * (std::f32::consts::TAU * hz * s + phase).sin(),
                };
            }
        }
        if matches!(kind, Tick::C) {
            x = bandpass(&x, 3500.0, 1.5).iter().map(|v| v * 0.5).collect();
        }
        Audio::mono(SR, x)
    }

    fn poisson_times(rate: f32, rng: &mut Rng) -> Vec<f32> {
        let mut t = 0.05;
        let mut v = Vec::new();
        loop {
            t += -(1.0 - rng.next()).max(1e-6).ln() / rate;
            if t > SECS - 0.05 {
                return v;
            }
            v.push(t);
        }
    }

    fn regular_times(rate: f32) -> Vec<f32> {
        (0..(rate * (SECS - 0.1)) as usize)
            .map(|i| 0.05 + i as f32 / rate)
            .collect()
    }

    /// Mean over `SEEDS` seeds of (measured, true) events per second.
    fn read(kind: Tick, rate: f32, poisson: bool) -> (f32, f32) {
        let (mut m, mut truth) = (0.0, 0.0);
        for seed in 1..=SEEDS {
            let mut rng = Rng(seed * 7919 + 3);
            let t = if poisson {
                poisson_times(rate, &mut rng)
            } else {
                regular_times(rate)
            };
            let a = ticks(&t, kind, &mut rng);
            m += measure(&a, Measure::Density);
            truth += t.len() as f32 / (SECS - 0.1);
        }
        (m / SEEDS as f32, truth / SEEDS as f32)
    }

    /// Reads of `kind` over `RATES`, as (measured, true) pairs.
    fn curve(kind: Tick, poisson: bool) -> Vec<(f32, f32)> {
        let v: Vec<(f32, f32)> = RATES.iter().map(|r| read(kind, *r, poisson)).collect();
        let shown: Vec<String> = v
            .iter()
            .map(|(m, t)| format!("{t:.0}->{m:.0} ({:.2}x)", m / t))
            .collect();
        println!(
            "{} {kind:?}: true->measured {}",
            if poisson { "poisson" } else { "regular" },
            shown.join(", ")
        );
        v
    }

    fn assert_rises(v: &[(f32, f32)], what: &str) {
        assert!(
            v.windows(2).all(|w| w[1].0 > w[0].0),
            "{what}: not strictly increasing: {v:?}"
        );
    }

    fn assert_within(v: &[(f32, f32)], tol: f32, what: &str) {
        for (m, t) in v {
            assert!(
                (m / t - 1.0).abs() <= tol,
                "{what}: true {t:.0}/s read {m:.0}/s ({:.2}x, allowed +-{:.0} %)",
                m / t,
                tol * 100.0
            );
        }
    }

    #[test]
    fn poisson_ticks_b_and_c_read_within_25_percent_and_rise() {
        for kind in [Tick::B, Tick::C] {
            let v = curve(kind, true);
            assert_rises(&v, &format!("poisson {kind:?}"));
            assert_within(&v, 0.25, &format!("poisson {kind:?}"));
        }
    }

    #[test]
    fn poisson_ticks_a_rise_and_read_within_25_percent_to_320() {
        let v = curve(Tick::A, true);
        assert_rises(&v, "poisson A");
        assert_within(&v[..5], 0.25, "poisson A to 320/s");
        // 35 % at 500/s: coherent-carrier ticks read 0.68x there (the peak
        // counter loses more to phase cancellation than on the other tick kinds).
        assert_within(&v[5..], 0.35, "poisson A at 500/s");
    }

    #[test]
    fn regular_trains_read_within_25_percent_and_rise() {
        for kind in [Tick::A, Tick::B, Tick::C] {
            let v = curve(kind, false);
            assert_rises(&v, &format!("regular {kind:?}"));
            assert_within(&v, 0.25, &format!("regular {kind:?}"));
            // Up to 160/s nothing is hidden: the count, within 5 %.
            assert_within(&v[..4], 0.05, &format!("regular {kind:?} to 160/s"));
        }
    }

    /// The short clicks the older density tests use: a 3 kHz burst with a 0.3 ms
    /// decay (descriptors.rs), or a 1.5 ms noise burst with a linear fall (r1c).
    fn click_train(rate: f32, noise_burst: bool) -> Audio {
        let mut x = vec![0.0f32; (1.5 * SR as f32) as usize];
        let mut rng = Rng(7);
        let n = (rate * 1.4) as usize;
        for i in 0..n {
            let at = ((0.01 + i as f32 / rate) * SR as f32) as usize;
            if noise_burst {
                let len = (0.0015 * SR as f32) as usize;
                for j in 0..len {
                    if let Some(v) = x.get_mut(at + j) {
                        *v += 0.5 * (rng.next() * 2.0 - 1.0) * (1.0 - j as f32 / len as f32);
                    }
                }
            } else {
                tick(&mut x, at as f32 / SR as f32, 0.5);
            }
        }
        Audio::mono(SR, x)
    }

    #[test]
    fn sparse_trains_are_unchanged_by_the_overlap_correction() {
        // The counter's own reading, as before: 5 to 200 per second, regular,
        // short clicks (the older tests) and the 3 ms ticks up to 100/s.
        for rate in [5.0f32, 10.0, 20.0, 40.0, 80.0, 100.0, 160.0, 200.0] {
            let mut cases: Vec<(String, Audio)> = vec![
                ("tick".into(), click_train(rate, false)),
                ("noise burst".into(), click_train(rate, true)),
            ];
            if rate <= 100.0 {
                let t: Vec<f32> = (0..((rate * 1.4) as usize))
                    .map(|i| 0.01 + i as f32 / rate)
                    .collect();
                cases.push(("3 ms B".into(), ticks(&t, Tick::B, &mut Rng(17))));
                cases.push(("3 ms C".into(), ticks(&t, Tick::C, &mut Rng(17))));
            }
            for (name, a) in cases {
                let x = a.mix_mono();
                let ev = fine_events(&x, SR, 1000.0, 4);
                let r = event_rate(&x, SR, 1.4, &ev);
                println!(
                    "{name} regular {rate}/s: counted {:.1}, rate {:.1} ({:?})",
                    r.counted_per_s, r.per_s, r.method
                );
                assert!(
                    (r.per_s - r.counted_per_s).abs() <= 0.05 * r.counted_per_s.max(1.0),
                    "{name} {rate}/s: counted {} rate {}",
                    r.counted_per_s,
                    r.per_s
                );
            }
        }
    }

    #[test]
    fn steady_signals_read_almost_no_events() {
        let mut rng = Rng(5);
        let noise: Vec<f32> = (0..SR as usize * 3)
            .map(|_| 0.3 * (rng.next() * 2.0 - 1.0))
            .collect();
        let tone: Vec<f32> = (0..SR as usize * 3)
            .map(|i| 0.3 * (std::f32::consts::TAU * 3300.0 * i as f32 / SR as f32).sin())
            .collect();
        for (name, x) in [("white noise", noise), ("3.3 kHz tone", tone)] {
            let m = measure(&Audio::mono(SR, x), Measure::Density);
            println!("{name}: {m:.1} events/s");
            assert!(m < 12.0, "{name}: {m}");
        }
    }

    #[test]
    fn the_reading_is_deterministic_and_the_tenths_follow_it() {
        let mut rng = Rng(21);
        let t = poisson_times(320.0, &mut rng);
        let a = ticks(&t, Tick::B, &mut rng);
        assert_eq!(
            measure(&a, Measure::Density).to_bits(),
            measure(&a, Measure::Density).to_bits()
        );
        let d = describe(&a);
        let mean: f32 = d.density_tenths.iter().sum::<f32>() / 10.0;
        println!(
            "320/s Poisson: per_s {:.0} ({:?}), counted {:.0}, tenths mean {mean:.0}",
            d.density.per_s, d.density.method, d.density.counted_per_s
        );
        // The line the table prints says it corrected.
        assert!(d.density.per_s > 1.5 * d.density.counted_per_s);
        assert!((mean / d.density.per_s - 1.0).abs() < 0.2, "{mean}");
        let table = d.table();
        assert!(table.contains("density estimate:"), "{table}");
        assert!(table.contains("density: "), "{table}");
        // measure and descriptors agree.
        assert!((measure(&a, Measure::Density) - d.density.per_s).abs() < 0.05 * d.density.per_s);
    }

    /// A servo whine with an 85 Hz gear wobble (6 dB deep) and two hits: the
    /// wobble repeats like a train, but it is not events (it read 111/s).
    #[test]
    fn a_wobbling_tone_is_not_a_tick_train() {
        let sr = 48_000.0f32;
        let n = (1.5 * sr) as usize;
        let mut x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                let wob = 0.75 + 0.25 * (std::f32::consts::TAU * 85.0 * t).sin();
                0.3 * wob * (std::f32::consts::TAU * 1500.0 * t).sin()
            })
            .collect();
        for at in [0.4f32, 1.1] {
            let i0 = (at * sr) as usize;
            for i in 0..(0.02 * sr) as usize {
                let t = i as f32 / sr;
                x[i0 + i] += (-t / 0.004).exp() * (std::f32::consts::TAU * 3000.0 * t).sin();
            }
        }
        let d = describe(&Audio {
            sample_rate: 48_000,
            channels: vec![x],
        });
        println!("wobbling tone: {:?}", d.density);
        assert!(d.density.per_s < 10.0, "{:?}", d.density);
    }

    /// A zipper-like train: teeth that speed up, each a click ringing three
    /// resonances (several envelope peaks per tooth: read 5 x too high from
    /// them), and every fifth tooth quiet enough for the counter to miss
    /// (a hole in the tenths).
    #[test]
    fn a_ringing_speeding_train_with_quiet_teeth_reads_its_tooth_rate() {
        let sr = 48_000.0;
        let mut times = vec![0.01f32];
        let mut gap = 0.016f32;
        while times.len() < 50 {
            let t = times.last().unwrap() + gap;
            times.push(t);
            gap *= 0.99;
        }
        let n = ((times.last().unwrap() + 0.05) * sr) as usize;
        let mut x = vec![0.0f32; n];
        for (k, t) in times.iter().enumerate() {
            let amp = if k % 5 == 4 { 0.02 } else { 1.0 };
            let i0 = (t * sr) as usize;
            for (f, d) in [(3100.0f32, 0.006f32), (4700.0, 0.005), (6900.0, 0.004)] {
                for i in 0..(0.03 * sr) as usize {
                    if i0 + i < n {
                        let tt = i as f32 / sr;
                        x[i0 + i] += amp * (-tt / d).exp() * (std::f32::consts::TAU * f * tt).sin();
                    }
                }
            }
        }
        let a = Audio {
            sample_rate: 48_000,
            channels: vec![x],
        };
        let d = describe(&a);
        let span = times.last().unwrap() - times[0];
        let truth = times.len() as f32 / span;
        println!(
            "ringing train: truth {truth:.0}/s, read {:?}, tenths {:?}",
            d.density, d.density_tenths
        );
        assert!(
            (d.density.per_s / truth - 1.0).abs() < 0.2,
            "{} vs {truth}",
            d.density.per_s
        );
        let inner = &d.density_tenths[1..8];
        assert!(
            inner.iter().all(|v| *v > 0.4 * truth),
            "a hole in the tenths: {inner:?}"
        );
    }

    #[test]
    fn dense_trains_report_how_the_rate_was_reached() {
        let t = regular_times(320.0);
        let a = ticks(&t, Tick::B, &mut Rng(3));
        let d = describe(&a);
        println!("regular 320/s: {:?}", d.density);
        assert_eq!(d.density.method, RateMethod::Period);
        assert!((d.density.per_s / 320.0 - 1.0).abs() < 0.05);
        let t = regular_times(40.0);
        let d = describe(&ticks(&t, Tick::B, &mut Rng(3)));
        assert_eq!(d.density.method, RateMethod::Count);
        assert!(!d.table().contains("density estimate:"));
    }

    // --- the events line ---------------------------------------------------------

    /// A 3 kHz tick of peak `amp` into `x` at `at` seconds.
    fn tick(x: &mut [f32], at: f32, amp: f32) {
        let s0 = (at * SR as f32) as usize;
        for j in 0..(0.003 * SR as f32) as usize {
            if let Some(v) = x.get_mut(s0 + j) {
                let s = j as f32 / SR as f32;
                *v += amp * (std::f32::consts::TAU * 3000.0 * s).sin() * (-s / 0.0005).exp();
            }
        }
    }

    fn line(table: &str) -> &str {
        table
            .lines()
            .find(|l| l.starts_with("events:"))
            .unwrap_or("")
    }

    #[test]
    fn the_events_line_counts_strong_and_weak_and_places_the_last() {
        // Three clinks at 0.8 and five chips at 0.08 (-20 dB), the last at 0.9 s,
        // then silence to 1.5 s.
        let mut x = vec![0.0f32; (1.5 * SR as f32) as usize];
        for (t, a) in [
            (0.10, 0.8),
            (0.22, 0.08),
            (0.31, 0.08),
            (0.40, 0.8),
            (0.52, 0.08),
            (0.61, 0.08),
            (0.75, 0.08),
            (0.90, 0.8),
        ] {
            tick(&mut x, t, a);
        }
        let d = describe(&Audio::mono(SR, x));
        let t = d.table();
        println!("{}", line(&t));
        assert_eq!((d.events.strong, d.events.weak), (3, 5), "{:?}", d.events);
        let last = d.events.last_s.unwrap();
        assert!((last - 0.9).abs() < 0.003, "{last}");
        assert!(
            line(&t).starts_with(
                "events: 3 strong (>= 0.5 of the loudest onset), 5 weak; last event at 0.900 s ("
            ),
            "{}",
            line(&t)
        );
        assert!(line(&t).contains("silence follows it"), "{}", line(&t));
    }

    #[test]
    fn the_events_line_says_when_a_tail_follows_the_last_event() {
        // One tick, then a quiet noise bed that decays over 0.6 s.
        let mut rng = Rng(8);
        let mut x = vec![0.0f32; SR as usize];
        for (i, v) in x.iter_mut().enumerate().skip(4800).take(28_800) {
            let s = (i - 4800) as f32 / SR as f32;
            *v = 0.02 * (rng.next() * 2.0 - 1.0) * (-s / 0.2).exp();
        }
        tick(&mut x, 0.1, 0.8);
        let d = describe(&Audio::mono(SR, x));
        let t = d.table();
        println!("{}", line(&t));
        assert_eq!(d.events.strong, 1);
        assert!(d.events.tail_s > 0.3, "{:?}", d.events);
        assert!(
            line(&t).contains("of sound with no new event: a decaying or continuous tail"),
            "{}",
            line(&t)
        );
    }

    #[test]
    fn the_events_line_is_left_out_for_loops_and_for_silence() {
        let mut x = vec![0.0f32; SR as usize];
        tick(&mut x, 0.1, 0.5);
        tick(&mut x, 0.5, 0.5);
        let d = describe(&Audio::mono(SR, x));
        assert!(d.table().contains("events: 2 strong"), "{}", d.table());
        assert_eq!(line(&d.with_loop(true).table()), "");
        let d = describe(&Audio::mono(SR, vec![0.0; 4800]));
        assert_eq!(line(&d.table()), "");
        assert_eq!(d.events.last_s, None);
    }

    #[test]
    fn existing_lines_keep_their_text() {
        let mut x = vec![0.0f32; SR as usize];
        tick(&mut x, 0.1, 0.5);
        tick(&mut x, 0.5, 0.5);
        let t = describe(&Audio::mono(SR, x)).table();
        for start in [
            "onsets 2 (40 ms merge); fine 2 (5 ms):",
            "density: 5 events/s (2 in ",
            "density by tenth (events/s):",
            "fine onsets:",
        ] {
            assert!(t.contains(start), "missing {start:?} in\n{t}");
        }
    }
}

mod gaps {
    //! `expect onsets` takes `gap: [lo, hi]` (every gap between
    //! consecutive onsets in range, failure text lists the offender and the
    //! times). Existing behaviour is unchanged without the field.

    use serde_json::{Value, json};
    use sfx_analysis::checks::Expect;
    use sfx_analysis::{CheckCx, CheckResult, run};
    use sfx_graph::{RenderOpts, SoundDoc};

    /// Short noise ticks at the given times (seconds).
    fn hits(times: &[f32]) -> SoundDoc {
        let mut d = SoundDoc::named("hits");
        d.nodes = vec![
            json!({"type": "pattern", "name": "ev", "times": times}),
            json!({"type": "noise", "name": "n", "color": "white"}),
            json!({"type": "decay", "name": "e", "time": 0.02}),
            json!({"type": "math", "name": "tick", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "grains", "name": "g", "source": "tick", "events": "ev", "size": 0.08}),
            json!({"type": "output", "name": "out", "input": "g", "peak_db": -3}),
        ];
        d.duration = Some(times.last().copied().unwrap_or(0.0) + 0.4);
        d
    }

    fn verify(d: &SoundDoc, onsets: Value) -> CheckResult {
        let e: Expect = serde_json::from_value(json!({"onsets": onsets})).unwrap();
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
        let mut v = run(
            &e,
            &CheckCx {
                doc: d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(v.len(), 1, "{v:?}");
        let c = v.remove(0);
        println!("{}: {}", c.name, c.detail);
        c
    }

    #[test]
    fn three_hits_in_range_pass() {
        let d = hits(&[0.1, 0.3, 0.55]);
        let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
        assert!(c.pass, "{c:?}");
        assert!(
            c.detail.contains("3 onsets") && c.detail.contains("2 gaps"),
            "{c:?}"
        );
    }

    #[test]
    fn a_gap_out_of_range_fails_and_names_the_gap_and_times() {
        // Gaps 0.2 and 0.45: the second is too long.
        let d = hits(&[0.1, 0.3, 0.75]);
        let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
        assert!(!c.pass, "{c:?}");
        assert!(
            c.detail.contains("gap 2 is 0.4") && c.detail.contains("1 of 2 gaps"),
            "{c:?}"
        );
        assert!(c.detail.contains("onsets at 0.0"), "{c:?}");
        // Too close.
        let d = hits(&[0.1, 0.3, 0.4]);
        let c = verify(&d, json!({"min": 3, "max": 3, "gap": [0.15, 0.3]}));
        assert!(!c.pass && c.detail.contains("gap 2 is 0.1"), "{c:?}");
    }

    #[test]
    fn the_count_still_fails_on_its_own_and_gap_alone_works() {
        let d = hits(&[0.1, 0.3, 0.55]);
        let c = verify(&d, json!({"min": 4, "gap": [0.1, 0.5]}));
        assert!(!c.pass && c.detail.contains("want"), "{c:?}");
        let c = verify(&d, json!({"gap": [0.1, 0.5]}));
        assert!(c.pass, "{c:?}");
    }

    #[test]
    fn spacing_ms_sets_the_resolution_of_the_gaps() {
        // Ticks 20 ms apart: the default detector merges them, the fine one sees
        // them, and the gap range applies to what was counted.
        let d = hits(&[0.1, 0.12, 0.14, 0.16]);
        let c = verify(
            &d,
            json!({"spacing_ms": 5, "min": 4, "gap": [0.015, 0.025]}),
        );
        assert!(c.pass, "{c:?}");
        let c = verify(&d, json!({"spacing_ms": 5, "min": 4, "gap": [0.05, 0.1]}));
        assert!(!c.pass && c.detail.contains("outside"), "{c:?}");
    }

    #[test]
    fn a_bad_range_is_an_error_and_no_gap_changes_nothing() {
        let d = hits(&[0.1, 0.3]);
        let e: Expect = serde_json::from_value(json!({"onsets": {"gap": [0.5, 0.1]}})).unwrap();
        let opts = RenderOpts::default();
        let r = d.render(&opts).unwrap();
        let err = run(
            &e,
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_err();
        assert!(err.contains("gap") && err.contains("lo <= hi"), "{err}");
        let c = verify(&d, json!({"min": 2, "max": 2}));
        assert!(c.pass, "{c:?}");
        assert!(!c.detail.contains("gaps"), "{c:?}");
    }
}
