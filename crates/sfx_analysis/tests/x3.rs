//! X3: `grains` per-grain size and pitch, and the self-scheduling `cloud`.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod common;

use serde_json::{Value, json};
use sfx_graph::{Buf, RenderOpts, SoundDoc};

/// FNV-1a over the bits of every channel's samples.
fn fnv(buf: &Buf) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: f32| {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    match buf {
        Buf::Audio(x) => x.iter().for_each(|v| eat(*v)),
        Buf::Stereo(x) => x.iter().for_each(|p| {
            eat(p[0]);
            eat(p[1]);
        }),
        other => panic!("not audio: {}", other.kind()),
    }
    h
}

fn render_node(nodes: &[Value], name: &str, secs: f32, seed: u64) -> std::sync::Arc<Buf> {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes.to_vec();
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": name}));
    d.duration = Some(secs);
    d.seed = seed;
    let r = d
        .render(&RenderOpts {
            cached: false,
            ..RenderOpts::default()
        })
        .unwrap_or_else(|e| panic!("{e}"));
    r.eval.buf(name).unwrap()
}

/// Existing-behaviour documents: the card says `grains` must stay
/// bit-identical, so these hashes were recorded from the code before X3
/// touched it.
fn legacy_docs() -> Vec<(&'static str, Vec<Value>, u64)> {
    let tone = json!({"type": "osc", "name": "tone", "freq": 440, "amp": 0.5});
    let noise = json!({"type": "noise", "name": "nz"});
    vec![
        (
            "poisson noise, defaults",
            vec![
                noise.clone(),
                json!({"type": "poisson", "name": "ev", "rate": 40, "values": {"min": 0.3, "max": 1}}),
                json!({"type": "grains", "name": "g", "source": "nz", "events": "ev", "size": 0.02}),
            ],
            0x7c42577fa478ec9f,
        ),
        (
            "pattern, position + pitch signals, tri",
            vec![
                tone.clone(),
                json!({"type": "pattern", "name": "ev", "every": 0.05, "count": 18}),
                json!({"type": "ramp", "name": "pos", "from": 0, "to": 0.8, "time": 1.0}),
                json!({"type": "ramp", "name": "pit", "from": 0.5, "to": 2, "time": 1.0}),
                json!({"type": "grains", "name": "g", "source": "tone", "events": "ev", "size": 0.04,
                    "position": "pos", "pitch": "pit", "window": "tri"}),
            ],
            0xe15fdb0f14b53635,
        ),
        (
            "burst, stereo, jitter, rect, seed",
            vec![
                tone.clone(),
                json!({"type": "burst", "name": "ev", "count": 30, "spread": 0.7, "curve": "exp"}),
                json!({"type": "grains", "name": "g", "source": "tone", "events": "ev", "size": 0.015,
                    "pan_spread": 0.7, "jitter": 0.2, "window": "rect", "seed": 9}),
            ],
            0xc546197563e0b6c8,
        ),
        (
            "poisson, fixed position and pitch",
            vec![
                noise,
                json!({"type": "poisson", "name": "ev", "rate": 60}),
                json!({"type": "grains", "name": "g", "source": "nz", "events": "ev", "size": 0.03,
                    "position": 0.3, "pitch": 0.5, "pan_spread": 0.3}),
            ],
            0x60e87bf68e4355b0,
        ),
    ]
}

#[test]
fn existing_grains_are_bit_identical_to_before_x3() {
    let mut bad = Vec::new();
    for (what, nodes, want) in legacy_docs() {
        let h = fnv(&render_node(&nodes, "g", 1.0, 7));
        println!("legacy grains `{what}`: fnv {h:#018x}");
        if h != want {
            bad.push(what);
        }
    }
    assert!(bad.is_empty(), "changed: {bad:?}");
}

// ---------------------------------------------------------------------
// New behaviour.

use common::*;
use sfx_graph::node::parse_body;

