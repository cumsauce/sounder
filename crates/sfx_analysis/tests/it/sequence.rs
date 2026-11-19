//! `trigger_env`, `event_gate`, `seq`, `notes`. Each test measures and
//! prints (these lines go in docs/nodes/sequence.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::pitch;
use sfx_graph::node::parse_body;
use sfx_graph::{Buf, Event, RenderOpts, SoundDoc};

const N: f32 = SR as f32;

fn pat(every: f32, count: u32) -> Value {
    json!({"type": "pattern", "name": "ev", "every": every, "count": count})
}

/// An event node's (or port's) stream.
fn events(nodes: Vec<Value>, name: &str, duration: f32) -> Vec<Event> {
    let mut d = SoundDoc::named("test");
    d.nodes = nodes;
    d.nodes
        .push(json!({"type": "osc", "name": "_quiet", "freq": 1, "amp": 0.0}));
    d.nodes
        .push(json!({"type": "output", "name": "_out", "input": "_quiet"}));
    d.duration = Some(duration);
    let r = d
        .render(&RenderOpts::default())
        .unwrap_or_else(|e| panic!("{e}"));
    let b = r.eval.buf(name).unwrap_or_else(|e| panic!("{e}"));
    match &*b {
        Buf::Events(v) => v.clone(),
        other => panic!("`{name}` is {}", other.kind()),
    }
}

fn render_error(nodes: Vec<Value>, read: &str) -> String {
    crate::common::render_error(nodes, read, 0.1)
}

fn idx(t: f32) -> usize {
    (t * N).round() as usize
}

#[test]
fn trigger_env_peaks_at_each_event_with_its_value() {
    for retrigger in ["restart", "add"] {
        let nodes = vec![
            with_fields(
                pat(0.1, 5),
                json!({"values": {"min": 0.3, "max": 1.7}, "seed": 4}),
            ),
            json!({"type": "trigger_env", "name": "env", "events": "ev", "attack": 0,
                   "decay": 0.04, "retrigger": retrigger}),
        ];
        let ev = events(nodes.clone(), "ev", 0.6);
        let x = node_audio(nodes, "env", 0.6);
        assert_finite(&x, "env");
        let mut worst_pos = 0usize;
        let mut worst_h = 0.0f32;
        for (k, e) in ev.iter().enumerate() {
            let lo = idx(e.t as f32);
            let hi = (lo + idx(0.1)).min(x.len());
            let (pos, h) = x[lo..hi]
                .iter()
                .enumerate()
                .fold(
                    (0, f32::MIN),
                    |m, (i, v)| if *v > m.1 { (i, *v) } else { m },
                );
            worst_pos = worst_pos.max(pos);
            worst_h = worst_h.max((h - e.value).abs());
            assert!(pos <= 1, "event {k}: peak {pos} samples after the event");
        }
        println!(
            "trigger_env {retrigger}: {} events, peak at most {worst_pos} samples after the event, height error at most {worst_h:.2e}",
            ev.len()
        );
        assert!(worst_h < 1e-4, "{worst_h}");
        // Gone by the end of the decay.
        assert_eq!(x[idx(0.1) + idx(0.04) + 2], 0.0);
    }
}

