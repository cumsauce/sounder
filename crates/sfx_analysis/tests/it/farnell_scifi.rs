//! The sci-fi nodes ported from Farnell's Pd patches (`droid`,
//! `transporter`, `torpedo`, `phaser_beam`): what each patch implies,
//! measured on the node (docs/nodes/farnell_scifi.md).

use crate::common::*;
use serde_json::{Value, json};
use sfx_analysis::describe;

fn one(ty: &str, extra: Value) -> Vec<Value> {
    let mut r = json!({"type": ty, "name": "n"});
    r.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    vec![r]
}

fn secs(x: &[f32], a: f32, b: f32) -> &[f32] {
    let sr = SR as f32;
    &x[(a * sr) as usize..((b * sr) as usize).min(x.len())]
}

#[test]
fn droid_babbles_in_the_patch_range_and_stops_at_length() {
    let x = node_audio(one("droid", json!({"length": 2})), "n", 3.0);
    assert_finite(&x, "droid");
    let top = peak(&x);
    println!(
        "droid peak {top:.3}, rms {:.1} dB",
        rms_db(secs(&x, 0.0, 2.0))
    );
    // The patch ends in x 0.06 after the un-normalised hip~s (gain up to ~2).
    assert!(top > 0.005 && top < 0.15, "peak {top}");
    assert!(rms_db(secs(&x, 0.0, 2.0)) > -60.0);
    assert!(
        secs(&x, 2.05, 3.0).iter().all(|v| *v == 0.0),
        "silent after length"
    );
    // The carrier is at most 0.6 x 9100 Hz, so little sits above 12 kHz,
    // and the 100 Hz high-pass leaves little under it.
    let low = band_db(&x, SR, 0.0, 60.0);
    let mid = band_db(&x, SR, 200.0, 6000.0);
    println!("droid under 60 Hz {low:.1} dB, 200-6000 Hz {mid:.1} dB");
    assert!(mid > low + 15.0);
}

#[test]
fn droid_seeds_differ_and_a_seed_repeats() {
    let a = node_audio(one("droid", json!({"seed": 1})), "n", 2.0);
    let b = node_audio(one("droid", json!({"seed": 2})), "n", 2.0);
    let a2 = node_audio(one("droid", json!({"seed": 1})), "n", 2.0);
    assert_eq!(fnv(&a), fnv(&a2));
    assert_ne!(fnv(&a), fnv(&b));
    assert_bit_identical(one("droid", json!({"seed": 3})), 1.0);
}

#[test]
fn droid_chatter_speeds_the_babble_and_pitch_scales_it() {
    let onsets = |chatter: f32| {
        let x = node_audio(
            one("droid", json!({"chatter": chatter, "length": 6})),
            "n",
            6.0,
        );
        describe(&mono_audio(&x)).onsets.len()
    };
    let (slow, fast) = (onsets(0.0), onsets(1.0));
    println!("droid onsets: chatter 0 {slow}, chatter 1 {fast}");
    assert!(fast > slow + 2, "{fast} vs {slow}");
    // The same seed makes the same decisions, so a pitch factor scales the
    // spectrum of the whole babble.
    let lo = node_audio(one("droid", json!({"pitch": 0.5})), "n", 2.0);
    let hi = node_audio(one("droid", json!({"pitch": 2.0})), "n", 2.0);
    let (clo, chi) = (spectrum_centroid(&lo), spectrum_centroid(&hi));
    println!("droid centroid pitch 0.5 {clo:.0} Hz, pitch 2 {chi:.0} Hz");
    assert!(chi > 1.5 * clo);
}

#[test]
fn droid_mood_zero_is_a_calm_tone() {
    let calm = node_audio(one("droid", json!({"mood": 0.0, "seed": 4})), "n", 2.0);
    let wild = node_audio(one("droid", json!({"mood": 1.0, "seed": 4})), "n", 2.0);
    let (a, b) = (describe(&mono_audio(&calm)), describe(&mono_audio(&wild)));
    let w = |d: &sfx_analysis::Descriptors| d.occupied_hz.1 - d.occupied_hz.0;
    println!(
        "droid occupied band calm {:.0} Hz wide, wild {:.0}",
        w(&a),
        w(&b)
    );
    assert!(w(&b) > 1.5 * w(&a));
}

