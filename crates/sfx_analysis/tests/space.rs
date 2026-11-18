//! S11: `reverb.model` and `ir`. The first test pins the default `reverb`
//! (model fdn) bit for bit against a hash recorded before this task changed
//! the node.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use common::*;
use serde_json::{Value, json};
use sfx_graph::node::parse_body;
use sfx_graph::{RenderOpts, SoundDoc};

/// FNV-1a over the bit patterns of every sample of every channel.
fn fnv(channels: &[Vec<f32>]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for c in channels {
        for v in c {
            for b in v.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// A click and a noise burst, so the tail has broadband content.
fn source() -> Vec<Value> {
    vec![
        json!({"type": "impulse", "name": "click"}),
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "decay", "name": "e", "time": 0.01}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "mix", "name": "src", "inputs": ["click", "burst"]}),
    ]
}

fn with(extra: Value) -> Vec<Value> {
    let mut r = json!({"type": "reverb", "name": "verb", "input": "src"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let mut v = source();
    v.push(r);
    v
}

/// (label, render, hash) of the default-model configurations.
fn default_hashes() -> Vec<(&'static str, u64)> {
    let cases: Vec<(&str, Value)> = vec![
        ("default", json!({})),
        (
            "tuned",
            json!({"size": 0.8, "decay": 2.2, "damp": 0.5, "mix": 0.6, "predelay": 0.03}),
        ),
        (
            "dry_wet_extremes",
            json!({"size": 0.0, "decay": 0.01, "damp": 1.5, "mix": 1.0}),
        ),
    ];
    let mut out = Vec::new();
    for (label, extra) in cases {
        let a = render(with(extra.clone()), 1.5);
        out.push((label, fnv(&a.channels)));
        // The wet port, through a unity gain.
        let mut nodes = with(extra);
        nodes.push(json!({"type": "gain", "name": "tail", "input": "verb.wet", "gain": 1.0}));
        let b = render(nodes, 1.5);
        out.push((label, fnv(&b.channels)));
    }
    out
}

/// Recorded on the unmodified `reverb` (commit 95c71e4).
const DEFAULT_HASHES: [u64; 6] = [
    0x90e7_6a83_e2b7_e1aa,
    0x9ca5_9984_5e99_7545,
    0x3887_84e6_fe14_aaaa,
    0x106a_ebad_0160_d73f,
    0xad3e_056e_3e48_6148,
    0xad3e_056e_3e48_6148,
];

#[test]
fn default_reverb_is_bit_identical_to_before_this_task() {
    let got = default_hashes();
    println!(
        "hashes: {:016x?}",
        got.iter().map(|g| g.1).collect::<Vec<_>>()
    );
    for ((label, h), want) in got.iter().zip(DEFAULT_HASHES) {
        assert_eq!(*h, want, "{label}: the default model changed");
    }
    // `model: "fdn"` written out is the default.
    let a = render(with(json!({"model": "fdn"})), 1.5);
    let b = render(with(json!({})), 1.5);
    assert_eq!(fnv(&a.channels), fnv(&b.channels));
}

// ------------------------------------------------------------ measurement

/// Schroeder backward integration of `h`, in dB re its total, and the T60
/// extrapolated from the least-squares line between `hi` and `lo` dB (both
/// negative, `hi` > `lo`), seconds.
fn t60(h: &[f32], sr: u32, hi: f32, lo: f32) -> f32 {
    let mut acc = 0.0f64;
    let mut e: Vec<f64> = h
        .iter()
        .rev()
        .map(|v| {
            acc += f64::from(*v) * f64::from(*v);
            acc
        })
        .collect();
    e.reverse();
    let total = e[0].max(1e-30);
    let pts: Vec<(f64, f64)> = e
        .iter()
        .enumerate()
        .map(|(i, v)| {
            (
                i as f64 / f64::from(sr),
                10.0 * (v / total).max(1e-30).log10(),
            )
        })
        .filter(|(_, d)| *d <= f64::from(hi) && *d >= f64::from(lo))
        .collect();
    assert!(pts.len() > 10, "decay never reaches {lo} dB");
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f64>()
        / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>();
    (-60.0 / slope) as f32
}

/// The wet port of a reverb fed one click, left channel.
fn wet_ir(extra: Value, duration: f32) -> Vec<f32> {
    let mut nodes = vec![
        json!({"type": "impulse", "name": "src"}),
        json!({"type": "reverb", "name": "verb", "input": "src", "mix": 1.0}),
    ];
    nodes[1]
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    nodes.push(json!({"type": "gain", "name": "tail", "input": "verb.wet", "gain": 1.0}));
    node_audio(nodes, "tail", duration)
}

/// `x` with everything outside `lo..hi` Hz removed (an FFT mask).
fn band_pass(x: &[f32], sr: u32, lo: f32, hi: f32) -> Vec<f32> {
    use realfft::RealFftPlanner;
    let size = x.len().next_power_of_two();
    let mut planner = RealFftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mut a = vec![0.0f32; size];
    a[..x.len()].copy_from_slice(x);
    let mut f = fwd.make_output_vec();
    fwd.process(&mut a, &mut f).unwrap();
    for (k, z) in f.iter_mut().enumerate() {
        let hz = k as f32 * sr as f32 / size as f32;
        if hz < lo || hz >= hi {
            *z = Default::default();
        }
    }
    let mut out = inv.make_output_vec();
    inv.process(&mut f, &mut out).unwrap();
    out[..x.len()].iter().map(|v| v / size as f32).collect()
}

/// (seconds at which 10 % of the energy has arrived, energy centroid in
/// seconds) of `x[..window]`.
fn arrival(x: &[f32], sr: u32, window: usize) -> (f32, f32) {
    let e: Vec<f64> = x[..window].iter().map(|v| f64::from(*v).powi(2)).collect();
    let total: f64 = e.iter().sum();
    let mut acc = 0.0;
    let mut onset = 0;
    for (i, v) in e.iter().enumerate() {
        acc += v;
        if acc >= 0.1 * total {
            onset = i;
            break;
        }
    }
    let centroid = e.iter().enumerate().map(|(i, v)| i as f64 * v).sum::<f64>() / total;
    (onset as f32 / sr as f32, (centroid / f64::from(sr)) as f32)
}

#[test]
fn t60_is_within_20_percent_of_decay_for_every_model() {
    for model in ["fdn", "room", "plate", "spring"] {
        for decay in [0.8f32, 1.5, 3.0] {
            // The default damping and none: damping darkens the tail, so it
            // reads shorter than `decay`, which is the low-frequency time.
            for damp in [0.0, 0.3] {
                let h = wet_ir(json!({"model": model, "decay": decay, "damp": damp}), 6.0);
                assert_finite(&h, model);
                let t = t60(&h, SR, -5.0, -35.0);
                println!(
                    "{model} decay {decay} damp {damp}: T60 {t:.3} s ({:+.1} %)",
                    100.0 * (t / decay - 1.0)
                );
                assert!(
                    (t / decay - 1.0).abs() < 0.2,
                    "{model} decay {decay} damp {damp}: T60 {t}"
                );
            }
        }
    }
}

#[test]
fn spring_four_khz_arrives_after_five_hundred_hz() {
    // size 0.5: a 50 ms loop, so the first pass through the chain is
    // measured in the first 80 ms.
    let h = wet_ir(
        json!({"model": "spring", "size": 0.5, "damp": 0.0, "decay": 1.5}),
        1.0,
    );
    let window = (0.08 * SR as f32) as usize;
    let low = band_pass(&h, SR, 400.0, 600.0);
    let high = band_pass(&h, SR, 3500.0, 4500.0);
    let (lo_on, lo_c) = arrival(&low, SR, window);
    let (hi_on, hi_c) = arrival(&high, SR, window);
    println!(
        "spring first pass: 500 Hz band 10 % at {:.2} ms, centroid {:.2} ms; 4 kHz band 10 % at {:.2} ms, centroid {:.2} ms; 4 kHz is {:.2} ms later (centroid)",
        lo_on * 1e3,
        lo_c * 1e3,
        hi_on * 1e3,
        hi_c * 1e3,
        (hi_c - lo_c) * 1e3
    );
    assert!(hi_c > lo_c + 0.002, "centroid {hi_c} vs {lo_c}");
    assert!(hi_on > lo_on + 0.001, "onset {hi_on} vs {lo_on}");
}

#[test]
fn plate_builds_faster_and_denser_than_fdn() {
    // Echo density: the share of 1 ms windows in the first 60 ms whose peak
    // is within 30 dB of the loudest window.
    let density = |model: &str| {
        let h = wet_ir(json!({"model": model, "damp": 0.0}), 1.0);
        let win = SR as usize / 1000;
        let peaks: Vec<f32> = h[..SR as usize * 6 / 100]
            .chunks(win)
            .map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())))
            .collect();
        let top = peaks.iter().fold(0.0f32, |m, v| m.max(*v));
        let live = peaks.iter().filter(|p| **p > top * 0.0316).count();
        let first = h.iter().position(|v| v.abs() > top * 0.0316).unwrap();
        (live as f32 / peaks.len() as f32, first as f32 / SR as f32)
    };
    let (fdn_d, fdn_t) = density("fdn");
    let (plate_d, plate_t) = density("plate");
    println!(
        "echo density in the first 60 ms: fdn {fdn_d:.2} (first echo {:.1} ms), plate {plate_d:.2} (first echo {:.1} ms)",
        fdn_t * 1e3,
        plate_t * 1e3
    );
    assert!(plate_d >= fdn_d && plate_t <= fdn_t);
}

