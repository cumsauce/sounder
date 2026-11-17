//! Digital waveguide models: `tube` (a bore driven by a reed, a lip, a jet or
//! noise) and `string` (a bidirectional string plucked, bowed or struck).
//! Both are delay-line loops with a lossy reflection filter and, for the
//! sustained excitations, a memoryless nonlinearity at the point where the
//! player meets the instrument. Sample by sample, no lookahead, so both are
//! causal. The structure follows Smith's waveguide clarinet and bowed string
//! (the reed and bow tables are the ones in the Synthesis ToolKit).

use serde::Deserialize;

use super::{mono, non_negative, one, positive};
use crate::dsp::{Biquad, DelayLine, FilterMode, Smooth, flush};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound the tube is tuned with, m/s (air at 20 C).
const C: f32 = 343.0;
/// Longest tube, metres: the work cap (the delay line is 2 L / c seconds).
const MAX_LENGTH: f32 = 20.0;
/// Shortest loop either model runs, samples.
const MIN_LOOP: f32 = 8.0;

// ---------------------------------------------------------------- helpers

/// The phase delay in samples and the gain of `(b0 + b1 z^-1) / (1 + a1 z^-1)`
/// at angular frequency `w` (radians per sample, below pi / 4).
fn section(b0: f64, b1: f64, a1: f64, w: f64) -> (f64, f64) {
    let (s, c) = w.sin_cos();
    let (nr, ni) = (b0 + b1 * c, -b1 * s);
    let (dr, di) = (1.0 + a1 * c, -a1 * s);
    let phase = ni.atan2(nr) - di.atan2(dr);
    let gain = ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt();
    (-phase / w, gain)
}

/// A one-pole low-pass with its pole `p` (0 passes everything, towards 1 is
/// darker); its phase delay and gain at `w`.
fn lp_response(p: f32, w: f64) -> (f32, f32) {
    let p = f64::from(p);
    let (d, g) = section(1.0 - p, 0.0, -p, w);
    (d as f32, g as f32)
}

/// The same for a first-order all-pass `(a + z^-1) / (1 + a z^-1)`.
fn ap_delay(a: f32, w: f64) -> f32 {
    let a = f64::from(a);
    section(a, 1.0, a, w).0 as f32
}

/// The pole of a one-pole low-pass whose -3 dB point is `hz`.
fn lp_pole(hz: f32, sr: f32) -> f32 {
    (-std::f32::consts::TAU * hz.clamp(1.0, 0.45 * sr) / sr).exp()
}

/// A one-pole low-pass.
#[derive(Clone, Copy, Debug, Default)]
struct Lp1 {
    y: f32,
}

impl Lp1 {
    #[inline]
    fn tick(&mut self, x: f32, p: f32) -> f32 {
        self.y = flush((1.0 - p) * x + p * self.y);
        self.y
    }
}

/// A first-order all-pass `(a + z^-1) / (1 + a z^-1)`.
#[derive(Clone, Copy, Debug, Default)]
struct Ap1 {
    x1: f32,
    y1: f32,
}

impl Ap1 {
    #[inline]
    fn tick(&mut self, x: f32, a: f32) -> f32 {
        let y = a * x + self.x1 - a * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        y
    }
}

/// A DC blocker for the output: 2nd-order high-pass at `hz`.
fn dc_blocker(hz: f32, sr: f32) -> Biquad {
    let mut b = Biquad::new();
    b.set(FilterMode::Hp, hz, 0.707, 0.0, sr);
    b
}

/// The gain and phase delay (samples) at `hz` of any linear filter, by
/// running an impulse through it and summing the response (the filters here
/// ring for well under 4096 samples).
fn probe(mut tick: impl FnMut(f32) -> f32, hz: f32, sr: f32) -> (f32, f32) {
    let w = f64::from(std::f32::consts::TAU * hz / sr);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for n in 0..4096 {
        let h = f64::from(tick(if n == 0 { 1.0 } else { 0.0 }));
        let (s, c) = (w * f64::from(n)).sin_cos();
        re += h * c;
        im -= h * s;
    }
    let delay = -im.atan2(re) / w;
    ((re * re + im * im).sqrt() as f32, delay as f32)
}

/// A signal as a number in `lo..=hi`: non-finite reads as `lo`.
#[inline]
fn clamped(x: f32, lo: f32, hi: f32) -> f32 {
    if x.is_finite() { x.clamp(lo, hi) } else { lo }
}

// ------------------------------------------------------------------- tube

