//! Waveguide bodies for `rig`: a `string` or a `tube` as a digital
//! waveguide (two delay lines of velocity waves with reflections at the
//! ends), stepped once per rig substep. Joints read and push it at a point
//! along it (0..1); an `air` joint couples a body's velocity into one end of
//! a tube and the air's force back onto the body.

use serde::Deserialize;

use crate::dsp::flush;

/// Speed of sound in the tube, m/s (as `tube`).
const C: f32 = 343.0;
/// Density of air, kg/m^3.
const RHO: f32 = 1.2;
/// The longest delay line, substeps (work cap).
const MAX_LINE: usize = 1 << 20;

/// Which waveguide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WaveguideKind {
    /// A string fixed at both ends.
    #[default]
    String,
    /// An air column.
    Tube,
}

/// The ends of a tube: `open` (both open, c / 2L), `closed` (closed at 0,
/// open at 1: c / 4L), `stopped` (both closed, c / 2L).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TubeEnds {
    #[default]
    Open,
    Closed,
    Stopped,
}

/// A rig body that is a waveguide (`waveguide` on a body; the body's `mass`
/// is the string's mass, and a tube ignores it). It does not translate.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RigWaveguide {
    /// string (default) or tube.
    #[serde(default)]
    pub kind: WaveguideKind,
    /// Lowest frequency, Hz (a string: or `length` with `tension`).
    #[serde(default)]
    pub f0: Option<f32>,
    /// Length, metres (a tube: tunes it; a string: with `tension`).
    #[serde(default)]
    pub length: Option<f32>,
    /// A string's tension, newtons (with `length`).
    #[serde(default)]
    pub tension: Option<f32>,
    /// A tube's ends: open (default), closed, stopped.
    #[serde(default)]
    pub ends: TubeEnds,
    /// A tube's radius, metres (default 0.02).
    #[serde(default)]
    pub radius: Option<f32>,
    /// Seconds for the lowest frequency to fall 60 dB (default: string 3,
    /// tube 0.8).
    #[serde(default)]
    pub decay: Option<f32>,
    /// Loss of the highs at each round trip, 0..1 (default 0.3): 0 bright,
    /// 1 dull.
    #[serde(default)]
    pub damping: Option<f32>,
}

/// A waveguide while it runs.
pub(super) struct WgBody {
    kind: WaveguideKind,
    f0: f32,
    z: f32,
    refl0: f32,
    refl1: f32,
    t60: f32,
    lp_hz: f32,
    m: usize,
    right: Vec<f32>,
    left: Vec<f32>,
    h: usize,
    g: usize,
    p: f32,
    gain: f32,
    lp: f32,
    inj0: f32,
    dc: (f32, f32),
    taps: Vec<(f32, usize, f32)>,
}

impl RigWaveguide {
    /// Checks it; `mass` is the body's.
    pub(super) fn live(&self, name: &str, mass: f32) -> Result<WgBody, String> {
        let bad = |what: &str| format!("body `{name}`: waveguide {what}");
        let pos = |x: Option<f32>, what: &str, lo: f32, hi: f32| -> Result<Option<f32>, String> {
            match x {
                Some(v) if !(v.is_finite() && (lo..=hi).contains(&v)) => {
                    Err(bad(&format!("{what} {v}: give {lo}..{hi}")))
                }
                v => Ok(v),
            }
        };
        let f0 = pos(self.f0, "f0", 20.0, 8000.0)?;
        let length = pos(self.length, "length", 0.01, 20.0)?;
        let tension = pos(self.tension, "tension", 0.1, 1e5)?;
        let radius = pos(self.radius, "radius", 0.001, 1.0)?.unwrap_or(0.02);
        let (f0, z, refl0, refl1) = match self.kind {
            WaveguideKind::String => {
                let f0 = match (f0, length, tension) {
                    (Some(f), None, None) => f,
                    (None, Some(l), Some(t)) => (t * l / mass).sqrt() / (2.0 * l),
                    _ => return Err(bad("string: give f0, or length with tension")),
                };
                // Z = sqrt(T mu) = mu c = 2 m f0 for a string of mass m.
                (f0, 2.0 * mass * f0, -1.0, -1.0)
            }
            WaveguideKind::Tube => {
                if tension.is_some() {
                    return Err(bad("tube: tension is a string's"));
                }
                let l = match (f0, length) {
                    (None, Some(l)) => l,
                    (Some(f), None) => match self.ends {
                        TubeEnds::Closed => C / (4.0 * f),
                        _ => C / (2.0 * f),
                    },
                    _ => return Err(bad("tube: give length or f0")),
                };
                // Velocity waves: an open end reflects them +1, a closed -1.
                let (r0, r1) = match self.ends {
                    TubeEnds::Open => (1.0, 1.0),
                    TubeEnds::Closed => (-1.0, 1.0),
                    TubeEnds::Stopped => (-1.0, -1.0),
                };
                let f0 = if r0 == r1 {
                    C / (2.0 * l)
                } else {
                    C / (4.0 * l)
                };
                let area = std::f32::consts::PI * radius * radius;
                (f0, RHO * C * area, r0, r1)
            }
        };
        if !(20.0..=8000.0).contains(&f0) {
            return Err(bad(&format!(
                "lowest frequency {f0:.1} Hz: keep it in 20..8000"
            )));
        }
        let default_decay = match self.kind {
            WaveguideKind::String => 3.0,
            WaveguideKind::Tube => 0.8,
        };
        let t60 = pos(self.decay, "decay", 0.01, 60.0)?.unwrap_or(default_decay);
        let damping = pos(self.damping, "damping", 0.0, 1.0)?.unwrap_or(0.3);
        Ok(WgBody {
            kind: self.kind,
            f0,
            z: z.max(1e-9),
            refl0,
            refl1,
            t60,
            lp_hz: 16_000.0 * 0.02f32.powf(damping),
            m: 0,
            right: Vec::new(),
            left: Vec::new(),
            h: 0,
            g: 0,
            p: 0.0,
            gain: 1.0,
            lp: 0.0,
            inj0: 0.0,
            dc: (0.0, 0.0),
            taps: Vec::new(),
        })
    }
}

