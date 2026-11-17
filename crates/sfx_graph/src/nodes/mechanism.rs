//! Mechanism sounds (R1-F): `click`, a small mechanical click (a latch, a
//! switch, a button, a lock, a clasp) as one or two transients through a few
//! body resonances. Tests in `crates/sfx_analysis/tests/mechanism.rs`.

use serde::Deserialize;

use super::{non_negative, one};
use crate::buf::Buf;
use crate::dsp::{Resonator, Rng, max_hz};
use crate::node::{Cx, Describe, Eval, Outs};

/// Which mechanism clicks. Sets the gap between the two transients, the body
/// modes, the ring time and how the second transient differs from the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ClickKind {
    /// A door or box latch: the press, then the catch about 45 ms later, a
    /// little lower.
    Latch,
    /// A small toggle or rocker switch: bright and short, a second snick
    /// about 24 ms after the first.
    Switch,
    /// A push button: the press, then a softer, lower release about 70 ms
    /// later.
    Button,
    /// A lock: a longer ring and a heavier bolt about 72 ms after the first.
    Lock,
    /// A metal clasp or buckle: a bright snick, the second about 35 ms later.
    Clasp,
}

/// What one kind is made of.
struct Recipe {
    /// Seconds from the first transient to the second.
    gap: f32,
    /// Body modes at size 0.5: (Hz, gain).
    modes: &'static [(f32, f32)],
    /// Time for a mode to fall 60 dB at size 0.5, seconds.
    t60: f32,
    /// The second transient: level and frequency factor against the first.
    second_amp: f32,
    second_freq: f32,
    /// Level of the broadband noise tick in the excitation, at hardness 1.
    tick: f32,
}

impl ClickKind {
    fn recipe(self) -> Recipe {
        match self {
            ClickKind::Latch => Recipe {
                gap: 0.045,
                modes: &[(2100.0, 1.0), (3400.0, 0.8), (5200.0, 0.5)],
                t60: 0.035,
                second_amp: 0.8,
                second_freq: 0.85,
                tick: 0.5,
            },
            ClickKind::Switch => Recipe {
                gap: 0.024,
                modes: &[(3000.0, 1.0), (4600.0, 0.8), (6500.0, 0.6)],
                t60: 0.014,
                second_amp: 0.7,
                second_freq: 0.92,
                tick: 0.6,
            },
            ClickKind::Button => Recipe {
                gap: 0.07,
                modes: &[(1800.0, 1.0), (2900.0, 0.7), (4300.0, 0.4)],
                t60: 0.028,
                second_amp: 0.55,
                second_freq: 0.8,
                tick: 0.4,
            },
            ClickKind::Lock => Recipe {
                gap: 0.072,
                modes: &[(1500.0, 1.0), (2400.0, 0.8), (3700.0, 0.6), (6000.0, 0.3)],
                t60: 0.06,
                second_amp: 0.75,
                second_freq: 0.7,
                tick: 0.4,
            },
            ClickKind::Clasp => Recipe {
                gap: 0.035,
                modes: &[(2600.0, 1.0), (4200.0, 0.9), (6800.0, 0.7)],
                t60: 0.022,
                second_amp: 0.75,
                second_freq: 0.9,
                tick: 0.6,
            },
        }
    }
}

/// Most local samples one click may take (a pulse, the gap and the ring):
/// the longest recipe needs about 0.4 s.
const MAX_LOCAL_SECONDS: f32 = 1.0;

/// Peak of the press (the first transient) before the output stage (-6 dBFS).
const PEAK: f32 = 0.5;

