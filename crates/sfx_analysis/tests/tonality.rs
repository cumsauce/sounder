//! `modal` `noise`: real wood and metal are less tonal than
//! an ideal plate struck by an impulse (the Phase 0 and Phase 6 listening
//! tests: a crate read as a marimba, a lid as a tuned gong). Off by default;
//! on, the voiced share, the ringing partials and the tonal peaks fall and
//! the occupied band's flatness rises. Numbers in `docs/nodes/exaggerate.md`.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use sfx_analysis::checks::ringing_partials;
use sfx_analysis::describe;
use sfx_graph::{RenderOpts, SoundDoc};

fn render(modal: Value, secs: f32) -> sfx_graph::Audio {
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        json!({"type": "noise", "name": "n", "color": "pink", "seed": 3}),
        json!({"type": "decay", "name": "e", "time": 0.012, "at": 0.01}),
        json!({"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"}),
        modal,
        json!({"type": "output", "name": "out", "input": "m", "peak_db": -3}),
    ];
    d.duration = Some(secs);
    d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap()
    .audio
}

fn with(base: &Value, extra: Value) -> Value {
    let mut m = base.clone();
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    m
}

struct Tone {
    voiced: f32,
    ringing: usize,
    flat: f32,
    /// The strongest partial over the median of the modes' levels, dB.
    peaky: f32,
}

fn tone(a: &sfx_graph::Audio) -> Tone {
    let d = describe(a);
    let mut lv: Vec<f32> = d.modes.iter().map(|m| m.level_db).collect();
    lv.sort_by(|x, y| x.total_cmp(y));
    let peaky = lv.last().copied().unwrap_or(0.0) - lv.get(lv.len() / 2).copied().unwrap_or(0.0);
    Tone {
        voiced: d.voiced,
        ringing: ringing_partials(&d).len(),
        flat: d.flatness_occupied.unwrap_or(0.0),
        peaky,
    }
}

fn report(label: &str, t: &Tone) {
    println!(
        "{label}: voiced {:.2}, ringing {}, occupied flatness {:.3}, strongest partial {:.1} dB over the median",
        t.voiced, t.ringing, t.flat, t.peaky
    );
}

#[test]
fn defaults_and_zero_are_unchanged() {
    let base = json!({"type": "modal", "name": "m", "excite": "burst", "material": "wood", "f0": 180, "loss": 1.2});
    let a = render(base.clone(), 1.0);
    let b = render(with(&base, json!({"noise": 0})), 1.0);
    assert!(
        a.channels[0]
            .iter()
            .zip(&b.channels[0])
            .all(|(x, y)| x.to_bits() == y.to_bits())
    );
}

#[test]
fn wood_and_metal_lose_their_tuned_ring() {
    let plank = json!({"type": "modal", "name": "m", "excite": "burst", "material": "wood", "f0": 180, "loss": 1.2});
    let lid = json!({"type": "modal", "name": "m", "excite": "burst", "material": "metal", "shape": "membrane", "f0": 300, "loss": 0.5, "modes": 128});
    for (name, base, secs) in [("plank", plank, 1.0f32), ("lid", lid, 2.5)] {
        let rows = [
            ("plain", json!({})),
            ("noise 0.6", json!({"noise": 0.6})),
            (
                "noise 0.6 loss x2.5",
                json!({"noise": 0.6, "loss": base["loss"].as_f64().unwrap() * 2.5}),
            ),
            ("noise 0.5 tilt 4", json!({"noise": 0.5, "tilt": 4})),
            (
                "noise 0.6 tilt 4 loss x2.5",
                json!({"noise": 0.6, "tilt": 4, "loss": base["loss"].as_f64().unwrap() * 2.5}),
            ),
        ];
        let mut got = Vec::new();
        for (label, extra) in rows {
            let t = tone(&render(with(&base, extra), secs));
            report(&format!("{name} {label}"), &t);
            got.push(t);
        }
        let (plain, noisy, tilted) = (&got[0], &got[1], &got[3]);
        // `noise` takes the pitch out and spreads the energy.
        assert!(
            noisy.voiced < 0.35 && noisy.voiced < plain.voiced - 0.5,
            "{name}: voiced {} -> {}",
            plain.voiced,
            noisy.voiced
        );
        assert!(
            noisy.flat > plain.flat,
            "{name}: flatness {} -> {}",
            plain.flat,
            noisy.flat
        );
        // `tilt` with it takes the dominant partial down.
        assert!(
            tilted.peaky < plain.peaky - 4.0,
            "{name}: {} -> {}",
            plain.peaky,
            tilted.peaky
        );
    }
}

