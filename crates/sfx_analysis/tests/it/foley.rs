mod models {
    //! The foley physical-model nodes: bubble, air, glottis, tract, syrinx,
    //! thunder. Numbers in comments are what these tests measured.

    use serde_json::{Value, json};
    use sfx_analysis::stft::stft;
    use sfx_analysis::{describe, pitch};
    use sfx_graph::{Audio, RenderOpts, SoundDoc, wav};

    const SR: u32 = 48_000;

    /// Renders `nodes` (the last one named `last`) through a bare output stage.
    fn render(nodes: Vec<Value>, last: &str, secs: f32) -> Vec<f32> {
        let mut nodes = nodes;
        nodes.push(json!({"type": "output", "name": "out", "input": last,
        "fade_in": 0, "fade_out": 0, "ceiling_db": 12}));
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.duration = Some(secs);
        d.render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"))
        .audio
        .mix_mono()
    }

    /// Frequency of the strongest bin of the mean spectrum between `lo` and `hi`.
    fn peak_hz(x: &[f32], lo: f32, hi: f32) -> f32 {
        let s = stft(x, SR, 8192, 4096);
        let mean = s.mean(0..s.frames.len());
        let (a, b) = (s.bin(lo), s.bin(hi));
        let best = (a..=b)
            .max_by(|i, j| mean[*i].total_cmp(&mean[*j]))
            .unwrap();
        s.hz(best)
    }

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32)
            .max(1e-12)
            .log10()
    }

    fn median_pitch(x: &[f32]) -> f32 {
        let mut v: Vec<f32> = pitch::yin(x, SR).iter().filter_map(|f| f.hz).collect();
        assert!(!v.is_empty(), "no voiced frames");
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn bubble_pitch_follows_minnaert() {
        let mut last = 0.0;
        for (mm, expect) in [(2.0f32, 1643.0f32), (4.0, 821.0)] {
            let x = render(
                vec![json!({"type": "bubble", "name": "b", "radius_mm": mm, "rise": 0})],
                "b",
                0.3,
            );
            let hz = peak_hz(&x, 200.0, 8000.0);
            eprintln!("bubble {mm} mm: peak {hz:.0} Hz (Minnaert {expect})");
            assert!((hz - expect).abs() < 0.06 * expect, "{mm} mm: {hz}");
            if last > 0.0 {
                assert!(hz < last, "larger rings lower");
            }
            last = hz;
        }
    }

    /// Spectral centroid of the whole signal, Hz.
    fn centroid(x: &[f32]) -> f32 {
        let s = stft(x, SR, 2048, 1024);
        let mean = s.mean(0..s.frames.len());
        let total: f32 = mean.iter().sum();
        mean.iter()
            .enumerate()
            .map(|(b, p)| s.hz(b) * p)
            .sum::<f32>()
            / total.max(1e-20)
    }

    #[test]
    fn air_loses_highs_and_level_monotonically() {
        // Measured (white noise in): centroid 11.9 kHz at 1 m and 10 m (air
        // takes under 6 dB at 20 kHz there), 4.3 kHz at 50 m, 3.2 kHz at 100 m,
        // 1.9 kHz at 200 m; level -20 dB per decade of distance plus the filter.
        let mut prev: Option<(f32, f32)> = None;
        let mut first = (0.0, 0.0);
        for d in [1.0f32, 10.0, 50.0, 100.0, 200.0] {
            let x = render(
                vec![
                    json!({"type": "noise", "name": "n", "seed": 5, "amp": 0.5}),
                    json!({"type": "air", "name": "a", "input": "n", "distance": d}),
                ],
                "a",
                1.0,
            );
            let (c, l) = (centroid(&x), rms_db(&x));
            eprintln!("air {d} m: centroid {c:.0} Hz, rms {l:.1} dB");
            if let Some((pc, pl)) = prev {
                assert!(
                    c <= pc + 1.0,
                    "centroid never rises with distance: {c} vs {pc}"
                );
                assert!(l < pl, "level falls with distance: {l} vs {pl}");
            } else {
                first = (c, l);
            }
            prev = Some((c, l));
        }
        let (c, l) = prev.unwrap();
        assert!(
            c < 0.4 * first.0,
            "200 m is much duller than 1 m: {c} vs {}",
            first.0
        );
        assert!(l < first.1 - 40.0);
    }

    #[test]
    fn glottis_reads_its_f0() {
        for f0 in [120.0f32, 220.0] {
            let x = render(
                // The model's accuracy, without the natural drift (+-60 cents).
                vec![json!({"type": "glottis", "name": "g", "f0": f0, "natural": false})],
                "g",
                0.6,
            );
            let hz = median_pitch(&x);
            let cents = pitch::cents(hz, f0).abs();
            eprintln!("glottis {f0} Hz: YIN {hz:.1} Hz ({cents:.1} cents)");
            assert!(cents < 25.0, "{f0}: {hz}");
        }
    }

    #[test]
    fn tract_moves_the_dominant_formant() {
        let mut peaks = Vec::new();
        for v in ["a", "i", "u"] {
            let x = render(
                vec![
                    json!({"type": "glottis", "name": "g", "f0": 110, "breath": 0, "natural": false}),
                    json!({"type": "tract", "name": "t", "input": "g", "vowel": v, "natural": false}),
                ],
                "t",
                0.6,
            );
            let hz = peak_hz(&x, 150.0, 1500.0);
            eprintln!(
                "tract {v}: dominant peak {hz:.0} Hz, peak {:.2}",
                x.iter().fold(0f32, |m, s| m.max(s.abs()))
            );
            peaks.push(hz);
        }
        // /a/ F1 730, /i/ F1 270, /u/ F1 300.
        assert!((peaks[0] - 730.0).abs() < 130.0, "{peaks:?}");
        assert!((peaks[1] - 270.0).abs() < 90.0, "{peaks:?}");
        assert!((peaks[2] - 300.0).abs() < 90.0, "{peaks:?}");
        assert!(peaks[0] > peaks[1] + 300.0);
    }

    #[test]
    fn syrinx_species_and_whistle() {
        let a = render(
            vec![json!({"type": "syrinx", "name": "s", "species": "robin", "seed": 2})],
            "s",
            2.0,
        );
        assert!(a.iter().all(|v| v.is_finite()));
        let d = describe(&sfx_graph::Audio::mono(SR, a.clone()));
        eprintln!(
            "robin: peak {:.1} dB, pitch {:?}, centroid {:.0}",
            d.peak_db, d.pitch_hz, d.centroid_hz.middle
        );
        assert!(d.peak_db > -40.0);
        // A whistle follows its contour: 3 kHz glide down to 2 kHz.
        let w = render(
            vec![
                json!({"type": "ramp", "name": "r", "from": 3000, "to": 2000, "time": 0.4}),
                json!({"type": "syrinx", "name": "s", "pitch": "r", "amp": 0.5}),
            ],
            "s",
            0.4,
        );
        let hz = median_pitch(&w);
        eprintln!("whistle median {hz:.0}");
        assert!((2000.0..=3000.0).contains(&hz), "{hz}");
    }

    #[test]
    fn thunder_is_low_long_and_decays() {
        let x = render(
            vec![json!({"type": "thunder", "name": "t", "distance": 1500, "seed": 3})],
            "t",
            14.0,
        );
        let n = |secs: f32| (secs * SR as f32) as usize;
        assert!(x.iter().all(|v| v.is_finite()));
        // Silent until the sound arrives: 1500 m / 343 m/s = 4.37 s.
        assert!(rms_db(&x[..n(4.0)]) < -80.0, "quiet before arrival");
        assert!(rms_db(&x[n(4.4)..n(8.0)]) > -45.0);
        // Low: well under a few hundred Hz after the crack.
        let body = &x[n(4.4)..n(12.0)];
        let c = centroid(body);
        let d = describe(&Audio::mono(SR, x.clone()));
        eprintln!(
            "thunder: centroid {c:.0} Hz, peak {:.1} dB, audible {:.1}-{:.1} s, decay20 {:?}",
            d.peak_db,
            d.silence_start_s,
            14.0 - d.silence_end_s,
            d.decay20_s
        );
        assert!(c < 300.0, "centroid {c}");
        // Long: audible for many seconds (the rumble), and decaying.
        let thirds = [n(4.4), n(7.0), n(9.5), n(12.0)];
        let e: Vec<f32> = thirds.windows(2).map(|w| rms_db(&x[w[0]..w[1]])).collect();
        eprintln!("thunder rms by 2.5 s: {e:?}");
        assert!(e[2] < e[0] - 6.0, "decays: {e:?}");
        assert!(d.decay20_s.is_some_and(|t| t > 1.0), "long decay");
        // A nearer flash is brighter.
        let near = render(
            vec![json!({"type": "thunder", "name": "t", "distance": 150, "seed": 3})],
            "t",
            6.0,
        );
        let cn = centroid(&near);
        let pk = describe(&Audio::mono(SR, near.clone())).peak_db;
        eprintln!("thunder 150 m centroid {cn:.0}, peak {pk:.1} dB");
        assert!(cn > c, "{cn} vs {c}");
    }

    fn all_foley() -> Vec<Value> {
        vec![
            json!({"type": "bubble", "name": "b", "radius_mm": 2.5, "jitter": 0.3, "seed": 2, "at": 0.1}),
            json!({"type": "noise", "name": "n", "seed": 1}),
            json!({"type": "air", "name": "a", "input": "n", "distance": 60}),
            json!({"type": "glottis", "name": "g", "f0": 150, "vibrato_cents": 30, "seed": 3}),
            json!({"type": "tract", "name": "t", "input": "g", "vowel": "o", "vowel_to": "e", "morph": 0.4}),
            json!({"type": "syrinx", "name": "s", "species": "cardinal", "seed": 4}),
            json!({"type": "thunder", "name": "th", "distance": 300, "seed": 5}),
            json!({"type": "mix", "name": "all", "inputs": ["b", "a", "t", "s", "th"]}),
        ]
    }

    fn bytes(nodes: Vec<Value>, threads: usize) -> Vec<u8> {
        let mut nodes = nodes;
        nodes.push(json!({"type": "output", "name": "out", "input": "all", "lufs": -20}));
        let mut d = SoundDoc::named("t");
        d.nodes = nodes;
        d.duration = Some(3.0);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let r = d
                .render(&RenderOpts {
                    cached: false,
                    ..RenderOpts::default()
                })
                .unwrap_or_else(|e| panic!("{e}"));
            wav::to_bytes16(&r.audio)
        })
    }

    #[test]
    fn foley_nodes_are_deterministic_at_any_thread_count() {
        let one = bytes(all_foley(), 1);
        assert!(one.len() > 1000);
        assert_eq!(one, bytes(all_foley(), 8));
        // And the seed matters.
        let mut other = all_foley();
        other[5]["seed"] = json!(9);
        assert_ne!(one, bytes(other, 1));
    }

    #[test]
    fn foley_nodes_are_finite_on_edge_values() {
        let cases = [
            json!({"type": "bubble", "name": "x", "radius_mm": 0.1}),
            json!({"type": "bubble", "name": "x", "radius_mm": 80}),
            json!({"type": "thunder", "name": "x", "distance": 0}),
            json!({"type": "thunder", "name": "x", "distance": 30000, "exposure": 0}),
            json!({"type": "glottis", "name": "x", "f0": 0}),
            json!({"type": "glottis", "name": "x", "f0": 30000, "tension": 5, "breath": 3}),
            json!({"type": "syrinx", "name": "x", "pitch": 0}),
            json!({"type": "syrinx", "name": "x", "pitch": 40000}),
            json!({"type": "syrinx", "name": "x", "species": "dove", "rate": 8}),
        ];
        for c in cases {
            let x = render(vec![c.clone()], "x", 1.0);
            assert!(x.iter().all(|v| v.is_finite()), "{c}");
        }
        for d in [0.0f32, 0.001, 1e6] {
            let x = render(
                vec![
                    json!({"type": "noise", "name": "n"}),
                    json!({"type": "air", "name": "x", "input": "n", "distance": d, "humidity": 0}),
                ],
                "x",
                0.5,
            );
            assert!(x.iter().all(|v| v.is_finite()), "air {d}");
        }
        for scale in [0.1f32, 1.0, 9.0] {
            let x = render(
                vec![
                    json!({"type": "glottis", "name": "g", "f0": 200}),
                    json!({"type": "tract", "name": "x", "input": "g", "scale": scale}),
                ],
                "x",
                0.5,
            );
            assert!(x.iter().all(|v| v.is_finite()), "tract {scale}");
        }
    }

    #[test]
    fn foley_nodes_refuse_bad_fields_helpfully() {
        let mut d = SoundDoc::named("t");
        for bad in [
            json!({"type": "bubble", "name": "x", "radius_mm": 0}),
            json!({"type": "syrinx", "name": "x", "species": "dodo"}),
            json!({"type": "syrinx", "name": "x"}),
            json!({"type": "tract", "name": "x", "input": "x", "vowel": "q"}),
        ] {
            d.nodes = vec![
                bad.clone(),
                json!({"type": "output", "name": "out", "input": "x"}),
            ];
            assert!(d.render(&RenderOpts::default()).is_err(), "{bad}");
        }
    }
}

