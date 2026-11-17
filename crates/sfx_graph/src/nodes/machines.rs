//! Machines: `engine` (a piston engine from rpm) and `motor` (an electric
//! motor with a gearbox from rpm). Both are finished recipes: a few physical
//! fields in, tuned DSP inside. `rpm` is a signal, so a `ramp` or a `wander`
//! revs them and every internal frequency follows.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{mono, one, positive};
use crate::dsp::{DelayLine, FilterMode, OnePole, Svf};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Most revolutions a minute either node accepts.
const MAX_RPM: f32 = 20_000.0;
/// Speed of sound the exhaust pipe is tuned with, m/s.
const SOUND: f32 = 343.0;

/// A finite value clamped to `lo..=hi`; anything else is `lo`.
fn clean(x: f32, lo: f32, hi: f32) -> f32 {
    if x.is_finite() { x.clamp(lo, hi) } else { lo }
}

/// Gain 1 up to 0.4 x the sample rate, falling to 0 at 0.47 x: a partial
/// that would alias is faded out instead of folded back.
fn below_nyquist(hz: f32, sr: f32) -> f32 {
    ((0.47 * sr - hz) / (0.07 * sr)).clamp(0.0, 1.0)
}

// ------------------------------------------------------------------ engine

/// A piston engine: combustion pulses at the firing rate, an exhaust pipe
/// and an intake path, all following `rpm`. The pitch (the firing
/// frequency) is `rpm / 60 * cylinders / (stroke / 2)` Hz.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    /// Crankshaft speed, revolutions a minute. A number, or a node (a `ramp`
    /// to rev up). A number must be 0 to 20000; a signal is clamped to that
    /// range per sample. 0 is silence.
    pub rpm: In,
    /// Cylinders, a whole number from 1 to 16 (default 4). Each fires once
    /// per cycle, so the pitch is proportional to it. Each cylinder has its
    /// own seeded level error of up to 6 %.
    #[serde(default)]
    pub cylinders: Option<u32>,
    /// Throttle load, 0 to 1 (default 0.5). Up: each combustion pulse is
    /// stronger and shorter (more high frequencies), the intake noise opens
    /// and gets louder, the cycle-to-cycle level variation shrinks. A
    /// number outside 0..1 is an error; a signal is clamped per sample.
    #[serde(default)]
    pub load: Option<In>,
    /// Strokes a cycle, 2 or 4 (default 4). A two-stroke fires every
    /// revolution, so for the same cylinders and rpm it is twice the pitch.
    #[serde(default)]
    pub stroke: Option<u32>,
    /// Exhaust pipe length, metres (0.05 to 10, default 1.2). The pipe is
    /// open at one end, so it resonates at odd multiples of 343 / (4 length)
    /// Hz (1.2 m: 71, 214, 357 Hz ...): longer is lower.
    #[serde(default)]
    pub exhaust_length: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// cylinder level errors, and the cycle-to-cycle jitter (timing of up to
    /// 2 % of a firing period, level) and the intake noise.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Engine {
    const BLURB: &'static str = "piston engine from rpm: firing pulses at rpm/60 x cylinders/(stroke/2) Hz, exhaust pipe resonator, intake noise; rpm and load are signals";
    fn example() -> serde_json::Value {
        serde_json::json!({"rpm": 3000, "cylinders": 4, "load": 0.5, "stroke": 4, "exhaust_length": 1.2, "seed": 1})
    }
}

/// One combustion pulse: a raised cosine on a skewed time axis (fast rise,
/// slower fall) from `start` samples, `width` long, scaled by `amp`.
struct Pulse {
    start: f32,
    width: f32,
    amp: f32,
}

impl Eval for Engine {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rpm = cx.mono("rpm", &self.rpm)?;
        let load = cx.mono_or("load", &self.load, 0.5)?;
        let cylinders = self.cylinders.unwrap_or(4);
        if !(1..=16).contains(&cylinders) {
            return Err(format!(
                "cylinders: {cylinders}: give a whole number from 1 to 16"
            ));
        }
        let stroke = self.stroke.unwrap_or(4);
        if stroke != 2 && stroke != 4 {
            return Err(format!("stroke: {stroke}: give 2 or 4"));
        }
        let length = positive("exhaust_length", self.exhaust_length.unwrap_or(1.2))?;
        if !(0.05..=10.0).contains(&length) {
            return Err(format!(
                "exhaust_length: {length}: give metres from 0.05 to 10"
            ));
        }
        if let Some(r) = rpm.constant()
            && !(0.0..=MAX_RPM).contains(&r)
        {
            return Err(format!(
                "rpm: {r}: give a number from 0 to {MAX_RPM} (a signal is clamped to that range)"
            ));
        }
        if let Some(l) = load.constant()
            && !(0.0..=1.0).contains(&l)
        {
            return Err(format!("load: {l}: give a number from 0 to 1"));
        }

