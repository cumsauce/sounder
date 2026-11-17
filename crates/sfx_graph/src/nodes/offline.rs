//! Offline (acausal) processing and the output stage.

use std::f32::consts::PI;

use serde::Deserialize;

use super::{non_negative, one};
use crate::buf::Buf;
use crate::node::{Cx, Describe, Eval, In, Outs};
use crate::{db_to_gain, loudness};

/// Plays the input backwards.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reverse {
    pub input: In,
}

impl Describe for Reverse {
    const BLURB: &'static str = "plays the input backwards";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "verb"})
    }
}

impl Eval for Reverse {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        for c in &mut ch {
            c.reverse();
        }
        Ok(one(Buf::from_channels(ch)))
    }
}

/// Gain that brings `ch` to a peak or a loudness.
fn normalize_gain(
    ch: &[Vec<f32>],
    peak_db: Option<f32>,
    lufs: Option<f32>,
    sr: f32,
) -> Result<f32, String> {
    match (peak_db, lufs) {
        (Some(p), None) => {
            let peak = ch.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
            Ok(if peak > 1e-9 {
                db_to_gain(p) / peak
            } else {
                1.0
            })
        }
        (None, Some(l)) => {
            let now = loudness::integrated(ch, sr as u32);
            if now > -69.0 {
                return Ok(db_to_gain(l - now));
            }
            // `integrated` gates everything under -70 LUFS and reports
            // exactly -70 for it, so a very quiet sound would look silent.
            // Measure it again 40 dB hotter (up to three times, down to
            // -190 LUFS); only a signal still under the gate is silence.
            let mut lift = 0.0f32;
            for _ in 0..3 {
                lift += 40.0;
                let g = db_to_gain(lift);
                let hot: Vec<Vec<f32>> = ch
                    .iter()
                    .map(|c| c.iter().map(|x| x * g).collect())
                    .collect();
                let now = loudness::integrated(&hot, sr as u32);
                if now > -69.0 {
                    return Ok(db_to_gain(l - (now - lift)));
                }
            }
            Ok(1.0)
        }
        _ => Err("give exactly one of peak_db or lufs".into()),
    }
}

/// Scales to a peak or a loudness.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Normalize {
    pub input: In,
    /// Target sample peak, dBFS (or give lufs).
    #[serde(default)]
    pub peak_db: Option<f32>,
    /// Target integrated loudness, LUFS (or give peak_db).
    #[serde(default)]
    pub lufs: Option<f32>,
}

impl Describe for Normalize {
    const BLURB: &'static str = "scales to a peak or a loudness";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "peak_db": -1})
    }
}

impl Eval for Normalize {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        let g = normalize_gain(&ch, self.peak_db, self.lufs, cx.sr)?;
        for v in ch.iter_mut().flatten() {
            *v *= g;
        }
        Ok(one(Buf::from_channels(ch)))
    }
}

/// Removes leading silence.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Trim {
    pub input: In,
    /// Samples below this are silence, dBFS (default -60).
    #[serde(default)]
    pub threshold_db: Option<f32>,
}

impl Describe for Trim {
    const BLURB: &'static str = "removes leading silence (the end is padded with silence)";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "threshold_db": -60})
    }
}

impl Eval for Trim {
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let th = db_to_gain(self.threshold_db.unwrap_or(-60.0));
        let ch = input.channels(cx.n);
        let first = (0..cx.n)
            .find(|&i| ch.iter().any(|c| c[i].abs() >= th))
            .unwrap_or(0);
        Ok(one(Buf::from_channels(
            ch.into_iter()
                .map(|c| {
                    let mut v = c[first..].to_vec();
                    v.resize(cx.n, 0.0);
                    v
                })
                .collect(),
        )))
    }
}

