//! S8a: the senses for synthesized sources (aliasing, inharmonicity,
//! modulation, roughness) and the sturdier `decay` / `swing` measures.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::measure::{
    aliasing_db, amplitude_modulation, extra, inharmonicity_cents, measure, pitch_modulation,
    roughness_pct,
};
use sfx_analysis::{CheckCx, CheckResult, Expect, run};
use sfx_graph::{Audio, Measure, RenderOpts, SoundDoc};

fn doc_at(nodes: Vec<Value>, secs: f32, sr: u32) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "out", "input": "src"}));
    d.duration = Some(secs);
    d.sample_rate = sr;
    d
}

fn check(d: &SoundDoc, e: Value) -> Vec<CheckResult> {
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap_or_else(|e| panic!("{e}"));
    let e: Expect = serde_json::from_value(e).unwrap_or_else(|e| panic!("{e}"));
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

fn render(d: &SoundDoc) -> Audio {
    d.render(&RenderOpts::default()).unwrap().audio
}

fn mono(sr: u32, secs: f32, f: impl Fn(f32) -> f32) -> Audio {
    Audio::mono(
        sr,
        (0..(secs * sr as f32) as usize)
            .map(|i| f(i as f32 / sr as f32))
            .collect(),
    )
}

const TAU: f32 = std::f32::consts::TAU;

/// The `decay` formula before S8a (10 ms frames, last frame within 6 dB of
/// the peak to the last within 40 dB).
fn old_decay(audio: &Audio) -> f32 {
    let x = audio.mix_mono();
    let fr = (audio.sample_rate / 100).max(1) as usize;
    let lev: Vec<f32> = x
        .chunks(fr)
        .map(|c| {
            10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)
                .max(1e-20)
                .log10()
        })
        .collect();
    let top = lev.iter().copied().reduce(f32::max).unwrap();
    let last = |drop: f32| lev.iter().rposition(|v| *v > top - drop).unwrap_or(0);
    (last(40.0).saturating_sub(last(6.0))) as f32 / 100.0
}

fn old_swing(audio: &Audio) -> f32 {
    let lc: Vec<f32> = sfx_graph::loudness::curve(&audio.channels, audio.sample_rate, 0.2, 0.05)
        .into_iter()
        .filter(|v| *v > -70.0)
        .collect();
    if lc.len() < 3 {
        return 0.0;
    }
    let mut s = lc;
    s.sort_by(f32::total_cmp);
    s[s.len() * 9 / 10] - s[s.len() / 10]
}

fn bounce(restitution: f32, first: f32) -> Audio {
    let d = SoundDoc {
        nodes: vec![
            json!({"type": "bounce", "name": "ev", "first": first, "restitution": restitution}),
            json!({"type": "osc", "name": "o", "freq": 180, "amp": 0.8}),
            json!({"type": "decay", "name": "e", "time": 0.04}),
            json!({"type": "math", "name": "thump", "op": "mul", "a": "o", "b": "e"}),
            json!({"type": "grains", "name": "src", "source": "thump", "events": "ev", "size": 0.15}),
            json!({"type": "output", "name": "out", "input": "src"}),
        ],
        duration: Some(3.0),
        ..SoundDoc::named("t")
    };
    render(&d)
}

/// White noise rising by `depth` dB over its `secs` (a crescendo / swell).
fn swell(depth: f32, secs: f32) -> Audio {
    let d = SoundDoc {
        nodes: vec![
            json!({"type": "noise", "name": "n", "color": "white", "amp": 0.5}),
            json!({"type": "expr", "name": "src", "inputs": {"a": "n"},
                   "out": format!("a * pow(10, 0 - {depth} * (1 - t / {secs}) / 20)")}),
            json!({"type": "output", "name": "out", "input": "src", "fade_in": 0, "fade_out": 0}),
        ],
        duration: Some(secs),
        ..SoundDoc::named("t")
    };
    render(&d)
}

/// A click: noise times a decay of `time` seconds.
fn click(time: f32, secs: f32) -> Audio {
    let d = SoundDoc {
        nodes: vec![
            json!({"type": "noise", "name": "n", "color": "white", "amp": 0.5}),
            json!({"type": "decay", "name": "e", "time": time}),
            json!({"type": "math", "name": "src", "op": "mul", "a": "n", "b": "e"}),
            json!({"type": "output", "name": "out", "input": "src"}),
        ],
        duration: Some(secs),
        ..SoundDoc::named("t")
    };
    render(&d)
}

