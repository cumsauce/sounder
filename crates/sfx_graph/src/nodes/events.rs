//! Event streams and granular playback.
//!
//! An event is `{t seconds, value, id}`; a stream is sorted by time, lies
//! inside the document, and numbers its events 0..n in time order. Event
//! nodes output `Buf::Events`; `grains` (and, later, `voices`) consume them
//! through [`Cx::events`](crate::node::Cx::events).

use std::f64::consts::TAU;

use serde::Deserialize;

use super::{CurveSpec, non_negative, one, positive};
use crate::buf::{Buf, Event};
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, In, Outs, is_file};

/// Most events one node may make (a runaway rate is an error, not a hang).
const MAX_EVENTS: usize = 200_000;

/// A uniform draw in [0, 1) with 53 bits.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// One draw between `min` and `max` along `curve` (one RNG draw).
fn draw_between(min: f32, max: f32, curve: CurveSpec, rng: &mut Rng) -> f32 {
    let u = unit(rng);
    let (a, b) = (f64::from(min), f64::from(max));
    let v = match curve {
        CurveSpec::Linear => a + (b - a) * u,
        CurveSpec::Smooth => a + (b - a) * u * u * (3.0 - 2.0 * u),
        CurveSpec::Exp => a * (b / a).powf(u),
    };
    v as f32
}

/// Per-event randomness for the event `value` (default: always 1).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Values {
    pub min: f32,
    pub max: f32,
    /// linear (uniform, default), exp (geometric: both ends > 0), smooth
    /// (favours the ends).
    #[serde(default)]
    pub curve: CurveSpec,
}

impl Values {
    fn draw(&self, rng: &mut Rng) -> f32 {
        draw_between(self.min, self.max, self.curve, rng)
    }

    fn check(&self) -> Result<(), String> {
        if !(self.min.is_finite() && self.max.is_finite()) {
            return Err("values: min and max must be numbers".into());
        }
        if matches!(self.curve, CurveSpec::Exp) && !(self.min > 0.0 && self.max > 0.0) {
            return Err(format!(
                "values: curve \"exp\" needs min and max above 0 (got {}..{}): use \"linear\"",
                self.min, self.max
            ));
        }
        Ok(())
    }
}

/// Times (seconds) with a base strength to a finished stream: inside the
/// document, sorted, valued, numbered.
fn stream(
    cx: &Cx,
    rng: &mut Rng,
    values: &Option<Values>,
    mut times: Vec<(f64, f32)>,
) -> Result<Outs, String> {
    if let Some(v) = values {
        v.check()?;
    }
    let len = cx.n as f64 / f64::from(cx.sr);
    times.retain(|(t, _)| *t >= 0.0 && *t < len);
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    let events = times
        .into_iter()
        .enumerate()
        .map(|(i, (t, base))| Event {
            t,
            value: base * values.as_ref().map_or(1.0, |v| v.draw(rng)),
            id: i as u32,
        })
        .collect();
    Ok(one(Buf::Events(events)))
}

fn check_count(field: &str, n: f64) -> Result<(), String> {
    if n.is_finite() && n <= MAX_EVENTS as f64 {
        Ok(())
    } else {
        Err(format!(
            "{field}: would make about {n:.0} events (limit {MAX_EVENTS}): lower it"
        ))
    }
}

/// One event.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Once {
    /// Seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    #[serde(default)]
    pub values: Option<Values>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Once {
    const BLURB: &'static str = "an event stream with one event at a time";
    fn example() -> serde_json::Value {
        serde_json::json!({"at": 0.1})
    }
}

impl Eval for Once {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let mut rng = cx.rng(self.seed);
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        stream(cx, &mut rng, &self.values, vec![(f64::from(at), 1.0)])
    }
}

