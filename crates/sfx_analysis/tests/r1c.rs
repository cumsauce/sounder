//! R1-C: the `density`, `attack` and `spacing` macro measures, `onsets`
//! with `spacing_ms`, and the minimum effect of a macro sweep.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::measure::measure;
use sfx_analysis::{CheckCx, CheckResult, Expect, describe, run};
use sfx_graph::{Audio, Claim, DocEdit, Macro, MapCurve, MapEntry, Measure, RenderOpts, SoundDoc};

const SR: u32 = 48_000;

/// Deterministic white noise, -1..1.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// A noise burst `ms` long at each time in `times` (seconds), with the
/// matching amplitude, in `secs` of silence.
fn clicks(times: &[(f32, f32)], ms: f32, secs: f32) -> Audio {
    let mut x = vec![0.0f32; (secs * SR as f32) as usize];
    let mut rng = Lcg(7);
    let len = (ms * 0.001 * SR as f32) as usize;
    for (t, amp) in times {
        let at = (t * SR as f32) as usize;
        for i in 0..len {
            if let Some(v) = x.get_mut(at + i) {
                // A fast decay inside the burst.
                *v += amp * rng.next() * (1.0 - i as f32 / len as f32);
            }
        }
    }
    Audio::mono(SR, x)
}

fn train(rate: f32, secs: f32) -> Audio {
    let n = (rate * secs) as usize;
    let t: Vec<(f32, f32)> = (0..n)
        .map(|i| (0.01 + i as f32 / rate, 0.5))
        .filter(|(t, _)| *t < secs - 0.01)
        .collect();
    clicks(&t, 1.5, secs)
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

/// An impulse train at `rate` per second through a short resonator.
fn click_doc(rate: f32, secs: f32) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    let n = (rate * secs) as usize - 1;
    let at: Vec<f32> = (0..n).map(|i| 0.01 + i as f32 / rate).collect();
    d.nodes = vec![
        json!({"type": "impulse", "name": "i", "at": at}),
        json!({"type": "resonators", "name": "r", "input": "i", "modes": [[3000, 0.0008, 0.8]]}),
        json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
    ];
    d.duration = Some(secs);
    d
}

// --- density ---------------------------------------------------------------

#[test]
fn density_rises_with_the_rate_of_a_click_train_and_matches_the_descriptors() {
    let rates = [5.0f32, 10.0, 20.0, 40.0, 80.0, 160.0];
    let v: Vec<f32> = rates
        .iter()
        .map(|r| measure(&train(*r, 1.5), Measure::Density))
        .collect();
    println!("density of trains at {rates:?}/s: {v:?}");
    assert!(v.windows(2).all(|w| w[1] > w[0] * 1.5), "{v:?}");
    for (r, m) in rates.iter().zip(&v) {
        // The span runs from the first click to the end of the last, so the
        // count over the span reads a little over the rate.
        assert!((m / r - 1.0).abs() < 0.2, "rate {r}: measured {m}");
    }
    // The same number the descriptor table prints.
    for r in [10.0f32, 80.0] {
        let a = train(r, 1.5);
        let d = describe(&a);
        let m = measure(&a, Measure::Density);
        println!("rate {r}: measure {m}, descriptors {}", d.density.per_s);
        assert!((m - d.density.per_s).abs() < 0.05 * d.density.per_s, "{m}");
    }
    assert_eq!(
        measure(&Audio::mono(SR, vec![0.0; 4800]), Measure::Density),
        0.0
    );
}

// --- attack ----------------------------------------------------------------

#[test]
fn attack_rises_as_a_swell_lengthens_its_rise() {
    let rises = [0.005f32, 0.01, 0.02, 0.04, 0.08];
    let v: Vec<f32> = rises
        .iter()
        .map(|rise| {
            let n = (0.4 * SR as f32) as usize;
            let x: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f32 / SR as f32;
                    let env = if t < *rise {
                        t / rise
                    } else {
                        (-(t - rise) * 12.0).exp()
                    };
                    env * (std::f32::consts::TAU * 3000.0 * t).sin() * 0.5
                })
                .collect();
            measure(&Audio::mono(SR, x), Measure::Attack)
        })
        .collect();
    println!("attack (ms) of rises {rises:?} s: {v:?}");
    assert!(v.windows(2).all(|w| w[1] > w[0] * 1.5), "{v:?}");
    for (rise, a) in rises.iter().zip(&v) {
        let want = rise * 1000.0 * 0.9;
        assert!((a - want).abs() < 1.5, "rise {rise}: {a} ms, want {want}");
    }
    assert_eq!(
        measure(&Audio::mono(SR, vec![0.0; 4800]), Measure::Attack),
        0.0
    );
}

