//! Events into the main graph: `trigger_env`, `event_gate`, `seq` and
//! `notes` turn an event stream into control signals (envelopes, gates,
//! stepped values, a melody). Streams are read with
//! [`Cx::events`](crate::node::Cx::events).

use serde::Deserialize;

use super::{CurveSpec, mono, non_negative, one, positive};
use crate::buf::{Buf, Event};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Slack, in samples, when turning a time into a sample index, so an event
/// at 0.1 s (4800 samples, but 4800.00007 once `every: 0.1` went through
/// f32) starts at sample 4800.
const EPS: f64 = 1.0e-3;

/// Most work (samples written) one node may do for its events.
const MAX_WORK: f64 = 2.0e9;

/// Most entries in `seq.values` or `notes.notes`.
const MAX_LIST: usize = 100_000;

/// The sample an event at `t` seconds first affects.
fn start_index(t: f64, sr: f64) -> usize {
    (t * sr - EPS).ceil().max(0.0) as usize
}

// ------------------------------------------------------------ trigger_env

/// What a new event does to an envelope still running.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Retrigger {
    /// The new event replaces the old one: the attack starts from the
    /// current level, so a restart never jumps down.
    #[default]
    Restart,
    /// Every event keeps its own envelope and they sum.
    Add,
}

/// An envelope fired at each event of a stream.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriggerEnv {
    /// The event stream that fires the envelope (`pattern`, `poisson`,
    /// `bounce`, `burst`, `on_threshold`, `once`). Alias `input`.
    #[serde(alias = "input")]
    pub events: In,
    /// Linear rise to the peak, seconds, >= 0 (default 0.002). 0 puts the
    /// peak on the event's first sample.
    #[serde(default)]
    pub attack: Option<f32>,
    /// Fall from the peak to 0, seconds, > 0 (default 0.2). The envelope is
    /// exactly 0 after `attack + decay`.
    #[serde(default)]
    pub decay: Option<f32>,
    /// Shape of the fall: exp (default; 60 dB down at the end, then
    /// trimmed to 0), linear, smooth (raised cosine).
    #[serde(default)]
    pub curve: Option<CurveSpec>,
    /// true (default): the peak is the event's `value`; false: the peak
    /// is 1.
    #[serde(default)]
    pub scale_by_value: Option<bool>,
    /// What a new event does to a running envelope: restart (default) or
    /// add.
    #[serde(default)]
    pub retrigger: Option<Retrigger>,
}

impl Describe for TriggerEnv {
    const BLURB: &'static str = "an envelope fired at each event of a stream (attack, decay, peak = event value, restart or add)";
    const WORDS: &'static [&'static str] = &["retrigger"];
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "attack": 0.002, "decay": 0.2, "curve": "exp", "retrigger": "restart"})
    }
}

/// The fall at `x` in 0..1: 1 at the start, exactly 0 at the end.
fn fall(curve: CurveSpec, x: f64) -> f64 {
    match curve {
        CurveSpec::Linear => 1.0 - x,
        CurveSpec::Smooth => 0.5 + 0.5 * (std::f64::consts::PI * x).cos(),
        CurveSpec::Exp => {
            // 60 dB over the decay, shifted and scaled to end at 0.
            let floor: f64 = 0.001;
            (floor.powf(x) - floor) / (1.0 - floor)
        }
    }
}

/// One running envelope.
#[derive(Clone, Copy)]
struct Active {
    t0: f64,
    peak: f64,
    from: f64,
}

struct Shape {
    attack: f64,
    decay: f64,
    curve: CurveSpec,
}

impl Shape {
    fn level(&self, a: &Active, t: f64) -> f64 {
        let age = (t - a.t0).max(0.0);
        if age < self.attack {
            a.from + (a.peak - a.from) * age / self.attack
        } else {
            let x = (age - self.attack) / self.decay;
            if x >= 1.0 {
                0.0
            } else {
                a.peak * fall(self.curve, x)
            }
        }
    }
}

