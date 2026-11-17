//! Band-split processing: `filterbank` (a log-spaced bank of band-pass
//! filters with a gain each), `vocoder` (the carrier's bands scaled by the
//! modulator's band envelopes) and `multiband` (Linkwitz-Riley 4th-order
//! crossovers, then drive / compression / gain per band). All causal, none
//! random. A stereo input is run per channel (`filterbank`), or with linked
//! detectors (`multiband`); `vocoder` takes a mono modulator and a mono or
//! stereo carrier.

use std::f32::consts::FRAC_1_SQRT_2;

use serde::Deserialize;

use super::per_channel;
use crate::buf::Buf;
use crate::dsp::{Biquad, FilterMode, Follower};
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{db_to_gain, gain_to_db};

/// Most bands a `filterbank` or `vocoder` takes (work is bands x samples).
const MAX_BANDS: usize = 64;
/// Most crossovers a `multiband` takes (so 8 bands).
const MAX_CROSSOVERS: usize = 7;
/// Largest envelope time, seconds.
const MAX_SECONDS: f32 = 10.0;
/// Largest gain of a `filterbank` band, linear (either sign).
const MAX_BAND_GAIN: f32 = 1000.0;
/// Highest band or crossover frequency, as a share of the sample rate.
const TOP: f32 = 0.45;
/// Lowest band or crossover frequency, Hz.
const BOTTOM: f32 = 20.0;
/// The `multiband` output ports for each band after processing.
const BAND_PORTS: [&str; MAX_CROSSOVERS + 1] = [
    "band0", "band1", "band2", "band3", "band4", "band5", "band6", "band7",
];