fn buf_left(b: &Buf) -> Vec<f32> {
    match b {
        Buf::Audio(x) => x.clone(),
        Buf::Stereo(x) => x.iter().map(|p| p[0]).collect(),
        other => panic!("not audio: {}", other.kind()),
    }
}

/// A source that is 1.0 everywhere, so a `rect` grain is a run of ones.
fn ones() -> Value {
    json!({"type": "ramp", "name": "one", "from": 1, "to": 1, "time": 10})
}

/// A 1 kHz sine source.
fn kilo() -> Value {
    json!({"type": "osc", "name": "tone", "freq": 1000, "amp": 0.5})
}

/// (start, length) of each stretch where `x` is above `lo`.
fn runs(x: &[f32], lo: f32) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, v) in x.iter().enumerate() {
        match (start, *v > lo) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                out.push((s, i - s));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, x.len() - s));
    }
    out
}

/// Frequency of a sine-like stretch by counting sign changes.
fn freq_in(x: &[f32], start_s: f32, len_s: f32) -> f32 {
    let a = (start_s * SR as f32) as usize;
    let b = a + (len_s * SR as f32) as usize;
    let seg = &x[a..b];
    let crossings = seg.windows(2).filter(|w| w[0] * w[1] < 0.0).count();
    crossings as f32 / 2.0 / len_s
}

fn render_error(nodes: Vec<Value>, last: &str) -> String {
    let mut d = SoundDoc::named("t");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": last}));
    d.duration = Some(1.0);
    d.render(&RenderOpts::default()).unwrap_err()
}

fn grains(extra: Value) -> Vec<Value> {
    let mut g =
        json!({"type": "grains", "name": "g", "source": "one", "events": "ev", "window": "rect"});
    g.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![
        ones(),
        json!({"type": "pattern", "name": "ev", "every": 0.04, "count": 24, "start": 0.01}),
        g,
    ]
}

#[test]
fn grains_size_range_draws_a_length_per_grain() {
    let lens = |extra: Value, seed: u64| -> Vec<usize> {
        let b = render_node(&grains(extra), "g", 1.0, seed);
        runs(&buf_left(&b), 0.5).iter().map(|r| r.1).collect()
    };
    let lin = lens(json!({"size": {"min": 0.01, "max": 0.03}}), 1);
    let (lo, hi) = (*lin.iter().min().unwrap(), *lin.iter().max().unwrap());
    let mean = lin.iter().sum::<usize>() as f32 / lin.len() as f32;
    println!(
        "size 0.01..0.03 linear: {} grains, lengths {lo}..{hi} samples (limits 480..1440), mean {mean:.0} (uniform: 960)",
        lin.len()
    );
    assert_eq!(lin.len(), 24);
    assert!(lo >= 480 && hi <= 1440, "{lo}..{hi}");
    assert!(hi - lo > 300, "lengths vary: {lo}..{hi}");
    assert!((mean - 960.0).abs() < 120.0, "{mean}");
    // exp is geometric: the median sits near sqrt(min * max) = 0.0173 s
    // (830 samples) and the mean is under the linear one.
    let mut exp = lens(
        json!({"size": {"min": 0.01, "max": 0.03, "curve": "exp"}}),
        1,
    );
    exp.sort_unstable();
    let med = exp[exp.len() / 2];
    println!("same, exp: median {med} samples (geometric centre 831)");
    assert!(exp[0] >= 480 && *exp.last().unwrap() <= 1440);
    assert!((med as f32 - 831.0).abs() < 250.0, "{med}");
    // The same seed repeats, another seed changes the lengths.
    assert_eq!(lin, lens(json!({"size": {"min": 0.01, "max": 0.03}}), 1));
    assert_ne!(lin, lens(json!({"size": {"min": 0.01, "max": 0.03}}), 2));
    // A degenerate range is a fixed size.
    let fixed = lens(json!({"size": {"min": 0.02, "max": 0.02}}), 1);
    assert!(fixed.iter().all(|l| *l == 960), "{fixed:?}");
    // A plain number is still the same fixed size.
    let num = lens(json!({"size": 0.02}), 1);
    assert_eq!(fixed, num);
}