/// A mechanical click: one or two transients (the press, then the release or
/// second snick, 24 to 72 ms later by `kind`) through a few small body
/// resonances at 1.5 to 8 kHz. The press peaks at 0.5 (-6 dBFS) whatever the
/// kind, size or hardness, and the second transient is relative to it (if it
/// still comes out louder, the whole click is scaled to peak at 0.5); scale
/// it with a `gain`. A hit at `at` 0 starts at sample
/// 0: give the `output` a `fade_in` of 0.0005 or the default 2 ms fade cuts it.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Click {
    /// Which mechanism: `latch`, `switch`, `button`, `lock` or `clasp`.
    pub kind: ClickKind,
    /// Size of the mechanism, 0..1 (default 0.5). Bigger rings lower and
    /// longer: the body modes scale by 4^(0.5 - size) (x2 at 0, x0.5 at 1)
    /// and the ring time by 2^(size - 0.5).
    #[serde(default)]
    pub size: Option<f32>,
    /// How hard and bright the contact is, 0..1 (default 0.7). Harder is a
    /// shorter, brighter transient with more of the upper modes and more
    /// noise tick.
    #[serde(default)]
    pub hardness: Option<f32>,
    /// Time of the first transient, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Make the second transient (default true). False gives the press alone.
    #[serde(default)]
    pub double: Option<bool>,
    /// How noisy the body rings, 0..1 (default none): each resonance becomes
    /// (1 - noise) x its ring + noise x a band of noise at its frequency (as
    /// `modal` `noise`). The default click reads pitched (voiced 0.33-0.78,
    /// heard as "a chime"); left out or 0, the samples are unchanged.
    #[serde(default)]
    pub noise: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): another
    /// seed moves the mode frequencies, the gap and the levels by +-3 % (levels +-1.5 dB).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Click {
    const BLURB: &'static str = "a mechanical click: latch, switch, button, lock or clasp as one or two transients through small body resonances (size, hardness; seed for another)";
    const WORDS: &'static [&'static str] = &["kind"];
    fn example() -> serde_json::Value {
        serde_json::json!({"kind": "latch", "size": 0.5, "hardness": 0.7, "at": 0.0, "double": true, "seed": 1})
    }
}

/// Adds one transient into `buf` from sample `start`: an excitation (a
/// half-sine pulse of unit area plus a decaying noise tick) run through one
/// resonator per mode.
#[allow(clippy::too_many_arguments)]
fn strike(
    buf: &mut [f32],
    rng: &mut Rng,
    sr: f32,
    start: usize,
    modes: &[(f32, f32)],
    t60: f32,
    hardness: f32,
    tick: f32,
    amp: f32,
    noise: Option<f32>,
) {
    // Pulse width 0.4 ms (soft) to 0.06 ms (hard), unit area so a narrower
    // pulse keeps the low end and reaches higher.
    let width = (0.0004 - 0.00034 * hardness) * sr;
    let plen = (width.round() as usize).clamp(2, 4096);
    let tick_tau = 0.0004 * sr;
    let tlen = (tick_tau * 8.0) as usize;
    let ring = ((t60 * 1.5 * sr) as usize).max(1);
    let len = (plen.max(tlen) + ring).min(buf.len().saturating_sub(start));
    let psum: f32 = (0..plen)
        .map(|k| (std::f32::consts::PI * (k as f32 + 0.5) / plen as f32).sin())
        .sum();
    let mut exc = vec![0.0f32; len];
    for (k, e) in exc.iter_mut().enumerate().take(plen) {
        *e += amp * (std::f32::consts::PI * (k as f32 + 0.5) / plen as f32).sin() / psum;
    }
    let tick_amp = amp * tick * hardness * 0.02;
    for (k, e) in exc.iter_mut().enumerate().take(tlen) {
        *e += tick_amp * rng.signed() * (-(k as f32) / tick_tau).exp();
    }
    // Each mode is a resonator of unit-ish gain; the soft contact darkens
    // the upper modes.
    let f0 = modes.first().map_or(1.0, |m| m.0);
    let params = |hz: f32, g: f32| {
        let tilt = (hz / f0).powf(-1.5 * (1.0 - hardness));
        // Higher modes die first.
        let t = t60 * (f0 / hz).powf(0.5);
        (t, g * tilt)
    };
    if let Some(depth) = noise {
        let mut body = vec![0.0f32; len];
        let mut white = vec![0.0f32; len];
        for &(hz, g) in modes {
            let (t, gain) = params(hz, g);
            let mut r = Resonator::new(hz, t, gain, sr);
            let ring: Vec<f32> = exc.iter().map(|e| r.tick(*e)).collect();
            super::modal::add_noise_band(
                &mut body,
                &ring,
                (hz, t),
                depth,
                super::modal::DEFAULT_NOISE_WIDTH,
                rng,
                &mut white,
                sr,
            );
        }
        for (k, e) in exc.iter().enumerate() {
            buf[start + k] += body[k] + 0.2 * e;
        }
        return;
    }
    let mut bank: Vec<Resonator> = modes
        .iter()
        .map(|&(hz, g)| {
            let (t, gain) = params(hz, g);
            Resonator::new(hz, t, gain, sr)
        })
        .collect();
    for (k, e) in exc.iter().enumerate() {
        let mut y = 0.0;
        for r in &mut bank {
            y += r.tick(*e);
        }
        // A fraction of the excitation itself: the contact is audible apart
        // from the body.
        buf[start + k] += y + 0.2 * e;
    }
}

