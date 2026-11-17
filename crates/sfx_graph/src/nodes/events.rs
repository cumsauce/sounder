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
use crate::node::{Cx, Eval, In, Outs, is_file};

/// Most events one node may make (a runaway rate is an error, not a hang).
const MAX_EVENTS: usize = 200_000;

/// A uniform draw in [0, 1) with 53 bits.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
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
        let u = unit(rng);
        let (a, b) = (f64::from(self.min), f64::from(self.max));
        let v = match self.curve {
            CurveSpec::Linear => a + (b - a) * u,
            CurveSpec::Smooth => a + (b - a) * u * u * (3.0 - 2.0 * u),
            CurveSpec::Exp => a * (b / a).powf(u),
        };
        v as f32
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
    /// Each time moves by a seeded amount within +-jitter seconds.
    #[serde(default)]
    pub jitter: Option<f32>,
    #[serde(default)]
    pub values: Option<Values>,
    #[serde(default)]
    pub seed: u64,
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
            (Some(t), None, None) => t.iter().map(|x| f64::from(*x)).collect(),
            (None, Some(every), Some(count)) => {
                let every = f64::from(positive("every", every)?);
                check_count("count", count as f64)?;
                (0..count).map(|k| k as f64 * every).collect()
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
    #[serde(default)]
    pub seed: u64,
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
        let mut rng = cx.rng(self.seed);
        let mut t = f64::from(non_negative("start", self.start.unwrap_or(0.0))?);
        let mut gap = first;
        let mut strength = 1.0f64;
        let mut times = vec![(t, 1.0f32)];
        while gap >= min && times.len() < MAX_EVENTS {
            t += gap;
            gap *= r;
            strength *= r;
            times.push((t, strength as f32));
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

/// Granular playback: one windowed grain of the source per event.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grains {
    /// A node name (its audio) or a `.wav` file path.
    pub source: String,
    /// An event stream node.
    pub events: In,
    /// Grain length, seconds.
    pub size: f32,
    /// Where in the source each grain starts reading, 0..1 of its length
    /// (a number or a signal read at the event time; default 0).
    #[serde(default)]
    pub position: Option<In>,
    /// Playback ratio (2 = an octave up; a number or a signal; default 1).
    #[serde(default)]
    pub pitch: Option<In>,
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

impl Eval for Grains {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = positive("size", self.size)?;
        let events = cx.events("events", &self.events)?;
        let position = cx.mono_or("position", &self.position, 0.0)?;
        let pitch = cx.mono_or("pitch", &self.pitch, 1.0)?;
        let spread = self.pan_spread.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&spread) {
            return Err(format!("pan_spread: {spread}: give 0..1"));
        }
        let jitter = non_negative("jitter", self.jitter.unwrap_or(0.0))?;
        // The source as one mono channel, and its rate relative to ours.
        let (src, ratio): (Vec<f32>, f64) = if is_file(&self.source) {
            let a = crate::wav::read(&cx.path(&self.source)).map_err(|e| {
                format!("source: {e} (put the WAV in the project and give its path)")
            })?;
            (a.mix_mono(), f64::from(a.sample_rate) / f64::from(cx.sr))
        } else {
            let s = cx.audio("source", &crate::node::In::Name(self.source.clone()))?;
            ((0..cx.n).map(|i| s.at(i)).collect(), 1.0)
        };
        let mut rng = cx.rng(self.seed);
        let glen = ((size * cx.sr).round() as usize).max(2);
        let stereo = spread > 0.0;
        let mut left = vec![0.0f32; cx.n];
        let mut right = if stereo {
            vec![0.0f32; cx.n]
        } else {
            Vec::new()
        };
        let slen = src.len();
        if slen == 0 {
            return Err("source is empty".into());
        }
        for e in &events {
            // Draw first so the stream consumed per event never depends on
            // what the grain reads.
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
            if onset >= cx.n {
                continue;
            }
            let p = (position.at(onset) + jit).clamp(0.0, 1.0);
            let step = f64::from(pitch.at(onset).max(0.0)) * ratio;
            let mut pos = f64::from(p) * slen as f64;
            let (gl, gr) = {
                let a = (f64::from(pan) + 1.0) * 0.25 * std::f64::consts::PI;
                if stereo {
                    (a.cos() as f32, a.sin() as f32)
                } else {
                    (1.0, 1.0)
                }
            };
            for j in 0..glen {
                let o = onset + j;
                if o >= cx.n || pos >= (slen - 1) as f64 {
                    break;
                }
                let k = pos as usize;
                let f = (pos - k as f64) as f32;
                let v = (src[k] + (src[k + 1] - src[k]) * f)
                    * self.window.at((j as f64 + 0.5) / glen as f64)
                    * e.value;
                left[o] += v * gl;
                if stereo {
                    right[o] += v * gr;
                }
                pos += step;
            }
        }
        Ok(one(if stereo {
            Buf::from_channels(vec![left, right])
        } else {
            Buf::Audio(left)
        }))
    }
}
