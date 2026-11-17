//! `crossfade_loop`, `resample`, `tape` and `stutter`: each test measures
//! something and prints it (these lines go in docs/nodes/S12.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_analysis::{CheckCx, Expect, run};
use sfx_graph::node::parse_body;
use sfx_graph::{Kind, RenderOpts, SoundDoc};

/// The error a document with these nodes fails to render with (the last
/// node is read by the output).
fn render_error(nodes: Vec<Value>, duration: f32) -> String {
    let mut d = SoundDoc::named("test");
    let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": last}));
    d.duration = Some(duration);
    match d.render(&RenderOpts::default()) {
        Err(e) => e,
        Ok(_) => panic!("rendered, but an error was expected"),
    }
}

// ------------------------------------------------------------ crossfade_loop

/// A 2 s loop document of `nodes` (the last is read by the output).
fn loop_doc(nodes: &[Value]) -> SoundDoc {
    let mut d = SoundDoc::named("loop");
    d.kind = Kind::Loop;
    d.duration = Some(2.0);
    d.nodes = nodes.to_vec();
    let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": last}));
    d
}

/// (loop_seam pass, its detail, names of the failing default checks, output
/// samples).
fn seam(d: &SoundDoc) -> (bool, String, Vec<String>, Vec<f32>) {
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap();
    let res = run(
        &Expect::defaults_for(d),
        &CheckCx {
            doc: d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap();
    let s = res.iter().find(|c| c.name == "loop_seam").unwrap();
    (
        s.pass,
        s.detail.clone(),
        res.iter()
            .filter(|c| !c.pass)
            .map(|c| c.name.clone())
            .collect(),
        r.audio.channels[0].clone(),
    )
}

fn wander_noise(rate: f32, seed: u64) -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "wander", "name": "fq", "rate": rate, "range": [300, 3000], "seed": seed}),
        json!({"type": "filter", "name": "bp", "input": "n", "mode": "bp", "freq": "fq", "q": 3}),
        json!({"type": "gain", "name": "lev", "input": "bp", "gain": 0.5}),
    ]
}

fn with_xfade(mut nodes: Vec<Value>, fade: Value) -> Vec<Value> {
    let last = nodes.last().unwrap()["name"].as_str().unwrap().to_string();
    let mut x = json!({"type": "crossfade_loop", "name": "xf", "input": last});
    if !fade.is_null() {
        x["fade"] = fade;
    }
    nodes.push(x);
    nodes
}

fn rms(x: &[f32]) -> f32 {
    rms_db(x)
}