fn ir_node(extra: Value) -> Value {
    let mut n = json!({"type": "ir", "name": "ir1"});
    n.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    n
}

fn ir(extra: Value, duration: f32) -> Vec<f32> {
    node_audio(vec![ir_node(extra)], "ir1", duration)
}

/// |H(f)| of the response `h`, dB (a direct DFT at one frequency).
fn gain_db(h: &[f32], sr: u32, hz: f32) -> f32 {
    let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, v) in h.iter().enumerate() {
        re += f64::from(*v) * (w * n as f64).cos();
        im -= f64::from(*v) * (w * n as f64).sin();
    }
    (20.0 * (re.hypot(im)).max(1e-12).log10()) as f32
}

#[test]
fn room_ir_energy_decay_matches_sabine() {
    for (size, abs) in [
        (6.0f32, 0.3f32),
        (6.0, 0.1),
        (12.0, 0.2),
        (3.0, 0.5),
        (20.0, 0.05),
    ] {
        let h = ir(
            json!({"model": "room", "size": size, "absorption": abs}),
            12.0,
        );
        assert_finite(&h, "room ir");
        // Sides 1 : 0.8 : 0.4.
        let v = size * size * 0.8 * size * 0.4;
        let surface = 2.0 * (size * size * 0.8 + size * 0.8 * size * 0.4 + size * size * 0.4);
        let sabine = 0.161 * v / (surface * abs);
        let t = t60(&h, SR, -5.0, -35.0);
        println!(
            "ir room size {size} absorption {abs}: Sabine T60 {sabine:.3} s, measured {t:.3} s ({:+.1} %), direct sound {:.6} at sample 0",
            100.0 * (t / sabine - 1.0),
            h[0]
        );
        assert!((t / sabine - 1.0).abs() < 0.25, "{t} vs {sabine}");
        assert!((h[0] - 1.0).abs() < 1e-6, "the direct sound is at sample 0");
    }
}

