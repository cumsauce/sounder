//! `buckle` (sheet metal snapping through under load) and `tear` (ductile
//! sheet metal tearing along a line), in `crates/sfx_graph/src/nodes/crackle.rs`.
//! Reference: `docs/nodes/sheet_metal.md`.

use serde_json::{Value, json};
use sfx_analysis::describe;
use sfx_graph::{Buf, Ctx, Event, Params, evaluate};

use crate::common::*;

/// The events on `port` (e.g. "b.events") after evaluating `nodes`.
fn events(nodes: &[Value], port: &str, secs: f32) -> Vec<Event> {
    let ctx = Ctx {
        n: (secs * SR as f32) as usize,
        ..Ctx::default()
    };
    let ev = evaluate(nodes, &Params::new(), &ctx, false).unwrap_or_else(|e| panic!("{e}"));
    match &*ev.buf(port).unwrap_or_else(|e| panic!("{e}")) {
        Buf::Events(v) => v.clone(),
        other => panic!("`{port}` is {}, not events", other.kind()),
    }
}

fn buckle(fields: Value) -> Vec<Value> {
    vec![with_fields(json!({"type": "buckle", "name": "b"}), fields)]
}

fn tear(fields: Value) -> Vec<Value> {
    vec![with_fields(json!({"type": "tear", "name": "t"}), fields)]
}