mod voice {
    //! `glottis` roughness / subharmonic / fry and `tract` nasal /
    //! frication / growl. The first test pins the default outputs (recorded
    //! before the new fields existed); the rest measure each field.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::measure::roughness_pct;
    use sfx_graph::{RenderOpts, SoundDoc};

    /// FNV-1a over the bit patterns of every sample.
    fn hash(x: &[f32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for v in x {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// (label, nodes, node to hash) for the default-output pins.
    fn pins() -> Vec<(&'static str, Vec<Value>, &'static str)> {
        let g = |extra: Value| {
            // natural: false keeps the pre-voice_fix output these pins hold.
            let mut o = json!({"type": "glottis", "name": "g", "f0": 140, "natural": false});
            for (k, v) in extra.as_object().unwrap() {
                o[k] = v.clone();
            }
            o
        };
        let t = |extra: Value| {
            let mut o = json!({"type": "tract", "name": "t", "input": "g", "natural": false});
            for (k, v) in extra.as_object().unwrap() {
                o[k] = v.clone();
            }
            o
        };
        vec![
            ("glottis default", vec![g(json!({}))], "g"),
            (
                "glottis breathy vibrato seeded",
                vec![g(
                    json!({"tension": 0.1, "breath": 0.3, "vibrato_cents": 30, "vibrato_hz": 6, "seed": 3}),
                )],
                "g",
            ),
            (
                "glottis pressed, amp, no breath",
                vec![g(json!({"tension": 1, "breath": 0, "amp": 0.5, "f0": 90}))],
                "g",
            ),
            (
                "glottis f0 ramp",
                vec![
                    json!({"type": "ramp", "name": "r", "from": 80, "to": 300, "time": 0.4}),
                    g(json!({"f0": "r", "seed": 9})),
                ],
                "g",
            ),
            ("tract default", vec![g(json!({})), t(json!({}))], "t"),
            (
                "tract e",
                vec![g(json!({"seed": 2})), t(json!({"vowel": "e"}))],
                "t",
            ),
            (
                "tract o to i, morph, scale",
                vec![
                    g(json!({"vibrato_cents": 20})),
                    t(json!({"vowel": "o", "vowel_to": "i", "morph": 0.4, "scale": 1.3})),
                ],
                "t",
            ),
            (
                "tract formants list",
                vec![
                    g(json!({"f0": 200})),
                    t(json!({"formants": [[400, 60], [1500, 100], [2600, 150]]})),
                ],
                "t",
            ),
            (
                "tract on noise",
                vec![
                    json!({"type": "noise", "name": "g", "color": "white"}),
                    t(json!({"vowel": "u"})),
                ],
                "t",
            ),
        ]
    }

    const PINNED: &[(&str, u64)] = &[
        ("glottis default", 0x7733151fc79996c2),
        ("glottis breathy vibrato seeded", 0xc97c06f84fe3d7e5),
        ("glottis pressed, amp, no breath", 0x2133b0cf5a01bfa4),
        ("glottis f0 ramp", 0xa6945f0a49f3602a),
        ("tract default", 0x961146eeb06f2fd3),
        ("tract e", 0x0e422db4677f7f59),
        ("tract o to i, morph, scale", 0xae635e6fd25789e8),
        ("tract formants list", 0xe82fa998a4780f1c),
        ("tract on noise", 0x99934a530705f412),
    ];

    #[test]
    fn defaults_are_bit_identical_to_before() {
        for (label, nodes, node) in pins() {
            let h = hash(&node_audio(&nodes, node, 0.5));
            eprintln!("PIN {label}: {h:#018x}");
            let want = PINNED.iter().find(|(l, _)| *l == label).unwrap().1;
            assert_eq!(h, want, "{label}: default output changed");
        }
    }

    // ------------------------------------------------------------ helpers

    /// A glottis named `g` with `extra` fields merged in.
    fn glottis(f0: f32, extra: Value) -> Value {
        let mut o =
            json!({"type": "glottis", "name": "g", "f0": f0, "breath": 0, "natural": false});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    }

    /// Power-weighted mean frequency of `x`, Hz.
    fn centroid(x: &[f32]) -> f32 {
        let s = spectrum(x, SR);
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (f, d) in s {
            let p = 10f64.powf(f64::from(d) / 10.0);
            num += f64::from(f) * p;
            den += p;
        }
        (num / den.max(1e-30)) as f32
    }

    /// The 20 to 150 Hz envelope share (S8a `roughness`) of `nodes`, read from
    /// the output stage of a 1 s render.
    fn rough(nodes: &[Value]) -> f32 {
        roughness_pct(&render(nodes, 1.0))
    }

    /// Silence for a tract that should only add its own noise.
    fn silence() -> Value {
        json!({"type": "osc", "name": "g", "freq": 100, "amp": 0})
    }

    /// A tract whose single formant sits past 0.45 x sr, so the cascade is a
    /// flat pass-through (x 0.8) and only the new stages shape the sound.
    fn flat_tract(extra: Value) -> Value {
        let mut o = json!({"type": "tract", "name": "t", "input": "g", "formants": [[30000, 100]]});
        for (k, v) in extra.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    }

    fn white() -> Value {
        json!({"type": "noise", "name": "g", "color": "white"})
    }

    /// The error text of a document that must not render.
    fn err(nodes: &[Value]) -> String {
        let mut d = SoundDoc::named("t");
        d.nodes = nodes.to_vec();
        d.nodes
            .push(json!({"type": "output", "name": "out", "input": "t"}));
        d.duration = Some(0.2);
        match d.render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        }) {
            Ok(_) => panic!("rendered, expected an error"),
            Err(e) => e.to_string(),
        }
    }

    fn pin(label: &str) -> u64 {
        PINNED.iter().find(|(l, _)| *l == label).unwrap().1
    }

    // -------------------------------------------------------------- glottis

    #[test]
    fn fields_at_zero_equal_the_defaults() {
        // The pins use the default breath (0.05), so no `glottis()` helper here.
        let plain = json!({"type": "glottis", "name": "g", "f0": 140, "natural": false,
        "roughness": 0, "subharmonic": 0, "fry": 0});
        let x = node_audio([plain], "g", 0.5);
        assert_eq!(hash(&x), pin("glottis default"), "glottis fields at 0");
        let t = node_audio(
            [
                json!({"type": "glottis", "name": "g", "f0": 140, "natural": false}),
                json!({"type": "tract", "name": "t", "input": "g", "natural": false, "nasal": 0, "growl": 0,
                "frication": {"freq": 4000, "level": 0}}),
            ],
            "t",
            0.5,
        );
        assert_eq!(hash(&t), pin("tract default"), "tract fields at 0");
    }

    #[test]
    fn roughness_raises_the_envelope_roughness() {
        // f0 250 keeps the pulse train's own envelope out of 20..150 Hz.
        let mut prev = -1.0f32;
        let mut first = 0.0;
        for r in [0.0, 0.5, 1.0] {
            let v = rough(&[glottis(250.0, json!({"roughness": r, "seed": 1}))]);
            eprintln!("glottis roughness {r}: S8a roughness {v:.2} %");
            assert!(v > prev, "roughness {r}: {v} not above {prev}");
            if r == 0.0 {
                first = v;
            }
            prev = v;
        }
        assert!(prev > 1.5 * first.max(0.1), "{prev} vs {first}");
    }

    #[test]
    fn roughness_is_seeded() {
        let make = |seed: u64| {
            node_audio(
                [glottis(200.0, json!({"roughness": 0.8, "seed": seed}))],
                "g",
                0.4,
            )
        };
        assert_eq!(hash(&make(4)), hash(&make(4)));
        assert_ne!(hash(&make(4)), hash(&make(5)));
    }

    #[test]
    fn roughness_keeps_the_pitch() {
        // Jitter is zero-mean: the YIN pitch stays near f0 at roughness 0.5.
        let x = node_audio(
            [glottis(200.0, json!({"roughness": 0.5, "seed": 2}))],
            "g",
            1.0,
        );
        let mut v: Vec<f32> = sfx_analysis::pitch::yin(&x, SR)
            .iter()
            .filter_map(|f| f.hz)
            .collect();
        v.sort_by(f32::total_cmp);
        let hz = v[v.len() / 2];
        eprintln!("roughness 0.5: median YIN pitch {hz:.1} Hz (f0 200)");
        assert!((hz - 200.0).abs() < 12.0, "{hz}");
    }

    #[test]
    fn subharmonic_puts_a_peak_at_half_f0() {
        let f0 = 200.0;
        let mut prev = f32::MIN;
        let mut at_one = (0.0, 0.0);
        for b in [0.0, 0.25, 0.5, 1.0] {
            let x = node_audio([glottis(f0, json!({"subharmonic": b}))], "g", 1.0);
            let half = band_db(&x, SR, 0.5 * f0 - 4.0, 0.5 * f0 + 4.0);
            let full = band_db(&x, SR, f0 - 4.0, f0 + 4.0);
            eprintln!("glottis subharmonic {b}: f0/2 {half:.1} dB, f0 {full:.1} dB");
            assert!(half > prev, "subharmonic {b}: {half} not above {prev}");
            prev = half;
            if b == 0.0 {
                assert!(half < -100.0, "no subharmonic at 0: {half}");
            }
            if b == 1.0 {
                at_one = (half, full);
            }
        }
        assert!(at_one.0 > at_one.1 - 10.0, "{at_one:?}");
        // At 1 the strongest low peak is at f0 / 2.
        let x = node_audio([glottis(f0, json!({"subharmonic": 1}))], "g", 1.0);
        let (hz, _) = peaks(&x, SR, 6, 60.0)
            .into_iter()
            .find(|(f, _)| *f < 150.0)
            .unwrap();
        assert!((hz - 100.0).abs() < 2.0, "{hz}");
    }

    #[test]
    fn fry_raises_the_envelope_roughness_and_adds_half_f0() {
        let mut prev = -1.0f32;
        for f in [0.0, 0.5, 1.0] {
            let v = rough(&[glottis(250.0, json!({"fry": f, "seed": 1}))]);
            eprintln!("glottis fry {f}: S8a roughness {v:.2} %");
            assert!(v > prev, "fry {f}: {v} not above {prev}");
            prev = v;
        }
        let off = node_audio([glottis(200.0, json!({}))], "g", 1.0);
        let on = node_audio([glottis(200.0, json!({"fry": 1, "seed": 1}))], "g", 1.0);
        let (a, b) = (
            band_db(&off, SR, 90.0, 110.0),
            band_db(&on, SR, 90.0, 110.0),
        );
        eprintln!("fry 1: band 90..110 Hz {b:.1} dB (off {a:.1})");
        assert!(b > a + 30.0, "{b} vs {a}");
    }

    // ---------------------------------------------------------------- tract

    #[test]
    fn nasal_adds_a_murmur_and_an_antiformant() {
        let bands = |n: f32| {
            let x = node_audio([white(), flat_tract(json!({"nasal": n}))], "t", 1.0);
            (
                band_db(&x, SR, 200.0, 300.0),
                band_db(&x, SR, 900.0, 1100.0),
                band_db(&x, SR, 4000.0, 8000.0),
            )
        };
        let off = bands(0.0);
        let (mut low_prev, mut mid_prev) = (off.0, off.1);
        for n in [0.5, 1.0] {
            let b = bands(n);
            eprintln!(
                "tract nasal {n}: 200-300 {:+.1} dB, 900-1100 {:+.1} dB, 4k-8k {:+.1} dB vs off",
                b.0 - off.0,
                b.1 - off.1,
                b.2 - off.2
            );
            assert!(b.0 > low_prev && b.1 < mid_prev, "{n}: {b:?}");
            assert!((b.2 - off.2).abs() < 0.5, "highs pass unchanged: {b:?}");
            (low_prev, mid_prev) = (b.0, b.1);
        }
        assert!(low_prev > off.0 + 5.0, "murmur rises >= 5 dB");
        assert!(mid_prev < off.1 - 10.0, "antiformant falls >= 10 dB");
    }

    #[test]
    fn frication_band_follows_freq_and_level() {
        let mut prev = 0.0f32;
        for f in [1500.0f32, 4000.0, 9000.0] {
            let x = node_audio(
                [
                    silence(),
                    flat_tract(json!({"frication": {"freq": f, "level": 1}})),
                ],
                "t",
                1.0,
            );
            let c = centroid(&x);
            eprintln!(
                "frication freq {f}: centroid {c:.0} Hz, rms {:.1} dB",
                rms_db(&x)
            );
            assert!(c > prev * 1.5, "{f}: {c} vs {prev}");
            assert!(c > 0.6 * f && c < 1.6 * f, "centroid {c} near {f}");
            prev = c;
        }
        // Level: 20 dB a decade.
        let mut levels = Vec::new();
        for l in [0.01f32, 0.1, 1.0] {
            let x = node_audio(
                [
                    silence(),
                    flat_tract(json!({"frication": {"freq": 5000, "level": l}})),
                ],
                "t",
                1.0,
            );
            levels.push(rms_db(&x));
        }
        eprintln!("frication level 0.01 / 0.1 / 1: rms {levels:.1?} dB");
        for w in levels.windows(2) {
            assert!((w[1] - w[0] - 20.0).abs() < 1.5, "{levels:?}");
        }
    }

    #[test]
    fn frication_takes_signals_for_freq_and_level() {
        let x = node_audio(
            [
                silence(),
                json!({"type": "ramp", "name": "hz", "from": 1500, "to": 10000, "time": 1.0}),
                json!({"type": "ramp", "name": "lv", "from": 0, "to": 1, "time": 1.0}),
                flat_tract(json!({"frication": {"freq": "hz", "level": "lv"}})),
            ],
            "t",
            1.0,
        );
        let q = x.len() / 4;
        let (a, b) = (&x[..q], &x[3 * q..]);
        let (ca, cb) = (centroid(a), centroid(b));
        let (ra, rb) = (rms_db(a), rms_db(b));
        eprintln!(
            "frication ramps: first quarter centroid {ca:.0} Hz rms {ra:.1}, last {cb:.0} Hz rms {rb:.1}"
        );
        assert!(cb > 2.0 * ca);
        assert!(rb > ra + 15.0);
    }

    #[test]
    fn growl_raises_the_envelope_roughness() {
        let mut prev = -1.0f32;
        let mut first = 0.0;
        for g in [0.0, 0.5, 1.0] {
            let v = rough(&[
                glottis(250.0, json!({"breath": 0.05})),
                json!({"type": "tract", "name": "t", "input": "g", "growl": g, "seed": 1}),
            ]);
            eprintln!("tract growl {g}: S8a roughness {v:.2} %");
            assert!(v > prev, "growl {g}: {v} not above {prev}");
            if g == 0.0 {
                first = v;
            }
            prev = v;
        }
        assert!(prev > 4.0 * first.max(0.1), "{prev} vs {first}");
    }

    // ------------------------------------------------- determinism, extremes

    fn everything() -> Vec<Value> {
        vec![
            json!({"type": "ramp", "name": "pitch", "from": 90, "to": 220, "time": 0.6}),
            json!({"type": "ramp", "name": "fz", "from": 2000, "to": 8000, "time": 0.6}),
            json!({"type": "wander", "name": "lv", "range": [0.0, 0.2], "rate": 3}),
            glottis(
                0.0,
                json!({"f0": "pitch", "breath": 0.1, "roughness": 0.4, "subharmonic": 0.3,
                "fry": 0.5, "seed": 7}),
            ),
            json!({"type": "tract", "name": "t", "input": "g", "vowel": "o", "vowel_to": "i",
            "morph": 0.5, "nasal": 0.6, "growl": 0.4, "seed": 3,
            "frication": {"freq": "fz", "level": "lv"}}),
        ]
    }

    #[test]
    fn new_fields_are_bit_identical_on_any_thread_count() {
        assert_bit_identical(everything(), 0.6);
    }

    #[test]
    fn recipe_renders() {
        let x = node_audio(everything(), "t", 0.6);
        assert_eq!(x.len(), 28_800);
        assert_finite(&x, "recipe");
        eprintln!(
            "recipe: rms {:.2} dB, peak {:.2}",
            rms_db(&x),
            x.iter().fold(0f32, |m, v| m.max(v.abs()))
        );
        assert!(rms_db(&x) > -40.0);
    }

    #[test]
    fn extremes_stay_finite() {
        for f0 in [0.0f32, 1.0, 30_000.0] {
            for v in [0.0f32, 1.0] {
                let x = node_audio(
                    [
                        glottis(
                            f0,
                            json!({"roughness": v, "subharmonic": v, "fry": v, "tension": 5, "breath": 3}),
                        ),
                        json!({"type": "tract", "name": "t", "input": "g", "nasal": v, "growl": v,
                        "scale": 10, "frication": {"freq": 21_600, "level": 1e6}}),
                    ],
                    "t",
                    0.3,
                );
                assert_finite(&x, &format!("f0 {f0} fields {v}"));
            }
        }
        // Signals far outside their ranges are clamped, not trusted.
        let x = node_audio(
            [
                json!({"type": "ramp", "name": "wild", "from": -5, "to": 5, "time": 0.3}),
                json!({"type": "ramp", "name": "hz", "from": -1e9, "to": 1e9, "time": 0.3}),
                glottis(
                    150.0,
                    json!({"roughness": "wild", "subharmonic": "wild", "fry": "wild"}),
                ),
                json!({"type": "tract", "name": "t", "input": "g", "nasal": "wild", "growl": "wild",
                "frication": {"freq": "hz", "level": "wild"}}),
            ],
            "t",
            0.3,
        );
        assert_finite(&x, "wild signals");
    }

    #[test]
    fn errors_name_the_field() {
        let g = json!({"type": "glottis", "name": "g", "f0": 150});
        let t = |extra: Value| {
            let mut o = json!({"type": "tract", "name": "t", "input": "g", "natural": false});
            for (k, v) in extra.as_object().unwrap() {
                o[k] = v.clone();
            }
            o
        };
        let gl = |extra: Value| {
            let mut o = json!({"type": "glottis", "name": "t", "f0": 150});
            for (k, v) in extra.as_object().unwrap() {
                o[k] = v.clone();
            }
            o
        };
        let cases: Vec<(String, &str)> = vec![
            (
                err(&[gl(json!({"roughness": 2}))]),
                "roughness: 2: give a number from 0 to 1",
            ),
            (
                err(&[gl(json!({"subharmonic": -1}))]),
                "subharmonic: -1: give a number from 0 to 1",
            ),
            (
                err(&[gl(json!({"fry": 1.5}))]),
                "fry: 1.5: give a number from 0 to 1",
            ),
            (
                err(&[g.clone(), t(json!({"nasal": 1.5}))]),
                "nasal: 1.5: give a number from 0 to 1",
            ),
            (
                err(&[g.clone(), t(json!({"growl": 3}))]),
                "growl: 3: give a number from 0 to 1",
            ),
            (
                err(&[
                    g.clone(),
                    t(json!({"frication": {"freq": 30000, "level": 1}})),
                ]),
                "frication.freq: 30000: give a number from 20 to 21600",
            ),
            (
                err(&[
                    g.clone(),
                    t(json!({"frication": {"freq": 4000, "level": -1}})),
                ]),
                "frication.level: -1: give a number >= 0",
            ),
        ];
        for (got, want) in &cases {
            eprintln!("error: {got}");
            assert!(got.contains(want), "`{got}` lacks `{want}`");
        }
        let unknown = err(&[g, t(json!({"frication": {"freq": 4000, "lvl": 1}}))]);
        eprintln!("error: {unknown}");
        assert!(unknown.contains("lvl"), "{unknown}");
    }
}

mod voice_natural {
    //! The `natural` defaults of `glottis`, `tract` and `bubble`
    //! (batch 2 blind labels: creature calls heard as "SAM" / "artificial",
    //! drips heard as a UI click / cartoon pop). Each test measures the old
    //! output (`natural: false`) against the new default and prints both.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::describe;
    use sfx_graph::Audio;

