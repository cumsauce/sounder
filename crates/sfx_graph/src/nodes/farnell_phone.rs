//! Telephone tones and alarms from Farnell's *Designing Sound* patches
//! (`docs/nodes/farnell_phone.md`): `phone_tone` is PHONETONES/dialtone1.pd,
//! dialtone2.pd, busy-signal.pd, ringingtone.pd, pulsedial.pd and
//! telephone-line.pd plus DTMF/dtmf1.pd; `two_tone` is ALARMS/2tone-3.pd,
//! 2tone-5.pd and 2tone-10.pd. The filters are copies of Pd's, as in
//! `farnell.rs`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{mono, one};
use crate::node::{Cx, Describe, Eval, Outs};

/// Pd's `bp~` (centre Hz, Q); see `farnell.rs`.
#[derive(Default, Clone)]
struct PdBp {
    c1: f32,
    c2: f32,
    gain: f32,
    x1: f32,
    x2: f32,
}

impl PdBp {
    #[allow(clippy::approx_constant)]
    fn new(f: f32, q: f32, sr: f32) -> PdBp {
        let qcos = |f: f32| {
            if (-(0.5 * 3.14159)..=0.5 * 3.14159).contains(&f) {
                let g = f * f;
                ((g * g * g * (-1.0 / 720.0) + g * g * (1.0 / 24.0)) - g * 0.5) + 1.0
            } else {
                0.0
            }
        };
        let omega = f * (2.0 * 3.14159) / sr;
        let omr = if q < 0.001 { 1.0 } else { (omega / q).min(1.0) };
        let r = 1.0 - omr;
        PdBp {
            c1: 2.0 * qcos(omega) * r,
            c2: -r * r,
            gain: 2.0 * omr * (omr + r * omega),
            x1: 0.0,
            x2: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let o = x + self.c1 * self.x1 + self.c2 * self.x2;
        self.x2 = self.x1;
        self.x1 = o;
        self.gain * o
    }
}

/// Pd's `hip~` before 0.44 (no gain normalisation).
#[derive(Default, Clone)]
struct PdHip {
    coef: f32,
    last: f32,
}

impl PdHip {
    fn new(f: f32, sr: f32) -> PdHip {
        PdHip {
            coef: (1.0 - f * TAU / sr).clamp(0.0, 1.0),
            last: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let new = x + self.coef * self.last;
        let out = new - self.last;
        self.last = new;
        out
    }
}

/// Pd's `lop~`.
#[derive(Default, Clone)]
struct PdLop {
    k: f32,
    y: f32,
}

impl PdLop {
    fn new(f: f32, sr: f32) -> PdLop {
        PdLop {
            k: (f * TAU / sr).clamp(0.0, 1.0),
            y: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

// -------------------------------------------------------------- phone_tone

/// Which telephone signal `phone_tone` makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhoneKind {
    /// The dial tone: 350 + 440 Hz, steady (dialtone1.pd).
    Dial,
    /// The busy signal: 480 + 620 Hz, 0.25 s on and 0.25 s off (busy-signal.pd).
    Busy,
    /// The ringing tone: 480 + 440 Hz, 3 s on and 3 s off (ringingtone.pd).
    Ringback,
    /// Touch tones for `digits`: a row and a column sine, 200 ms each with
    /// 100 ms between (dtmf1.pd).
    Dtmf,
    /// A rotary dial: `digits` as trains of 10 pulses a second, 40 ms each
    /// (pulsedial.pd).
    PulseDial,
}

/// A telephone signal (PHONETONES, DTMF): `kind` picks the tone, `digits` the
/// numbers dialled and `line` how much of the telephone line is applied
/// (telephone-line.pd: clip at 0.9, `bp~` 2000 Hz Q 12, x 1.2, `bp~` 400 Hz
/// Q 7, two `hip~` 90). Dial, busy and ringback run for the whole document;
/// give it a `duration`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhoneTone {
    /// `dial`, `busy`, `ringback`, `dtmf` or `pulse_dial`.
    pub kind: PhoneKind,
    /// The numbers dialled, for `dtmf` (0-9 * # A-D) and `pulse_dial` (0-9; 0
    /// is ten pulses); spaces and dashes are ignored. Default "123".
    #[serde(default)]
    pub digits: Option<String>,
    /// How much of the line is applied, 0 (the clean tone) to 1 (the full
    /// patch: the sum is clipped at 0.9 and squeezed through two resonances,
    /// so the tone turns thin and boxy). Default: what the patch does, 1 for
    /// `dial`, `ringback` and `pulse_dial`, 0 for `busy` and `dtmf`. The
    /// level is held, so the knob changes the character, not the loudness.
    #[serde(default)]
    pub line: Option<f32>,
    /// Speed of the dialling, 0.25 to 4 (default 1): `dtmf` digit and gap
    /// times and the `pulse_dial` pulse rate and gap between digits scale by
    /// 1 / tempo.
    #[serde(default)]
    pub tempo: Option<f32>,
}

impl Describe for PhoneTone {
    const WORDS: &'static [&'static str] = &["kind", "digits"];
    const BLURB: &'static str = "a telephone signal (Farnell): kind dial / busy / ringback / dtmf / pulse_dial, digits (dtmf, pulse_dial), line 0..1 (the line's clip and band-limit); give it a duration";
    fn example() -> serde_json::Value {
        serde_json::json!({"kind": "dtmf", "digits": "5551234", "line": 0.3})
    }
}

/// The DTMF (high, low) pair of a key, from dtmf1.pd's tone list.
fn dtmf_pair(c: char) -> Option<(f32, f32)> {
    let (col, row) = match c.to_ascii_uppercase() {
        '1' => (0, 0),
        '2' => (1, 0),
        '3' => (2, 0),
        'A' => (3, 0),
        '4' => (0, 1),
        '5' => (1, 1),
        '6' => (2, 1),
        'B' => (3, 1),
        '7' => (0, 2),
        '8' => (1, 2),
        '9' => (2, 2),
        'C' => (3, 2),
        '*' => (0, 3),
        '0' => (1, 3),
        '#' => (2, 3),
        'D' => (3, 3),
        _ => return None,
    };
    Some((
        [1209.0, 1336.0, 1477.0, 1633.0][col],
        [697.0, 770.0, 852.0, 941.0][row],
    ))
}

/// The keys of `digits`, ignoring separators, checked against `allowed`.
fn keys(digits: &str, allowed: &str) -> Result<Vec<char>, String> {
    let mut out = Vec::new();
    for c in digits.chars() {
        if matches!(c, ' ' | '-' | ',' | '.') {
            continue;
        }
        if !allowed.contains(c.to_ascii_uppercase()) {
            return Err(format!(
                "digits: `{c}` is not a key here: give some of {allowed}"
            ));
        }
        out.push(c.to_ascii_uppercase());
    }
    if out.is_empty() {
        return Err("digits: give at least one key".into());
    }
    Ok(out)
}

/// Sum of two free-running sines at amplitude 0.5 each (peak 1).
struct Pair {
    a: f64,
    b: f64,
}

impl Pair {
    fn new() -> Pair {
        Pair { a: 0.0, b: 0.0 }
    }
    fn tick(&mut self, fa: f32, fb: f32, sr: f32) -> f32 {
        self.a = (self.a + f64::from(fa / sr)).fract();
        self.b = (self.b + f64::from(fb / sr)).fract();
        0.5 * ((self.a as f32 * TAU).sin() + (self.b as f32 * TAU).sin())
    }
}

impl Eval for PhoneTone {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let sr = cx.sr;
        let n = cx.n;
        let default_line = match self.kind {
            PhoneKind::Dial | PhoneKind::Ringback | PhoneKind::PulseDial => 1.0,
            PhoneKind::Busy | PhoneKind::Dtmf => 0.0,
        };
        let line = self.line.unwrap_or(default_line);
        if !(line.is_finite() && (0.0..=1.0).contains(&line)) {
            return Err(format!("line: {line}: give 0 to 1"));
        }
        let tempo = self.tempo.unwrap_or(1.0);
        if !(tempo.is_finite() && (0.25..=4.0).contains(&tempo)) {
            return Err(format!("tempo: {tempo}: give 0.25 to 4"));
        }
        let secs = |s: f32| (s * sr) as usize;
        let mut x = vec![0.0f32; n];
        let mut pair = Pair::new();
        match self.kind {
            PhoneKind::Dial => {
                for v in x.iter_mut() {
                    *v = pair.tick(350.0, 440.0, sr);
                }
            }
            PhoneKind::Busy => {
                // osc~ 2 x 10000, clip 0..1: 0.25 s on, 0.25 s off, which the
                // patch leaves hard; a 1.5 ms ramp keeps the edges from clicking.
                let mut gate = PdLop::new(110.0, sr);
                for (i, v) in x.iter_mut().enumerate() {
                    let on = (i as f32 / sr * 2.0).fract() < 0.5;
                    *v = pair.tick(480.0, 620.0, sr) * gate.tick(f32::from(u8::from(on)));
                }
            }
            PhoneKind::Ringback => {
                // metro 1000 counting mod 6, > 2, through lop~ 100. The patch
                // counts from 0, so it is silent for its first 3 s; here the
                // ring starts at once.
                let mut gate = PdLop::new(100.0, sr);
                for (i, v) in x.iter_mut().enumerate() {
                    let on = ((i as f32 / sr) as u32) % 6 < 3;
                    *v = pair.tick(480.0, 440.0, sr) * gate.tick(f32::from(u8::from(on)));
                }
            }
            PhoneKind::Dtmf => {
                let k = keys(self.digits.as_deref().unwrap_or("123"), "0123456789*#ABCD")?;
                let (on, gap, ramp) = (secs(0.2 / tempo), secs(0.1 / tempo), secs(0.001).max(1));
                let mut hip = PdHip::new(100.0, sr);
                for (d, c) in k.iter().enumerate() {
                    let (hi, lo) = dtmf_pair(*c).unwrap_or((0.0, 0.0));
                    let start = d * (on + gap) + secs(0.01);
                    for i in start..(start + on + ramp).min(n) {
                        // line~: 1 ms up, held, 1 ms down at the end.
                        let up = ((i - start) as f32 / ramp as f32).min(1.0);
                        let down = ((start + on + ramp - i) as f32 / ramp as f32).min(1.0);
                        x[i] = pair.tick(hi, lo, sr) * up.min(down);
                    }
                }
                for v in x.iter_mut() {
                    *v = hip.tick(*v);
                }
            }
            PhoneKind::PulseDial => {
                // A metro of 100 ms toggles a signal to 1 and, 40 ms later, to
                // 0; a digit runs the metro for digit x 100 ms. Only the line
                // turns those steps into sound, so the dry path is the steps
                // with the line's 90 Hz high-pass.
                let k = keys(self.digits.as_deref().unwrap_or("123"), "0123456789")?;
                let (period, make) = (secs(0.1 / tempo), secs(0.04 / tempo));
                let gap = secs(0.7 / tempo);
                let mut at = secs(0.02);
                for c in k {
                    let pulses = c.to_digit(10).map_or(1, |d| if d == 0 { 10 } else { d }) as usize;
                    for p in 0..pulses {
                        let s = at + p * period;
                        for v in x.iter_mut().take((s + make).min(n)).skip(s) {
                            *v = 1.0;
                        }
                    }
                    at += pulses * period + gap;
                }
                let mut hip = PdHip::new(90.0, sr);
                for v in x.iter_mut() {
                    *v = hip.tick(*v);
                }
            }
        }
        if line > 0.0 {
            // telephone-line.pd; the input is scaled up to 2 at line 1, the
            // amplitude the patches' two unit sines reach.
            let drive = 1.0 + line;
            let mut f = (
                PdBp::new(2000.0, 12.0, sr),
                PdBp::new(400.0, 7.0, sr),
                PdHip::new(90.0, sr),
                PdHip::new(90.0, sr),
            );
            let wet: Vec<f32> = x
                .iter()
                .map(|v| {
                    let y = f.0.tick((v * drive).clamp(-0.9, 0.9)) * 1.2;
                    f.3.tick(f.2.tick(f.1.tick(y)))
                })
                .collect();
            // The resonances take 20 to 40 dB off the tone: hold the peak so
            // the knob changes the character, not the level.
            let pk = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let make = if pk(&wet) > 1e-9 {
                (pk(&x) / pk(&wet)).min(1000.0)
            } else {
                0.0
            };
            for (d, w) in x.iter_mut().zip(&wet) {
                *d = (1.0 - line) * *d + line * make * w;
            }
        }
        Ok(one(mono(n, |i| x[i])))
    }
}