/// A non-finite input sample counts as silence, so one bad sample cannot
/// turn a filter's state into NaN for good.
#[inline]
fn clean(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// A time in seconds, 0 ..= [`MAX_SECONDS`], or `default` when left out.
fn seconds(field: &str, x: Option<f32>, default: f32) -> Result<f32, String> {
    let x = x.unwrap_or(default);
    if x.is_finite() && (0.0..=MAX_SECONDS).contains(&x) {
        Ok(x)
    } else {
        Err(format!(
            "{field}: {x}: give seconds between 0 and {MAX_SECONDS}"
        ))
    }
}

/// The band centres and Qs of a log-spaced bank: `bands` from `low` to
/// `high` (the first at `low`, the last at `high`; one band sits at their
/// geometric mean). The default Q makes the neighbours' -3 dB points meet.
fn layout(
    bands: usize,
    low: Option<f32>,
    high: Option<f32>,
    q: Option<f32>,
    sr: f32,
) -> Result<Vec<(f32, f32)>, String> {
    if !(1..=MAX_BANDS).contains(&bands) {
        return Err(format!(
            "bands: {bands}: give a whole number from 1 to {MAX_BANDS}"
        ));
    }
    let top = TOP * sr;
    let range = format!("between {BOTTOM} and {top} Hz (0.45 x the sample rate)");
    let low = low.unwrap_or(80.0);
    let high = high.unwrap_or(12_000.0f32.min(top));
    if !(low.is_finite() && (BOTTOM..=top).contains(&low)) {
        return Err(format!("low: {low}: give a frequency {range}"));
    }
    if !(high.is_finite() && (BOTTOM..=top).contains(&high)) {
        return Err(format!("high: {high}: give a frequency {range}"));
    }
    if low >= high {
        return Err(format!("low: {low}: give a frequency below high ({high})"));
    }
    let octaves = (high / low).log2();
    let q = match q {
        Some(q) if q.is_finite() && (0.1..=100.0).contains(&q) => q,
        Some(q) => return Err(format!("q: {q}: give a number from 0.1 to 100")),
        None => {
            let steps = if bands == 1 { 1 } else { bands - 1 };
            let w = (octaves / steps as f32).max(0.05);
            let r = 2f32.powf(w);
            (r.sqrt() / (r - 1.0)).clamp(0.1, 100.0)
        }
    };
    Ok((0..bands)
        .map(|k| {
            let hz = if bands == 1 {
                (low * high).sqrt()
            } else {
                low * (high / low).powf(k as f32 / (bands - 1) as f32)
            };
            (hz, q)
        })
        .collect())
}

/// Band-pass filters (0 dB at the centre) for a [`layout`].
fn bank(lay: &[(f32, f32)], sr: f32) -> Vec<Biquad> {
    lay.iter()
        .map(|&(hz, q)| {
            let mut b = Biquad::new();
            b.set(FilterMode::Bp, hz, q, 0.0, sr);
            b
        })
        .collect()
}

/// Band-passes in a row per vocoder band: 18 dB an octave skirts instead of
/// 6, so a loud band does not leak into a quiet one far away (measured with
/// two sections: 4 dB of error over a 20 dB tilt, with three: 2 dB).
const STEEP_STAGES: usize = 3;

struct Steep(Vec<Biquad>);

impl Steep {
    fn tick(&mut self, x: f32) -> f32 {
        self.0.iter_mut().fold(x, |y, b| b.tick(y))
    }
}

/// A [`bank`] of [`Steep`] band-passes. Each section's Q is scaled by
/// sqrt(2^(1/n) - 1) so the cascade keeps the -3 dB width of one section at
/// the layout's Q.
fn steep_bank(lay: &[(f32, f32)], sr: f32) -> Vec<Steep> {
    let k = (2f32.powf(1.0 / STEEP_STAGES as f32) - 1.0).sqrt();
    lay.iter()
        .map(|&(hz, q)| {
            let mut b = Biquad::new();
            b.set(FilterMode::Bp, hz, q * k, 0.0, sr);
            Steep(vec![b; STEEP_STAGES])
        })
        .collect()
}

// --------------------------------------------------------------- filterbank

/// A bank of band-pass filters, log-spaced, each scaled by a gain and summed:
/// a graphic equalizer, or a spectral shaper for noise.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filterbank {
    /// Mono or stereo audio (each channel through its own bank).
    pub input: In,
    /// How many bands, 1 ..= 64. The first is centred on `low`, the last on
    /// `high`, the rest log-spaced between (one band sits at their geometric
    /// mean).
    pub bands: usize,
    /// Linear gain per band, lowest first, each -1000 ..= 1000 (default 1).
    /// A list shorter than `bands` leaves the rest at 1; a longer one is an
    /// error. 0 removes a band, a negative gain flips its sign.
    #[serde(default)]
    pub gains: Option<Vec<f32>>,
    /// Quality factor of every band-pass, 0.1 ..= 100. Default: the one at
    /// which neighbouring bands cross at their -3 dB points.
    #[serde(default)]
    pub q: Option<f32>,
    /// Centre of the lowest band, Hz, 20 ..= 0.45 x sample rate (default 80).
    #[serde(default)]
    pub low: Option<f32>,
    /// Centre of the highest band, Hz, above `low` and at most 0.45 x sample
    /// rate (default 12000, or that limit if lower).
    #[serde(default)]
    pub high: Option<f32>,
}

impl Describe for Filterbank {
    const BLURB: &'static str =
        "bank of log-spaced band-pass filters with a gain each (graphic EQ / spectral shaper)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "bands": 8, "gains": [1, 1, 0.5, 0.25, 1, 1, 0, 0]})
    }
}

