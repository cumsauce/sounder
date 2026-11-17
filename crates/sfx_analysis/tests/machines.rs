//! M2: `engine` and `motor`. Each card assertion is a test that prints what
//! it measured (those lines are in docs/nodes/machines.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_graph::node::parse_body;
use sfx_graph::{Measure, RenderOpts, SoundDoc};

/// An `engine` named `eng` with extra fields.
fn engine(extra: Value) -> Vec<Value> {
    let mut e = json!({"type": "engine", "name": "eng", "rpm": 3000});
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![e]
}

/// A `motor` named `mot` with extra fields.
fn motor(extra: Value) -> Vec<Value> {
    let mut m = json!({"type": "motor", "name": "mot", "rpm": 3000});
    m.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![m]
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.duration = Some(0.1);
    let last = d.nodes.last().unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": last}));
    d.render(&RenderOpts::default()).unwrap_err()
}

/// The engine's pitch as a listener would name it: the lowest spectral peak
/// above 30 Hz that is within 15 dB of the strongest (the strongest line may
/// be the second harmonic; the cylinder-to-cylinder level errors put lines
/// 25 dB or more below at the cycle rate).
fn pitch(x: &[f32]) -> f32 {
    peaks(x, SR, 40, 15.0)
        .into_iter()
        .map(|p| p.0)
        .filter(|f| *f > 30.0)
        .fold(f32::MAX, f32::min)
}

/// The whole-sound spectral centroid and the rendered level of `nodes`.
fn centroid_and_rms(nodes: &[Value], name: &str, secs: f32) -> (f32, f32) {
    let x = node_audio(nodes, name, secs);
    let audio = sfx_graph::Audio {
        sample_rate: SR,
        channels: vec![x.clone()],
    };
    (measure(&audio, Measure::Centroid), rms_db(&x))
}

fn within(got: f32, want: f32, pct: f32) -> bool {
    (got - want).abs() <= want.abs() * pct / 100.0
}

// ------------------------------------------------------------------ engine

#[test]
fn engine_pitch_is_the_firing_frequency() {
    // (rpm, cylinders, stroke): f = rpm / 60 * cylinders / (stroke / 2).
    let cases = [
        (1800.0, 4, 4),
        (3000.0, 4, 4),
        (4800.0, 4, 4),
        (3000.0, 6, 4),
        (3000.0, 8, 4),
        (3000.0, 4, 2),
        (3000.0, 1, 2),
        (2400.0, 3, 4),
    ];
    let mut worst = 0.0f32;
    for (rpm, cyl, stroke) in cases {
        let x = node_audio(
            engine(json!({"rpm": rpm, "cylinders": cyl, "stroke": stroke})),
            "eng",
            2.0,
        );
        assert_finite(&x, "eng");
        let want = rpm / 60.0 * cyl as f32 / (stroke as f32 / 2.0);
        let got = pitch(&x);
        let err = (got - want).abs() / want * 100.0;
        worst = worst.max(err);
        println!(
            "rpm {rpm} cylinders {cyl} stroke {stroke}: want {want:.2} Hz, pitch {got:.2} Hz ({err:.2} %)"
        );
        assert!(err < 2.0, "{got} vs {want}");
    }
    println!("worst pitch error {worst:.2} %");
}

#[test]
fn engine_pitch_doubles_with_cylinders_and_two_stroke() {
    let p = |extra: Value| pitch(&node_audio(engine(extra), "eng", 2.0));
    let (c4, c8) = (p(json!({"cylinders": 4})), p(json!({"cylinders": 8})));
    let (s4, s2) = (p(json!({"stroke": 4})), p(json!({"stroke": 2})));
    println!(
        "cylinders 4 -> 8: {c4:.2} -> {c8:.2} Hz (ratio {:.3}); stroke 4 -> 2: {s4:.2} -> {s2:.2} Hz (ratio {:.3})",
        c8 / c4,
        s2 / s4
    );
    assert!(within(c8 / c4, 2.0, 2.0) && within(s2 / s4, 2.0, 2.0));
}