#[test]
fn trigger_env_attack_decay_and_curves() {
    let env = |extra: Value| {
        node_audio(
            vec![
                json!({"type": "once", "name": "ev", "at": 0.1}),
                with_fields(
                    json!({"type": "trigger_env", "name": "env", "events": "ev"}),
                    extra,
                ),
            ],
            "env",
            0.6,
        )
    };
    let x = env(json!({"attack": 0.01, "decay": 0.2, "curve": "linear"}));
    let (peak_i, peak) = x
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |m, (i, v)| if *v > m.1 { (i, *v) } else { m });
    let mid_fall = x[idx(0.1) + idx(0.01) + idx(0.1)];
    let mid_rise = x[idx(0.1) + idx(0.005)];
    println!(
        "linear, attack 0.01 decay 0.2: peak {peak:.4} at sample {peak_i} (expected {}), midway up {mid_rise:.4}, midway down {mid_fall:.4}, before {:.1}",
        idx(0.11),
        x[idx(0.1) - 1]
    );
    assert!((peak - 1.0).abs() < 1e-5 && peak_i.abs_diff(idx(0.11)) <= 1);
    assert!((mid_rise - 0.5).abs() < 0.01 && (mid_fall - 0.5).abs() < 0.001);
    assert_eq!(x[idx(0.1) - 1], 0.0);
    assert_eq!(x[idx(0.31) + 2], 0.0, "exactly 0 after attack + decay");

    let e = env(json!({"attack": 0, "decay": 0.2, "curve": "exp"}));
    let (db_half, db_end) = (
        20.0 * e[idx(0.2)].log10(),
        20.0 * e[idx(0.3) - 100].max(1e-9).log10(),
    );
    println!(
        "exp, decay 0.2: halfway {db_half:.2} dB (expected -30.3), 100 samples before the end {db_end:.1} dB, after the end {:.1}",
        e[idx(0.3) + 2]
    );
    assert!((db_half + 30.3).abs() < 0.5 && e[idx(0.3) + 2] == 0.0);

    let s = env(json!({"attack": 0, "decay": 0.2, "curve": "smooth"}));
    println!("smooth, decay 0.2: halfway {:.4}", s[idx(0.2)]);
    assert!((s[idx(0.2)] - 0.5).abs() < 0.001);

    let v = node_audio(
        vec![
            with_fields(pat(0.1, 2), json!({"values": {"min": 3.0, "max": 3.0}})),
            json!({"type": "trigger_env", "name": "env", "events": "ev", "attack": 0,
                   "scale_by_value": false}),
        ],
        "env",
        0.3,
    );
    println!("scale_by_value false: peak {:.4}", v[0]);
    assert!((v[0] - 1.0).abs() < 1e-6);
}

#[test]
fn trigger_env_restart_continues_from_the_level_and_add_sums() {
    let two = |retrigger: &str, curve: &str, attack: f32| {
        node_audio(
            vec![
                json!({"type": "pattern", "name": "ev", "times": [0.0, 0.01]}),
                json!({"type": "trigger_env", "name": "env", "events": "ev", "attack": attack,
                       "decay": 0.2, "curve": curve, "retrigger": retrigger}),
            ],
            "env",
            0.4,
        )
    };
    let add = two("add", "linear", 0.0);
    let at = idx(0.01);
    println!(
        "add, linear, events 10 ms apart: sample at the second event {:.4} (1 + 0.95 = 1.95)",
        add[at]
    );
    assert!((add[at] - 1.95).abs() < 1e-3);
    // Restart with an attack: no downward step at the second event, the
    // largest sample-to-sample change near it stays at the attack slope.
    let re = two("restart", "linear", 0.02);
    let step = (at - 3..at + 3)
        .map(|i| (re[i + 1] - re[i]).abs())
        .fold(0.0f32, f32::max);
    println!("restart, attack 0.02: largest step near the second event {step:.5}");
    assert!(step < 0.003);
    // Restart without an attack jumps to the new peak: the sum is not taken.
    let hard = two("restart", "linear", 0.0);
    println!(
        "restart, attack 0: level at the second event {:.4}",
        hard[at]
    );
    assert!((hard[at] - 1.0).abs() < 1e-4);
}