impl Eval for TriggerEnv {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        let attack = non_negative("attack", self.attack.unwrap_or(0.002))?;
        let decay = positive("decay", self.decay.unwrap_or(0.2))?;
        let shape = Shape {
            attack: f64::from(attack),
            decay: f64::from(decay),
            curve: self.curve.unwrap_or(CurveSpec::Exp),
        };
        let scale = self.scale_by_value.unwrap_or(true);
        let retrigger = self.retrigger.unwrap_or_default();
        let sr = f64::from(cx.sr);
        let peak_of = |e: &Event| if scale { f64::from(e.value) } else { 1.0 };
        let span = ((shape.attack + shape.decay) * sr).ceil() + 1.0;
        let work = span.min(cx.n as f64) * events.len() as f64;
        if retrigger == Retrigger::Add && work > MAX_WORK {
            return Err(format!(
                "{} events of {:.3} s envelopes is about {work:.0} samples of work (limit 2e9): shorten attack and decay, thin the events, or use retrigger \"restart\"",
                events.len(),
                shape.attack + shape.decay
            ));
        }
        match retrigger {
            Retrigger::Restart => {
                let mut k = 0;
                let mut cur: Option<Active> = None;
                Ok(one(mono(cx.n, |i| {
                    let t = i as f64 / sr;
                    while k < events.len() && events[k].t * sr <= i as f64 + EPS {
                        let e = &events[k];
                        let from = cur.map_or(0.0, |a| shape.level(&a, e.t));
                        cur = Some(Active {
                            t0: e.t,
                            peak: peak_of(e),
                            from,
                        });
                        k += 1;
                    }
                    cur.map_or(0.0, |a| shape.level(&a, t)) as f32
                })))
            }
            Retrigger::Add => {
                let mut out = vec![0.0f64; cx.n];
                for e in &events {
                    let a = Active {
                        t0: e.t,
                        peak: peak_of(e),
                        from: 0.0,
                    };
                    let first = start_index(e.t, sr);
                    let last = first
                        .saturating_add(span.min(cx.n as f64) as usize)
                        .min(cx.n);
                    for (i, o) in out.iter_mut().enumerate().take(last).skip(first) {
                        *o += shape.level(&a, i as f64 / sr);
                    }
                }
                Ok(one(Buf::Audio(out.into_iter().map(|v| v as f32).collect())))
            }
        }
    }
}

// ------------------------------------------------------------- event_gate

/// 1 for a while after each event, else 0.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventGate {
    /// The event stream.
    pub events: In,
    /// How long the gate stays 1 after an event, seconds, > 0 (default
    /// 0.05); at least one sample. Overlapping gates merge.
    #[serde(default)]
    pub length: Option<f32>,
}

impl Describe for EventGate {
    const BLURB: &'static str = "a 0 / 1 gate: 1 for `length` seconds after each event";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "length": 0.05})
    }
}

impl Eval for EventGate {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        let length = f64::from(positive("length", self.length.unwrap_or(0.05))?);
        let sr = f64::from(cx.sr);
        let mut out = vec![0.0f32; cx.n];
        // Events are sorted, so everything before `filled` is already done:
        // the work is the buffer length however the gates overlap.
        let mut filled = 0usize;
        for e in &events {
            let start = start_index(e.t, sr).max(filled);
            let end = (((e.t + length) * sr - EPS).ceil().max(0.0) as usize)
                .max(start_index(e.t, sr) + 1)
                .min(cx.n);
            if start < end {
                out[start..end].fill(1.0);
                filled = end;
            }
        }
        Ok(one(Buf::Audio(out)))
    }
}

// -------------------------------------------------------------------- seq

/// How `seq` picks the next value.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SeqMode {
    /// values[0], values[1], ... and round again.
    #[default]
    Cycle,
    /// A seeded uniform pick each event (a value may repeat).
    Random,
    /// Through the list once, then stay on the last value.
    Once,
}

/// A stepped value sequence advanced by events.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Seq {
    /// The event stream that advances the sequence (each event is one step;
    /// the event's own value is ignored).
    pub events: In,
    /// The values to step through, at least one, at most 100000.
    pub values: Vec<f32>,
    /// cycle (default), random, once.
    #[serde(default)]
    pub mode: SeqMode,
    /// Seconds to glide linearly from the old value to the new one after an
    /// event, >= 0 (default 0: jump). A new event mid-glide starts from the
    /// current value.
    #[serde(default)]
    pub glide: Option<f32>,
    /// Extra seed for `random` (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Seq {
    const BLURB: &'static str = "steps through a list of values on each event (cycle / random / once), holds between, optional glide";
    fn example() -> serde_json::Value {
        serde_json::json!({"events": "ev", "values": [100, 200, 300], "mode": "cycle", "glide": 0.0})
    }
}

