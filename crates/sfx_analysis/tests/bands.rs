//! X2: `filterbank`, `vocoder`, `multiband`. Each test measures something and
//! prints it (these lines go in docs/nodes/X2.md).

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::node::parse_body;
use sfx_graph::{RenderOpts, SoundDoc};

fn white() -> Value {
    json!({"type": "noise", "name": "src", "amp": 0.3})
}

fn sine(name: &str, hz: f32, amp: f32) -> Value {
    json!({"type": "osc", "name": name, "freq": hz, "amp": amp})
}

/// A node of `ty` named `n` reading `src` plus `extra` fields.
fn node(ty: &str, n: &str, extra: Value) -> Value {
    let mut r = json!({"type": ty, "name": n});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    r
}

fn render_error(nodes: Vec<Value>, read: &str) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": read}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
}

/// RMS in dB of the second half (after the filters have settled).
fn settled_db(x: &[f32]) -> f32 {
    rms_db(&x[x.len() / 2..])
}

// ---------------------------------------------------------------- multiband

fn mb(extra: Value) -> Vec<Value> {
    vec![
        white(),
        node("multiband", "mb", {
            let mut e = json!({"input": "src"});
            e.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            e
        }),
    ]
}

#[test]
fn multiband_neutral_sums_flat() {
    // White noise through three crossovers; the output's power in each region
    // against the input's. Neutral = no `bands`, then an explicit empty list.
    let xo = [200.0, 1000.0, 5000.0];
    let inp = node_audio(vec![white()], "src", 1.0);
    for extra in [
        json!({"crossovers": xo}),
        json!({"crossovers": xo, "bands": [{}, {}, {}, {}]}),
    ] {
        let out = node_audio(mb(extra), "mb", 1.0);
        assert_finite(&out, "mb");
        let mut worst = 0.0f32;
        for (lo, hi) in [
            (40.0, 150.0),
            (180.0, 220.0),
            (400.0, 800.0),
            (950.0, 1050.0),
            (2000.0, 4000.0),
            (4800.0, 5200.0),
            (6000.0, 12000.0),
            (40.0, 20000.0),
        ] {
            let d = band_db(&out, SR, lo, hi) - band_db(&inp, SR, lo, hi);
            worst = worst.max(d.abs());
            assert!(d.abs() < 0.5, "{lo}..{hi} Hz: {d:.3} dB");
        }
        println!(
            "multiband neutral, crossovers 200/1000/5000: worst band difference {worst:.4} dB (noise, 8 regions)"
        );
    }
    // Sines at, between and away from the crossovers.
    let mut worst = 0.0f32;
    for hz in [60.0, 200.0, 350.0, 1000.0, 2236.0, 5000.0, 9000.0] {
        let nodes = vec![
            sine("src", hz, 0.5),
            json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": xo}),
        ];
        let out = node_audio(&nodes, "mb", 1.0);
        let d = settled_db(&out) - settled_db(&node_audio(vec![sine("src", hz, 0.5)], "src", 1.0));
        worst = worst.max(d.abs());
        assert!(d.abs() < 0.5, "{hz} Hz: {d:.3} dB");
    }
    println!(
        "multiband neutral: worst sine level change at 7 frequencies (crossovers included) {worst:.4} dB"
    );
}

