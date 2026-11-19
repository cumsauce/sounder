//! Deterministic checks on a rendered sound (`verify` and `sound_ops`
//! `expect`): each prints PASS / FAIL with the measured value, and a FAIL
//! says what to change. Modeled on modeler's `MatExpect` + `MatDoc::check`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sfx_graph::{Kind, RenderOpts, Rendered, SoundDoc, db_to_gain, gain_to_db};

use crate::descriptors::{self, BAND_LABELS, BANDS, Descriptors, SEGMENTS};
use crate::stft;

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Max {
    pub max: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
}

/// The `onsets` check: a count in [min, max].
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OnsetsCheck {
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
    /// Events closer than this many ms are one (default 40: the spectral
    /// detector, which merges under 30-40 ms). Under 30 the count switches to
    /// a fine 1 ms detector above 1 kHz (the one behind `density`), so
    /// `spacing_ms: 5` counts a 200/s click train; over 40 merges the
    /// default onsets that are closer.
    #[serde(default)]
    pub spacing_ms: Option<f32>,
    /// `[lo, hi]` seconds: every gap between consecutive onsets (counted at
    /// the `spacing_ms` resolution) must lie in the range. "Exactly 3 hits
    /// 0.15-0.3 s apart" is `{min: 3, max: 3, gap: [0.15, 0.3]}`. Fewer than
    /// two onsets have no gaps (the count bounds catch that).
    #[serde(default)]
    pub gap: Option<[f32; 2]>,
}

impl OnsetsCheck {
    fn range(&self) -> Range {
        Range {
            min: self.min,
            max: self.max,
        }
    }
}

/// `true`, or the check's settings.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Toggle<T> {
    On(bool),
    With(T),
}

/// A check that takes `true` or settings: its name and its setting fields,
/// for the error a wrong shape gets.
pub trait ToggleSettings {
    const CHECK: &'static str;
    const FIELDS: &'static str;
}

impl<'de, T> Deserialize<'de> for Toggle<T>
where
    T: serde::de::DeserializeOwned + ToggleSettings,
{
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let v = Value::deserialize(d)?;
        let forms = || format!("{}: expected true or {{{}}}", T::CHECK, T::FIELDS);
        match v {
            Value::Bool(b) => Ok(Toggle::On(b)),
            Value::Object(_) => serde_json::from_value::<T>(v)
                .map(Toggle::With)
                .map_err(|e| D::Error::custom(format!("{}: {e}", forms()))),
            other => Err(D::Error::custom(format!("{}, got {other}", forms()))),
        }
    }
}

impl ToggleSettings for DcCheck {
    const CHECK: &'static str = "dc";
    const FIELDS: &'static str = "max";
}

impl ToggleSettings for Variations {
    const CHECK: &'static str = "variations";
    const FIELDS: &'static str = "seeds, lufs_spread, peak_spread_db, min_distance";
}

impl ToggleSettings for AliasingCheck {
    const CHECK: &'static str = "aliasing";
    const FIELDS: &'static str = "max_db";
}

