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
    /// Folders searched after `base_dir` for a relative file that is not
    /// there (the project's `assets/sources/`, the library's `sources/`).
    pub sources: &'a [PathBuf],
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

    /// A file path as the node wrote it, resolved against the project and
    /// then the sources folders (see [`resolve_file`]).
    pub fn path(&self, file: &str) -> PathBuf {
        resolve_file(file, self.base_dir, self.sources)
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

/// What `describe_op` and reference scanning need to know about a node type,
/// kept beside the node's struct.
pub trait Describe {
    /// One line: what the node is for.
    const BLURB: &'static str;
    /// Field names whose string values are words, not node references.
    const WORDS: &'static [&'static str] = &[];
    /// The example body, without `type` / `name`. May only reference the
    /// stand-in names `every_node_type_renders_its_example` wires up
    /// (`crates/sfx_graph/tests/graph.rs`).
    fn example() -> serde_json::Value;
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

        /// One line per type: what the node is for (its [`Describe::BLURB`]).
        pub fn blurb(ty: &str) -> &'static str {
            match ty {
                $($ty => <$var as Describe>::BLURB,)*
                _ => "",
            }
        }

        /// The example body of a type (without `type` / `name`).
        fn example_body(ty: &str) -> Option<Value> {
            match ty {
                $($ty => Some(<$var as Describe>::example()),)*
                _ => None,
            }
        }

        /// Field names of a type whose string values are words, not node
        /// references (its [`Describe::WORDS`]).
        fn words(ty: &str) -> &'static [&'static str] {
            match ty {
                $($ty => <$var as Describe>::WORDS,)*
                _ => &[],
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
    // spectral
    "pitch_shift" => PitchShift,
    "time_stretch" => TimeStretch,
    "spectral" => Spectral,
    // voices
    "voices" => Voices,
    "sound" => SoundNode,
    // events
    "once" => Once,
    "pattern" => Pattern,
    "poisson" => Poisson,
    "bounce" => Bounce,
    "burst" => Burst,
    "on_threshold" => OnThreshold,
    "grains" => Grains,
    // expr
    "expr" => Expr,
    // modal
    "modal" => Modal,
    "contact" => Contact,
    "friction" => Friction,
    "impact" => Impact,
    "rig" => Rig,
    // foley
    "bubble" => BubbleNode,
    "air" => Air,
    "glottis" => Glottis,
    "tract" => Tract,
    "syrinx" => Syrinx,
    "thunder" => Thunder,

    "ringmod" => RingMod,


    "fm" => Fm,
    "wavetable" => Wavetable,


    "ladder" => Ladder,
    "eq" => Equalizer,

    // dynamics
    "transient" => Transient,
    "gate" => Gate,
    "sub" => Sub,
    "exciter" => Exciter,

    "chorus" => Chorus,
    "flanger" => Flanger,
    "phaser" => Phaser,
    "freq_shift" => FreqShift,

    "doppler" => Doppler,
    "haas" => Haas,
    "decorrelate" => Decorrelate,


    "chaos" => Chaos,
    "fbm" => Fbm,
    "curve" => CurveLookup,
    "switch" => Switch,
    "pitch_follow" => PitchFollow,
    "centroid_follow" => CentroidFollow,

    "trigger_env" => TriggerEnv,
    "event_gate" => EventGate,
    "seq" => Seq,
    "notes" => Notes,

    "ir" => Ir,

    "crossfade_loop" => CrossfadeLoop,
    "resample" => Resample,
    "tape" => Tape,
    "stutter" => Stutter,

    "explosion" => Explosion,
    "gunshot" => Gunshot,

    "engine" => Engine,
    "motor" => Motor,
    "clock" => Clock,
    "fan" => Fan,
    "bell" => Bell,
    "ball_bounce" => BallBounce,
    "cricket" => Cricket,
    "twang" => Twang,
    "siren" => Siren,
    "door_creak" => DoorCreak,
    "jet" => Jet,
    "red_alert" => RedAlert,
    "phone_tone" => PhoneTone,
    "two_tone" => TwoTone,
    "rain" => Rain,
    "step" => Step,
    "lava" => Lava,
    "cave" => Cave,
    "frog" => Frog,
    "creature" => Creature,
    "surf" => Surf,
    "leaves" => Leaves,
    "underwater" => Underwater,
    "roar" => Roar,
    "fly" => Fly,
    "cicada" => Cicada,
    "helicopter" => Helicopter,
    "rocket" => Rocket,
    "droid" => Droid,
    "transporter" => Transporter,
    "torpedo" => Torpedo,
    "phaser_beam" => PhaserBeam,
    "stream" => Stream,
    "pour" => Pour,

    "fire" => Fire,
    "wind" => Wind,
    "spark" => Spark,

    "water" => Water,
    "splash" => Splash,

    "tube" => Tube,
    "string" => StringNode,

    "rolling" => Rolling,
    "cloth" => Cloth,
    "footstep" => Footstep,

    "shatter" => Shatter,
    "collapse" => Collapse,
    "debris" => Debris,
    "fracture_on" => FractureOn,
    "dust" => Dust,



    "filterbank" => Filterbank,
    "vocoder" => Vocoder,
    "multiband" => Multiband,
    "finish" => Finish,
    "subdrop" => Subdrop,
    "riser" => Riser,

    "cloud" => Cloud,

    "click" => Click,

    "crumple" => Crumple,
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
/// Where a file a node names is read from: an absolute path as given; a
/// relative one against `base_dir` (the project) when it exists there, else
/// the first of `sources` that holds it (the path as written, then its file
/// name alone), else against `base_dir` (so the error names the project
/// path).
pub fn resolve_file(file: &str, base_dir: Option<&Path>, sources: &[PathBuf]) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let first = match base_dir {
        Some(b) => b.join(p),
        None => p.to_path_buf(),
    };
    if first.exists() {
        return first;
    }
    for dir in sources {
        let full = dir.join(p);
        if full.exists() {
            return full;
        }
        if let Some(name) = p.file_name() {
            let flat = dir.join(name);
            if flat.exists() {
                return flat;
            }
        }
    }
    first
}

pub fn is_file(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.ends_with(".wav") || l.ends_with(".wave")
}

/// Node references in a node body: every string that is not a mode word or
/// a file, as (node, port). If the body carries `"type"`, that type's
/// [`Describe::WORDS`] are skipped too; without it, use [`refs_typed`].
pub fn refs(body: &Value) -> Vec<(String, String)> {
    match body.get("type").and_then(Value::as_str) {
        Some(ty) => refs_typed(ty, body),
        None => refs_typed("", body),
    }
}

/// [`refs`] for a body whose `type` was already taken out of it: the global
/// word list plus the type's own [`Describe::WORDS`].
pub fn refs_typed(ty: &str, body: &Value) -> Vec<(String, String)> {
    refs_with(words(ty), body)
}

fn refs_with(own: &[&str], body: &Value) -> Vec<(String, String)> {
    const WORDS: &[&str] = &[
        "type", "name", "op", "mode", "shape", "curve", "color", "file", "window", "sound",
        // modal (a material name is a word, not a node)
        "material", "vowel", "vowel_to", "species",
        // enum-string fields every node may use
        "model", "preset", "style",
    ];
    let mut out = Vec::new();
    // voices. The `patch` is its own graph and `map` is expression
    // text: only `events` reads another node.
    if body.get("patch").is_some_and(Value::is_array) {
        if let Some(e) = body.get("events") {
            walk(e, Some("events"), own, &mut out);
        }
        return out;
    }
    // expr. Its `out` / `update` / `state` are expression text, not
    // references: only the `inputs` values read other nodes. (No other node
    // has a string `out` without an `input`; `fade`'s `out` is a number.)
    if body.get("out").is_some_and(Value::is_string) && body.get("input").is_none() {
        if let Some(Value::Object(inputs)) = body.get("inputs") {
            for v in inputs.values() {
                walk(v, Some("inputs"), own, &mut out);
            }
        }
        return out;
    }
    fn walk(v: &Value, key: Option<&str>, own: &[&str], out: &mut Vec<(String, String)>) {
        match v {
            Value::String(s) => {
                if key.is_some_and(|k| WORDS.contains(&k) || own.contains(&k))
                    || (key == Some("to") && (s == "mono" || s == "stereo"))
                    || is_file(s)
                    || crate::params::is_expr(s)
                {
                    return;
                }
                let (n, p) = s.split_once('.').unwrap_or((s, ""));
                out.push((n.to_string(), p.to_string()));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, key, own, out)),
            Value::Object(m) => m.iter().for_each(|(k, x)| walk(x, Some(k), own, out)),
            _ => {}
        }
    }
    walk(body, None, own, &mut out);
    out
}