/// Events at listed times, or evenly spaced.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    /// Explicit times, seconds (or give `every` + `count`).
    #[serde(default)]
    pub times: Option<Vec<f32>>,
    /// Spacing, seconds.
    #[serde(default)]
    pub every: Option<f32>,
    /// How many events with `every`.
    #[serde(default)]
    pub count: Option<usize>,
    /// Added to every time (default 0).
    #[serde(default)]
    pub start: Option<f32>,
    /// With `every` + `count`: the last gap is `accel` x the first, the gaps
    /// between interpolated geometrically (0.7 speeds up, 2 slows down;
    /// 0.1..10, default 1 = even). `every` is the first gap.
    #[serde(default)]
    pub accel: Option<f32>,
    /// Each time moves by a seeded amount within +-jitter seconds.
    #[serde(default)]
    pub jitter: Option<f32>,
    #[serde(default)]
    pub values: Option<Values>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Pattern {
    const BLURB: &'static str = "events at listed times, or every N seconds (optional jitter)";
    fn example() -> serde_json::Value {
        serde_json::json!({"every": 0.25, "count": 4, "jitter": 0.01})
    }
}

impl Eval for Pattern {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let mut rng = cx.rng(self.seed);
        let start = f64::from(self.start.unwrap_or(0.0));
        let jitter = f64::from(non_negative("jitter", self.jitter.unwrap_or(0.0))?);
        let base: Vec<f64> = match (&self.times, self.every, self.count) {
            (Some(t), None, None) if self.accel.is_none() => {
                t.iter().map(|x| f64::from(*x)).collect()
            }
            (None, Some(every), Some(count)) => {
                let every = f64::from(positive("every", every)?);
                check_count("count", count as f64)?;
                match self.accel {
                    None => (0..count).map(|k| k as f64 * every).collect(),
                    Some(a) => {
                        if !(0.1..=10.0).contains(&a) {
                            return Err(format!(
                                "accel: {a}: give the last gap as a multiple of the first, 0.1 to 10 (0.7 speeds up, 2 slows down)"
                            ));
                        }
                        let ratio = f64::from(a);
                        let steps = count.saturating_sub(2).max(1) as f64;
                        let mut t = 0.0;
                        (0..count)
                            .map(|k| {
                                let at = t;
                                t += every * ratio.powf(k as f64 / steps);
                                at
                            })
                            .collect()
                    }
                }
            }
            (Some(_), _, _) if self.accel.is_some() => {
                return Err("`accel` needs `every` and `count`, not `times`".into());
            }
            _ => {
                return Err(
                    "give either `times` [..] or both `every` and `count` (not a mix)".into(),
                );
            }
        };
        check_count("times", base.len() as f64)?;
        let times = base
            .into_iter()
            .map(|t| {
                let j = if jitter > 0.0 {
                    jitter * (2.0 * unit(&mut rng) - 1.0)
                } else {
                    0.0
                };
                ((t + start + j).max(0.0), 1.0)
            })
            .collect();
        stream(cx, &mut rng, &self.values, times)
    }
}

/// Random events at an average `rate` per second (a Poisson process).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Poisson {
    /// Events a second: a number, or a signal so the rate can vary (thinning).
    pub rate: In,
    #[serde(default)]
    pub values: Option<Values>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Poisson {
    const BLURB: &'static str = "random events at a rate per second (the rate may be a signal)";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 20, "values": {"min": 0.3, "max": 1.0}})
    }
}

impl Eval for Poisson {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = cx.mono("rate", &self.rate)?;
        let mut rng = cx.rng(self.seed);
        let len = cx.n as f64 / f64::from(cx.sr);
        let rmax = match rate.constant() {
            Some(r) => f64::from(r),
            None => (0..cx.n)
                .map(|i| f64::from(rate.at(i)))
                .fold(0.0f64, f64::max),
        };
        if !rmax.is_finite() || rmax < 0.0 {
            return Err(format!("rate: {rmax}: give events per second >= 0"));
        }
        check_count("rate", rmax * len)?;
        let mut times = Vec::new();
        if rmax > 0.0 {
            let mut t = 0.0f64;
            loop {
                t += -(1.0 - unit(&mut rng)).ln() / rmax;
                if t >= len {
                    break;
                }
                let i = ((t * f64::from(cx.sr)) as usize).min(cx.n - 1);
                if unit(&mut rng) * rmax < f64::from(rate.at(i)) {
                    times.push((t, 1.0));
                }
            }
        }
        stream(cx, &mut rng, &self.values, times)
    }
}