#[test]
fn event_gate_is_one_for_length_after_each_event() {
    let nodes = |every: f32, length: f32| {
        vec![
            pat(every, 3),
            json!({"type": "event_gate", "name": "g", "events": "ev", "length": length}),
        ]
    };
    let g = node_audio(nodes(0.1, 0.03), "g", 0.4);
    let ones = g.iter().filter(|v| **v == 1.0).count();
    let zeros = g.iter().filter(|v| **v == 0.0).count();
    let first_fall = g.iter().position(|v| *v == 0.0).unwrap();
    let second_rise = g.iter().skip(first_fall).position(|v| *v == 1.0).unwrap() + first_fall;
    println!(
        "gate every 0.1 s length 0.03 s: {ones} ones (expected {}), high at sample 0, falls at {first_fall} (expected {}), rises again at {second_rise} (expected {})",
        3 * idx(0.03),
        idx(0.03),
        idx(0.1)
    );
    assert_eq!(ones, 3 * idx(0.03));
    assert_eq!(ones + zeros, g.len(), "only 0 and 1");
    assert!(g[0] == 1.0 && first_fall == idx(0.03) && second_rise == idx(0.1));
    // Overlapping gates merge into one high stretch.
    let m = node_audio(nodes(0.1, 0.15), "g", 0.5);
    let high = m.iter().filter(|v| **v == 1.0).count();
    println!(
        "length 0.15 on a 0.1 s grid: {high} high samples (expected {})",
        idx(0.35)
    );
    assert_eq!(high, idx(0.35));
    assert!(m[..idx(0.35)].iter().all(|v| *v == 1.0));
    // A length under one sample still fires for one sample.
    let tiny = node_audio(nodes(0.1, 1.0e-9), "g", 0.4);
    assert_eq!(tiny.iter().filter(|v| **v == 1.0).count(), 3);
}

fn seq_nodes(extra: Value) -> Vec<Value> {
    vec![
        pat(0.1, 7),
        with_fields(
            json!({"type": "seq", "name": "s", "events": "ev", "values": [100, 200, 300]}),
            extra,
        ),
    ]
}

/// The held value in the middle of each 0.1 s step.
fn steps(x: &[f32], count: usize) -> Vec<f32> {
    (0..count).map(|k| x[idx(0.1 * k as f32 + 0.05)]).collect()
}

#[test]
fn seq_cycles_runs_once_and_draws_seeded_randoms() {
    let c = steps(&node_audio(seq_nodes(json!({})), "s", 0.8), 7);
    println!("seq cycle [100, 200, 300] over 7 events: {c:?}");
    assert_eq!(c, [100.0, 200.0, 300.0, 100.0, 200.0, 300.0, 100.0]);
    let o = steps(&node_audio(seq_nodes(json!({"mode": "once"})), "s", 0.8), 7);
    println!("seq once: {o:?}");
    assert_eq!(o, [100.0, 200.0, 300.0, 300.0, 300.0, 300.0, 300.0]);
    let r = |seed: u64| {
        let nodes = seq_nodes(json!({"mode": "random", "seed": seed}));
        steps(&node_audio(nodes, "s", 0.8), 7)
    };
    let (a, b, a2) = (r(1), r(2), r(1));
    println!("seq random seed 1: {a:?}; seed 1 again: {a2:?}; seed 2: {b:?}");
    assert_eq!(a, a2, "same seed, same sequence");
    assert_ne!(a, b, "another seed, another sequence");
    assert!(a.iter().all(|v| [100.0, 200.0, 300.0].contains(v)));
    assert!(a.iter().any(|v| *v != a[0]));
    // Before the first event the output holds values[0]; the first event
    // plays values[0] too, the second values[1].
    let x = node_audio(
        vec![
            json!({"type": "pattern", "name": "ev", "times": [0.3, 0.4]}),
            json!({"type": "seq", "name": "s", "events": "ev", "values": [7, 9]}),
        ],
        "s",
        0.5,
    );
    assert_eq!((x[0], x[idx(0.35)], x[idx(0.45)]), (7.0, 7.0, 9.0));
}

#[test]
fn seq_glides_from_the_current_value() {
    let x = node_audio(seq_nodes(json!({"glide": 0.05})), "s", 0.8);
    let mid = x[idx(0.1) + idx(0.025)];
    let done = x[idx(0.1) + idx(0.06)];
    println!("glide 0.05 from 100 to 200 at 0.1 s: halfway {mid:.2} (150), after {done:.2} (200)");
    assert!((mid - 150.0).abs() < 0.5 && done == 200.0);
    // Interrupted at 0.02 s into a 0.05 s glide: starts from where it was.
    let nodes = vec![
        json!({"type": "pattern", "name": "ev", "times": [0.0, 0.1, 0.12]}),
        json!({"type": "seq", "name": "s", "events": "ev", "values": [0, 100, 0], "glide": 0.05}),
    ];
    let y = node_audio(nodes, "s", 0.4);
    let at = y[idx(0.12)];
    let after = y[idx(0.12) + idx(0.025)];
    println!("interrupted: level at the second event {at:.2} (40), 25 ms later {after:.2} (20)");
    assert!((at - 40.0).abs() < 0.5 && (after - 20.0).abs() < 0.5);
}