#[test]
fn room_ir_is_stereo_and_seeded() {
    let nodes = vec![
        ir_node(json!({"model": "room"})),
        json!({"type": "impulse", "name": "src"}),
        json!({"type": "convolve", "name": "wetsig", "input": "src", "ir": "ir1"}),
    ];
    let a = render(&nodes, 0.5);
    assert_eq!(a.channels.len(), 2, "a room is two ears");
    let (l, r) = (&a.channels[0], &a.channels[1]);
    let dot: f64 = l.iter().zip(r).map(|(a, b)| f64::from(a * b)).sum();
    let corr = dot
        / (l.iter().map(|v| f64::from(v * v)).sum::<f64>().sqrt()
            * r.iter().map(|v| f64::from(v * v)).sum::<f64>().sqrt());
    println!("room ir: left/right correlation {corr:.3}");
    assert!(corr < 0.9 && corr > -0.9);
    let one = ir(json!({"model": "room", "seed": 1}), 0.5);
    let again = ir(json!({"model": "room", "seed": 1}), 0.5);
    let other = ir(json!({"model": "room", "seed": 2}), 0.5);
    assert!(
        one.iter()
            .zip(&again)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert!(one.iter().zip(&other).any(|(a, b)| a != b));
}

#[test]
fn convolve_with_an_ir_node_returns_the_response() {
    let nodes = vec![
        ir_node(json!({"model": "phone"})),
        json!({"type": "impulse", "name": "src"}),
        json!({"type": "convolve", "name": "c", "input": "src", "ir": "ir1"}),
    ];
    let h = ir(json!({"model": "phone"}), 0.3);
    let c = node_audio(nodes, "c", 0.3);
    let worst = h
        .iter()
        .zip(&c)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("convolve(impulse, ir phone) differs from the ir by at most {worst:.2e}");
    assert!(worst < 1e-4);
}

#[test]
fn phone_ir_passes_300_to_3400_and_cuts_100_and_8000() {
    let h = ir(json!({"model": "phone"}), 0.2);
    assert_finite(&h, "phone");
    let reference = gain_db(&h, SR, 1000.0);
    let rel = |hz| gain_db(&h, SR, hz) - reference;
    let (a, b, c, d) = (rel(300.0), rel(3400.0), rel(100.0), rel(8000.0));
    println!(
        "ir phone re 1 kHz ({reference:.2} dB): 300 Hz {a:.2} dB, 3.4 kHz {b:.2} dB, 100 Hz {c:.1} dB, 8 kHz {d:.1} dB"
    );
    assert!(a > -3.0 && b > -3.0, "{a} {b}");
    assert!(c < -30.0 && d < -30.0, "{c} {d}");
}

#[test]
fn cabinet_ir_has_a_low_resonance_and_rolls_off() {
    let h = ir(json!({"model": "cabinet", "size": 0.6}), 0.3);
    assert_finite(&h, "cabinet");
    let low: Vec<(f32, f32)> = (30..400)
        .map(|hz| (hz as f32, gain_db(&h, SR, hz as f32)))
        .collect();
    let top = low
        .iter()
        .fold((0.0, f32::MIN), |m, p| if p.1 > m.1 { *p } else { m });
    let reference = gain_db(&h, SR, 1000.0);
    let (g40, g10k) = (gain_db(&h, SR, 40.0), gain_db(&h, SR, 10_000.0));
    println!(
        "ir cabinet: resonance {:.0} Hz at {:.1} dB re 1 kHz (expected {:.0} Hz); 40 Hz {:.1} dB, 10 kHz {:.1} dB",
        top.0,
        top.1 - reference,
        70.0 / 0.6,
        g40 - reference,
        g10k - reference
    );
    assert!((top.0 - 70.0 / 0.6).abs() < 0.2 * 70.0 / 0.6, "{}", top.0);
    assert!(g40 - reference < -10.0 && g10k - reference < -20.0);
}

#[test]
fn tube_ir_has_modes_at_k_c_over_2l() {
    let h = ir(
        json!({"model": "tube", "size": 1.0, "absorption": 0.05}),
        1.0,
    );
    assert_finite(&h, "tube");
    let f1 = 343.0 / 2.0;
    let p = peaks(&h, SR, 12, 60.0);
    let near = |want: f32| {
        p.iter()
            .map(|q| q.0)
            .find(|hz| (hz - want).abs() < 0.01 * want)
    };
    let found: Vec<Option<f32>> = (1..=4).map(|k| near(k as f32 * f1)).collect();
    println!(
        "ir tube 1 m: modes wanted {:.1} {:.1} {:.1} {:.1} Hz, found {found:.1?}",
        f1,
        2.0 * f1,
        3.0 * f1,
        4.0 * f1
    );
    assert!(found.iter().all(Option::is_some));
}

#[test]
fn box_ir_has_the_modes_of_all_three_axes() {
    let h = ir(
        json!({"model": "box", "size": 0.5, "absorption": 0.05}),
        1.0,
    );
    assert_finite(&h, "box");
    let p = peaks(&h, SR, 40, 40.0);
    // 0.5 m, 0.4 m and 0.2 m axes: first modes 343, 428.75 and 857.5 Hz.
    let wanted = [343.0f32, 428.75, 857.5];
    let found: Vec<Option<f32>> = wanted
        .iter()
        .map(|w| p.iter().map(|q| q.0).find(|hz| (hz - w).abs() < 0.01 * w))
        .collect();
    println!("ir box 0.5 m: modes wanted {wanted:?} Hz, found {found:.1?}");
    assert!(found.iter().all(Option::is_some));
}

#[test]
fn room_reverb_first_reflection_arrives_when_the_geometry_says() {
    // size 0.5: sides 11.5 x 9.2 x 4.6 m. The nearest first-order image to
    // the left ear, from the geometry alone.
    let l = 3.0 + 17.0 * 0.5f32;
    let dims = [l, l * 0.8, l * 0.4];
    let src = [dims[0] * 0.3, dims[1] * 0.45, dims[2] * 0.4];
    let ear = |side: f32| [dims[0] * 0.7, dims[1] * 0.5 + side * 0.09, dims[2] * 0.4];
    let dist = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt();
    let r0 = dist(src, ear(-1.0)).min(dist(src, ear(1.0)));
    let mut first = f32::MAX;
    for axis in 0..3 {
        for wall in [0.0, dims[axis]] {
            let mut img = src;
            img[axis] = 2.0 * wall - src[axis];
            first = first.min(dist(img, ear(-1.0)));
        }
    }
    let want = (first - r0) / 343.0;
    let h = wet_ir(
        json!({"model": "room", "er_mix": 1.0, "predelay": 0.0}),
        0.5,
    );
    let got = h.iter().position(|v| v.abs() > 1e-9).unwrap() as f32 / SR as f32;
    println!(
        "room reverb, left ear: first reflection expected {:.3} ms, found {:.3} ms",
        want * 1e3,
        got * 1e3
    );
    assert!((got - want).abs() < 2.0 / SR as f32);
    // The early part carries energy the fdn alone has not built yet.
    let fdn = wet_ir(json!({"model": "fdn"}), 0.5);
    let early = |x: &[f32]| rms_db(&x[..SR as usize / 20]);
    println!(
        "first 50 ms rms: room er_mix 1 {:.1} dB, fdn {:.1} dB",
        early(&h),
        early(&fdn)
    );
    assert!(early(&h) > early(&fdn));
}

#[test]
fn width_scales_the_side_signal() {
    let lr = |w: Option<f32>| {
        let extra = match w {
            Some(w) => json!({"model": "plate", "width": w}),
            None => json!({"model": "plate"}),
        };
        let mut nodes = source();
        nodes.push(json!({"type": "reverb", "name": "verb", "input": "src", "mix": 1.0}));
        nodes[5]
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let a = render(&nodes, 0.6);
        (a.channels[0].clone(), a.channels[1].clone())
    };
    let side_rms = |(l, r): &(Vec<f32>, Vec<f32>)| {
        let s: Vec<f32> = l.iter().zip(r).map(|(a, b)| (a - b) * 0.5).collect();
        rms_db(&s)
    };
    let (mono, own, wide) = (lr(Some(0.0)), lr(None), lr(Some(2.0)));
    println!(
        "plate side rms: width 0 {:.1} dB, own {:.1} dB, width 2 {:.1} dB",
        side_rms(&mono),
        side_rms(&own),
        side_rms(&wide)
    );
    assert!(
        mono.0
            .iter()
            .zip(&mono.1)
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    assert!((side_rms(&wide) - side_rms(&own) - 6.02).abs() < 0.3);
}

#[test]
fn every_model_is_bit_identical() {
    for model in ["fdn", "room", "plate", "spring"] {
        assert_bit_identical(with(json!({"model": model, "width": 1.5})), 0.8);
    }
    for model in ["room", "tube", "box", "cabinet", "phone"] {
        let mut nodes = source();
        nodes.push(ir_node(json!({"model": model})));
        nodes.push(json!({"type": "convolve", "name": "c", "input": "src", "ir": "ir1"}));
        assert_bit_identical(nodes, 0.8);
    }
}

#[test]
fn every_model_is_finite_at_the_extremes() {
    let loud = vec![json!({"type": "osc", "name": "src", "freq": 220, "amp": 1.0e3})];
    for model in ["fdn", "room", "plate", "spring"] {
        for size in [0.0, 1.0] {
            for decay in [0.0, 1.0e6] {
                for damp in [0.0, 1.0] {
                    for (predelay, width) in [(0.0, 0.0), (1.0e9, 2.0)] {
                        let mut nodes = loud.clone();
                        nodes.push(json!({"type": "reverb", "name": "verb", "input": "src",
                            "model": model, "size": size, "decay": decay, "damp": damp,
                            "predelay": predelay, "width": width, "mix": 1.0}));
                        let x = node_audio(&nodes, "verb", 0.6);
                        assert_finite(&x, &format!("{model} {size} {decay} {damp}"));
                        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                        assert!(
                            peak < 1.0e5,
                            "{model} size {size} decay {decay}: peak {peak}"
                        );
                    }
                }
            }
        }
    }
    for model in ["room", "tube", "box", "cabinet", "phone"] {
        for size in [0.05, 100.0] {
            for absorption in [0.005, 1.0] {
                let mut extra = json!({"model": model, "length": 30.0});
                if model != "phone" {
                    extra["size"] = json!(size);
                }
                if matches!(model, "room" | "tube" | "box") {
                    extra["absorption"] = json!(absorption);
                }
                let h = ir(extra, 2.0);
                assert_finite(&h, &format!("ir {model} {size} {absorption}"));
                let peak = h.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(peak < 100.0, "ir {model} {size} {absorption}: peak {peak}");
            }
        }
    }
    println!(
        "extremes: reverb 4 models x size 0/1 x decay 0/1e6 x damp 0/1 x predelay 0/1e9 at input 1e3, and ir 5 models x size 0.05/100 x absorption 0.005/1 x length 30: all finite and bounded"
    );
}

fn render_error(nodes: Vec<Value>) -> String {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "last"}));
    d.duration = Some(0.1);
    d.render(&RenderOpts::default()).unwrap_err()
}

