mod compare {
    //! Reference compare: alignment, loudness matching, the distance, the
    //! words, the difference image and the `reference` check.

    use serde_json::{Value, json};
    use sfx_analysis::image::MAX_PIXELS;
    use sfx_analysis::{
        CheckCx, Column, DiffColumn, Expect, View, compare, listen_png, listen_png_diff, run,
    };
    use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

    /// A struck object: five modes plus a noise tick.
    fn struck(modes: Value, lp: Option<f64>, duration: f32) -> Audio {
        let mut d = SoundDoc::named("t");
        let src = if lp.is_some() { "lp" } else { "mixed" };
        d.nodes = vec![
            json!({"type": "impulse", "name": "hit"}),
            json!({"type": "resonators", "name": "body", "input": "hit", "modes": modes}),
            json!({"type": "noise", "name": "n", "seed": 3}),
            json!({"type": "decay", "name": "e", "time": 0.004}),
            json!({"type": "math", "name": "tick", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "gain", "name": "tickg", "input": "tick", "gain": 0.05}),
            json!({"type": "mix", "name": "mixed", "inputs": ["body", "tickg"]}),
        ];
        if let Some(f) = lp {
            d.nodes.push(json!({"type": "filter", "name": "lp", "input": "mixed", "mode": "lp", "freq": f, "q": 0.7}));
        }
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": src, "lufs": -20}));
        d.duration = Some(duration);
        d.render(&RenderOpts::default()).unwrap().audio
    }

    fn modes() -> Value {
        json!([
            [300, 0.25, 1.0],
            [870, 0.2, 0.6],
            [1700, 0.15, 0.5],
            [3200, 0.1, 0.4],
            [5200, 0.08, 0.3]
        ])
    }

    fn base() -> Audio {
        struck(modes(), None, 0.8)
    }

    fn scaled(a: &Audio, db: f32) -> Audio {
        let g = 10f32.powf(db / 20.0);
        Audio {
            sample_rate: a.sample_rate,
            channels: a
                .channels
                .iter()
                .map(|c| c.iter().map(|v| v * g).collect())
                .collect(),
        }
    }

    fn delayed(a: &Audio, secs: f32) -> Audio {
        let n = (secs * a.sample_rate as f32) as usize;
        Audio {
            sample_rate: a.sample_rate,
            channels: a
                .channels
                .iter()
                .map(|c| {
                    let mut v = vec![0.0; n];
                    v.extend_from_slice(c);
                    v
                })
                .collect(),
        }
    }

    #[test]
    fn identical_sounds_are_at_zero_distance_with_no_deviations() {
        let a = base();
        let c = compare(&a, &a);
        assert!(c.distance_db < 0.01, "distance {}", c.distance_db);
        assert!(c.deviations.is_empty(), "{:?}", c.deviations);
        assert!(c.shift_s.abs() < 1e-4 && c.gain_db.abs() < 0.05);
        assert!(c.notes.is_empty(), "{:?}", c.notes);
    }

    #[test]
    fn a_quieter_copy_matches_after_loudness_matching_and_reports_the_gain() {
        let a = base();
        let c = compare(&a, &scaled(&a, -6.0));
        assert!(c.distance_db < 0.2, "distance {}", c.distance_db);
        assert!((c.gain_db - 6.0).abs() < 0.3, "gain {}", c.gain_db);
        assert!(c.deviations.is_empty(), "{:?}", c.deviations);
        assert!(
            c.text().contains("+6.0 dB")
                || c.text().contains("+5.9 dB")
                || c.text().contains("+6.1 dB")
        );
    }

    #[test]
    fn a_late_copy_is_aligned_by_its_onset() {
        let a = base();
        let late = delayed(&a, 0.1);
        let c = compare(&a, &late);
        assert!((c.shift_s - 0.1).abs() < 0.002, "shift {}", c.shift_s);
        assert!(c.distance_db < 0.5, "distance {}", c.distance_db);
        let early = compare(&late, &a);
        assert!(
            (early.shift_s + 0.1).abs() < 0.002,
            "shift {}",
            early.shift_s
        );
        assert!(early.text().contains("moved later"), "{}", early.text());
    }