// ----------------------------------------------------------------- two_tone

/// How `two_tone` changes between its tones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TwoToneShape {
    /// A square change through `lop~` 70 Hz (2tone-5.pd: the siren-like
    /// alarm of two notes).
    Switch,
    /// Each tone on for 40 % of its half with silence between (2tone-3.pd's
    /// gated beeps, taken for both tones).
    Gated,
    /// A cosine crossfade of equal power (the `cos~` windows of
    /// 2tone-10.pd).
    Smooth,
}

/// Two tones that alternate: ALARMS/2tone-5.pd runs an `osc~ 2` square
/// through `lop~ 70` as g and plays 800 Hz x g + 600 Hz x (1 - g) at 0.2.
/// Both oscillators run all the time, so each tone keeps its phase while it
/// is off. The high tone comes first. Needs a `duration`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TwoTone {
    /// The lower tone, Hz, 20 to 8000 (default 600).
    #[serde(default)]
    pub low: Option<f32>,
    /// The higher tone, Hz, 20 to 8000 (default 800).
    #[serde(default)]
    pub high: Option<f32>,
    /// Full cycles (high then low) a second, 0.05 to 20 (default 2).
    #[serde(default)]
    pub rate: Option<f32>,
    /// `switch` (default), `gated` or `smooth`.
    #[serde(default)]
    pub shape: Option<TwoToneShape>,
}