impl Eval for Filterbank {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let lay = layout(self.bands, self.low, self.high, self.q, cx.sr)?;
        let mut gains = vec![1.0f32; self.bands];
        if let Some(g) = &self.gains {
            if g.len() > self.bands {
                return Err(format!(
                    "gains: {} values for {} bands: give at most one gain per band",
                    g.len(),
                    self.bands
                ));
            }
            for (i, v) in g.iter().enumerate() {
                if !(v.is_finite() && v.abs() <= MAX_BAND_GAIN) {
                    return Err(format!(
                        "gains[{i}]: {v}: give a linear gain between -{MAX_BAND_GAIN} and {MAX_BAND_GAIN}"
                    ));
                }
                gains[i] = *v;
            }
        }
        Ok(super::one(per_channel(&input, cx.n, |x| {
            let mut filters = bank(&lay, cx.sr);
            x.iter()
                .map(|&v| {
                    let v = clean(v);
                    filters
                        .iter_mut()
                        .zip(&gains)
                        .map(|(f, g)| f.tick(v) * g)
                        .sum()
                })
                .collect()
        })))
    }
}

// ------------------------------------------------------------------ vocoder

/// A channel vocoder: the modulator's level in each band scales the same
/// band of the carrier, so the carrier takes on the modulator's spectral
/// shape over time.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Vocoder {
    /// The sound that is shaped: mono or stereo (each channel through its own
    /// synthesis bank, sharing the modulator's envelopes). A rich source
    /// (a buzz, noise) gives the bands something to scale.
    pub carrier: In,
    /// The sound whose band levels are imposed: must be mono.
    pub modulator: In,
    /// How many bands, 1 ..= 64 (default 16), log-spaced from `low` to `high`
    /// as in `filterbank`.
    #[serde(default)]
    pub bands: Option<usize>,
    /// Seconds the band envelope takes to rise (a time constant on the
    /// band's power), 0 ..= 10 (default 0.01).
    #[serde(default)]
    pub attack: Option<f32>,
    /// Seconds the band envelope takes to fall, 0 ..= 10 (default 0.04).
    /// Short times follow the modulator closely but add ripple in the low
    /// bands.
    #[serde(default)]
    pub release: Option<f32>,
    /// Quality factor of every band, 0.1 ..= 100: the -3 dB width of a single
    /// 2nd-order band-pass of this Q. Each band is three of them in a row
    /// (18 dB an octave skirts) with their Q scaled so that width is kept.
    /// Default: neighbours cross at -3 dB.
    #[serde(default)]
    pub q: Option<f32>,
    /// Centre of the lowest band, Hz, 20 ..= 0.45 x sample rate (default 80).
    #[serde(default)]
    pub low: Option<f32>,
    /// Centre of the highest band, Hz (default 12000, or 0.45 x sample rate
    /// if lower).
    #[serde(default)]
    pub high: Option<f32>,
}

impl Describe for Vocoder {
    const BLURB: &'static str =
        "channel vocoder: the modulator's band levels scale the carrier's bands";
    fn example() -> serde_json::Value {
        serde_json::json!({"carrier": "src", "modulator": "noise1", "bands": 16, "attack": 0.01, "release": 0.04})
    }
}

impl Eval for Vocoder {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let carrier = cx.audio("carrier", &self.carrier)?;
        let modulator = cx.mono("modulator", &self.modulator)?;
        let lay = layout(self.bands.unwrap_or(16), self.low, self.high, self.q, cx.sr)?;
        let attack = seconds("attack", self.attack, 0.01)?;
        let release = seconds("release", self.release, 0.04)?;
        let chans = carrier.channels(cx.n);
        let nb = lay.len();
        let mut analysis = steep_bank(&lay, cx.sr);
        let mut follow = vec![Follower::new(); nb];
        let mut synth: Vec<Vec<Steep>> = chans.iter().map(|_| steep_bank(&lay, cx.sr)).collect();
        let mut env = vec![0.0f32; nb];
        let mut out: Vec<Vec<f32>> = chans.iter().map(|_| Vec::with_capacity(cx.n)).collect();
        for i in 0..cx.n {
            let m = clean(modulator.at(i));
            for k in 0..nb {
                let a = analysis[k].tick(m);
                // The follower runs on power; sqrt(2 p) is the amplitude of
                // the sine with that mean power, so a band holding a sine of
                // amplitude A reads A.
                let p = follow[k].tick((a * a).min(1e30), attack, release, cx.sr);
                env[k] = (2.0 * p).sqrt();
            }
            for (c, ch) in chans.iter().enumerate() {
                let x = clean(ch[i]);
                let y: f32 = synth[c]
                    .iter_mut()
                    .zip(&env)
                    .map(|(f, e)| f.tick(x) * e)
                    .sum();
                out[c].push(y);
            }
        }
        Ok(super::one(Buf::from_channels(out)))
    }
}

