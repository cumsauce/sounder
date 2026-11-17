//! Graph nodes: one strict serde struct per type (unknown fields are
//! rejected) and its kernel. Every numeric field that makes sense as a signal
//! is an [`In`]: a number, or another node's output by name (`"sweep"` or
//! `"sweep.port"`), so any oscillator or filter can be modulated at audio
//! rate. `"$param"` and `"=expr"` are resolved to numbers before parsing.
//!
//! The `node_types!` macro, `de`, `with_schema`, `refs`, `example` and
//! `describe` follow modeler's `src/material/node.rs`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::buf::{Buf, Event, Sig};
use crate::dsp::Rng;
use crate::nodes::*;
use crate::params::Params;

/// An input: a number, a constant stereo pair `[l, r]`, or another node's
/// output by name (`"env"` or `"env.port"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum In {
    Num(f32),
    Vec(Vec<f32>),
    Name(String),
}

impl In {
    pub fn num(x: f32) -> In {
        In::Num(x)
    }
}

/// A node's outputs: (port, buffer), the first being the default port.
pub type Outs = Vec<(&'static str, Buf)>;

/// What a kernel sees.
pub struct Cx<'a> {
    /// Sample rate, Hz.
    pub sr: f32,
    /// Samples every audio output must have.
    pub n: usize,
    /// This node's seed: the document seed mixed with the node's name (the
    /// node mixes in its own `seed` field with [`Cx::rng`]).
    pub seed: u64,
    /// The document is a loop (the output stage skips its default fades).
    pub looped: bool,
    /// Where relative file paths (`sample`, `convolve`) resolve.
    pub base_dir: Option<&'a Path>,
    /// The document's params.
    pub params: &'a Params,
    /// How `sound` nodes reach other documents.
    pub sounds: Option<&'a crate::instance::SoundSource>,
    /// The documents being rendered, outermost first.
    pub stack: &'a [String],
    pub get: &'a (dyn Fn(&In) -> Result<Sig, String> + Sync),
}

impl Cx<'_> {
    /// Any audio input (mono or stereo).
    pub fn audio(&self, field: &str, i: &In) -> Result<Sig, String> {
        let s = (self.get)(i).map_err(|e| format!("{field}: {e}"))?;
        if let Sig::Buf(b) = &s
            && let Buf::Events(_) = **b
        {
            return Err(format!(
                "{field}: `{}` is an event stream, not audio: render events through `voices`",
                name_of(i)
            ));
        }
        Ok(s)
    }

    /// A mono input (a modulation signal or a mono-only node's input).
    pub fn mono(&self, field: &str, i: &In) -> Result<Sig, String> {
        let s = self.audio(field, i)?;
        if s.is_stereo() {
            return Err(format!(
                "{field}: `{}` is stereo but this field is mono: add a mix node with \"to\": \"mono\" in between",
                name_of(i)
            ));
        }
        Ok(s)
    }

    /// An event-stream input: its events, sorted by time, ids 0..n.
    pub fn events(&self, field: &str, i: &In) -> Result<Vec<Event>, String> {
        match (self.get)(i).map_err(|e| format!("{field}: {e}"))? {
            Sig::Buf(b) => match &*b {
                Buf::Events(v) => Ok(v.clone()),
                _ => Err(format!(
                    "{field}: `{}` is audio, not events: use on_threshold to make events from a signal",
                    name_of(i)
                )),
            },
            _ => Err(format!(
                "{field}: `{}` is a number, not events: use an event node (once, pattern, poisson, bounce, burst, on_threshold)",
                name_of(i)
            )),
        }
    }

    /// An optional mono input with a default.
    pub fn mono_or(&self, field: &str, i: &Option<In>, default: f32) -> Result<Sig, String> {
        match i {
            Some(i) => self.mono(field, i),
            None => Ok(Sig::Const(default)),
        }
    }

    /// The node's random generator: the document seed, the node's name and
    /// the node's own `seed` field mixed together.
    pub fn rng(&self, own: u64) -> Rng {
        Rng::new(crate::mix_seed(self.seed, own))
    }

    /// A file path as the node wrote it, resolved against the project.
    pub fn path(&self, file: &str) -> PathBuf {
        let p = Path::new(file);
        match self.base_dir {
            Some(b) if p.is_relative() => b.join(p),
            _ => p.to_path_buf(),
        }
    }

    /// Seconds to a sample index (clamped to the buffer).
    pub fn at(&self, t: f32) -> usize {
        ((t.max(0.0) * self.sr).round() as usize).min(self.n)
    }
}