        // Pass 1: when each cylinder fires. A firing is `stroke / 2 /
        // cylinders` revolutions after the last, so the phase advances by
        // rpm / 60 / that a second. At most a quarter of a firing a sample
        // (the cap is never reached with 16 cylinders at 20000 rpm).
        let rev_per_fire = stroke as f32 / 2.0 / cylinders as f32;
        let mut rng = cx.rng(self.seed);
        let level_error: Vec<f32> = (0..cylinders).map(|_| 1.0 + 0.06 * rng.signed()).collect();
        let mut phase = 0.5f64;
        let mut count = 0usize;
        let mut pulses: Vec<Pulse> = Vec::new();
        for i in 0..cx.n {
            let hz = clean(rpm.at(i), 0.0, MAX_RPM) / 60.0 / rev_per_fire;
            let inc = f64::from(hz / cx.sr).min(0.25);
            if inc <= 0.0 {
                continue;
            }
            let before = phase;
            phase += inc;
            if phase < 1.0 {
                continue;
            }
            phase -= 1.0;
            let l = clean(load.at(i), 0.0, 1.0);
            let period = (1.0 / inc) as f32;
            let at = i as f32 + ((1.0 - before) / inc) as f32 + 0.02 * period * rng.signed();
            let wobble = 0.04 + 0.12 * (1.0 - l);
            let amp = level_error[count % cylinders as usize]
                * (1.0 + wobble * rng.signed())
                * (0.25 + 0.75 * l);
            // The valve is open for a fixed part of a revolution (0.30 rev at
            // no load down to 0.12 rev at full load), never longer than one
            // firing period or a quarter second.
            let width = (period * rev_per_fire * (0.30 - 0.18 * l))
                .min(period)
                .min(0.25 * cx.sr)
                .max(4.0);
            pulses.push(Pulse {
                start: at.max(0.0),
                width,
                amp,
            });
            count += 1;
        }

        // Pass 2: sum the pulses.
        let mut drive = vec![0.0f32; cx.n];
        for p in &pulses {
            let from = p.start.ceil() as usize;
            let to = ((p.start + p.width).ceil() as usize).min(cx.n);
            for (j, d) in drive.iter_mut().enumerate().take(to).skip(from) {
                let x = ((j as f32 - p.start) / p.width).clamp(0.0, 1.0);
                *d += p.amp * 0.5 * (1.0 - (TAU * x.powf(0.6)).cos());
            }
        }

        // Pass 3: the exhaust pipe (an inverting, low-passed comb: a pipe
        // open at one end) and the intake.
        let delay_samples = 2.0 * length / SOUND * cx.sr;
        let mut line = DelayLine::new(delay_samples.ceil() as usize + 4);
        let mut loop_lp = OnePole::default();
        let mut dc = OnePole::default();
        let mut intake_lp = Svf::default();
        let mut noise = cx.rng(self.seed ^ 0x1A7A_CE00_D15C_0001);
        Ok(one(mono(cx.n, |i| {
            let l = clean(load.at(i), 0.0, 1.0);
            let r = clean(rpm.at(i), 0.0, MAX_RPM);
            let x = dc.tick(drive[i], 20.0, cx.sr).1;
            let tail = loop_lp.tick(line.read(delay_samples), 2500.0, cx.sr).0;
            let y = x - 0.6 * tail;
            line.write(y);
            let hiss = noise.signed();
            let intake = intake_lp.tick(hiss, FilterMode::Lp, 300.0 + 2500.0 * l, 0.7, 0.0, cx.sr)
                * (0.01 + 0.12 * l)
                * (r / (r + 1500.0))
                * (0.6 + 0.4 * drive[i].clamp(0.0, 1.0));
            (0.5 * y + intake).tanh()
        })))
    }
}

// ------------------------------------------------------------------- motor

/// An electric motor with a gearbox: the electrical hum (harmonics of the
/// pole-pair rate), the PWM drive's whine with sidebands, the gear-mesh tone
/// and brush noise, all following `rpm`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Motor {
    /// Rotor speed, revolutions a minute. A number, or a node. A number
    /// must be 0 to 20000; a signal is clamped to that range. 0 is silence.
    pub rpm: In,
    /// Magnetic poles, an even number from 2 to 64 (default 4). The
    /// electrical hum is at `rpm / 60 * poles / 2` Hz with harmonics 2, 3, 4.
    #[serde(default)]
    pub poles: Option<u32>,
    /// Teeth on the gear that meshes at the shaft speed, 0 to 400 (default 0:
    /// no gearbox). The mesh tone is at `rpm / 60 * gear_teeth` Hz with a
    /// second harmonic and sidebands at the shaft speed. Partials above 0.47
    /// x the sample rate are faded out.
    #[serde(default)]
    pub gear_teeth: Option<u32>,
    /// Load, 0 to 1 (default 0.3). Up: the hum, mesh tone, whine and brush
    /// noise get louder. A number outside 0..1 is an error; a signal is
    /// clamped per sample.
    #[serde(default)]
    pub load: Option<In>,
    /// PWM whine level, 0 to 1 (default 0.3): a carrier at `carrier` Hz with
    /// sidebands at 2 and 4 times the electrical rate. 0 is none.
    #[serde(default)]
    pub whine: Option<In>,
    /// PWM carrier frequency, Hz (200 to 0.45 x sample rate, default 4000).
    #[serde(default)]
    pub carrier: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// brush noise.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Motor {
    const BLURB: &'static str = "electric motor with gearbox from rpm: pole-rate hum and harmonics, PWM whine with sidebands, gear-mesh tone at rpm/60 x gear_teeth Hz, brush noise";
    fn example() -> serde_json::Value {
        serde_json::json!({"rpm": 3000, "poles": 4, "gear_teeth": 20, "load": 0.3, "whine": 0.3, "carrier": 4000, "seed": 1})
    }
}