/// An example node of each type (with `type` and `name`), used by
/// `describe_op` and checked to parse by a test.
pub fn example(ty: &str) -> Option<Value> {
    use serde_json::json;
    let body = example_body(ty)?;
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
    fn describe_trait_feeds_blurb_example_and_words() {
        struct Fake;
        impl Describe for Fake {
            const BLURB: &'static str = "a fake";
            const WORDS: &'static [&'static str] = &["flavour"];
            fn example() -> Value {
                serde_json::json!({})
            }
        }
        assert_eq!(<Fake as Describe>::WORDS, ["flavour"]);
        // Defaults: no words; every real type's words come from its impl.
        assert!(words("noise").is_empty());
        assert_eq!(words("osc"), ["wave"]);
        assert!(words("nope").is_empty());
        assert_eq!(blurb("noise"), <Noise as Describe>::BLURB);
        assert!(blurb("nope").is_empty());
        assert!(example("nope").is_none());
    }

    #[test]
    fn global_words_include_model_preset_style() {
        let v = serde_json::json!({"model": "a", "preset": "b", "style": "c", "input": "d"});
        let r: Vec<String> = refs(&v).into_iter().map(|(n, _)| n).collect();
        assert_eq!(r, ["d"]);
    }

    #[test]
    fn a_types_own_words_are_not_references() {
        let v = serde_json::json!({"flavour": "sweet", "input": "a", "list": [{"flavour": "x"}]});
        let names = |own: &[&str]| -> Vec<String> {
            refs_with(own, &v).into_iter().map(|(n, _)| n).collect()
        };
        let mut all = names(&[]);
        all.sort();
        assert_eq!(all, ["a", "sweet", "x"]);
        assert_eq!(names(&["flavour"]), ["a"]);
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