impl Eval for Click {
    fn random(&self) -> bool {
        true
    }
    // The press is scaled by its own peak, found after rendering it.
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let noise = match self.noise {
            None => None,
            Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => (x > 0.0).then_some(x),
            Some(x) => return Err(format!("noise: {x}: give 0..1")),
        };
        let size = self.size.unwrap_or(0.5);
        if !(size.is_finite() && (0.0..=1.0).contains(&size)) {
            return Err(format!("size: {size}: give a number from 0 to 1"));
        }
        let hardness = self.hardness.unwrap_or(0.7);
        if !(hardness.is_finite() && (0.0..=1.0).contains(&hardness)) {
            return Err(format!("hardness: {hardness}: give a number from 0 to 1"));
        }
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let double = self.double.unwrap_or(true);
        let recipe = self.kind.recipe();
        // Draw every random number up front, in a fixed order, so `double`
        // never changes the first transient.
        let mut rng = cx.rng(self.seed);
        let gap = recipe.gap * (1.0 + 0.03 * rng.signed());
        let level = [
            10f32.powf(1.5 * rng.signed() / 20.0),
            10f32.powf(1.5 * rng.signed() / 20.0),
        ];
        let jitter: Vec<[f32; 2]> = recipe
            .modes
            .iter()
            .map(|_| [1.0 + 0.03 * rng.signed(), 1.0 + 0.03 * rng.signed()])
            .collect();
        let scale = 4f32.powf(0.5 - size);
        let t60 = recipe.t60 * 2f32.powf(size - 0.5);
        let limit = max_hz(cx.sr) * 0.9;
        let mut out = vec![0.0f32; cx.n];
        let start = at as f64 * f64::from(cx.sr);
        if start >= cx.n as f64 {
            return Ok(one(Buf::Audio(out)));
        }
        let start = start as usize;
        // Render relative to the first transient into a local buffer long
        // enough for both rings, so the peak does not depend on the length
        // of the document.
        let local_len = (((gap + 3.0 * t60 + 0.01) * cx.sr) as usize)
            .min((MAX_LOCAL_SECONDS * cx.sr) as usize)
            .max(16);
        let mut local = vec![0.0f32; local_len];
        let mut g = 0.0f32;
        for hit in 0..if double { 2 } else { 1 } {
            let (fscale, amp, off) = if hit == 0 {
                (1.0, 1.0, 0)
            } else {
                (
                    recipe.second_freq,
                    recipe.second_amp,
                    (gap * cx.sr) as usize,
                )
            };
            // Modes above 0.9 of the usable top are dropped, not folded.
            let modes: Vec<(f32, f32)> = recipe
                .modes
                .iter()
                .enumerate()
                .map(|(i, &(hz, mg))| (hz * scale * fscale * jitter[i][hit], mg))
                .filter(|(hz, _)| *hz < limit)
                .collect();
            if modes.is_empty() {
                return Err(format!(
                    "size: {size}: every body mode is above {limit:.0} Hz at this sample rate: raise `size` or the document's sample_rate"
                ));
            }
            strike(
                &mut local,
                &mut rng,
                cx.sr,
                off,
                &modes,
                t60,
                hardness,
                recipe.tick,
                amp * level[hit] * if hit == 0 { 1.0 } else { g },
                noise,
            );
            if hit == 0 {
                // The press peaks at PEAK; the second transient is relative
                // to it, and `double` never changes the press.
                let peak = local.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                g = if peak > 0.0 { PEAK / peak } else { 0.0 };
                for v in &mut local {
                    *v *= g;
                }
            }
        }
        // The second transient can come out louder than the press (a soft,
        // small lock): hold the whole click to PEAK.
        let top = local.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if top > PEAK {
            let down = PEAK / top;
            for v in &mut local {
                *v *= down;
            }
        }
        for (k, v) in local.iter().enumerate() {
            if let Some(o) = out.get_mut(start + k) {
                *o = if v.is_finite() { *v } else { 0.0 };
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}