/// The 2 s loops used below: brown noise at many seeds (a low-passed noise
/// whose end is rarely near its start) and slowly modulated band-passed
/// noise (a `wander` moves the centre frequency).
#[test]
fn crossfade_loop_makes_noise_loops_pass_loop_seam() {
    // Brown noise, 40 seeds.
    let brown = vec![json!({"type": "noise", "name": "n", "color": "brown"})];
    let (mut fail_before, mut fail_after, mut worst_level) = (0, 0, 0.0f32);
    let mut dc_new = 0;
    for seed in 1..=40u64 {
        let mut d = loop_doc(&brown);
        d.seed = seed;
        let before = seam(&d);
        let mut d = loop_doc(&with_xfade(brown.clone(), Value::Null));
        d.seed = seed;
        let after = seam(&d);
        fail_before += usize::from(!before.0);
        fail_after += usize::from(!after.0);
        assert!(after.0, "brown seed {seed}: {}", after.1);
        // No other default check is newly failed, except `dc`: brown noise
        // is mostly sub-audio, its mean is whatever the random walk made, and
        // the fade moves the mean (see the doc's limits).
        assert!(
            after.2.iter().all(|c| c == "dc" || before.2.contains(c)),
            "{:?} vs {:?}",
            after.2,
            before.2
        );
        dc_new += usize::from(
            after.2.contains(&"dc".to_string()) && !before.2.contains(&"dc".to_string()),
        );
        worst_level = worst_level.max((rms(&before.3) - rms(&after.3)).abs());
    }
    println!(
        "brown noise, 40 seeds, 2 s loop, default fade: {fail_before} fail loop_seam before, {fail_after} after; worst whole-sound rms change {worst_level:.2} dB; `dc` newly fails on {dc_new}"
    );
    assert!(fail_before >= 10 && fail_after == 0);
    assert!(worst_level < 0.5);

    // White and pink noise already pass (measured: 0 of 60 seeds fail); they
    // still must after.
    for color in ["white", "pink"] {
        let n = vec![json!({"type": "noise", "name": "n", "color": color})];
        let after = seam(&loop_doc(&with_xfade(n, Value::Null)));
        assert!(
            after.0 && after.2.is_empty(),
            "{color}: {} {:?}",
            after.1,
            after.2
        );
    }

    // A slowly modulated filtered noise (one new centre frequency every
    // 3.3 s, 200 to 4000 Hz, q 3): this one fails before.
    let mut wind = wander_noise(0.3, 2);
    wind[2]["q"] = json!(3);
    wind[1]["range"] = json!([200, 4000]);
    let before = seam(&loop_doc(&wind));
    let after = seam(&loop_doc(&with_xfade(wind.clone(), Value::Null)));
    println!(
        "wander filter noise, rate 0.3 /s:\n  before: {}\n  after:  {}",
        before.1, after.1
    );
    assert!(!before.0, "this loop is meant to fail before");
    assert!(after.0 && after.2.is_empty(), "{} {:?}", after.1, after.2);
    // Level away from the seam: the middle is the input, bit for bit, and
    // the whole sound's level moves by under 0.5 dB.
    let f = (0.25 * SR as f32) as usize;
    let n = before.3.len();
    assert!(
        before.3[f..n - f]
            .iter()
            .zip(&after.3[f..n - f])
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    let (rb, ra) = (rms(&before.3), rms(&after.3));
    println!(
        "  whole-sound rms {rb:.2} -> {ra:.2} dB; middle (not within 0.25 s of the ends) identical"
    );
    assert!((rb - ra).abs() < 0.5);

    // A sweep of wander-filter loops (200 to 4000 Hz, q 3 and 8, 12 seeds
    // each, 2 s). The spectral half of loop_seam compares the first and the
    // last 0.34 s, so a filter that moves faster than that fails whatever
    // the node does; the doc has the full table.
    let mut table = Vec::new();
    for (rate, fade) in [(0.3f32, 0.25f32), (0.5, 0.25), (0.5, 0.5), (1.0, 0.5)] {
        let (mut before, mut after, mut total) = (0, 0, 0);
        for q in [3.0f32, 8.0] {
            for seed in 1..=12u64 {
                let mut w = wander_noise(rate, seed);
                w[2]["q"] = json!(q);
                w[1]["range"] = json!([200, 4000]);
                before += usize::from(!seam(&loop_doc(&w)).0);
                after += usize::from(!seam(&loop_doc(&with_xfade(w, json!(fade)))).0);
                total += 1;
            }
        }
        println!(
            "wander loops, {rate} targets/s, fade {fade}: {before}/{total} fail loop_seam before, {after}/{total} after"
        );
        table.push((rate, fade, before, after));
    }
    assert_eq!(table[0].3, 0, "{table:?}");
    assert!(table[0].2 >= 6, "{table:?}");
}

#[test]
fn crossfade_loop_is_equal_power_and_wraps_into_the_neighbouring_sample() {
    let n = 2 * SR as usize;
    let f = (0.25 * SR as f32) as usize;
    let nodes = vec![
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "crossfade_loop", "name": "xf", "input": "n", "fade": 0.25}),
    ];
    let raw = node_audio(&nodes[..1], "n", 2.0);
    let y = node_audio(&nodes, "xf", 2.0);
    assert_eq!(y.len(), n);
    // The wrap: y[n-1] is the sample just before y[0]'s source.
    assert_eq!(y[n - 1], raw[n - f - 1]);
    assert_eq!(y[0], raw[n - f]);
    let (mid, head, tail) = (rms(&y[f..n - f]), rms(&y[..f]), rms(&y[n - f..]));
    // Uncorrelated halves of white noise keep their power through the fade.
    println!(
        "white noise, fade 0.25 s: rms middle {mid:.2} dB, first 0.25 s {head:.2} dB, last 0.25 s {tail:.2} dB; y[0] = x[n-F], y[n-1] = x[n-F-1] exactly"
    );
    assert!((head - mid).abs() < 0.7 && (tail - mid).abs() < 0.7);
    // Different fade lengths are all wrap-safe (the end of the sound plays
    // on into its start).
    for fade in [0.001, 0.05, 1.0] {
        let nodes = vec![
            json!({"type": "noise", "name": "n", "color": "white"}),
            json!({"type": "crossfade_loop", "name": "xf", "input": "n", "fade": fade}),
        ];
        let y = node_audio(&nodes, "xf", 2.0);
        let f = (fade * SR as f32).round() as usize;
        assert_eq!(y[0], raw[n - f], "fade {fade}");
        assert_eq!(y[n - 1], raw[n - f - 1], "fade {fade}");
        assert_finite(&y, "xf");
    }
}