    const SR: u32 = 48_000;

    /// Off-grid against on-grid energy (dB) above 4 kHz, minus the same ratio
    /// under 4 kHz. A bin is off the grid at least 0.3 x f0 from every harmonic.
    /// Shimmer noise sits between the harmonics in proportion to them at every
    /// frequency, so it cancels; aliasing folds only into the top and stays.
    fn alias_excess_db(x: &[f32], f0: f32) -> f32 {
        let s = spectrum(x, SR);
        let mut e = [[0.0f64; 2]; 2]; // [band][on, off]
        for (f, d) in s {
            if f < 0.5 * f0 {
                continue;
            }
            let p = 10f64.powf(f64::from(d) / 10.0);
            let k = (f / f0).round();
            let off = usize::from((f - k * f0).abs() > 0.3 * f0);
            e[usize::from(f > 4000.0)][off] += p;
        }
        let r = |b: [f64; 2]| 10.0 * (b[1].max(1e-30) / b[0].max(1e-30)).log10();
        (r(e[1]) - r(e[0])) as f32
    }

    fn glottis(f0: f32, natural: bool) -> Value {
        json!({"type": "glottis", "name": "g", "f0": f0, "breath": 0, "natural": natural})
    }

    #[test]
    fn natural_glottis_is_band_limited() {
        let mut res = vec![];
        for natural in [false, true] {
            let x = node_audio([glottis(2900.0, natural)], "g", 1.0);
            assert_finite(&x, "glottis");
            res.push(alias_excess_db(&x[4800..], 2900.0));
        }
        eprintln!(
            "alias excess (off/on grid above 4 kHz minus below) at f0 2900: old {:.1} dB, natural {:.1} dB",
            res[0], res[1]
        );
        assert!(res[1] < res[0] - 12.0, "{res:?}");
    }