fn mean(x: impl Iterator<Item = f32>) -> f32 {
    let v: Vec<f32> = x.collect();
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

#[test]
fn buckle_and_tear_are_finite_over_their_ranges() {
    let lfo = json!({"type": "lfo", "name": "wob", "freq": 3, "range": [0, 1]});
    let buckles = [
        json!({}),
        json!({"size": 0.01, "thickness": 0.05, "load": 1}),
        json!({"size": 20, "thickness": 0.00001, "load": 1, "exponent": 0.5}),
        json!({"size": 20, "thickness": 0.05, "exponent": 6, "ring": 0}),
        json!({"load": 0}),
        json!({"rate": 2000, "material": "copper", "ring": 1}),
        json!({"load": "wob", "material": "aluminum"}),
        json!({"rate": "wob", "material": "titanium", "size": 0.3, "thickness": 0.0005}),
    ];
    for f in buckles {
        let mut nodes = vec![lfo.clone()];
        nodes.extend(buckle(f.clone()));
        for port in ["b", "b.force"] {
            let x = node_audio(&nodes, port, 1.0);
            assert_finite(&x, &format!("buckle {f} {port}"));
        }
    }
    let speed = json!({"type": "lfo", "name": "wob", "freq": 2, "range": [0, 300]});
    let tears = [
        json!({}),
        json!({"speed": 0}),
        json!({"speed": 300, "thickness": 0.0001, "grain": 0.00001}),
        json!({"speed": 300, "thickness": 0.02, "grain": 0.1, "jitter": 0}),
        json!({"speed": "wob", "pitch_m": 0.005, "pop_db": 30, "hiss": 1, "rumble": 1}),
        json!({"speed": 1, "pitch_m": 10, "material": "steel", "hiss": 0, "rumble": 0}),
    ];
    for f in tears {
        let mut nodes = vec![speed.clone()];
        nodes.extend(tear(f.clone()));
        for port in ["t", "t.force"] {
            let x = node_audio(&nodes, port, 1.0);
            assert_finite(&x, &format!("tear {f} {port}"));
        }
    }
    // Out-of-range numbers are errors that name the field.
    for (node, field) in [
        (json!({"type": "buckle", "name": "x", "load": 1.5}), "load"),
        (
            json!({"type": "buckle", "name": "x", "thickness": 0.1}),
            "thickness",
        ),
        (json!({"type": "buckle", "name": "x", "rate": 5000}), "rate"),
        (json!({"type": "tear", "name": "x", "speed": -1}), "speed"),
        (
            json!({"type": "tear", "name": "x", "pitch_m": 0.001}),
            "pitch_m",
        ),
    ] {
        let e = render_error_raw(
            vec![node, json!({"type": "output", "name": "o", "input": "x"})],
            0.2,
        );
        assert!(e.contains(field), "{field}: {e}");
    }
}

#[test]
fn buckle_rate_follows_the_load_and_the_rate_signal() {
    // A 0.5 m, 1 mm steel sheet: the law gives 3 x 0.5 / 0.15 = 10 per
    // second at load 1.
    let sheet = json!({"size": 0.5, "thickness": 0.001, "seed": 3});
    let count = |load: f32| {
        events(
            &buckle(with_fields(sheet.clone(), json!({"load": load}))),
            "b.events",
            20.0,
        )
        .len()
    };
    let (quarter, full) = (count(0.25), count(1.0));
    println!("buckle 0.5 m 1 mm: {quarter} events at load 0.25, {full} at load 1 over 20 s");
    assert!((150..=250).contains(&full), "load 1: {full} (law: 200)");
    let ratio = full as f32 / quarter.max(1) as f32;
    assert!((3.0..5.3).contains(&ratio), "full / quarter {ratio:.2}");
    // An explicit rate wins over the law.
    let n = events(&buckle(json!({"rate": 50, "seed": 2})), "b.events", 4.0).len();
    println!("buckle rate 50: {n} events in 4 s");
    assert!((160..=240).contains(&n), "rate 50 over 4 s: {n}");
    // A load ramp: more events in the second half than the first.
    let nodes = vec![
        json!({"type": "ramp", "name": "push", "from": 0.05, "to": 1, "time": 4, "curve": "linear"}),
        with_fields(
            json!({"type": "buckle", "name": "b"}),
            json!({"load": "push", "size": 2.0, "thickness": 0.001, "seed": 1}),
        ),
    ];
    let ev = events(&nodes, "b.events", 4.0);
    let early = ev.iter().filter(|e| e.t < 2.0).count();
    let late = ev.len() - early;
    println!("buckle load ramp: {early} events in the first 2 s, {late} in the last 2");
    // Expected (law: 40/s at load 1): 23 early, 61 late.
    assert!(
        late as f32 > 1.8 * early as f32,
        "early {early}, late {late}"
    );
}

/// (events per second, mean value, centroid Hz of `out`) of a sheet.
fn sheet_stats(fields: Value) -> (f32, f32, f32) {
    let nodes = buckle(with_fields(fields, json!({"load": 0.6, "seed": 5})));
    let ev = events(&nodes, "b.events", 4.0);
    let x = node_audio(&nodes, "b", 4.0);
    let d = describe(&mono_audio(&x));
    (
        ev.len() as f32 / 4.0,
        mean(ev.iter().map(|e| e.value)),
        d.centroid_all_hz,
    )
}

#[test]
fn thicker_and_larger_sheets_snap_fewer_stronger_lower() {
    let foil = sheet_stats(json!({"size": 0.2, "thickness": 0.00002, "material": "aluminium"}));
    let thin = sheet_stats(json!({"size": 1.0, "thickness": 0.001}));
    let thick = sheet_stats(json!({"size": 1.0, "thickness": 0.003}));
    let big = sheet_stats(json!({"size": 4.0, "thickness": 0.003}));
    for (name, s) in [
        ("foil 0.2 m 0.02 mm", foil),
        ("steel 1 m 1 mm", thin),
        ("steel 1 m 3 mm", thick),
        ("steel 4 m 3 mm", big),
    ] {
        println!(
            "buckle {name}: {:.1} events/s, mean value {:.3}, centroid {:.0} Hz",
            s.0, s.1, s.2
        );
    }
    // Thicker: fewer, stronger, lower.
    for (a, b, what) in [(foil, thin, "foil -> 1 mm"), (thin, thick, "1 mm -> 3 mm")] {
        assert!(b.0 < a.0, "{what}: rate {:.1} -> {:.1}", a.0, b.0);
        assert!(b.1 > a.1, "{what}: value {:.3} -> {:.3}", a.1, b.1);
        assert!(b.2 < a.2, "{what}: centroid {:.0} -> {:.0}", a.2, b.2);
    }
    // Larger at the same thickness: stronger and lower on average (bigger
    // facets fit); the rate grows with the room for facets, as documented.
    assert!(
        big.1 > thick.1,
        "4 m vs 1 m value {:.3} vs {:.3}",
        big.1,
        thick.1
    );
    assert!(
        big.2 < thick.2 * 1.05,
        "4 m vs 1 m centroid {:.0} vs {:.0}",
        big.2,
        thick.2
    );
    // Each event carries its facet span, inside the sheet.
    let ev = events(
        &buckle(json!({"size": 1.0, "thickness": 0.001, "load": 1})),
        "b.events",
        2.0,
    );
    assert!(
        ev.iter()
            .all(|e| e.extra.size.is_some_and(|s| (0.15..=0.5).contains(&s)))
    );
}

#[test]
fn buckling_foil_sits_in_the_band_crumple_gives_foil() {
    let foil = buckle(
        json!({"size": 0.2, "thickness": 0.00002, "material": "aluminium", "load": 0.5, "seed": 4}),
    );
    let crumple = [json!({"type": "crumple", "name": "c", "hardness": 1, "seed": 4})];
    let b = describe(&mono_audio(&node_audio(&foil, "b", 2.0)));
    let c = describe(&mono_audio(&node_audio(crumple, "c", 2.0)));
    println!(
        "foil buckle: centroid {:.0} Hz, shares {:?}, {:.0} fine events/s; crumple hardness 1: centroid {:.0} Hz, shares {:?}",
        b.centroid_all_hz,
        b.band_shares,
        mean(b.density_tenths.iter().copied()),
        c.centroid_all_hz,
        c.band_shares
    );
    assert!(
        (3000.0..9000.0).contains(&b.centroid_all_hz),
        "{}",
        b.centroid_all_hz
    );
    let octaves = (b.centroid_all_hz / c.centroid_all_hz).log2().abs();
    assert!(octaves < 1.0, "{octaves:.2} octaves from crumple");
    assert!(
        b.band_shares[0] < 5.0,
        "under 200 Hz {:.1} %",
        b.band_shares[0]
    );
    assert!(b.voiced < 0.1, "voiced {:.2}", b.voiced);
}

#[test]
fn tear_sits_where_recorded_tears_do() {
    // Recorded aluminium tears: centroid 3.0-3.9 kHz, 1-5 % of the energy
    // under 200 Hz, unvoiced.
    for speed in [5.0, 10.0, 20.0] {
        let x = node_audio(tear(json!({"speed": speed, "seed": 1})), "t", 2.0);
        let d = describe(&mono_audio(&x));
        println!(
            "tear {speed} m/s: centroid {:.0} Hz, shares {:?}, voiced {:.2}, flatness {:.3}, occupied {:?}, bands {:?}",
            d.centroid_all_hz,
            d.band_shares,
            d.voiced,
            d.flatness_all,
            d.occupied_hz,
            d.bands_total
        );
        assert!(
            (2500.0..4200.0).contains(&d.centroid_all_hz),
            "{speed}: centroid {}",
            d.centroid_all_hz
        );
        assert!(
            (0.5..6.0).contains(&d.band_shares[0]),
            "{speed}: under 200 Hz {:.1} %",
            d.band_shares[0]
        );
        assert!(d.voiced < 0.1, "{speed}: voiced {:.2}", d.voiced);
    }
}

#[test]
fn tear_has_a_pitch_only_when_pitch_m_asks() {
    let plain = describe(&mono_audio(&node_audio(tear(json!({"seed": 2})), "t", 2.0)));
    let regular = describe(&mono_audio(&node_audio(
        tear(json!({"seed": 2, "jitter": 0, "grain": 0.025})),
        "t",
        2.0,
    )));
    let rivets = describe(&mono_audio(&node_audio(
        tear(json!({"seed": 2, "pitch_m": 0.025, "pop_db": 12})),
        "t",
        2.0,
    )));
    println!(
        "tear voiced: plain {:.2}, jitter 0 grain 25 mm {:.2} ({:?} Hz), pitch_m 25 mm {:.2} ({:?} Hz)",
        plain.voiced, regular.voiced, regular.pitch_hz, rivets.voiced, rivets.pitch_hz
    );
    assert!(plain.voiced < 0.1, "plain voiced {:.2}", plain.voiced);
    // 10 m/s over 25 mm rivets: 400 pops a second, heard as 400 Hz.
    assert!(rivets.voiced > 0.5, "rivets voiced {:.2}", rivets.voiced);
    let p = rivets.pitch_hz.unwrap();
    assert!((p / 400.0).log2().abs() < 0.1, "rivet pitch {p}");
}

#[test]
fn tear_follows_its_speed() {
    let at = |speed: f32| {
        let nodes = tear(json!({"speed": speed, "seed": 6}));
        let n = events(&nodes, "t.events", 4.0).len();
        let x = node_audio(&nodes, "t", 4.0);
        (n, rms_db(&x), describe(&mono_audio(&x)).centroid_all_hz)
    };
    let (slow, fast) = (at(5.0), at(20.0));
    println!(
        "tear 5 m/s: {} snaps, {:.1} dB, {:.0} Hz; 20 m/s: {} snaps, {:.1} dB, {:.0} Hz",
        slow.0, slow.1, slow.2, fast.0, fast.1, fast.2
    );
    // Snaps: one per 200 thicknesses (0.2 m) of tear: 25/s at 5 m/s.
    assert!(
        (80..=120).contains(&slow.0),
        "5 m/s over 4 s: {} snaps (law: 100)",
        slow.0
    );
    let ratio = fast.0 as f32 / slow.0 as f32;
    assert!((3.3..4.8).contains(&ratio), "snap ratio {ratio:.2}");
    assert!(
        fast.1 > slow.1 + 3.0,
        "louder when faster: {:.1} vs {:.1}",
        fast.1,
        slow.1
    );
    assert!(fast.2 > slow.2, "brighter when faster");
    // The hiss swells with a speed ramp and is silent at rest.
    let nodes = vec![
        json!({"type": "ramp", "name": "run", "from": 0, "to": 15, "time": 1.0, "curve": "linear"}),
        json!({"type": "tear", "name": "t", "speed": "run"}),
    ];
    let x = node_audio(&nodes, "t", 1.0);
    let q = x.len() / 4;
    assert!(rms_db(&x[3 * q..]) > rms_db(&x[q..2 * q]) + 3.0);
    let still = node_audio(tear(json!({"speed": 0})), "t", 0.5);
    assert!(peak(&still) == 0.0, "speed 0 peak {}", peak(&still));
}

#[test]
fn buckle_and_tear_are_seeded() {
    for nodes in [buckle(json!({"load": 0.8})), tear(json!({}))] {
        let name = nodes[0]["name"].as_str().unwrap().to_string();
        let with = |seed: u64| {
            let mut n = nodes.clone();
            n[0]["seed"] = json!(seed);
            fnv(&node_audio(&n, &name, 1.0))
        };
        assert_eq!(with(1), with(1), "{name}: same seed, same render");
        assert_ne!(with(1), with(2), "{name}: another seed, another render");
        let mut n = nodes.clone();
        n.push(json!({"type": "output", "name": "o", "input": name}));
        assert_bit_identical(&n, 0.5);
    }
}

#[test]
fn events_ports_drive_voices() {
    let patch = json!([
        {"type": "osc", "name": "tone", "freq": "$pitch", "amp": "$value"},
        {"type": "decay", "name": "amp", "time": 0.08},
        {"type": "math", "name": "hit", "op": "mul", "a": "tone", "b": "amp"}
    ]);
    for (src, port) in [
        (
            json!({"type": "buckle", "name": "b", "load": 1, "size": 0.5, "thickness": 0.001}),
            "b.events",
        ),
        (
            json!({"type": "tear", "name": "t", "pitch_m": 0.5, "speed": 5}),
            "t.events",
        ),
    ] {
        let nodes = vec![
            src,
            json!({"type": "voices", "name": "v", "events": port, "patch": patch, "length": 0.2, "max": 400,
                "map": {"pitch": "100 + 50 / $size"}}),
        ];
        let n = events(&nodes, port, 1.0).len();
        let x = node_audio(&nodes, "v", 1.0);
        println!("{port}: {n} events, voices peak {:.1} dB", peak_db(&x));
        assert!(n > 2, "{port}: {n} events");
        assert_finite(&x, port);
        assert!(peak(&x) > 1e-4, "{port}: silent voices");
    }
    // The force ports excite a body.
    let nodes = vec![
        json!({"type": "buckle", "name": "b", "load": 0.8, "size": 0.5, "thickness": 0.001}),
        json!({"type": "modal", "name": "m", "excite": "b.force", "material": "metal", "size": 0.5}),
    ];
    let x = node_audio(&nodes, "m", 1.0);
    assert_finite(&x, "modal from buckle.force");
    assert!(peak(&x) > 1e-3);
}