/// A dropped object: impacts at shrinking intervals.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bounce {
    /// Seconds between the first impact and the second.
    pub first: f32,
    /// 0..1 (exclusive): each interval is this times the one before, and an
    /// impact's value is this to the k-th power.
    pub restitution: f32,
    /// Stop when the next interval would be shorter (default 0.004 s).
    #[serde(default)]
    pub min_interval: Option<f32>,
    /// Time of the first impact, seconds (default 0).
    #[serde(default)]
    pub start: Option<f32>,
    /// Multiplies the impact strength by a seeded draw.
    #[serde(default)]
    pub values: Option<Values>,
    /// 0..0.5 (default 0): each hit after the first moves by a seeded
    /// random amount of up to this fraction of the gap before it, so
    /// exported variations (other seeds) differ in timing.
    #[serde(default)]
    pub jitter: Option<f32>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Bounce {
    const BLURB: &'static str =
        "events of a dropped object: intervals shrink by the restitution (optional jitter)";
    fn example() -> serde_json::Value {
        serde_json::json!({"first": 0.3, "restitution": 0.6, "min_interval": 0.004})
    }
}

impl Eval for Bounce {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let first = f64::from(positive("first", self.first)?);
        let r = f64::from(self.restitution);
        if !(r > 0.0 && r < 1.0) {
            return Err(format!(
                "restitution: {r}: give a number between 0 and 1 (exclusive); 0.6 is a rubber ball"
            ));
        }
        let min = f64::from(positive(
            "min_interval",
            self.min_interval.unwrap_or(0.004),
        )?);
        let jitter = f64::from(self.jitter.unwrap_or(0.0));
        if !(0.0..=0.5).contains(&jitter) {
            return Err(format!(
                "jitter: {jitter}: give a fraction of the gap from 0 to 0.5"
            ));
        }
        let mut rng = cx.rng(self.seed);
        let mut t = f64::from(non_negative("start", self.start.unwrap_or(0.0))?);
        let mut gap = first;
        let mut strength = 1.0f64;
        let mut times = vec![(t, 1.0f32)];
        while gap >= min && times.len() < MAX_EVENTS {
            t += gap;
            // The unshifted times keep running; only the stored hit moves,
            // and no draw is made at jitter 0 (the stream is unchanged).
            let shifted = if jitter > 0.0 {
                t + (unit(&mut rng) * 2.0 - 1.0) * jitter * gap
            } else {
                t
            };
            gap *= r;
            strength *= r;
            times.push((shifted, strength as f32));
        }
        if gap >= min {
            return Err(format!(
                "bounce: more than {MAX_EVENTS} impacts: raise min_interval or lower restitution"
            ));
        }
        stream(cx, &mut rng, &self.values, times)
    }
}

/// A cluster of events over a short span.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Burst {
    pub count: usize,
    /// Seconds from the first event to the last.
    pub spread: f32,
    /// Timing density: linear (even), exp (dense at the start, thinning out),
    /// smooth (dense at both ends).
    #[serde(default)]
    pub curve: CurveSpec,
    /// Time of the first event, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    #[serde(default)]
    pub values: Option<Values>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Burst {
    const BLURB: &'static str = "a cluster of events over a short span";
    fn example() -> serde_json::Value {
        serde_json::json!({"count": 6, "spread": 0.05, "curve": "exp"})
    }
}