// --- spacing ---------------------------------------------------------------

/// Hits at `first`, `first*r`, ... apart, each `r` times the last.
fn bounce(first: f32, r: f32, hits: usize) -> Audio {
    let (mut t, mut gap, mut amp) = (0.02f32, first, 0.8f32);
    let mut ev = Vec::new();
    for _ in 0..hits {
        ev.push((t, amp));
        t += gap;
        gap *= r;
        amp *= r;
    }
    clicks(&ev, 3.0, t + 0.2)
}

#[test]
fn spacing_rises_as_the_first_gap_of_a_bounce_grows() {
    let firsts = [0.1f32, 0.15, 0.2, 0.3, 0.4];
    let v: Vec<f32> = firsts
        .iter()
        .map(|f| measure(&bounce(*f, 0.7, 5), Measure::Spacing))
        .collect();
    println!("spacing (s) of bounces with first gap {firsts:?}: {v:?}");
    assert!(v.windows(2).all(|w| w[1] > w[0] * 1.2), "{v:?}");
    // Gaps f, .7f, .49f, .34f: the median (upper middle of four) is .7 f.
    for (f, s) in firsts.iter().zip(&v) {
        assert!((s - f * 0.7).abs() < 0.012, "first {f}: {s}");
    }
    // A lone hit has no spacing.
    assert_eq!(measure(&bounce(0.2, 0.7, 1), Measure::Spacing), 0.0);
}

// --- onsets with spacing_ms ------------------------------------------------

#[test]
fn onsets_spacing_ms_counts_a_dense_click_train() {
    let d = click_doc(200.0, 1.0);
    let n = 199.0f32;
    let fine = check(
        &d,
        json!({"onsets": {"min": 0, "max": 100000, "spacing_ms": 5}}),
    );
    let default = check(&d, json!({"onsets": {"min": 0, "max": 100000}}));
    println!("fine: {}\ndefault: {}", fine[0].detail, default[0].detail);
    let counted: f32 = fine[0]
        .detail
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!((counted / n - 1.0).abs() < 0.1, "{counted} of {n}");
    // The default merges them: far fewer than there are clicks.
    let merged: f32 = default[0]
        .detail
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(merged < 0.5 * n, "{merged}");
    // min / max apply to the fine count.
    let ok = check(
        &d,
        json!({"onsets": {"min": 180, "max": 220, "spacing_ms": 5}}),
    );
    assert!(ok[0].pass, "{:?}", ok[0]);
    let bad = check(&d, json!({"onsets": {"max": 50, "spacing_ms": 5}}));
    assert!(!bad[0].pass, "{:?}", bad[0]);
    // The default fail text points at spacing_ms.
    let hint = check(&d, json!({"onsets": {"min": 150}}));
    assert!(
        !hint[0].pass && hint[0].detail.contains("spacing_ms"),
        "{:?}",
        hint[0]
    );
}