fn line<'a>(res: &'a [CheckResult], name: &str) -> &'a CheckResult {
    res.iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("{name}: {res:?}"))
}

fn naive_saw(hz: f32, sr: u32) -> SoundDoc {
    doc_at(
        vec![json!({"type": "expr", "name": "src", "inputs": {},
                    "out": format!("0.5 * (2 * fract(t * {hz}) - 1)")})],
        1.0,
        sr,
    )
}

fn osc_saw(hz: f32, sr: u32) -> SoundDoc {
    doc_at(
        vec![json!({"type": "osc", "name": "src", "shape": "saw", "freq": hz, "amp": 0.5})],
        1.0,
        sr,
    )
}

#[test]
fn a_naive_saw_at_3khz_fails_aliasing_and_the_osc_saw_passes() {
    let (bad, good) = (naive_saw(3000.0, 44_100), osc_saw(3000.0, 44_100));
    let (vb, vg) = (
        aliasing_db(&render(&bad)).unwrap(),
        aliasing_db(&render(&good)).unwrap(),
    );
    println!(
        "3 kHz saw at 44.1 kHz: naive (expr) {vb:.1} dB, osc {vg:.1} dB off the harmonic grid above 4 kHz"
    );
    assert!(vb > -15.0 && vg < -20.0 && vb - vg > 10.0, "{vb} {vg}");
    let e = json!({"aliasing": {"max_db": -20}});
    let r = check(&bad, e.clone());
    let l = line(&r, "aliasing");
    println!("{}", l.detail);
    assert!(!l.pass && l.detail.contains("likely aliasing"), "{l:?}");
    let r = check(&good, e);
    assert!(line(&r, "aliasing").pass, "{r:?}");
    // At musical pitches the default (-35 dB) separates them too.
    let (vb, vg) = (
        aliasing_db(&render(&naive_saw(440.0, 48_000))).unwrap(),
        aliasing_db(&render(&osc_saw(440.0, 48_000))).unwrap(),
    );
    println!("440 Hz saw at 48 kHz: naive {vb:.1} dB, osc {vg:.1} dB");
    assert!(vb > -30.0 && vg < -35.0, "{vb} {vg}");
    let r = check(&naive_saw(440.0, 48_000), json!({"aliasing": true}));
    assert!(!line(&r, "aliasing").pass);
    let r = check(&osc_saw(440.0, 48_000), json!({"aliasing": true}));
    assert!(line(&r, "aliasing").pass, "{r:?}");
}

#[test]
fn aliasing_names_the_shape_node_to_oversample() {
    let d = doc_at(
        vec![
            json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.9}),
            json!({"type": "shape", "name": "src", "input": "tone", "mode": "fold", "drive": 6}),
        ],
        1.0,
        44_100,
    );
    let v = aliasing_db(&render(&d)).unwrap();
    println!("1 kHz sine folded (drive 6) at 44.1 kHz: {v:.1} dB");
    let r = check(&d, json!({"aliasing": {"max_db": -60}}));
    let l = line(&r, "aliasing");
    println!("{}", l.detail);
    assert!(
        !l.pass && l.detail.contains("raise `oversample` on shape `src`"),
        "{l:?}"
    );
    // A pure sine has none.
    let sine = mono(48_000, 1.0, |t| 0.5 * (TAU * 1000.0 * t).sin());
    let v = aliasing_db(&sine).unwrap();
    println!("pure 1 kHz sine: {v:.1} dB");
    assert!(v < -55.0, "{v}");
    // A clean sweep reads clean (only its steadiest frames count).
    let sweep = mono(48_000, 1.0, |t| {
        0.5 * (TAU * (200.0 * t + 400.0 * t * t)).sin()
    });
    println!("sweep: {:?}", aliasing_db(&sweep));
    assert!(aliasing_db(&sweep).is_none_or(|v| v < -55.0));
    // White noise has no pitch to measure.
    let noise = render(&doc_at(
        vec![json!({"type": "noise", "name": "src", "color": "white", "amp": 0.3})],
        1.0,
        48_000,
    ));
    println!("noise: {:?}", aliasing_db(&noise));
    assert!(aliasing_db(&noise).is_none());
}