/// Raised-cosine fades at the edges of `ch` (`fin`, `fout` seconds).
fn fade_edges(ch: &mut [Vec<f32>], fin: f32, fout: f32, sr: f32) {
    let n = ch.first().map_or(0, Vec::len);
    let a = ((fin * sr) as usize).min(n);
    let b = ((fout * sr) as usize).min(n);
    for c in ch.iter_mut() {
        for i in 0..a {
            c[i] *= 0.5 - 0.5 * (PI * i as f32 / a as f32).cos();
        }
        for k in 0..b {
            // k = 0 is the last sample: exactly silent.
            c[n - 1 - k] *= 0.5 - 0.5 * (PI * k as f32 / b as f32).cos();
        }
    }
}

/// Fades in and out.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    pub input: In,
    /// Fade-in seconds (default 0).
    #[serde(default, rename = "in")]
    pub fade_in: Option<f32>,
    /// Fade-out seconds at the very end (default 0).
    #[serde(default)]
    pub out: Option<f32>,
}

impl Describe for Fade {
    const BLURB: &'static str = "raised-cosine fade in and out";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "in": 0.01, "out": 0.2})
    }
}

impl Eval for Fade {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let mut ch = input.channels(cx.n);
        fade_edges(
            &mut ch,
            non_negative("in", self.fade_in.unwrap_or(0.0))?,
            non_negative("out", self.out.unwrap_or(0.0))?,
            cx.sr,
        );
        Ok(one(Buf::from_channels(ch)))
    }
}

/// The sound.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub input: In,
    /// Gain first, dB (default 0).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Then normalize to this integrated loudness, LUFS (default: leave).
    /// A very peaky or short sound cannot reach a high target under the
    /// ceiling: use `peak_db` for it. Not together with `peak_db`.
    #[serde(default)]
    pub lufs: Option<f32>,
    /// Or normalize the pre-limiter sample peak to this, dBFS (clicks,
    /// ticks, short hits; default: leave). Not together with `lufs`; not
    /// above `ceiling_db`. `gain_db` is added on top, as it is for `lufs`.
    #[serde(default)]
    pub peak_db: Option<f32>,
    /// Edge fades, seconds (defaults 0.002 in, 0.005 out; 0 for a loop).
    /// A hit that starts inside the fade-in is cut by it: set `fade_in`
    /// 0.0005 (0.5 ms; zero at sample 0, so `silent_edges` passes). The
    /// `lufs` / `peak_db` target is met after the fades.
    #[serde(default)]
    pub fade_in: Option<f32>,
    #[serde(default)]
    pub fade_out: Option<f32>,
    /// Lookahead limiter ceiling, dBFS (default -1).
    #[serde(default)]
    pub ceiling_db: Option<f32>,
}

/// The output stage in its three steps, kept apart for the warnings.
pub struct Staged {
    /// After gain and normalization, before the limiter.
    pub pre_limiter: Vec<Vec<f32>>,
    /// After the limiter, before the edge fades.
    pub limited: Vec<Vec<f32>>,
    /// Set when the target was measured on the faded signal: the limiter then
    /// ran on the faded signal (a hit the fade cut must not drive it), and
    /// this is the finished output, fades included.
    pub delivered: Option<Vec<Vec<f32>>>,
    /// Edge fades in seconds, as applied.
    pub fade_in: f32,
    pub fade_out: f32,
}

fn peak_of(ch: &[Vec<f32>]) -> f32 {
    ch.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()))
}