#[test]
fn grains_size_can_be_a_signal() {
    let mut nodes = grains(json!({"size": "grow"}));
    nodes.insert(
        0,
        json!({"type": "ramp", "name": "grow", "from": 0.005, "to": 0.03, "time": 1.0, "curve": "linear"}),
    );
    let b = render_node(&nodes, "g", 1.0, 1);
    let r = runs(&buf_left(&b), 0.5);
    let mut worst = 0i64;
    for (k, (start, len)) in r.iter().enumerate() {
        let want = (0.005 + 0.025 * (*start as f32 / SR as f32)) * SR as f32;
        worst = worst.max((*len as f32 - want).abs().round() as i64);
        assert!(*len >= 240, "grain {k}");
    }
    println!(
        "size from a 0.005 -> 0.03 s ramp: {} grains, first {} samples, last {}, worst error {worst} samples",
        r.len(),
        r[0].1,
        r.last().unwrap().1
    );
    assert_eq!(r.len(), 24);
    assert!(r.windows(2).all(|w| w[1].1 >= w[0].1), "lengths grow");
    assert!(worst <= 3, "{worst}");
}

#[test]
fn grains_pitch_range_draws_a_ratio_per_grain() {
    let nodes = |extra: Value| {
        let mut g =
            json!({"type": "grains", "name": "g", "source": "tone", "events": "ev", "size": 0.1});
        g.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        vec![
            kilo(),
            json!({"type": "pattern", "name": "ev", "every": 0.2, "count": 5, "start": 0.01}),
            g,
        ]
    };
    let freqs = |extra: Value| -> Vec<f32> {
        let x = buf_left(&render_node(&nodes(extra), "g", 1.0, 3));
        (0..5)
            .map(|k| freq_in(&x, 0.01 + 0.2 * k as f32 + 0.02, 0.06))
            .collect()
    };
    let f = freqs(json!({"pitch": {"min": 0.5, "max": 2.0, "curve": "exp"}}));
    println!("pitch 0.5..2 exp, five grains of a 1 kHz sine: {f:.0?} Hz (limits 500..2000)");
    assert!(f.iter().all(|h| (480.0..=2060.0).contains(h)), "{f:?}");
    let (lo, hi) = (
        f.iter().cloned().fold(f32::MAX, f32::min),
        f.iter().cloned().fold(0.0, f32::max),
    );
    assert!(hi / lo > 1.4, "the five grains differ: {f:?}");
    // A one-point range is a fixed ratio; a signal still works.
    let f = freqs(json!({"pitch": {"min": 1.5, "max": 1.5}}));
    println!("pitch 1.5..1.5: {f:.0?} Hz");
    assert!(f.iter().all(|h| (1450.0..=1550.0).contains(h)), "{f:?}");
    let f = freqs(json!({"pitch": 0.5}));
    assert!(f.iter().all(|h| (480.0..=520.0).contains(h)), "{f:?}");
}

/// A cloud named `c` over the stand-in sources, with extra fields.
fn cloud(source: Value, extra: Value) -> Vec<Value> {
    let mut c = json!({"type": "cloud", "name": "c", "window": "rect"});
    c.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![source, c]
}

/// Grains in a rect cloud over `ones`: the sum of the output over the
/// grain length (grains do not run off the end when none start in the last
/// grain length).
fn grain_count(x: &[f32], glen: usize) -> f32 {
    x.iter().sum::<f32>() / glen as f32
}