impl Eval for Burst {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        check_count("count", self.count as f64)?;
        let spread = f64::from(non_negative("spread", self.spread)?);
        let at = f64::from(non_negative("at", self.at.unwrap_or(0.0))?);
        let mut rng = cx.rng(self.seed);
        let last = self.count.saturating_sub(1).max(1) as f64;
        let times = (0..self.count)
            .map(|k| {
                let u = k as f64 / last;
                let s = match self.curve {
                    CurveSpec::Linear => u,
                    CurveSpec::Exp => ((4.0 * u).exp() - 1.0) / (4.0f64.exp() - 1.0),
                    CurveSpec::Smooth => u * u * (3.0 - 2.0 * u),
                };
                (at + spread * s, 1.0)
            })
            .collect();
        stream(cx, &mut rng, &self.values, times)
    }
}

/// An event at each rising crossing of a level by an audio signal.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OnThreshold {
    /// A mono audio signal.
    pub input: In,
    pub level: f32,
    /// No new event for this many seconds after one (default 0.01).
    #[serde(default)]
    pub hold: Option<f32>,
}

impl Describe for OnThreshold {
    const BLURB: &'static str = "an event at each rising crossing of a level by an audio signal";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "level": 0.2, "hold": 0.01})
    }
}

impl Eval for OnThreshold {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let x = cx.mono("input", &self.input)?;
        let hold = f64::from(non_negative("hold", self.hold.unwrap_or(0.01))?);
        let sr = f64::from(cx.sr);
        let mut out: Vec<Event> = Vec::new();
        let mut prev = f32::NEG_INFINITY;
        let mut last = f64::NEG_INFINITY;
        for i in 0..cx.n {
            let cur = x.at(i);
            if prev < self.level && cur >= self.level {
                // Interpolate the crossing between samples i-1 and i.
                let frac = if prev.is_finite() && cur > prev {
                    f64::from((self.level - prev) / (cur - prev))
                } else {
                    1.0
                };
                let t = ((i as f64 - 1.0 + frac) / sr).max(0.0);
                if t - last >= hold {
                    out.push(Event {
                        t,
                        value: cur,
                        id: out.len() as u32,
                    });
                    last = t;
                }
            }
            prev = cur;
            if out.len() >= MAX_EVENTS {
                break;
            }
        }
        Ok(one(Buf::Events(out)))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GrainWindow {
    #[default]
    Hann,
    Tri,
    Rect,
}

impl GrainWindow {
    fn at(self, x: f64) -> f32 {
        match self {
            GrainWindow::Hann => (0.5 - 0.5 * (TAU * x).cos()) as f32,
            GrainWindow::Tri => (1.0 - (2.0 * x - 1.0).abs()) as f32,
            GrainWindow::Rect => 1.0,
        }
    }
}

/// A random range drawn once per grain.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrainRange {
    /// Lower end of the range (min <= max).
    pub min: f32,
    /// Upper end of the range.
    pub max: f32,
    /// linear (uniform, default), exp (geometric: both ends > 0), smooth
    /// (favours the ends).
    #[serde(default)]
    pub curve: CurveSpec,
}

/// A per-grain quantity: a number, a signal read at the grain's start, or a
/// `{min, max, [curve]}` range drawn for each grain.
#[derive(Clone, Debug)]
pub enum PerGrain {
    Signal(In),
    Range(GrainRange),
}

impl<'de> Deserialize<'de> for PerGrain {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = serde_json::Value::deserialize(d)?;
        if v.is_object() {
            serde_json::from_value::<GrainRange>(v)
                .map(PerGrain::Range)
                .map_err(D::Error::custom)
        } else {
            serde_json::from_value::<In>(v)
                .map(PerGrain::Signal)
                .map_err(D::Error::custom)
        }
    }
}

impl schemars::JsonSchema for PerGrain {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PerGrain".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let signal = generator.subschema_for::<In>();
        let range = generator.subschema_for::<GrainRange>();
        schemars::json_schema!({
            "description": "a number, a signal (\"$param\", \"=expr\" or a node name) read at the grain's start, or a {min, max, curve} range drawn for each grain",
            "anyOf": [signal, range]
        })
    }
}

