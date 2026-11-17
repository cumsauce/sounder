//! `modal`, `contact` and `friction`: size and material move the ring the way
//! physics says, the contact pulse follows Hertz, friction follows speed.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] fns

use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_graph::{Buf, RenderOpts, SoundDoc, wav};

fn struck(material: &str, size: f64, pos: f64) -> SoundDoc {
    let mut d = SoundDoc::named("struck");
    d.nodes = vec![
        json!({"type": "impulse", "name": "hit", "at": [0.0]}),
        json!({"type": "modal", "name": "plate", "excite": "hit", "material": material,
               "size": size, "strike_position": pos}),
        json!({"type": "output", "name": "out", "input": "plate", "lufs": -20}),
    ];
    d.duration = Some(1.5);
    d
}

fn audio_of(d: &SoundDoc, node: &str) -> Vec<f32> {
    let r = d.render(&RenderOpts::default()).unwrap();
    match &*r.eval.buf(node).unwrap() {
        Buf::Audio(x) => x.clone(),
        other => panic!("{node}: {}", other.kind()),
    }
}

#[test]
fn a_bigger_plate_rings_lower() {
    let mut last = f32::MAX;
    for size in [0.25, 0.5, 1.0] {
        let r = struck("wood", size, 0.3)
            .render(&RenderOpts::default())
            .unwrap();
        let d = describe(&r.audio);
        let c = d.centroid_hz.start;
        println!("size {size}: centroid {c:.0} Hz, pitch {:?}", d.pitch_hz);
        assert!(c < last, "size {size}: centroid {c} not below {last}");
        last = c;
    }
}

#[test]
fn steel_rings_longer_than_wood() {
    let decay = |m: &str| {
        let r = struck(m, 0.5, 0.3).render(&RenderOpts::default()).unwrap();
        describe(&r.audio).decay20_s.unwrap_or(99.0)
    };
    let (steel, wood) = (decay("metal"), decay("wood"));
    println!("decay to -20 dB: metal {steel:.3} s, wood {wood:.3} s");
    assert!(steel > 2.0 * wood, "metal {steel} vs wood {wood}");
}

#[test]
fn material_props_are_the_named_material() {
    let named = audio_of(&struck("wood", 0.5, 0.3), "plate");
    let props =
        json!({"young": 10e9, "density": 500, "poisson": 0.3, "loss": 0.025, "thickness": 0.025});
    let by_props = audio_of(&struck_json(props, 0.5, 0.3), "plate");
    assert_eq!(named, by_props);
}

fn struck_json(material: Value, size: f64, pos: f64) -> SoundDoc {
    let mut d = struck("wood", size, pos);
    d.nodes[1]["material"] = material;
    d
}

#[test]
fn the_strike_position_changes_the_spectrum() {
    let c = |p: f64| {
        let r = struck("metal", 0.5, p)
            .render(&RenderOpts::default())
            .unwrap();
        describe(&r.audio).centroid_hz.start
    };
    let (a, b) = (c(0.12), c(0.5));
    println!("centroid: edge {a:.0} Hz, centre {b:.0} Hz");
    assert!((a - b).abs() / a.max(b) > 0.05, "{a} vs {b}");
}

#[test]
fn unknown_materials_say_what_exists() {
    let e = struck("cheese", 0.5, 0.3)
        .render(&RenderOpts::default())
        .err()
        .unwrap();
    assert!(e.contains("cheese") && e.contains("wood"), "{e}");
}

fn contact_doc(speed: f64, mass: f64, material: &str, hardness: f64) -> SoundDoc {
    let mut d = SoundDoc::named("c");
    d.nodes = vec![
        json!({"type": "contact", "name": "c", "speed": speed, "mass": mass,
               "material": material, "hardness": hardness, "at": 0.01}),
        json!({"type": "output", "name": "out", "input": "c"}),
    ];
    d.duration = Some(0.5);
    d
}

/// Seconds the pulse is nonzero.
fn width(d: &SoundDoc) -> f32 {
    let x = audio_of(d, "c");
    x.iter().filter(|v| **v != 0.0).count() as f32 / 48_000.0
}