#[test]
fn cloud_density_sets_the_grain_rate() {
    let secs = 4.0;
    let glen = 24; // 0.5 ms
    let count = |mode: &str, density: Value| {
        let nodes = cloud(
            ones(),
            json!({"source": "one", "density": density, "size": 0.0005, "mode": mode}),
        );
        let x = buf_left(&render_node(&nodes, "c", secs, 1));
        assert_eq!(x.len(), (secs * SR as f32) as usize);
        grain_count(&x, glen)
    };
    let regular = count("regular", json!(200));
    let random = count("random", json!(200));
    println!(
        "density 200/s over 4 s: regular {regular:.1} grains, random {random:.1} (want 800; Poisson sd 28)"
    );
    assert!((regular - 800.0).abs() <= 2.0, "{regular}");
    assert!((random - 800.0).abs() < 112.0, "{random}");
    assert!(
        count("random", json!(0)) == 0.0,
        "density 0 makes no grains"
    );
    // A density that ramps 0 -> 1000 over 2 s: the integral is 1000 grains
    // and a quarter of them fall in the first half.
    let ramp = |mode: &str| {
        let mut nodes = cloud(
            ones(),
            json!({"source": "one", "density": "dens", "size": 0.0005, "mode": mode}),
        );
        nodes.insert(
            0,
            json!({"type": "ramp", "name": "dens", "from": 0, "to": 1000, "time": 2.0, "curve": "linear"}),
        );
        let x = buf_left(&render_node(&nodes, "c", 2.0, 5));
        let half = x.len() / 2;
        (grain_count(&x, glen), grain_count(&x[..half], glen))
    };
    let (total, first) = ramp("regular");
    println!(
        "density ramp 0 -> 1000 over 2 s, regular: {total:.1} grains, first half {:.1}% (want 1000, 25%)",
        100.0 * first / total
    );
    assert!((total - 1000.0).abs() <= 2.0);
    assert!((first / total - 0.25).abs() < 0.01);
    let (total, first) = ramp("random");
    println!(
        "same, random: {total:.1} grains, first half {:.1}% (Poisson sd 32 grains)",
        100.0 * first / total
    );
    assert!((total - 1000.0).abs() < 128.0);
    assert!((first / total - 0.25).abs() < 0.05);
}

#[test]
fn cloud_position_and_pitch_spread() {
    // A 0 -> 1 ramp as source: a rect grain starting at position p reads
    // about p, so the first sample of each grain is the position signal.
    let mut nodes = cloud(
        json!({"type": "ramp", "name": "up", "from": 0, "to": 1, "time": 1.0, "curve": "linear"}),
        json!({"source": "up", "density": 50, "size": 0.002, "mode": "regular", "position": "sweep"}),
    );
    nodes.insert(
        1,
        json!({"type": "ramp", "name": "sweep", "from": 0, "to": 1, "time": 1.0, "curve": "linear"}),
    );
    let x = buf_left(&render_node(&nodes, "c", 1.0, 1));
    let r = runs(&x, 1e-6);
    let worst = r
        .iter()
        .map(|(s, _)| (x[*s] - *s as f32 / SR as f32).abs())
        .fold(0.0, f32::max);
    println!(
        "position from a 0 -> 1 ramp over a 0 -> 1 ramp source: {} grains, worst |first sample - position| {worst:.4}",
        r.len()
    );
    assert!(r.len() >= 48, "{}", r.len());
    assert!(worst < 0.01, "{worst}");
    // pitch_spread: +-12 semitones is a ratio in 0.5..2 per grain.
    let freqs = |spread: f32| -> Vec<f32> {
        let nodes = cloud(
            kilo(),
            json!({"source": "tone", "density": 5, "size": 0.1, "mode": "regular", "window": "hann", "pitch_spread": spread}),
        );
        let x = buf_left(&render_node(&nodes, "c", 1.0, 2));
        (1..5)
            .map(|k| freq_in(&x, 0.2 * k as f32 + 0.02, 0.06))
            .collect()
    };
    let none = freqs(0.0);
    let wide = freqs(12.0);
    println!("pitch_spread 0: {none:.0?} Hz; 12 semitones: {wide:.0?} Hz (limits 500..2000)");
    assert!(
        none.iter().all(|h| (970.0..=1030.0).contains(h)),
        "{none:?}"
    );
    assert!(
        wide.iter().all(|h| (470.0..=2080.0).contains(h)),
        "{wide:?}"
    );
    let (lo, hi) = (
        wide.iter().cloned().fold(f32::MAX, f32::min),
        wide.iter().cloned().fold(0.0, f32::max),
    );
    assert!(hi / lo > 1.3, "{wide:?}");
    // Base pitch moves the centre.
    let nodes = cloud(
        kilo(),
        json!({"source": "tone", "density": 5, "size": 0.1, "mode": "regular", "window": "hann", "pitch": 2}),
    );
    let x = buf_left(&render_node(&nodes, "c", 1.0, 2));
    let f = freq_in(&x, 0.42, 0.06);
    println!("pitch 2: {f:.0} Hz");
    assert!((1940.0..=2060.0).contains(&f));
}