/// What drives the tube.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TubeExcite {
    /// A single reed (Smith's clarinet): the reed closes as the mouth
    /// pressure exceeds the pressure in the bore.
    #[default]
    Reed,
    /// A lip valve (brass): a resonant lip opening, squared.
    Lip,
    /// An air jet across an edge (flute): the bore's own wave, delayed and
    /// bent by a cubic table, steers the jet.
    Jet,
    /// Broadband noise into the bore: no self-oscillation, the tube only
    /// colours the noise.
    Noise,
}

/// A wind instrument bore: a delay-line loop of length 2 L / c with an
/// end-reflection low-pass and loss, driven by a reed, lip, jet or noise at
/// a `pressure` you can shape with an envelope.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tube {
    /// Acoustic length, metres, from 4 c / sample rate (0.029 at 48 kHz: the
    /// loop is at least 8 samples) to 20. The end correction is not added:
    /// this is the length the pitch formula uses.
    pub length: f32,
    /// What drives the tube: reed (default), lip, jet or noise.
    #[serde(default)]
    pub excite: TubeExcite,
    /// Blowing pressure, 0..1: a number or a node (an `adsr` for a
    /// breath). A signal is clamped to 0..1 and smoothed over 8 ms. Below a
    /// threshold the tube only rings down; above it the tone is steady.
    /// With the defaults (0.6 m, bore 0.01) a rising ramp starts the tone
    /// near 0.54 for the reed and 0.3 to 0.35 for the lip and jet; the
    /// threshold moves with the loss (a longer or narrower tube needs more).
    /// Noise has no threshold: its level follows the pressure.
    pub pressure: In,
    /// Bore radius, metres, 0.001 to 0.5 (default 0.01). A wider bore
    /// loses less to friction but radiates more at the open end, which
    /// darkens the tone: the end low-pass sits near 0.8 c / (2 pi r), never
    /// under three times the pitch.
    #[serde(default)]
    pub bore: Option<f32>,
    /// True (default): open at both ends, pitch c / (2 L), all harmonics.
    /// False: closed at one end, pitch c / (4 L), odd harmonics.
    #[serde(default)]
    pub open_end: Option<bool>,
    /// Mixed with the document seed and the node name (default 0): the
    /// breath noise.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Tube {
    const BLURB: &'static str = "wind instrument bore (waveguide): length sets pitch c/(2L) open or c/(4L) closed; reed, lip, jet or noise driven by pressure";
    const WORDS: &'static [&'static str] = &["excite"];
    fn example() -> serde_json::Value {
        serde_json::json!({"length": 0.5, "excite": "reed", "pressure": 0.8, "bore": 0.01, "open_end": false, "seed": 1})
    }
}

impl Eval for Tube {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let sr = cx.sr;
        let min_len = 0.5 * MIN_LOOP * C / sr;
        let l = self.length;
        if !(l.is_finite() && (min_len..=MAX_LENGTH).contains(&l)) {
            return Err(format!(
                "length: {l}: give a length in metres from {min_len:.3} (a loop of {MIN_LOOP} samples at {sr} Hz) to {MAX_LENGTH}"
            ));
        }
        let bore = self.bore.unwrap_or(0.01);
        if !(bore.is_finite() && (0.001..=0.5).contains(&bore)) {
            return Err(format!(
                "bore: {bore}: give the bore radius in metres, 0.001 to 0.5"
            ));
        }
        let pressure = cx.mono("pressure", &self.pressure)?;
        if let Some(p) = pressure.constant()
            && !(0.0..=1.0).contains(&p)
        {
            return Err(format!("pressure: {p}: give a number from 0 to 1"));
        }
        let open = self.open_end.unwrap_or(true);

        // Loop timing: one trip down the bore and back is 2 L / c.
        let d_total = 2.0 * l * sr / C;
        let (sign, f0) = if open {
            (1.0, C / (2.0 * l))
        } else {
            (-1.0, C / (4.0 * l))
        };
        if f0 >= 0.4 * sr {
            return Err(format!(
                "length: {l}: the pitch {f0:.0} Hz is over 40 % of the sample rate: give a longer tube"
            ));
        }
        let w0 = f64::from(std::f32::consts::TAU * f0 / sr);

        // Reflection at the open end(s): friction loss, then a low-pass
        // (twice when both ends are open), then a DC blocker for the loop.
        let loss = (-3.0e-5 * f0.sqrt() / bore * 2.0 * l)
            .exp()
            .clamp(0.6, 0.999);
        let fc = (0.8 * C / (std::f32::consts::TAU * bore))
            .max(3.0 * f0)
            .min(0.45 * sr);
        let pole = lp_pole(fc, sr);
        let stages = if open { 2 } else { 1 };
        let hp_hz = (f0 / 3.0).clamp(1.0, 60.0);
        let new_hp = || dc_blocker(hp_hz, sr);
        let (_, hp_delay) = {
            let mut hp = new_hp();
            probe(|x| hp.tick(x), f0, sr)
        };
        let lp_delay = lp_response(pole, w0).0 * stages as f32;