#[test]
fn inharmonicity_tells_a_harmonic_tone_from_a_bell() {
    let sr = 48_000;
    let harmonic = mono(sr, 1.0, |t| {
        (1..=6)
            .map(|k| (TAU * 200.0 * k as f32 * t).sin() / k as f32)
            .sum::<f32>()
            * 0.3
    });
    let bell = mono(sr, 1.0, |t| {
        [1.0f32, 2.1, 3.3]
            .iter()
            .map(|k| (TAU * 200.0 * k * t).sin() * 0.3)
            .sum::<f32>()
    });
    let (h, b) = (
        inharmonicity_cents(&harmonic).unwrap(),
        inharmonicity_cents(&bell).unwrap(),
    );
    println!(
        "inharmonicity: 6 harmonics of 200 Hz {h:.2} cents; partials 1, 2.1, 3.3 x 200 Hz {b:.1} cents"
    );
    assert!(h < 2.0, "{h}");
    assert!(b > 50.0, "{b}");
    // A noisy sound has no clear partials.
    let noise = render(&doc_at(
        vec![json!({"type": "noise", "name": "src", "color": "white", "amp": 0.3})],
        1.0,
        sr,
    ));
    println!(
        "inharmonicity of white noise: {:?}",
        inharmonicity_cents(&noise)
    );
    // Through the check, on a rendered sound.
    let d = doc_at(
        vec![json!({"type": "resonators", "name": "src", "input": "hit",
                    "modes": [[300, 0.8, 1.0], [630, 0.6, 0.8], [990, 0.5, 0.6]]})],
        1.5,
        48_000,
    );
    let mut d = d;
    d.nodes.insert(0, json!({"type": "impulse", "name": "hit"}));
    let r = check(&d, json!({"inharmonicity": {"min": 50}}));
    println!("{}", line(&r, "inharmonicity").detail);
    assert!(line(&r, "inharmonicity").pass, "{r:?}");
    let r = check(&d, json!({"inharmonicity": {"max": 10}}));
    let l = line(&r, "inharmonicity");
    println!("{}", l.detail);
    assert!(
        !l.pass && l.detail.contains("off integer multiples"),
        "{l:?}"
    );
}

#[test]
fn a_6hz_tremolo_of_6db_reads_6hz_and_6db() {
    let sr = 48_000;
    // 6 dB peak-to-peak, sinusoidal in dB (+-3 dB).
    let trem = mono(sr, 2.0, |t| {
        (TAU * 1000.0 * t).sin() * 10f32.powf(3.0 * (TAU * 6.0 * t).sin() / 20.0) * 0.2
    });
    let m = amplitude_modulation(&trem);
    println!("tremolo 6 Hz, +-3 dB: {m:?}");
    assert!(
        (m[0].rate_hz - 6.0).abs() < 0.2 && (m[0].depth - 6.0).abs() < 1.0,
        "{m:?}"
    );
    // A linear tremolo whose gain runs 0.5 .. 1 (6.02 dB between the extremes).
    let lin = mono(sr, 2.0, |t| {
        (TAU * 1000.0 * t).sin() * (0.75 + 0.25 * (TAU * 6.0 * t).sin()) * 0.5
    });
    let m = amplitude_modulation(&lin);
    println!("tremolo 6 Hz, gain 0.5..1: {m:?}");
    assert!(
        (m[0].rate_hz - 6.0).abs() < 0.2 && (m[0].depth - 6.0).abs() < 1.0,
        "{m:?}"
    );
    // Two modulators: both rates are listed, strongest first.
    let two = mono(sr, 3.0, |t| {
        (TAU * 800.0 * t).sin()
            * 10f32.powf((3.0 * (TAU * 4.0 * t).sin() + 1.5 * (TAU * 17.0 * t).sin()) / 20.0)
            * 0.2
    });
    let m = amplitude_modulation(&two);
    println!("4 Hz +-3 dB and 17 Hz +-1.5 dB: {m:?}");
    assert!(m.len() >= 2 && (m[0].rate_hz - 4.0).abs() < 0.2 && (m[1].rate_hz - 17.0).abs() < 0.3);
    assert!((m[1].depth - 3.0).abs() < 0.7, "{m:?}");
    // A steady tone has none; an exponential decay is detrended away.
    let steady = mono(sr, 2.0, |t| (TAU * 1000.0 * t).sin() * 0.3);
    assert!(amplitude_modulation(&steady).is_empty());
    let decay = mono(sr, 2.0, |t| (TAU * 1000.0 * t).sin() * (-t * 3.0).exp());
    println!("decaying tone: {:?}", amplitude_modulation(&decay));
    assert!(amplitude_modulation(&decay).iter().all(|p| p.depth < 1.0));
    // Through the check on a rendered sound.
    let d = doc_at(
        vec![
            json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.4}),
            json!({"type": "lfo", "name": "lf", "freq": 6, "range": [0.5, 1.0]}),
            json!({"type": "math", "name": "src", "op": "mul", "a": "tone", "b": "lf"}),
        ],
        2.0,
        48_000,
    );
    let r = check(&d, json!({"modulation": {"rate_hz": 6.0, "tol_hz": 0.3}}));
    println!("{}", line(&r, "modulation").detail);
    let r = check(&d, json!({"modulation": {"rate_hz": 9.0}}));
    let l = line(&r, "modulation");
    println!("{}", l.detail);
    assert!(!l.pass && l.detail.contains("no rate within"), "{l:?}");
}