#[test]
fn cloud_pan_spread_makes_stereo() {
    let nodes = cloud(
        kilo(),
        json!({"source": "tone", "density": 80, "size": {"min": 0.01, "max": 0.03}, "pan_spread": 0.9, "window": "hann"}),
    );
    let b = render_node(&nodes, "c", 1.0, 1);
    let Buf::Stereo(s) = &*b else {
        panic!("stereo expected, got {}", b.kind())
    };
    let (l, r): (Vec<f32>, Vec<f32>) = s.iter().map(|p| (p[0], p[1])).unzip();
    println!(
        "pan_spread 0.9: L {:.2} dB, R {:.2} dB, {} samples",
        rms_db(&l),
        rms_db(&r),
        s.len()
    );
    assert_eq!(s.len(), SR as usize);
    assert!(s.iter().any(|p| (p[0] - p[1]).abs() > 1e-3));
    assert!(rms_db(&l) > -40.0 && rms_db(&r) > -40.0);
}

#[test]
fn cloud_and_ranged_grains_are_bit_identical_and_seeded() {
    let c = |seed: u64| {
        cloud(
            kilo(),
            json!({"source": "tone", "density": 120, "size": {"min": 0.005, "max": 0.05, "curve": "smooth"},
                "pitch_spread": 5, "pan_spread": 0.7, "jitter": 0.1, "position": 0.2, "seed": seed}),
        )
    };
    assert_bit_identical(c(1), 0.5);
    let mut reg = c(1);
    reg[1]["mode"] = json!("regular");
    assert_bit_identical(reg, 0.5);
    let mut g = grains(
        json!({"size": {"min": 0.005, "max": 0.05}, "pitch": {"min": 0.5, "max": 2},
        "pan_spread": 0.5, "jitter": 0.2}),
    );
    g[0] = kilo();
    g[2]["source"] = json!("tone");
    assert_bit_identical(g, 0.5);
    let a = fnv(&render_node(&c(1), "c", 0.5, 1));
    let b = fnv(&render_node(&c(2), "c", 0.5, 1));
    let again = fnv(&render_node(&c(1), "c", 0.5, 1));
    println!("cloud fnv: seed 1 {a:#018x}, seed 2 {b:#018x}, seed 1 again {again:#018x}");
    assert_eq!(a, again);
    assert_ne!(a, b);
}