        // The lip: a resonant low-pass on the pressure difference, tuned
        // above the pitch so its lag at the pitch is small.
        let lip_fc = f0;
        let new_lip = || {
            let mut b = Biquad::new();
            b.set(FilterMode::Bp, lip_fc, 1.5, 0.0, sr);
            b
        };
        let lip_delay = {
            let mut b = new_lip();
            probe(|x| b.tick(x), f0, sr).1
        };

        // The jet's travel time is a fixed fraction of the loop.
        let jet_delay = if matches!(self.excite, TubeExcite::Jet) {
            0.32 * d_total
        } else {
            0.0
        };
        let jet_pole = lp_pole((1.5 * f0).min(0.45 * sr), sr);
        let fd = hp_delay
            + lp_delay
            + match self.excite {
                TubeExcite::Lip => lip_delay,
                TubeExcite::Jet => lp_response(jet_pole, w0).0,
                _ => 0.0,
            };
        let d_bore = (d_total - jet_delay - fd - 1.0).max(1.0);

        let mut bore_line = DelayLine::new(d_bore.ceil() as usize + 8);
        let mut jet_line = DelayLine::new(jet_delay.ceil() as usize + 8);
        let mut lp = [Lp1::default(); 2];
        let mut hp = new_hp();
        let mut jet_lp = Lp1::default();
        let mut lip = new_lip();
        let mut out_hp = dc_blocker(10.0, sr);
        let bias = 0.3f32;
        let mut smooth = Smooth::new(0.0);
        let mut rng = cx.rng(self.seed);
        let excite = self.excite;
        let out_gain = match excite {
            TubeExcite::Reed | TubeExcite::Noise => 1.0,
            TubeExcite::Lip | TubeExcite::Jet => 0.4,
        };

        Ok(one(mono(cx.n, |i| {
            let pm = smooth.tick(clamped(pressure.at(i), 0.0, 1.0), 0.008, sr);
            let y = bore_line.read_lagrange3(d_bore);
            // The wave comes back from the end: loss, low-pass, sign.
            let mut r = y;
            let mut radiated = y;
            for (k, s) in lp.iter_mut().take(stages).enumerate() {
                r = s.tick(r, pole);
                if k == 0 {
                    radiated = r;
                }
            }
            let u = hp.tick(sign * loss * r);
            // Breath noise rides on the pressure.
            let noise = rng.signed();
            let v = match excite {
                TubeExcite::Reed => {
                    let m = pm * (1.0 + 0.02 * noise);
                    let delta = u - m;
                    let reed = (0.7 - 0.3 * delta).clamp(-1.0, 1.0);
                    m + delta * reed
                }
                TubeExcite::Lip => {
                    // A resonant lip filter on the returning wave feeds a
                    // valve biased by the mouth pressure (even harmonics).
                    let z = lip.tick(u) + bias * pm;
                    let t = |z: f32| {
                        let z = z.clamp(-1.0, 1.0);
                        z - z * z * z / 3.0
                    };
                    pm * (2.6 * (t(z) - t(bias * pm)) + 0.02 * noise)
                }
                TubeExcite::Jet => {
                    // The jet's gain falls with frequency: a low-pass just
                    // above the pitch keeps the loop on the fundamental.
                    let uj = jet_lp.tick(u, jet_pole);
                    jet_line.write(uj);
                    let s = jet_line.read_lagrange3(jet_delay).clamp(-1.0, 1.0);
                    let table = s - s * s * s / 3.0;
                    pm * (2.6 * table + 0.02 * noise)
                }
                TubeExcite::Noise => u + 0.3 * pm * noise,
            };
            bore_line.write(v);
            out_hp.tick(out_gain * radiated)
        })))
    }
}

// ----------------------------------------------------------------- string

/// How the string is excited.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StringExcite {
    /// A 2 ms burst of seeded noise at `position`: a plectrum.
    #[default]
    Pluck,
    /// A bow with a stick-slip friction curve, held on for the whole
    /// sound; `bow_pressure` presses it (0 lifts it).
    Bow,
    /// One cycle of a 2 ms sine, a hammer: smoother than a pluck.
    Strike,
}