// ---------------------------------------------------------------- multiband

/// One band of a `multiband`: drive, then compression, then gain. Every
/// field left out does nothing.
#[derive(Clone, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BandSpec {
    /// Gain after the rest, dB, -60 ..= 60 (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Saturation, 0 ..= 100 (default 0 = off): the band is `tanh(drive x) /
    /// tanh(drive)`, which keeps full scale at full scale, rounds peaks and
    /// raises quiet material by about `drive`. Not oversampled: harmonics
    /// above Nyquist fold back.
    #[serde(default)]
    pub drive: Option<f32>,
    /// Compressor threshold, dBFS, -120 ..= 24. Giving it turns the
    /// compressor on (feed-forward, peak detector, hard knee).
    #[serde(default)]
    pub threshold_db: Option<f32>,
    /// Compression ratio, 1 ..= 100 (default 4 once `threshold_db` is
    /// given): each dB over the threshold comes out `1 / ratio` dB over.
    /// Needs `threshold_db`.
    #[serde(default)]
    pub ratio: Option<f32>,
    /// Compressor attack, seconds (time constant of the detector), 0 ..= 10
    /// (default 0.01). Needs `threshold_db`.
    #[serde(default)]
    pub attack: Option<f32>,
    /// Compressor release, seconds, 0 ..= 10 (default 0.1). Needs
    /// `threshold_db`.
    #[serde(default)]
    pub release: Option<f32>,
}

/// A [`BandSpec`] checked and resolved.
struct Band {
    gain: f32,
    drive: f32,
    comp: Option<Comp>,
}

struct Comp {
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
}

impl Band {
    fn neutral() -> Band {
        Band {
            gain: 1.0,
            drive: 0.0,
            comp: None,
        }
    }

    fn resolve(i: usize, s: &BandSpec) -> Result<Band, String> {
        let at = |f: &str| format!("bands[{i}].{f}");
        let gain_db = s.gain_db.unwrap_or(0.0);
        if !(gain_db.is_finite() && gain_db.abs() <= 60.0) {
            return Err(format!(
                "{}: {gain_db}: give dB between -60 and 60",
                at("gain_db")
            ));
        }
        let drive = s.drive.unwrap_or(0.0);
        if !(drive.is_finite() && (0.0..=100.0).contains(&drive)) {
            return Err(format!(
                "{}: {drive}: give a number from 0 to 100",
                at("drive")
            ));
        }
        let comp = match s.threshold_db {
            Some(t) => {
                if !(t.is_finite() && (-120.0..=24.0).contains(&t)) {
                    return Err(format!(
                        "{}: {t}: give dB between -120 and 24",
                        at("threshold_db")
                    ));
                }
                let ratio = s.ratio.unwrap_or(4.0);
                if !(ratio.is_finite() && (1.0..=100.0).contains(&ratio)) {
                    return Err(format!(
                        "{}: {ratio}: give a ratio from 1 to 100",
                        at("ratio")
                    ));
                }
                Some(Comp {
                    threshold: t,
                    ratio,
                    attack: seconds(&at("attack"), s.attack, 0.01)?,
                    release: seconds(&at("release"), s.release, 0.1)?,
                })
            }
            None => {
                for (name, v) in [
                    ("ratio", s.ratio),
                    ("attack", s.attack),
                    ("release", s.release),
                ] {
                    if let Some(v) = v {
                        return Err(format!(
                            "{}: {v}: the compressor is off; add threshold_db to this band",
                            at(name)
                        ));
                    }
                }
                None
            }
        };
        Ok(Band {
            gain: db_to_gain(gain_db),
            drive,
            comp,
        })
    }

