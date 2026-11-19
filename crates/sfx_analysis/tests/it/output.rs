mod stage {
    //! The output stage: lufs / peak_db targets, the truthful missed-target
    //! warning, normalizing up from very quiet, and the fade-in warning.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_graph::{Kind, RenderOpts, Rendered, SoundDoc};

    fn render(d: &SoundDoc) -> Result<Rendered, String> {
        d.render(&RenderOpts::default())
    }

    fn ok(d: &SoundDoc) -> Rendered {
        render(d).unwrap_or_else(|e| panic!("{e}"))
    }

    /// FNV-1a over the bit patterns of every sample, channel by channel.
    fn hash(r: &Rendered) -> u64 {
        fnv_channels(&r.audio.channels)
    }

    /// The `laser` example's graph (examples/laser.rs).
    fn laser() -> SoundDoc {
        bare_doc(
            vec![
                json!({"type": "ramp", "name": "sweep", "from": 2000, "to": 200, "time": 0.25, "curve": "exp"}),
                json!({"type": "osc", "name": "tone", "shape": "saw", "freq": "sweep"}),
                json!({"type": "decay", "name": "amp", "time": 0.3}),
                json!({"type": "math", "name": "zap", "op": "mul", "a": "tone", "b": "amp"}),
                json!({"type": "math", "name": "cutoff", "op": "mul", "a": "sweep", "b": 3}),
                json!({"type": "filter", "name": "lp", "input": "zap", "mode": "lp", "freq": "cutoff", "q": 1.5}),
                json!({"type": "output", "name": "out", "input": "lp", "lufs": -18}),
            ],
            None,
        )
    }

    #[test]
    fn existing_output_behaviour_is_bit_identical() {
        // Hashes taken on the commit before the output-stage change.
        let a = ok(&laser());
        let mut b = laser();
        b.nodes[6] = json!({"type": "output", "name": "out", "input": "lp", "gain_db": -6,
        "fade_in": 0.01, "fade_out": 0.02, "ceiling_db": -3});
        let b = ok(&b);
        let mut c = laser();
        c.nodes[6] = json!({"type": "output", "name": "out", "input": "lp"});
        let c = ok(&c);
        // A peaky sound that misses its lufs target (only the warning changes).
        let d = bare_doc(
            vec![
                json!({"type": "impulse", "name": "i"}),
                json!({"type": "output", "name": "out", "input": "i", "lufs": -14}),
            ],
            Some(0.5),
        );
        let d = ok(&d);
        // R1-B: the laser starts at full level at t = 0, so the default 2 ms
        // fade-in cuts its peak and loudness by 0.24 dB; the lufs target is now
        // met after the fades and this hash moved (it was 0x43ce_d14c_0173_460a).
        assert_eq!(hash(&a), LASER_LUFS_AFTER_FADES, "laser lufs -18");
        assert_eq!(
            hash(&b),
            0x729c_2d96_e9f7_2090,
            "laser gain / fades / ceiling"
        );
        // R2-B: the bare laser peaks at +2.2 dBFS, so the ceiling limiter acts;
        // it now holds the TRUE peak, so this hash moved (it was
        // 0xc7a9_1213_d063_39b9, the sample-peak limiter). Every other hash here
        // is a sound whose true peak is under the ceiling: unchanged.
        assert_eq!(hash(&c), BARE_LASER_TRUE_PEAK_LIMITED, "laser bare output");
        let tp = sfx_graph::gain_to_db(sfx_graph::loudness::true_peak(&c.audio.channels));
        assert!(tp <= -0.95, "bare laser true peak {tp}");
        assert_eq!(hash(&d), 0x5eac_53d2_74f9_ef25, "impulse missing lufs -14");
        // The same laser with no hit near 0 (30 ms late), default fades: hash
        // taken on the commit before R1-B.
        let mut e = laser();
        e.nodes[6] = json!({"type": "output", "name": "out", "input": "late", "lufs": -18});
        e.nodes.insert(
            6,
            json!({"type": "delay", "name": "late", "input": "lp", "time": 0.03}),
        );
        assert_eq!(hash(&ok(&e)), DELAYED_LASER_BEFORE, "late laser lufs -18");
    }

    const BARE_LASER_TRUE_PEAK_LIMITED: u64 = 8_512_498_053_249_768_001;
    const LASER_LUFS_AFTER_FADES: u64 = 13_211_617_635_871_047_603;
    const DELAYED_LASER_BEFORE: u64 = 13_838_043_244_579_427_677;

    fn noise_at(db: f32, output: Value, duration: f32) -> SoundDoc {
        bare_doc(
            vec![
                json!({"type": "noise", "name": "n", "seed": 4}),
                json!({"type": "gain", "name": "q", "input": "n", "db": db}),
                output,
            ],
            Some(duration),
        )
    }

    fn lufs_of(r: &Rendered) -> f32 {
        r.lufs
    }

    fn train(output: Value) -> SoundDoc {
        bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [0.1, 0.3, 0.5, 0.7]}),
                output,
            ],
            Some(1.0),
        )
    }

    fn has(r: &Rendered, needle: &str) -> Option<String> {
        r.warnings.iter().find(|w| w.contains(needle)).cloned()
    }

    #[test]
    fn peak_db_normalizes_the_peak_of_an_impulse_train() {
        let d = train(json!({"type": "output", "name": "out", "input": "i", "peak_db": -3}));
        let r = ok(&d);
        println!("peak_db -3: peak {:.3} dBFS, {:.1} LUFS", r.peak_db, r.lufs);
        assert!((r.peak_db + 3.0).abs() < 0.1, "peak {}", r.peak_db);
        assert_eq!(hash(&r), hash(&ok(&d)), "bit-identical on a second render");
        // The same train under lufs cannot get there; peak_db has no loudness warning.
        assert!(has(&r, "output lufs").is_none(), "{:?}", r.warnings);
    }

    #[test]
    fn lufs_and_peak_db_together_is_an_error_naming_both() {
        let d = train(
            json!({"type": "output", "name": "out", "input": "i", "peak_db": -3, "lufs": -20}),
        );
        let e = render(&d).unwrap_err();
        println!("{e}");
        assert!(
            e.contains("lufs") && e.contains("peak_db") && e.contains("pick one"),
            "{e}"
        );
        let d = train(json!({"type": "output", "name": "out", "input": "i", "peak_db": 0}));
        let e = render(&d).unwrap_err();
        println!("{e}");
        assert!(e.contains("peak_db") && e.contains("ceiling_db"), "{e}");
    }

    #[test]
    fn a_missed_target_states_the_numbers_and_points_at_peak_db() {
        let d = train(json!({"type": "output", "name": "out", "input": "i", "lufs": -14}));
        let r = ok(&d);
        let w = has(&r, "output lufs").unwrap_or_else(|| panic!("{:?}", r.warnings));
        println!("{w}");
        for k in [
            "pre-limiter peak",
            "post-limiter peak",
            "crest",
            "limiter removed",
            "peak_db",
        ] {
            assert!(w.contains(k), "missing `{k}` in {w}");
        }
        assert!(w.contains("too short or too sparse"), "{w}");
    }

    #[test]
    fn heavy_fades_no_longer_cause_a_miss_the_target_is_met_after_them() {
        // 1 s of noise at -35 LUFS peaks about 20 dB under the ceiling; the 0.45 s
        // fades at both ends used to pull the loudness 6 LU under the target
        // (a warning blaming the fades). The target is now met after the fades.
        let out = json!({"type": "output", "name": "out", "input": "q", "lufs": -35,
        "fade_in": 0.45, "fade_out": 0.45});
        let r = ok(&noise_at(-30.0, out, 1.0));
        println!("final {:.2} LUFS, peak {:.1}", r.lufs, r.peak_db);
        assert!((r.lufs + 35.0).abs() < 0.5, "{}", r.lufs);
        assert!(has(&r, "output lufs").is_none(), "{:?}", r.warnings);
    }

    #[test]
    fn a_hot_miss_that_the_limiter_caused_says_so() {
        // A sine (crest 3 dB) at 0 LUFS needs peaks over 0 dBFS.
        let d = bare_doc(
            vec![
                json!({"type": "osc", "name": "s", "shape": "sine", "freq": 1000}),
                json!({"type": "output", "name": "out", "input": "s", "lufs": 0}),
            ],
            Some(1.0),
        );
        let r = ok(&d);
        let w = has(&r, "output lufs").unwrap_or_else(|| panic!("{:?}", r.warnings));
        println!("{w}");
        assert!(w.contains("the limiter took"), "{w}");
        assert!(w.contains("ceiling_db"), "{w}");
        assert!(!w.contains("too short"), "{w}");
    }

    #[test]
    fn a_target_that_is_met_has_no_warning() {
        let out = json!({"type": "output", "name": "out", "input": "q", "lufs": -20});
        let r = ok(&noise_at(-30.0, out, 1.0));
        println!("{:.2} LUFS, warnings {:?}", r.lufs, r.warnings);
        assert!((lufs_of(&r) + 20.0).abs() < 1.0);
        assert!(has(&r, "output lufs").is_none());
    }

    #[test]
    fn a_sound_at_minus_70_lufs_and_quieter_normalizes_up() {
        for from in [-45.0f32, -50.0, -58.0, -75.0, -90.0] {
            // Measure where the noise sits, then lift it to the wanted level.
            let probe = ok(&noise_at(
                0.0,
                json!({"type": "output", "name": "out", "input": "q", "fade_in": 0, "fade_out": 0, "ceiling_db": 20}),
                2.0,
            ));
            let db = from - probe.lufs;
            let quiet = ok(&noise_at(
                db,
                json!({"type": "output", "name": "out", "input": "q", "fade_in": 0, "fade_out": 0, "ceiling_db": 20}),
                2.0,
            ));
            let out = json!({"type": "output", "name": "out", "input": "q", "lufs": -20});
            let r = ok(&noise_at(db, out, 2.0));
            println!(
                "input {:.1} LUFS (asked {from}) -> output {:.2} LUFS, peak {:.1} dBFS",
                quiet.lufs, r.lufs, r.peak_db
            );
            assert!((r.lufs + 20.0).abs() < 1.0, "from {from}: got {}", r.lufs);
            assert!(
                r.warnings.iter().all(|w| !w.contains("output lufs")),
                "{:?}",
                r.warnings
            );
            assert_eq!(
                hash(&r),
                hash(&ok(&noise_at(
                    db,
                    json!({"type": "output", "name": "out", "input": "q", "lufs": -20}),
                    2.0
                )))
            );
        }
    }

    #[test]
    fn true_silence_stays_silent_and_finite() {
        let d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "gain", "name": "q", "input": "n", "gain": 0}),
                json!({"type": "output", "name": "out", "input": "q", "lufs": -20}),
            ],
            Some(0.5),
        );
        let r = ok(&d);
        assert!(
            r.audio
                .channels
                .iter()
                .flatten()
                .all(|x| x.is_finite() && *x == 0.0)
        );
    }

    #[test]
    fn extremes_stay_finite() {
        for out in [
            json!({"type": "output", "name": "out", "input": "q", "peak_db": -200}),
            json!({"type": "output", "name": "out", "input": "q", "peak_db": -1, "gain_db": 120}),
            json!({"type": "output", "name": "out", "input": "q", "lufs": -100}),
            json!({"type": "output", "name": "out", "input": "q", "lufs": 0, "ceiling_db": -30}),
        ] {
            for db in [-160.0f32, 0.0, 60.0] {
                let r = ok(&noise_at(db, out.clone(), 0.3));
                assert!(
                    r.audio.channels.iter().flatten().all(|x| x.is_finite()),
                    "{out} at {db}"
                );
            }
        }
    }

    #[test]
    fn the_normalize_node_also_lifts_a_quiet_sound() {
        let d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "gain", "name": "q", "input": "n", "db": -85}),
                json!({"type": "normalize", "name": "nm", "input": "q", "lufs": -20}),
                json!({"type": "output", "name": "out", "input": "nm", "fade_in": 0, "fade_out": 0}),
            ],
            Some(2.0),
        );
        let r = ok(&d);
        println!("normalize node: {:.2} LUFS", r.lufs);
        assert!((r.lufs + 20.0).abs() < 1.0, "{}", r.lufs);
    }

    fn click_at(t: f32, output: Value) -> Rendered {
        ok(&bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [t]}),
                json!({"type": "decay", "name": "e", "time": 0.05}),
                json!({"type": "noise", "name": "n"}),
                json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
                json!({"type": "math", "name": "hit", "op": "add", "a": "i", "b": "burst"}),
                output,
            ],
            Some(0.5),
        ))
    }

    #[test]
    fn a_click_at_zero_triggers_the_fade_in_warning_and_one_at_5ms_does_not() {
        let out = json!({"type": "output", "name": "out", "input": "hit"});
        // The decaying noise burst starts at 0 with a 2 ms fade: a click at 0.
        let r = click_at(0.0, out.clone());
        let w = has(&r, "fade-in").unwrap_or_else(|| panic!("{:?}", r.warnings));
        println!("{w}");
        assert!(
            w.contains("default 2 ms fade-in")
                && w.contains("fade_in 0.0005")
                && w.contains("silent_edges")
                && w.contains(" dB "),
            "{w}"
        );
        assert_eq!(
            r.warnings.iter().filter(|w| w.contains("fade-in")).count(),
            1
        );

        // Same graph with the noise burst delayed: use a pure impulse at 5 ms.
        let late = ok(&bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [0.005]}),
                json!({"type": "output", "name": "out", "input": "i"}),
            ],
            Some(0.2),
        ));
        assert!(has(&late, "fade-in").is_none(), "{:?}", late.warnings);
        let early = ok(&bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [0.0]}),
                json!({"type": "output", "name": "out", "input": "i"}),
            ],
            Some(0.2),
        ));
        let w = has(&early, "fade-in").unwrap_or_else(|| panic!("{:?}", early.warnings));
        println!("{w}");

        // fade_in 0 removes the cause; the default value is unchanged.
        let r = click_at(
            0.0,
            json!({"type": "output", "name": "out", "input": "hit", "fade_in": 0}),
        );
        assert!(has(&r, "fade-in").is_none(), "{:?}", r.warnings);
        let r = click_at(
            0.0,
            json!({"type": "output", "name": "out", "input": "hit", "fade_in": 0.004}),
        );
        let w = has(&r, "fade-in").unwrap();
        assert!(w.starts_with("the 4 ms"), "{w}");
    }

    #[test]
    fn a_loop_has_no_fade_in_to_warn_about() {
        let mut d = bare_doc(
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "output", "name": "out", "input": "n"}),
            ],
            Some(0.5),
        );
        d.kind = Kind::Loop;
        let r = ok(&d);
        assert!(has(&r, "fade-in").is_none(), "{:?}", r.warnings);
    }

    /// A noise burst that starts at full level at t = 0.
    fn burst_at_zero(output: Value) -> SoundDoc {
        bare_doc(
            vec![
                json!({"type": "decay", "name": "e", "time": 0.05}),
                json!({"type": "noise", "name": "n", "seed": 3}),
                json!({"type": "math", "name": "hit", "op": "mul", "a": "n", "b": "e"}),
                output,
            ],
            Some(0.3),
        )
    }

    #[test]
    fn peak_db_is_met_after_the_fades_for_a_hit_at_zero() {
        for fade_in in [Some(0.0005), None] {
            // ceiling_db 3: the test is the normalization to peak_db; the true-peak
            // limiter (default ceiling -1) would otherwise take noise's inter-sample
            // overs off the sample peak.
            let mut out = json!({"type": "output", "name": "out", "input": "hit", "peak_db": -1.5,
            "ceiling_db": 3});
            if let Some(f) = fade_in {
                out["fade_in"] = json!(f);
            }
            let r = ok(&burst_at_zero(out));
            println!("fade_in {fade_in:?}: final peak {:.3} dBFS", r.peak_db);
            assert!((r.peak_db + 1.5).abs() < 0.1, "{fade_in:?}: {}", r.peak_db);
        }
    }

    #[test]
    fn lufs_is_met_after_the_fades_for_a_hit_at_zero() {
        let out = json!({"type": "output", "name": "out", "input": "hit", "lufs": -20,
        "fade_in": 0.01, "ceiling_db": 3});
        let r = ok(&burst_at_zero(out));
        println!("final {:.2} LUFS", r.lufs);
        assert!((r.lufs + 20.0).abs() < 0.3, "{}", r.lufs);
    }

    #[test]
    fn a_hit_wholly_inside_the_fade_is_not_boosted_without_limit() {
        // A single impulse at sample 0 is zeroed by any fade-in: silence stays
        // silence, finite.
        let r = ok(&bare_doc(
            vec![
                json!({"type": "impulse", "name": "i", "at": [0.0]}),
                json!({"type": "output", "name": "out", "input": "i", "peak_db": -3}),
            ],
            Some(0.1),
        ));
        assert!(r.audio.channels.iter().flatten().all(|x| x.is_finite()));
    }

    #[test]
    fn the_squash_warning_is_silent_when_peak_db_normalizes_the_input() {
        let hot = |output: Value| {
            bare_doc(
                vec![
                    json!({"type": "noise", "name": "n", "seed": 4}),
                    json!({"type": "gain", "name": "q", "input": "n", "db": 12}),
                    output,
                ],
                Some(0.3),
            )
        };
        // Control: no target, a hot input, the limiter squashes it.
        let r = ok(&hot(json!({"type": "output", "name": "out", "input": "q"})));
        assert!(has(&r, "squashing").is_some(), "{:?}", r.warnings);
        // peak_db: the stage scales the input to the target; nothing is squashed.
        let r = ok(&hot(
            json!({"type": "output", "name": "out", "input": "q", "peak_db": -3, "ceiling_db": 3}),
        ));
        println!("peak {:.2}", r.peak_db);
        assert!((r.peak_db + 3.0).abs() < 0.1, "{}", r.peak_db);
        assert!(has(&r, "squashing").is_none(), "{:?}", r.warnings);
        // peak_db plus a gain_db that pushes it over the ceiling is still squashed.
        let r = ok(&hot(
            json!({"type": "output", "name": "out", "input": "q", "peak_db": -3, "gain_db": 6}),
        ));
        assert!(has(&r, "squashing").is_some(), "{:?}", r.warnings);
    }

    fn brightness_macro(node: &str) -> sfx_graph::Macro {
        sfx_graph::Macro {
            name: "brightness".into(),
            value: 0.5,
            default: None,
            doc: String::new(),
            claim: None,
            game: false,
            map: vec![sfx_graph::MapEntry {
                node: node.into(),
                field: "freq".into(),
                param: None,
                from: 500.0,
                to: 5000.0,
                curve: sfx_graph::MapCurve::Exp,
                col: None,
                index: None,
                key: None,
            }],
        }
    }

    fn lp_nodes() -> Vec<Value> {
        vec![
            json!({"type": "noise", "name": "n"}),
            json!({"type": "filter", "name": "lp", "input": "n", "mode": "lp", "freq": 1000}),
            json!({"type": "output", "name": "out", "input": "lp", "lufs": -20}),
        ]
    }

    fn edit_macro(m: sfx_graph::Macro) -> sfx_graph::DocEdit {
        sfx_graph::DocEdit {
            macros: Some(vec![m]),
            ..sfx_graph::DocEdit::default()
        }
    }

    #[test]
    fn no_driven_by_macro_warning_when_node_and_macro_are_made_together() {
        let mut d = SoundDoc::named("t");
        d.duration = Some(0.2);
        let rep = d
            .apply(
                lp_nodes(),
                edit_macro(brightness_macro("lp")),
                &RenderOpts::default(),
            )
            .unwrap();
        assert!(!rep.text.contains("driven by macro"), "{}", rep.text);
        // The same macro already there, then a later edit of the driven field:
        // the warning is back.
        let mut lp = lp_nodes()[1].clone();
        lp["freq"] = json!(2000);
        let rep = d
            .apply(
                vec![lp],
                sfx_graph::DocEdit::default(),
                &RenderOpts::default(),
            )
            .unwrap();
        assert!(
            rep.text
                .contains("node `lp` field `freq` is driven by macro `brightness`"),
            "{}",
            rep.text
        );
    }

    #[test]
    fn a_macro_naming_a_node_inside_a_voices_patch_points_at_param() {
        let mut d = SoundDoc::named("t");
        d.duration = Some(0.5);
        let nodes = vec![
            json!({"type": "pattern", "name": "ev", "times": [0.05, 0.2]}),
            json!({"type": "voices", "name": "v", "events": "ev", "length": 0.2,
               "patch": [
                   {"type": "osc", "name": "o", "freq": 400, "amp": 0.5},
                   {"type": "decay", "name": "e", "time": 0.08},
                   {"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}]}),
            json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
        ];
        let err = d
            .apply(
                nodes.clone(),
                edit_macro(brightness_macro("o")),
                &RenderOpts::default(),
            )
            .unwrap_err();
        println!("{err}");
        assert!(
            err.contains("node `o` is inside the voices patch of `v`")
                && err.contains("sound_ops params")
                && err.contains("\"param\""),
            "{err}"
        );
        // A node that exists nowhere keeps the old error.
        let err = d
            .apply(
                nodes,
                edit_macro(brightness_macro("nope")),
                &RenderOpts::default(),
            )
            .unwrap_err();
        assert!(err.contains("no node named `nope`"), "{err}");
    }

    // ---------------------------------------------------------------- R2-B

    fn true_peak_db(r: &Rendered) -> f32 {
        sfx_graph::gain_to_db(sfx_graph::loudness::true_peak(&r.audio.channels))
    }

    fn crest_db(r: &Rendered) -> f32 {
        let x = &r.audio.channels[0];
        let rms = (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        20.0 * (f64::from(r.audio.peak()) / rms).log10() as f32
    }

    #[test]
    fn fade_in_warning_is_silent_for_a_short_fade_or_a_met_target() {
        let hit = |extra: Value| {
            let mut o = json!({"type": "output", "name": "out", "input": "hit"});
            for (k, v) in extra.as_object().unwrap() {
                o[k] = v.clone();
            }
            click_at(0.0, o)
        };
        // No target, default 2 ms: still warns (and with a 4 ms fade).
        let r = hit(json!({}));
        assert!(has(&r, "fade-in").is_some(), "{:?}", r.warnings);
        let r = hit(json!({"fade_in": 0.004}));
        assert!(has(&r, "fade-in").is_some(), "{:?}", r.warnings);
        // 1 ms and shorter: never.
        for f in [0.001, 0.0005, 0.0002] {
            let r = hit(json!({"fade_in": f}));
            assert!(has(&r, "fade-in").is_none(), "{f}: {:?}", r.warnings);
        }
        // A met target: the delivered level is right, whatever the fade cut.
        let r = hit(json!({"peak_db": -3}));
        println!("peak_db -3: peak {:.2}", r.peak_db);
        assert!(has(&r, "fade-in").is_none(), "{:?}", r.warnings);
        let r = hit(json!({"lufs": -30, "fade_in": 0.004}));
        println!("lufs -30: {:.2}", r.lufs);
        assert!((r.lufs + 30.0).abs() < 1.0, "{}", r.lufs);
        assert!(has(&r, "fade-in").is_none(), "{:?}", r.warnings);
        // A target that is not met (gain_db pushes the peak into the ceiling
        // limiter) does not hide a real cut.
        let r = hit(json!({"peak_db": -3, "gain_db": 6, "fade_in": 0.004}));
        println!("peak_db -3 + 6 dB: peak {:.2}", r.peak_db);
        assert!(has(&r, "fade-in").is_some(), "{:?}", r.warnings);
    }

    #[test]
    fn a_sine_at_the_ceiling_ends_at_a_true_peak_under_it() {
        for (freq, phase) in [(1000.0, 0.0), (12000.0, 0.125), (11025.0, 0.1)] {
            for ceiling in [-1.0f32, -6.0] {
                // Amplitude 1 dB over the ceiling: the limiter must act.
                let amp = 10f32.powf((ceiling + 1.0) / 20.0);
                let r = ok(&bare_doc(
                    vec![
                        json!({"type": "osc", "name": "s", "freq": freq, "phase": phase, "amp": amp}),
                        json!({"type": "output", "name": "out", "input": "s", "fade_in": 0,
                        "fade_out": 0, "ceiling_db": ceiling}),
                    ],
                    Some(0.5),
                ));
                let tp = true_peak_db(&r);
                println!(
                    "{freq} Hz ceiling {ceiling}: sample {:.3} true {tp:.3}",
                    r.peak_db
                );
                assert!(tp <= ceiling + 0.05, "{freq} Hz: true peak {tp}");
                assert!(tp >= ceiling - 1.5, "{freq} Hz: over-limited to {tp}");
            }
        }
        // Exactly at the ceiling (amplitude = ceiling): the same bound.
        let amp = 10f32.powf(-1.0 / 20.0);
        let r = ok(&bare_doc(
            vec![
                json!({"type": "osc", "name": "s", "freq": 1000, "amp": amp}),
                json!({"type": "output", "name": "out", "input": "s", "fade_in": 0, "fade_out": 0}),
            ],
            Some(0.5),
        ));
        let tp = true_peak_db(&r);
        println!("1 kHz at the ceiling: true {tp:.3}");
        assert!(tp <= -0.95, "{tp}");
    }

    /// Noise bursts, 1 % of every 0.2 s at full level over a floor 22 dB down.
    fn burst_train(output: Value) -> SoundDoc {
        bare_doc(
            vec![
                json!({"type": "noise", "name": "n", "seed": 9}),
                json!({"type": "expr", "name": "env", "inputs": {}, "state": {},
                "out": "fract(t * 5) < 0.01 ? 1 : 0.02"}),
                json!({"type": "math", "name": "train", "op": "mul", "a": "n", "b": "env"}),
                output,
            ],
            Some(2.0),
        )
    }

    #[test]
    fn a_noise_burst_train_ends_at_a_true_peak_under_the_ceiling() {
        for out in [
            json!({"type": "output", "name": "out", "input": "train", "gain_db": 12}),
            json!({"type": "output", "name": "out", "input": "train", "peak_db": -1, "ceiling_db": -1}),
            json!({"type": "output", "name": "out", "input": "train", "lufs": -14}),
            json!({"type": "output", "name": "out", "input": "train", "gain_db": 12,
            "ceiling_db": -6}),
        ] {
            let ceiling = out["ceiling_db"].as_f64().unwrap_or(-1.0) as f32;
            let r = ok(&burst_train(out.clone()));
            let (tp, crest) = (true_peak_db(&r), crest_db(&r));
            println!(
                "{out}: sample {:.3} true {tp:.3} crest {crest:.1} dB",
                r.peak_db
            );
            assert!(tp <= ceiling + 0.05, "{out}: true peak {tp}");
            assert!(r.audio.channels.iter().flatten().all(|x| x.is_finite()));
        }
        let r = ok(&burst_train(
            json!({"type": "output", "name": "out", "input": "train", "gain_db": 12}),
        ));
        let crest = crest_db(&r);
        assert!((18.0..=30.0).contains(&crest), "crest {crest}");
    }

    #[test]
    fn a_stereo_sound_is_limited_on_the_true_peak_of_either_channel() {
        let r = ok(&bare_doc(
            vec![
                json!({"type": "osc", "name": "s", "freq": 12000, "phase": 0.125, "amp": 1.2}),
                json!({"type": "pan", "name": "p", "input": "s", "pan": 0.5}),
                json!({"type": "output", "name": "out", "input": "p", "fade_in": 0, "fade_out": 0}),
            ],
            Some(0.3),
        ));
        assert_eq!(r.audio.channels.len(), 2);
        assert!(true_peak_db(&r) <= -0.95, "{}", true_peak_db(&r));
    }

    #[test]
    fn a_true_peak_under_the_ceiling_is_bit_identical_whatever_the_ceiling() {
        // -6 dBFS amplitude: under both ceilings (and the sample-peak limiter did
        // not touch it either), so the limiter is a pass-through.
        let mk = |ceiling: f32| {
            hash(&ok(&bare_doc(
                vec![
                    json!({"type": "noise", "name": "n", "seed": 5}),
                    json!({"type": "gain", "name": "q", "input": "n", "db": -12}),
                    json!({"type": "output", "name": "out", "input": "q", "ceiling_db": ceiling}),
                ],
                Some(0.3),
            )))
        };
        assert_eq!(mk(-1.0), mk(6.0));
    }
}

mod variations {
    //! `variations` compares what the output stage normalizes (sample peak
    //! for `peak_db`, loudness for `lufs`), and a wrong shape for a check that
    //! takes `true` or settings names the check and its fields.

    use serde_json::{Value, json};
    use sfx_analysis::checks::Expect;
    use sfx_analysis::{CheckCx, CheckResult, run};
    use sfx_graph::{RenderOpts, SoundDoc};

    /// A footstep-like seeded hit: a noise burst whose length and filter move with
    /// the seed (so its loudness varies a lot), then an output stage.
    fn hit(output: Value) -> SoundDoc {
        let mut d = SoundDoc::named("step");
        d.nodes = vec![
            json!({"type": "noise", "name": "n", "color": "pink"}),
            json!({"type": "wander", "name": "w", "rate": 30, "range": [300, 3000]}),
            json!({"type": "filter", "name": "f", "input": "n", "mode": "bp", "freq": "w", "q": 2}),
            json!({"type": "decay", "name": "e", "time": 0.03}),
            json!({"type": "math", "name": "m", "op": "mul", "a": "f", "b": "e"}),
            output,
        ];
        d.duration = Some(0.3);
        d
    }

    fn verify(d: &SoundDoc, e: Value) -> Vec<CheckResult> {
        let e: Expect = serde_json::from_value(e).unwrap_or_else(|e| panic!("{e}"));
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

    #[test]
    fn peak_db_output_compares_the_peak_and_passes() {
        let d = hit(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
        let v = verify(&d, json!({"variations": {"seeds": 6}}));
        println!("{}", v[0].detail);
        assert!(v[0].pass, "{:?}", v[0]);
        assert!(
            v[0].detail.contains("peak spread") && v[0].detail.contains("reported only"),
            "{:?}",
            v[0]
        );
    }

    #[test]
    fn peak_db_output_fails_on_peak_spread_with_text_saying_what_was_compared() {
        // A negative limit cannot be met: the message names the peak comparison.
        let d = hit(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
        let v = verify(
            &d,
            json!({"variations": {"seeds": 4, "peak_spread_db": -1}}),
        );
        println!("{}", v[0].detail);
        assert!(!v[0].pass, "{:?}", v[0]);
        assert!(
            v[0].detail.contains("peak was compared") && v[0].detail.contains("peak_spread_db"),
            "{:?}",
            v[0]
        );
    }

    #[test]
    fn lufs_output_keeps_the_loudness_limit() {
        let d = hit(json!({"type": "output", "name": "out", "input": "m", "lufs": -20}));
        let v = verify(&d, json!({"variations": {"seeds": 4, "lufs_spread": -1}}));
        println!("{}", v[0].detail);
        assert!(!v[0].pass, "{:?}", v[0]);
        assert!(
            v[0].detail.contains("loudness spread") && v[0].detail.contains("lufs_spread"),
            "{:?}",
            v[0]
        );
        assert!(!v[0].detail.contains("peak spread"), "{:?}", v[0]);
        // The old LU limit, unchanged (this hit spreads about 3 LU).
        let v = verify(&d, json!({"variations": {"seeds": 4, "lufs_spread": 4}}));
        assert!(v[0].pass, "{:?}", v[0]);
        // No target at all: the LU limit too.
        let d = hit(json!({"type": "output", "name": "out", "input": "m"}));
        let v = verify(&d, json!({"variations": true}));
        assert!(v[0].detail.contains("loudness spread"), "{:?}", v[0]);
    }

    #[test]
    fn a_bad_shape_names_the_check_and_its_fields() {
        for (check, bad, field) in [
            ("variations", json!({"count": 4}), "count"),
            ("variations", json!(4), "expected true or {seeds"),
            ("dc", json!({"min": 1}), "min"),
            ("aliasing", json!("yes"), "expected true or {max_db}"),
        ] {
            let e = serde_json::from_value::<Expect>(json!({ check: bad })).unwrap_err();
            let m = e.to_string();
            println!("{m}");
            assert!(
                m.starts_with(&format!("{check}: expected true or {{")) && m.contains(field),
                "{m}"
            );
            assert!(!m.contains("untagged enum"), "{m}");
        }
    }

    #[test]
    fn toggles_still_parse_in_every_good_form() {
        let e: Expect = serde_json::from_value(
            json!({"variations": {"seeds": 4}, "dc": true, "aliasing": {"max_db": -30}}),
        )
        .unwrap();
        assert!(e.variations.is_some() && e.dc.is_some() && e.aliasing.is_some());
        let e: Expect = serde_json::from_value(json!({"dc": false})).unwrap();
        assert!(e.dc.is_some());
    }
}