    #[test]
    fn a_darker_sound_is_quieter_in_the_high_bands_and_the_words_name_them() {
        let bright = base();
        let dark = struck(modes(), Some(500.0), 0.8);
        let c = compare(&dark, &bright);
        assert!(c.distance_db > 3.0, "distance {}", c.distance_db);
        assert!(!c.deviations.is_empty());
        let high_quieter = c
            .deviations
            .iter()
            .filter(|d| d.band >= 5 && d.diff_db < 0.0)
            .count();
        assert!(high_quieter >= 2, "{:#?}", c.deviations);
        assert!(c.deviations[0].band >= 5, "{:#?}", c.deviations);
        let text = c.text();
        assert!(
            text.contains("0-20% of the sound:")
                && text.contains("kHz is")
                && text.contains("quieter than the reference"),
            "{text}"
        );
        assert!(
            text.contains("centroid") && text.contains("darker"),
            "{text}"
        );
        assert!(c.deviations.len() <= 5);
        // The brighter sound against the dark reference is the mirror image.
        let m = compare(&bright, &dark);
        assert!(
            m.text().contains("louder than the reference"),
            "{}",
            m.text()
        );
    }

    #[test]
    fn a_longer_ringing_reference_is_reported_as_longer() {
        let short = struck(
            json!([[300, 0.1, 1.0], [870, 0.08, 0.6], [1700, 0.06, 0.5]]),
            None,
            1.0,
        );
        let long = struck(
            json!([[300, 0.6, 1.0], [870, 0.5, 0.6], [1700, 0.4, 0.5]]),
            None,
            1.0,
        );
        let c = compare(&short, &long);
        let text = c.text();
        assert!(
            c.notes
                .iter()
                .any(|n| n.contains("the reference is") && n.contains("longer")),
            "{text}"
        );
        assert!(text.contains("decay"), "{text}");
        assert!(c.distance_db > 2.0, "distance {}", c.distance_db);
    }

    #[test]
    fn another_sample_rate_and_mono_against_stereo_are_handled() {
        let a = base();
        // The same sound at 44.1 kHz, as a stereo file.
        let r = sfx_analysis::reference::resample(&a, 44_100);
        assert_eq!(r.sample_rate, 44_100);
        assert!((r.len() as f32 - a.len() as f32 * 44_100.0 / 48_000.0).abs() < 2.0);
        let stereo = Audio {
            sample_rate: 44_100,
            channels: vec![r.channels[0].clone(), r.channels[0].clone()],
        };
        let c = compare(&a, &stereo);
        // Not zero: the 44.1 kHz copy has nothing above 22 kHz, where the
        // original still has a little noise (the note says so).
        let far = compare(&a, &struck(modes(), Some(500.0), 0.8)).distance_db;
        assert!(
            c.distance_db < 4.0 && c.distance_db < 0.5 * far,
            "distance {} vs {far} for a darker sound",
            c.distance_db
        );
        assert!(
            c.notes.iter().any(|n| n.contains("nothing above 22.0 kHz")),
            "{:?}",
            c.notes
        );
        assert!(
            c.notes.iter().any(|n| n.contains("resampled")),
            "{:?}",
            c.notes
        );
        assert!(c.notes.iter().any(|n| n.contains("mono")), "{:?}", c.notes);
    }

    #[test]
    fn the_image_has_three_columns_inside_the_pixel_budget() {
        let a = base();
        let dark = struck(modes(), Some(500.0), 0.8);
        let c = compare(&dark, &a);
        let cols = [
            Column {
                label: "sound".into(),
                audio: &dark,
            },
            Column {
                label: "reference".into(),
                audio: &c.aligned,
            },
        ];
        let d = DiffColumn {
            label: "sound minus reference (dB)".into(),
            a: &c.padded,
            b: &c.aligned,
        };
        let png = listen_png_diff(&cols, Some(&d), View::default()).unwrap();
        if let Ok(path) = std::env::var("SOUNDER_SAVE_PNG") {
            std::fs::write(path, &png).unwrap();
        }
        let (w, h) = png_size(&png);
        assert!(w * h <= MAX_PIXELS, "{w}x{h}");
        assert!(w % 3 == 0 && w / 3 >= 240, "three columns of {}", w / 3);
        let one = png_size(&listen_png(&cols[..1], View::default()).unwrap());
        assert!(one.0 < w, "one column {} px, three {w} px", one.0);
        // Blue (quieter) and red (louder) both show up in the difference column.
        let px = decode(&png);
        let third = (w / 3 * 2)..w;
        let (mut blue, mut red) = (0, 0);
        for y in 0..h {
            for x in third.clone() {
                let i = (y * w + x) * 4;
                let (r, g, b) = (px[i], px[i + 1], px[i + 2]);
                if b > 200 && r < 80 && g < 150 {
                    blue += 1;
                }
                if r > 200 && g < 120 && b < 80 {
                    red += 1;
                }
            }
        }
        assert!(blue > 500 && red > 50, "blue {blue}, red {red}");
    }