#[test]
fn crossfade_loop_stereo_is_bit_identical_finite_and_errors_explain() {
    let nodes = vec![
        json!({"type": "noise", "name": "n", "color": "pink"}),
        json!({"type": "pan", "name": "st", "input": "n", "pan": 0.6}),
        json!({"type": "crossfade_loop", "name": "xf", "input": "st"}),
    ];
    let a = render(&nodes, 1.0);
    assert_eq!(a.channels.len(), 2);
    assert_bit_identical(&nodes, 1.0);
    // Extremes: the longest fade (half the sound) and the shortest (2 samples).
    for fade in [0.5, 2.0 / SR as f32] {
        let mut nodes = nodes.clone();
        nodes[2]["fade"] = json!(fade);
        let a = render(&nodes, 1.0);
        for c in &a.channels {
            assert_finite(c, "xf");
        }
    }
    let loud = vec![
        json!({"type": "noise", "name": "n", "color": "white", "amp": 1.0e6}),
        json!({"type": "crossfade_loop", "name": "xf", "input": "n", "fade": 0.5}),
    ];
    let y = node_audio(&loud, "xf", 1.0);
    assert_finite(&y, "xf loud");
    for (fade, want) in [
        (json!(0), "fade: 0: give seconds more than 0"),
        (json!(0.6), "give at most 0.500 s"),
        (json!(0.00001), "under 2 samples"),
    ] {
        let mut nodes = vec![
            json!({"type": "noise", "name": "n"}),
            json!({"type": "crossfade_loop", "name": "xf", "input": "n"}),
        ];
        nodes[1]["fade"] = fade;
        let e = render_error(nodes, 1.0);
        println!("crossfade_loop error: {e}");
        assert!(e.contains(want), "{e}");
    }
}

// ----------------------------------------------------------------- resample

fn sine(hz: f32, amp: f32) -> Value {
    json!({"type": "osc", "name": "src", "freq": hz, "amp": amp})
}

fn resampled(hz: f32, rate: Value, anti: bool) -> Vec<f32> {
    let nodes = vec![
        sine(hz, 0.5),
        json!({"type": "resample", "name": "rs", "input": "src", "rate": rate, "antialias": anti}),
    ];
    node_audio(nodes, "rs", 1.0)
}

#[test]
fn resample_hold_makes_images_and_antialias_removes_them() {
    let raw = resampled(1000.0, json!(8000), false);
    let clean = resampled(1000.0, json!(8000), true);
    let rel = |x: &[f32], lo: f32, hi: f32| band_db(x, SR, lo, hi) - band_db(x, SR, 990.0, 1010.0);
    let (i7, i9) = (rel(&raw, 6990.0, 7010.0), rel(&raw, 8990.0, 9010.0));
    let (c7, c9) = (rel(&clean, 6990.0, 7010.0), rel(&clean, 8990.0, 9010.0));
    let fund = band_db(&clean, SR, 990.0, 1010.0) - band_db(&raw, SR, 990.0, 1010.0);
    println!(
        "1 kHz sine, rate 8000: images 7 kHz {i7:.1} dB, 9 kHz {i9:.1} dB re 1 kHz; with antialias {c7:.1} / {c9:.1} dB ({:.1} / {:.1} dB lower); 1 kHz level changes {fund:.2} dB",
        i7 - c7,
        i9 - c9
    );
    // The hold's sinc roll-off puts the images near -17 dB; they are there.
    assert!(i7 > -25.0 && i9 > -25.0);
    assert!(i7 - c7 >= 30.0 && i9 - c9 >= 30.0);
    assert!(fund.abs() < 1.0);
    // A 7 kHz sine sampled at 8 kHz folds to 1 kHz; the input filter stops it.
    let fold = resampled(7000.0, json!(8000), false);
    let folded_clean = resampled(7000.0, json!(8000), true);
    let (a, b) = (
        band_db(&fold, SR, 990.0, 1010.0),
        band_db(&folded_clean, SR, 990.0, 1010.0),
    );
    println!(
        "7 kHz sine, rate 8000: alias at 1 kHz {a:.1} dB, with antialias {b:.1} dB ({:.1} dB lower)",
        a - b
    );
    assert!(a - b >= 30.0);
}