    fn is_neutral(&self) -> bool {
        self.gain == 1.0 && self.drive == 0.0 && self.comp.is_none()
    }

    /// Processes the band's channels together (the compressor's detector is
    /// linked across them) in place.
    fn process(&self, ch: &mut [Vec<f32>], sr: f32) {
        if self.is_neutral() {
            return;
        }
        let norm = if self.drive > 0.0 {
            self.drive.tanh()
        } else {
            1.0
        };
        let mut det = Follower::new();
        for i in 0..ch.first().map_or(0, Vec::len) {
            if self.drive > 0.0 {
                for c in ch.iter_mut() {
                    c[i] = (c[i] * self.drive).tanh() / norm;
                }
            }
            let mut g = self.gain;
            if let Some(k) = &self.comp {
                let peak = ch.iter().map(|c| c[i].abs()).fold(0.0, f32::max);
                let level = det.tick(peak.min(1e30), k.attack, k.release, sr);
                let over = gain_to_db(level) - k.threshold;
                if over > 0.0 {
                    g *= db_to_gain(-over * (1.0 - 1.0 / k.ratio));
                }
            }
            for c in ch.iter_mut() {
                c[i] *= g;
            }
        }
    }
}

/// A Linkwitz-Riley 4th-order crossover: two cascaded Butterworth 2nd-order
/// sections per side. The low and high sides sum to an all-pass (flat
/// magnitude), each is 6 dB down at the crossover frequency.
struct Lr4 {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Lr4 {
    fn new(hz: f32, sr: f32) -> Lr4 {
        let make = |mode| {
            let mut b = Biquad::new();
            b.set(mode, hz, FRAC_1_SQRT_2, 0.0, sr);
            [b.clone(), b]
        };
        Lr4 {
            lp: make(FilterMode::Lp),
            hp: make(FilterMode::Hp),
        }
    }

    #[inline]
    fn split(&mut self, x: f32) -> (f32, f32) {
        let lo = self.lp[0].tick(x);
        let lo = self.lp[1].tick(lo);
        let hi = self.hp[0].tick(x);
        let hi = self.hp[1].tick(hi);
        (lo, hi)
    }

    /// The same crossover's all-pass: low plus high.
    #[inline]
    fn allpass(&mut self, x: f32) -> f32 {
        let (lo, hi) = self.split(x);
        lo + hi
    }
}

/// Splits one channel into `crossovers.len() + 1` bands whose sum has a flat
/// magnitude. The input is split from the bottom up (low band off, the rest
/// onward); each band below the top is then passed through the all-pass of
/// every crossover above it, so all bands have the phase of the whole chain
/// and add up without a notch at any crossover.
fn split_bands(x: &[f32], crossovers: &[f32], sr: f32) -> Vec<Vec<f32>> {
    let nx = crossovers.len();
    let mut split: Vec<Lr4> = crossovers.iter().map(|&f| Lr4::new(f, sr)).collect();
    // ap[k][j]: the all-pass of crossover k + 1 + j, applied to band k.
    let mut ap: Vec<Vec<Lr4>> = (0..nx)
        .map(|k| {
            crossovers[k + 1..]
                .iter()
                .map(|&f| Lr4::new(f, sr))
                .collect()
        })
        .collect();
    let mut bands = vec![Vec::with_capacity(x.len()); nx + 1];
    for &v in x {
        let mut rest = clean(v);
        for k in 0..nx {
            let (lo, hi) = split[k].split(rest);
            let mut b = lo;
            for a in &mut ap[k] {
                b = a.allpass(b);
            }
            bands[k].push(b);
            rest = hi;
        }
        bands[nx].push(rest);
    }
    bands
}

/// A multiband processor: Linkwitz-Riley crossovers split the sound into
/// bands, each can be driven, compressed and scaled, and the bands are summed
/// again. With every band neutral the sum has a flat magnitude (the phase
/// follows the crossovers' all-pass).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Multiband {
    /// Mono or stereo audio. Stereo channels are split separately and share
    /// each band's compressor detector (one gain for both).
    pub input: In,
    /// Crossover frequencies, Hz, ascending, 1 ..= 7 of them, each 20 ..=
    /// 0.45 x sample rate. N crossovers make N + 1 bands, lowest first.
    pub crossovers: Vec<f32>,
    /// Settings per band, lowest first. A list shorter than the band count
    /// leaves the rest neutral; a longer one is an error. Ports `band0` ..
    /// `band7` carry each processed band alone.
    #[serde(default)]
    pub bands: Option<Vec<BandSpec>>,
}