/// `strike_glide` and `noise` together: the noise is kept (it was once
/// dropped when the glide rebuilt the output from the clean modes).
#[test]
fn glide_keeps_the_noise() {
    let lid = json!({"type": "modal", "name": "m", "excite": "burst", "material": "metal", "shape": "membrane", "f0": 300, "loss": 0.5, "modes": 128, "strike_glide": 35, "tilt": 6});
    let plain = tone(&render(lid.clone(), 2.5));
    let noisy = tone(&render(
        with(&lid, json!({"noise": 1.0, "noise_width": 1.0})),
        2.5,
    ));
    report("lid glide 35 tilt 6", &plain);
    report("lid glide 35 tilt 6 noise 1 width 1", &noisy);
    assert!(
        noisy.flat > plain.flat + 0.1,
        "{} -> {}",
        plain.flat,
        noisy.flat
    );
}

fn render_node(node: Value, out: &str, secs: f32) -> sfx_graph::Audio {
    let mut d = SoundDoc::named("t");
    d.nodes = vec![
        node,
        json!({"type": "output", "name": "out", "input": out, "peak_db": -3, "fade_in": 0.0005}),
    ];
    d.duration = Some(secs);
    d.render(&RenderOpts {
        cached: false,
        ..RenderOpts::default()
    })
    .unwrap()
    .audio
}

/// `click` and wood `footstep` take the same `noise`: their default rings
/// read pitched (click voiced 0.33-0.78, heard as "a chime"; a wood step
/// 0.84 at 158 Hz). Off or 0: bit-identical.
#[test]
fn click_and_wood_footstep_take_noise() {
    let same = |a: &sfx_graph::Audio, b: &sfx_graph::Audio| {
        a.channels[0]
            .iter()
            .zip(&b.channels[0])
            .all(|(x, y)| x.to_bits() == y.to_bits())
    };
    for k in ["latch", "switch", "button", "lock", "clasp"] {
        let base = json!({"type": "click", "name": "c", "kind": k});
        let plain = render_node(base.clone(), "c", 0.4);
        assert!(
            same(
                &plain,
                &render_node(with(&base, json!({"noise": 0})), "c", 0.4)
            ),
            "{k}"
        );
        let p = tone(&plain);
        let n = tone(&render_node(with(&base, json!({"noise": 0.6})), "c", 0.4));
        report(&format!("click {k} plain"), &p);
        report(&format!("click {k} noise 0.6"), &n);
        assert!(n.voiced <= p.voiced, "{k}: {} -> {}", p.voiced, n.voiced);
    }
    let base = json!({"type": "footstep", "name": "f", "surface": "wood"});
    let plain = render_node(base.clone(), "f", 0.8);
    assert!(same(
        &plain,
        &render_node(with(&base, json!({"noise": 0})), "f", 0.8)
    ));
    let p = tone(&plain);
    let n = tone(&render_node(with(&base, json!({"noise": 0.6})), "f", 0.8));
    report("footstep wood plain", &p);
    report("footstep wood noise 0.6", &n);
    assert!(n.voiced < p.voiced - 0.3, "{} -> {}", p.voiced, n.voiced);
}

/// Wood `footstep` `scuff` (the roll between heel and toe) and `creak` (a
/// board under the weight): each adds energy where it should; 0 is
/// bit-identical.
#[test]
fn wood_footstep_scuff_and_creak() {
    let base =
        json!({"type": "footstep", "name": "f", "surface": "wood", "weight": 0.4, "noise": 0.35});
    let plain = render_node(base.clone(), "f", 0.8);
    for f in ["scuff", "creak"] {
        let z = render_node(with(&base, json!({f: 0})), "f", 0.8);
        assert!(
            plain.channels[0]
                .iter()
                .zip(&z.channels[0])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{f}"
        );
    }
    // Energy (dB) in a time window, against the plain step.
    let gain = |a: &sfx_graph::Audio, t0: f32, t1: f32| {
        let (i, j) = ((t0 * 48_000.0) as usize, (t1 * 48_000.0) as usize);
        let energy = |x: &[f32]| {
            sfx_analysis::stft::db(x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32)
        };
        energy(&a.channels[0][i..j]) - energy(&plain.channels[0][i..j])
    };
    let sc = render_node(with(&base, json!({"scuff": 0.8})), "f", 0.8);
    let cr = render_node(with(&base, json!({"creak": 0.8})), "f", 0.8);
    let (s_gain, c_gain) = (gain(&sc, 0.02, 0.09), gain(&cr, 0.15, 0.4));
    println!(
        "scuff 0.8: {s_gain:+.1} dB between heel and toe; creak 0.8: {c_gain:+.1} dB at 0.15-0.4 s"
    );
    assert!(s_gain > 1.0, "{s_gain}");
    assert!(c_gain > 3.0, "{c_gain}");
}
