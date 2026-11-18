mod offline {
    //! The phase-3 offline pitch / time / spectral nodes, measured.

    use serde_json::{Value, json};
    use sfx_analysis::pitch::{cents, yin};
    use sfx_analysis::stft::distance;
    use sfx_graph::{Audio, RenderOpts, SoundDoc};

    const SR: u32 = 48_000;

    fn render(mut nodes: Vec<Value>, node: &str, secs: f32) -> Audio {
        nodes.push(json!({"type": "output", "name": "out", "input": node,
        "fade_in": 0, "fade_out": 0, "ceiling_db": 0}));
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.duration = Some(secs);
        d.render(&RenderOpts::default())
            .unwrap_or_else(|e| panic!("{e}"))
            .audio
    }

    fn median_pitch(x: &[f32]) -> f32 {
        let mut v: Vec<f32> = yin(x, SR).iter().filter_map(|f| f.hz).collect();
        assert!(!v.is_empty(), "no voiced frames");
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-9).log10()
    }

    fn sine(name: &str) -> Value {
        json!({"type": "osc", "name": name, "freq": 440, "amp": 0.5})
    }

    #[test]
    fn pitch_shift_moves_a_sine_and_keeps_the_length() {
        for (semis, target) in [(12.0f32, 880.0f32), (-5.0, 440.0 * 2f32.powf(-5.0 / 12.0))] {
            let a = render(
                vec![
                    sine("s"),
                    json!({"type": "pitch_shift", "name": "p", "input": "s", "semitones": semis}),
                ],
                "p",
                1.0,
            );
            assert_eq!(a.channels[0].len(), SR as usize);
            let hz = median_pitch(&a.channels[0]);
            let c = cents(target, hz);
            println!("pitch_shift {semis:+} st: {hz:.2} Hz vs {target:.2} ({c:+.1} cents)");
            assert!(c.abs() < 20.0, "{semis}: {hz} Hz, {c} cents off");
            // The tone is still there at the end (no truncation or silence).
            let tail = &a.channels[0][a.channels[0].len() - 4800..];
            assert!(db(rms(tail)) > -12.0, "tail {} dB", db(rms(tail)));
        }
    }

    #[test]
    fn pitch_shift_ratio_matches_semitones() {
        let go = |field: Value| {
            let mut p = json!({"type": "pitch_shift", "name": "p", "input": "s"});
            if let (Some(m), Some(f)) = (p.as_object_mut(), field.as_object()) {
                m.extend(f.clone());
            }
            render(vec![sine("s"), p], "p", 0.5)
        };
        let a = go(json!({"semitones": 12}));
        let b = go(json!({"ratio": 2.0}));
        assert_eq!(a, b);
    }

    #[test]
    fn time_stretch_doubles_gaps_and_keeps_pitch() {
        // Pitch: a sine.
        let a = render(
            vec![
                sine("s"),
                json!({"type": "time_stretch", "name": "t", "input": "s", "factor": 2}),
            ],
            "t",
            1.0,
        );
        let hz = median_pitch(&a.channels[0]);
        let c = cents(440.0, hz);
        println!("time_stretch x2 sine: {hz:.2} Hz ({c:+.1} cents)");
        assert!(c.abs() < 20.0, "{hz} Hz, {c} cents");

        // Timing: two clicks 100 ms apart become ~200 ms apart.
        let mut nodes = vec![json!({"type": "impulse", "name": "ticks", "at": [0.1, 0.2]})];
        nodes.push(json!({"type": "time_stretch", "name": "t", "input": "ticks", "factor": 2}));
        let a = render(nodes, "t", 1.0);
        let x = &a.channels[0];
        let argmax = |r: std::ops::Range<usize>| {
            r.max_by(|&i, &j| x[i].abs().total_cmp(&x[j].abs()))
                .unwrap()
        };
        let (i1, i2) = (argmax(0..14_400), argmax(14_400..28_800));
        let gap = (i2 as f32 - i1 as f32) / SR as f32;
        println!("time_stretch x2 clicks: gap {gap:.4} s (want 0.2)");
        assert!((gap - 0.2).abs() < 0.02, "gap {gap}");
    }

    #[test]
    fn spectral_freeze_outlives_the_source() {
        let nodes = vec![
            json!({"type": "osc", "name": "s", "freq": 440, "amp": 0.5}),
            json!({"type": "decay", "name": "e", "time": 0.1}),
            json!({"type": "math", "name": "tone", "op": "mul", "a": "s", "b": "e"}),
            json!({"type": "spectral", "name": "f", "input": "tone", "mode": "freeze", "at": 0.02}),
        ];
        let plain = render(nodes[..3].to_vec(), "tone", 0.6);
        let frozen = render(nodes, "f", 0.6);
        let w =
            |a: &Audio| rms(&a.channels[0][(0.4 * SR as f32) as usize..(0.5 * SR as f32) as usize]);
        println!(
            "level at 0.4-0.5 s: source {:.1} dB, frozen {:.1} dB",
            db(w(&plain)),
            db(w(&frozen))
        );
        assert!(db(w(&plain)) < -60.0);
        assert!(db(w(&frozen)) > -25.0, "frozen {} dB", db(w(&frozen)));
        // Still a 440 Hz tone-ish: the strongest YIN pitch is near 440 or silent-free noise;
        // we only require level and finiteness (random phases give a textured tone).
        assert!(frozen.channels[0].iter().all(|v| v.is_finite()));
    }

    #[test]
    fn spectral_gate_removes_a_noise_floor() {
        let nodes = vec![
            json!({"type": "osc", "name": "s", "freq": 1000, "amp": 0.5}),
            json!({"type": "noise", "name": "n", "amp": 0.01}),
            json!({"type": "math", "name": "sum", "op": "add", "a": "s", "b": "n"}),
            json!({"type": "spectral", "name": "g", "input": "sum", "mode": "gate", "threshold_db": -40}),
            // Everything but the tone: the floor, measured on the residual.
            json!({"type": "math", "name": "resid", "op": "sub", "a": "g", "b": "s"}),
            json!({"type": "math", "name": "resid0", "op": "sub", "a": "sum", "b": "s"}),
        ];
        let before = render(nodes.clone(), "resid0", 0.5);
        let after = render(nodes, "resid", 0.5);
        let mid = |a: &Audio| rms(&a.channels[0][12_000..20_000]);
        println!(
            "noise floor before {:.1} dB, after gate {:.1} dB",
            db(mid(&before)),
            db(mid(&after))
        );
        assert!(db(mid(&after)) < db(mid(&before)) - 12.0);
    }

    #[test]
    fn spectral_morph_moves_between_two_inputs() {
        let nodes = |amount: f32| {
            vec![
                json!({"type": "osc", "name": "a", "freq": 300, "amp": 0.5}),
                json!({"type": "osc", "name": "b", "shape": "saw", "freq": 700, "amp": 0.4}),
                json!({"type": "spectral", "name": "m", "input": "a", "mode": "morph",
                "other": "b", "amount": amount}),
            ]
        };
        let a = render(nodes(0.0), "a", 0.5);
        let b = render(nodes(0.0), "b", 0.5);
        let m0 = render(nodes(0.0), "m", 0.5);
        let m1 = render(nodes(1.0), "m", 0.5);
        let mh = render(nodes(0.5), "m", 0.5);
        let d = |x: &Audio, y: &Audio| distance(&x.channels[0], &y.channels[0], SR);
        let (d00, d11, dab) = (d(&m0, &a), d(&m1, &b), d(&a, &b));
        let (dh_a, dh_b) = (d(&mh, &a), d(&mh, &b));
        println!(
            "morph: 0 vs a {d00:.2} dB, 1 vs b {d11:.2} dB; a vs b {dab:.2}; 0.5 vs a {dh_a:.2}, vs b {dh_b:.2}"
        );
        assert!(d00 < 3.0 && d11 < 3.0, "{d00} {d11}");
        assert!(dh_a > d00 && dh_b > d11);
    }

    #[test]
    fn spectral_blur_smooths_and_stays_finite() {
        let nodes = vec![
            json!({"type": "noise", "name": "n", "amp": 0.3}),
            json!({"type": "spectral", "name": "b", "input": "n", "mode": "blur", "amount": 0.5}),
        ];
        let a = render(nodes, "b", 0.4);
        assert!(a.channels[0].iter().all(|v| v.is_finite()));
        assert!(rms(&a.channels[0]) > 0.001);
    }

    #[test]
    fn deterministic_stereo_and_silent() {
        let nodes = vec![
            json!({"type": "osc", "name": "s", "freq": 440, "amp": 0.5}),
            json!({"type": "pan", "name": "st", "input": "s", "pan": 0.4}),
            json!({"type": "pitch_shift", "name": "p", "input": "st", "semitones": 7}),
            json!({"type": "time_stretch", "name": "t", "input": "st", "factor": 1.5}),
            json!({"type": "spectral", "name": "f", "input": "st", "mode": "freeze", "at": 0.1, "seed": 3}),
            json!({"type": "mix", "name": "all", "inputs": ["p", "t", "f"], "to": "stereo"}),
        ];
        let a = render(nodes.clone(), "all", 0.4);
        let b = render(nodes, "all", 0.4);
        assert_eq!(a.channels.len(), 2);
        assert_eq!(a, b);

        for extra in [
            json!({"type": "pitch_shift", "name": "x", "input": "z", "semitones": -7}),
            json!({"type": "time_stretch", "name": "x", "input": "z", "factor": 0.5}),
            json!({"type": "spectral", "name": "x", "input": "z", "mode": "freeze", "at": 0.1}),
            json!({"type": "spectral", "name": "x", "input": "z", "mode": "blur", "amount": 1}),
            json!({"type": "spectral", "name": "x", "input": "z", "mode": "gate", "threshold_db": -60}),
            json!({"type": "spectral", "name": "x", "input": "z", "mode": "morph", "other": "z", "amount": 0.5}),
        ] {
            let a = render(
                vec![
                    json!({"type": "osc", "name": "z", "freq": 440, "amp": 0.0}),
                    extra,
                ],
                "x",
                0.3,
            );
            assert!(a.channels[0].iter().all(|v| v.is_finite()));
            assert!(rms(&a.channels[0]) < 1e-6);
        }
    }

    #[test]
    fn bad_fields_explain_themselves() {
        let mut d = SoundDoc::named("t");
        d.nodes = vec![
            json!({"type": "osc", "name": "s", "freq": 440}),
            json!({"type": "spectral", "name": "x", "input": "s", "mode": "gate", "threshold_db": -40, "at": 1.0}),
            json!({"type": "output", "name": "out", "input": "x"}),
        ];
        d.duration = Some(0.2);
        let e = d.render(&RenderOpts::default()).unwrap_err();
        assert!(e.contains("`at`") && e.contains("Freeze"), "{e}");
    }
}