/// A [`PerGrain`] with its signals resolved.
enum Draw {
    Signal(crate::buf::Sig),
    Range(GrainRange),
}

impl Draw {
    /// `lowest` is the smallest value the field accepts (`strict`: the
    /// value must be above it, not equal).
    fn resolve(
        cx: &Cx,
        field: &str,
        spec: &PerGrain,
        lowest: f32,
        strict: bool,
    ) -> Result<Draw, String> {
        match spec {
            PerGrain::Signal(i) => Ok(Draw::Signal(cx.mono(field, i)?)),
            PerGrain::Range(r) => {
                let (a, b) = (r.min, r.max);
                if !(a.is_finite() && b.is_finite()) {
                    return Err(format!(
                        "{field}: min {a} and max {b} must be numbers: give e.g. {{\"min\": 0.5, \"max\": 2}}"
                    ));
                }
                if a > b {
                    return Err(format!("{field}: min {a} is above max {b}: swap them"));
                }
                let bad = if strict { a <= lowest } else { a < lowest };
                if bad {
                    let rel = if strict { "above" } else { "at least" };
                    return Err(format!(
                        "{field}: min {a}: give a range that stays {rel} {lowest}"
                    ));
                }
                if matches!(r.curve, CurveSpec::Exp) && !(a > 0.0 && b > 0.0) {
                    return Err(format!(
                        "{field}: curve \"exp\" needs min and max above 0 (got {a}..{b}): use \"linear\""
                    ));
                }
                Ok(Draw::Range(r.clone()))
            }
        }
    }

    /// The value for a grain starting at sample `onset`. A range draws once
    /// from `rng`; a signal draws nothing.
    fn value(&self, onset: usize, rng: &mut Rng) -> f32 {
        match self {
            Draw::Signal(s) => s.at(onset),
            Draw::Range(r) => draw_between(r.min, r.max, r.curve, rng),
        }
    }
}

/// Most samples of grain work one node may do.
const MAX_GRAIN_WORK: f64 = 2.0e9;

/// The source as one mono channel, and its rate relative to ours.
fn load_source(cx: &Cx, source: &str) -> Result<(Vec<f32>, f64), String> {
    if is_file(source) {
        let a = crate::wav::read(&cx.path(source))
            .map_err(|e| format!("source: {e} (put the WAV in the project and give its path)"))?;
        Ok((a.mix_mono(), f64::from(a.sample_rate) / f64::from(cx.sr)))
    } else {
        let s = cx.audio("source", &In::Name(source.to_string()))?;
        Ok(((0..cx.n).map(|i| s.at(i)).collect(), 1.0))
    }
}

/// Grain length in samples for `size` seconds.
fn grain_len(size: f32, sr: f32) -> usize {
    ((size * sr).round() as usize).max(2)
}

/// A size read from a signal or drawn: finite and not negative (the
/// length is then at least two samples).
fn clean_size(size: f32) -> f32 {
    if size.is_finite() { size.max(0.0) } else { 0.0 }
}

/// The two channel gains of a pan position in -1..1 (1, 1 when mono).
fn pan_gains(pan: f32, stereo: bool) -> (f32, f32) {
    let a = (f64::from(pan) + 1.0) * 0.25 * std::f64::consts::PI;
    if stereo {
        (a.cos() as f32, a.sin() as f32)
    } else {
        (1.0, 1.0)
    }
}

/// The output channels grains are summed into.
struct GrainBus {
    left: Vec<f32>,
    right: Vec<f32>,
    stereo: bool,
}

impl GrainBus {
    fn new(n: usize, stereo: bool) -> Self {
        GrainBus {
            left: vec![0.0; n],
            right: if stereo { vec![0.0; n] } else { Vec::new() },
            stereo,
        }
    }