#[test]
fn engine_ramped_rpm_gives_a_rising_pitch_track() {
    let nodes = vec![
        json!({"type": "ramp", "name": "revs", "from": 1800, "to": 5400, "time": 2.0, "curve": "linear"}),
        json!({"type": "engine", "name": "eng", "rpm": "revs"}),
    ];
    let x = node_audio(&nodes, "eng", 2.0);
    assert_finite(&x, "eng");
    // Seven 0.5 s windows centred 0.25 s apart; the ramp is linear, so the
    // rpm at a window's centre is its mean rpm.
    let win = SR as usize / 2;
    let mut track = Vec::new();
    for k in 0..7 {
        let lo = k * win / 2;
        let hz = pitch(&x[lo..lo + win]);
        let centre = (lo + win / 2) as f32 / SR as f32;
        let rpm = 1800.0 + 3600.0 * centre / 2.0;
        track.push((centre, hz, rpm / 60.0 * 2.0));
    }
    for (t, hz, want) in &track {
        println!(
            "t {t:.2} s: pitch {hz:.1} Hz, rpm-implied {want:.1} Hz ({:.1} %)",
            (hz - want).abs() / want * 100.0
        );
    }
    assert!(
        track.windows(2).all(|w| w[1].1 > w[0].1 + 2.0),
        "rising: {track:?}"
    );
    let first = track.first().unwrap();
    let last = track.last().unwrap();
    println!(
        "pitch track {:.1} -> {:.1} Hz over the ramp (ratio {:.2}; rpm ratio {:.2})",
        first.1,
        last.1,
        last.1 / first.1,
        last.2 / first.2
    );
    for (_, hz, want) in &track {
        assert!(within(*hz, *want, 3.0), "{hz} vs {want}");
    }
}

#[test]
fn engine_load_raises_level_and_centroid() {
    let mut rows = Vec::new();
    for load in [0.1, 0.5, 0.9] {
        let (c, r) = centroid_and_rms(&engine(json!({"load": load})), "eng", 2.0);
        println!("load {load}: centroid {c:.1} Hz, rms {r:.2} dB");
        rows.push((c, r));
    }
    assert!(rows.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1));
}

#[test]
fn engine_load_signal_changes_the_sound_over_time() {
    let nodes = vec![
        json!({"type": "ramp", "name": "throttle", "from": 0.05, "to": 1.0, "time": 2.0}),
        json!({"type": "engine", "name": "eng", "rpm": 3000, "load": "throttle"}),
    ];
    let x = node_audio(&nodes, "eng", 2.0);
    let h = x.len() / 2;
    let (a, b) = (rms_db(&x[..h]), rms_db(&x[h..]));
    println!("load ramp 0.05 -> 1: first half rms {a:.2} dB, second half {b:.2} dB");
    assert!(b > a + 3.0);
}

#[test]
fn engine_exhaust_pipe_echo_arrives_after_the_round_trip() {
    // One cylinder at 3000 rpm, 55 ms: only the first combustion pulse (at
    // 20 ms, 4.2 ms wide) is in it. The pipe is open at one end, so its echo
    // is the pulse inverted, 2 L / c later. The most negative sample after
    // the pulse marks the echo; moving the pipe from L1 to L2 must move it
    // by 2 (L2 - L1) / 343 s.
    // The pulse (19 to 26 ms, before any echo for pipes of 1.6 m and up) is
    // the template. Both have a centred 10 ms moving average subtracted
    // first, which removes the DC blocker's slow sag. The lag of the most negative correlation with the
    // rest of the render is the echo's delay.
    let ms = |t: f32| (t * 0.001 * SR as f32) as usize;
    let mut found = Vec::new();
    for length in [1.6, 2.4, 3.2] {
        let raw = node_audio(
            engine(json!({"cylinders": 1, "exhaust_length": length, "seed": 3})),
            "eng",
            0.055,
        );
        let half = ms(5.0);
        let x: Vec<f32> = (0..raw.len())
            .map(|i| {
                let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(raw.len()));
                raw[i] - raw[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            })
            .collect();
        let t = &x[ms(19.0)..ms(26.0)];
        let corr = |lag: usize| -> f32 {
            t.iter()
                .enumerate()
                .map(|(k, v)| v * x[ms(19.0) + lag + k])
                .sum()
        };
        let lags = ms(5.0)..(x.len() - ms(26.0));
        let best = lags
            .clone()
            .min_by(|a, b| corr(*a).total_cmp(&corr(*b)))
            .unwrap();
        let delay = best as f32 / SR as f32 * 1000.0;
        let round_trip = 2.0 * length / 343.0 * 1000.0;
        println!(
            "exhaust_length {length} m: most negative correlation at lag {delay:.2} ms, round trip 2L/c = {round_trip:.2} ms"
        );
        // Measured 0.33, 0.27 and 0.03 ms early (the template's edges and
        // the moving average pull the minimum a little): allow 0.4 ms.
        assert!((delay - round_trip).abs() < 0.4, "{delay} vs {round_trip}");
        found.push(delay);
    }
    assert!(found.windows(2).all(|w| w[1] > w[0]));
}