#[test]
fn transporter_swells_and_rings_on_the_whole_tone_notes() {
    // The shimmer reaches the output only through the reverb, as in the patch.
    let doc = one("transporter", json!({"length": 3, "chime": 0, "reverb": 1}));
    let x = node_audio(&doc, "n", 6.0);
    assert_finite(&x, "transporter");
    let (start, mid, end) = (
        rms_db(secs(&x, 0.0, 0.3)),
        rms_db(secs(&x, 1.2, 1.8)),
        rms_db(secs(&x, 5.0, 6.0)),
    );
    println!("transporter rms: start {start:.1}, middle {mid:.1}, end {end:.1} dB");
    // The triangle swell peaks at length / 2.
    assert!(mid > start + 6.0);
    assert!(mid > end + 6.0);
    // The strong lines are the resonators' notes (466 x 1.12247^k).
    let notes: Vec<f32> = (0..12).map(|k| 466.164 * 1.12247f32.powi(k)).collect();
    let top = peaks(secs(&x, 1.0, 2.5), SR, 4, 20.0);
    for (hz, db) in &top {
        println!("transporter peak {hz:.1} Hz {db:.1} dB");
        assert!(
            notes.iter().any(|n| (n - hz).abs() / n < 0.01),
            "{hz} Hz is not a note of the scale"
        );
    }
}

#[test]
fn transporter_chime_is_the_activation_whistle() {
    let with = node_audio(
        one("transporter", json!({"chime": 1, "reverb": 0})),
        "n",
        6.0,
    );
    let without = node_audio(
        one("transporter", json!({"chime": 0, "reverb": 0})),
        "n",
        6.0,
    );
    // The whistle sits at 775-895 Hz and falls over 5 s, so the first 0.5 s
    // of the two renders differ a lot and the 6th second does not.
    let d = |a: f32, b: f32| {
        rms_db(
            &secs(&with, a, b)
                .iter()
                .zip(secs(&without, a, b))
                .map(|(p, q)| p - q)
                .collect::<Vec<_>>(),
        )
    };
    println!(
        "transporter chime part: 0-0.5 s {:.1} dB, 5.5-6 s {:.1} dB",
        d(0.0, 0.5),
        d(5.5, 6.0)
    );
    assert!(d(0.0, 0.5) > d(5.5, 6.0) + 20.0);
    let whistle = peaks(secs(&with, 0.1, 0.6), SR, 1, 0.0)[0].0;
    println!("transporter whistle peak {whistle:.0} Hz");
    assert!((700.0..1000.0).contains(&whistle), "{whistle}");
    // The noise burst reads the seed.
    let other = node_audio(
        one("transporter", json!({"chime": 1, "reverb": 0, "seed": 9})),
        "n",
        1.0,
    );
    assert_ne!(fnv(secs(&with, 0.0, 0.5)), fnv(secs(&other, 0.0, 0.5)));
}