#[test]
fn resample_holds_exactly_and_takes_a_swept_rate() {
    // 12000 Hz at 48 kHz: every 4 samples are one value, the first of each.
    let raw = node_audio([sine(1000.0, 0.5)], "src", 0.2);
    let held = resampled_len(1000.0, json!(12000), 0.2);
    assert!(
        held.chunks(4)
            .zip(raw.chunks(4))
            .all(|(h, r)| h.iter().all(|v| v.to_bits() == r[0].to_bits()))
    );
    // The sample rate passes the input through.
    let same = resampled_len(1000.0, json!(48000), 0.2);
    assert!(
        same.iter()
            .zip(&raw)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    // Fractional ratio: 7000 Hz at 48 kHz holds each value 6 or 7 samples.
    let frac = resampled_len(1000.0, json!(7000), 0.2);
    let mut runs = Vec::new();
    let mut k = 1;
    for w in frac.windows(2) {
        if w[0].to_bits() == w[1].to_bits() {
            k += 1;
        } else {
            runs.push(k);
            k = 1;
        }
    }
    let (lo, hi) = (*runs.iter().min().unwrap(), *runs.iter().max().unwrap());
    println!("rate 7000: held runs are {lo} to {hi} samples (48000/7000 = 6.86)");
    assert!(lo >= 6 && hi <= 7);
    // A swept rate with and without antialias, stereo input.
    let nodes = vec![
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "pan", "name": "st", "input": "n", "pan": -0.3}),
        json!({"type": "ramp", "name": "r", "from": 20000, "to": 300, "time": 0.5, "curve": "exp"}),
        json!({"type": "resample", "name": "rs", "input": "st", "rate": "r", "antialias": true}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2);
    for c in &a.channels {
        assert_finite(c, "rs");
    }
    assert_bit_identical(&nodes, 0.5);
}

fn resampled_len(hz: f32, rate: Value, secs: f32) -> Vec<f32> {
    let nodes = vec![
        sine(hz, 0.5),
        json!({"type": "resample", "name": "rs", "input": "src", "rate": rate}),
    ];
    node_audio(nodes, "rs", secs)
}

#[test]
fn resample_is_finite_at_the_extremes_and_errors_explain() {
    for rate in [json!(1), json!(48000), json!(1.0e9), json!(-5.0e8)] {
        // number errors are tested below; signals clamp
        let nodes = vec![
            json!({"type": "noise", "name": "n", "color": "white", "amp": 1.0e6}),
            json!({"type": "gain", "name": "r", "input": "n", "gain": 1.0}),
            json!({"type": "resample", "name": "rs", "input": "n", "rate": if rate.as_f64().unwrap() > 48000.0 || rate.as_f64().unwrap() < 0.0 {json!("r")} else {rate.clone()}, "antialias": true}),
        ];
        let x = node_audio(nodes, "rs", 0.3);
        assert_finite(&x, "rs");
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!("rate {rate}: peak {peak:.3e} at input amplitude 1e6");
    }
    for (rate, want) in [
        (json!(0), "rate: 0: give a number from 1 to 48000 Hz"),
        (
            json!(60000),
            "rate: 60000: give a number from 1 to 48000 Hz",
        ),
    ] {
        let e = render_error(
            vec![
                sine(100.0, 0.5),
                json!({"type": "resample", "name": "rs", "input": "src", "rate": rate}),
            ],
            0.1,
        );
        println!("resample error: {e}");
        assert!(e.contains(want), "{e}");
    }
}

// --------------------------------------------------------------------- tape

/// Rising zero crossings of `x`, interpolated, as seconds.
fn crossings(x: &[f32]) -> Vec<f64> {
    let mut t = Vec::new();
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let f = f64::from(x[i - 1]) / f64::from(x[i - 1] - x[i]);
            t.push((i as f64 - 1.0 + f) / f64::from(SR));
        }
    }
    t
}

/// (time, Hz) of each full cycle.
fn fm_track(x: &[f32]) -> Vec<(f64, f64)> {
    let c = crossings(x);
    c.windows(2)
        .map(|w| (0.5 * (w[0] + w[1]), 1.0 / (w[1] - w[0])))
        .collect()
}

/// Least-squares amplitude of a sinusoid at `hz` in the track (mean removed).
fn amp_at(track: &[(f64, f64)], hz: f64) -> f64 {
    let mean = track.iter().map(|p| p.1).sum::<f64>() / track.len() as f64;
    let (mut cc, mut ss, mut cs, mut cy, mut sy) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (t, f) in track {
        let (s, c) = (std::f64::consts::TAU * hz * t).sin_cos();
        let y = f - mean;
        cc += c * c;
        ss += s * s;
        cs += c * s;
        cy += c * y;
        sy += s * y;
    }
    let det = cc * ss - cs * cs;
    let (a, b) = ((cy * ss - sy * cs) / det, (sy * cc - cy * cs) / det);
    (a * a + b * b).sqrt()
}

/// Strongest sinusoid in `lo..hi` Hz, as (Hz, amplitude).
fn best_component(track: &[(f64, f64)], lo: f64, hi: f64) -> (f64, f64) {
    let mut best = (lo, 0.0);
    let mut hz = lo;
    while hz <= hi {
        let a = amp_at(track, hz);
        if a > best.1 {
            best = (hz, a);
        }
        hz += 0.02;
    }
    best
}

fn tape_nodes(extra: Value) -> Vec<Value> {
    let mut t = json!({"type": "tape", "name": "tp", "input": "src", "wow": 0, "flutter": 0,
        "drive": 0, "hiss_db": -120, "tone": 21600});
    t.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![sine(1000.0, 0.5), t]
}

