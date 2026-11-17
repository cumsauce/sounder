//! A Karplus-Strong string: a noise burst into a delay + filter feedback loop
//! reads the pitch the delay length sets.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::json;
use sfx_analysis::{Expect, describe};
use sfx_graph::{RenderOpts, SoundDoc};

fn string(hz: f64, gain: f64) -> SoundDoc {
    let mut d = SoundDoc::named("string");
    d.nodes = vec![
        json!({"type": "noise", "name": "n", "seed": 5}),
        json!({"type": "decay", "name": "e", "time": 0.004}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
        json!({"type": "mix", "name": "m", "inputs": ["burst", "g"]}),
        json!({"type": "delay", "name": "d", "input": "m", "time": 1.0 / hz}),
        json!({"type": "onepole", "name": "lp", "input": "d", "mode": "lp", "freq": 9000}),
        json!({"type": "gain", "name": "g", "input": "lp", "gain": gain}),
        // The burst's own DC rings in the loop: block it, as the check advises.
        json!({"type": "onepole", "name": "hp", "input": "g", "mode": "hp", "freq": 30}),
        json!({"type": "output", "name": "out", "input": "hp", "lufs": -20}),
    ];
    d.duration = Some(0.6);
    d
}

#[test]
fn the_string_rings_at_the_pitch_the_delay_sets() {
    for hz in [220.0, 330.0, 440.0, 660.0] {
        let d = string(hz, 0.997);
        let r = d.render(&RenderOpts::default()).unwrap();
        let p = describe(&r.audio)
            .pitch_hz
            .unwrap_or_else(|| panic!("{hz} Hz: no pitch found"));
        // The one-pole adds a fraction of a sample to the loop: a little flat.
        let cents = 1200.0 * (f64::from(p) / hz).log2();
        assert!(
            cents.abs() < 25.0,
            "{hz} Hz string read {p} Hz ({cents:+.1} cents)"
        );
        assert!(
            cents < 5.0,
            "the filter's delay can only lower the pitch: {cents}"
        );
        assert!(r.peak_db > -3.0 && r.audio.peak() < 1.0);
    }
}

#[test]
fn a_loop_gain_over_one_is_caught_by_verify_and_a_decaying_one_passes() {
    use sfx_analysis::{CheckCx, run};
    let opts = RenderOpts::default();
    let ok = string(440.0, 0.99);
    let r = ok.render(&opts).unwrap();
    let checks = run(
        &Expect::defaults(),
        &CheckCx {
            doc: &ok,
            rendered: &r,
            opts: &opts,
        },
    )
    .unwrap();
    assert!(checks.iter().all(|c| c.pass), "{checks:?}");
    // Gain 1.05 grows until the output limiter holds it: a sound that never
    // decays (the render warns) and whose loop is unstable.
    let mut bad = string(440.0, 1.05);
    bad.duration = None;
    let r = bad.render(&opts).unwrap();
    assert!(
        r.warnings.iter().any(|w| w.contains("never decays")),
        "{:?}",
        r.warnings
    );
}