#[test]
fn contact_pulse_grows_with_mass_and_shrinks_with_speed() {
    let w = |speed, mass| width(&contact_doc(speed, mass, "wood", 0.7));
    let (light, mid, heavy) = (w(1.0, 0.05), w(1.0, 0.5), w(1.0, 5.0));
    println!("mass 0.05/0.5/5 kg: {light:.5} {mid:.5} {heavy:.5} s");
    assert!(light < mid && mid < heavy);
    let (slow, fast) = (w(0.5, 0.5), w(5.0, 0.5));
    println!("speed 0.5/5 m/s: {slow:.5} {fast:.5} s");
    assert!(fast < slow);
    let soft = width(&contact_doc(1.0, 0.5, "wood", 0.1));
    let hard = width(&contact_doc(1.0, 0.5, "wood", 0.95));
    println!("hardness 0.1/0.95: {soft:.5} {hard:.5} s");
    assert!(hard < soft);
}

fn friction_doc(speed: Value, pressure: f64, roughness: f64) -> SoundDoc {
    let mut d = SoundDoc::named("f");
    d.nodes = vec![
        json!({"type": "friction", "name": "f", "speed": speed, "pressure": pressure,
               "roughness": roughness, "seed": 3}),
        json!({"type": "output", "name": "out", "input": "f", "lufs": -20}),
    ];
    d.duration = Some(0.6);
    d
}

#[test]
fn friction_pitch_follows_speed() {
    let pitch = |speed: f64| {
        let r = friction_doc(json!(speed), 0.5, 0.0)
            .render(&RenderOpts::default())
            .unwrap();
        describe(&r.audio).pitch_hz.unwrap()
    };
    let (a, b) = (pitch(0.5), pitch(1.0));
    println!("slip rate at speed 0.5 / 1.0: {a:.1} / {b:.1} Hz");
    assert!((a / 100.0 - 1.0).abs() < 0.05, "{a}");
    assert!((b / 200.0 - 1.0).abs() < 0.05, "{b}");
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn friction_pressure_scales_level() {
    let lo = rms(&audio_of(&friction_doc(json!(0.8), 0.25, 0.3), "f"));
    let hi = rms(&audio_of(&friction_doc(json!(0.8), 0.5, 0.3), "f"));
    println!("rms at pressure 0.25 / 0.5: {lo:.4} / {hi:.4}");
    assert!((hi / lo - 2.0).abs() < 0.02, "{}", hi / lo);
}

#[test]
fn friction_is_finite_silent_at_rest_and_deterministic() {
    let still = audio_of(&friction_doc(json!(0.0), 1.0, 1.0), "f");
    assert!(still.iter().all(|v| v.is_finite() && *v == 0.0));
    for bad in [-0.7, 1.0e9] {
        let x = audio_of(&friction_doc(json!(bad), 1.0, 1.0), "f");
        assert!(x.iter().all(|v| v.is_finite()), "speed {bad}");
    }
    // A creak that starts at rest and speeds up.
    let mut d = friction_doc(json!("v"), 0.8, 0.6);
    d.nodes.insert(
        0,
        json!({"type": "ramp", "name": "v", "from": 0, "to": 1, "time": 0.6}),
    );
    let bytes = |threads| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let r = d
                .render(&RenderOpts {
                    cached: false,
                    ..RenderOpts::default()
                })
                .unwrap();
            wav::to_bytes16(&r.audio)
        })
    };
    assert_eq!(bytes(1), bytes(8));
    let x = audio_of(&d, "f");
    assert!(x.iter().all(|v| v.is_finite()) && rms(&x) > 0.01);
}

#[test]
fn a_modal_chain_is_deterministic() {
    let mut d = struck("glass", 0.2, 0.3);
    d.nodes[0] =
        json!({"type": "contact", "name": "hit", "speed": 2, "mass": 0.05, "material": "glass"});
    let a = audio_of(&d, "plate");
    d.nodes[1]["seed"] = json!(9);
    let b = audio_of(&d, "plate");
    assert_ne!(a, b, "the node seed changes the mistuning");
    assert!(a.iter().all(|v| v.is_finite()));
}