impl Describe for Multiband {
    const BLURB: &'static str =
        "Linkwitz-Riley multiband: crossovers, then drive / compress / gain per band";
    fn example() -> serde_json::Value {
        serde_json::json!({
            "input": "src",
            "crossovers": [200, 2000],
            "bands": [
                {"gain_db": 3, "threshold_db": -20, "ratio": 3},
                {},
                {"drive": 2, "gain_db": -2}
            ]
        })
    }
}

impl Eval for Multiband {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let xo = &self.crossovers;
        if xo.is_empty() || xo.len() > MAX_CROSSOVERS {
            return Err(format!(
                "crossovers: {} values: give from 1 to {MAX_CROSSOVERS} frequencies, ascending",
                xo.len()
            ));
        }
        let top = TOP * cx.sr;
        for (i, f) in xo.iter().enumerate() {
            if !(f.is_finite() && (BOTTOM..=top).contains(f)) {
                return Err(format!(
                    "crossovers[{i}]: {f}: give a frequency between {BOTTOM} and {top} Hz (0.45 x the sample rate)"
                ));
            }
            if i > 0 && *f <= xo[i - 1] {
                return Err(format!(
                    "crossovers[{i}]: {f}: give frequencies in ascending order (the one before is {})",
                    xo[i - 1]
                ));
            }
        }
        let nb = xo.len() + 1;
        let mut bands: Vec<Band> = (0..nb).map(|_| Band::neutral()).collect();
        if let Some(specs) = &self.bands {
            if specs.len() > nb {
                return Err(format!(
                    "bands: {} entries for {nb} bands ({} crossovers): give at most one per band",
                    specs.len(),
                    xo.len()
                ));
            }
            for (i, s) in specs.iter().enumerate() {
                bands[i] = Band::resolve(i, s)?;
            }
        }
        let chans = input.channels(cx.n);
        // by_channel[c][k] -> by_band[k][c]
        let mut by_band: Vec<Vec<Vec<f32>>> = vec![Vec::with_capacity(chans.len()); nb];
        for c in &chans {
            for (k, b) in split_bands(c, xo, cx.sr).into_iter().enumerate() {
                by_band[k].push(b);
            }
        }
        for (b, ch) in bands.iter().zip(by_band.iter_mut()) {
            b.process(ch, cx.sr);
        }
        let mut sum = vec![vec![0.0f32; cx.n]; chans.len()];
        for ch in &by_band {
            for (s, c) in sum.iter_mut().zip(ch) {
                for (a, b) in s.iter_mut().zip(c) {
                    *a += b;
                }
            }
        }
        let mut outs: Outs = vec![("out", Buf::from_channels(sum))];
        for (k, ch) in by_band.into_iter().enumerate() {
            outs.push((BAND_PORTS[k], Buf::from_channels(ch)));
        }
        Ok(outs)
    }
}