    /// Spread (max - min, cents) of the pitch track over the voiced frames.
    fn pitch_spread_cents(x: &[f32]) -> f32 {
        let v: Vec<f32> = sfx_analysis::pitch::yin(x, SR)
            .iter()
            .filter_map(|f| f.hz)
            .collect();
        let lo = v.iter().copied().fold(f32::MAX, f32::min);
        let hi = v.iter().copied().fold(0.0, f32::max);
        1200.0 * (hi / lo).log2()
    }

    #[test]
    fn natural_glottis_wanders_and_breathes_with_the_pulse() {
        let old = node_audio(
            [json!({"type": "glottis", "name": "g", "f0": 140, "natural": false})],
            "g",
            2.0,
        );
        let new = node_audio(
            [json!({"type": "glottis", "name": "g", "f0": 140})],
            "g",
            2.0,
        );
        let (so, sn) = (pitch_spread_cents(&old), pitch_spread_cents(&new));
        eprintln!("pitch spread over 2 s: old {so:.1} cents, natural {sn:.1} cents");
        assert!(so < 6.0, "{so}");
        // Round 2: +-60 cents of drift (round 1 read 25 cents and was heard
        // as identical to the old voice).
        assert!((40.0..200.0).contains(&sn), "{sn}");
        // Still reads its f0 (median).
        let mut v: Vec<f32> = sfx_analysis::pitch::yin(&new, SR)
            .iter()
            .filter_map(|f| f.hz)
            .collect();
        v.sort_by(f32::total_cmp);
        let med = v[v.len() / 2];
        assert!((1200.0 * (med / 140.0).log2()).abs() < 40.0, "{med}");
        // The aspiration is above the voice band.
        let hi = |x: &[f32]| band_db(x, SR, 6000.0, 16000.0) - band_db(x, SR, 100.0, 16000.0);
        eprintln!(
            "share above 6 kHz: old {:.1} dB, natural {:.1} dB",
            hi(&old),
            hi(&new)
        );
        assert!(hi(&new) > hi(&old));
    }