impl<T: Default + Clone> Toggle<T> {
    fn get(&self) -> Option<T> {
        match self {
            Toggle::On(true) => Some(T::default()),
            Toggle::On(false) => None,
            Toggle::With(t) => Some(t.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DcCheck {
    /// Largest DC offset, dB (default -40).
    #[serde(default)]
    pub max: Option<f32>,
}

/// One cell (or a whole row) of the band table.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BandCheck {
    /// "63", "125", "250", "500", "1k", "2k", "4k", "8k" (or a number of Hz:
    /// the band holding it).
    pub band: serde_json::Value,
    /// Time segment 0..4 (fifths of the sound); omitted = the whole sound.
    #[serde(default)]
    pub segment: Option<usize>,
    #[serde(default)]
    pub min_db: Option<f32>,
    #[serde(default)]
    pub max_db: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PitchCheck {
    pub hz: f32,
    /// Tolerance, cents (default 25).
    #[serde(default)]
    pub tol_cents: Option<f32>,
}

/// The `aliasing` check.
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AliasingCheck {
    /// Largest energy off the harmonic grid above 4 kHz, relative to the
    /// total, dB (default -40).
    #[serde(default)]
    pub max_db: Option<f32>,
}

/// The `modulation` check: the sound has a rate near `rate_hz`.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModulationCheck {
    /// The rate wanted, Hz (0.5 to 200 for amplitude; pitch up to about 40).
    pub rate_hz: f32,
    /// Accepted distance from it, Hz (default 0.5).
    #[serde(default)]
    pub tol_hz: Option<f32>,
    /// Smallest peak-to-peak depth, dB for amplitude, cents for pitch
    /// (default 1 dB / 10 cents).
    #[serde(default)]
    pub min_depth: Option<f32>,
    /// "amplitude" (default) or "pitch".
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variations {
    /// Seeds 1..=N rendered (default 6).
    #[serde(default)]
    pub seeds: Option<u64>,
    /// Largest loudness spread across them, LU (default 1). Applies when the
    /// output normalizes loudness (`lufs`) or nothing; when the output uses
    /// `peak_db` the loudness spread is only reported and `peak_spread_db`
    /// applies instead.
    #[serde(default)]
    pub lufs_spread: Option<f32>,
    /// Largest sample-peak spread across them, dB (default 1). Applies when
    /// the output uses `peak_db`.
    #[serde(default)]
    pub peak_spread_db: Option<f32>,
    /// Smallest STFT distance between any two, dB (default 1).
    #[serde(default)]
    pub min_distance: Option<f32>,
}

/// The `layers` check: is the sound a designed one (layers in their bands,
/// atonal where it should be, dry)? Each field is one measure; `preset`
/// fills the ones not given. Energy shares are percent of the whole sound's
/// energy: weight under 200 Hz, body 200 Hz-2 kHz, crack above 2 kHz.
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayersCheck {
    /// "impact" (a hit, a slam, a knock, a blow): weight 15..60 %, body
    /// 30..80 %, crack at least 1.5 %, voiced at most 0.3, at most 1 ringing
    /// partial, crest at most 21 dB (active frames), dry. The Phase 0 door slams: the round-0
    /// build fails weight, body and crest, the first layered one ringing,
    /// crest and dry; the one the listener approved passes.
    #[serde(default)]
    pub preset: Option<String>,
    /// Percent of the energy under 200 Hz.
    #[serde(default)]
    pub weight: Option<Range>,
    /// Percent of the energy from 200 Hz to 2 kHz.
    #[serde(default)]
    pub body: Option<Range>,
    /// Percent of the energy above 2 kHz.
    #[serde(default)]
    pub crack: Option<Range>,
    /// Largest share of frames the pitch tracker calls voiced (0..1): an
    /// atonal impact reads near 0.
    #[serde(default)]
    pub voiced_max: Option<f32>,
    /// Most partials (from the `modes` line) within 6 dB of the strongest
    /// that ring longer than 0.6 s to -60 dB: more than one reads as a tuned
    /// drum (the Phase 0 slam the listener called "melodic" had three; the
    /// real slam one).
    #[serde(default)]
    pub ringing_max: Option<usize>,
    /// Peak over RMS of the active part (10 ms frames within 40 dB of the
    /// loudest; silence between and after hits is not counted), dB.
    #[serde(default)]
    pub crest_db: Option<Range>,
    /// Time from the peak to 20 dB under it, ms.
    #[serde(default)]
    pub tail_ms: Option<Range>,
    /// No `reverb` or `convolve` node anywhere in the sound (patches
    /// included): SFX are dry unless the brief names a space.
    #[serde(default)]
    pub dry: Option<bool>,
}

impl LayersCheck {
    /// The settings with the preset's values filled in where none is given.
    pub fn resolved(&self) -> Result<LayersCheck, String> {
        let base = match self.preset.as_deref() {
            None => LayersCheck::default(),
            Some("impact") => LayersCheck {
                preset: None,
                weight: Some(Range {
                    min: Some(15.0),
                    max: Some(60.0),
                }),
                body: Some(Range {
                    min: Some(30.0),
                    max: Some(80.0),
                }),
                crack: Some(Range {
                    min: Some(1.5),
                    max: None,
                }),
                voiced_max: Some(0.3),
                ringing_max: Some(1),
                // Over the active frames (`active_crest_db`): slam_a 21.3,
                // slam_b 23.4, the approved slam_b2 17.8.
                crest_db: Some(Range {
                    min: None,
                    max: Some(21.0),
                }),
                tail_ms: None,
                dry: Some(true),
            },
            Some("footstep") => LayersCheck {
                preset: None,
                // A board's or a floor's main mode sits under 200 Hz, so a
                // step's weight runs higher and its body lower than a hit's.
                weight: Some(Range {
                    min: Some(20.0),
                    max: Some(70.0),
                }),
                body: Some(Range {
                    min: Some(10.0),
                    max: Some(60.0),
                }),
                crack: Some(Range {
                    min: Some(2.0),
                    max: None,
                }),
                voiced_max: Some(0.3),
                ringing_max: Some(1),
                crest_db: Some(Range {
                    min: None,
                    max: Some(22.0),
                }),
                tail_ms: None,
                dry: Some(true),
            },
            Some(other) => {
                return Err(format!(
                    "layers: preset `{other}`: the presets are impact and footstep (or leave preset out and give the measures)"
                ));
            }
        };
        Ok(LayersCheck {
            preset: self.preset.clone(),
            weight: self.weight.clone().or(base.weight),
            body: self.body.clone().or(base.body),
            crack: self.crack.clone().or(base.crack),
            voiced_max: self.voiced_max.or(base.voiced_max),
            ringing_max: self.ringing_max.or(base.ringing_max),
            crest_db: self.crest_db.clone().or(base.crest_db),
            tail_ms: self.tail_ms.clone().or(base.tail_ms),
            dry: self.dry.or(base.dry),
        })
    }
}

/// A partial counts as ringing (`layers` `ringing_max`) when it is within
/// this many dB of the strongest and takes longer than `RING_S` to fall 60 dB.
const RING_WITHIN_DB: f32 = 6.0;
const RING_S: f32 = 0.6;

/// The strong partials that ring (within `RING_WITHIN_DB` of the strongest,
/// over `RING_S` to fall 60 dB), as "hz (decay)" words.
pub fn ringing_partials(d: &Descriptors) -> Vec<String> {
    let top = d.modes.iter().map(|x| x.level_db).fold(f32::MIN, f32::max);
    d.modes
        .iter()
        // A ring estimated longer than 1.5 x the sound is a misread (round 6:
        // 1.2 s on a 0.25 s step), not a ringing partial.
        .filter(|x| {
            x.level_db >= top - RING_WITHIN_DB
                && x.decay_s > RING_S
                && x.decay_s < 600.0
                && x.decay_s <= 1.5 * d.seconds
        })
        .map(|x| format!("{:.0} Hz ({:.2} s)", x.hz, x.decay_s))
        .collect()
}

/// Every `reverb` / `convolve` node in a list of nodes, patches included.
fn wet_nodes(nodes: &[Value], out: &mut Vec<String>) {
    for n in nodes {
        let ty = n.get("type").and_then(Value::as_str).unwrap_or("");
        if matches!(ty, "reverb" | "convolve") {
            out.push(format!(
                "{ty} `{}`",
                n.get("name").and_then(Value::as_str).unwrap_or("?")
            ));
        }
        if let Some(Value::Array(p)) = n.get("patch") {
            wet_nodes(p, out);
        }
    }
}

/// The `layers` measures: (name, pass, detail) each.
/// Peak over RMS (dB) of the active part of a sound: the RMS counts only
/// the 10 ms frames within 40 dB of the loudest, so silence between and
/// after hits does not inflate it (round 6: a knock train read 24-25 dB).
pub fn active_crest_db(a: &sfx_graph::Audio) -> f32 {
    let x = a.mix_mono();
    let w = ((0.01 * a.sample_rate as f32) as usize).max(1);
    let frames: Vec<f64> = x
        .chunks(w)
        .map(|c| c.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / c.len() as f64)
        .collect();
    let top = frames.iter().copied().fold(0.0f64, f64::max);
    let keep: Vec<f64> = frames.into_iter().filter(|e| *e >= top * 1e-4).collect();
    let rms = (keep.iter().sum::<f64>() / keep.len().max(1) as f64).sqrt();
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if rms <= 0.0 || peak <= 0.0 {
        return 0.0;
    }
    20.0 * (f64::from(peak) / rms).log10() as f32
}

fn layers_results(
    l: &LayersCheck,
    d: &Descriptors,
    audio: &sfx_graph::Audio,
    doc: &SoundDoc,
) -> Vec<(String, bool, String)> {
    let mut out = Vec::new();
    let [low, mid, hi, air] = d.band_shares;
    let share = |out: &mut Vec<(String, bool, String)>,
                 role: &str,
                 band: &str,
                 v: f32,
                 r: &Option<Range>,
                 low_fix: &str,
                 high_fix: &str| {
        if let Some(r) = r {
            let ok = range_ok(r, v);
            let detail = if ok {
                format!("{v:.1} % of the energy {band} ({})", range_text(r))
            } else if r.min.is_some_and(|m| v < m) {
                format!(
                    "{v:.1} % of the energy {band}, want {}: {low_fix}",
                    range_text(r)
                )
            } else {
                format!(
                    "{v:.1} % of the energy {band}, want {}: {high_fix}",
                    range_text(r)
                )
            };
            out.push((format!("layers {role}"), ok, detail));
        }
    };
    share(
        &mut out,
        "weight",
        "under 200 Hz",
        low,
        &l.weight,
        "add a weight layer (brown noise x decay through a lp at 150-250 Hz, slope 24; or finish weight_db)",
        "the low end carries the sound: lower the weight / sub layer, or add a body layer at 200 Hz-2 kHz",
    );
    share(
        &mut out,
        "body",
        "from 200 Hz to 2 kHz",
        mid,
        &l.body,
        "add a body layer (a modal of the object's material struck by a noise burst, or band-passed noise at 200 Hz-2 kHz)",
        "the body crowds out the rest: add weight under 200 Hz or a crack above 2 kHz, or lower the body",
    );
    share(
        &mut out,
        "crack",
        "above 2 kHz",
        hi + air,
        &l.crack,
        "add a crack (white noise x a decay of 0.008-0.03 s through a bp or hp at 2-6 kHz; or finish air_db)",
        "too bright: lower the crack, or low-pass it",
    );
    if let Some(m) = l.voiced_max {
        let ok = d.voiced <= m + 1e-6;
        out.push((
            "layers voiced".into(),
            ok,
            if ok {
                format!("{:.2} of the frames voiced (max {m})", d.voiced)
            } else {
                format!(
                    "{:.2} of the frames voiced (max {m}), pitch {}: the sound has a pitch; build weight from band-limited noise instead of a sine, strike modal with a noise burst, raise its loss",
                    d.voiced,
                    d.pitch_hz.map_or("n/a".into(), |p| format!("{p:.0} Hz"))
                )
            },
        ));
    }
    if let Some(m) = l.ringing_max {
        let ringing = ringing_partials(d);
        let ok = ringing.len() <= m;
        out.push((
            "layers ringing".into(),
            ok,
            if ok {
                format!(
                    "{} strong partial(s) ringing over {RING_S} s (max {m}){}",
                    ringing.len(),
                    if ringing.is_empty() { String::new() } else { format!(": {}", ringing.join(", ")) }
                )
            } else {
                format!(
                    "{} strong partials ring over {RING_S} s (max {m}): {}: that reads as a tuned drum; raise the modal loss, strike it with a noise burst, or replace a sine (a sub drop) with band-limited noise",
                    ringing.len(),
                    ringing.join(", ")
                )
            },
        ));
    }
    if let Some(r) = &l.crest_db {
        let v = active_crest_db(audio);
        let ok = range_ok(r, v);
        out.push((
            "layers crest".into(),
            ok,
            if ok {
                format!("{v:.1} dB ({})", range_text(r))
            } else if r.max.is_some_and(|m| v > m) {
                format!("{v:.1} dB, want {}: too spiky: finish glue, or longer body and weight layers under the hit", range_text(r))
            } else {
                format!("{v:.1} dB, want {}: too flat: finish punch, or a sharper crack", range_text(r))
            },
        ));
    }
    if let Some(r) = &l.tail_ms {
        match d.decay20_s {
            Some(t) => {
                let v = t * 1000.0;
                let ok = range_ok(r, v);
                out.push((
                    "layers tail".into(),
                    ok,
                    if ok {
                        format!("{v:.0} ms to -20 dB ({})", range_text(r))
                    } else {
                        format!("{v:.0} ms to -20 dB, want {}: lengthen or shorten the body and weight decays (or finish tail_db)", range_text(r))
                    },
                ));
            }
            None => out.push((
                "layers tail".into(),
                false,
                "never falls 20 dB under its peak: give the layers a decay".into(),
            )),
        }
    }
    if l.dry == Some(true) {
        let mut wet = Vec::new();
        wet_nodes(&doc.nodes, &mut wet);
        let ok = wet.is_empty();
        out.push((
            "layers dry".into(),
            ok,
            if ok {
                "no reverb or convolve node".into()
            } else {
                format!(
                    "{}: SFX are dry unless the brief names a space (the game places the sound); delete it, or drop `dry` from the check when the brief asks for a room",
                    wet.join(", ")
                )
            },
        ));
    }
    out
}

/// Share of a layer's energy in [t0, t1] s (10 ms of margin each side).
const SLOT_SHARE: f32 = 0.8;
/// Share of a layer's energy in its band.
const BAND_SHARE: f32 = 0.5;

/// The `spec` check: one line per component (`spec <role> <node>`).
fn spec_results(cx: &CheckCx) -> Result<Vec<(String, bool, String)>, String> {
    let Some(sp) = &cx.doc.spec else {
        return Ok(vec![(
            "spec".into(),
            false,
            "the sound has no spec: give sound_ops a spec {brief, components: [{role, node, material, at, band}]}".into(),
        )]);
    };
    if sp.components.is_empty() {
        return Ok(vec![(
            "spec".into(),
            false,
            "the spec lists no components: add one per layer (role, node)".into(),
        )]);
    }
    let sr = cx.rendered.audio.sample_rate;
    let len = cx.rendered.audio.len();
    let mut out = Vec::new();
    let statics = sp.static_findings(&cx.doc.nodes);
    for (c, (_, problems)) in sp.components.iter().zip(statics) {
        let name = format!(
            "spec {} {}",
            c.role,
            c.node.as_deref().unwrap_or("(not built)")
        );
        if !problems.is_empty() {
            out.push((name, false, problems.join("; ")));
            continue;
        }
        let node = c.node.as_deref().unwrap_or_default();
        let mono = match cx
            .rendered
            .eval
            .buf(node)
            .and_then(|b| sfx_graph::Audio::from_buf(sr, &b))
        {
            Ok(a) => {
                let mut m = a.mix_mono();
                m.truncate(len);
                m
            }
            Err(e) => {
                out.push((name, false, format!("`{node}` is not audio: {e}")));
                continue;
            }
        };
        let total: f64 = mono.iter().map(|x| f64::from(*x) * f64::from(*x)).sum();
        if total <= 1e-12 {
            out.push((name, false, format!("`{node}` is silent")));
            continue;
        }
        let mut ok = true;
        let mut words = Vec::new();
        if let Some(m) = &c.material {
            let found = sfx_graph::spec::materials(&cx.doc.nodes, node);
            words.push(if found.is_empty() {
                format!("material {m} (no material field to check)")
            } else {
                format!("material {m}")
            });
        }
        // When the energy sits: the times holding the middle 80 %.
        let mut acc = 0.0f64;
        let (mut s10, mut s90) = (None, None);
        for (i, x) in mono.iter().enumerate() {
            acc += f64::from(*x) * f64::from(*x);
            if s10.is_none() && acc >= 0.1 * total {
                s10 = Some(i as f32 / sr as f32);
            }
            if s90.is_none() && acc >= 0.9 * total {
                s90 = Some(i as f32 / sr as f32);
            }
        }
        let span = format!(
            "{:.0}-{:.0} ms",
            1000.0 * s10.unwrap_or(0.0),
            1000.0 * s90.unwrap_or(0.0)
        );
        if let Some([t0, t1]) = c.at {
            let a = (((t0 - 0.01).max(0.0)) * sr as f32) as usize;
            let b = (((t1 + 0.01) * sr as f32) as usize).min(mono.len());
            let inside: f64 = mono[a.min(b)..b]
                .iter()
                .map(|x| f64::from(*x) * f64::from(*x))
                .sum();
            let share = (inside / total) as f32;
            if share >= SLOT_SHARE {
                words.push(format!(
                    "{:.0} % of its energy in {t0}-{t1} s (middle 80 % at {span})",
                    100.0 * share
                ));
            } else {
                ok = false;
                words.push(format!(
                    "only {:.0} % of its energy in {t0}-{t1} s, want {:.0} % (the middle 80 % sits at {span}): move its `at`, shorten its decay, or correct the slot",
                    100.0 * share,
                    100.0 * SLOT_SHARE
                ));
            }
        }
        if let Some([lo, hi]) = c.length_ms {
            let v = 1000.0 * (s90.unwrap_or(0.0) - s10.unwrap_or(0.0));
            if v >= lo - 0.5 && v <= hi + 0.5 {
                words.push(format!("lasts {v:.0} ms ({lo}-{hi})"));
            } else {
                ok = false;
                words.push(format!(
                    "lasts {v:.0} ms (middle 80 % of its energy at {span}), want {lo}-{hi}: {}",
                    if v < lo && c.role == "body" {
                        "too short: a longer decay or a lower modal loss (a modal body is mostly its strike: add band-passed noise under it with a 0.3-0.6 s decay)"
                    } else if v < lo {
                        "too short: give it a longer decay"
                    } else {
                        "too long: a shorter decay or a higher modal loss"
                    }
                ));
            }
        }
        if let Some([lo, hi]) = c.band() {
            let spec = stft::stft(&mono, sr, 2048, 1024);
            let mean = spec.mean(0..0);
            let all: f32 = mean[1..].iter().sum();
            let inside: f32 = mean
                .iter()
                .enumerate()
                .skip(1)
                .filter(|(b, _)| {
                    let f = spec.hz(*b);
                    f >= lo && f <= hi
                })
                .map(|(_, p)| p)
                .sum();
            let centroid = mean
                .iter()
                .enumerate()
                .skip(1)
                .map(|(b, p)| spec.hz(b) * p)
                .sum::<f32>()
                / all.max(1e-20);
            let share = inside / all.max(1e-20);
            let src = if c.band.is_some() {
                ""
            } else {
                " (the role's band)"
            };
            if share >= BAND_SHARE {
                words.push(format!(
                    "{:.0} % in {lo}-{hi} Hz{src}, centroid {centroid:.0} Hz",
                    100.0 * share
                ));
            } else {
                ok = false;
                words.push(format!(
                    "only {:.0} % in {lo}-{hi} Hz{src}, want {:.0} % (centroid {centroid:.0} Hz): {} it, or correct the band",
                    100.0 * share,
                    100.0 * BAND_SHARE,
                    if centroid < lo { "high-pass or brighten" } else { "low-pass or darken" }
                ));
            }
        }
        if words.is_empty() {
            words.push("built and reaching the output".into());
        }
        out.push((name, ok, words.join("; ")));
    }
    Ok(out)
}

/// The `reference_set` check.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceSetCheck {
    /// The set: a folder name under `<project>/refs/`, or a path.
    pub set: String,
    /// Largest STFT distance to the nearest recording, dB (left out: not
    /// checked; the features are).
    #[serde(default)]
    pub max_distance: Option<f32>,
}

/// The `reference` check: the sound stays within `max_distance` dB of a WAV.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceCheck {
    /// A WAV: relative paths resolve against the project, or absolute.
    pub file: String,
    /// Largest multi-resolution STFT distance, dB, after aligning by onset
    /// and matching loudness (0 = identical; `listen reference` shows it).
    pub max_distance: f32,
}

/// The checks. With none given, `verify` runs [`Expect::defaults`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Sample peak at most `max` dBFS.
    #[serde(default)]
    pub peak_db: Option<Max>,
    /// True peak (4x oversampled) at most `max` dBTP.
    #[serde(default)]
    pub true_peak_db: Option<Max>,
    /// Integrated loudness in [min, max] LUFS.
    #[serde(default)]
    pub lufs: Option<Range>,
    /// Length in [min, max] seconds.
    #[serde(default)]
    pub duration: Option<Range>,
    /// First and last samples under -60 dBFS (no click at the edges).
    #[serde(default)]
    pub silent_edges: Option<bool>,
    /// No sample steps far above the local level (foley's click detector).
    #[serde(default)]
    pub clicks: Option<bool>,
    /// DC offset under `max` dB (default -40).
    #[serde(default)]
    pub dc: Option<Toggle<DcCheck>>,
    /// No NaN or infinite samples.
    #[serde(default)]
    pub finite: Option<bool>,
    /// Loop kind: no jump and no spectral change across the wrap.
    #[serde(default)]
    pub loop_seam: Option<bool>,
    /// Band-table limits.
    #[serde(default)]
    pub bands: Option<Vec<BandCheck>>,
    /// Median pitch within tol_cents of hz.
    #[serde(default)]
    pub pitch: Option<PitchCheck>,
    /// Onset count in [min, max]; `spacing_ms` (default 40) is the merge
    /// distance, 5 counts dense ticks.
    #[serde(default)]
    pub onsets: Option<OnsetsCheck>,
    /// Seeds differ audibly yet sit within lufs_spread LU.
    #[serde(default)]
    pub variations: Option<Toggle<Variations>>,
    /// An uncached re-render is bit-identical.
    #[serde(default)]
    pub rebuild: Option<bool>,
    /// Close to a reference recording (see `listen` `reference`).
    #[serde(default)]
    pub reference: Option<ReferenceCheck>,
    /// Energy off the harmonic grid above 4 kHz stays under `max_db` (default
    /// -40): the aliasing a nonlinearity or a naive oscillator leaves in a
    /// steady pitched sound.
    #[serde(default)]
    pub aliasing: Option<Toggle<AliasingCheck>>,
    /// Mean deviation of the strongest 8 partials from integer multiples of
    /// f0 in [min, max] cents (a harmonic tone: under 2; a bell: tens).
    #[serde(default)]
    pub inharmonicity: Option<Range>,
    /// The amplitude envelope (or pitch track) has a rate near `rate_hz`.
    #[serde(default)]
    pub modulation: Option<ModulationCheck>,
    /// Percent of the envelope's energy in 20 to 150 Hz in [min, max].
    #[serde(default)]
    pub roughness: Option<Range>,
    /// Every macro keeps its claim: swept over 0, .25, .5, .75, 1 (the others
    /// where they are), its measure moves the claimed way at every step and
    /// no position peaks over -0.3 dBFS.
    #[serde(default)]
    pub macros: Option<bool>,
    /// A designed sound: energy by role (weight, body, crack), voiced share,
    /// crest, tail and no reverb; `{"preset": "impact"}` for a hit.
    #[serde(default)]
    pub layers: Option<LayersCheck>,
    /// Inside the span of a set of recordings (`<project>/refs/<set>/`):
    /// every feature within the set's range, and the nearest within
    /// `max_distance` dB when given.
    #[serde(default)]
    pub reference_set: Option<ReferenceSetCheck>,
    /// The sound matches its spec (the brief as data, `sound_ops` `spec`):
    /// every component built and reaching the output, of its material, its
    /// energy in its time slot (80 %) and its band (half).
    #[serde(default)]
    pub spec: Option<bool>,
}

impl Expect {
    /// No check chosen (every field absent): `verify` then runs the defaults.
    pub fn is_empty(&self) -> bool {
        serde_json::to_value(self)
            .map(|v| {
                v.as_object()
                    .is_none_or(|m| m.values().all(serde_json::Value::is_null))
            })
            .unwrap_or(true)
    }