impl Output {
    /// Gain, normalization and limiter over a whole buffer, no fades.
    pub fn stage(&self, mut ch: Vec<Vec<f32>>, sr: f32, looped: bool) -> Result<Staged, String> {
        if self.lufs.is_some() && self.peak_db.is_some() {
            return Err(
                "lufs and peak_db are both set: pick one (lufs for sustained sounds, peak_db for clicks, ticks and short hits)"
                    .into(),
            );
        }
        let ceiling_db = self.ceiling_db.unwrap_or(-1.0);
        if let Some(p) = self.peak_db {
            if !p.is_finite() {
                return Err(format!("peak_db: {p}: give a level in dBFS, e.g. -3"));
            }
            if p > ceiling_db {
                return Err(format!(
                    "peak_db: {p} is above ceiling_db {ceiling_db}: the limiter would take it back; lower peak_db or raise ceiling_db"
                ));
            }
        }
        let g = db_to_gain(self.gain_db.unwrap_or(0.0));
        let (fi, fo) = if looped { (0.0, 0.0) } else { (0.002, 0.005) };
        let fade_in = non_negative("fade_in", self.fade_in.unwrap_or(fi))?;
        let fade_out = non_negative("fade_out", self.fade_out.unwrap_or(fo))?;
        let mut use_faded = false;
        if self.lufs.is_some() || self.peak_db.is_some() {
            // The target is met AFTER the edge fades: a hit the fade-in
            // cuts must not set a peak that is then never delivered. The
            // faded measure is used only when it moves the gain by more
            // than a threshold (0.02 dB for a peak, 0.1 dB for loudness, so
            // sounds the fades barely touch keep their exact gain), and
            // never by more than +12 dB (a hit lying wholly inside the fade
            // is the fade-in warning's job, not a reason to boost its tail).
            let g_plain = normalize_gain(&ch, self.peak_db, self.lufs, sr)?;
            let mut faded = ch.clone();
            fade_edges(&mut faded, fade_in, fade_out, sr);
            let g2 = if peak_of(&faded) > 1e-9 {
                let g_faded = normalize_gain(&faded, self.peak_db, self.lufs, sr)?;
                let moved = 20.0 * (g_faded / g_plain.max(1e-30)).log10();
                let threshold = if self.peak_db.is_some() { 0.02 } else { 0.1 };
                if moved.abs() > threshold {
                    use_faded = true;
                    g_faded.min(g_plain * db_to_gain(12.0))
                } else {
                    g_plain
                }
            } else {
                g_plain
            };
            for v in ch.iter_mut().flatten() {
                *v *= g * g2;
            }
        } else if g != 1.0 {
            for v in ch.iter_mut().flatten() {
                *v *= g;
            }
        }
        let lim = |c: Vec<Vec<f32>>| super::nonlinear::limit(c, db_to_gain(ceiling_db), 0.05, sr);
        let limited = lim(ch.clone());
        let delivered = use_faded.then(|| {
            let mut f = ch.clone();
            fade_edges(&mut f, fade_in, fade_out, sr);
            lim(f)
        });
        Ok(Staged {
            pre_limiter: ch,
            limited,
            delivered,
            fade_in,
            fade_out,
        })
    }

    /// The output stage over a whole buffer (also re-run by the renderer
    /// after the auto-duration length is found).
    pub fn process(
        &self,
        ch: Vec<Vec<f32>>,
        sr: f32,
        looped: bool,
    ) -> Result<Vec<Vec<f32>>, String> {
        let s = self.stage(ch, sr, looped)?;
        if let Some(done) = s.delivered {
            return Ok(done);
        }
        let mut ch = s.limited;
        fade_edges(&mut ch, s.fade_in, s.fade_out, sr);
        Ok(ch)
    }