    /// One windowed grain of `src` read from `pos` at `step` source samples
    /// per output sample, `glen` long, starting at output sample `onset`.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        src: &[f32],
        mut pos: f64,
        step: f64,
        glen: usize,
        window: GrainWindow,
        gains: (f32, f32),
        value: f32,
        onset: usize,
    ) {
        let slen = src.len();
        let n = self.left.len();
        for j in 0..glen {
            let o = onset + j;
            if o >= n || pos >= (slen - 1) as f64 {
                break;
            }
            let k = pos as usize;
            let f = (pos - k as f64) as f32;
            let v = (src[k] + (src[k + 1] - src[k]) * f)
                * window.at((j as f64 + 0.5) / glen as f64)
                * value;
            self.left[o] += v * gains.0;
            if self.stereo {
                self.right[o] += v * gains.1;
            }
            pos += step;
        }
    }

    fn finish(self) -> Outs {
        one(if self.stereo {
            Buf::from_channels(vec![self.left, self.right])
        } else {
            Buf::Audio(self.left)
        })
    }
}

/// Granular playback: one windowed grain of the source per event.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grains {
    /// A node name (its audio) or a `.wav` file path.
    pub source: String,
    /// An event stream node.
    pub events: In,
    /// Grain length, seconds, above 0: a number, a `{min, max, [curve]}`
    /// range drawn for each grain, or a signal read at the grain's start.
    /// A length under two samples counts as two.
    pub size: PerGrain,
    /// Where in the source each grain starts reading, 0..1 of its length
    /// (a number or a signal read at the event time; default 0).
    #[serde(default)]
    pub position: Option<In>,
    /// Playback ratio (2 = an octave up; default 1): a number, a signal read
    /// at the grain's start (negative counts as 0), or a `{min, max, [curve]}`
    /// range drawn for each grain (min >= 0; use curve exp for an even
    /// spread in octaves).
    #[serde(default)]
    pub pitch: Option<PerGrain>,
    /// hann (default), tri, rect.
    #[serde(default)]
    pub window: GrainWindow,
    /// 0..1: random pan per grain; makes the output stereo.
    #[serde(default)]
    pub pan_spread: Option<f32>,
    /// Random shift of each grain's position, +-jitter of the source (0..1).
    #[serde(default)]
    pub jitter: Option<f32>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Grains {
    const BLURB: &'static str = "granular playback of a node or WAV, one windowed grain per event; size and pitch can be ranges or signals";
    fn example() -> serde_json::Value {
        serde_json::json!({"source": "noise1", "events": "ev", "size": 0.03, "pan_spread": 0.5})
    }
}