    /// `defaults`, plus `macros` when the sound has any.
    ///
    /// A loop checks `loop_seam` instead of `silent_edges` (a loop's first
    /// and last samples are not meant to be silent).
    pub fn defaults_for(doc: &SoundDoc) -> Expect {
        let looped = doc.kind == Kind::Loop;
        Expect {
            macros: (!doc.macros.is_empty()).then_some(true),
            spec: doc
                .spec
                .as_ref()
                .is_some_and(|sp| !sp.components.is_empty())
                .then_some(true),
            silent_edges: (!looped).then_some(true),
            loop_seam: looped.then_some(true),
            ..Expect::defaults()
        }
    }

    /// `finite`, `peak_db` max -0.3, `silent_edges`, `dc`, `rebuild`.
    pub fn defaults() -> Expect {
        Expect {
            finite: Some(true),
            peak_db: Some(Max { max: -0.3 }),
            silent_edges: Some(true),
            dc: Some(Toggle::On(true)),
            rebuild: Some(true),
            ..Expect::default()
        }
    }
}

/// One check's verdict.
#[derive(Clone, Debug, serde::Serialize)]
pub struct CheckResult {
    pub name: String,
    pub pass: bool,
    pub detail: String,
}

/// PASS / FAIL lines.
pub fn report(results: &[CheckResult]) -> String {
    results
        .iter()
        .map(|r| {
            format!(
                "{} {}: {}\n",
                if r.pass { "PASS" } else { "FAIL" },
                r.name,
                r.detail
            )
        })
        .collect()
}

fn range_ok(r: &Range, v: f32) -> bool {
    r.min.is_none_or(|m| v >= m - 1e-4) && r.max.is_none_or(|m| v <= m + 1e-4)
}

fn range_text(r: &Range) -> String {
    match (r.min, r.max) {
        (Some(a), Some(b)) => format!("{a}..{b}"),
        (Some(a), None) => format!(">= {a}"),
        (None, Some(b)) => format!("<= {b}"),
        (None, None) => "any".into(),
    }
}

/// Which band `v` names.
fn band_index(v: &serde_json::Value) -> Result<usize, String> {
    let hz = match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
        serde_json::Value::String(s) => {
            let t = s.trim().trim_end_matches('+').to_ascii_lowercase();
            if let Some(i) = BAND_LABELS
                .iter()
                .position(|l| l.trim_end_matches('+') == t)
            {
                return Ok(i);
            }
            let (num, k) = match t.strip_suffix('k') {
                Some(n) => (n, 1000.0),
                None => (t.trim_end_matches("hz").trim(), 1.0),
            };
            num.parse::<f32>().map_err(|_| {
                format!(
                    "band `{s}`: use one of {} or a number of Hz",
                    BAND_LABELS.join(", ")
                )
            })? * k
        }
        other => {
            return Err(format!(
                "band {other}: use one of {} or a number of Hz",
                BAND_LABELS.join(", ")
            ));
        }
    };
    Ok(BANDS
        .iter()
        .enumerate()
        .min_by(|a, b| {
            (a.1.ln() - hz.max(1.0).ln())
                .abs()
                .total_cmp(&(b.1.ln() - hz.max(1.0).ln()).abs())
        })
        .map_or(0, |(i, _)| i))
}