#[test]
fn errors_say_the_field_value_and_fix() {
    let verb = |extra: Value| {
        let mut v = with(extra);
        v.last_mut().unwrap()["name"] = json!("last");
        render_error(v)
    };
    let e = verb(json!({"er_mix": 0.5}));
    println!("{e}");
    assert!(
        e.contains("er_mix: 0.5") && e.contains("\"model\": \"room\""),
        "{e}"
    );
    let e = verb(json!({"model": "room", "er_mix": 1.5}));
    println!("{e}");
    assert!(e.contains("er_mix: 1.5: give a number from 0 to 1"), "{e}");
    let e = verb(json!({"width": 3}));
    println!("{e}");
    assert!(
        e.contains("width: 3: give a number from 0 (mono) to 2"),
        "{e}"
    );
    let e = verb(json!({"model": "hall"}));
    println!("{e}");
    assert!(e.contains("hall") && e.contains("spring"), "{e}");
    let ir_err = |extra: Value| {
        let mut n = ir_node(extra);
        n["name"] = json!("last");
        render_error(vec![n])
    };
    let e = ir_err(json!({"size": 500}));
    println!("{e}");
    assert!(
        e.contains("size: 500: give a number from 0.05 to 100"),
        "{e}"
    );
    let e = ir_err(json!({"absorption": 0}));
    println!("{e}");
    assert!(
        e.contains("absorption: 0: give a number from 0.005 to 1"),
        "{e}"
    );
    let e = ir_err(json!({"length": 60}));
    println!("{e}");
    assert!(
        e.contains("length: 60: give a number from 0.01 to 30"),
        "{e}"
    );
    let e = ir_err(json!({"model": "phone", "size": 2}));
    println!("{e}");
    assert!(e.contains("size: 2: a phone has no size"), "{e}");
    let e = ir_err(json!({"model": "cabinet", "absorption": 0.2}));
    println!("{e}");
    assert!(
        e.contains("absorption: 0.2: only room, tube and box"),
        "{e}"
    );
    let e = ir_err(json!({"model": "garage"}));
    println!("{e}");
    assert!(e.contains("garage") && e.contains("cabinet"), "{e}");
}