#[test]
fn tape_wow_modulates_the_pitch_by_the_set_depth_below_2_hz() {
    for (wow, seed) in [(1.0f32, 1u64), (1.0, 2), (1.0, 3), (0.5, 4), (2.0, 5)] {
        let x = node_audio(tape_nodes(json!({"wow": wow, "seed": seed})), "tp", 6.0);
        let track = fm_track(&x);
        let (hz, amp) = best_component(&track, 0.2, 2.0);
        // 1 kHz tone: the deviation in Hz is the percent times 10.
        let want = f64::from(wow) * 10.0;
        println!(
            "wow {wow} %, seed {seed}: strongest pitch component {hz:.2} Hz, amplitude {amp:.2} Hz (want {want:.1}, {:+.0} %)",
            (amp / want - 1.0) * 100.0
        );
        assert!(hz < 2.0);
        assert!((amp / want - 1.0).abs() < 0.30, "{amp} vs {want}");
        // Nothing else is moving the pitch: the above 2 Hz part is small.
        let (hf, af) = best_component(&track, 3.0, 20.0);
        assert!(af < 0.1 * want, "wow only, but {af} Hz at {hf} Hz");
    }
}

#[test]
fn tape_flutter_is_fast_and_wow_flutter_off_is_clean() {
    let x = node_audio(tape_nodes(json!({"flutter": 1.0, "seed": 2})), "tp", 4.0);
    let track = fm_track(&x);
    let mean = track.iter().map(|p| p.1).sum::<f64>() / track.len() as f64;
    let rms_dev =
        (track.iter().map(|p| (p.1 - mean).powi(2)).sum::<f64>() / track.len() as f64).sqrt();
    // Two partials, 0.65 and 0.35 of 10 Hz: rms 10 * sqrt((0.65^2 + 0.35^2) / 2).
    let want = 10.0 * ((0.65f64.powi(2) + 0.35f64.powi(2)) / 2.0).sqrt();
    let (hz, amp) = best_component(&track, 4.0, 16.0);
    let (_, slow) = best_component(&track, 0.2, 2.0);
    println!(
        "flutter 1 %: pitch deviation rms {rms_dev:.2} Hz (want {want:.2}), strongest component {hz:.2} Hz at {amp:.2} Hz, under 2 Hz {slow:.2} Hz"
    );
    assert!((rms_dev / want - 1.0).abs() < 0.3);
    assert!(hz > 4.9 && slow < 1.0);
    // With wow, flutter, drive and hiss off the tape is a one sample delay
    // and a high tone: the level moves under 0.1 dB.
    let clean = node_audio(tape_nodes(json!({})), "tp", 0.5);
    let raw = node_audio([sine(1000.0, 0.5)], "src", 0.5);
    let d = rms(&clean[100..]) - rms(&raw[100..]);
    let lag = clean[100..]
        .iter()
        .zip(&raw[99..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!(
        "all off: level change {d:.3} dB; max difference from the input delayed by 1 sample {lag:.4}"
    );
    assert!(d.abs() < 0.1 && lag < 0.01);
}

#[test]
fn tape_drive_hiss_and_tone() {
    // Drive: tanh(d x) / d. A quiet tone keeps its level; a loud one is
    // limited to tanh(d a) / d.
    let quiet = node_audio(
        vec![
            sine(1000.0, 0.01),
            tape_nodes(json!({"drive": 4}))[1].clone(),
        ],
        "tp",
        0.3,
    );
    let raw = node_audio([sine(1000.0, 0.01)], "src", 0.3);
    let g = rms(&quiet[200..]) - rms(&raw[200..]);
    let loud = node_audio(
        vec![
            sine(1000.0, 0.9),
            tape_nodes(json!({"drive": 4}))[1].clone(),
        ],
        "tp",
        0.3,
    );
    let peak = loud.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let limit = (4.0f32 * 0.9).tanh() / 4.0;
    let h3 = band_db(&loud, SR, 2990.0, 3010.0) - band_db(&loud, SR, 990.0, 1010.0);
    println!(
        "drive 4: 0.01 amplitude tone {g:.3} dB; 0.9 amplitude tone peaks {peak:.4} (limit {limit:.4}), 3rd harmonic {h3:.1} dB re fundamental"
    );
    assert!(g.abs() < 0.1 && peak <= limit * 1.02 && peak > 0.95 * limit && h3 > -40.0);
    // Hiss: silence through the node, white noise at hiss_db RMS. With the
    // tone filter at 21600 about all of it passes; at 2400 Hz a 2nd order
    // low-pass leaves 1.1107 x 2400 / 24000 of the power (-9.5 dB).
    let silent = json!({"type": "gain", "name": "src", "input": "z", "gain": 0});
    let zero = |extra: Value| {
        let mut t = tape_nodes(extra)[1].clone();
        t["input"] = json!("src");
        vec![json!({"type": "noise", "name": "z"}), silent.clone(), t]
    };
    let wide = node_audio(zero(json!({"hiss_db": -40})), "tp", 1.0);
    let narrow = node_audio(zero(json!({"hiss_db": -40, "tone": 2400})), "tp", 1.0);
    let (w, n) = (rms(&wide[1000..]), rms(&narrow[1000..]));
    println!(
        "hiss -40 dB: rms {w:.2} dB with tone 21600 Hz, {n:.2} dB with tone 2400 Hz (want about -40.5 and -49.5)"
    );
    assert!((w + 40.5).abs() < 1.0 && (n + 49.5).abs() < 1.0);
    // Seeds change the hiss; the same seed repeats.
    let s1 = node_audio(zero(json!({"hiss_db": -40, "seed": 1})), "tp", 0.2);
    let s2 = node_audio(zero(json!({"hiss_db": -40, "seed": 2})), "tp", 0.2);
    let s1b = node_audio(zero(json!({"hiss_db": -40, "seed": 1})), "tp", 0.2);
    assert_ne!(s1, s2);
    assert_eq!(s1, s1b);
    // Tone: a 2 kHz corner, 12 dB/oct: -3 dB at 2 kHz, about -24 dB at 8 kHz.
    let level = |hz: f32| {
        let mut nodes = vec![sine(hz, 0.5), tape_nodes(json!({"tone": 2000}))[1].clone()];
        nodes[1]["input"] = json!("src");
        let y = node_audio(nodes, "tp", 0.5);
        let raw = node_audio([sine(hz, 0.5)], "src", 0.5);
        rms(&y[2000..]) - rms(&raw[2000..])
    };
    let (a, b, c) = (level(200.0), level(2000.0), level(8000.0));
    println!("tone 2000 Hz: 200 Hz {a:.2} dB, 2 kHz {b:.2} dB, 8 kHz {c:.2} dB");
    assert!(a.abs() < 0.2 && (b + 3.0).abs() < 0.5 && c < -22.0 && c > -26.0);
}

#[test]
fn tape_is_bit_identical_stereo_finite_at_the_extremes_and_errors_explain() {
    let nodes = vec![
        json!({"type": "noise", "name": "n", "color": "pink"}),
        json!({"type": "pan", "name": "st", "input": "n", "pan": 0.3}),
        json!({"type": "tape", "name": "tp", "input": "st", "seed": 7}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2);
    assert_ne!(a.channels[0], a.channels[1]);
    assert_bit_identical(&nodes, 0.5);
    // Both channels share the wobble: a mono input panned to the centre
    // gives two channels that differ only by hiss (-60 dB).
    // Extremes: every field at both ends, input amplitude 1e6.
    for (wow, flutter, drive, hiss, tone) in [
        (5.0, 5.0, 40.0, 0.0, 20.0),
        (0.0, 0.0, 0.0, -120.0, 21600.0),
        (5.0, 0.0, 40.0, -120.0, 21600.0),
    ] {
        let nodes = vec![
            json!({"type": "noise", "name": "n", "color": "white", "amp": 1.0e6}),
            json!({"type": "tape", "name": "tp", "input": "n", "wow": wow, "flutter": flutter,
                "drive": drive, "hiss_db": hiss, "tone": tone}),
        ];
        let x = node_audio(nodes, "tp", 0.5);
        assert_finite(&x, "tp");
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        println!(
            "tape wow {wow} flutter {flutter} drive {drive} hiss {hiss} tone {tone} at input amplitude 1e6: peak {peak:.3e}"
        );
    }
    for (extra, want) in [
        (
            json!({"wow": 6}),
            "wow: 6: give percent of speed from 0 to 5",
        ),
        (json!({"flutter": -1}), "flutter: -1:"),
        (
            json!({"drive": 100}),
            "drive: 100: give a number from 0 to 40",
        ),
        (
            json!({"hiss_db": 5}),
            "hiss_db: 5: give dBFS from -120 (off) to 0",
        ),
        (
            json!({"tone": 30000}),
            "tone: 30000: give Hz from 20 to 21600",
        ),
    ] {
        let mut t = json!({"type": "tape", "name": "tp", "input": "src"});
        t.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let e = render_error(vec![sine(100.0, 0.5), t], 0.1);
        println!("tape error: {e}");
        assert!(e.contains(want), "{e}");
    }
}

// ------------------------------------------------------------------ stutter

fn ramp_src() -> Value {
    // A line from 0 to 1 over one second: a sample's value tells where it came from.
    json!({"type": "ramp", "name": "src", "from": 0, "to": 1, "time": 1.0, "curve": "linear"})
}

fn stutter_nodes(times: &[f32], extra: Value) -> Vec<Value> {
    let mut s = json!({"type": "stutter", "name": "st", "input": "src", "events": "ev"});
    s.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![
        ramp_src(),
        json!({"type": "pattern", "name": "ev", "times": times}),
        s,
    ]
}

#[test]
fn stutter_repeats_the_last_slice_and_leaves_the_rest_alone() {
    let raw = node_audio(vec![ramp_src()], "src", 1.0);
    let y = node_audio(
        stutter_nodes(&[0.5], json!({"slice": 0.05, "repeats": 4})),
        "st",
        1.0,
    );
    let te = (0.5 * SR as f32) as usize;
    let l = (0.05 * SR as f32) as usize;
    let xf = (0.002 * SR as f32) as usize;
    assert_eq!(y.len(), raw.len());
    // Untouched before the event and from the end of the repeats plus fade.
    assert!(
        y[..te]
            .iter()
            .zip(&raw)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    let end = te + 4 * l;
    assert!(
        y[end + xf..]
            .iter()
            .zip(&raw[end + xf..])
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    // Inside, after each repeat's 2 ms fade-in, the samples are the slice
    // that played before the event (raw[te - l .. te]).
    let mut worst = 0.0f32;
    for r in 0..4 {
        for k in xf..l {
            worst = worst.max((y[te + r * l + k] - raw[te - l + k]).abs());
        }
    }
    // The joins are continuous: the biggest step in the region against the
    // ramp's own step per sample.
    let step = 1.0 / SR as f32;
    let jump = y[te - 1..end + xf + 1]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max);
    println!(
        "ramp, event at 0.5 s, slice 0.05 s x 4: repeated samples differ from the slice by at most {worst:.2e}; largest step in the region {:.1} x the ramp's own",
        jump / step
    );
    assert!(worst < 1e-6);
    // Going back 0.05 (a cut would be a 2400 x step) is spread over 2 ms
    // (about 26 x); the exit moves 0.2 over 2 ms (about 104 x).
    assert!(jump / step < 120.0);
}

#[test]
fn stutter_reverse_plays_the_next_slice_backwards_and_is_acausal() {
    let raw = node_audio(vec![ramp_src()], "src", 1.0);
    let nodes = stutter_nodes(
        &[0.5],
        json!({"slice": 0.05, "repeats": 3, "reverse": true}),
    );
    let y = node_audio(nodes, "st", 1.0);
    let te = (0.5 * SR as f32) as usize;
    let l = (0.05 * SR as f32) as usize;
    let xf = (0.002 * SR as f32) as usize;
    let mut worst = 0.0f32;
    for r in 0..3 {
        for k in xf..l {
            worst = worst.max((y[te + r * l + k] - raw[te + l - 1 - k]).abs());
        }
    }
    println!("reverse: repeats match the following slice reversed to {worst:.2e}");
    assert!(worst < 1e-6);
    let body = |rev: bool| json!({"input": "src", "events": "ev", "reverse": rev});
    let causal = |rev: bool| {
        parse_body("stutter", &body(rev), "stutter")
            .unwrap()
            .causal()
    };
    assert!(causal(false) && !causal(true));
}

#[test]
fn stutter_edges_stereo_bit_identical_and_errors_explain() {
    // An event at 0 has nothing to repeat; one past the end is ignored; one
    // early in the sound repeats what there is.
    let raw = node_audio(vec![ramp_src()], "src", 1.0);
    let y = node_audio(stutter_nodes(&[0.0, 5.0], json!({})), "st", 1.0);
    assert!(y.iter().zip(&raw).all(|(a, b)| a.to_bits() == b.to_bits()));
    let y = node_audio(
        stutter_nodes(&[0.01], json!({"slice": 0.05, "repeats": 2})),
        "st",
        1.0,
    );
    let te = (0.01 * SR as f32) as usize;
    let xf = (0.002 * SR as f32) as usize;
    let worst = (xf..te)
        .map(|k| (y[te + k] - raw[k]).abs())
        .fold(0.0f32, f32::max);
    println!("event at 10 ms, slice 50 ms: repeats the 10 ms there is, differing by {worst:.2e}");
    assert!(worst < 1e-6);
    // Stereo, jittered events, bit-identical on a second render.
    let nodes = vec![
        json!({"type": "noise", "name": "n", "color": "pink"}),
        json!({"type": "pan", "name": "src", "input": "n", "pan": 0.4}),
        json!({"type": "poisson", "name": "ev", "rate": 8}),
        json!({"type": "stutter", "name": "st", "input": "src", "events": "ev", "slice": 0.03, "repeats": 5}),
    ];
    let a = render(&nodes, 1.0);
    assert_eq!(a.channels.len(), 2);
    assert_bit_identical(&nodes, 1.0);
    // Extremes: the smallest slice, 256 repeats, many overlapping events.
    for (slice, repeats) in [(0.0001, 256), (10.0, 256), (0.0001, 1)] {
        let nodes = vec![
            json!({"type": "noise", "name": "src", "color": "white", "amp": 1.0e6}),
            json!({"type": "poisson", "name": "ev", "rate": 200}),
            json!({"type": "stutter", "name": "st", "input": "src", "events": "ev", "slice": slice, "repeats": repeats, "reverse": true}),
        ];
        let x = node_audio(nodes, "st", 1.0);
        assert_finite(&x, "st");
    }
    let mk = |extra: Value| {
        let mut s = json!({"type": "stutter", "name": "st", "input": "src", "events": "ev"});
        s.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![
            ramp_src(),
            json!({"type": "pattern", "name": "ev", "every": 0.0001, "count": 100000}),
            s,
        ]
    };
    for (extra, want) in [
        (
            json!({"slice": 20}),
            "slice: 20: give seconds from 0.0001 to 10",
        ),
        (
            json!({"repeats": 0}),
            "repeats: 0: give a whole number from 1 to 256",
        ),
        (
            json!({"slice": 2, "repeats": 256}),
            "over the limit of 400000000 samples",
        ),
    ] {
        let e = render_error(mk(extra), 1.0);
        println!("stutter error: {e}");
        assert!(e.contains(want), "{e}");
    }
    let e = render_error(
        vec![
            ramp_src(),
            json!({"type": "stutter", "name": "st", "input": "src", "events": "src"}),
        ],
        0.1,
    );
    assert!(e.contains("events:"), "{e}");
}

// ------------------------------------------------------------------ recipes

/// The four recipes in docs/nodes/S12.md, as written.
#[test]
fn lofi_recipes_render() {
    // Seamless wind bed: a loop.
    let wind: Vec<Value> = serde_json::from_str(
        r#"[
  {"type": "noise", "name": "air", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 0.3, "range": [300, 2500], "seed": 2},
  {"type": "filter", "name": "band", "input": "air", "mode": "bp", "freq": "sweep", "q": 3},
  {"type": "crossfade_loop", "name": "seamless", "input": "band", "fade": 0.4}
]"#,
    )
    .unwrap();
    let r = seam(&loop_doc(&wind));
    println!(
        "recipe loop: loop_seam {} ({}), failing default checks {:?}",
        r.0, r.1, r.2
    );
    assert!(r.0);
    let bare = seam(&loop_doc(&wind[..3]));
    println!(
        "  the same without crossfade_loop: loop_seam {} ({})",
        bare.0, bare.1
    );

    // A crunchy 8-bit hold of a sawtooth.
    let crunch: Vec<Value> = serde_json::from_str(
        r#"[
  {"type": "osc", "name": "tone", "shape": "saw", "freq": 220, "amp": 0.5},
  {"type": "resample", "name": "crunch", "input": "tone", "rate": 6000}
]"#,
    )
    .unwrap();
    let x = node_audio(&crunch, "crunch", 0.5);
    println!("recipe resample: rms {:.2} dB", rms(&x));
    assert_finite(&x, "crunch");

    // An old cassette: a held chord.
    let cassette: Vec<Value> = serde_json::from_str(
        r#"[
  {"type": "osc", "name": "a", "freq": 330, "amp": 0.3},
  {"type": "osc", "name": "b", "freq": 495, "amp": 0.3},
  {"type": "mix", "name": "chord", "inputs": ["a", "b"]},
  {"type": "tape", "name": "old", "input": "chord", "wow": 0.6, "flutter": 0.3, "drive": 2, "hiss_db": -50, "tone": 5000, "seed": 3}
]"#,
    )
    .unwrap();
    let x = node_audio(&cassette, "old", 2.0);
    println!("recipe tape: rms {:.2} dB", rms(&x));
    assert_finite(&x, "old");

    // A skipping record: the last 80 ms repeat 3 times at 0.6 s and 1.3 s.
    let skip: Vec<Value> = serde_json::from_str(
        r#"[
  {"type": "ramp", "name": "pitch", "from": 220, "to": 440, "time": 2, "curve": "exp"},
  {"type": "osc", "name": "tone", "freq": "pitch", "amp": 0.5},
  {"type": "pattern", "name": "skips", "times": [0.6, 1.3]},
  {"type": "stutter", "name": "skip", "input": "tone", "events": "skips", "slice": 0.08, "repeats": 3}
]"#,
    )
    .unwrap();
    let x = node_audio(&skip, "skip", 2.0);
    println!("recipe stutter: rms {:.2} dB", rms(&x));
    assert_finite(&x, "skip");
}