/// Samples whose step from the last is far (12x) above what is usual in the
/// 10 ms round them (foley's `foley-sim` `clicks`). Their times, seconds.
pub fn clicks(x: &[f32], sr: u32) -> Vec<f32> {
    let frame = ((sr / 100) as usize).max(2);
    let d: Vec<f32> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let mut out = Vec::new();
    for (k, f) in d.chunks(frame).enumerate() {
        let rms = (f.iter().map(|v| v * v).sum::<f32>() / f.len() as f32).sqrt();
        if rms < 1e-5 {
            continue;
        }
        for (j, v) in f.iter().enumerate() {
            if v.abs() > 12.0 * rms {
                out.push((k * frame + j + 1) as f32 / sr as f32);
            }
        }
    }
    out
}

/// How much the spectrum changes across a loop's wrap: the mean |dB|
/// difference between the end and the start in third-octave bands from 100 Hz
/// up (bands within 60 dB of the loudest). Bands, not bins: two windows of
/// noise differ by several dB in every bin and by well under 1 dB in a band.
fn seam_spectral(tail: &[f32], head: &[f32], sr: u32) -> f32 {
    let win = 2048.min(tail.len()).max(16).next_power_of_two() / 2;
    let inner = |x: &[f32]| {
        let s = stft::stft(x, sr, win, win / 4);
        // Frame k is centred on k * hop: keep those wholly inside.
        let first = (win / 2).div_ceil(win / 4);
        let last = (x.len().saturating_sub(win / 2)) / (win / 4);
        let mean = s.mean(first..last.max(first + 1).min(s.frames.len()));
        let nyq = sr as f32 / 2.0;
        let mut bands = Vec::new();
        let mut lo = 100.0f32;
        while lo < nyq {
            let hi = (lo * 2f32.powf(1.0 / 3.0)).min(nyq);
            let p: f32 = (0..mean.len())
                .filter(|&b| s.hz(b) >= lo && s.hz(b) < hi)
                .map(|b| mean[b])
                .sum();
            bands.push(p);
            lo = hi;
        }
        bands
    };
    let (a, b) = (inner(tail), inner(head));
    let top = a.iter().chain(&b).copied().fold(0.0f32, f32::max);
    let floor = stft::db(top) - 60.0;
    let (mut sum, mut k) = (0.0f32, 0usize);
    for (p, q) in a.iter().zip(&b) {
        let (x, y) = (stft::db(*p), stft::db(*q));
        if x > floor || y > floor {
            sum += (x.max(floor) - y.max(floor)).abs();
            k += 1;
        }
    }
    if k == 0 { 0.0 } else { sum / k as f32 }
}