fn notes_nodes(extra: Value) -> Vec<Value> {
    vec![with_fields(
        json!({"type": "notes", "name": "n", "notes": [["A4", 1], ["C5", 1]], "bpm": 120}),
        extra,
    )]
}

#[test]
fn notes_hold_pitches_and_gate_and_emit_events() {
    let out = node_audio(notes_nodes(json!({})), "n", 1.0);
    let gate = node_audio(notes_nodes(json!({})), "n.gate", 1.0);
    let (a4, c5) = (out[idx(0.25)], out[idx(0.75)]);
    let step = out.iter().position(|v| *v != 440.0).unwrap();
    println!(
        "notes A4 C5, 120 bpm: {a4:.4} Hz at 0.25 s, {c5:.4} Hz at 0.75 s (C5 = 523.2511), switch at sample {step} (expected {})",
        idx(0.5)
    );
    assert_eq!(a4, 440.0);
    assert!((c5 - 523.25).abs() < 0.1);
    assert_eq!(step, idx(0.5));
    assert_eq!(out[idx(1.0) - 1], c5, "the last pitch is held");
    let ones = gate.iter().filter(|v| **v == 1.0).count();
    let zeros = gate.iter().filter(|v| **v == 0.0).count();
    let fall = gate.iter().position(|v| *v == 0.0).unwrap();
    println!(
        "gate 0.9: {ones} high samples (expected {}), first fall at {fall} (expected {})",
        2 * idx(0.45),
        idx(0.45)
    );
    assert_eq!(ones + zeros, gate.len());
    assert_eq!((ones, fall), (2 * idx(0.45), idx(0.45)));
    let ev = events(notes_nodes(json!({})), "n.events", 1.0);
    println!(
        "events port: {:?}",
        ev.iter().map(|e| (e.t, e.value, e.id)).collect::<Vec<_>>()
    );
    assert_eq!(ev.len(), 2);
    assert_eq!((ev[0].t, ev[0].id, ev[0].value), (0.0, 0, 440.0));
    assert!((ev[1].t - 0.5).abs() < 1e-9 && ev[1].id == 1 && (ev[1].value - 523.25).abs() < 0.1);
    // The default port is the pitch: an `osc` driven by it, read by YIN.
    let nodes = vec![
        json!({"type": "notes", "name": "n", "notes": [["A4", 1], ["C5", 1]], "bpm": 120}),
        json!({"type": "osc", "name": "o", "freq": "n", "amp": 0.5}),
    ];
    let x = node_audio(nodes, "o", 1.0);
    let read = |lo: f32, hi: f32| {
        let seg = &x[idx(lo)..idx(hi)];
        let mut v: Vec<f32> = pitch::yin(seg, SR).iter().filter_map(|f| f.hz).collect();
        v.sort_by(f32::total_cmp);
        assert!(!v.is_empty(), "no voiced frames in {lo}..{hi}");
        v[v.len() / 2]
    };
    let (y1, y2) = (read(0.05, 0.45), read(0.55, 0.95));
    println!("osc driven by notes, YIN median: {y1:.2} Hz (440), {y2:.2} Hz (523.25)");
    assert!((y1 - 440.0).abs() < 2.0 && (y2 - 523.25).abs() < 2.0);
}

