//! `camera`: where the microphone of a video or film shot stands relative to
//! the source. Distance (1/r level, air absorption, propagation delay), a
//! ground reflection and a `space` preset built on the `reverb` room model
//! and the feedback delay network. Game sounds stay dry (the engine adds the
//! room); `camera` is the path for a render that has no engine after it.
//!
//! Tests: `crates/sfx_analysis/tests/it/space.rs` (`mod camera`). Docs:
//! `docs/nodes/camera.md`.

use serde::Deserialize;

use super::one;
use super::space::room_wet;
use super::time::fdn_tail;
use crate::buf::{Buf, Sig};
use crate::dsp::{DelayLine, OnePole, flush};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound, m/s (20 C).
const C: f32 = 343.0;

/// The space around source and camera.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CameraSpace {
    /// No space: the direct sound (and the ground reflection) only.
    #[default]
    None,
    /// Open ground: two late, dull slaps from far buildings or trees and a
    /// faint short tail.
    Outdoor,
    /// A street between buildings: dense flutter between the facades, a
    /// tail under a second.
    Street,
    /// A furnished room about 5 m long: early reflections, 0.6 s tail.
    Room,
    /// A hall about 18 m long: 2.2 s tail.
    Hall,
}

/// A camera (a microphone at a distance) on a mono source.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub input: In,
    /// Metres from the source to the camera (a number, or a signal for a
    /// moving source). Left out: no distance processing.
    #[serde(default)]
    pub distance: Option<In>,
    /// The distance (m) the source was designed at: there the level is
    /// unchanged (default 1).
    #[serde(default)]
    pub ref_distance: Option<f32>,
    /// Apply the 1/r level (-6 dB per doubling past `ref_distance`; default
    /// true). An output `peak_db` / `lufs` target renormalizes, undoing it:
    /// keep the target off (`gain_db` only) to hear the distance as level.
    #[serde(default)]
    pub inverse_law: Option<bool>,
    /// Air absorption by distance (ISO 9613-1, 20 C; default true).
    #[serde(default)]
    pub air: Option<bool>,
    /// Relative humidity, percent (default 50): drier air loses more highs.
    #[serde(default)]
    pub humidity: Option<f32>,
    /// Delay the sound by distance / 343 m/s (default false). With a moving
    /// distance this gives the Doppler shift.
    #[serde(default)]
    pub delay: Option<bool>,
    /// Source height above the ground, m. With `camera_height`, adds the
    /// ground reflection (a comb from the path difference).
    #[serde(default)]
    pub source_height: Option<f32>,
    /// Camera height above the ground, m (default 1.6 when `source_height`
    /// is given).
    #[serde(default)]
    pub camera_height: Option<f32>,
    /// The ground's pressure reflection, 0 (absorbing) .. 1 (hard; default
    /// 0.8: asphalt; grass about 0.5).
    #[serde(default)]
    pub ground: Option<f32>,
    /// none (default), outdoor, street, room, hall.
    #[serde(default)]
    pub space: CameraSpace,
    /// Level of the space against the direct sound at `ref_distance`, dB
    /// (default 0: the preset's own). The space does not fall with
    /// distance, so the farther camera hears more room.
    #[serde(default)]
    pub space_db: Option<f32>,
    /// Stereo output when a space is set (default true); false folds to
    /// mono. Without a space the output is mono.
    #[serde(default)]
    pub stereo: Option<bool>,
}

impl Describe for Camera {
    const BLURB: &'static str = "the camera's distance for a video / film render: 1/r level, air absorption, delay, ground reflection, space none / outdoor / street / room / hall (game sounds stay dry)";
    const WORDS: &'static [&'static str] = &["space"];
    fn example() -> serde_json::Value {
        serde_json::json!({"input": "src", "distance": 20, "source_height": 0.5, "space": "outdoor"})
    }
}

/// ISO 9613-1 pure-tone air absorption at 20 C and 1 atm, dB per metre.
pub(crate) fn air_db_per_m(f: f32, humidity: f32) -> f32 {
    let t = 293.15f64;
    let t0 = 293.15f64;
    let t01 = 273.16f64;
    let f = f as f64;
    let psat = 10f64.powf(-6.8346 * (t01 / t).powf(1.261) + 4.6151);
    let h = humidity as f64 * psat;
    let fro = 24.0 + 4.04e4 * h * (0.02 + h) / (0.391 + h);
    let frn = (t / t0).powf(-0.5)
        * (9.0 + 280.0 * h * (-4.170 * ((t / t0).powf(-1.0 / 3.0) - 1.0)).exp());
    let a = 8.686
        * f
        * f
        * (1.84e-11 * (t / t0).sqrt()
            + (t / t0).powf(-2.5)
                * (0.01275 * (-2239.1 / t).exp() / (fro + f * f / fro)
                    + 0.1068 * (-3352.0 / t).exp() / (frn + f * f / frn)));
    a as f32
}