/// Sweeps one macro that has a claim; (pass, text).
/// How far one sweep step may go against a macro's claim, as a share of
/// the end-to-end move.
const CLAIM_BACKSTEP: f32 = 0.15;

fn macro_check(
    cx: &CheckCx,
    m: &sfx_graph::Macro,
    claim: sfx_graph::Claim,
) -> Result<(bool, String), String> {
    use crate::measure::{fmt, measure};
    let sfx_graph::Claim(measure_of, dir) = claim;
    let xs = sfx_graph::macros::SWEEP;
    let (mut vals, mut peaks) = (Vec::new(), Vec::new());
    for x in xs {
        let r = cx.doc.render(&RenderOpts {
            macros: [(m.name.clone(), x)].into(),
            seed: Some(cx.rendered.seed),
            ..cx.opts.clone()
        })?;
        vals.push(measure(&r.audio, measure_of));
        peaks.push(r.peak_db);
    }
    let d = f32::from(dir);
    // A step against the claim may be up to 15 % of the end-to-end move:
    // noise layers (`modal` `noise`, a noise crack) jitter a measure from
    // one knob position to the next, and a 1 % tolerance pushed agents to
    // drop the noise to pass (round 6).
    let span = (vals[4] - vals[0]).abs();
    let tol = (span * CLAIM_BACKSTEP).max(1e-6);
    let steady = vals.windows(2).all(|w| (w[1] - w[0]) * d > -tol);
    let net = (vals[4] - vals[0]) * d > (span * 0.01).max(1e-6);
    let clip = peaks.iter().position(|p| *p > -0.3);
    // The knob must move the measure by at least 2 % of the largest value it
    // reads: a claim that holds only by a hair is a knob nobody can hear.
    let scale = vals
        .iter()
        .map(|v| v.abs())
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let barely = (vals[4] - vals[0]).abs() < 0.02 * scale;
    let shown: Vec<String> = vals.iter().map(|v| fmt(measure_of, *v)).collect();
    let word = if dir > 0 { "rise" } else { "fall" };
    let list = shown.join(" -> ");
    Ok(match (steady && net, clip) {
        (true, None) if barely => (
            false,
            format!(
                "the knob barely moves {} ({list}: under 2 % of the value across 0..1): widen its map, or rename the macro to what it really does",
                measure_of.name()
            ),
        ),
        (true, None) => (
            true,
            format!("{} {}s over 0..1: {list}", measure_of.name(), word),
        ),
        (_, Some(i)) => (
            false,
            format!(
                "clips at {} (peak {:.1} dBFS): lower the gains it drives, or limit before the output",
                xs[i], peaks[i]
            ),
        ),
        (false, None) => (
            false,
            format!(
                "{} does not {word} over 0..1 ({list}): rename the macro to what it really does (claim), or fix what it moves",
                measure_of.name()
            ),
        ),
    })
}

/// What the checks read.
pub struct CheckCx<'a> {
    pub doc: &'a SoundDoc,
    pub rendered: &'a Rendered,
    pub opts: &'a RenderOpts,
}