#[test]
fn notes_names_semitones_rests_root_and_glide() {
    let hz = |notes: Value, extra: Value| {
        let n = with_fields(
            json!({"type": "notes", "name": "n", "notes": notes, "bpm": 60}),
            extra,
        );
        node_audio(vec![n], "n", 4.0)
    };
    let x = hz(
        json!([["C4", 1], ["D#5", 1], ["Bb3", 1], [12, 1]]),
        json!({}),
    );
    let got = [x[idx(0.5)], x[idx(1.5)], x[idx(2.5)], x[idx(3.5)]];
    println!("C4 D#5 Bb3 and +12 semitones: {got:.4?} Hz");
    for (g, want) in got.iter().zip([261.6256, 622.254, 233.082, 880.0]) {
        assert!((g - want).abs() < 0.01, "{g} vs {want}");
    }
    let r = hz(
        json!([["A4", 1], ["rest", 1], ["E5", 1]]),
        json!({"root": 432}),
    );
    println!(
        "root 432: A4 {:.3} Hz, held through a rest {:.3}, E5 {:.3}",
        r[idx(0.5)],
        r[idx(1.5)],
        r[idx(2.5)]
    );
    assert_eq!((r[idx(0.5)], r[idx(1.5)]), (432.0, 432.0));
    assert!((r[idx(2.5)] - 432.0 * (7.0f32 / 12.0).exp2()).abs() < 0.01);
    let gate = node_audio(
        vec![
            json!({"type": "notes", "name": "n", "bpm": 60, "notes": [["A4", 1], ["rest", 1], ["E5", 1]]}),
        ],
        "n.gate",
        3.0,
    );
    println!(
        "gate over A4, rest, E5: {} high samples (expected {})",
        gate.iter().filter(|v| **v == 1.0).count(),
        2 * idx(0.9)
    );
    assert!(gate[idx(1.5)] == 0.0 && gate[idx(0.5)] == 1.0 && gate[idx(2.5)] == 1.0);
    // A rest first: the pitch before the first note is that note's.
    let lead = hz(json!([["rest", 1], ["A3", 1]]), json!({}));
    assert_eq!((lead[0], lead[idx(1.5)]), (220.0, 220.0));
    // Glide: linear in semitones, so halfway between A4 and A5 is A4 * sqrt 2.
    let g = hz(json!([["A4", 1], ["A5", 1]]), json!({"glide": 0.4}));
    let mid = g[idx(1.2)];
    println!(
        "glide 0.4 s A4 -> A5: halfway {mid:.3} Hz (expected {:.3}), done {:.3}",
        440.0 * 2f32.sqrt(),
        g[idx(1.5)]
    );
    assert!((mid - 440.0 * 2f32.sqrt()).abs() < 0.05 && g[idx(1.5)] == 880.0);
    assert_eq!(g[idx(0.99)], 440.0);
    // Gate 1 keeps the gate high across notes.
    let legato = node_audio(notes_nodes(json!({"gate": 1.0, "bpm": 60})), "n.gate", 2.0);
    assert!(legato.iter().all(|v| *v == 1.0));
    // Notes past the document are cut; the events port stays inside it.
    let ev = events(
        vec![
            json!({"type": "notes", "name": "n", "bpm": 60, "notes": [["A4", 1], ["A4", 1], ["A4", 1]]}),
        ],
        "n.events",
        1.5,
    );
    assert_eq!(ev.len(), 2);
}

#[test]
fn notes_events_drive_a_trigger_env() {
    // Another node reads the events port by name.
    let nodes = vec![
        json!({"type": "notes", "name": "n", "bpm": 120, "notes": [["A4", 1], ["A5", 1]]}),
        json!({"type": "trigger_env", "name": "e", "events": "n.events", "attack": 0,
               "decay": 0.1, "retrigger": "add"}),
    ];
    let x = node_audio(nodes, "e", 1.0);
    println!(
        "env from notes.events: peaks {:.1} (A4 = 440) at 0 s and {:.1} (A5 = 880) at 0.5 s",
        x[0],
        x[idx(0.5)]
    );
    assert!((x[0] - 440.0).abs() < 0.01 && (x[idx(0.5)] - 880.0).abs() < 0.01);
}