#[test]
fn a_5hz_vibrato_reads_5hz_in_the_pitch_track() {
    let sr = 48_000;
    let mut ph = 0.0f32;
    let vib = Audio::mono(
        sr,
        (0..2 * sr as usize)
            .map(|i| {
                let t = i as f32 / sr as f32;
                let f = 440.0 * 2f32.powf(30.0 * (TAU * 5.0 * t).sin() / 1200.0);
                ph += TAU * f / sr as f32;
                ph.sin() * 0.5
            })
            .collect(),
    );
    let m = pitch_modulation(&vib);
    println!("vibrato 5 Hz +-30 cents: {m:?}");
    assert!(
        (m[0].rate_hz - 5.0).abs() < 0.3 && (m[0].depth - 60.0).abs() < 8.0,
        "{m:?}"
    );
    let steady = mono(sr, 2.0, |t| (TAU * 440.0 * t).sin() * 0.5);
    assert!(pitch_modulation(&steady).is_empty());
}

#[test]
fn two_sines_40hz_apart_are_rougher_than_two_400hz_apart() {
    let sr = 48_000;
    let pair = |a: f32, b: f32| {
        mono(sr, 1.0, |t| {
            ((TAU * a * t).sin() + (TAU * b * t).sin()) * 0.25
        })
    };
    let near = roughness_pct(&pair(1000.0, 1040.0));
    let far = roughness_pct(&pair(1000.0, 1400.0));
    let low_near = roughness_pct(&pair(400.0, 440.0));
    let one = roughness_pct(&pair(1000.0, 1000.0));
    println!(
        "roughness: 40 Hz apart {near:.2} %, {low_near:.2} % (at 400 Hz); 400 Hz apart {far:.3} %; one tone {one:.3} %"
    );
    assert!(near > 5.0 && near > 20.0 * far.max(0.01), "{near} {far}");
    let d = doc_at(
        vec![
            json!({"type": "osc", "name": "a", "freq": 1000, "amp": 0.3}),
            json!({"type": "osc", "name": "b", "freq": 1040, "amp": 0.3}),
            json!({"type": "mix", "name": "src", "inputs": ["a", "b"]}),
        ],
        1.0,
        48_000,
    );
    let r = check(&d, json!({"roughness": {"min": 5}}));
    println!("{}", line(&r, "roughness").detail);
    assert!(line(&r, "roughness").pass);
    let r = check(&d, json!({"roughness": {"max": 2}}));
    assert!(!line(&r, "roughness").pass);
}

#[test]
fn the_extra_senses_are_deterministic_finite_and_survive_degenerate_input() {
    let sr = 48_000;
    let a = mono(sr, 1.0, |t| {
        (TAU * 220.0 * t).sin() * (1.0 + 0.5 * (TAU * 7.0 * t).sin()) * 0.3
    });
    let (p, q) = (extra(&a), extra(&a));
    assert_eq!(format!("{p:?}"), format!("{q:?}"));
    for line in p.lines() {
        println!("{line}");
    }
    for audio in [
        Audio::mono(sr, vec![]),
        Audio::mono(sr, vec![0.0; 10]),
        Audio::mono(sr, vec![0.0; 48_000]),
        Audio::mono(sr, vec![1.0; 5000]),
        Audio::mono(sr, vec![f32::MAX / 4.0; 3000]),
        Audio::mono(8000, vec![0.1; 40_000]),
    ] {
        let e = extra(&audio);
        assert!(e.roughness_pct.is_finite());
        assert!(e.aliasing_db.is_none_or(f32::is_finite));
        assert!(e.inharmonicity_cents.is_none_or(f32::is_finite));
        for m in e.amp_mod.iter().chain(&e.pitch_mod) {
            assert!(m.rate_hz.is_finite() && m.depth.is_finite());
        }
        for m in [Measure::Decay, Measure::Swing] {
            assert!(measure(&audio, m).is_finite());
        }
    }
}