    fn vowel(natural: bool) -> Vec<Value> {
        vec![
            json!({"type": "glottis", "name": "g", "f0": 120, "natural": natural}),
            json!({"type": "tract", "name": "t", "input": "g", "vowel": "a", "natural": natural}),
            json!({"type": "output", "name": "out", "input": "t", "lufs": -20}),
        ]
    }

    fn centroid_range(a: &Audio) -> f32 {
        let d = describe(a);
        let c: Vec<f32> = d.centroid_tenths.iter().flatten().copied().collect();
        let lo = c.iter().copied().fold(f32::MAX, f32::min);
        let hi = c.iter().copied().fold(0.0, f32::max);
        hi / lo
    }

    #[test]
    fn natural_tract_moves_and_is_not_razor_sharp() {
        let old = render(vowel(false), 3.0);
        let new = render(vowel(true), 3.0);
        let (ro, rn) = (centroid_range(&old), centroid_range(&new));
        eprintln!("centroid by tenth max/min: old {ro:.3}, natural {rn:.3}");
        assert!(rn > ro, "{ro} {rn}");
        let (dold, dnew) = (describe(&old), describe(&new));
        eprintln!(
            "voiced {:.2} -> {:.2}, harmonicity {:.2} -> {:.2}, flatness {:.3} -> {:.3}",
            dold.voiced,
            dnew.voiced,
            dold.harmonicity,
            dnew.harmonicity,
            dold.flatness_all,
            dnew.flatness_all
        );
        assert!(dnew.voiced > 0.8, "still a voice");
    }