impl Eval for Seq {
    fn random(&self) -> bool {
        self.mode == SeqMode::Random
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let events = cx.events("events", &self.events)?;
        if self.values.is_empty() {
            return Err("values: give at least one number".into());
        }
        if self.values.len() > MAX_LIST {
            return Err(format!(
                "values: {} numbers (limit {MAX_LIST}): give fewer",
                self.values.len()
            ));
        }
        if let Some(bad) = self.values.iter().find(|v| !v.is_finite()) {
            return Err(format!("values: {bad}: give finite numbers"));
        }
        let glide = f64::from(non_negative("glide", self.glide.unwrap_or(0.0))?);
        let sr = f64::from(cx.sr);
        let mut rng = cx.rng(self.seed);
        let len = self.values.len();
        // (start time, from, to) of the current move; values hold at `to`.
        let mut seg = (0.0f64, f64::from(self.values[0]), f64::from(self.values[0]));
        let at = |seg: &(f64, f64, f64), t: f64| {
            if glide > 0.0 && t < seg.0 + glide {
                seg.1 + (seg.2 - seg.1) * ((t - seg.0) / glide).max(0.0)
            } else {
                seg.2
            }
        };
        let mut k = 0;
        Ok(one(mono(cx.n, |i| {
            let t = i as f64 / sr;
            while k < events.len() && events[k].t * sr <= i as f64 + EPS {
                let e = &events[k];
                let pick = match self.mode {
                    SeqMode::Cycle => k % len,
                    SeqMode::Once => k.min(len - 1),
                    SeqMode::Random => (rng.next_u64() % len as u64) as usize,
                };
                let from = at(&seg, e.t);
                seg = (e.t, from, f64::from(self.values[pick]));
                k += 1;
            }
            at(&seg, t) as f32
        })))
    }
}

// ------------------------------------------------------------------ notes

/// A pitch in `notes`: a number of semitones from `root`, or a note name
/// (`C4`, `D#5`, `Bb3`), or `rest`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Pitch {
    Semitones(f32),
    Name(String),
}

/// A melody: pitches and lengths in beats, played as a held pitch, a gate
/// and a note-on event stream.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    /// `[[pitch, beats], ...]`, at most 100000. A pitch is a note name
    /// (`C4 D#5 Bb3`, A4 = `root`; octave -1..9), a number of semitones
    /// from `root` (0 = `root`), or `"rest"` (gate low, pitch held).
    /// `beats` > 0. The melody starts at 0 s and plays once.
    pub notes: Vec<(Pitch, f32)>,
    /// Tempo, beats per minute, 1..2000 (default 120).
    #[serde(default)]
    pub bpm: Option<f32>,
    /// Hz of A4 and of semitone 0, > 0 (default 440).
    #[serde(default)]
    pub root: Option<f32>,
    /// Seconds to glide (exponentially in Hz, so linear in semitones) from
    /// the previous pitch to the next, >= 0 (default 0: jump).
    #[serde(default)]
    pub glide: Option<f32>,
    /// Fraction of each note's length the gate stays high, 0 < gate <= 1
    /// (default 0.9). 1 keeps the gate high across consecutive notes.
    #[serde(default)]
    pub gate: Option<f32>,
}

impl Describe for Notes {
    const BLURB: &'static str = "a melody [[\"A4\", beats], ...]: ports out (Hz, held), gate (0 / 1) and events (note-on, value = Hz)";
    const WORDS: &'static [&'static str] = &["notes"];
    fn example() -> serde_json::Value {
        serde_json::json!({"notes": [["A4", 1], ["C5", 1], ["rest", 0.5], ["E5", 1.5]], "bpm": 120, "root": 440, "glide": 0.0, "gate": 0.9})
    }
}

/// Semitones from A4 of a note name, or `None` for a rest.
fn parse_name(idx: usize, s: &str) -> Result<Option<f64>, String> {
    let bad = |why: &str| {
        format!(
            "notes[{idx}]: `{s}`: {why}; give a note name like C4, D#5, Bb3, a number of semitones, or \"rest\""
        )
    };
    let t = s.trim();
    if t.eq_ignore_ascii_case("rest") || t.eq_ignore_ascii_case("r") {
        return Ok(None);
    }
    let mut chars = t.chars();
    let letter = chars.next().ok_or_else(|| bad("empty"))?;
    let class = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return Err(bad("the letter must be A to G")),
    };
    let rest = chars.as_str();
    let (accidental, octave) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let oct: i32 = octave
        .parse()
        .map_err(|_| bad("the octave must be a whole number"))?;
    if !(-1..=9).contains(&oct) {
        return Err(bad("the octave must be from -1 to 9"));
    }
    Ok(Some(f64::from(12 * (oct - 4) + class - 9 + accidental)))
}