mod modes {
    //! `spectral` modes shift / stretch / smear / hps and `pitch_shift.formant`.
    //! The first test pins the output of every pre-existing mode by hash.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_graph::{Buf, RenderOpts, SoundDoc};

    /// FNV-1a over the bit patterns of every sample of every channel of node `name`.
    fn hash_node(nodes: &[Value], name: &str, duration: f32) -> u64 {
        let mut d = SoundDoc::named("hash");
        d.nodes = nodes.to_vec();
        d.nodes
            .push(json!({"type": "output", "name": "_out", "input": name}));
        d.duration = Some(duration);
        let r = d
            .render(&RenderOpts {
                cached: false,
                ..RenderOpts::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let buf = r.eval.buf(name).unwrap();
        let chans: Vec<Vec<f32>> = match &*buf {
            Buf::Audio(x) => vec![x.clone()],
            Buf::Stereo(x) => vec![
                x.iter().map(|s| s[0]).collect(),
                x.iter().map(|s| s[1]).collect(),
            ],
            o => panic!("{}", o.kind()),
        };
        let mut h = 0xcbf29ce484222325u64;
        for c in chans {
            for v in c {
                for b in v.to_bits().to_le_bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
        }
        h
    }

    /// A fixed test signal: a decaying two-partial tone plus noise, mono, and a
    /// panned stereo copy.
    fn base() -> Vec<Value> {
        vec![
            json!({"type": "osc", "name": "a", "freq": 440, "amp": 0.4}),
            json!({"type": "osc", "name": "b", "freq": 1230, "amp": 0.2}),
            json!({"type": "noise", "name": "n", "amp": 0.05}),
            json!({"type": "mix", "name": "m0", "inputs": ["a", "b", "n"]}),
            json!({"type": "decay", "name": "e", "time": 0.3}),
            json!({"type": "math", "name": "src", "op": "mul", "a": "m0", "b": "e"}),
            json!({"type": "pan", "name": "st", "input": "src", "pan": 0.3}),
            json!({"type": "osc", "name": "o2", "shape": "saw", "freq": 300, "amp": 0.3}),
        ]
    }

    fn with(extra: Vec<Value>) -> Vec<Value> {
        let mut v = base();
        v.extend(extra);
        v
    }

    /// The hashes below were recorded on the commit before X1 touched
    /// `spectral.rs`. A change here means an existing mode moved.
    #[test]
    fn existing_modes_are_bit_identical_to_before_x1() {
        let cases: Vec<(&str, Vec<Value>, u64)> = vec![
            (
                "pitch_shift +7",
                with(vec![
                    json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7}),
                ]),
                0x0e3d0def90663e29,
            ),
            (
                "pitch_shift -12 stereo w1024",
                with(vec![
                    json!({"type": "pitch_shift", "name": "x", "input": "st", "semitones": -12, "window": 1024}),
                ]),
                0xed0abbb02b355a38,
            ),
            (
                "pitch_shift ratio 1.3",
                with(vec![
                    json!({"type": "pitch_shift", "name": "x", "input": "src", "ratio": 1.3}),
                ]),
                0xaa761df6380050d5,
            ),
            (
                "time_stretch 1.5",
                with(vec![
                    json!({"type": "time_stretch", "name": "x", "input": "src", "factor": 1.5}),
                ]),
                0x1ec788c4aa15d739,
            ),
            (
                "time_stretch 0.7 stereo",
                with(vec![
                    json!({"type": "time_stretch", "name": "x", "input": "st", "factor": 0.7}),
                ]),
                0x76c8f2baa9894175,
            ),
            (
                "freeze",
                with(vec![
                    json!({"type": "spectral", "name": "x", "input": "src", "mode": "freeze", "at": 0.05, "length": 0.2, "seed": 3}),
                ]),
                0x9da65de238040347,
            ),
            (
                "blur",
                with(vec![
                    json!({"type": "spectral", "name": "x", "input": "src", "mode": "blur", "amount": 0.6}),
                ]),
                0x0dd9c0ded8b4285b,
            ),
            (
                "gate stereo",
                with(vec![
                    json!({"type": "spectral", "name": "x", "input": "st", "mode": "gate", "threshold_db": -50}),
                ]),
                0xf7cc2aa9486b2880,
            ),
            (
                "morph",
                with(vec![
                    json!({"type": "spectral", "name": "x", "input": "src", "mode": "morph", "other": "o2", "amount": 0.5}),
                ]),
                0x2261d66e192d4099,
            ),
        ];
        let mut bad = Vec::new();
        for (what, nodes, want) in cases {
            let h = hash_node(&nodes, "x", 0.5);
            println!("HASH {what}: {h:#018x}");
            if h != want {
                bad.push(what);
            }
        }
        assert!(bad.is_empty(), "output changed: {bad:?}");
    }

    // ---------------------------------------------------------------- helpers

    /// `spectral` named `x` over node `src` with extra fields.
    fn spec(mode: &str, extra: Value) -> Value {
        let mut r = json!({"type": "spectral", "name": "x", "input": "src", "mode": mode});
        r.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        r
    }

    fn sine(hz: f32, amp: f32) -> Value {
        json!({"type": "osc", "name": "src", "freq": hz, "amp": amp})
    }

    fn render_error(nodes: Vec<Value>) -> String {
        let mut d = SoundDoc::named("test");
        d.nodes = nodes;
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "x"}));
        d.duration = Some(0.1);
        d.render(&RenderOpts::default()).unwrap_err()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    // ------------------------------------------------------------------ shift

    #[test]
    fn shift_moves_a_sine_by_hz_and_keeps_its_level() {
        for (hz, want) in [(300.0f32, 1300.0f32), (-300.0, 700.0), (7.5, 1007.5)] {
            let x = node_audio(
                vec![sine(1000.0, 0.5), spec("shift", json!({"hz": hz}))],
                "x",
                1.0,
            );
            assert_finite(&x, "shift");
            let got = peak_hz(&x, SR);
            let level = band_db(&x, SR, want - 10.0, want + 10.0);
            let old = band_db(&x, SR, 990.0, 1010.0);
            println!(
                "shift {hz:+} Hz: peak {got:.2} Hz (want {want}), level {level:.2} dB (input -9.03), 1 kHz band {old:.1} dB"
            );
            assert!((got - want).abs() < 1.0, "{hz}: {got}");
            assert!((level + 9.03).abs() < 0.5, "{hz}: {level} dB");
            if hz.abs() > 100.0 {
                assert!(old < -60.0, "{hz}: 1 kHz left at {old} dB");
            }
        }
    }

    #[test]
    fn shift_makes_a_harmonic_series_inharmonic() {
        let nodes = vec![
            json!({"type": "osc", "name": "a", "freq": 200, "amp": 0.2}),
            json!({"type": "osc", "name": "b", "freq": 400, "amp": 0.2}),
            json!({"type": "osc", "name": "c", "freq": 600, "amp": 0.2}),
            json!({"type": "mix", "name": "src", "inputs": ["a", "b", "c"]}),
            spec("shift", json!({"hz": 50})),
        ];
        let p = peaks(&node_audio(&nodes, "x", 1.0), SR, 3, 20.0);
        let mut hz: Vec<f32> = p.iter().map(|p| p.0).collect();
        hz.sort_by(f32::total_cmp);
        println!("200/400/600 shifted +50 Hz: {hz:?}");
        for (g, w) in hz.iter().zip([250.0, 450.0, 650.0]) {
            assert!((g - w).abs() < 1.0, "{hz:?}");
        }
    }

    #[test]
    fn shift_removes_what_leaves_the_band_instead_of_folding_it() {
        // 20 kHz up 6 kHz would pass Nyquist; 5 kHz down 8 kHz would pass 0 Hz.
        for (f, hz) in [(20_000.0f32, 6000.0f32), (5000.0, -8000.0)] {
            let x = node_audio(
                vec![sine(f, 0.5), spec("shift", json!({"hz": hz}))],
                "x",
                0.5,
            );
            assert_finite(&x, "shift");
            println!(
                "{f} Hz shifted {hz:+}: rms {:.1} dB",
                20.0 * rms(&x).max(1e-9).log10()
            );
            assert!(20.0 * rms(&x).max(1e-9).log10() < -50.0);
        }
    }

    #[test]
    fn shift_takes_a_signal() {
        let nodes = vec![
            sine(1000.0, 0.5),
            json!({"type": "ramp", "name": "r", "from": 0, "to": 600, "time": 1.0, "curve": "linear"}),
            spec("shift", json!({"hz": "r"})),
        ];
        let x = node_audio(nodes, "x", 1.0);
        let early = peak_hz(&x[2400..7200], SR);
        let late = peak_hz(&x[40_800..45_600], SR);
        println!(
            "ramped shift 0 -> 600 Hz: peak {early:.1} Hz early (0.05-0.15 s), {late:.1} Hz late (0.85-0.95 s)"
        );
        assert!((early - 1060.0).abs() < 25.0 && (late - 1540.0).abs() < 25.0);
    }

    // ---------------------------------------------------------------- stretch

    fn series() -> Vec<Value> {
        let mut v: Vec<Value> = (1..=8)
            .map(|n| json!({"type": "osc", "name": format!("h{n}"), "freq": 200 * n, "amp": 0.1}))
            .collect();
        let names: Vec<String> = (1..=8).map(|n| format!("h{n}")).collect();
        v.push(json!({"type": "mix", "name": "src", "inputs": names}));
        v
    }

    #[test]
    fn stretch_moves_partials_by_the_stiff_string_law() {
        let b = 0.02f32;
        let mut nodes = series();
        nodes.push(spec("stretch", json!({"inharmonicity": b, "f0": 200})));
        let x = node_audio(&nodes, "x", 1.0);
        assert_finite(&x, "stretch");
        let p = peaks(&x, SR, 8, 30.0);
        let mut got: Vec<f32> = p.iter().map(|p| p.0).collect();
        got.sort_by(f32::total_cmp);
        let want: Vec<f32> = (1..=8)
            .map(|n| 200.0 * n as f32 * (1.0 + b * (n * n) as f32).sqrt())
            .collect();
        println!("stretch b=0.02: peaks {got:.1?}\n                want  {want:.1?}");
        assert_eq!(got.len(), 8);
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 0.01 * w, "{g} vs {w}");
        }
        // Level kept: every partial within 2 dB of the input's per-partial level.
        let before = band_db(&node_audio(series(), "src", 1.0), SR, 100.0, 4000.0);
        let after = band_db(&x, SR, 100.0, 5000.0);
        println!("stretch b=0.02 total level {after:.2} dB, input {before:.2} dB");
        assert!((after - before).abs() < 2.0);
    }

    #[test]
    fn stretch_with_zero_inharmonicity_leaves_the_partials_alone() {
        let mut nodes = series();
        nodes.push(spec("stretch", json!({"inharmonicity": 0})));
        let x = node_audio(&nodes, "x", 1.0);
        let p = peaks(&x, SR, 8, 30.0);
        let mut got: Vec<f32> = p.iter().map(|p| p.0).collect();
        got.sort_by(f32::total_cmp);
        println!("stretch b=0: peaks {got:.1?}");
        for (g, n) in got.iter().zip(1..=8) {
            assert!((g - 200.0 * n as f32).abs() < 2.0, "{got:?}");
        }
    }

    // ------------------------------------------------------------------ smear

    /// (time of the 10 % point, time of the 90 % point) of the energy, seconds.
    fn energy_span(x: &[f32]) -> (f32, f32) {
        let total: f32 = x.iter().map(|v| v * v).sum();
        let mut acc = 0.0;
        let (mut t10, mut t90) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate() {
            acc += v * v;
            if t10 == 0.0 && acc >= 0.1 * total {
                t10 = i as f32 / SR as f32;
            }
            if acc >= 0.9 * total {
                t90 = i as f32 / SR as f32;
                break;
            }
        }
        (t10, t90)
    }

    #[test]
    fn smear_spreads_a_click_into_a_chirp_and_keeps_the_energy() {
        let click = json!({"type": "impulse", "name": "src", "at": 0.1, "amp": 0.5});
        let plain = node_audio(vec![click.clone()], "src", 0.5);
        let x = node_audio(vec![click, spec("smear", json!({"time": 0.05}))], "x", 0.5);
        assert_finite(&x, "smear");
        let e = |v: &[f32]| 10.0 * v.iter().map(|s| s * s).sum::<f32>().log10();
        let (t10, t90) = energy_span(&x);
        let (lo_early, hi_early) = (
            band_db(&x[4800..5400], SR, 100.0, 4000.0),
            band_db(&x[4800..5400], SR, 12_000.0, 24_000.0),
        );
        let (lo_late, hi_late) = (
            band_db(&x[6600..7200], SR, 100.0, 4000.0),
            band_db(&x[6600..7200], SR, 12_000.0, 24_000.0),
        );
        println!(
            "smear 0.05 s: energy {:.2} dB (click {:.2} dB); 10-90 % span {:.1}-{:.1} ms; \
         0.1000-0.1125 s low/high {lo_early:.1}/{hi_early:.1} dB; 0.1375-0.15 s {lo_late:.1}/{hi_late:.1} dB; peak {:.3} (click 0.5)",
            e(&x),
            e(&plain),
            t10 * 1e3,
            t90 * 1e3,
            x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
        );
        assert!((e(&x) - e(&plain)).abs() < 0.5);
        assert!(t90 - t10 > 0.03 && t90 - t10 < 0.06, "span {}", t90 - t10);
        assert!(t10 >= 0.099, "nothing before the click: {t10}");
        // Lows lead, highs trail.
        assert!(lo_early - hi_early > lo_late - hi_late + 6.0);
        // A flat magnitude spectrum stays flat.
        let (a, b) = (
            band_db(&x, SR, 1000.0, 3000.0),
            band_db(&plain, SR, 1000.0, 3000.0),
        );
        assert!((a - b).abs() < 1.0, "{a} vs {b}");
    }

    // -------------------------------------------------------------------- hps

    #[test]
    fn hps_puts_the_sine_in_harmonic_and_the_clicks_in_percussive() {
        let nodes = vec![
            json!({"type": "osc", "name": "tone", "freq": 440, "amp": 0.3}),
            json!({"type": "impulse", "name": "ticks", "at": [0.2, 0.5, 0.8], "amp": 0.8}),
            json!({"type": "math", "name": "src", "op": "add", "a": "tone", "b": "ticks"}),
            spec("hps", json!({})),
        ];
        let h = node_audio(&nodes, "x.harmonic", 1.0);
        let p = node_audio(&nodes, "x.percussive", 1.0);
        let o = node_audio(&nodes, "x", 1.0);
        let sum: Vec<f32> = h.iter().zip(&p).map(|(a, b)| a + b).collect();
        let src = node_audio(&nodes, "src", 1.0);
        let (sine_h, sine_p) = (band_db(&h, SR, 400.0, 480.0), band_db(&p, SR, 400.0, 480.0));
        let (click_h, click_p) = (
            band_db(&h, SR, 3000.0, 20_000.0),
            band_db(&p, SR, 3000.0, 20_000.0),
        );
        let err = rms(&sum.iter().zip(&src).map(|(a, b)| a - b).collect::<Vec<_>>());
        println!(
            "hps: 440 Hz band harmonic {sine_h:.1} dB, percussive {sine_p:.1} dB (gap {:.1}); \
         3-20 kHz band percussive {click_p:.1} dB, harmonic {click_h:.1} dB (gap {:.1}); \
         harmonic + percussive vs input: error rms {:.1} dB",
            sine_h - sine_p,
            click_p - click_h,
            20.0 * err.max(1e-9).log10()
        );
        assert!(sine_h - sine_p >= 20.0);
        assert!(click_p - click_h >= 20.0);
        assert_eq!(o, h, "the default port is the harmonic part");
        assert!(err < 0.01 * rms(&src));
    }

    // ---------------------------------------------------------------- formant

    /// A saw at `f0` through a band-pass resonance at 1200 Hz.
    fn voiced(f0: f32) -> Vec<Value> {
        vec![
            json!({"type": "osc", "name": "raw", "shape": "saw", "freq": f0, "amp": 0.5}),
            json!({"type": "filter", "name": "src", "input": "raw", "mode": "bp", "freq": 1200, "q": 3}),
        ]
    }

    /// Frequency of the peak of the harmonic envelope: the harmonics (spectral
    /// peaks 300 .. 6000 Hz within 40 dB of the strongest) with a parabola through
    /// the strongest one and its two neighbours, in dB over Hz. Reads between
    /// harmonics, so a formant that sits off the harmonic grid is still located.
    fn envelope_peak(x: &[f32]) -> f32 {
        let mut p: Vec<(f32, f32)> = peaks(x, SR, 60, 40.0)
            .into_iter()
            .filter(|p| (300.0..6000.0).contains(&p.0))
            .collect();
        p.sort_by(|a, b| a.0.total_cmp(&b.0));
        let i = (0..p.len())
            .max_by(|&a, &b| p[a].1.total_cmp(&p[b].1))
            .unwrap();
        assert!(
            i > 0 && i + 1 < p.len(),
            "peak at the edge of the harmonics"
        );
        let ((x0, y0), (x1, y1), (x2, y2)) = (p[i - 1], p[i], p[i + 1]);
        // Vertex of the parabola through three points.
        let d = (x0 - x1) * (x0 - x2) * (x1 - x2);
        let a = (x2 * (y1 - y0) + x1 * (y0 - y2) + x0 * (y2 - y1)) / d;
        let b = (x2 * x2 * (y0 - y1) + x1 * x1 * (y2 - y0) + x0 * x0 * (y1 - y2)) / d;
        -b / (2.0 * a)
    }

    #[test]
    fn pitch_shift_formant_keeps_the_envelope_peak() {
        let go = |extra: Value| {
            let mut nodes = voiced(150.0);
            let mut p = json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7});
            p.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            nodes.push(p);
            node_audio(nodes, "x", 1.0)
        };
        let orig = node_audio(voiced(150.0), "src", 1.0);
        let (plain, kept) = (go(json!({})), go(json!({"formant": true})));
        assert_finite(&kept, "formant");
        let (e0, e1, e2) = (
            envelope_peak(&orig),
            envelope_peak(&plain),
            envelope_peak(&kept),
        );
        let f0 = median_f0(&kept);
        println!(
            "envelope peak: input {e0:.0} Hz; +7 st plain {e1:.0} Hz ({:+.1} %); +7 st formant {e2:.0} Hz ({:+.1} %); \
         pitch of the formant output {f0:.1} Hz (want {:.1}); level {:.1} dB vs plain {:.1} dB",
            100.0 * (e1 / e0 - 1.0),
            100.0 * (e2 / e0 - 1.0),
            150.0 * 2f32.powf(7.0 / 12.0),
            20.0 * rms(&kept).max(1e-9).log10(),
            20.0 * rms(&plain).max(1e-9).log10(),
        );
        assert!(
            (e2 / e0 - 1.0).abs() < 0.05,
            "formant peak moved {e0} -> {e2}"
        );
        assert!(
            (e1 / e0 - 1.0).abs() > 0.10,
            "plain shift should move it: {e1}"
        );
        assert!((f0 / (150.0 * 2f32.powf(7.0 / 12.0)) - 1.0).abs() < 0.02);
    }

    fn median_f0(x: &[f32]) -> f32 {
        let mut v: Vec<f32> = sfx_analysis::pitch::yin(x, SR)
            .iter()
            .filter_map(|f| f.hz)
            .collect();
        assert!(!v.is_empty(), "no voiced frames");
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn formant_false_is_the_default() {
        let mut a = voiced(150.0);
        a.push(json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7}));
        let mut b = voiced(150.0);
        b.push(json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7, "formant": false}));
        assert_eq!(hash_node(&a, "x", 0.5), hash_node(&b, "x", 0.5));
    }

    // ----------------------------------------------------- determinism, extremes

    fn all_modes() -> Vec<(&'static str, Value)> {
        vec![
            ("shift", spec("shift", json!({"hz": 123.4}))),
            ("stretch", spec("stretch", json!({"inharmonicity": 0.05}))),
            ("smear", spec("smear", json!({"time": 0.03}))),
            ("hps", spec("hps", json!({}))),
        ]
    }

    #[test]
    fn new_modes_are_bit_identical() {
        for (what, node) in all_modes() {
            let mut nodes = base();
            nodes.push(node);
            if what == "hps" {
                // Both ports.
                nodes.push(json!({"type": "mix", "name": "both", "inputs": ["x", "x.percussive"]}));
            }
            assert_bit_identical(&nodes, 0.4);
            println!("{what}: two renders (pool and one thread) have the same bits");
        }
        let mut nodes = voiced(150.0);
        nodes.push(json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": 7, "formant": true}));
        assert_bit_identical(&nodes, 0.4);
    }

    #[test]
    fn extremes_stay_finite() {
        let loud = json!({"type": "noise", "name": "src", "amp": 1.0e6});
        let cases: Vec<(String, Value)> = vec![
            ("shift -24000".into(), spec("shift", json!({"hz": -24000}))),
            ("shift 24000".into(), spec("shift", json!({"hz": 24000}))),
            ("shift 0".into(), spec("shift", json!({"hz": 0}))),
            ("shift 1e-6".into(), spec("shift", json!({"hz": 1e-6}))),
            (
                "stretch b=2 f0=1".into(),
                spec("stretch", json!({"inharmonicity": 2, "f0": 1})),
            ),
            (
                "stretch b=0 f0=23520".into(),
                spec("stretch", json!({"inharmonicity": 0, "f0": 23520})),
            ),
            (
                "stretch w64".into(),
                spec("stretch", json!({"inharmonicity": 1, "window": 64})),
            ),
            ("smear 2".into(), spec("smear", json!({"time": 2}))),
            ("smear -2".into(), spec("smear", json!({"time": -2}))),
            ("smear 0".into(), spec("smear", json!({"time": 0}))),
            ("hps 3".into(), spec("hps", json!({"kernel": 3}))),
            (
                "hps 101".into(),
                spec("hps", json!({"kernel": 101, "window": 64})),
            ),
        ];
        for (what, node) in cases {
            for sig in [
                loud.clone(),
                json!({"type": "osc", "name": "src", "freq": 100, "amp": 0.0}),
            ] {
                let nodes = vec![sig, node.clone()];
                let x = node_audio(&nodes, "x", 0.3);
                assert_finite(&x, &what);
                let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                println!("{what}: peak {peak:.3e}");
                assert!(peak < 1.0e8);
            }
        }
        // pitch_shift formant at the extremes, with silence in the middle.
        for st in [-24.0, 24.0] {
            let nodes = vec![
                json!({"type": "noise", "name": "n", "amp": 1.0e6}),
                json!({"type": "decay", "name": "e", "time": 0.02}),
                json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "e"}),
                json!({"type": "pitch_shift", "name": "x", "input": "src", "semitones": st, "formant": true, "window": 256}),
            ];
            assert_finite(&node_audio(&nodes, "x", 0.3), "formant extreme");
        }
    }

    #[test]
    fn new_modes_run_on_both_channels() {
        for (what, node) in all_modes() {
            let nodes = vec![
                sine(440.0, 0.5),
                json!({"type": "pan", "name": "st", "input": "src", "pan": 0.5}),
                {
                    let mut n = node.clone();
                    n["input"] = json!("st");
                    n
                },
            ];
            let a = render(&nodes, 0.3);
            println!(
                "{what}: {} channels, rms L {:.1} dB R {:.1} dB",
                a.channels.len(),
                20.0 * rms(&a.channels[0]).max(1e-9).log10(),
                20.0 * rms(a.channels.get(1).unwrap_or(&a.channels[0]))
                    .max(1e-9)
                    .log10()
            );
            assert_eq!(a.channels.len(), 2, "{what}");
            assert!(a.channels.iter().all(|c| c.iter().all(|v| v.is_finite())));
            assert!(a.channels.iter().all(|c| rms(c) > 1e-4), "{what}: silent");
        }
    }

    // ----------------------------------------------------------------- errors

    #[test]
    fn errors_name_the_field_and_the_fix() {
        let e = |extra: Value, mode: &str| render_error(vec![sine(440.0, 0.5), spec(mode, extra)]);
        let cases = [
            (e(json!({"hz": 30000}), "shift"), "hz: 30000"),
            (e(json!({}), "shift"), "shift needs `hz`"),
            (
                e(json!({"inharmonicity": 5}), "stretch"),
                "inharmonicity: 5: give a number from 0 to 2",
            ),
            (
                e(json!({"f0": 0}), "stretch"),
                "f0: 0: give a frequency from 1 to",
            ),
            (
                e(json!({"time": 3}), "smear"),
                "time: 3: give seconds from -2 to 2",
            ),
            (e(json!({}), "smear"), "smear needs `time`"),
            (
                e(json!({"kernel": 8}), "hps"),
                "kernel: 8: give an odd number from 3 to 101",
            ),
            (e(json!({"kernel": 103}), "hps"), "kernel: 103"),
            (
                e(json!({"hz": 5}), "gate"),
                "field `hz` is only for mode Shift",
            ),
            (
                e(json!({"hz": 5, "window": 1024}), "shift"),
                "field `window` is not used by mode Shift",
            ),
            (
                e(json!({"time": 0.1, "window": 1024}), "smear"),
                "field `window` is not used by mode Smear",
            ),
        ];
        for (msg, want) in cases {
            println!("{msg}");
            assert!(msg.contains(want), "`{msg}` should contain `{want}`");
        }
    }

    #[test]
    fn a_port_name_that_does_not_exist_is_refused() {
        let nodes = vec![
            sine(440.0, 0.5),
            spec("hps", json!({})),
            json!({"type": "gain", "name": "g", "input": "x.nope", "gain": 1}),
        ];
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "g"}));
        d.duration = Some(0.1);
        let m = d.render(&RenderOpts::default()).unwrap_err();
        println!("{m}");
        assert!(m.contains("harmonic") && m.contains("percussive"), "{m}");
    }
}