fn name_of(i: &In) -> String {
    match i {
        In::Name(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// A kernel.
pub trait Eval {
    fn eval(&self, cx: &Cx) -> Result<Outs, String>;
    /// Could a live (sample-by-sample, no lookahead) runtime run this node?
    fn causal(&self) -> bool {
        true
    }
    /// Does the node read the seed (so its cache key must include it)?
    fn random(&self) -> bool {
        false
    }
}

macro_rules! node_types {
    ($( $ty:literal => $var:ident ),* $(,)?) => {
        /// One parsed node.
        #[derive(Clone, Debug)]
        #[allow(clippy::large_enum_variant)]
        pub enum Node { $($var($var)),* }

        /// Every node type, as written in `"type"`.
        pub const NODE_TYPES: &[&str] = &[$($ty),*];

        /// Parses a node body (without `type`/`name`) of type `ty`.
        pub fn parse_body(ty: &str, body: &Value, ctx: &str) -> Result<Node, String> {
            match ty {
                $($ty => Ok(Node::$var(de(ctx, body).map_err(|e| with_schema(e, ty))?)),)*
                other => Err(unknown_type(ctx, other)),
            }
        }

        /// The JSON schema of a node type's fields.
        pub fn schema(ty: &str) -> Option<schemars::Schema> {
            match ty {
                $($ty => Some(schemars::schema_for!($var)),)*
                _ => None,
            }
        }

        impl Node {
            pub fn eval(&self, cx: &Cx) -> Result<Outs, String> {
                match self { $(Node::$var(n) => n.eval(cx)),* }
            }
            pub fn causal(&self) -> bool {
                match self { $(Node::$var(n) => n.causal()),* }
            }
            pub fn random(&self) -> bool {
                match self { $(Node::$var(n) => n.random()),* }
            }
        }
    };
}

node_types! {
    // sources
    "osc" => Osc,
    "noise" => Noise,
    "impulse" => Impulse,
    "sample" => Sample,
    // envelopes
    "env" => Env,
    "adsr" => Adsr,
    "decay" => Decay,
    "follower" => Follower,
    // filters
    "filter" => Filter,
    "onepole" => Onepole,
    "comb" => Comb,
    "allpass" => Allpass,
    "resonators" => Resonators,
    // time
    "delay" => Delay,
    "reverb" => Reverb,
    "convolve" => Convolve,
    // nonlinear
    "shape" => Shape,
    "compress" => Compress,
    "limit" => Limit,
    // control
    "lfo" => Lfo,
    "wander" => Wander,
    "sample_hold" => SampleHold,
    "slew" => Slew,
    "ramp" => Ramp,
    // maths
    "math" => Math,
    "remap" => Remap,
    "gain" => Gain,
    // mix
    "mix" => Mix,
    "pan" => Pan,
    "width" => Width,
    // offline
    "reverse" => Reverse,
    "normalize" => Normalize,
    "trim" => Trim,
    "fade" => Fade,
    // output
    "output" => Output,
    // phase 3: spectral
    "pitch_shift" => PitchShift,
    "time_stretch" => TimeStretch,
    "spectral" => Spectral,
    // phase 3: voices
    "voices" => Voices,
    "sound" => SoundNode,
    // phase 3: events
    "once" => Once,
    "pattern" => Pattern,
    "poisson" => Poisson,
    "bounce" => Bounce,
    "burst" => Burst,
    "on_threshold" => OnThreshold,
    "grains" => Grains,
    // phase 3: expr
    "expr" => Expr,
    // phase 4: modal
    "modal" => Modal,
    "contact" => Contact,
    "friction" => Friction,
    // phase 4: foley
    "bubble" => BubbleNode,
    "air" => Air,
    "glottis" => Glottis,
    "tract" => Tract,
    "syrinx" => Syrinx,
    "thunder" => Thunder,
}

/// "unknown node type", naming the closest types.
fn unknown_type(ctx: &str, ty: &str) -> String {
    let mut near: Vec<(usize, &str)> = NODE_TYPES
        .iter()
        .map(|t| (edit_distance(ty, t), *t))
        .filter(|(d, t)| *d <= 2.max(t.len() / 3))
        .collect();
    near.sort();
    let hint = match near.first() {
        Some((_, t)) => format!("did you mean `{t}`? "),
        None => String::new(),
    };
    format!(
        "{ctx}: unknown node type `{ty}`: {hint}(types: {}; describe_op lists them with examples)",
        NODE_TYPES.join(", ")
    )
}

/// Levenshtein distance (as modeler's `spec.rs`).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != *cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Appends the node type's field list (name: type, from its schema) to a
/// field error, so a wrong field shows what the node takes.
fn with_schema(e: String, ty: &str) -> String {
    let Some(schema) = schema(ty) else { return e };
    let v = schema.as_value();
    let Some(props) = v.get("properties").and_then(Value::as_object) else {
        return e;
    };
    let required: Vec<&str> = v
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let fields: Vec<String> = props
        .keys()
        .map(|k| {
            if required.contains(&k.as_str()) {
                k.clone()
            } else {
                format!("[{k}]")
            }
        })
        .collect();
    format!(
        "{e}\n  `{ty}` fields: {} (describe_op \"{ty}\" for the schema)",
        fields.join(", ")
    )
}

fn de<T: serde::de::DeserializeOwned>(ctx: &str, body: &Value) -> Result<T, String> {
    serde_path_to_error::deserialize(body).map_err(|e| {
        let path = e.path().to_string();
        if path == "." {
            format!("{ctx}: {}", e.inner())
        } else {
            format!("{ctx}: field `{path}`: {}", e.inner())
        }
    })
}

/// True for a string that names a file rather than a node.
pub fn is_file(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.ends_with(".wav") || l.ends_with(".wave")
}

/// Node references in a node body: every string that is not a mode word or
/// a file, as (node, port).
pub fn refs(body: &Value) -> Vec<(String, String)> {
    const WORDS: &[&str] = &[
        "type", "name", "op", "mode", "shape", "curve", "color", "file", "window", "sound",
        // phase 4: modal (a material name is a word, not a node)
        "material",
        "vowel", "vowel_to", "species",
    ];
    let mut out = Vec::new();
    // phase 3: voices. The `patch` is its own graph and `map` is expression
    // text: only `events` reads another node.
    if body.get("patch").is_some_and(Value::is_array) {
        if let Some(e) = body.get("events") {
            walk(e, Some("events"), &mut out);
        }
        return out;
    }
    // phase 3: expr. Its `out` / `update` / `state` are expression text, not
    // references: only the `inputs` values read other nodes. (No other node
    // has a string `out` without an `input`; `fade`'s `out` is a number.)
    if body.get("out").is_some_and(Value::is_string) && body.get("input").is_none() {
        if let Some(Value::Object(inputs)) = body.get("inputs") {
            for v in inputs.values() {
                walk(v, Some("inputs"), &mut out);
            }
        }
        return out;
    }
    fn walk(v: &Value, key: Option<&str>, out: &mut Vec<(String, String)>) {
        match v {
            Value::String(s) => {
                if key.is_some_and(|k| WORDS.contains(&k))
                    || (key == Some("to") && (s == "mono" || s == "stereo"))
                    || is_file(s)
                {
                    return;
                }
                let (n, p) = s.split_once('.').unwrap_or((s, ""));
                out.push((n.to_string(), p.to_string()));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
            Value::Object(m) => m.iter().for_each(|(k, x)| walk(x, Some(k), out)),
            _ => {}
        }
    }
    walk(body, None, &mut out);
    out
}

/// One line per type: what the node is for.
fn blurb(ty: &str) -> &'static str {
    match ty {
        "osc" => {
            "band-limited oscillator (sine/saw/square/tri/pulse); freq may be a node (pitch sweeps, FM)"
        }
        "noise" => "white/pink/brown/velvet noise (seeded)",
        "impulse" => "single-sample clicks at the given times (excites resonators, comb, reverb)",
        "sample" => "a WAV file from the project, at a playback rate",
        "env" => "breakpoint envelope [[t, v], ...]",
        "adsr" => "attack/decay/sustain/release; held for hold seconds, or by a gate signal",
        "decay" => {
            "exponential fall from 1 (time = seconds to -60 dB): the workhorse amplitude envelope"
        }
        "follower" => "envelope follower: the input's level over time",
        "filter" => {
            "state-variable filter lp/hp/bp/notch/peak/lowshelf/highshelf; freq and q take signals"
        }
        "onepole" => "gentle 6 dB/oct low- or high-pass",
        "comb" => "feedback comb (resonant pitch at freq, or echo at time)",
        "allpass" => "Schroeder allpass (diffusion, smearing transients)",
        "resonators" => "bank of ringing modes [[hz, decay_s, gain], ...] (struck objects)",
        "delay" => "interpolated delay with optional internal feedback; time may be a signal",
        "reverb" => "8-line feedback delay network; stereo out",
        "convolve" => "convolution with an impulse response (a WAV file or a node)",
        "shape" => "waveshaper tanh/clip/fold/bitcrush/rectify",
        "compress" => "feed-forward compressor",
        "limit" => "lookahead peak limiter",
        "lfo" => "low-frequency oscillator, -1..1 or a range",
        "wander" => "smooth random motion at a rate (seeded), -1..1 or a range",
        "sample_hold" => "samples the input at a rate or on a trigger and holds it",
        "slew" => "limits how fast a signal may rise and fall",
        "ramp" => "a sweep from one value to another over time (pitch drops, filter sweeps)",
        "math" => "add/sub/mul/div/min/max/pow/abs/mix of signals or numbers",
        "remap" => "maps a range onto another",
        "gain" => "scales by db or a linear gain (either may be a signal)",
        "mix" => "sums inputs with gain, pan and time offset; mono or stereo",
        "pan" => "places mono in stereo (or balances stereo)",
        "width" => "stereo width (0 = mono, 1 = as is, 2 = wider)",
        "reverse" => "plays the input backwards",
        "normalize" => "scales to a peak or a loudness",
        "trim" => "removes leading silence (the end is padded with silence)",
        "fade" => "raised-cosine fade in and out",
        "output" => {
            "the sound: gain, loudness target, edge fades and a ceiling limiter; exactly one"
        }
        // phase 3: spectral
        "pitch_shift" => "phase-vocoder pitch shift by semitones or ratio, same length (acausal)",
        "time_stretch" => {
            "phase-vocoder time stretch (factor > 1 = slower), cut or padded to the document length (acausal)"
        }
        "spectral" => {
            "spectrum edits: freeze (hold a moment as texture), blur, gate (zero quiet bins), morph (toward another node) (acausal)"
        }
        // phase 3: voices
        "voices" => {
            "renders a sub-patch once per event (rain, gravel, shards, gunfire), summed at the event times"
        }
        "sound" => {
            "an instance of another sound (open or saved): layer a transient, a body and a tail from separate documents"
        }
        // phase 3: events
        "once" => "an event stream with one event at a time",
        "pattern" => "events at listed times, or every N seconds (optional jitter)",
        "poisson" => "random events at a rate per second (the rate may be a signal)",
        "bounce" => "events of a dropped object: intervals shrink by the restitution",
        "burst" => "a cluster of events over a short span",
        "on_threshold" => "an event at each rising crossing of a level by an audio signal",
        "grains" => "granular playback of a node or WAV, one windowed grain per event",
        // phase 3: expr
        "expr" => {
            "per-sample maths with state: out/update text over t, sr, inputs, state (sin tanh min clamp noise, ?:, ...)"
        }
        // phase 4: modal
        "modal" => {
            "a struck plate: excite audio rings modes computed from material, size and strike position (bigger = lower)"
        }
        "contact" => {
            "the force pulse of a strike (Hertz contact time: heavier, slower, softer = longer, duller); excites modal"
        }
        "friction" => {
            "stick-slip sawtooth whose rate follows speed and level follows pressure (creaks, squeaks, scrapes); excites modal"
        // phase 4: foley
        "bubble" => {
            "a water bubble: Minnaert resonance from radius_mm (smaller = higher), decaying, rising (foley)"
        }
        "air" => {
            "distance through air: high-frequency absorption (ISO 9613-1) and 1/distance level; distance may be a signal"
        }
        "glottis" => {
            "voice source: Liljencrants-Fant glottal pulse at f0 (signal), tension, breath, vibrato (foley)"
        }
        "tract" => {
            "vocal tract: formant cascade, a vowel preset (or glide between two) or a formant list; feed it a glottis"
        }
        "syrinx" => {
            "a bird: foley's call for one of 57 species, or a whistle from a pitch contour signal"
        }
        "thunder" => {
            "one flash of thunder from its distance (foley's N-wave model): clap, then rumble; silent until the sound arrives"
        }
        _ => "",
    }
}

/// An example node of each type (with `type` and `name`), used by
/// `describe_op` and checked to parse by a test.
pub fn example(ty: &str) -> Option<Value> {
    use serde_json::json;
    let body = match ty {
        "osc" => json!({"shape": "saw", "freq": "sweep", "amp": 0.5}),
        "noise" => json!({"color": "pink", "seed": 3}),
        "impulse" => json!({"at": [0.0, 0.25]}),
        "sample" => json!({"file": "refs/knock.wav", "rate": 1.2, "start": 0.0}),
        "env" => json!({"points": [[0, 0], [0.01, 1], [0.3, 0.2], [1.0, 0]], "curve": "smooth"}),
        "adsr" => {
            json!({"attack": 0.005, "decay": 0.1, "sustain": 0.4, "release": 0.3, "hold": 0.2})
        }
        "decay" => json!({"time": 0.4}),
        "follower" => json!({"input": "drums", "attack": 0.002, "release": 0.08}),
        "filter" => json!({"input": "src", "mode": "lp", "freq": "cutoff_env", "q": 2.0}),
        "onepole" => json!({"input": "src", "mode": "hp", "freq": 120}),
        "comb" => json!({"input": "burst", "freq": 220, "feedback": 0.9, "damp": 0.3}),
        "allpass" => json!({"input": "src", "time": 0.005, "feedback": 0.6}),
        "resonators" => {
            json!({"input": "hit", "modes": [[523, 1.2, 1.0], [1410, 0.6, 0.5], [2730, 0.3, 0.3]]})
        }
        "delay" => json!({"input": "src", "time": 0.25, "feedback": 0.4}),
        "reverb" => json!({"input": "src", "size": 0.6, "decay": 1.8, "damp": 0.4, "mix": 0.3}),
        "convolve" => json!({"input": "src", "ir": "refs/room_ir.wav", "mix": 0.5}),
        "shape" => json!({"input": "src", "mode": "tanh", "drive": 3.0}),
        "compress" => {
            json!({"input": "src", "threshold_db": -18, "ratio": 4, "attack": 0.005, "release": 0.1})
        }
        "limit" => json!({"input": "src", "ceiling_db": -1}),
        "lfo" => json!({"shape": "sine", "freq": 6, "range": [0.8, 1.2]}),
        "wander" => json!({"rate": 3, "range": [200, 800], "seed": 2}),
        "sample_hold" => json!({"input": "noise1", "rate": 20}),
        "slew" => json!({"input": "steps", "up": 0.01, "down": 0.05}),
        "ramp" => json!({"from": 2000, "to": 200, "time": 0.3, "curve": "exp"}),
        "math" => json!({"op": "mul", "a": "tone", "b": "amp_env"}),
        "remap" => json!({"input": "lfo1", "from": [-1, 1], "to": [400, 1200], "clamp": true}),
        "gain" => json!({"input": "src", "db": -6}),
        "mix" => {
            json!({"inputs": ["body", {"input": "click", "gain": 0.5, "pan": -0.3, "at": 0.01}], "to": "stereo"})
        }
        "pan" => json!({"input": "src", "pan": "lfo1"}),
        "width" => json!({"input": "verb", "amount": 1.5}),
        "reverse" => json!({"input": "verb"}),
        "normalize" => json!({"input": "src", "peak_db": -1}),
        "trim" => json!({"input": "src", "threshold_db": -60}),
        "fade" => json!({"input": "src", "in": 0.01, "out": 0.2}),
        "output" => json!({"input": "mix1", "lufs": -16}),
        // phase 3: spectral
        "pitch_shift" => json!({"input": "src", "semitones": -5}),
        "time_stretch" => json!({"input": "src", "factor": 1.5}),
        "spectral" => json!({"input": "src", "mode": "freeze", "at": 0.1, "length": 0.15}),
        // phase 3: voices
        "voices" => json!({"events": "ev", "length": 0.3, "map": {"pitch": "300 + $rand * 600"},
            "patch": [{"type": "osc", "name": "tone", "freq": "$pitch", "amp": "$value"},
                      {"type": "decay", "name": "amp", "time": 0.08},
                      {"type": "math", "name": "hit", "op": "mul", "a": "tone", "b": "amp"}]}),
        "sound" => json!({"sound": "thud", "at": 0.05, "gain_db": -6}),
        // phase 3: events
        "once" => json!({"at": 0.1}),
        "pattern" => json!({"every": 0.25, "count": 4, "jitter": 0.01}),
        "poisson" => json!({"rate": 20, "values": {"min": 0.3, "max": 1.0}}),
        "bounce" => json!({"first": 0.3, "restitution": 0.6, "min_interval": 0.004}),
        "burst" => json!({"count": 6, "spread": 0.05, "curve": "exp"}),
        "on_threshold" => json!({"input": "src", "level": 0.2, "hold": 0.01}),
        "grains" => json!({"source": "noise1", "events": "ev", "size": 0.03, "pan_spread": 0.5}),
        // phase 3: expr
        "expr" => json!({"inputs": {"a": "src"}, "state": {"y1": 0}, "out": "y1",
            "update": {"y1": "y1 + 0.1 * (a - y1)"}}),
        // phase 4: modal
        "modal" => json!({"excite": "contact1", "material": "wood", "size": 0.5,
            "strike_position": 0.3, "modes": 48}),
        "contact" => {
            json!({"speed": 1.5, "mass": 0.2, "material": "wood", "hardness": 0.7, "at": 0.0})
        }
        "friction" => json!({"speed": "sweep", "pressure": 0.5, "roughness": 0.2, "grain": 200}),
        // phase 4: foley
        "bubble" => json!({"radius_mm": 2.0, "at": 0.02, "seed": 1}),
        "air" => json!({"input": "src", "distance": 80, "humidity": 50}),
        "glottis" => {
            json!({"f0": 140, "tension": 0.65, "breath": 0.05, "vibrato_cents": 20, "seed": 1})
        }
        "tract" => json!({"input": "src", "vowel": "a", "vowel_to": "i", "morph": 0.5}),
        "syrinx" => json!({"species": "robin", "at": 0.0, "seed": 1}),
        "thunder" => json!({"distance": 400, "exposure": 0.8, "seed": 1}),
        _ => return None,
    };
    let mut m = serde_json::Map::new();
    m.insert("type".into(), json!(ty));
    m.insert("name".into(), json!(format!("{ty}1")));
    if let Value::Object(b) = body {
        m.extend(b);
    }
    Some(Value::Object(m))
}

/// `describe_op()` (every type, one line) / `describe_op(kind)` (example +
/// schema).
pub fn describe(ty: Option<&str>) -> Result<String, String> {
    let Some(ty) = ty else {
        let mut out = vec![
            "sound_ops node types (describe_op \"<type>\" for the schema). Signal fields take a number, \"$param\", \"=expr\" or a node (\"name\" / \"name.port\"). Time in seconds, frequency in Hz, *_db in dB."
                .to_string(),
        ];
        for t in NODE_TYPES {
            out.push(format!("- {t}: {}", blurb(t)));
        }
        return Ok(out.join("\n"));
    };
    let Some(s) = schema(ty) else {
        return Err(unknown_type("describe_op", ty));
    };
    Ok(format!(
        "{ty}: {}\n\nexample:\n{}\n\nschema (plus \"type\": \"{ty}\", \"name\"):\n{}",
        blurb(ty),
        serde_json::to_string_pretty(&example(ty).unwrap_or(Value::Null)).unwrap_or_default(),
        serde_json::to_string_pretty(&s).unwrap_or_default()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_type_has_a_parseable_example() {
        for t in NODE_TYPES {
            let Some(Value::Object(mut m)) = example(t) else {
                panic!("no example for {t}");
            };
            m.remove("type");
            m.remove("name");
            parse_body(t, &Value::Object(m), t).unwrap_or_else(|e| panic!("{e}"));
            assert!(describe(Some(t)).is_ok());
            assert!(!blurb(t).is_empty(), "{t} has no blurb");
        }
        assert!(describe(None).is_ok_and(|s| s.contains("resonators")));
    }

    #[test]
    fn unknown_types_suggest_the_closest() {
        let e = unknown_type("node #0", "fliter");
        assert!(e.contains("did you mean `filter`"), "{e}");
    }

    #[test]
    fn refs_skip_words_and_files() {
        let v = serde_json::json!({"input": "a.out", "mode": "lp", "ir": "x.wav", "to": "stereo",
            "inputs": ["b", {"input": "c"}]});
        let mut r: Vec<String> = refs(&v).into_iter().map(|(n, _)| n).collect();
        r.sort();
        assert_eq!(r, ["a", "b", "c"]);
    }
}