impl Eval for Notes {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if self.notes.is_empty() {
            return Err("notes: give at least one [pitch, beats]".into());
        }
        if self.notes.len() > MAX_LIST {
            return Err(format!(
                "notes: {} notes (limit {MAX_LIST}): give fewer",
                self.notes.len()
            ));
        }
        let bpm = self.bpm.unwrap_or(120.0);
        if !(bpm.is_finite() && (1.0..=2000.0).contains(&bpm)) {
            return Err(format!("bpm: {bpm}: give a number from 1 to 2000"));
        }
        let root = f64::from(positive("root", self.root.unwrap_or(440.0))?);
        let glide = f64::from(non_negative("glide", self.glide.unwrap_or(0.0))?);
        let gate = self.gate.unwrap_or(0.9);
        if !(gate.is_finite() && gate > 0.0 && gate <= 1.0) {
            return Err(format!("gate: {gate}: give a number above 0, up to 1"));
        }
        let sr = f64::from(cx.sr);
        let beat = 60.0 / f64::from(bpm);
        // (start s, length s, Hz or None for a rest)
        let mut plan: Vec<(f64, f64, Option<f64>)> = Vec::new();
        let mut t = 0.0f64;
        for (idx, (pitch, beats)) in self.notes.iter().enumerate() {
            if !(beats.is_finite() && *beats > 0.0) {
                return Err(format!("notes[{idx}]: beats: {beats}: give a number > 0"));
            }
            let semis = match pitch {
                Pitch::Semitones(s) if s.is_finite() => Some(f64::from(*s)),
                Pitch::Semitones(s) => {
                    return Err(format!(
                        "notes[{idx}]: {s}: give a finite number of semitones"
                    ));
                }
                Pitch::Name(n) => parse_name(idx, n)?,
            };
            let hz = semis.map(|s| root * (s / 12.0).exp2());
            if let Some(f) = hz
                && !(f > 0.0 && f < sr / 2.0)
            {
                return Err(format!(
                    "notes[{idx}]: gives {f:.3} Hz, outside 0 to {} Hz (half the sample rate): lower the pitch or `root`",
                    sr / 2.0
                ));
            }
            let len = f64::from(*beats) * beat;
            plan.push((t, len, hz));
            t += len;
        }
        let Some(first) = plan.iter().find_map(|p| p.2) else {
            return Err(
                "notes: only rests: give at least one pitched note (a name or semitones)".into(),
            );
        };
        let mut pitch = vec![0.0f32; cx.n];
        let mut gate_buf = vec![0.0f32; cx.n];
        let mut events = Vec::new();
        let mut seg = (0.0f64, first, first);
        let mut ids = 0u32;
        let index = |t: f64| ((t * sr).round().max(0.0) as usize).min(cx.n);
        let hz_at = |seg: &(f64, f64, f64), t: f64| {
            if glide > 0.0 && t < seg.0 + glide {
                let x = ((t - seg.0) / glide).max(0.0);
                seg.1 * (seg.2 / seg.1).powf(x)
            } else {
                seg.2
            }
        };
        // Pitch changes happen at note starts; fill between them.
        let mut changes: Vec<(usize, f64, Option<f64>)> = Vec::new();
        for (start, len, hz) in &plan {
            let s = index(*start);
            if s >= cx.n {
                break;
            }
            changes.push((s, *start, *hz));
            let g_end = index(start + len * f64::from(gate));
            if let Some(f) = hz {
                gate_buf[s..g_end.max(s)].fill(1.0);
                events.push(Event {
                    t: *start,
                    value: *f as f32,
                    id: ids,
                });
                ids += 1;
            }
        }
        for (ci, (s, st, hz)) in changes.iter().enumerate() {
            if let Some(f) = hz {
                seg = (*st, hz_at(&seg, *st), *f);
            }
            let end = changes.get(ci + 1).map_or(cx.n, |c| c.0);
            for (i, p) in pitch.iter_mut().enumerate().take(end).skip(*s) {
                *p = hz_at(&seg, i as f64 / sr) as f32;
            }
        }
        let mut stream = events;
        stream.retain(|e| e.t < cx.n as f64 / sr);
        Ok(vec![
            ("out", Buf::Audio(pitch)),
            ("gate", Buf::Audio(gate_buf)),
            ("events", Buf::Events(stream)),
        ])
    }
}
