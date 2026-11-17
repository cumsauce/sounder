//! The output stage: lufs / peak_db targets, the truthful missed-target
//! warning, normalizing up from very quiet, and the fade-in warning.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_graph::{Kind, RenderOpts, Rendered, SoundDoc};

fn doc(nodes: Vec<Value>, duration: Option<f32>) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.duration = duration;
    d
}

fn render(d: &SoundDoc) -> Result<Rendered, String> {
    d.render(&RenderOpts::default())
}

fn ok(d: &SoundDoc) -> Rendered {
    render(d).unwrap_or_else(|e| panic!("{e}"))
}

/// FNV-1a over the bit patterns of every sample, channel by channel.
fn hash(r: &Rendered) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in &r.audio.channels {
        for x in c {
            for b in x.to_bits().to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    h
}

/// The `laser` example's graph (examples/laser.rs).
fn laser() -> SoundDoc {
    doc(
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
    let d = doc(
        vec![
            json!({"type": "impulse", "name": "i"}),
            json!({"type": "output", "name": "out", "input": "i", "lufs": -14}),
        ],
        Some(0.5),
    );
    let d = ok(&d);
    assert_eq!(hash(&a), 0x43ce_d14c_0173_460a, "laser lufs -18");
    assert_eq!(
        hash(&b),
        0x729c_2d96_e9f7_2090,
        "laser gain / fades / ceiling"
    );
    assert_eq!(hash(&c), 0xc7a9_1213_d063_39b9, "laser bare output");
    assert_eq!(hash(&d), 0x5eac_53d2_74f9_ef25, "impulse missing lufs -14");
}

fn noise_at(db: f32, output: Value, duration: f32) -> SoundDoc {
    doc(
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
    doc(
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
    let d =
        train(json!({"type": "output", "name": "out", "input": "i", "peak_db": -3, "lufs": -20}));
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
fn a_miss_with_a_wide_peak_margin_does_not_blame_the_ceiling() {
    // 1 s of noise at -35 LUFS peaks about 20 dB under the ceiling; the
    // 0.45 s fades at both ends are what move the loudness.
    let out = json!({"type": "output", "name": "out", "input": "q", "lufs": -35,
        "fade_in": 0.45, "fade_out": 0.45});
    let r = ok(&noise_at(-30.0, out, 1.0));
    let w = has(&r, "output lufs").unwrap_or_else(|| panic!("{:?}", r.warnings));
    println!("{w} (final {:.1} LUFS, peak {:.1})", r.lufs, r.peak_db);
    assert!(r.peak_db < -15.0, "peak {}", r.peak_db);
    assert!(!w.contains("ceiling"), "{w}");
    assert!(w.contains("fades"), "{w}");
}

#[test]
fn a_hot_miss_that_the_limiter_caused_says_so() {
    // A sine (crest 3 dB) at 0 LUFS needs peaks over 0 dBFS.
    let d = doc(
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
    let d = doc(
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
    let d = doc(
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
    ok(&doc(
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
        w.contains("default 2 ms fade-in") && w.contains("fade_in 0"),
        "{w}"
    );
    assert_eq!(
        r.warnings.iter().filter(|w| w.contains("fade-in")).count(),
        1
    );

    // Same graph with the noise burst delayed: use a pure impulse at 5 ms.
    let late = ok(&doc(
        vec![
            json!({"type": "impulse", "name": "i", "at": [0.005]}),
            json!({"type": "output", "name": "out", "input": "i"}),
        ],
        Some(0.2),
    ));
    assert!(has(&late, "fade-in").is_none(), "{:?}", late.warnings);
    let early = ok(&doc(
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
    let mut d = doc(
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