#[test]
fn a_bad_modulation_mode_is_an_error_that_says_what_to_use() {
    let d = osc_saw(440.0, 48_000);
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap();
    let e: Expect =
        serde_json::from_value(json!({"modulation": {"rate_hz": 5, "mode": "phase"}})).unwrap();
    let err = run(
        &e,
        &CheckCx {
            doc: &d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_err();
    println!("{err}");
    assert!(err.contains("\"amplitude\" or \"pitch\""), "{err}");
}

// ---- decay and swing ------------------------------------------------------

#[test]
fn decay_and_swing_keep_their_values_on_sounds_that_already_worked() {
    // One-hit sounds of 0.5 s and longer (decaying noise at several
    // lengths, a pitched ring) read exactly as before.
    for (time, secs) in [(0.05f32, 0.6f32), (0.1, 1.0), (0.3, 2.0), (0.8, 4.0)] {
        let a = click(time, secs);
        let (n, o) = (measure(&a, Measure::Decay), old_decay(&a));
        println!("decay noise x decay({time}) over {secs} s: new {n:.3} old {o:.3}");
        assert_eq!(n.to_bits(), o.to_bits(), "{time} {secs}");
    }
    let ring = mono(48_000, 2.0, |t| {
        (TAU * 600.0 * t).sin() * (-t * 6.0).exp() * 0.5
    });
    assert_eq!(
        measure(&ring, Measure::Decay).to_bits(),
        old_decay(&ring).to_bits()
    );
    // Swing: identical from 0.6 s up.
    for secs in [0.6f32, 1.0, 2.5] {
        for depth in [0.0f32, 9.0, 24.0] {
            let a = swell(depth, secs);
            let (n, o) = (measure(&a, Measure::Swing), old_swing(&a));
            assert_eq!(n.to_bits(), o.to_bits(), "{secs} {depth}: {n} {o}");
        }
    }
}

#[test]
fn decay_of_a_bouncing_ball_grows_with_restitution_and_with_the_first_gap() {
    let by_r: Vec<(f32, f32)> = [0.5f32, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8]
        .iter()
        .map(|r| {
            let a = bounce(*r, 0.3);
            (measure(&a, Measure::Decay), old_decay(&a))
        })
        .collect();
    println!("bounce restitution 0.50..0.80 (new, old): {by_r:?}");
    assert!(by_r.windows(2).all(|w| w[1].0 > w[0].0), "{by_r:?}");
    assert!(
        by_r.windows(2).any(|w| w[1].1 <= w[0].1),
        "the old measure was monotone here; the test no longer shows the fix"
    );
    let by_f: Vec<f32> = [0.15f32, 0.2, 0.25, 0.3, 0.35, 0.4]
        .iter()
        .map(|f| measure(&bounce(0.6, *f), Measure::Decay))
        .collect();
    println!("bounce first gap 0.15..0.40: {by_f:?}");
    assert!(by_f.windows(2).all(|w| w[1] > w[0]), "{by_f:?}");
}

#[test]
fn decay_and_swing_resolve_short_sounds() {
    // A 0.1 s click sweeping its decay time: decay never falls and ends
    // higher; the 10 ms frames of before read most of these as 0 or 0.01.
    let v: Vec<f32> = [0.004f32, 0.008, 0.012, 0.016, 0.02, 0.03]
        .iter()
        .map(|t| measure(&click(*t, 0.3), Measure::Decay))
        .collect();
    let o: Vec<f32> = [0.004f32, 0.008, 0.012, 0.016, 0.02, 0.03]
        .iter()
        .map(|t| old_decay(&click(*t, 0.3)))
        .collect();
    println!("decay of a 0.3 s click, decay time 4..30 ms: new {v:?} old {o:?}");
    assert!(
        v.windows(2).all(|w| w[1] >= w[0]) && v[5] > v[1] * 2.0,
        "{v:?}"
    );
    // Swing of a 0.1 s swell: 4 distinct growing values where the old
    // measure had under 3 windows and read 0.
    let s: Vec<f32> = [0.0f32, 6.0, 12.0, 18.0, 24.0]
        .iter()
        .map(|d| measure(&swell(*d, 0.1), Measure::Swing))
        .collect();
    let o: Vec<f32> = [0.0f32, 6.0, 12.0, 18.0, 24.0]
        .iter()
        .map(|d| old_swing(&swell(*d, 0.1)))
        .collect();
    println!("swing of a 0.1 s swell of 0..24 dB: new {s:?} old {o:?}");
    assert!(s.windows(2).all(|w| w[1] > w[0] + 2.0), "{s:?}");
    assert!(o.iter().all(|v| *v == 0.0));
}