/// A bidirectional-waveguide string: two delay lines meet at `position`,
/// reflect at the nut (rigid) and at the bridge (a loss low-pass and an
/// all-pass chain for stiffness), and the player acts where they meet.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StringNode {
    /// Pitch, Hz: a number (20 to a sixteenth of the sample rate: 3000 at
    /// 48 kHz) or a node for glides and vibrato (clamped to that range).
    pub freq: In,
    /// pluck (default), bow or strike. A pluck and a strike happen at time
    /// 0; a bow starts at 0 and plays until `bow_pressure` lifts it.
    #[serde(default)]
    pub excite: StringExcite,
    /// Where the string is excited and read, as a fraction of its length
    /// from the bridge, 0..1 (default 0.15; clamped to keep both segments
    /// at least 2 samples). 0.5 is the middle: the even harmonics cancel.
    #[serde(default)]
    pub position: Option<f32>,
    /// 0..1 (default 0.3): how fast the string dies and how fast the highs
    /// go first. The decay time to -60 dB at the pitch runs from 12 s
    /// (0) to 20 ms (1) on an exponential scale; the loop low-pass runs
    /// from 62 times the pitch (0) to twice the pitch (1).
    #[serde(default)]
    pub damping: Option<f32>,
    /// 0..1 (default 0): bending stiffness. A cascade of four all-pass
    /// filters in the loop delays the lows more than the highs, so upper
    /// partials land above whole multiples of the pitch (a piano's
    /// inharmonicity). The pitch itself is kept. For a high pitch the
    /// delay budget limits it (the effect shrinks when the loop is short).
    #[serde(default)]
    pub stiffness: Option<f32>,
    /// Bow pressure, 0..1 (default 0.5): a number or a node. 0 lifts the
    /// bow, ~0.3..0.9 sustains a steady tone, and the friction curve
    /// narrows as it rises. Only the `bow` excitation reads it.
    #[serde(default)]
    pub bow_pressure: Option<In>,
    /// Mixed with the document seed and the node name (default 0): the
    /// pluck's noise burst.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for StringNode {
    const BLURB: &'static str = "plucked, bowed or struck string (waveguide): freq, position along the string, damping, stiffness (inharmonic partials), bow_pressure";
    const WORDS: &'static [&'static str] = &["excite"];
    fn example() -> serde_json::Value {
        serde_json::json!({"freq": 220, "excite": "pluck", "position": 0.15, "damping": 0.3, "stiffness": 0.0, "seed": 1})
    }
}

/// Dispersion sections in the stiffness chain.
const SECTIONS: usize = 4;
/// Control values (delay lengths, filters) are recomputed this often.
const CONTROL: usize = 16;