#[test]
fn torpedo_zaps_down_then_slides() {
    let x = node_audio(one("torpedo", json!({"length": 8, "reverb": 0})), "n", 9.0);
    assert_finite(&x, "torpedo");
    println!("torpedo peak {:.2}", peak(&x));
    // The zap starts at 2700 Hz and the envelope^7 drops it fast: the first
    // 10 ms is high, the 50-85 ms stretch is nearly DC.
    let early = spectrum_centroid(secs(&x, 0.0, 0.012));
    let later = spectrum_centroid(secs(&x, 0.05, 0.085));
    println!("torpedo centroid 0-12 ms {early:.0} Hz, 50-85 ms {later:.0} Hz");
    assert!(early > 1000.0 && later < early / 2.0);
    // The delay grows from 1.2 to 241 ms, so each pass round the loop reads
    // slower and the stored zap slides down: the centroid falls.
    let a = spectrum_centroid(secs(&x, 0.3, 0.6));
    let b = spectrum_centroid(secs(&x, 1.5, 2.0));
    let c = spectrum_centroid(secs(&x, 5.0, 6.0));
    println!("torpedo centroid: 0.3 s {a:.0} Hz, 1.5 s {b:.0} Hz, 5 s {c:.0} Hz");
    assert!(a > 1.5 * b && b > 1.5 * c);
    // Fades out at length.
    assert!(secs(&x, 8.05, 9.0).iter().all(|v| *v == 0.0));
}

#[test]
fn torpedo_ring_and_reverb_are_knobs() {
    let tail = |ring: f32, rev: f32| {
        let x = node_audio(
            one("torpedo", json!({"ring": ring, "reverb": rev, "length": 6})),
            "n",
            6.0,
        );
        rms_db(secs(&x, 3.0, 4.0)) - rms_db(secs(&x, 0.1, 0.6))
    };
    let (short, long) = (tail(0.8, 0.0), tail(0.98, 0.0));
    println!("torpedo tail re start: ring 0.8 {short:.1} dB, ring 0.98 {long:.1} dB");
    assert!(long > short + 10.0);
    let a = node_audio(one("torpedo", json!({"reverb": 0, "length": 3})), "n", 3.0);
    let b = node_audio(one("torpedo", json!({"reverb": 1, "length": 3})), "n", 3.0);
    assert_ne!(fnv(&a), fnv(&b));
    assert_bit_identical(one("torpedo", json!({})), 2.0);
}

#[test]
fn phaser_beam_sits_between_the_wobbling_carriers() {
    let x = node_audio(one("phaser_beam", json!({"length": 1})), "n", 1.5);
    assert_finite(&x, "phaser_beam");
    let body = secs(&x, 0.05, 0.95);
    let total = band_db(body, SR, 0.0, 24000.0);
    let band = band_db(body, SR, 1000.0, 3500.0);
    println!(
        "phaser_beam 1-3.5 kHz {band:.1} dB of {total:.1} dB, centroid {:.0} Hz",
        spectrum_centroid(body)
    );
    assert!(band > total - 1.5, "most energy is in the carriers' band");
    // The 1200 Hz high-pass removes the low end.
    assert!(band_db(body, SR, 0.0, 600.0) < total - 20.0);
    assert!(
        secs(&x, 1.05, 1.5).iter().all(|v| *v == 0.0),
        "silent after length"
    );
}

#[test]
fn phaser_beam_stun_is_6_db_under_kill_and_noise_reads_the_seed() {
    let kill = node_audio(
        one("phaser_beam", json!({"mode": "kill", "seed": 1})),
        "n",
        1.0,
    );
    let stun = node_audio(
        one("phaser_beam", json!({"mode": "stun", "seed": 1})),
        "n",
        1.0,
    );
    let d = rms_db(&kill) - rms_db(&stun);
    println!("phaser_beam kill - stun {d:.2} dB");
    assert!((d - 6.02).abs() < 0.1);
    let other = node_audio(one("phaser_beam", json!({"seed": 2})), "n", 1.0);
    assert_ne!(fnv(&kill), fnv(&other));
    let err = render_error(one("phaser_beam", json!({"mode": "love"})), "n", 0.1);
    assert!(err.contains("kill or stun"), "{err}");
}

#[test]
fn transporter_has_no_dc() {
    // Pd's bp~ passes DC and the reverb sums it up; the node blocks it.
    let x = node_audio(one("transporter", json!({})), "n", 16.0);
    let mean = x.iter().sum::<f32>() / x.len() as f32;
    println!("transporter mean {mean:.6}, rms {:.1} dB", rms_db(&x));
    assert!(mean.abs() < 0.002, "{mean}");
}