#[test]
fn onsets_without_spacing_ms_is_unchanged() {
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        json!({"type": "impulse", "name": "i", "at": [0.0, 0.3, 0.6]}),
        json!({"type": "resonators", "name": "r", "input": "i", "modes": [[440, 0.2, 0.5]]}),
        json!({"type": "output", "name": "out", "input": "r", "lufs": -20}),
    ];
    let a = check(&d, json!({"onsets": {"min": 3, "max": 3}}));
    let b = check(
        &d,
        json!({"onsets": {"min": 3, "max": 3, "spacing_ms": 40}}),
    );
    assert!(a[0].pass && b[0].pass, "{:?} {:?}", a[0], b[0]);
    assert_eq!(a[0].detail, "3 onsets (3..3)");
    assert_eq!(b[0].detail, a[0].detail);
    // 0.3 s apart survives a 500 ms merge distance as 1 onset.
    let c = check(
        &d,
        json!({"onsets": {"min": 1, "max": 1, "spacing_ms": 700}}),
    );
    assert!(c[0].pass, "{:?}", c[0]);
    // Unknown fields and a bad spacing are errors that say what to do.
    let e: Result<Expect, _> = serde_json::from_value(json!({"onsets": {"gap": 5}}));
    assert!(e.is_err());
    let opts = RenderOpts::default();
    let r = d.render(&opts).unwrap();
    let e: Expect = serde_json::from_value(json!({"onsets": {"spacing_ms": 0}})).unwrap();
    let err = run(
        &e,
        &CheckCx {
            doc: &d,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap_err();
    assert!(err.contains("spacing_ms"), "{err}");
}

// --- the sweep's minimum effect and the new claims --------------------------

fn knob(name: &str, claim: Claim, node: &str, field: &str, from: f64, to: f64) -> Macro {
    Macro {
        name: name.into(),
        value: 0.5,
        default: None,
        doc: String::new(),
        claim: Some(claim),
        game: false,
        map: vec![MapEntry {
            node: node.into(),
            field: field.into(),
            param: None,
            from,
            to,
            curve: MapCurve::Linear,
            col: None,
            index: None,
            key: None,
        }],
    }
}

fn with_macro(mut d: SoundDoc, m: Macro) -> SoundDoc {
    d.apply(
        vec![],
        DocEdit {
            macros: Some(vec![m]),
            ..DocEdit::default()
        },
        &RenderOpts::default(),
    )
    .unwrap();
    d
}

fn macro_line(d: &SoundDoc, name: &str) -> CheckResult {
    let res = check(d, json!({"macros": true}));
    res.into_iter()
        .find(|c| c.name == format!("macro {name}"))
        .unwrap_or_else(|| panic!("no line for {name}"))
}

fn tone_doc(freq: f64) -> SoundDoc {
    let mut d = SoundDoc::named("t");
    d.duration = Some(0.3);
    d.nodes = vec![
        json!({"type": "osc", "name": "o", "freq": freq, "amp": 0.5}),
        json!({"type": "output", "name": "out", "input": "o", "lufs": -20}),
    ];
    d
}

#[test]
fn a_knob_that_barely_moves_its_claim_fails_and_a_real_one_passes() {
    // 2000 -> 2020 Hz: a rising centroid, 1 % of the value.
    let d = with_macro(
        tone_doc(2000.0),
        knob(
            "bright",
            Claim(Measure::Centroid, 1),
            "o",
            "freq",
            2000.0,
            2020.0,
        ),
    );
    let l = macro_line(&d, "bright");
    println!("{}", l.detail);
    assert!(
        !l.pass && l.detail.contains("barely moves") && l.detail.contains("widen"),
        "{l:?}"
    );
    // 2000 -> 3000 Hz is a real move.
    let d = with_macro(
        tone_doc(2000.0),
        knob(
            "bright",
            Claim(Measure::Centroid, 1),
            "o",
            "freq",
            2000.0,
            3000.0,
        ),
    );
    let l = macro_line(&d, "bright");
    println!("{}", l.detail);
    assert!(l.pass, "{l:?}");
}

#[test]
fn the_new_claims_are_swept_by_verify() {
    // A click train whose rate the knob sets: a density claim holds.
    let mut d = SoundDoc::named("t");
    d.duration = Some(1.0);
    d.nodes = vec![
        json!({"type": "pattern", "name": "ev", "every": 0.1, "count": 9, "start": 0.01}),
        json!({"type": "voices", "name": "v", "events": "ev", "length": 0.01, "max": 400,
               "patch": [
                   {"type": "noise", "name": "n"},
                   {"type": "decay", "name": "e", "time": 0.002},
                   {"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}]}),
        json!({"type": "output", "name": "out", "input": "v", "lufs": -20}),
    ];
    let d = with_macro(
        d,
        knob("rate", Claim(Measure::Density, 1), "ev", "every", 0.1, 0.02),
    );
    // `every` falls as the knob rises, and the count stays 9: the rate rises
    // only if the span shrinks, which it does.
    let l = macro_line(&d, "rate");
    println!("{}", l.detail);
    assert!(l.pass, "{l:?}");
    // Naming it the wrong way round fails and says so.
    let mut wrong = d.clone();
    wrong.macros[0].claim = Some(Claim(Measure::Density, -1));
    let l = macro_line(&wrong, "rate");
    println!("{}", l.detail);
    assert!(!l.pass && l.detail.contains("density"), "{l:?}");
    // The claim names parse from JSON.
    for (name, m) in [
        ("density", Measure::Density),
        ("attack", Measure::Attack),
        ("spacing", Measure::Spacing),
    ] {
        let c: Claim = serde_json::from_value(json!([name, 1])).unwrap();
        assert_eq!(c, Claim(m, 1));
        assert_eq!(m.name(), name);
    }
}