fn all_four() -> Vec<Value> {
    vec![
        with_fields(
            pat(0.07, 9),
            json!({"values": {"min": 0.4, "max": 1.0}, "jitter": 0.01}),
        ),
        json!({"type": "trigger_env", "name": "e", "events": "ev", "retrigger": "add", "curve": "smooth"}),
        json!({"type": "event_gate", "name": "g", "events": "ev", "length": 0.02}),
        json!({"type": "seq", "name": "s", "events": "ev", "values": [220, 330, 440], "mode": "random", "glide": 0.01}),
        json!({"type": "notes", "name": "n", "notes": [["A3", 1], ["E4", 0.5], [3, 1]], "bpm": 180, "glide": 0.03}),
        json!({"type": "osc", "name": "o", "freq": "s", "amp": 0.3}),
        json!({"type": "osc", "name": "p", "freq": "n", "amp": 0.3}),
        json!({"type": "math", "name": "a", "op": "mul", "a": "o", "b": "e"}),
        json!({"type": "math", "name": "b", "op": "mul", "a": "p", "b": "n.gate"}),
        json!({"type": "mix", "name": "m", "inputs": ["a", "b", "g"]}),
    ]
}

#[test]
fn sequence_nodes_are_bit_identical() {
    assert_bit_identical(all_four(), 0.8);
}

#[test]
fn sequence_nodes_are_finite_at_the_extremes() {
    let sets = [
        json!({"attack": 0, "decay": 1.0e-9}),
        json!({"attack": 1.0e9, "decay": 1.0e9}),
        json!({"attack": 3.0e-8, "decay": 3.0e-8, "retrigger": "add"}),
    ];
    for extra in sets {
        for (curve, retrigger) in [("exp", "restart"), ("linear", "add"), ("smooth", "restart")] {
            let nodes = [
                with_fields(
                    pat(0.01, 20),
                    json!({"values": {"min": -1.0e6, "max": 1.0e6}}),
                ),
                with_fields(
                    json!({"type": "trigger_env", "name": "e", "events": "ev", "curve": curve,
                           "retrigger": retrigger}),
                    extra.clone(),
                ),
            ];
            // `add` with a huge span is bounded by the document, not the span.
            let x = node_audio(nodes, "e", 0.3);
            assert_finite(&x, &format!("trigger_env {extra} {curve} {retrigger}"));
        }
    }
    let gate = node_audio(
        vec![
            pat(0.01, 20),
            json!({"type": "event_gate", "name": "g", "events": "ev", "length": 1.0e9}),
        ],
        "g",
        0.3,
    );
    assert_finite(&gate, "event_gate length 1e9");
    assert!(gate.iter().all(|v| *v == 1.0));
    let s = node_audio(
        vec![
            pat(0.001, 100),
            json!({"type": "seq", "name": "s", "events": "ev", "values": [-1.0e30, 1.0e30, 0.0], "glide": 1.0e9}),
        ],
        "s",
        0.2,
    );
    assert_finite(&s, "seq glide 1e9, values +-1e30");
    let n = node_audio(
        vec![
            json!({"type": "notes", "name": "n", "bpm": 2000, "glide": 1.0e9, "gate": 1.0,
                    "notes": [[-60, 0.001], [60, 1.0e-6], ["C-1", 1.0e6], ["G9", 1]]}),
        ],
        "n",
        0.2,
    );
    assert_finite(&n, "notes");
    println!(
        "extremes: trigger_env attack/decay 0..1e9 with values +-1e6 (3 curves, restart and add), event_gate length 1e9, seq glide 1e9 with values +-1e30, notes at 2000 bpm with 1e-6 beat notes and a 1e9 glide: all finite"
    );
}

