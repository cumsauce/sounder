//! S7: `doppler`, `haas`, `decorrelate`. Each test measures and prints the
//! numbers that go in docs/nodes/motion.md.

#![allow(clippy::unwrap_used)] // tests

mod common;

use common::*;
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
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "m"}));
    d.duration = Some(0.5);
    d.render(&opts()).unwrap_err().to_string()
}

fn sine(hz: f32, amp: f32) -> Value {
    json!({"type": "osc", "name": "src", "freq": hz, "amp": amp})
}

fn node(extra: Value, ty: &str) -> Vec<Value> {
    let mut m = json!({"type": ty, "name": "m", "input": "src"});
    m.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![sine(1000.0, 0.5), m]
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
        sine(1000.0, 0.5),
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
        sine(1000.0, 0.5),
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
        sine(1000.0, 0.5),
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
        sine(1000.0, 0.5),
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
        sine(1000.0, 0.5),
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
        sine(1000.0, 0.5),
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