impl Eval for Grains {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = Draw::resolve(cx, "size", &self.size, 0.0, true)?;
        if let Draw::Signal(s) = &size
            && let Some(v) = s.constant()
        {
            positive("size", v)?;
        }
        let events = cx.events("events", &self.events)?;
        let position = cx.mono_or("position", &self.position, 0.0)?;
        let pitch = match &self.pitch {
            Some(p) => Draw::resolve(cx, "pitch", p, 0.0, false)?,
            None => Draw::Signal(crate::buf::Sig::Const(1.0)),
        };
        let spread = self.pan_spread.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&spread) {
            return Err(format!("pan_spread: {spread}: give 0..1"));
        }
        let jitter = non_negative("jitter", self.jitter.unwrap_or(0.0))?;
        let (src, ratio) = load_source(cx, &self.source)?;
        let mut rng = cx.rng(self.seed);
        let stereo = spread > 0.0;
        let mut bus = GrainBus::new(cx.n, stereo);
        // A fixed size or a range has a longest grain: the work is bounded
        // before anything renders. A signal is counted as it goes.
        let longest = match &size {
            Draw::Signal(s) => s.constant().map(|v| (grain_len(v, cx.sr), v)),
            Draw::Range(r) => Some((grain_len(r.max, cx.sr), r.max)),
        };
        if let Some((glen, longest_size)) = longest {
            let work = events.len() as f64 * glen.min(cx.n) as f64;
            if work > MAX_GRAIN_WORK {
                return Err(format!(
                    "{} events of {longest_size} s grains is about {work:.0} samples of work (limit 2e9): shorten size or thin the events",
                    events.len()
                ));
            }
        }
        let slen = src.len();
        if slen == 0 {
            return Err("source is empty".into());
        }
        let mut work = 0.0f64;
        for e in &events {
            // Draw first so the stream consumed per event never depends on
            // what the grain reads. Existing draws (pan, jitter) come first
            // and the new ones (size, pitch) after them, so a document that
            // uses neither draws exactly what it always did.
            let pan = if stereo {
                spread * (2.0 * unit(&mut rng) as f32 - 1.0)
            } else {
                0.0
            };
            let jit = if jitter > 0.0 {
                jitter * (2.0 * unit(&mut rng) as f32 - 1.0)
            } else {
                0.0
            };
            let onset = (e.t * f64::from(cx.sr)).round() as usize;
            let gsize = size.value(onset, &mut rng);
            let gpitch = pitch.value(onset, &mut rng);
            if onset >= cx.n {
                continue;
            }
            let glen = grain_len(clean_size(gsize), cx.sr);
            if longest.is_none() {
                work += glen.min(cx.n) as f64;
                if work > MAX_GRAIN_WORK {
                    return Err(format!(
                        "size: the grains so far are about {work:.0} samples of work (limit 2e9): give a shorter size or thin the events"
                    ));
                }
            }
            let p = (position.at(onset) + jit).clamp(0.0, 1.0);
            let step = f64::from(gpitch.max(0.0)) * ratio;
            let gains = pan_gains(pan, stereo);
            bus.add(
                &src,
                f64::from(p) * slen as f64,
                step,
                glen,
                self.window,
                gains,
                e.value,
                onset,
            );
        }
        Ok(bus.finish())
    }
}

/// How `cloud` spaces its grains.
#[derive(Clone, Copy, Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CloudMode {
    /// Grain starts are a Poisson process: gaps are exponential around
    /// 1 / density (default).
    #[default]
    Random,
    /// One grain each time density has accumulated to 1: even spacing at a
    /// steady density.
    Regular,
}

/// A grain cloud: grains of a source scheduled by a density signal, with no
/// event stream.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cloud {
    /// A node name (its audio) or a `.wav` file path.
    pub source: String,
    /// Grains per second: a number or a signal (read every sample; negative
    /// counts as 0, above the sample rate as the sample rate). 0 makes no
    /// grains. At most 200000 grains in a sound.
    pub density: In,
    /// Grain length, seconds, above 0: a number, a `{min, max, [curve]}`
    /// range drawn for each grain, or a signal read at the grain's start.
    pub size: PerGrain,
    /// Where each grain starts reading, 0..1 of the source (a number or a
    /// signal read at the grain's start; default 0).
    #[serde(default)]
    pub position: Option<In>,
    /// Random shift of each grain's position, +-jitter of the source
    /// (0..1, default 0).
    #[serde(default)]
    pub jitter: Option<f32>,
    /// Playback ratio before the spread (2 = an octave up; a number or a
    /// signal read at the grain's start; default 1).
    #[serde(default)]
    pub pitch: Option<In>,
    /// Random detune per grain, +- this many semitones around `pitch`
    /// (>= 0, default 0; uniform in semitones).
    #[serde(default)]
    pub pitch_spread: Option<f32>,
    /// random (default) or regular spacing.
    #[serde(default)]
    pub mode: CloudMode,
    /// hann (default), tri, rect.
    #[serde(default)]
    pub window: GrainWindow,
    /// 0..1: random pan per grain; makes the output stereo.
    #[serde(default)]
    pub pan_spread: Option<f32>,
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Cloud {
    const BLURB: &'static str = "grain cloud: grains of a node or WAV scheduled by a density signal, with random size, detune and pan";
    fn example() -> serde_json::Value {
        serde_json::json!({"source": "tone", "density": 80, "size": {"min": 0.02, "max": 0.06},
            "position": 0.2, "pitch_spread": 3, "pan_spread": 0.5})
    }
}