#[test]
fn sequence_errors_say_the_field_value_and_fix() {
    let ev = pat(0.1, 3);
    let env = |extra: Value| {
        vec![
            ev.clone(),
            with_fields(
                json!({"type": "trigger_env", "name": "x", "events": "ev"}),
                extra,
            ),
        ]
    };
    for (extra, want) in [
        (json!({"decay": 0}), "decay: 0: give a number > 0"),
        (json!({"attack": -1}), "attack: -1: give a number >= 0"),
        (json!({"retrigger": "stack"}), "stack"),
    ] {
        let e = render_error(env(extra), "x");
        println!("{e}");
        assert!(e.contains(want), "{e}");
    }
    // A stream of 200k events times a long envelope is refused with the limit.
    let e = crate::common::render_error(
        vec![
            json!({"type": "pattern", "name": "ev", "every": 0.00001, "count": 150000}),
            json!({"type": "trigger_env", "name": "x", "events": "ev", "decay": 10, "retrigger": "add"}),
        ],
        "x",
        2.0,
    );
    println!("{e}");
    assert!(e.contains("limit 2e9") && e.contains("restart"), "{e}");
    // Audio where events belong.
    let e = render_error(
        vec![
            json!({"type": "osc", "name": "o", "freq": 5}),
            json!({"type": "event_gate", "name": "x", "events": "o"}),
        ],
        "x",
    );
    println!("{e}");
    assert!(e.contains("events") && e.contains("on_threshold"), "{e}");
    let e = render_error(
        vec![
            ev.clone(),
            json!({"type": "event_gate", "name": "x", "events": "ev", "length": 0}),
        ],
        "x",
    );
    assert!(e.contains("length: 0: give a number > 0"), "{e}");
    let seq = |values: Value| {
        render_error(
            vec![
                ev.clone(),
                json!({"type": "seq", "name": "x", "events": "ev", "values": values}),
            ],
            "x",
        )
    };
    let e = seq(json!([]));
    println!("{e}");
    assert!(e.contains("values: give at least one number"), "{e}");
    let e = render_error(
        vec![
            ev.clone(),
            json!({"type": "seq", "name": "x", "events": "ev", "values": [1], "mode": "shuffle"}),
        ],
        "x",
    );
    println!("{e}");
    assert!(e.contains("shuffle") && e.contains("cycle"), "{e}");
    let notes = |extra: Value| {
        render_error(
            vec![with_fields(json!({"type": "notes", "name": "x"}), extra)],
            "x",
        )
    };
    for (extra, want) in [
        (
            json!({"notes": [["H4", 1]]}),
            "notes[0]: `H4`: the letter must be A to G",
        ),
        (
            json!({"notes": [["A4", 1], ["C#", 1]]}),
            "notes[1]: `C#`: the octave must be a whole number",
        ),
        (
            json!({"notes": [["A44", 1]]}),
            "the octave must be from -1 to 9",
        ),
        (
            json!({"notes": [["A4", 0]]}),
            "notes[0]: beats: 0: give a number > 0",
        ),
        (
            json!({"notes": [["A4", 1]], "bpm": 0}),
            "bpm: 0: give a number from 1 to 2000",
        ),
        (
            json!({"notes": [["A4", 1]], "gate": 0}),
            "gate: 0: give a number above 0, up to 1",
        ),
        (
            json!({"notes": [["A4", 1]], "root": -3}),
            "root: -3: give a number > 0",
        ),
        (
            json!({"notes": []}),
            "notes: give at least one [pitch, beats]",
        ),
        (json!({"notes": [["rest", 1]]}), "only rests"),
        (json!({"notes": [[200, 1]]}), "notes[0]: gives"),
    ] {
        let e = notes(extra);
        println!("{e}");
        assert!(e.contains(want), "{e}");
    }
}

#[test]
fn sequence_nodes_are_registered_with_the_right_flags() {
    for (ty, random_example) in [
        ("trigger_env", false),
        ("event_gate", false),
        ("seq", false),
        ("notes", false),
    ] {
        let ex = sfx_graph::example(ty).expect("registered");
        let mut body = ex.clone();
        body.as_object_mut().unwrap().remove("type");
        body.as_object_mut().unwrap().remove("name");
        let node = parse_body(ty, &body, "x").unwrap();
        assert!(node.causal() && node.random() == random_example, "{ty}");
        assert!(
            sfx_graph::describe(None)
                .unwrap()
                .contains(&format!("- {ty}:"))
        );
    }
    let mut body = json!({"events": "ev", "values": [1, 2], "mode": "random"});
    body.as_object_mut()
        .unwrap()
        .insert("seed".into(), json!(3));
    assert!(parse_body("seq", &body, "x").unwrap().random());
    // Note names and `retrigger` words are not read as node references.
    let r = sfx_graph::node::refs(
        &json!({"type": "notes", "notes": [["A4", 1], ["Bb3", 1], ["rest", 1]]}),
    );
    assert!(r.is_empty(), "{r:?}");
    let r =
        sfx_graph::node::refs(&json!({"type": "trigger_env", "events": "ev", "retrigger": "add"}));
    assert_eq!(r, [("ev".to_string(), String::new())]);
    let text = sfx_graph::describe(Some("notes")).unwrap();
    assert!(text.contains("note name") && text.contains("Tempo"));
}