#[test]
fn engine_seed_changes_the_cycle_jitter_not_the_pitch() {
    let a = node_audio(engine(json!({"seed": 1})), "eng", 2.0);
    let b = node_audio(engine(json!({"seed": 2})), "eng", 2.0);
    let c = node_audio(engine(json!({"seed": 1})), "eng", 2.0);
    assert!(a.iter().zip(&c).all(|(p, q)| p.to_bits() == q.to_bits()));
    let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
    let (pa, pb) = (pitch(&a), pitch(&b));
    println!(
        "seeds 1 and 2: {diff} of {} samples differ; pitch {pa:.2} and {pb:.2} Hz; rms {:.2} and {:.2} dB",
        a.len(),
        rms_db(&a),
        rms_db(&b)
    );
    assert!(diff > a.len() / 2);
    assert!(within(pa, pb, 1.0));
    assert!((rms_db(&a) - rms_db(&b)).abs() < 1.0);
}

#[test]
fn engine_rpm_zero_is_silent() {
    let x = node_audio(engine(json!({"rpm": 0})), "eng", 0.5);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!("rpm 0: peak {peak}");
    assert!(peak == 0.0);
}

#[test]
fn engine_is_bit_identical() {
    assert_bit_identical(engine(json!({"seed": 5, "load": 0.8})), 0.5);
    let nodes = vec![
        json!({"type": "ramp", "name": "revs", "from": 800, "to": 6000, "time": 0.5}),
        json!({"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "stroke": 2}),
    ];
    assert_bit_identical(nodes, 0.5);
}

#[test]
fn engine_is_finite_at_the_extremes() {
    let mut worst = 0.0f32;
    for rpm in [0.0, 1.0, 20_000.0] {
        for cylinders in [1, 16] {
            for stroke in [2, 4] {
                for load in [0.0, 1.0] {
                    for length in [0.05, 10.0] {
                        let x = node_audio(
                            engine(
                                json!({"rpm": rpm, "cylinders": cylinders, "stroke": stroke, "load": load, "exhaust_length": length}),
                            ),
                            "eng",
                            0.3,
                        );
                        let what = format!(
                            "rpm {rpm} cyl {cylinders} stroke {stroke} load {load} L {length}"
                        );
                        assert_finite(&x, &what);
                        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        worst = worst.max(peak);
                        assert!(peak <= 1.0, "{what}: peak {peak}");
                    }
                }
            }
        }
    }
    // Signals far outside the range are clamped, not NaN.
    let nodes = vec![
        json!({"type": "ramp", "name": "r", "from": -1.0e9, "to": 1.0e9, "time": 0.3}),
        json!({"type": "ramp", "name": "l", "from": -50, "to": 50, "time": 0.3}),
        json!({"type": "engine", "name": "eng", "rpm": "r", "load": "l", "cylinders": 16, "stroke": 2}),
    ];
    let x = node_audio(&nodes, "eng", 0.3);
    assert_finite(&x, "signals out of range");
    println!(
        "extremes: 96 settings of rpm/cylinders/stroke/load/length and a +-1e9 rpm sweep: all finite, peak at most {worst:.3}"
    );
}

#[test]
fn engine_errors_say_the_field_value_and_fix() {
    let cases = [
        (
            json!({"cylinders": 0}),
            "cylinders: 0: give a whole number from 1 to 16",
        ),
        (
            json!({"cylinders": 20}),
            "cylinders: 20: give a whole number from 1 to 16",
        ),
        (json!({"stroke": 3}), "stroke: 3: give 2 or 4"),
        (
            json!({"exhaust_length": 11.0}),
            "exhaust_length: 11: give metres from 0.05 to 10",
        ),
        (
            json!({"exhaust_length": 0.0}),
            "exhaust_length: 0: give a number > 0",
        ),
        (
            json!({"rpm": 30000}),
            "rpm: 30000: give a number from 0 to 20000",
        ),
        (json!({"load": 1.5}), "load: 1.5: give a number from 0 to 1"),
    ];
    for (extra, want) in cases {
        let e = render_error(engine(extra));
        println!("{e}");
        assert!(e.contains(want), "{e}");
    }
    let e = render_error(engine(json!({"cyl": 4})));
    assert!(e.contains("cyl") && e.contains("cylinders"), "{e}");
}

// ------------------------------------------------------------------- motor

#[test]
fn motor_peaks_at_the_gear_mesh_frequency() {
    let mut worst = 0.0f32;
    for (rpm, teeth) in [
        (1800.0, 20),
        (3000.0, 20),
        (6000.0, 20),
        (3000.0, 10),
        (3000.0, 40),
    ] {
        let x = node_audio(motor(json!({"rpm": rpm, "gear_teeth": teeth})), "mot", 1.0);
        assert_finite(&x, "mot");
        let want = rpm / 60.0 * teeth as f32;
        let top = peaks(&x, SR, 1, 0.0)[0];
        let err = (top.0 - want).abs() / want * 100.0;
        worst = worst.max(err);
        println!(
            "rpm {rpm} teeth {teeth}: mesh {want:.1} Hz, strongest peak {:.2} Hz ({err:.3} %, {:.1} dB)",
            top.0, top.1
        );
        assert!(err < 1.0, "{} vs {want}", top.0);
    }
    println!("worst mesh-frequency error {worst:.3} %");
}

#[test]
fn motor_mesh_peak_follows_a_ramp() {
    let nodes = vec![
        json!({"type": "ramp", "name": "revs", "from": 1500, "to": 4500, "time": 2.0, "curve": "linear"}),
        json!({"type": "motor", "name": "mot", "rpm": "revs", "gear_teeth": 20}),
    ];
    let x = node_audio(&nodes, "mot", 2.0);
    assert_finite(&x, "mot");
    let win = SR as usize / 5;
    let mut track = Vec::new();
    for k in 0..8 {
        let lo = k * 12_000;
        let seg = &x[lo..lo + win];
        let centre = (lo + win / 2) as f32 / SR as f32;
        let want = (1500.0 + 3000.0 * centre / 2.0) / 60.0 * 20.0;
        // The strongest peak above 300 Hz: the hum's lines are below.
        let top = peaks(seg, SR, 12, 60.0)
            .into_iter()
            .filter(|p| (300.0..3000.0).contains(&p.0))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        println!(
            "t {centre:.2} s: strongest peak above 300 Hz {:.1} Hz, mesh {want:.1} Hz",
            top.0
        );
        track.push((top.0, want));
    }
    assert!(track.windows(2).all(|w| w[1].0 > w[0].0));
    // Measured at most 1.2 % off (a 0.2 s window spans a 10 % sweep): allow 3 %.
    for (got, want) in &track {
        assert!(within(*got, *want, 3.0), "{got} vs {want}");
    }
}

#[test]
fn motor_hum_follows_rpm_and_poles() {
    // No gearbox, no whine: the strongest line is the electrical rate.
    let mut worst = 0.0f32;
    for (rpm, poles) in [
        (3000.0, 2),
        (3000.0, 4),
        (3000.0, 8),
        (1800.0, 4),
        (6000.0, 4),
    ] {
        let x = node_audio(
            motor(json!({"rpm": rpm, "poles": poles, "whine": 0})),
            "mot",
            1.0,
        );
        let want = rpm / 60.0 * poles as f32 / 2.0;
        let top = peaks(&x, SR, 1, 0.0)[0];
        let err = (top.0 - want).abs() / want * 100.0;
        worst = worst.max(err);
        println!(
            "rpm {rpm} poles {poles}: electrical {want:.1} Hz, strongest peak {:.2} Hz ({err:.3} %)",
            top.0
        );
        assert!(err < 1.0);
    }
    println!("worst hum error {worst:.3} %");
}

#[test]
fn motor_whine_sets_the_carrier_level_with_sidebands() {
    let mut rows = Vec::new();
    for whine in [0.0, 0.3, 1.0] {
        let x = node_audio(motor(json!({"whine": whine, "carrier": 4000})), "mot", 1.0);
        let carrier = band_db(&x, SR, 3990.0, 4010.0);
        rows.push(carrier);
        println!("whine {whine}: 4 kHz band {carrier:.1} dB");
    }
    assert!(rows[1] > rows[0] + 6.0 && rows[2] > rows[1] + 6.0);
    // Sidebands: 3000 rpm, 4 poles -> electrical 100 Hz, so 3800 and 4200
    // (2x) and 3600 and 4400 (4x).
    let x = node_audio(motor(json!({"whine": 1.0, "carrier": 4000})), "mot", 1.0);
    let band = |c: f32| band_db(&x, SR, c - 10.0, c + 10.0);
    println!(
        "whine 1 sidebands: 4000 {:.1}, 4000 +-200 Hz {:.1} / {:.1}, 4000 +-400 Hz {:.1} / {:.1} dB",
        band(4000.0),
        band(3800.0),
        band(4200.0),
        band(3600.0),
        band(4400.0)
    );
    assert!(band(3800.0) > band(4000.0) - 15.0 && band(4200.0) > band(4000.0) - 15.0);
    assert!(band(3600.0) > band(4000.0) - 25.0 && band(4400.0) > band(4000.0) - 25.0);
    assert!(
        band(3900.0) < band(3800.0) - 20.0,
        "nothing between the lines"
    );
}

#[test]
fn motor_load_raises_level_and_centroid() {
    let mut rows = Vec::new();
    for load in [0.1, 0.5, 0.9] {
        let (c, r) = centroid_and_rms(&motor(json!({"load": load, "gear_teeth": 20})), "mot", 1.0);
        println!("load {load}: centroid {c:.1} Hz, rms {r:.2} dB");
        rows.push((c, r));
    }
    assert!(rows.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1));
}

#[test]
fn motor_rpm_zero_is_silent() {
    let x = node_audio(motor(json!({"rpm": 0, "gear_teeth": 20})), "mot", 0.5);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!("rpm 0: peak {peak}");
    assert!(peak == 0.0);
}

#[test]
fn motor_is_bit_identical() {
    assert_bit_identical(motor(json!({"gear_teeth": 17, "seed": 3})), 0.5);
}

#[test]
fn motor_is_finite_at_the_extremes() {
    let mut worst = 0.0f32;
    for rpm in [0.0, 1.0, 20_000.0] {
        for poles in [2, 64] {
            for teeth in [0, 1, 400] {
                for load in [0.0, 1.0] {
                    for whine in [0.0, 1.0] {
                        let x = node_audio(
                            motor(
                                json!({"rpm": rpm, "poles": poles, "gear_teeth": teeth, "load": load, "whine": whine, "carrier": 21600}),
                            ),
                            "mot",
                            0.2,
                        );
                        let what = format!(
                            "rpm {rpm} poles {poles} teeth {teeth} load {load} whine {whine}"
                        );
                        assert_finite(&x, &what);
                        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        worst = worst.max(peak);
                        assert!(peak <= 1.5, "{what}: peak {peak}");
                    }
                }
            }
        }
    }
    let nodes = vec![
        json!({"type": "ramp", "name": "r", "from": -1.0e9, "to": 1.0e9, "time": 0.3}),
        json!({"type": "motor", "name": "mot", "rpm": "r", "gear_teeth": 400, "poles": 64}),
    ];
    assert_finite(&node_audio(&nodes, "mot", 0.3), "signal out of range");
    println!("extremes: 144 settings and a +-1e9 rpm sweep: all finite, peak at most {worst:.3}");
}

#[test]
fn motor_errors_say_the_field_value_and_fix() {
    let cases = [
        (
            json!({"poles": 3}),
            "poles: 3: give an even number from 2 to 64",
        ),
        (
            json!({"poles": 0}),
            "poles: 0: give an even number from 2 to 64",
        ),
        (
            json!({"gear_teeth": 401}),
            "gear_teeth: 401: give 0 to 400 teeth",
        ),
        (
            json!({"carrier": 100}),
            "carrier: 100: give Hz from 200 to 21600",
        ),
        (json!({"rpm": -5}), "rpm: -5: give a number from 0 to 20000"),
        (json!({"whine": 2}), "whine: 2: give a number from 0 to 1"),
    ];
    for (extra, want) in cases {
        let e = render_error(motor(extra));
        println!("{e}");
        assert!(e.contains(want), "{e}");
    }
}

// ---------------------------------------------------------------- registry

#[test]
fn machines_are_registered_and_random() {
    for ty in ["engine", "motor"] {
        let mut body = sfx_graph::example(ty).expect("registered");
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, "x").unwrap();
        assert!(node.random(), "{ty} reads the seed");
        let text = sfx_graph::describe(Some(ty)).unwrap();
        assert!(text.contains("rpm"));
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
}

/// The recipe in docs/nodes/machines.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "ramp", "name": "revs", "from": 1200, "to": 5200, "time": 2.0, "curve": "exp"}),
        json!({"type": "ramp", "name": "throttle", "from": 0.2, "to": 0.9, "time": 2.0}),
        json!({"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "load": "throttle", "exhaust_length": 1.5, "seed": 1}),
        json!({"type": "filter", "name": "body", "input": "eng", "mode": "lp", "freq": 6000}),
    ]
}

#[test]
fn machines_recipe_renders() {
    let a = render(recipe(), 2.0);
    let x = node_audio(recipe(), "body", 2.0);
    assert_finite(&x, "body");
    let (h1, h2) = (&x[..x.len() / 2], &x[x.len() / 2..]);
    println!(
        "recipe: {} samples, rms {:.2} dB (first half {:.2}, second {:.2})",
        a.len(),
        rms_db(&x),
        rms_db(h1),
        rms_db(h2)
    );
    assert_eq!(a.len(), 2 * SR as usize);
    assert!(rms_db(h2) > rms_db(h1));
}