    /// The warning for a `lufs` target the render missed, or None when it
    /// was met. `final_lufs` is the exported sound's loudness; `ch` is the
    /// output's input at the exported length.
    pub fn missed_target_warning(
        &self,
        ch: Vec<Vec<f32>>,
        sr: f32,
        looped: bool,
        final_lufs: f32,
    ) -> Option<String> {
        let target = self.lufs?;
        // The loudness gate floors the measure at -70: nothing to compare.
        if target <= -69.0 || (final_lufs - target).abs() <= 1.0 {
            return None;
        }
        let s = self.stage(ch, sr, looped).ok()?;
        let ceiling_db = self.ceiling_db.unwrap_or(-1.0);
        let db = |g: f32| 20.0 * g.max(1e-9).log10();
        let pre = db(peak_of(&s.pre_limiter));
        let post = db(peak_of(&s.limited));
        let n = s.pre_limiter.first().map_or(0, Vec::len).max(1) as f64;
        let energy: f64 = s
            .pre_limiter
            .iter()
            .flatten()
            .map(|x| f64::from(*x).powi(2))
            .sum::<f64>()
            / (n * s.pre_limiter.len().max(1) as f64);
        let rms = 10.0 * (energy.max(1e-18)).log10() as f32;
        let crest = pre - rms;
        let took = (pre - post).max(0.0);
        let head = format!(
            "output lufs {target} came out at {final_lufs:.1} LUFS (pre-limiter peak {pre:+.1} dBFS, post-limiter peak {post:+.1} dBFS, crest {crest:.1} dB, limiter removed {took:.1} dB)"
        );
        if took >= 0.5 {
            if crest >= 20.0 {
                return Some(format!(
                    "{head}: the sound is too short or too sparse to reach this loudness: use peak_db, or lower the target"
                ));
            }
            return Some(format!(
                "{head}: the limiter took {took:.1} dB off the peak (ceiling_db {ceiling_db}): lower the target, raise ceiling_db, or compress / shape before the output"
            ));
        }
        // The peak is not the reason: say what else changed the loudness.
        let lim_lufs = loudness::integrated(&s.limited, sr as u32);
        let fades = lim_lufs - final_lufs;
        if fades > 1.0 && (s.fade_in > 0.0 || s.fade_out > 0.0) {
            return Some(format!(
                "{head}: the edge fades (fade_in {} s, fade_out {} s) took {fades:.1} LU: shorten them (the sound is short) or raise the target",
                s.fade_in, s.fade_out
            ));
        }
        Some(format!(
            "{head}: the peak is {:.1} dB under its limit, so the peak did not cause it: the sound may be too short or too sparse to measure; use peak_db",
            ceiling_db - pre
        ))
    }

    /// A warning when the first hit starts inside the fade-in, or None.
    /// `ch` is the output's input at the exported length.
    pub fn fade_in_warning(&self, ch: Vec<Vec<f32>>, sr: f32, looped: bool) -> Option<String> {
        let s = self.stage(ch, sr, looped).ok()?;
        let a = ((s.fade_in * sr) as usize).min(s.limited.first().map_or(0, Vec::len));
        if a == 0 {
            return None;
        }
        let th = db_to_gain(-60.0);
        let n = s.limited[0].len();
        let first = (0..n).find(|&i| s.limited.iter().any(|c| c[i].abs() > th))?;
        if first >= a {
            return None;
        }
        // The first hit: from its start to a few ms past the fade.
        let end = (first + a.max((0.005 * sr) as usize)).min(n);
        let mut faded = s.limited.clone();
        fade_edges(&mut faded, s.fade_in, 0.0, sr);
        let hit =
            |c: &[Vec<f32>]| peak_of(&c.iter().map(|x| x[first..end].to_vec()).collect::<Vec<_>>());
        let (before, after) = (hit(&s.limited), hit(&faded));
        let cut = (20.0 * (before / after.max(1e-9)).log10()).min(60.0);
        if cut < 1.0 {
            return None;
        }
        let which = if self.fade_in.is_some() {
            "the"
        } else {
            "the default"
        };
        Some(format!(
            "{which} {:.0} ms fade-in is cutting the first hit by {cut:.1} dB (it starts at {:.1} ms): set fade_in 0.0005 (0.5 ms: it reaches zero at sample 0, so silent_edges still passes) or start the hit later",
            s.fade_in * 1000.0,
            first as f32 / sr * 1000.0
        ))
    }
}

impl Describe for Output {
    const BLURB: &'static str =
        "the sound: gain, loudness or peak target, edge fades and a ceiling limiter; exactly one";
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "mix1", "lufs": -16})
    }
}

impl Eval for Output {
    fn causal(&self) -> bool {
        self.lufs.is_none() && self.peak_db.is_none()
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("input", &self.input)?;
        let ch = self.process(input.channels(cx.n), cx.sr, cx.looped)?;
        Ok(one(Buf::from_channels(ch)))
    }
}