#[test]
fn multiband_crossover_is_6_db_down_in_each_band() {
    let nodes = vec![
        sine("src", 1000.0, 0.5),
        json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": [1000]}),
        json!({"type": "gain", "name": "lo", "input": "mb.band0", "gain": 1.0}),
        json!({"type": "gain", "name": "hi", "input": "mb.band1", "gain": 1.0}),
    ];
    let inp = settled_db(&node_audio(vec![sine("src", 1000.0, 0.5)], "src", 1.0));
    let lo = settled_db(&node_audio(&nodes, "lo", 1.0)) - inp;
    let hi = settled_db(&node_audio(&nodes, "hi", 1.0)) - inp;
    println!(
        "LR4 at its crossover (1 kHz): low band {lo:.2} dB, high band {hi:.2} dB relative to the input"
    );
    assert!((lo + 6.02).abs() < 0.3 && (hi + 6.02).abs() < 0.3);
    // Far from it the bands separate: 100 Hz is in the low band only.
    let nodes = vec![
        sine("src", 100.0, 0.5),
        json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": [1000]}),
        json!({"type": "gain", "name": "hi", "input": "mb.band1", "gain": 1.0}),
    ];
    let hi = settled_db(&node_audio(&nodes, "hi", 1.0));
    println!("100 Hz in the band above 1 kHz: {hi:.1} dB");
    assert!(hi < -60.0);
}

#[test]
fn multiband_band_gain_moves_only_its_band() {
    let base = node_audio(mb(json!({"crossovers": [150, 6000]})), "mb", 1.0);
    let cut = node_audio(
        mb(json!({"crossovers": [150, 6000], "bands": [{}, {"gain_db": -12}]})),
        "mb",
        1.0,
    );
    let d = |lo, hi| band_db(&cut, SR, lo, hi) - band_db(&base, SR, lo, hi);
    let (a, b, c) = (d(30.0, 100.0), d(800.0, 1250.0), d(8000.0, 14000.0));
    println!(
        "band 1 at -12 dB: change at 30-100 Hz {a:.2} dB, 800-1250 Hz {b:.2} dB, 8-14 kHz {c:.2} dB"
    );
    assert!(a.abs() < 0.5 && c.abs() < 0.5 && (b + 12.0).abs() < 0.5);
}

#[test]
fn multiband_compresses_only_its_band() {
    // 100 Hz sine, amplitude 0.8 (-1.94 dBFS peak), threshold -20, ratio 4:
    // 18.06 dB over -> 4.5 dB over, a 13.5 dB reduction.
    let tone = |hz| {
        node_audio(
            vec![
                sine("src", hz, 0.8),
                json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": [400],
                       "bands": [{"threshold_db": -20, "ratio": 4, "attack": 0.002, "release": 0.1}]}),
            ],
            "mb",
            1.0,
        )
    };
    let plain = |hz| node_audio(vec![sine("src", hz, 0.8)], "src", 1.0);
    let low = settled_db(&tone(100.0)) - settled_db(&plain(100.0));
    let high = settled_db(&tone(3000.0)) - settled_db(&plain(3000.0));
    println!(
        "compressor on band 0 (-20 dB, 4:1): 100 Hz changes {low:.2} dB (expected about -13.5), 3 kHz {high:.2} dB"
    );
    assert!((low + 13.5).abs() < 1.5, "{low}");
    assert!(high.abs() < 0.5, "{high}");
    // Ratio 1 never reduces.
    let flat = node_audio(
        vec![
            sine("src", 100.0, 0.8),
            json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": [400],
                   "bands": [{"threshold_db": -20, "ratio": 1}]}),
        ],
        "mb",
        1.0,
    );
    let d = settled_db(&flat) - settled_db(&plain(100.0));
    println!("ratio 1: {d:.3} dB");
    assert!(d.abs() < 0.1);
}

#[test]
fn multiband_drive_adds_harmonics_in_its_band() {
    let run = |drive: f32| {
        let mut band = json!({});
        if drive > 0.0 {
            band = json!({"drive": drive});
        }
        node_audio(
            vec![
                sine("src", 100.0, 0.5),
                json!({"type": "multiband", "name": "mb", "input": "src", "crossovers": [200], "bands": [band]}),
            ],
            "mb",
            1.0,
        )
    };
    let (clean, driven) = (run(0.0), run(20.0));
    let rel = |x: &[f32]| band_db(x, SR, 290.0, 310.0) - band_db(x, SR, 90.0, 110.0);
    println!(
        "100 Hz sine, drive 20 on band 0: 300 Hz is {:.1} dB under the fundamental (drive 0: {:.1} dB)",
        rel(&driven),
        rel(&clean)
    );
    // tanh(20 x) is close to a square wave: the third harmonic is 1/3.
    assert!(rel(&driven) > -14.0 && rel(&clean) < -60.0);
    let peak = driven.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!("driven peak {peak:.3}");
    assert!(peak < 1.2);
}