impl Eval for StringNode {
    fn random(&self) -> bool {
        matches!(self.excite, StringExcite::Pluck)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let sr = cx.sr;
        let freq = cx.mono("freq", &self.freq)?;
        let max_f = sr / MIN_LOOP / 2.0;
        if let Some(f) = freq.constant()
            && !(f.is_finite() && (20.0..=max_f).contains(&f))
        {
            return Err(format!(
                "freq: {f}: give a pitch from 20 to {max_f} Hz (a loop of at least {} samples)",
                2.0 * MIN_LOOP
            ));
        }
        let position = self.position.unwrap_or(0.15);
        let damping = self.damping.unwrap_or(0.3);
        let stiffness = self.stiffness.unwrap_or(0.0);
        for (field, x) in [
            ("position", position),
            ("damping", damping),
            ("stiffness", stiffness),
        ] {
            if !(0.0..=1.0).contains(&x) {
                return Err(format!("{field}: {x}: give a number from 0 to 1"));
            }
        }
        let bow_pressure = cx.mono_or("bow_pressure", &self.bow_pressure, 0.5)?;
        if let Some(p) = bow_pressure.constant()
            && !(0.0..=1.0).contains(&p)
        {
            return Err(format!("bow_pressure: {p}: give a number from 0 to 1"));
        }
        non_negative("damping", damping)?;
        positive("sample rate", sr)?;

        // Nut-to-player fraction.
        let neck_frac = 1.0 - position;
        let max_loop = (sr / 20.0).ceil() as usize + 16;
        let mut neck = DelayLine::new(max_loop);
        let mut bridge = DelayLine::new(max_loop);
        let mut lp = Lp1::default();
        let mut ap = [Ap1::default(); SECTIONS];
        let mut out_hp = dc_blocker(15.0, sr);
        let mut smooth_bow = Smooth::new(0.0);
        let mut rng = cx.rng(self.seed);
        let excite = self.excite;

        // The pluck and the strike are a short signal added at the player.
        let burst_len = ((0.002 * sr).round() as usize).max(4);
        let burst: Vec<f32> = match excite {
            StringExcite::Pluck => {
                // Noise shaped by a half-sine window and two one-pole
                // low-passes at 2.5 kHz, scaled to unit RMS.
                let pole = lp_pole(2500.0, sr);
                let mut lps = [Lp1::default(); 2];
                let mut b: Vec<f32> = (0..burst_len)
                    .map(|k| {
                        let w = (std::f32::consts::PI * (k as f32 + 0.5) / burst_len as f32).sin();
                        let mut x = rng.signed() * w;
                        for l in lps.iter_mut() {
                            x = l.tick(x, pole);
                        }
                        x
                    })
                    .collect();
                let rms = (b.iter().map(|v| v * v).sum::<f32>() / b.len() as f32).sqrt();
                b.iter_mut().for_each(|v| *v /= rms.max(1e-6));
                b
            }
            StringExcite::Strike => (0..burst_len)
                .map(|k| (std::f32::consts::TAU * (k as f32 + 0.5) / burst_len as f32).sin())
                .collect(),
            StringExcite::Bow => Vec::new(),
        };
        let out_gain = match excite {
            StringExcite::Pluck => 0.2,
            StringExcite::Strike => 0.4,
            StringExcite::Bow => 0.5,
        };

        // Control state, refreshed every CONTROL samples.
        let (mut dn, mut db, mut g, mut pole, mut a_disp) =
            (1.0f32, 1.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut last_f = f32::NAN;
        let t60_of = |d: f32| 12.0 * (0.05f32 / 12.0).powf(d);

        Ok(one(mono(cx.n, |i| {
            if i % CONTROL == 0 {
                let f = clamped(freq.at(i), 20.0, max_f);
                if f != last_f {
                    last_f = f;
                    let d = sr / f;
                    let w = f64::from(std::f32::consts::TAU * f / sr);
                    let beta = neck_frac.clamp(3.0 / d, 1.0 - 6.0 / d);
                    // The loop low-pass, and the gain that gives the decay.
                    let fc = f * (2.0 + 60.0 * (1.0 - damping) * (1.0 - damping));
                    pole = lp_pole(fc, sr);
                    let (lp_d, lp_g) = lp_response(pole, w);
                    let tg = 10f32.powf(-3.0 * (d / sr) / t60_of(damping));
                    g = (tg / lp_g.max(1e-3)).min(0.9999);
                    // Stiffness: the all-pass chain, within a delay budget.
                    let budget = 0.6 * d - lp_d - 4.0;
                    a_disp = -0.85 * stiffness.powf(0.25);
                    for _ in 0..24 {
                        if SECTIONS as f32 * ap_delay(a_disp, w) <= budget.max(0.0) {
                            break;
                        }
                        a_disp *= 0.8;
                    }
                    if a_disp.abs() < 1e-4 {
                        a_disp = 0.0;
                    }
                    let disp_d = SECTIONS as f32 * ap_delay(a_disp, w);
                    // The filters' delay comes out of both segments in
                    // proportion, so the player stays where it was put.
                    let fd = lp_d + disp_d;
                    dn = (beta * (d - fd) - 1.0).max(1.0);
                    db = ((1.0 - beta) * (d - fd) - 1.0).max(1.0);
                }
            }
            let wb = bridge.read_lagrange3(db);
            let wn = neck.read_lagrange3(dn);
            // Reflect: the bridge loses and disperses, the nut is rigid.
            let mut r = lp.tick(wb, pole);
            for s in ap.iter_mut() {
                r = s.tick(r, a_disp);
            }
            let br = -g * r;
            let nr = -wn;
            let inject = match excite {
                StringExcite::Pluck | StringExcite::Strike => burst.get(i).copied().unwrap_or(0.0),
                StringExcite::Bow => {
                    let p = smooth_bow.tick(clamped(bow_pressure.at(i), 0.0, 1.0), 0.01, sr);
                    let ramp = (i as f32 / (0.05 * sr)).min(1.0);
                    let vb = 0.25 * ramp;
                    let vd = vb - (br + nr);
                    let slope = 5.0 - 4.0 * p;
                    let table = ((slope * vd).abs() + 0.75).powi(-4).min(1.0);
                    vd * table * (2.0 * p).min(1.0)
                }
            };
            neck.write(br + inject);
            bridge.write(nr + inject);
            out_hp.tick(out_gain * wb)
        })))
    }
}