impl Eval for Cloud {
    fn causal(&self) -> bool {
        false
    }
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let density = cx.mono("density", &self.density)?;
        if let Some(d) = density.constant()
            && !(d.is_finite() && (0.0..=cx.sr).contains(&d))
        {
            return Err(format!(
                "density: {d}: give grains per second from 0 to {} (the sample rate)",
                cx.sr
            ));
        }
        let size = Draw::resolve(cx, "size", &self.size, 0.0, true)?;
        if let Draw::Signal(s) = &size
            && let Some(v) = s.constant()
        {
            positive("size", v)?;
        }
        let position = cx.mono_or("position", &self.position, 0.0)?;
        let pitch = cx.mono_or("pitch", &self.pitch, 1.0)?;
        let jitter = non_negative("jitter", self.jitter.unwrap_or(0.0))?;
        let detune = non_negative("pitch_spread", self.pitch_spread.unwrap_or(0.0))?;
        if detune > 120.0 {
            return Err(format!(
                "pitch_spread: {detune}: give semitones from 0 to 120"
            ));
        }
        let spread = self.pan_spread.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&spread) {
            return Err(format!("pan_spread: {spread}: give 0..1"));
        }
        let (src, ratio) = load_source(cx, &self.source)?;
        if src.is_empty() {
            return Err("source is empty".into());
        }
        let slen = src.len();
        let mut rng = cx.rng(self.seed);
        let stereo = spread > 0.0;
        let mut bus = GrainBus::new(cx.n, stereo);
        let regular = matches!(self.mode, CloudMode::Regular);
        let max_density = f64::from(cx.sr);
        // Accumulated density against the next threshold: 1 for regular
        // spacing, an Exp(1) draw for a Poisson process (so a changing
        // density stays exact).
        let threshold = |rng: &mut Rng| {
            if regular {
                1.0
            } else {
                -(1.0 - unit(rng)).ln()
            }
        };
        let mut next = threshold(&mut rng);
        let mut acc = 0.0f64;
        let mut count = 0usize;
        let mut work = 0.0f64;
        for onset in 0..cx.n {
            let d = f64::from(density.at(onset));
            let d = if d.is_finite() {
                d.clamp(0.0, max_density)
            } else {
                0.0
            };
            acc += d / f64::from(cx.sr);
            if acc < next {
                continue;
            }
            acc -= next;
            next = threshold(&mut rng);
            count += 1;
            if count > MAX_EVENTS {
                return Err(format!(
                    "density: more than {MAX_EVENTS} grains by {:.2} s: lower density or shorten the sound",
                    onset as f32 / cx.sr
                ));
            }
            // Draws per grain, in this order: pan, position jitter, size,
            // detune.
            let pan = if stereo {
                spread * (2.0 * unit(&mut rng) as f32 - 1.0)
            } else {
                0.0
            };
            let jit = if jitter > 0.0 {
                jitter * (2.0 * unit(&mut rng) as f32 - 1.0)
            } else {
                0.0
            };
            let gsize = size.value(onset, &mut rng);
            let semis = if detune > 0.0 {
                detune * (2.0 * unit(&mut rng) as f32 - 1.0)
            } else {
                0.0
            };
            let glen = grain_len(clean_size(gsize), cx.sr);
            work += glen.min(cx.n) as f64;
            if work > MAX_GRAIN_WORK {
                return Err(format!(
                    "size: the grains so far are about {work:.0} samples of work (limit 2e9): give a shorter size or lower density"
                ));
            }
            let p = (position.at(onset) + jit).clamp(0.0, 1.0);
            let ratio_detune = 2f64.powf(f64::from(semis) / 12.0);
            let step = f64::from(pitch.at(onset).max(0.0)) * ratio_detune * ratio;
            bus.add(
                &src,
                f64::from(p) * slen as f64,
                step,
                glen,
                self.window,
                pan_gains(pan, stereo),
                1.0,
                onset,
            );
        }
        Ok(bus.finish())
    }
}