    fn decode(png: &[u8]) -> Vec<u8> {
        let mut r = png::Decoder::new(std::io::BufReader::new(std::io::Cursor::new(png)))
            .read_info()
            .unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        buf
    }

    fn png_size(png: &[u8]) -> (usize, usize) {
        let r = png::Decoder::new(std::io::BufReader::new(std::io::Cursor::new(png)))
            .read_info()
            .unwrap();
        let i = r.info();
        (i.width as usize, i.height as usize)
    }

    fn check_reference(
        doc: &SoundDoc,
        dir: &std::path::Path,
        file: &str,
        max: f32,
    ) -> Vec<sfx_analysis::CheckResult> {
        let opts = RenderOpts {
            base_dir: Some(dir.to_path_buf()),
            ..RenderOpts::default()
        };
        let r = doc.render(&opts).unwrap();
        let e: Expect =
            serde_json::from_value(json!({"reference": {"file": file, "max_distance": max}}))
                .unwrap();
        run(
            &e,
            &CheckCx {
                doc,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap_or_else(|e| {
            vec![sfx_analysis::CheckResult {
                name: "error".into(),
                pass: false,
                detail: e,
            }]
        })
    }

    #[test]
    fn the_reference_check_passes_close_and_fails_far_with_words() {
        let dir = std::env::temp_dir().join(format!("sounder_reftest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dark = struck(modes(), Some(500.0), 0.8);
        wav::write(&dir.join("dark.wav"), &dark, wav::Bits::Float32).unwrap();
        let mut doc = SoundDoc::named("t");
        doc.nodes = vec![
            json!({"type": "impulse", "name": "hit"}),
            json!({"type": "resonators", "name": "body", "input": "hit", "modes": modes()}),
            json!({"type": "output", "name": "out", "input": "body", "lufs": -20}),
        ];
        doc.duration = Some(0.8);
        // Against a file of its own render: relative path, resolved on base_dir.
        let own = doc.render(&RenderOpts::default()).unwrap().audio;
        wav::write(&dir.join("own.wav"), &own, wav::Bits::Float32).unwrap();
        let ok = check_reference(&doc, &dir, "own.wav", 1.0);
        assert!(ok[0].pass, "{}", ok[0].detail);
        assert!(ok[0].detail.contains("own.wav"), "{}", ok[0].detail);
        // Against the dark one: far, and the FAIL says where.
        let bad = check_reference(&doc, &dir, "dark.wav", 1.0);
        assert!(!bad[0].pass, "{}", bad[0].detail);
        assert!(
            bad[0].detail.contains("max 1") && bad[0].detail.contains("louder than the reference"),
            "{}",
            bad[0].detail
        );
        // A missing file is an error naming it.
        let missing = check_reference(&doc, &dir, "nope.wav", 1.0);
        assert!(
            !missing[0].pass && missing[0].detail.contains("nope.wav"),
            "{}",
            missing[0].detail
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Renders an `impulse` into `node` (a resonators node reading `hit`) with
    /// no loudness target: whatever level the gains give.
    fn rebuild(node: &Value, duration: f32) -> Audio {
        let mut d = SoundDoc::named("rebuilt");
        d.nodes = vec![
            json!({"type": "impulse", "name": "hit"}),
            node.clone(),
            json!({"type": "output", "name": "out", "input": "modes"}),
        ];
        d.duration = Some(duration);
        d.render(&RenderOpts::default()).unwrap().audio
    }

    fn shifted_modes(node: &Value, factor: f64) -> Value {
        let mut n = node.clone();
        for m in n["modes"].as_array_mut().unwrap() {
            m[0] = json!(m[0].as_f64().unwrap() * factor);
        }
        n
    }

    fn rebuild_distances(modes: Value, count: usize) -> (f32, f32, f32, Value) {
        let original = struck(modes, None, 0.8);
        let node = sfx_analysis::modes_node(&original, count, "modes", "hit");
        let good = compare(&rebuild(&node, 0.8), &original);
        let bad = compare(&rebuild(&shifted_modes(&node, 1.10), 0.8), &original);
        (good.distance_db, good.gain_db, bad.distance_db, node)
    }

    #[test]
    fn extract_modes_rebuilds_a_struck_object_close_to_its_own_render() {
        // A hit with five well-spaced modes and a noise tick.
        let (good, gain, bad, node) = rebuild_distances(modes(), 5);
        eprintln!("easy: {good:.2} dB, gain {gain:+.2} dB, shifted 10%: {bad:.2} dB\n{node}");
        // Measured 0.69 dB (a plain peak-pick of the old extractor: 3.77 dB, and
        // a +2.1 dB loudness error); the shifted rebuild 13.6 dB.
        assert!(good < 1.2, "rebuilt distance {good}");
        assert!(gain.abs() < 0.5, "the rebuilt level is off by {gain} dB");
        assert!(bad > 4.0 * good && bad > 8.0, "shifted: {bad} vs {good}");
    }

    #[test]
    fn extract_modes_handles_fast_decays_and_close_pairs() {
        // 30 ms and 20 ms modes, and a pair 25 Hz apart.
        let hard = json!([
            [400, 0.03, 1.0],
            [1000, 0.3, 0.8],
            [1025, 0.3, 0.6],
            [2500, 0.05, 0.5],
            [7000, 0.02, 0.4]
        ]);
        let (good, gain, bad, node) = rebuild_distances(hard, 5);
        // Measured 1.21 dB, level +0.06 dB, shifted rebuild 13.3 dB.
        assert!(good < 2.0, "rebuilt distance {good}");
        assert!(gain.abs() < 0.5, "the rebuilt level is off by {gain} dB");
        assert!(bad > 4.0 * good, "shifted: {bad} vs {good}");
        let rows: Vec<Vec<f64>> = node["modes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                m.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), 5, "{node}");
        for (row, hz) in rows.iter().zip([400.0, 1000.0, 1025.0, 2500.0, 7000.0]) {
            assert!(
                (row[0] - hz).abs() < 3.0,
                "mode at {} Hz, wanted {hz}: {node}",
                row[0]
            );
        }
        // The 30 ms mode reads 30 ms (not smeared by the analysis window).
        assert!((rows[0][1] - 0.03).abs() < 0.01, "decay {}", rows[0][1]);
    }

    /// Modes plus the residual (`residual_after`) rebuild the recording
    /// closer than the modes alone, most of all when the modes miss part of it.
    /// Both renders use the same output (`fade_in` 0.0005: this recording's hit
    /// starts 15 samples in, so the default 2 ms fade would cut it).
    #[test]
    fn the_residual_restores_what_the_modes_miss() {
        let dir = std::env::temp_dir().join(format!("sounder_residual_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = struck(modes(), None, 0.8);
        let render = |nodes: Vec<Value>, out: &str| {
            let mut d = SoundDoc::named("rebuilt");
            d.nodes = nodes;
            d.nodes
                .push(json!({"type": "output", "name": "out", "input": out, "fade_in": 0.0005}));
            d.duration = Some(0.8);
            // The residual sits in a sources folder, found by file name.
            d.render(&RenderOpts {
                sources: vec![dir.clone()],
                ..RenderOpts::default()
            })
            .unwrap()
            .audio
        };
        for count in [5, 2] {
            let node = sfx_analysis::modes_node(&original, count, "modes", "hit");
            let (residual, at) = sfx_analysis::modal::residual_after(&original, &node).unwrap();
            let file = format!("residual_{count}.wav");
            wav::write(
                &dir.join(&file),
                &Audio {
                    sample_rate: original.sample_rate,
                    channels: vec![residual],
                },
                wav::Bits::Float32,
            )
            .unwrap();
            let hit = json!({"type": "impulse", "name": "hit", "at": at});
            let only =
                compare(&render(vec![hit.clone(), node.clone()], "modes"), &original).distance_db;
            let both = render(
                vec![
                    hit,
                    node.clone(),
                    json!({"type": "sample", "name": "residual", "file": file}),
                    json!({"type": "mix", "name": "rebuild", "inputs": ["modes", "residual"]}),
                ],
                "rebuild",
            );
            let with = compare(&both, &original).distance_db;
            eprintln!("{count} modes: alone {only:.2} dB, with the residual {with:.2} dB");
            assert!(with < only, "{count} modes: {with} vs {only}");
            assert!(
                with < 0.5,
                "{count} modes: modes + residual {with} dB from the recording"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A noisy, non-modal hit (band-passed noise under a decay) peels into
    /// partials that cancel each other (they played 85 dB over the recording
    /// before the fallback). `extract_fit` detects it and returns an envelope fit
    /// levelled to the recording, whose residual rebuilds it; a clean struck
    /// object keeps the modal fit.
    #[test]
    fn a_non_modal_recording_gets_an_envelope_fit() {
        use sfx_analysis::modal::{FitKind, extract_fit, modes_over_db, residual_after};
        let mut d = SoundDoc::named("noisy");
        d.nodes = vec![
            json!({"type": "noise", "name": "n", "color": "white", "seed": 3}),
            json!({"type": "decay", "name": "e", "time": 0.8, "at": 0.01}),
            json!({"type": "math", "name": "b", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "filter", "name": "f", "input": "b", "mode": "bp", "freq": 300, "q": 4}),
            json!({"type": "output", "name": "out", "input": "f", "peak_db": -1}),
        ];
        d.duration = Some(1.0);
        let noisy = d.render(&RenderOpts::default()).unwrap().audio;
        let (fit, kind) = extract_fit(&noisy, 8);
        let node = sfx_analysis::modes_node_from(&fit, "modes", "hit");
        let over = modes_over_db(&noisy, &node).unwrap();
        eprintln!("noisy hit: {kind:?}, modes play {over:.1} dB over the recording\n{node}");
        assert_eq!(kind, FitKind::Envelope);
        assert!(over.abs() < 1.0, "{over}");
        // Peaks spaced at least 15 % apart.
        for w in fit.windows(2) {
            assert!(w[1].hz >= 1.14 * w[0].hz, "{} then {}", w[0].hz, w[1].hz);
        }
        // The residual exists and the two rebuild the recording.
        let (residual, at) = residual_after(&noisy, &node).unwrap();
        let rec_peak = noisy.peak();
        let res_peak = residual.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        eprintln!(
            "noisy hit: residual peak {res_peak:.3} against the recording's {rec_peak:.3}, impulse at {at} s"
        );
        assert!(res_peak < 2.0 * rec_peak);
        // The clean struck object keeps the modal fit.
        let clean = struck(modes(), None, 0.8);
        let (_, k2) = extract_fit(&clean, 5);
        let n2 = sfx_analysis::modes_node(&clean, 5, "modes", "hit");
        let o2 = modes_over_db(&clean, &n2).unwrap();
        eprintln!("clean hit: {k2:?}, modes play {o2:.1} dB over the recording");
        assert_eq!(k2, FitKind::Modal);
        assert!(o2 <= sfx_analysis::modal::MODES_OVER_DB, "{o2}");
    }
}

mod sets {
    //! Reference sets (`refs/<set>/`): a sound in the family lands inside every
    //! feature's span; a tonal ring is outside (voiced, ringing); the verify check
    //! reports per feature; a missing or one-file set is an error that helps.

    use serde_json::{Value, json};
    use sfx_analysis::refset::{compare_set, load_set};
    use sfx_analysis::{CheckCx, Expect, run};
    use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

    /// A noise hit band-passed at `hz` with a `decay` s body and a low thump.
    fn hit(hz: f32, decay: f32, seed: u64) -> Vec<Value> {
        vec![
            json!({"type": "noise", "name": "n", "color": "pink", "seed": seed}),
            json!({"type": "decay", "name": "e", "time": decay, "at": 0.01}),
            json!({"type": "math", "name": "b", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "filter", "name": "body", "input": "b", "mode": "bp", "freq": hz, "q": 0.7}),
            json!({"type": "noise", "name": "w", "color": "brown", "seed": seed + 10}),
            json!({"type": "decay", "name": "we", "time": decay * 0.7, "at": 0.01}),
            json!({"type": "math", "name": "wb", "op": "mul", "a": "w", "b": "we"}),
            json!({"type": "filter", "name": "thump", "input": "wb", "mode": "lp", "freq": 160, "slope": 24}),
            json!({"type": "mix", "name": "all", "inputs": ["body", {"input": "thump", "gain": 0.6}]}),
            json!({"type": "output", "name": "out", "input": "all", "peak_db": -3}),
        ]
    }

    fn doc(nodes: Vec<Value>) -> SoundDoc {
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.duration = Some(1.0);
        d
    }

    fn render(nodes: Vec<Value>) -> Audio {
        doc(nodes).render(&RenderOpts::default()).unwrap().audio
    }

    fn project(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("sounder_refset_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let set = p.join("refs").join("hits");
        std::fs::create_dir_all(&set).unwrap();
        for (i, (hz, decay)) in [(280.0, 0.5), (330.0, 0.6), (380.0, 0.7)]
            .iter()
            .enumerate()
        {
            let a = render(hit(*hz, *decay, i as u64 + 1));
            wav::write(&set.join(format!("hit_{i}.wav")), &a, wav::Bits::Float32).unwrap();
        }
        p
    }

    #[test]
    fn a_family_member_is_inside_and_a_ringing_tone_is_outside() {
        let p = project("family");
        let members = load_set("hits", Some(&p)).unwrap();
        assert_eq!(members.len(), 3);
        let inside = compare_set(&render(hit(310.0, 0.55, 7)), "hits", &members);
        println!("{}", inside.text());
        assert!(inside.failing().is_empty(), "{}", inside.text());
        let tone = vec![
            json!({"type": "osc", "name": "o", "freq": 200}),
            json!({"type": "decay", "name": "e", "time": 2.0, "at": 0.01}),
            json!({"type": "math", "name": "t", "op": "mul", "a": "o", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "t", "peak_db": -3}),
        ];
        let outside = compare_set(&render(tone), "hits", &members);
        println!("{}", outside.text());
        let names: Vec<&str> = outside.failing().iter().map(|f| f.name).collect();
        assert!(names.contains(&"voiced"), "{names:?}");
        assert!(
            names.contains(&"weight") || names.contains(&"body"),
            "{names:?}"
        );
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn the_verify_check_reports_each_feature_and_the_nearest() {
        let p = project("verify");
        let d = doc(hit(310.0, 0.55, 7));
        let opts = RenderOpts {
            base_dir: Some(p.clone()),
            ..RenderOpts::default()
        };
        let r = d.render(&opts).unwrap();
        let e: Expect =
            serde_json::from_value(json!({"reference_set": {"set": "hits", "max_distance": 30}}))
                .unwrap();
        let res = run(
            &e,
            &CheckCx {
                doc: &d,
                rendered: &r,
                opts: &opts,
            },
        )
        .unwrap();
        for c in &res {
            println!(
                "{} {}: {}",
                if c.pass { "PASS" } else { "FAIL" },
                c.name,
                c.detail
            );
        }
        assert!(res.iter().all(|c| c.pass));
        assert!(
            res.iter()
                .any(|c| c.name == "reference_set nearest" && c.detail.contains("hit_"))
        );
        assert!(res.iter().any(|c| c.name == "reference_set crest"));
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn a_missing_or_one_file_set_is_an_error_that_helps() {
        let p = project("errors");
        let e = load_set("doors", Some(&p)).err().unwrap();
        assert!(e.contains("doors") && e.contains("hits"), "{e}");
        let one = p.join("refs").join("single");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::copy(p.join("refs/hits/hit_0.wav"), one.join("a.wav")).unwrap();
        let e = load_set("single", Some(&p)).err().unwrap();
        assert!(e.contains("at least two"), "{e}");
        let _ = std::fs::remove_dir_all(&p);
    }
}