/// The cutoff of a two-pole (two one-pole) low-pass whose 6 dB point
/// matches the air's 6 dB loss over `d` metres. Above 20 kHz the air does
/// nothing audible: `None`.
fn air_cutoff(d: f32, humidity: f32) -> Option<f32> {
    if d <= 0.0 || air_db_per_m(20_000.0, humidity) * d < 6.0 {
        return None;
    }
    let (mut lo, mut hi) = (20.0f32, 20_000.0f32);
    for _ in 0..30 {
        let mid = (lo * hi).sqrt();
        if air_db_per_m(mid, humidity) * d > 6.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some((lo * hi).sqrt())
}

/// The air low-pass: cutoff updated every 32 samples from the distance.
struct Air {
    lp: [OnePole; 2],
    hz: Option<f32>,
}

impl Air {
    fn tick(&mut self, x: f32, sr: f32) -> f32 {
        match self.hz {
            Some(hz) => {
                let a = self.lp[0].tick(x, hz, sr).0;
                self.lp[1].tick(a, hz, sr).0
            }
            None => x,
        }
    }
}

impl Camera {
    fn check(&self) -> Result<(), String> {
        let pos = |f: &str, v: Option<f32>| match v {
            Some(x) if !(x.is_finite() && x > 0.0) => Err(format!("{f}: {x}: give a number > 0")),
            _ => Ok(()),
        };
        pos("ref_distance", self.ref_distance)?;
        if let Some(h) = self.humidity
            && !(1.0..=100.0).contains(&h)
        {
            return Err(format!("humidity: {h}: give a percentage from 1 to 100"));
        }
        for (f, v) in [
            ("source_height", self.source_height),
            ("camera_height", self.camera_height),
        ] {
            if let Some(x) = v
                && !(x.is_finite() && x >= 0.0)
            {
                return Err(format!("{f}: {x}: give a height >= 0 m"));
            }
        }
        if self.camera_height.is_some() && self.source_height.is_none() {
            return Err("camera_height: the ground reflection needs source_height too".into());
        }
        if let Some(g) = self.ground
            && !(0.0..=1.0).contains(&g)
        {
            return Err(format!("ground: {g}: give a reflection from 0 to 1"));
        }
        if (self.ground.is_some()) && self.source_height.is_none() {
            return Err("ground: the ground reflection needs source_height".into());
        }
        if self.distance.is_none()
            && (self.inverse_law.is_some()
                || self.air.is_some()
                || self.delay.is_some()
                || self.humidity.is_some())
        {
            return Err("distance: inverse_law, air, delay and humidity need a distance".into());
        }
        if self.source_height.is_some() && self.distance.is_none() {
            return Err("distance: the ground reflection needs a distance".into());
        }
        if self.space == CameraSpace::None && (self.space_db.is_some() || self.stereo.is_some()) {
            return Err(
                "space: space_db and stereo need a space (outdoor, street, room, hall)".into(),
            );
        }
        Ok(())
    }

    /// The preset's wet signal for the dry source at the reference level.
    fn space_wet(&self, x: &[f32], sr: f32) -> Option<Vec<[f32; 2]>> {
        let pre = (0.005 * sr) as usize;
        let mut wet = match self.space {
            CameraSpace::None => return None,
            CameraSpace::Outdoor => {
                // Two slaps (a facade 40 m off, trees 70 m off), dull from
                // the air and the far surface, and a faint short tail.
                let tail = fdn_tail(x, (0.06 * sr) as usize, 0.9, 0.5, 0.6, sr);
                let mut w: Vec<[f32; 2]> =
                    tail.iter().map(|t| [t[0] * 0.15, t[1] * 0.15]).collect();
                for (t, g, pan) in [(0.233f32, 0.18f32, 0.3f32), (0.41, 0.1, -0.4)] {
                    let d = (t * sr) as usize;
                    let mut lp = OnePole::default();
                    for i in d..x.len() {
                        let y = lp.tick(x[i - d], 2500.0, sr).0 * g;
                        w[i][0] += y * (1.0 - pan);
                        w[i][1] += y * (1.0 + pan);
                    }
                }
                w
            }
            CameraSpace::Street => room_wet(x, pre, 0.75, 0.9, 0.35, 0.6, sr),
            CameraSpace::Room => room_wet(x, pre, 0.12, 0.6, 0.4, 0.4, sr),
            CameraSpace::Hall => room_wet(x, pre, 0.88, 2.2, 0.3, 0.3, sr),
        };
        let level = match self.space {
            CameraSpace::Outdoor => 1.0,
            CameraSpace::Street => 0.35,
            CameraSpace::Room => 0.4,
            _ => 0.45,
        } * 10f32.powf(self.space_db.unwrap_or(0.0) / 20.0);
        for s in &mut wet {
            s[0] *= level;
            s[1] *= level;
        }
        Some(wet)
    }
}

impl Eval for Camera {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        self.check()?;
        let input = cx.mono("input", &self.input)?;
        let off = self.distance.is_none() && self.space == CameraSpace::None;
        if off {
            // Bit-identical passthrough.
            return Ok(one(match &input {
                Sig::Buf(b) => (**b).clone(),
                s => Buf::Audio((0..cx.n).map(|i| s.at(i)).collect()),
            }));
        }
        let n = cx.n;
        let sr = cx.sr;
        let x: Vec<f32> = (0..n).map(|i| input.at(i)).collect();
        let r0 = self.ref_distance.unwrap_or(1.0);
        let mut direct = x.clone();
        let mut dist_at = vec![r0; n];
        if let Some(d) = &self.distance {
            let dsig = cx.mono("distance", d)?;
            for (i, v) in dist_at.iter_mut().enumerate() {
                let di = dsig.at(i);
                if !(di.is_finite() && di >= 0.0) {
                    return Err(format!(
                        "distance: {di} m at {:.3} s: give metres >= 0",
                        i as f32 / sr
                    ));
                }
                *v = di.max(0.01);
            }
            let max_d = dist_at.iter().copied().fold(0.0f32, f32::max);
            let law = self.inverse_law.unwrap_or(true);
            let air = self.air.unwrap_or(true);
            let hum = self.humidity.unwrap_or(50.0);
            let ground = self.source_height.map(|hs| {
                (
                    hs,
                    self.camera_height.unwrap_or(1.6),
                    self.ground.unwrap_or(0.8),
                )
            });
            let delay = self.delay.unwrap_or(false);
            // One line holds the ground's extra path and the propagation.
            let extra = ground.map(|(hs, hl, _)| hs + hl).unwrap_or(0.0);
            let max_samples = ((max_d + extra) / C * sr) as usize + 8;
            let mut line = DelayLine::new(if delay || ground.is_some() {
                max_samples
            } else {
                4
            });
            let mut air_d = Air {
                lp: Default::default(),
                hz: None,
            };
            let mut air_g = Air {
                lp: Default::default(),
                hz: None,
            };
            for i in 0..n {
                let d = dist_at[i];
                line.write(x[i]);
                let (direct_r, refl) = match ground {
                    Some((hs, hl, g)) => {
                        let r1 = (d * d + (hs - hl).powi(2)).sqrt();
                        let r2 = (d * d + (hs + hl).powi(2)).sqrt();
                        (r1, Some((r2, g)))
                    }
                    None => (d, None),
                };
                if i % 32 == 0 && air {
                    air_d.hz = air_cutoff(direct_r, hum);
                    air_g.hz = refl.and_then(|(r2, _)| air_cutoff(r2, hum));
                }
                let base = if delay { direct_r / C * sr } else { 0.0 };
                let mut y = if delay { line.read(base) } else { x[i] };
                y = air_d.tick(y, sr);
                let gain = |r: f32| if law { r0 / r.max(1e-3) } else { 1.0 };
                y *= gain(direct_r);
                if let Some((r2, g)) = refl {
                    let dt = base + (r2 - direct_r) / C * sr;
                    let z = air_g.tick(line.read(dt), sr);
                    // Pressure: 1/r of each path; without the law the
                    // reflection keeps its ratio to the direct sound.
                    let rel = direct_r / r2;
                    y += z * g * rel * gain(direct_r);
                }
                direct[i] = flush(y);
            }
        }
        let wet = self.space_wet(&x, sr);
        let Some(mut wet) = wet else {
            return Ok(one(Buf::Audio(direct)));
        };
        // The space is a diffuse field: it does not fall with distance. With
        // the law off the direct sound keeps its level, so the room rises
        // instead, keeping the direct-to-room ratio.
        let law = self.inverse_law.unwrap_or(true) && self.distance.is_some();
        for (i, s) in wet.iter_mut().enumerate() {
            let k = if law { 1.0 } else { dist_at[i] / r0 };
            s[0] = direct[i] + s[0] * k;
            s[1] = direct[i] + s[1] * k;
        }
        if self.stereo.unwrap_or(true) {
            Ok(one(Buf::Stereo(wet)))
        } else {
            Ok(one(Buf::Audio(
                wet.iter().map(|s| 0.5 * (s[0] + s[1])).collect(),
            )))
        }
    }
}