impl Eval for Motor {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rpm = cx.mono("rpm", &self.rpm)?;
        let load = cx.mono_or("load", &self.load, 0.3)?;
        let whine = cx.mono_or("whine", &self.whine, 0.3)?;
        let poles = self.poles.unwrap_or(4);
        if !(2..=64).contains(&poles) || !poles.is_multiple_of(2) {
            return Err(format!("poles: {poles}: give an even number from 2 to 64"));
        }
        let teeth = self.gear_teeth.unwrap_or(0);
        if teeth > 400 {
            return Err(format!("gear_teeth: {teeth}: give 0 to 400 teeth"));
        }
        let carrier = self.carrier.unwrap_or(4000.0);
        if !carrier.is_finite() || carrier < 200.0 || carrier > 0.45 * cx.sr {
            return Err(format!(
                "carrier: {carrier}: give Hz from 200 to {} (0.45 x the sample rate)",
                0.45 * cx.sr
            ));
        }
        if let Some(r) = rpm.constant()
            && !(0.0..=MAX_RPM).contains(&r)
        {
            return Err(format!(
                "rpm: {r}: give a number from 0 to {MAX_RPM} (a signal is clamped to that range)"
            ));
        }
        for (field, sig) in [("load", &load), ("whine", &whine)] {
            if let Some(v) = sig.constant()
                && !(0.0..=1.0).contains(&v)
            {
                return Err(format!("{field}: {v}: give a number from 0 to 1"));
            }
        }

        let sr = f64::from(cx.sr);
        let (mut rotor, mut elec, mut mesh, mut pwm) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut noise = cx.rng(self.seed);
        let mut brush_hp = Svf::default();
        let carrier_hz = carrier;
        let tone = |ph: f64, k: f32| (TAU * (ph * f64::from(k)).fract() as f32).sin();
        Ok(one(mono(cx.n, |i| {
            let r = clean(rpm.at(i), 0.0, MAX_RPM);
            let l = clean(load.at(i), 0.0, 1.0);
            let w = clean(whine.at(i), 0.0, 1.0);
            let shaft = r / 60.0;
            let f_elec = shaft * poles as f32 / 2.0;
            let f_mesh = shaft * teeth as f32;
            rotor = (rotor + f64::from(shaft) / sr).fract();
            elec = (elec + f64::from(f_elec) / sr).fract();
            mesh = (mesh + f64::from(f_mesh) / sr).fract();
            pwm = (pwm + f64::from(carrier_hz) / sr).fract();
            let speed = r / (r + 500.0);
            let hum = 0.7 + 0.3 * l;
            let torque = 0.3 + 0.7 * l;

            // The hum: the pole-pair rate and its first harmonics, and the
            // rotor's own once-a-turn tone.
            let mut s = 0.06 * tone(rotor, 1.0);
            for (k, a) in [(1.0f32, 0.22f32), (2.0, 0.12), (3.0, 0.08), (4.0, 0.04)] {
                s += a * hum * below_nyquist(k * f_elec, cx.sr) * tone(elec, k);
            }
            // The gear mesh, its second harmonic, and a shaft-rate
            // modulation (sidebands at mesh +- shaft speed).
            if teeth > 0 {
                let a = 0.60 * torque;
                let wobble = 1.0 + 0.3 * tone(rotor, 1.0);
                s += a * below_nyquist(f_mesh, cx.sr) * wobble * tone(mesh, 1.0);
                s += 0.4 * a * below_nyquist(2.0 * f_mesh, cx.sr) * tone(mesh, 2.0);
            }
            // The PWM whine: a fixed carrier modulated at 2 and 4 times the
            // electrical rate.
            let ripple = 1.0
                + 0.8 * below_nyquist(2.0 * f_elec, cx.sr) * tone(elec, 2.0)
                + 0.4 * below_nyquist(4.0 * f_elec, cx.sr) * tone(elec, 4.0);
            s += 0.12 * w * torque * ripple * tone(pwm, 1.0);
            // Brush noise.
            let hiss = brush_hp.tick(noise.signed(), FilterMode::Hp, 1500.0, 0.7, 0.0, cx.sr);
            s += 0.05 * (0.3 + 0.7 * l) * hiss;
            0.5 * speed * s
        })))
    }
}