/// Runs the checks in `e` against a render.
pub fn run(e: &Expect, cx: &CheckCx) -> Result<Vec<CheckResult>, String> {
    let a = &cx.rendered.audio;
    let sr = a.sample_rate;
    let mut out = Vec::new();
    let mut push = |name: &str, pass: bool, detail: String| {
        out.push(CheckResult {
            name: name.into(),
            pass,
            detail,
        })
    };
    let mut desc: Option<Descriptors> = None;
    let mut d = || -> Descriptors { desc.get_or_insert_with(|| descriptors::describe(a)).clone() };

    if e.finite == Some(true) {
        let bad = a
            .channels
            .iter()
            .flatten()
            .filter(|x| !x.is_finite())
            .count();
        push(
            "finite",
            bad == 0,
            if bad == 0 {
                "every sample is finite".into()
            } else {
                format!(
                    "{bad} NaN or infinite samples: look for a division by zero, a pow of a negative, or a filter with q near 0"
                )
            },
        );
    }
    if let Some(m) = &e.peak_db {
        let v = cx.rendered.peak_db;
        push(
            "peak_db",
            v <= m.max + 1e-4,
            if v <= m.max + 1e-4 {
                format!("{v:.2} dBFS (max {})", m.max)
            } else {
                format!(
                    "{v:.2} dBFS is over {}: lower output ceiling_db (or gain_db)",
                    m.max
                )
            },
        );
    }
    if let Some(m) = &e.true_peak_db {
        let v = gain_to_db(sfx_graph::loudness::true_peak(&a.channels));
        push(
            "true_peak_db",
            v <= m.max + 1e-4,
            if v <= m.max + 1e-4 {
                format!("{v:.2} dBTP (max {})", m.max)
            } else {
                format!(
                    "{v:.2} dBTP is over {}: lower output ceiling_db by {:.1} dB",
                    m.max,
                    v - m.max
                )
            },
        );
    }
    if let Some(r) = &e.lufs {
        let v = cx.rendered.lufs;
        let ok = range_ok(r, v);
        push(
            "lufs",
            ok,
            if ok {
                format!("{v:.2} LUFS ({})", range_text(r))
            } else {
                let target = r
                    .min
                    .zip(r.max)
                    .map_or(r.min.or(r.max).unwrap_or(v), |(a, b)| 0.5 * (a + b));
                format!(
                    "{v:.2} LUFS, want {}: set output lufs to {target:.1}",
                    range_text(r)
                )
            },
        );
    }
    if let Some(r) = &e.duration {
        let v = a.seconds();
        let ok = range_ok(r, v);
        push(
            "duration",
            ok,
            if ok {
                format!("{v:.3} s ({})", range_text(r))
            } else if cx.rendered.auto_length {
                format!(
                    "{v:.3} s, want {}: shorten or lengthen the decay envelope, or set duration",
                    range_text(r)
                )
            } else {
                format!("{v:.3} s, want {}: change duration", range_text(r))
            },
        );
    }
    if e.silent_edges == Some(true) {
        let th = db_to_gain(-60.0);
        let first = a
            .channels
            .iter()
            .map(|c| c.first().map_or(0.0, |v| v.abs()))
            .fold(0.0, f32::max);
        let last = a
            .channels
            .iter()
            .map(|c| c.last().map_or(0.0, |v| v.abs()))
            .fold(0.0, f32::max);
        let ok = first < th && last < th;
        let mut why = Vec::new();
        if first >= th {
            why.push(format!(
                "first sample at {:.1} dBFS (a click on start: add output fade_in)",
                gain_to_db(first)
            ));
        }
        if last >= th {
            why.push(format!(
                "last sample at {:.1} dBFS (cut off mid-wave: add output fade_out, or let it decay)",
                gain_to_db(last)
            ));
        }
        push(
            "silent_edges",
            ok,
            if ok {
                format!(
                    "edges at {:.0} / {:.0} dBFS",
                    gain_to_db(first),
                    gain_to_db(last)
                )
            } else {
                why.join("; ")
            },
        );
    }
    if e.clicks == Some(true) {
        let t = clicks(&a.mix_mono(), sr);
        push(
            "clicks",
            t.is_empty(),
            if t.is_empty() {
                "no clicks".into()
            } else {
                let at: Vec<String> = t.iter().take(5).map(|s| format!("{s:.3}")).collect();
                format!(
                    "{} clicks (at {} s): smooth hard edges (an envelope that jumps, a hard sync, sample_hold on audio) or low-pass them",
                    t.len(),
                    at.join(", ")
                )
            },
        );
    }
    if let Some(dc) = e.dc.as_ref().and_then(Toggle::get) {
        let max = dc.max.unwrap_or(-40.0);
        let worst = a
            .channels
            .iter()
            .map(|c| {
                gain_to_db(
                    (c.iter().map(|x| f64::from(*x)).sum::<f64>() / c.len().max(1) as f64) as f32,
                )
            })
            .fold(-200.0f32, f32::max);
        push(
            "dc",
            worst <= max,
            if worst <= max {
                format!("{worst:.0} dB (max {max})")
            } else {
                format!(
                    "DC offset {worst:.1} dB is over {max}: add an onepole hp at 20 Hz before the output"
                )
            },
        );
    }
    if e.loop_seam == Some(true) {
        if cx.doc.kind != Kind::Loop {
            push(
                "loop_seam",
                false,
                "the sound is a oneshot: set kind \"loop\" and a duration".into(),
            );
        } else {
            // The wrap sample against the loop's own statistics: the step
            // (a jump) and the error predicting x[0] from the last two
            // samples (a kink: a slope reversal clicks too).
            let mut worst_ratio = 0.0f32;
            for c in &a.channels {
                let n = c.len();
                if n < 4 {
                    continue;
                }
                let p99 = |mut v: Vec<f32>| {
                    v.sort_by(f32::total_cmp);
                    v[(v.len() * 99) / 100].max(1e-5)
                };
                let steps = p99(c.windows(2).map(|w| (w[1] - w[0]).abs()).collect());
                let curves = p99(c
                    .windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .collect());
                let step = (c[0] - c[n - 1]).abs() / steps;
                let kink = (c[0] - (2.0 * c[n - 1] - c[n - 2]))
                    .abs()
                    .max((c[1] - (2.0 * c[0] - c[n - 1])).abs())
                    / curves;
                worst_ratio = worst_ratio.max(step).max(kink);
            }
            let m = a.mix_mono();
            let w = (16_384.min(m.len() / 2)).max(16).min(m.len());
            let tail = &m[m.len() - w..];
            let head = &m[..w];
            let spec_d = seam_spectral(tail, head, sr);
            let ok = worst_ratio <= 2.0 && spec_d <= 6.0;
            push(
                "loop_seam",
                ok,
                format!(
                    "wrap discontinuity {worst_ratio:.2}x the loop's 99th-percentile step / curvature (max 2), spectral change {spec_d:.1} dB (max 6){}",
                    if ok {
                        ""
                    } else {
                        ": make every element periodic in the duration (lfo freq a multiple of 1/duration) or crossfade the end into the start"
                    }
                ),
            );
        }
    }
    if let Some(bands) = &e.bands {
        let dd = d();
        for b in bands {
            let i = band_index(&b.band)?;
            let (v, wh) = match b.segment {
                Some(s) if s < SEGMENTS => (dd.bands[i][s], format!("{}-{}%", s * 20, s * 20 + 20)),
                Some(s) => return Err(format!("bands: segment {s}: use 0..{}", SEGMENTS - 1)),
                None => (dd.bands_total[i], "whole sound".into()),
            };
            let ok = b.min_db.is_none_or(|m| v >= m) && b.max_db.is_none_or(|m| v <= m);
            let want = range_text(&Range {
                min: b.min_db,
                max: b.max_db,
            });
            push(
                &format!("bands {} Hz {wh}", BAND_LABELS[i]),
                ok,
                if ok {
                    format!("{v:.1} dB ({want})")
                } else {
                    format!(
                        "{v:.1} dB, want {want}: {} that band with a filter (peak / shelf) or change the source",
                        if b.min_db.is_some_and(|m| v < m) {
                            "boost"
                        } else {
                            "cut"
                        }
                    )
                },
            );
        }
    }
    if let Some(p) = &e.pitch {
        let dd = d();
        let tol = p.tol_cents.unwrap_or(25.0);
        match dd.pitch_hz {
            Some(hz) => {
                let c = crate::pitch::cents(p.hz, hz);
                push(
                    "pitch",
                    c.abs() <= tol,
                    if c.abs() <= tol {
                        format!("{hz:.2} Hz ({c:+.1} cents from {})", p.hz)
                    } else {
                        format!(
                            "{hz:.2} Hz is {c:+.0} cents from {} (tolerance {tol}): scale the frequencies by {:.4}",
                            p.hz,
                            p.hz / hz
                        )
                    },
                );
            }
            None => push(
                "pitch",
                false,
                format!(
                    "no pitch found (voiced {:.0}%): the sound is noisy or too short to hold a pitch",
                    dd.voiced * 100.0
                ),
            ),
        }
    }
    if let Some(al) = e.aliasing.as_ref().and_then(Toggle::get) {
        let max = al.max_db.unwrap_or(-40.0);
        match crate::measure::aliasing_db(a) {
            Some(v) => {
                let ok = v <= max;
                let fix = {
                    let shapes: Vec<&str> = cx
                        .doc
                        .nodes
                        .iter()
                        .filter(|n| n.get("type").and_then(|t| t.as_str()) == Some("shape"))
                        .filter_map(|n| n.get("name").and_then(|t| t.as_str()))
                        .collect();
                    match shapes.as_slice() {
                        [] => "likely aliasing: a naive oscillator or a nonlinearity folding harmonics back; band-limit the source (osc) or filter before the nonlinearity".to_string(),
                        [one] => format!("raise `oversample` on shape `{one}`"),
                        many => format!(
                            "raise `oversample` on shape `{}` (or one of: {})",
                            many[0],
                            many.join(", ")
                        ),
                    }
                };
                push(
                    "aliasing",
                    ok,
                    if ok {
                        format!("{v:.1} dB off the harmonic grid above 4 kHz (max {max})")
                    } else {
                        format!(
                            "{v:.1} dB off the harmonic grid above 4 kHz is over {max}: {fix}"
                        )
                    },
                );
            }
            None => push(
                "aliasing",
                false,
                "no steady pitched segment to measure (needs a pitch of 60 Hz to 4 kHz that holds within 1.5 % across 12 periods, and mostly harmonic): check the source with listen zoom instead".into(),
            ),
        }
    }
    if let Some(r) = &e.inharmonicity {
        match crate::measure::inharmonicity_cents(a) {
            Some(v) => {
                let ok = range_ok(r, v);
                push(
                    "inharmonicity",
                    ok,
                    if ok {
                        format!("{v:.1} cents ({})", range_text(r))
                    } else if r.max.is_some_and(|m| v > m) {
                        format!(
                            "{v:.1} cents, want {}: the partials are off integer multiples; tune them (a modal / resonators table) or use a harmonic source",
                            range_text(r)
                        )
                    } else {
                        format!(
                            "{v:.1} cents, want {}: the partials sit on integer multiples; stretch them apart (mode ratios like 2.76, 5.4 for a bell)",
                            range_text(r)
                        )
                    },
                );
            }
            None => push(
                "inharmonicity",
                false,
                "under 3 clear partials (20 dB over the median bin): the sound is noisy or too sparse to measure".into(),
            ),
        }
    }
    if let Some(m) = &e.modulation {
        let pitch_mode = match m.mode.as_deref() {
            None | Some("amplitude") => false,
            Some("pitch") => true,
            Some(other) => {
                return Err(format!(
                    "modulation: mode {other:?}: use \"amplitude\" or \"pitch\""
                ));
            }
        };
        let tol = m.tol_hz.unwrap_or(0.5);
        let min_depth = m.min_depth.unwrap_or(if pitch_mode { 10.0 } else { 1.0 });
        let unit = if pitch_mode { "cents" } else { "dB" };
        let found = if pitch_mode {
            crate::measure::pitch_modulation(a)
        } else {
            crate::measure::amplitude_modulation(a)
        };
        let near = found.iter().find(|p| (p.rate_hz - m.rate_hz).abs() <= tol);
        let seen = found
            .iter()
            .map(|p| format!("{:.1} Hz {:.1} {unit}", p.rate_hz, p.depth))
            .collect::<Vec<_>>()
            .join(", ");
        let name = if pitch_mode {
            "modulation (pitch)"
        } else {
            "modulation"
        };
        match near {
            Some(p) if p.depth >= min_depth => push(
                name,
                true,
                format!(
                    "{:.2} Hz at {:.1} {unit} peak-to-peak (wanted {} +- {tol} Hz, depth >= {min_depth})",
                    p.rate_hz, p.depth, m.rate_hz
                ),
            ),
            Some(p) => push(
                name,
                false,
                format!(
                    "{:.2} Hz found but only {:.1} {unit} deep, want >= {min_depth}: deepen the modulation",
                    p.rate_hz, p.depth
                ),
            ),
            None => push(
                name,
                false,
                format!(
                    "no rate within {tol} Hz of {} (found: {}): change the modulator's rate{}",
                    m.rate_hz,
                    if seen.is_empty() { "none" } else { &seen },
                    if pitch_mode {
                        " (pitch rates are limited to about 40 Hz by the pitch frame rate)"
                    } else {
                        ""
                    }
                ),
            ),
        }
    }
    if let Some(r) = &e.roughness {
        let v = crate::measure::roughness_pct(a);
        let ok = range_ok(r, v);
        push(
            "roughness",
            ok,
            if ok {
                format!(
                    "{v:.1} % of the envelope energy in 20-150 Hz ({})",
                    range_text(r)
                )
            } else if r.max.is_some_and(|m| v > m) {
                format!(
                    "{v:.1} % in 20-150 Hz, want {}: slow the beating / modulation below 20 Hz or push it above 150 Hz, or lower its depth",
                    range_text(r)
                )
            } else {
                format!(
                    "{v:.1} % in 20-150 Hz, want {}: add amplitude modulation or beating between partials 20-150 Hz apart",
                    range_text(r)
                )
            },
        );
    }
    if let Some(oc) = &e.onsets {
        let r = oc.range();
        let r = &r;
        if oc.spacing_ms.is_some_and(|m| !(m.is_finite() && m > 0.0)) {
            return Err(format!(
                "onsets: spacing_ms {}: give a positive number of milliseconds (default 40, 5 counts dense ticks)",
                oc.spacing_ms.unwrap_or(0.0)
            ));
        }
        if let Some([lo, hi]) = oc.gap
            && !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && lo <= hi)
        {
            return Err(format!(
                "onsets: gap [{lo}, {hi}]: give [lo, hi] in seconds with 0 <= lo <= hi, e.g. [0.15, 0.3]"
            ));
        }
        let (times, how): (Vec<f32>, String) = match oc.spacing_ms {
            Some(m) if m < 30.0 => (
                crate::measure::fine_onsets(a, m),
                format!(" (fine detector above 1 kHz, {m} ms apart)"),
            ),
            Some(m) if m > 40.0 => {
                let mut t: Vec<f32> = Vec::new();
                for o in &d().onsets {
                    if t.last().is_none_or(|l| (o.t - l) * 1000.0 >= m) {
                        t.push(o.t);
                    }
                }
                (t, format!(" (merged under {m} ms)"))
            }
            _ => (d().onsets.iter().map(|o| o.t).collect(), String::new()),
        };
        let k = times.len() as f32;
        let count_ok = range_ok(r, k);
        // Gaps outside [lo, hi], as (index, gap, from, to).
        let bad_gaps: Vec<(usize, f32, f32, f32)> = match oc.gap {
            Some([lo, hi]) => times
                .windows(2)
                .enumerate()
                .filter(|(_, w)| {
                    let g = w[1] - w[0];
                    g < lo - 1e-4 || g > hi + 1e-4
                })
                .map(|(i, w)| (i + 1, w[1] - w[0], w[0], w[1]))
                .collect(),
            None => Vec::new(),
        };
        let gap_text = oc.gap.map(|[lo, hi]| {
            if times.len() < 2 {
                format!("no gaps to check (gap {lo}-{hi} s)")
            } else {
                let gs: Vec<f32> = times.windows(2).map(|w| w[1] - w[0]).collect();
                let gmin = gs.iter().copied().fold(f32::INFINITY, f32::min);
                let gmax = gs.iter().copied().fold(0.0, f32::max);
                format!(
                    "{} gaps {:.3}-{:.3} s (want {lo}-{hi} s)",
                    gs.len(),
                    gmin,
                    gmax
                )
            }
        });
        let ok = count_ok && bad_gaps.is_empty();
        let mut detail = if count_ok {
            format!("{k} onsets{how} ({})", range_text(r))
        } else if oc.spacing_ms.is_none() && k > 0.0 && r.min.is_some_and(|m| k < m) {
            format!(
                "{k} onsets, want {}: events closer than 30-40 ms merge; pass spacing_ms: 5 to count dense ticks",
                range_text(r)
            )
        } else {
            format!("{k} onsets{how}, want {}", range_text(r))
        };
        if let (Some(g), Some([lo, hi])) = (&gap_text, oc.gap) {
            if bad_gaps.is_empty() {
                detail.push_str(&format!("; {g}"));
            } else {
                let list: Vec<String> = bad_gaps
                    .iter()
                    .take(6)
                    .map(|(i, g, a, b)| format!("gap {i} is {g:.3} s ({a:.3} -> {b:.3} s)"))
                    .collect();
                detail.push_str(&format!(
                    "; {} of {} gaps outside {lo}-{hi} s: {}{}; onsets at {}{}: move those hits or widen `gap`",
                    bad_gaps.len(),
                    times.len() - 1,
                    list.join(", "),
                    if bad_gaps.len() > 6 { ", ..." } else { "" },
                    times
                        .iter()
                        .take(12)
                        .map(|t| format!("{t:.3}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    if times.len() > 12 { " ..." } else { "" },
                ));
            }
        }
        push("onsets", ok, detail);
    }
    if let Some(v) = e.variations.as_ref().and_then(Toggle::get) {
        let n = v.seeds.unwrap_or(6).clamp(2, 32);
        let spread_max = v.lufs_spread.unwrap_or(1.0);
        let dmin = v.min_distance.unwrap_or(1.0);
        let mut renders = Vec::new();
        let mut peaks = Vec::new();
        for s in 1..=n {
            let r = cx.doc.render(&RenderOpts {
                seed: Some(s),
                ..cx.opts.clone()
            })?;
            renders.push((r.lufs, r.audio.mix_mono()));
            peaks.push(r.peak_db);
        }
        let lo = renders.iter().map(|r| r.0).fold(f32::MAX, f32::min);
        let hi = renders.iter().map(|r| r.0).fold(f32::MIN, f32::max);
        let mut closest = (f32::MAX, 0u64, 0u64);
        for i in 0..renders.len() {
            for j in i + 1..renders.len() {
                let dist = stft::distance(&renders[i].1, &renders[j].1, sr);
                if dist < closest.0 {
                    closest = (dist, i as u64 + 1, j as u64 + 1);
                }
            }
        }
        let by_peak = cx.doc.nodes.iter().any(|n| {
            n.get("type").and_then(Value::as_str) == Some("output")
                && n.get("peak_db").is_some_and(|v| !v.is_null())
        });
        let lu = hi - lo;
        let pk_lo = peaks.iter().cloned().fold(f32::MAX, f32::min);
        let pk_hi = peaks.iter().cloned().fold(f32::MIN, f32::max);
        let pk = pk_hi - pk_lo;
        let peak_max = v.peak_spread_db.unwrap_or(1.0);
        let ok_spread = if by_peak {
            pk <= peak_max
        } else {
            lu <= spread_max
        };
        let ok_dist = closest.0 >= dmin;
        let pair = format!(
            "closest pair seeds {} and {} at {:.2} dB (min {dmin})",
            closest.1, closest.2, closest.0
        );
        let mut why = if by_peak {
            format!(
                "{n} seeds: peak spread {pk:.2} dB (max {peak_max}; the output uses peak_db, so peak was compared), loudness spread {lu:.2} LU (reported only), {pair}"
            )
        } else {
            format!("{n} seeds: loudness spread {lu:.2} LU (max {spread_max}), {pair}")
        };
        if !ok_spread {
            why += if by_peak {
                ": the peak varies too much between seeds (the output peak_db should pin it: is a limiter or a clipped node in the way?); narrow the random ranges that drive level or raise peak_spread_db"
            } else {
                ": the loudness varies too much; narrow the random ranges that drive level, set output lufs, or raise lufs_spread"
            };
        } else if !ok_dist {
            why += ": the seeds sound alike; let the seed drive more (noise seeds, wander, random pitch)";
        }
        push("variations", ok_spread && ok_dist, why);
    }
    if let Some(rc) = &e.reference {
        let rf = crate::reference::load(&rc.file, cx.opts.base_dir.as_deref(), &cx.opts.sources)?;
        let c = crate::reference::compare(a, &rf);
        let ok = c.distance_db <= rc.max_distance;
        let mut why = format!(
            "{:.2} dB from {} (max {}; reference scaled {:+.1} dB, shifted {:.3} s)",
            c.distance_db, rc.file, rc.max_distance, c.gain_db, c.shift_s
        );
        if !ok {
            why += &format!(": {}", c.advice());
            for d in c.deviations.iter().skip(1).take(2) {
                why += &format!("; also {}", d.words);
            }
        }
        push("reference", ok, why);
    }
    if let Some(rs) = &e.reference_set {
        let members = crate::refset::load_set(&rs.set, cx.opts.base_dir.as_deref())?;
        let c = crate::refset::compare_set(a, &rs.set, &members);
        for f in &c.features {
            push(&format!("reference_set {}", f.name), f.pass, f.words());
        }
        let (near, dn) = &c.distances[c.nearest];
        let ok = rs.max_distance.is_none_or(|m| *dn <= m);
        push(
            "reference_set nearest",
            ok,
            format!(
                "{near} at {dn:.2} dB (the set's own spread {:.2} dB{})",
                c.spread_db,
                rs.max_distance
                    .map_or(String::new(), |m| format!("; max {m}"))
            ),
        );
    }
    if let Some(l) = &e.layers {
        let l = l.resolved()?;
        for (name, pass, detail) in layers_results(&l, &d(), a, cx.doc) {
            push(&name, pass, detail);
        }
    }
    if e.spec == Some(true) {
        for (name, pass, detail) in spec_results(cx)? {
            push(&name, pass, detail);
        }
    }
    if e.macros == Some(true) {
        let mut unclaimed: Vec<&str> = Vec::new();
        for m in &cx.doc.macros {
            let Some(claim) = m.claim else {
                unclaimed.push(&m.name);
                continue;
            };
            match macro_check(cx, m, claim) {
                Ok(r) => push(&format!("macro {}", m.name), r.0, r.1),
                Err(e) => push(
                    &format!("macro {}", m.name),
                    false,
                    format!("the sweep failed to render: {e}"),
                ),
            }
        }
        if !unclaimed.is_empty() {
            push(
                "macros",
                true,
                format!(
                    "{} macros unclaimed, not swept: {} (give one a claim [measure, direction] to have verify hold its name to it)",
                    unclaimed.len(),
                    unclaimed.join(", ")
                ),
            );
        }
    }
    if e.rebuild == Some(true) {
        let fresh = cx.doc.render(&RenderOpts {
            cached: false,
            seed: Some(cx.rendered.seed),
            ..cx.opts.clone()
        })?;
        let same = fresh.audio.channels.len() == a.channels.len()
            && fresh.audio.channels.iter().zip(&a.channels).all(|(p, q)| {
                p.len() == q.len() && p.iter().zip(q).all(|(x, y)| x.to_bits() == y.to_bits())
            });
        push(
            "rebuild",
            same,
            if same {
                "an uncached re-render is bit-identical".into()
            } else {
                "an uncached re-render differs: a node is not deterministic (report it as a bug)"
                    .into()
            },
        );
    }
    Ok(out)
}