impl WgBody {
    /// A point at `pos` (0..1) a joint reads and pushes; its index.
    pub(super) fn tap(&mut self, pos: f32) -> usize {
        self.taps.push((pos.clamp(0.0, 1.0), 0, 0.0));
        self.taps.len() - 1
    }

    /// The delay lines for substeps of `dt` seconds.
    pub(super) fn start(&mut self, dt: f32) -> Result<(), String> {
        let rate = 1.0 / dt;
        self.p = (-std::f32::consts::TAU * self.lp_hz.min(0.45 * rate) / rate).exp();
        // The loss filter's delay at low frequency, p / (1 - p) substeps.
        let filt = self.p / (1.0 - self.p);
        // One round trip is 2m substeps (a tube with unlike ends sounds an
        // octave below its round trip: the sign flips twice per period).
        let trip = match self.refl0 == self.refl1 {
            true => rate / self.f0,
            false => rate / (2.0 * self.f0),
        };
        let m = ((trip - filt) / 2.0).round().max(2.0) as usize;
        if m > MAX_LINE {
            return Err("waveguide too long for this oversample: raise f0".into());
        }
        self.m = m;
        self.right = vec![0.0; m];
        self.left = vec![0.0; m];
        self.gain = 10f32.powf(-3.0 / (self.t60 * self.f0));
        for t in &mut self.taps {
            t.1 = (t.0 * (m - 1) as f32).round() as usize;
        }
        Ok(())
    }

    #[inline]
    fn ri(&self, p: usize) -> usize {
        (self.h + self.m - p) % self.m
    }
    #[inline]
    fn li(&self, p: usize) -> usize {
        (self.g + 2 * self.m - 1 - p) % self.m
    }

    /// Displacement and velocity at tap `t`.
    pub(super) fn at(&self, t: usize) -> (f32, f32) {
        let (_, p, u) = self.taps[t];
        (u, self.right[self.ri(p)] + self.left[self.li(p)])
    }

    /// A force `f` (newtons, up) on the waveguide at tap `t` this substep.
    pub(super) fn push(&mut self, t: usize, f: f32) {
        let p = self.taps[t].1;
        let dv = f / (4.0 * self.z);
        let (r, l) = (self.ri(p), self.li(p));
        self.right[r] += dv;
        self.left[l] += dv;
    }

    /// The largest force that closes an overlap `d` in one substep `dt`.
    pub(super) fn give(&self, d: f32, dt: f32) -> f32 {
        (2.0 * self.z * d.max(0.0) / dt).max(0.0)
    }

    /// The air joint: a body moving at `v` m/s drives end 0 with coupling
    /// `s`; the force of the air back on it.
    pub(super) fn air(&mut self, v: f32, s: f32) -> f32 {
        let (r, l) = (self.right[self.ri(0)], self.left[self.li(0)]);
        self.inj0 += s * v;
        -s * self.z * (r - l)
    }

    /// One substep: the waves move one cell, reflect at the ends.
    pub(super) fn step(&mut self, dt: f32) {
        // Each tap moves with what is in its cell now, this substep's
        // pushes included (after the shift they have left it).
        for i in 0..self.taps.len() {
            let v = self.at(i).1;
            let t = &mut self.taps[i];
            t.2 = (t.2 + dt * v) * 0.99999;
        }
        let r_end = self.right[self.ri(self.m - 1)];
        let l_end = self.left[self.li(0)];
        self.lp = flush((1.0 - self.p) * r_end + self.p * self.lp);
        self.h = (self.h + 1) % self.m;
        self.g = (self.g + self.m - 1) % self.m;
        let (h, g) = (self.h, self.g);
        self.right[h] = self.refl0 * l_end + self.inj0;
        self.left[(g + self.m) % self.m] = self.refl1 * self.gain * self.lp;
        self.inj0 = 0.0;
    }

    /// What it radiates now: velocity at an open end, force at a fixed one
    /// (end 1), in velocity units.
    /// Called once per output sample; a DC blocker near 15 Hz takes out
    /// the steady flow an open tube carries.
    pub(super) fn radiate(&mut self) -> f32 {
        let p = self.m - 1;
        let (r, l) = (self.right[self.ri(p)], self.left[self.li(p)]);
        let x = if self.refl1 > 0.0 { r + l } else { r - l };
        self.dc.1 = flush(x - self.dc.0 + 0.998 * self.dc.1);
        self.dc.0 = x;
        self.dc.1
    }

    /// Is it a tube (the air joint's other side must be).
    pub(super) fn is_tube(&self) -> bool {
        self.kind == WaveguideKind::Tube
    }
}