#[test]
fn ir_is_registered_and_reverb_documents_its_models() {
    let ex = sfx_graph::example("ir").expect("registered");
    let mut body = ex.clone();
    body.as_object_mut().unwrap().remove("type");
    body.as_object_mut().unwrap().remove("name");
    let node = parse_body("ir", &body, "ir1").unwrap();
    assert!(node.causal() && node.random());
    let phone = parse_body("ir", &json!({"model": "phone"}), "ir1").unwrap();
    assert!(!phone.random(), "only the room reads the seed");
    let text = sfx_graph::describe(Some("ir")).unwrap();
    assert!(text.contains("impulse response") && text.contains("Sabine"));
    let text = sfx_graph::describe(Some("reverb")).unwrap();
    assert!(text.contains("spring") && text.contains("er_mix") && text.contains("width"));
    assert!(sfx_graph::describe(None).unwrap().contains("- ir:"));
}

/// The recipe in docs/nodes/space.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "noise", "name": "n", "color": "white"}),
        json!({"type": "decay", "name": "e", "time": 0.004}),
        json!({"type": "math", "name": "snap", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "ir", "name": "hall", "model": "room", "size": 14, "absorption": 0.15}),
        json!({"type": "convolve", "name": "inroom", "input": "snap", "ir": "hall", "mix": 1.0}),
        json!({"type": "reverb", "name": "boing", "input": "snap", "model": "spring", "size": 0.4, "decay": 1.2, "mix": 1.0}),
        json!({"type": "mix", "name": "both", "inputs": ["inroom", "boing"], "to": "stereo"}),
    ]
}

#[test]
fn recipe_renders() {
    let a = render(recipe(), 2.0);
    assert_eq!(a.len(), 2 * SR as usize);
    let l = &a.channels[0];
    println!("recipe: {} samples, left rms {:.2} dB", a.len(), rms_db(l));
    assert_finite(l, "recipe");
    assert!(rms_db(l) > -80.0);
}