    fn drip(natural: bool) -> Vec<Value> {
        vec![
            json!({"type": "bubble", "name": "b", "radius_mm": 2.5, "at": 0.01, "natural": natural, "seed": 1}),
        ]
    }

    #[test]
    fn natural_bubble_has_an_impact_and_a_shorter_ring() {
        let old = node_audio(drip(false), "b", 0.5);
        let new = node_audio(drip(true), "b", 0.5);
        assert_finite(&new, "bubble");
        let s = (0.01 * SR as f32) as usize;
        let w = (0.004 * SR as f32) as usize;
        let hi = |x: &[f32]| band_db(&x[s..s + w], SR, 6000.0, 20000.0);
        eprintln!(
            "first 4 ms above 6 kHz: old {:.1} dB, natural {:.1} dB",
            hi(&old),
            hi(&new)
        );
        assert!(hi(&new) > hi(&old) + 20.0);
        let d = |x: &[f32]| describe(&Audio::mono(SR, x.to_vec()));
        let (dold, dnew) = (d(&old), d(&new));
        eprintln!(
            "decay -20 dB: old {:?}, natural {:?}; flatness {:.4} -> {:.4}; pitch {:?} -> {:?}",
            dold.decay20_s,
            dnew.decay20_s,
            dold.flatness_all,
            dnew.flatness_all,
            dold.pitch_hz,
            dnew.pitch_hz
        );
        assert!(dnew.decay20_s.unwrap() < dold.decay20_s.unwrap());
        assert!(dnew.flatness_all > dold.flatness_all);
        // The ring is still the Minnaert pitch: the strongest bin under 4 kHz.
        let p = peaks(&new, SR, 1, 0.0)[0].0;
        assert!((p - 3290.0 / 2.5).abs() < 0.1 * 1316.0, "{p}");
    }

    #[test]
    fn natural_voices_are_deterministic() {
        let mut nodes = vowel(true);
        nodes.extend(drip(true));
        nodes[2] = json!({"type": "mix", "name": "m", "inputs": ["t", "b"]});
        nodes.push(json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}));
        assert_bit_identical(nodes, 1.0);
    }
}

mod thunder_brightness {
    //! `thunder` `brightness` / `length`, `gain` `gain_db`.

    use crate::common::*;
    use serde_json::{Value, json};
    use sfx_analysis::stft::stft;
    use sfx_graph::{RenderOpts, SoundDoc};