#[test]
fn cloud_and_ranged_grains_are_finite_at_the_extremes() {
    let check = |nodes: Vec<Value>, what: &str| {
        let b = render_node(&nodes, "c", 0.3, 1);
        let b = match &*b {
            Buf::Audio(x) => x.clone(),
            Buf::Stereo(x) => x.iter().flat_map(|p| [p[0], p[1]]).collect(),
            _ => panic!(),
        };
        assert_finite(&b, what);
    };
    let tone = || kilo();
    for (what, extra) in [
        (
            "tiny sizes",
            json!({"size": {"min": 1.0e-9, "max": 1.0e-7}}),
        ),
        ("long sizes", json!({"size": {"min": 1.0e3, "max": 1.0e6}})),
        ("huge pitch", json!({"size": 0.01, "pitch": 1.0e9})),
        ("zero pitch", json!({"size": 0.01, "pitch": 0})),
        ("max spread", json!({"size": 0.01, "pitch_spread": 120})),
        (
            "jitter 5",
            json!({"size": 0.01, "jitter": 5, "position": 7}),
        ),
        ("full density", json!({"size": 0.01, "density": 48000})),
    ] {
        let mut full = json!({"source": "tone", "density": 400, "pan_spread": 1});
        full.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        check(cloud(tone(), full), what);
    }
    // Signals far out of range.
    let mut nodes = cloud(
        tone(),
        json!({"source": "tone", "density": "wild", "size": "wild", "position": "wild", "pitch": "wild"}),
    );
    nodes.insert(
        1,
        json!({"type": "ramp", "name": "wild", "from": -1.0e9, "to": 1.0e9, "time": 0.3, "curve": "linear"}),
    );
    check(nodes, "signals of +-1e9");
    // Grains with a ranged pitch from 0 and sizes under two samples.
    let mut g =
        grains(json!({"size": {"min": 1.0e-9, "max": 1.0e-6}, "pitch": {"min": 0, "max": 1.0e6}}));
    g[0] = kilo();
    g[2]["source"] = json!("tone");
    g[2]["name"] = json!("c");
    check(g, "grains with extreme ranges");
    println!(
        "extremes: cloud sizes 1e-9..1e6, pitch 0 / 1e9, spread 120, jitter 5, full density, signals +-1e9, grains with ranges: all finite"
    );
}

#[test]
fn cloud_and_grains_errors_say_the_field_value_and_fix() {
    let e = render_error(grains(json!({"size": {"min": 0.05, "max": 0.01}})), "g");
    println!("{e}");
    assert!(
        e.contains("size: min 0.05 is above max 0.01: swap them"),
        "{e}"
    );
    let e = render_error(grains(json!({"size": 0})), "g");
    println!("{e}");
    assert!(e.contains("size: 0: give a number > 0"), "{e}");
    let e = render_error(grains(json!({"size": {"min": 0, "max": 0.01}})), "g");
    println!("{e}");
    assert!(
        e.contains("size: min 0: give a range that stays above 0"),
        "{e}"
    );
    let e = render_error(
        grains(json!({"size": 0.01, "pitch": {"min": -1, "max": 2}})),
        "g",
    );
    println!("{e}");
    assert!(
        e.contains("pitch: min -1: give a range that stays at least 0"),
        "{e}"
    );
    let e = render_error(
        grains(json!({"size": 0.01, "pitch": {"min": 0, "max": 2, "curve": "exp"}})),
        "g",
    );
    println!("{e}");
    assert!(
        e.contains("pitch: curve \"exp\" needs min and max above 0"),
        "{e}"
    );
    let e = render_error(grains(json!({"size": {"min": 1, "max": 2, "mx": 3}})), "g");
    println!("{e}");
    assert!(e.contains("mx") && e.contains("min"), "{e}");
    let c = |extra: Value| {
        let mut full = json!({"source": "one", "density": 100, "size": 0.01});
        full.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        render_error(cloud(ones(), full), "c")
    };
    let e = c(json!({"density": 1.0e9}));
    println!("{e}");
    assert!(
        e.contains("density: 1000000000: give grains per second from 0 to 48000"),
        "{e}"
    );
    let e = c(json!({"density": -3}));
    assert!(
        e.contains("density: -3: give grains per second from 0"),
        "{e}"
    );
    let e = c(json!({"pitch_spread": 500}));
    println!("{e}");
    assert!(
        e.contains("pitch_spread: 500: give semitones from 0 to 120"),
        "{e}"
    );
    let e = c(json!({"pan_spread": 2}));
    assert!(e.contains("pan_spread: 2: give 0..1"), "{e}");
    let e = c(json!({"size": -1}));
    assert!(e.contains("size: -1: give a number > 0"), "{e}");
    let e = c(json!({"mode": "clumped"}));
    println!("{e}");
    assert!(e.contains("clumped") && e.contains("regular"), "{e}");
    let e = c(json!({"dens": 3}));
    assert!(e.contains("dens") && e.contains("density"), "{e}");
    // More than 200000 grains in a sound is refused, not cut.
    let mut d = SoundDoc::named("t");
    d.nodes = cloud(
        ones(),
        json!({"source": "one", "density": 40000, "size": 0.0002}),
    );
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": "c"}));
    d.duration = Some(6.0);
    let e = d.render(&RenderOpts::default()).unwrap_err();
    println!("{e}");
    assert!(e.contains("density: more than 200000 grains"), "{e}");
    // An event stream is not a source.
    let e = render_error(
        vec![
            json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 3}),
            json!({"type": "cloud", "name": "c", "source": "ev", "density": 10, "size": 0.01}),
        ],
        "c",
    );
    println!("{e}");
    assert!(e.contains("source") && e.contains("event stream"), "{e}");
}