impl Describe for TwoTone {
    const BLURB: &'static str = "a two-tone alarm (Farnell 2tone patches): low and high Hz alternating rate times a second; shape switch / gated / smooth; give it a duration";
    fn example() -> serde_json::Value {
        serde_json::json!({"low": 600, "high": 800, "rate": 2, "shape": "switch"})
    }
}

impl Eval for TwoTone {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let sr = cx.sr;
        let low = self.low.unwrap_or(600.0);
        let high = self.high.unwrap_or(800.0);
        let rate = self.rate.unwrap_or(2.0);
        if !(low.is_finite() && (20.0..=8000.0).contains(&low)) {
            return Err(format!("low: {low}: give Hz from 20 to 8000"));
        }
        if !(high.is_finite() && (20.0..=8000.0).contains(&high)) {
            return Err(format!("high: {high}: give Hz from 20 to 8000"));
        }
        if !(rate.is_finite() && (0.05..=20.0).contains(&rate)) {
            return Err(format!(
                "rate: {rate}: give cycles a second from 0.05 to 20"
            ));
        }
        let shape = self.shape.unwrap_or(TwoToneShape::Switch);
        let (mut p, mut pl, mut ph) = (0.0f64, 0.0f64, 0.0f64);
        let (mut glo, mut ghi) = (PdLop::new(70.0, sr), PdLop::new(70.0, sr));
        Ok(one(mono(cx.n, |_| {
            let (sl, sh) = ((pl as f32 * TAU).sin(), (ph as f32 * TAU).sin());
            let ph_now = p as f32;
            p = (p + f64::from(rate / sr)).fract();
            pl = (pl + f64::from(low / sr)).fract();
            ph = (ph + f64::from(high / sr)).fract();
            let y = match shape {
                TwoToneShape::Switch => {
                    let g = ghi.tick(f32::from(u8::from(ph_now < 0.5)));
                    sh * g + sl * (1.0 - g)
                }
                TwoToneShape::Gated => {
                    let hi_on = ph_now < 0.4;
                    let lo_on = (0.5..0.9).contains(&ph_now);
                    sh * ghi.tick(f32::from(u8::from(hi_on)))
                        + sl * glo.tick(f32::from(u8::from(lo_on)))
                }
                TwoToneShape::Smooth => {
                    let g = 0.5 + 0.5 * (ph_now * TAU).cos();
                    let a = g * std::f32::consts::FRAC_PI_2;
                    sh * a.sin() + sl * a.cos()
                }
            };
            0.2 * y
        })))
    }
}