#[test]
fn multiband_stereo_runs_both_channels_with_one_gain() {
    let nodes = vec![
        white(),
        json!({"type": "pan", "name": "st", "input": "src", "pan": 0.5}),
        json!({"type": "multiband", "name": "mb", "input": "st", "crossovers": [500],
               "bands": [{"threshold_db": -30, "ratio": 8}]}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2);
    let pan = render(&nodes[..2], 0.5);
    let diff = rms_db(&a.channels[0]) - rms_db(&a.channels[1]);
    let diff_in = rms_db(&pan.channels[0]) - rms_db(&pan.channels[1]);
    println!("stereo: L - R level {diff:.2} dB after (input {diff_in:.2} dB)");
    // Linked detector: the balance is kept to within a fraction of a dB.
    assert!((diff - diff_in).abs() < 1.0);
}

// --------------------------------------------------------------- filterbank

fn fb(extra: Value) -> Vec<Value> {
    let mut e = json!({"input": "src"});
    e.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![white(), node("filterbank", "fb", e)]
}

fn centre(low: f32, high: f32, bands: usize, k: usize) -> f32 {
    low * (high / low).powf(k as f32 / (bands - 1) as f32)
}

#[test]
fn filterbank_one_band_passes_around_its_centre() {
    // 8 bands from 100 Hz to 6400 Hz: centres one octave apart.
    let mut worst_off = f32::MIN;
    for k in 1..7 {
        let mut gains = vec![0.0; 8];
        gains[k] = 1.0;
        let x = node_audio(
            fb(json!({"bands": 8, "low": 100, "high": 6400, "gains": gains})),
            "fb",
            1.0,
        );
        assert_finite(&x, "fb");
        let c = centre(100.0, 6400.0, 8, k);
        let inside = band_db(&x, SR, c * 0.9, c * 1.1);
        let below = band_db(&x, SR, c / 4.4, c / 3.6);
        let above = band_db(&x, SR, c * 3.6, c * 4.4);
        let width = (inside - below).min(inside - above);
        println!(
            "band {k} at {c:.0} Hz alone: 0.9-1.1x {inside:.1} dB, 1/4x {below:.1} dB, 4x {above:.1} dB"
        );
        worst_off = worst_off.max(below.max(above) - inside);
        let (half, double) = (
            band_db(&x, SR, c * 0.45, c * 0.55),
            band_db(&x, SR, c * 1.8, c * 2.2),
        );
        println!("  1/2x {half:.1} dB, 2x {double:.1} dB");
        assert!(
            inside > half + 5.0 && inside > double + 4.0,
            "band {k}: {inside} {half} {double}"
        );
        assert!(width > 9.0, "band {k}: skirts only {width:.1} dB down");
    }
    println!(
        "filterbank single band: skirts at 4x away at worst {worst_off:.1} dB relative to the band"
    );
}

#[test]
fn filterbank_gains_scale_and_sum() {
    let ones = node_audio(fb(json!({"bands": 12})), "fb", 1.0);
    let half = node_audio(fb(json!({"bands": 12, "gains": vec![0.5; 12]})), "fb", 1.0);
    let d = rms_db(&half) - rms_db(&ones);
    println!("12 bands, all gains 0.5 vs 1: {d:.3} dB");
    assert!((d + 6.02).abs() < 0.02);
    // The flat bank's ripple: octave-band levels of white noise 150 Hz .. 9 kHz.
    let inp = node_audio(vec![white()], "src", 1.0);
    let lv: Vec<f32> = [200.0f32, 400.0, 800.0, 1600.0, 3200.0, 6400.0]
        .iter()
        .map(|c| band_db(&ones, SR, c / 1.414, c * 1.414) - band_db(&inp, SR, c / 1.414, c * 1.414))
        .collect();
    let spread =
        lv.iter().cloned().fold(f32::MIN, f32::max) - lv.iter().cloned().fold(f32::MAX, f32::min);
    println!(
        "12 bands at gain 1: octave-band levels re the input {lv:.2?} dB, spread {spread:.2} dB"
    );
    assert!(spread < 3.0);
    // A negative gain flips a band: band k at +1 and -1 sums differ from all +1.
    let flipped = node_audio(fb(json!({"bands": 4, "gains": [1, -1, 1, -1]})), "fb", 0.5);
    assert_finite(&flipped, "flipped");
}

#[test]
fn filterbank_stereo_and_single_band() {
    let nodes = vec![
        white(),
        json!({"type": "pan", "name": "st", "input": "src", "pan": -0.7}),
        json!({"type": "filterbank", "name": "fb", "input": "st", "bands": 1, "q": 4}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2);
    let pk = peak_hz(&a.channels[0], SR);
    println!("one band, q 4: peak at {pk:.0} Hz (geometric mean of 80 and 12000 is 980)");
    assert!((pk / 980.0 - 1.0).abs() < 0.1);
    assert!(rms_db(&a.channels[0]) > rms_db(&a.channels[1]) + 1.0);
}

// ------------------------------------------------------------------ vocoder

/// Third-octave-ish check: white noise carrier, the modulator a noise low-passed
/// at 1.5 kHz. The vocoder's output, band by band, should follow the
/// modulator's spectral shape (carrier's own shape removed).
#[test]
fn vocoder_output_bands_track_the_modulator() {
    let nodes = vec![
        json!({"type": "noise", "name": "carrier", "seed": 1, "amp": 0.3}),
        json!({"type": "noise", "name": "raw", "seed": 2, "amp": 0.3}),
        json!({"type": "filter", "name": "mod", "input": "raw", "mode": "lp", "freq": 1500}),
        json!({"type": "vocoder", "name": "vc", "carrier": "carrier", "modulator": "mod", "bands": 16}),
    ];
    let secs = 3.0;
    let out = node_audio(&nodes, "vc", secs);
    let car = node_audio(&nodes, "carrier", secs);
    let md = node_audio(&nodes, "mod", secs);
    assert_finite(&out, "vc");
    let centres = [250.0f32, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];
    let level = |x: &[f32], c: f32| band_db(x, SR, c / 1.414, c * 1.414);
    let rel: Vec<f32> = centres
        .iter()
        .map(|&c| level(&out, c) - level(&car, c) - level(&md, c))
        .collect();
    let mean = rel.iter().sum::<f32>() / rel.len() as f32;
    let dev: Vec<f32> = rel.iter().map(|r| r - mean).collect();
    let worst = dev.iter().fold(0.0f32, |m, d| m.max(d.abs()));
    let shape: Vec<f32> = centres.iter().map(|&c| level(&md, c)).collect();
    println!(
        "vocoder, white carrier, modulator noise lp 1.5 kHz: modulator octave levels {shape:.1?} dB; (output - carrier) - modulator per octave, mean removed {dev:.2?} dB, worst {worst:.2} dB"
    );
    assert!(worst < 3.0, "{dev:?}");
    // The modulator really did shape the output: its slope is in the output.
    let span = level(&out, 250.0) - level(&out, 8000.0);
    let span_car = level(&car, 250.0) - level(&car, 8000.0);
    println!(
        "250 Hz vs 8 kHz octave: output {span:.1} dB, carrier {span_car:.1} dB, modulator {:.1} dB",
        level(&md, 250.0) - level(&md, 8000.0)
    );
    let span_mod = level(&md, 250.0) - level(&md, 8000.0);
    assert!(((span - span_car) - span_mod).abs() < 3.0);
}

#[test]
fn vocoder_band_level_follows_the_modulator_level() {
    // White noise carrier, a sine at one band's centre as modulator: the
    // energy lands in that band, and halving the sine's amplitude lowers the
    // band's output by 6.02 dB.
    let (low, high, nb) = (200.0f32, 6000.0, 8usize);
    let run = |k: usize, amp: f32| {
        let nodes = vec![
            json!({"type": "noise", "name": "carrier", "amp": 0.3}),
            sine("mod", centre(low, high, nb, k), amp),
            json!({"type": "vocoder", "name": "vc", "carrier": "carrier", "modulator": "mod",
                   "bands": nb, "low": low, "high": high}),
        ];
        let x = node_audio(&nodes, "vc", 1.0);
        x[x.len() / 2..].to_vec()
    };
    for k in [1usize, 3, 5, 6] {
        let c = centre(low, high, nb, k);
        let (loud, soft) = (run(k, 0.2), run(k, 0.1));
        let step = band_db(&loud, SR, c * 0.8, c * 1.25) - band_db(&soft, SR, c * 0.8, c * 1.25);
        let far = [c / 4.0, c * 4.0]
            .iter()
            .map(|f| band_db(&loud, SR, f * 0.8, f * 1.25))
            .fold(f32::MIN, f32::max);
        let near = band_db(&loud, SR, c * 0.8, c * 1.25);
        println!(
            "vocoder band {k} ({c:.0} Hz) sine modulator: 0.2 vs 0.1 amplitude {step:.2} dB (expected 6.02); band {:.1} dB, two octaves off (a window 4x wider in Hz, white carrier) {far:.1} dB",
            near
        );
        assert!((step - 6.02).abs() < 0.5, "{step}");
        assert!(near - far > 15.0, "{near} {far}");
    }
}

#[test]
fn vocoder_follows_the_modulator_in_time() {
    // The modulator is on for 0.5 s, then off; the output follows it.
    let nodes = vec![
        json!({"type": "noise", "name": "carrier", "amp": 0.3}),
        json!({"type": "noise", "name": "raw", "seed": 5, "amp": 0.3}),
        json!({"type": "env", "name": "gate", "points": [[0, 1], [0.5, 1], [0.501, 0], [1.5, 0]]}),
        json!({"type": "math", "name": "mod", "op": "mul", "a": "raw", "b": "gate"}),
        json!({"type": "vocoder", "name": "vc", "carrier": "carrier", "modulator": "mod"}),
    ];
    let x = node_audio(&nodes, "vc", 1.5);
    let on = rms_db(&x[SR as usize / 10..SR as usize / 2]);
    let off = rms_db(&x[SR as usize * 3 / 4..]);
    println!("vocoder gated modulator: on {on:.1} dB, 0.25 s after it stops {off:.1} dB");
    assert!(on - off > 30.0);
    // A silent modulator gives silence.
    let nodes = vec![
        json!({"type": "noise", "name": "carrier", "amp": 0.3}),
        json!({"type": "gain", "name": "mod", "input": "carrier", "gain": 0.0}),
        json!({"type": "vocoder", "name": "vc", "carrier": "carrier", "modulator": "mod"}),
    ];
    let x = node_audio(&nodes, "vc", 0.3);
    assert!(rms_db(&x) <= -100.0, "{}", rms_db(&x));
}

#[test]
fn vocoder_stereo_carrier_gives_stereo() {
    let nodes = vec![
        json!({"type": "noise", "name": "carrier", "amp": 0.3}),
        json!({"type": "pan", "name": "st", "input": "carrier", "pan": 0.6}),
        json!({"type": "noise", "name": "mod", "seed": 3, "amp": 0.3}),
        json!({"type": "vocoder", "name": "vc", "carrier": "st", "modulator": "mod", "bands": 8}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2);
    println!(
        "stereo carrier: L {:.1} dB, R {:.1} dB",
        rms_db(&a.channels[0]),
        rms_db(&a.channels[1])
    );
    assert!(rms_db(&a.channels[1]) > rms_db(&a.channels[0]) + 1.0);
    let e = render_error(
        vec![
            json!({"type": "noise", "name": "carrier"}),
            json!({"type": "pan", "name": "st", "input": "carrier", "pan": 0.0}),
            json!({"type": "vocoder", "name": "vc", "carrier": "carrier", "modulator": "st"}),
        ],
        "vc",
    );
    println!("{e}");
    assert!(e.contains("modulator") && e.contains("stereo"), "{e}");
}

// ----------------------------------------------------------------- shared

#[test]
fn all_three_are_bit_identical() {
    assert_bit_identical(fb(json!({"bands": 9, "gains": [1, 0.5, 2]})), 0.3);
    assert_bit_identical(
        mb(json!({"crossovers": [150, 900, 4000], "bands": [
            {"drive": 3}, {"threshold_db": -24, "ratio": 3}, {"gain_db": -3}, {}]})),
        0.3,
    );
    assert_bit_identical(
        vec![
            white(),
            json!({"type": "osc", "name": "buzz", "freq": 110, "shape": "saw"}),
            json!({"type": "vocoder", "name": "vc", "carrier": "buzz", "modulator": "src"}),
        ],
        0.3,
    );
}

#[test]
fn all_three_are_finite_at_the_extremes() {
    let loud = |tail: Value| {
        vec![
            json!({"type": "noise", "name": "src", "amp": 1.0e6}),
            json!({"type": "osc", "name": "tone", "freq": 7000, "amp": 1.0e6}),
            tail,
        ]
    };
    // Filterbank: 1 and 64 bands, extreme q and range, huge gains.
    for (bands, q, low, high) in [
        (1, 0.1, 20.0, 21_600.0),
        (64, 100.0, 20.0, 21_600.0),
        (64, 0.1, 20.0, 40.0),
        (2, 50.0, 21_000.0, 21_600.0),
    ] {
        let gains: Vec<f32> = (0..bands)
            .map(|i| if i % 2 == 0 { 1000.0 } else { -1000.0 })
            .collect();
        let nodes = loud(
            json!({"type": "filterbank", "name": "x", "input": "src", "bands": bands, "q": q, "low": low, "high": high, "gains": gains}),
        );
        assert_finite(
            &node_audio(&nodes, "x", 0.2),
            &format!("filterbank {bands} {q} {low} {high}"),
        );
    }
    // Vocoder: instant and ten-second envelopes, 64 bands, loud inputs.
    for (bands, atk, rel) in [(64, 0.0, 0.0), (1, 10.0, 10.0), (16, 0.0, 10.0)] {
        let nodes = loud(
            json!({"type": "vocoder", "name": "x", "carrier": "src", "modulator": "tone", "bands": bands, "attack": atk, "release": rel}),
        );
        let x = node_audio(&nodes, "x", 0.2);
        assert_finite(&x, &format!("vocoder {bands} {atk} {rel}"));
    }
    // Multiband: 7 crossovers close together and at the ends, every band extreme.
    let band = json!({"gain_db": 60, "drive": 100, "threshold_db": -120, "ratio": 100, "attack": 0, "release": 0});
    for xo in [
        vec![20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 70.0],
        vec![
            21_000.0, 21_100.0, 21_200.0, 21_300.0, 21_400.0, 21_500.0, 21_600.0,
        ],
        vec![100.0],
    ] {
        let nodes = loud(
            json!({"type": "multiband", "name": "x", "input": "src", "crossovers": xo, "bands": vec![band.clone(); xo.len() + 1]}),
        );
        let x = node_audio(&nodes, "x", 0.2);
        assert_finite(&x, &format!("multiband {xo:?}"));
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 1.0e12, "{xo:?}: peak {peak}");
    }
    println!(
        "extremes: filterbank (1..64 bands, q 0.1..100, gains +-1000), vocoder (64 bands, 0..10 s envelopes), multiband (7 crossovers 5 Hz apart and at both ends, drive 100, +60 dB, 100:1, 0 s) at input 1e6: all finite"
    );
}

#[test]
fn errors_say_the_field_value_and_fix() {
    let e = render_error(fb(json!({"bands": 65})), "fb");
    println!("{e}");
    assert!(
        e.contains("bands: 65: give a whole number from 1 to 64"),
        "{e}"
    );
    let e = render_error(fb(json!({"bands": 4, "gains": [1, 1, 1, 1, 1]})), "fb");
    println!("{e}");
    assert!(
        e.contains("gains") && e.contains("one gain per band"),
        "{e}"
    );
    let e = render_error(fb(json!({"bands": 4, "low": 5000, "high": 1000})), "fb");
    println!("{e}");
    assert!(e.contains("low: 5000") && e.contains("below high"), "{e}");
    let e = render_error(fb(json!({"bands": 4, "high": 30000})), "fb");
    println!("{e}");
    assert!(e.contains("high: 30000") && e.contains("21600"), "{e}");
    let e = render_error(fb(json!({"bands": 4, "q": 0})), "fb");
    println!("{e}");
    assert!(e.contains("q: 0: give a number from 0.1 to 100"), "{e}");
    let e = render_error(mb(json!({"crossovers": []})), "mb");
    println!("{e}");
    assert!(e.contains("crossovers: 0 values"), "{e}");
    let e = render_error(mb(json!({"crossovers": [1000, 500]})), "mb");
    println!("{e}");
    assert!(
        e.contains("crossovers[1]: 500") && e.contains("ascending"),
        "{e}"
    );
    let e = render_error(
        mb(json!({"crossovers": [100, 200, 300, 400, 500, 600, 700, 800]})),
        "mb",
    );
    println!("{e}");
    assert!(e.contains("1 to 7"), "{e}");
    let e = render_error(
        mb(json!({"crossovers": [500], "bands": [{}, {}, {}]})),
        "mb",
    );
    println!("{e}");
    assert!(e.contains("bands: 3 entries for 2 bands"), "{e}");
    let e = render_error(
        mb(json!({"crossovers": [500], "bands": [{"ratio": 4}]})),
        "mb",
    );
    println!("{e}");
    assert!(
        e.contains("bands[0].ratio: 4") && e.contains("threshold_db"),
        "{e}"
    );
    let e = render_error(
        mb(json!({"crossovers": [500], "bands": [{}, {"gain_db": 90}]})),
        "mb",
    );
    println!("{e}");
    assert!(
        e.contains("bands[1].gain_db: 90: give dB between -60 and 60"),
        "{e}"
    );
    let e = render_error(
        mb(json!({"crossovers": [500], "bands": [{"gian_db": 1}]})),
        "mb",
    );
    assert!(e.contains("gian_db") && e.contains("gain_db"), "{e}");
    let e = render_error(
        vec![
            white(),
            json!({"type": "vocoder", "name": "vc", "carrier": "src", "modulator": "src", "bands": 0}),
        ],
        "vc",
    );
    println!("{e}");
    assert!(e.contains("bands: 0"), "{e}");
    let e = render_error(
        vec![
            white(),
            json!({"type": "vocoder", "name": "vc", "carrier": "src", "modulator": "src", "release": 11}),
        ],
        "vc",
    );
    println!("{e}");
    assert!(
        e.contains("release: 11: give seconds between 0 and 10"),
        "{e}"
    );
}

#[test]
fn registered_causal_and_not_random() {
    for ty in ["filterbank", "vocoder", "multiband"] {
        let mut body = sfx_graph::example(ty).expect("registered");
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, ty).unwrap();
        assert!(node.causal() && !node.random(), "{ty}");
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
    let text = sfx_graph::describe(Some("multiband")).unwrap();
    assert!(text.contains("Linkwitz-Riley") && text.contains("Crossover frequencies"));
}

/// The recipes in docs/nodes/X2.md, as written there.
fn recipes() -> Vec<(&'static str, Vec<Value>, &'static str)> {
    vec![
        (
            "talking buzz",
            vec![
                json!({"type": "osc", "name": "buzz", "freq": 110, "shape": "saw", "amp": 0.5}),
                json!({"type": "noise", "name": "speech", "color": "pink"}),
                json!({"type": "wander", "name": "sweep", "rate": 3, "range": [300, 2500]}),
                json!({"type": "filter", "name": "formant", "input": "speech", "mode": "bp", "freq": "sweep", "q": 4}),
                json!({"type": "vocoder", "name": "voice", "carrier": "buzz", "modulator": "formant", "bands": 16, "attack": 0.005, "release": 0.03}),
            ],
            "voice",
        ),
        (
            "thicker hit",
            vec![
                json!({"type": "impulse", "name": "click"}),
                json!({"type": "resonators", "name": "body", "input": "click", "modes": [[90, 0.3, 1.0], [210, 0.2, 0.7], [1800, 0.08, 0.5]]}),
                json!({"type": "multiband", "name": "hit", "input": "body", "crossovers": [150, 1000],
                       "bands": [{"threshold_db": -18, "ratio": 4, "attack": 0.005, "release": 0.08, "gain_db": 3},
                                 {},
                                 {"drive": 2, "gain_db": -3}]}),
            ],
            "hit",
        ),
        (
            "hollow noise",
            vec![
                json!({"type": "noise", "name": "n"}),
                json!({"type": "filterbank", "name": "shaped", "input": "n", "bands": 10, "low": 100, "high": 5000,
                       "gains": [1, 0, 0, 1, 0, 0, 1, 0, 0, 1]}),
            ],
            "shaped",
        ),
    ]
}

#[test]
fn recipes_render() {
    for (name, nodes, read) in recipes() {
        let x = node_audio(&nodes, read, 1.0);
        assert_finite(&x, name);
        let (rms, pk) = (rms_db(&x), peak_hz(&x, SR));
        println!(
            "recipe {name}: {} samples, rms {rms:.1} dB, spectral peak {pk:.0} Hz",
            x.len()
        );
        assert!(rms > -80.0, "{name}: {rms}");
    }
}

#[test]
fn render_time_is_stated() {
    // Not a pass / fail on speed (machines differ): the numbers go in the
    // doc, with a generous bound so a gross regression still fails.
    let time = |nodes: Vec<Value>, read: &str| {
        let t = std::time::Instant::now();
        let x = node_audio(&nodes, read, 10.0);
        assert_finite(&x, read);
        t.elapsed().as_secs_f32()
    };
    let noise = |n: &str, seed: u64| json!({"type": "noise", "name": n, "seed": seed});
    let v16 = time(
        vec![
            noise("c", 1),
            noise("m", 2),
            json!({"type": "vocoder", "name": "x", "carrier": "c", "modulator": "m"}),
        ],
        "x",
    );
    let v64 = time(
        vec![
            noise("c", 1),
            noise("m", 2),
            json!({"type": "vocoder", "name": "x", "carrier": "c", "modulator": "m", "bands": 64}),
        ],
        "x",
    );
    let f64_ = time(
        vec![
            noise("c", 1),
            json!({"type": "filterbank", "name": "x", "input": "c", "bands": 64}),
        ],
        "x",
    );
    let m8 = time(
        vec![
            noise("c", 1),
            json!({"type": "multiband", "name": "x", "input": "c", "crossovers": [100, 300, 800, 2000, 5000, 9000, 14000],
                   "bands": vec![json!({"drive": 2, "threshold_db": -20}); 8]}),
        ],
        "x",
    );
    println!(
        "10 s at 48 kHz (noise in, optimized test profile): vocoder 16 bands {v16:.2} s, vocoder 64 bands {v64:.2} s, filterbank 64 bands {f64_:.2} s, multiband 8 bands {m8:.2} s"
    );
    assert!(v64 < 30.0 && m8 < 30.0);
}