#[test]
fn cloud_and_grains_are_registered() {
    let ex = sfx_graph::example("cloud").expect("registered");
    let mut body = ex.clone();
    body.as_object_mut().unwrap().remove("type");
    body.as_object_mut().unwrap().remove("name");
    let node = parse_body("cloud", &body, "cloud1").unwrap();
    assert!(!node.causal() && node.random());
    let text = sfx_graph::describe(Some("cloud")).unwrap();
    println!("{text}");
    assert!(text.contains("grain cloud") && text.contains("density"));
    assert!(sfx_graph::describe(None).unwrap().contains("- cloud:"));
    let text = sfx_graph::describe(Some("grains")).unwrap();
    println!("{text}");
    assert!(text.contains("range") && text.contains("min"));
    // Both example documents render with the stand-in names.
    let r = render_node(
        &[
            kilo(),
            json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 5}),
            json!({"type": "grains", "name": "g", "source": "tone", "events": "ev", "size": {"min": 0.01, "max": 0.02}, "pitch": {"min": 0.8, "max": 1.2, "curve": "exp"}}),
        ],
        "g",
        0.6,
        1,
    );
    assert!(buf_left(&r).iter().any(|v| v.abs() > 0.01));
}

/// The recipe in docs/nodes/grains.md, as written there.
fn recipe() -> Vec<Value> {
    vec![
        json!({"type": "osc", "name": "tone", "freq": 330, "amp": 0.5}),
        json!({"type": "ramp", "name": "swell", "from": 4, "to": 160, "time": 2.0, "curve": "exp"}),
        json!({"type": "cloud", "name": "swarm", "source": "tone", "density": "swell",
            "size": {"min": 0.02, "max": 0.08}, "position": 0.1, "pitch_spread": 2, "pan_spread": 0.6}),
    ]
}

#[test]
fn cloud_recipe_renders() {
    let b = render_node(&recipe(), "swarm", 2.0, 1);
    let x = buf_left(&b);
    let (early, late) = (&x[..SR as usize / 2], &x[x.len() - SR as usize / 2..]);
    println!(
        "recipe: {} samples, rms first quarter {:.2} dB, last quarter {:.2} dB",
        x.len(),
        rms_db(early),
        rms_db(late)
    );
    assert_eq!(x.len(), 2 * SR as usize);
    assert!(rms_db(late) > rms_db(early) + 6.0);
    assert_finite(&x, "swarm");
}