    fn fnv(x: &[f32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for v in x {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    fn thunder(extra: Value) -> Vec<f32> {
        let mut t = json!({"type": "thunder", "name": "t", "distance": 600, "seed": 3});
        t.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        node_audio(vec![t], "t", 12.0)
    }

    fn centroid(x: &[f32]) -> f32 {
        let s = stft(x, SR, 2048, 1024);
        let mean = s.mean(0..s.frames.len());
        let total: f32 = mean.iter().sum();
        mean.iter()
            .enumerate()
            .map(|(b, p)| s.hz(b) * p)
            .sum::<f32>()
            / total.max(1e-20)
    }

    /// (first, last) seconds where the 100 ms rms is within 30 dB of the
    /// loudest 100 ms.
    fn audible_s(x: &[f32]) -> (f32, f32) {
        let w = SR as usize / 10;
        let env: Vec<f32> = x
            .chunks(w)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let top = env.iter().fold(0.0f32, |a, v| a.max(*v));
        let floor = top * 10f32.powf(-30.0 / 20.0);
        let first = env.iter().position(|e| *e > floor).unwrap_or(0);
        let last = env.iter().rposition(|e| *e > floor).unwrap_or(0);
        (first as f32 * 0.1, (last + 1) as f32 * 0.1)
    }

    /// FNV-1a of the f32 bits of the node, recorded before `brightness` and
    /// `length` existed.
    const BASELINES: [(&str, u64); 3] = [
        ("default", 0x6f38_e1e7_f185_1bf5),
        ("exposure", 0xae10_af28_2243_6477),
        ("near", 0x9d35_d7b6_76fb_22df),
    ];

    fn baseline_renders() -> Vec<(&'static str, Vec<f32>)> {
        vec![
            ("default", thunder(json!({}))),
            ("exposure", thunder(json!({"exposure": 0.4}))),
            ("near", thunder(json!({"distance": 150, "seed": 9}))),
        ]
    }

    #[test]
    fn defaults_are_bit_identical_to_before() {
        for ((name, x), (_, want)) in baseline_renders().iter().zip(BASELINES) {
            println!("{name}: hash {:016x}", fnv(x));
            assert_eq!(fnv(x), want, "{name}");
        }
    }

    fn gain_doc(extra: Value) -> Result<Vec<f32>, String> {
        let mut g = json!({"type": "gain", "name": "g", "input": "src"});
        g.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let mut d = SoundDoc::named("test");
        d.nodes = vec![
            json!({"type": "osc", "name": "src", "freq": 1000, "amp": 0.5}),
            g,
            json!({"type": "output", "name": "out", "input": "g"}),
        ];
        d.duration = Some(0.2);
        d.render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .map(|r| r.audio.channels[0].clone())
    }

    #[test]
    fn gain_db_is_db() {
        let a = gain_doc(json!({"db": -6})).unwrap();
        let b = gain_doc(json!({"gain_db": -6})).unwrap();
        println!(
            "db -6 rms {:.2} dB, gain_db -6 rms {:.2} dB",
            rms_db(&a),
            rms_db(&b)
        );
        assert_eq!(a, b, "gain_db -6 differs from db -6");
        // -6 dB on a 0.5 amplitude sine: rms -9.03 - 6 = about -15 dB (power).
        let full = gain_doc(json!({"db": 0})).unwrap();
        assert!((rms_db(&full) - rms_db(&a) - 6.0).abs() < 0.2);
        // A signal works too (a param).
        let mut d = SoundDoc::named("test");
        d.nodes = vec![
            json!({"type": "osc", "name": "src", "freq": 1000, "amp": 0.5}),
            json!({"type": "gain", "name": "g", "input": "src", "gain_db": "$g"}),
            json!({"type": "output", "name": "out", "input": "g"}),
        ];
        d.params.insert("g".into(), -6.0);
        d.duration = Some(0.2);
        let r = d.render(&RenderOpts::default()).unwrap();
        assert_eq!(r.audio.channels[0], a);
    }

    #[test]
    fn gain_db_and_db_together_is_an_error() {
        let e = gain_doc(json!({"db": -6, "gain_db": -6})).unwrap_err();
        println!("both: {e}");
        assert!(e.contains("duplicate") || e.contains("both"), "{e}");
        assert!(e.contains("db"), "{e}");
        let e = gain_doc(json!({"gain_db": -6, "gain": 0.5})).unwrap_err();
        println!("gain_db + gain: {e}");
        assert!(e.contains("exactly one"), "{e}");
    }

    #[test]
    fn explicit_neutral_values_are_the_default() {
        let base = thunder(json!({}));
        assert_eq!(base, thunder(json!({"brightness": 0.5, "length": 1})));
    }

    #[test]
    fn brightness_raises_the_centroid_monotonically() {
        let mut prev = 0.0f32;
        let mut peaks = vec![];
        for b in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let x = thunder(json!({"brightness": b}));
            assert_finite(&x, "thunder");
            let c = centroid(&x);
            let above = band_db(&x, SR, 100.0, 20_000.0) - band_db(&x, SR, 1.0, 100.0);
            let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            println!(
                "brightness {b}: centroid {c:.0} Hz, >100 Hz vs <100 Hz {above:.1} dB, peak {pk:.3}"
            );
            assert!(c > prev, "brightness {b}: {c} not above {prev}");
            prev = c;
            peaks.push(pk);
        }
        // The peak is kept (the tilt changes tone, not level).
        let p0 = peaks[2];
        for p in &peaks {
            assert!((p / p0 - 1.0).abs() < 0.02, "peaks {peaks:?}");
        }
    }

    #[test]
    fn brightness_moves_energy_above_100_hz() {
        let dark = thunder(json!({"brightness": 0.0}));
        let bright = thunder(json!({"brightness": 1.0}));
        let ratio = |x: &[f32]| band_db(x, SR, 100.0, 20_000.0) - band_db(x, SR, 1.0, 100.0);
        let (d, b) = (ratio(&dark), ratio(&bright));
        println!("above/below 100 Hz: dark {d:.1} dB, bright {b:.1} dB");
        assert!(b - d > 20.0, "{d} {b}");
    }

    #[test]
    fn length_stretches_the_rumble() {
        let base = thunder(json!({}));
        let (first0, last0) = audible_s(&base);
        println!(
            "length 1: audible {first0:.2}-{last0:.2} s, centroid {:.0} Hz",
            centroid(&base)
        );
        let mut prev = last0;
        for (l, lo, hi) in [(2.0f32, 1.5, 2.4), (3.0, 2.2, 3.5)] {
            let x = thunder(json!({"length": l}));
            assert_finite(&x, "thunder");
            let (first, last) = audible_s(&x);
            let rumble = (last - first) / (last0 - first0);
            let c = centroid(&x);
            println!(
                "length {l}: audible {first:.2}-{last:.2} s ({rumble:.2}x the rumble), centroid {c:.0} Hz, rms {:.1} dB",
                rms_db(&x)
            );
            assert!(
                (first - first0).abs() < 0.1,
                "arrival moved: {first} vs {first0}"
            );
            assert!((lo..=hi).contains(&rumble), "length {l}: {rumble}x");
            assert!(last > prev, "{last} vs {prev}");
            prev = last;
            // The spectrum stays: the stretch is not a resample.
            let c0 = centroid(&base);
            assert!((c / c0 - 1.0).abs() < 0.4, "centroid {c} vs {c0}");
        }
        let short = thunder(json!({"length": 0.5}));
        let (f, l) = audible_s(&short);
        println!("length 0.5: audible {f:.2}-{l:.2} s");
        let rumble = (l - f) / (last0 - first0);
        assert!((0.35..=0.7).contains(&rumble), "length 0.5: {rumble}x");
    }

    #[test]
    fn length_keeps_the_clap_and_is_bit_identical() {
        let base = thunder(json!({}));
        let long = thunder(json!({"length": 2}));
        // The clap: the first 0.1 s after the arrival matches in level.
        let a = base.iter().position(|v| *v != 0.0).unwrap();
        let n = (0.1 * SR as f32) as usize;
        let (r0, r1) = (rms_db(&base[a..a + n]), rms_db(&long[a..a + n]));
        println!("clap rms: length 1 {r0:.1} dB, length 2 {r1:.1} dB");
        assert!((r0 - r1).abs() < 3.0);
        assert_bit_identical(
            vec![
                json!({"type": "thunder", "name": "t", "distance": 600, "length": 2.5, "brightness": 0.9, "seed": 3}),
            ],
            12.0,
        );
    }

    #[test]
    fn thunder_new_fields_are_validated() {
        let err = |extra: Value| {
            let mut t = json!({"type": "thunder", "name": "t"});
            t.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let mut d = SoundDoc::named("test");
            d.nodes = vec![t, json!({"type": "output", "name": "o", "input": "t"})];
            d.duration = Some(3.0);
            d.render(&RenderOpts::default()).unwrap_err()
        };
        for (extra, field) in [
            (json!({"brightness": 1.5}), "brightness"),
            (json!({"brightness": -0.1}), "brightness"),
            (json!({"length": 0.2}), "length"),
            (json!({"length": 4}), "length"),
        ] {
            let e = err(extra);
            println!("{e}");
            assert!(e.contains(field) && e.contains("give"), "{e}");
        }
    }
}

mod thunder_rumble {
    //! `thunder.rumble`.

    use crate::common::*;
    use serde_json::{Value, json};

    fn fnv(x: &[f32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for v in x {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    fn thunder(base: Value, extra: Value, secs: f32) -> Vec<f32> {
        let mut n = json!({"type": "thunder", "name": "t"});
        let o = n.as_object_mut().unwrap();
        o.extend(base.as_object().unwrap().clone());
        o.extend(extra.as_object().unwrap().clone());
        node_audio(vec![n], "t", secs)
    }

    fn flash() -> Value {
        json!({"distance": 1500, "seed": 3})
    }

    /// Hashes taken from the code before `rumble` existed.
    #[test]
    fn defaults_are_bit_identical() {
        let cases: [(Value, f32, u64); 4] = [
            (flash(), 16.0, 0xebec_a2f8_5b5e_d486),
            (json!({"distance": 300, "seed": 5}), 4.0, HASH_300),
            (
                json!({"distance": 800, "seed": 2, "exposure": 0.5, "brightness": 0.8}),
                8.0,
                HASH_800,
            ),
            (
                json!({"distance": 500, "seed": 7, "length": 1.5, "at": 0.2}),
                8.0,
                HASH_LEN,
            ),
        ];
        for (base, secs, want) in cases {
            let none = fnv(&thunder(base.clone(), json!({}), secs));
            let zero = fnv(&thunder(base.clone(), json!({"rumble": 0}), secs));
            assert_eq!(none, want, "{base}: {none:016x}");
            assert_eq!(zero, want, "{base} rumble 0");
        }
    }

    const HASH_300: u64 = 0x5af5_d872_afe9_e8b6;
    const HASH_800: u64 = 0x4b63_e5a6_60da_b7e0;
    const HASH_LEN: u64 = 0xc595_7d04_50da_faa0;

    /// First sample above 1e-4 and the last above 1e-3, in samples.
    fn span(x: &[f32]) -> (usize, usize) {
        (
            x.iter().position(|v| v.abs() > 1e-4).unwrap(),
            x.iter().rposition(|v| v.abs() > 1e-3).unwrap(),
        )
    }

    /// Min and max of the 400 ms rms (dB) in windows between 40 % and 80 % of
    /// the audible span (hop 100 ms).
    fn mid_spread(x: &[f32]) -> (f32, f32) {
        let (a, e) = span(x);
        let (lo, hi) = (a + (e - a) * 4 / 10, a + (e - a) * 8 / 10);
        let w = SR as usize * 4 / 10;
        let mut s = lo;
        let (mut mn, mut mx) = (f32::MAX, f32::MIN);
        while s + w <= hi {
            let r = rms_db(&x[s..s + w]);
            mn = mn.min(r);
            mx = mx.max(r);
            s += SR as usize / 10;
        }
        (mn, mx)
    }

    #[test]
    fn rumble_makes_the_tail_continuous() {
        let mut worse = 0;
        for seed in [1u64, 3, 5, 8] {
            let base = json!({"distance": 1500, "seed": seed});
            let plain = thunder(base.clone(), json!({}), 16.0);
            let bed = thunder(base.clone(), json!({"rumble": 0.6}), 16.0);
            let (pa, pb) = mid_spread(&plain);
            let (ba, bb) = mid_spread(&bed);
            eprintln!(
                "seed {seed}: default {pa:.1}..{pb:.1} ({:.1} dB), rumble 0.6 {ba:.1}..{bb:.1} ({:.1} dB)",
                pb - pa,
                bb - ba
            );
            assert!(bb - ba < 6.0, "seed {seed}: spread {:.1} dB", bb - ba);
            if pb - pa > bb - ba {
                worse += 1;
            }
        }
        assert!(
            worse == 4,
            "the default varies more than the bed in every seed"
        );
    }

    #[test]
    fn rumble_stays_low_and_scales() {
        let base = flash();
        let plain = thunder(base.clone(), json!({}), 16.0);
        let mut prev = rms_db(&plain);
        for r in [0.3f32, 0.6, 1.0] {
            let x = thunder(base.clone(), json!({"rumble": r}), 16.0);
            let level = rms_db(&x);
            let sp = peaks(&x, SR, 1, 60.0)[0].0;
            let (lo, hi) = (
                band_db(&x, SR, 25.0, 90.0),
                band_db(&x, SR, 2000.0, 20000.0),
            );
            eprintln!(
                "rumble {r}: rms {level:.1} dB, peak bin {sp:.0} Hz, 25-90 Hz {lo:.1}, >2k {hi:.1}"
            );
            assert!(level > prev, "louder with more rumble: {level} vs {prev}");
            prev = level;
            assert!(lo > hi + 20.0, "low band dominates");
            // The spectral centroid, from the power spectrum.
            let s = spectrum(&x, SR);
            let (num, den) = s.iter().fold((0.0f64, 0.0f64), |(n, d), (hz, db)| {
                let p = 10f64.powf(f64::from(*db) / 10.0);
                (n + f64::from(*hz) * p, d + p)
            });
            let c = num / den;
            eprintln!("  centroid {c:.1} Hz");
            assert!(c < 100.0, "centroid {c}");
        }
    }

    #[test]
    fn rumble_is_seeded_and_finite() {
        let a = thunder(flash(), json!({"rumble": 0.6}), 16.0);
        let b = thunder(flash(), json!({"rumble": 0.6}), 16.0);
        assert_eq!(fnv(&a), fnv(&b));
        let c = thunder(
            json!({"distance": 1500, "seed": 4}),
            json!({"rumble": 0.6}),
            16.0,
        );
        assert_ne!(fnv(&a), fnv(&c));
        // Before the sound arrives nothing is added.
        assert!(rms_db(&a[..SR as usize * 4]) < -80.0);
        assert_bit_identical(
            vec![
                json!({"type": "thunder", "name": "t", "distance": 300, "rumble": 0.8, "seed": 2}),
            ],
            4.0,
        );
        for extra in [
            json!({"rumble": 1, "distance": 0}),
            json!({"rumble": 1, "distance": 30000, "exposure": 0}),
            json!({"rumble": 1, "length": 3, "brightness": 1}),
            json!({"rumble": 1, "length": 0.5, "brightness": 0, "amp": 4}),
            json!({"rumble": 1, "at": 50}),
            json!({"rumble": 0.01, "celsius": -60, "humidity": 0}),
        ] {
            let x = thunder(json!({}), extra.clone(), 3.0);
            assert_finite(&x, &extra.to_string());
        }
    }

    #[test]
    fn rumble_out_of_range_is_an_error() {
        for r in [json!(-0.1), json!(1.5)] {
            let d = {
                let mut d = sfx_graph::SoundDoc::named("t");
                d.nodes = vec![
                    json!({"type": "thunder", "name": "t", "rumble": r}),
                    json!({"type": "output", "name": "o", "input": "t"}),
                ];
                d.duration = Some(1.0);
                d
            };
            let e = d.render(&sfx_graph::RenderOpts::default()).err().unwrap();
            assert!(e.to_string().contains("rumble"), "{e}");
        }
    }
}