/// The recipes in docs/nodes/sequence.md, as written there.
fn drum_recipe() -> Vec<Value> {
    vec![
        json!({"type": "pattern", "name": "hits", "every": 0.125, "count": 8, "jitter": 0.004,
               "values": {"min": 0.5, "max": 1.0}, "seed": 2}),
        json!({"type": "trigger_env", "name": "amp", "events": "hits", "attack": 0.001,
               "decay": 0.12, "curve": "exp"}),
        json!({"type": "seq", "name": "pitch", "events": "hits", "values": [180, 150, 120, 150], "glide": 0.02}),
        json!({"type": "osc", "name": "body", "freq": "pitch", "amp": 0.5}),
        json!({"type": "math", "name": "tom", "op": "mul", "a": "body", "b": "amp"}),
    ]
}

fn melody_recipe() -> Vec<Value> {
    vec![
        json!({"type": "notes", "name": "tune", "bpm": 150, "glide": 0.03, "gate": 0.8,
               "notes": [["A3", 1], ["C4", 1], ["E4", 1], ["rest", 1], ["A4", 2]]}),
        json!({"type": "osc", "name": "voice", "freq": "tune", "shape": "saw", "amp": 0.4}),
        json!({"type": "event_gate", "name": "pluck_gate", "events": "tune.events", "length": 0.03}),
        json!({"type": "trigger_env", "name": "pluck", "events": "tune.events", "attack": 0.003,
               "decay": 0.25, "scale_by_value": false}),
        json!({"type": "math", "name": "line", "op": "mul", "a": "voice", "b": "pluck"}),
    ]
}

#[test]
fn sequence_recipes_render() {
    let x = node_audio(drum_recipe(), "tom", 1.0);
    assert_finite(&x, "tom");
    let pk = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    println!(
        "drum recipe: 1 s, peak {pk:.3}, rms {:.2} dB, first-hit pitch {:.1} Hz",
        rms_db(&x),
        peak_hz(&x[..idx(0.04)], SR)
    );
    assert!(pk > 0.1 && rms_db(&x) > -40.0);
    let y = node_audio(melody_recipe(), "line", 3.0);
    assert_finite(&y, "line");
    let g = node_audio(melody_recipe(), "pluck_gate", 3.0);
    println!(
        "melody recipe: 3 s, rms {:.2} dB; gate from tune.events: {} high samples (4 notes x {})",
        rms_db(&y),
        g.iter().filter(|v| **v == 1.0).count(),
        idx(0.03)
    );
    assert_eq!(g.iter().filter(|v| **v == 1.0).count(), 4 * idx(0.03));
    assert!(rms_db(&y) > -40.0);
}

#[test]
fn pattern_accel_scales_the_last_gap_and_default_is_unchanged() {
    let ev = |extra: Value| events(vec![with_fields(pat(0.1, 10), extra)], "ev", 1.5);
    let plain = ev(json!({}));
    let one = ev(json!({"accel": 1.0}));
    let ts = |v: &[Event]| v.iter().map(|e| e.t).collect::<Vec<_>>();
    assert_eq!(ts(&plain), ts(&one));
    let fast = ev(json!({"accel": 0.5}));
    assert_eq!(fast.len(), 10);
    let gap = |k: usize| fast[k + 1].t - fast[k].t;
    let sr = f64::from(SR as f32);
    assert!((gap(0) - 0.1).abs() * sr < 1.0, "first gap {}", gap(0));
    assert!((gap(8) - 0.05).abs() * sr < 1.0, "last gap {}", gap(8));
    // jitter still applies on top
    let j = ev(json!({"accel": 0.5, "jitter": 0.004}));
    assert_ne!(ts(&j), ts(&fast));
    let err = render_error(vec![with_fields(pat(0.1, 4), json!({"accel": 20}))], "ev");
    assert!(err.contains("accel"), "{err}");
}
