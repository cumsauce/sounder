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

/// A piston engine after Farnell (Designing Sound ch. 45): unevenly firing
/// cylinder pulses, transmission overtones, combustion noise and a warping
/// two-delay exhaust, all following `rpm`. The pitch (the firing frequency)
/// is `rpm / 60 * cylinders / (stroke / 2)` Hz.
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
    /// Throttle load, 0 to 1 (default 0.5). Up: each pulse is louder and
    /// narrower, the combustion noise louder, the saturation harder and the
    /// output low-pass higher (2 to 6 kHz). A number outside 0..1 is an
    /// error; a signal is clamped per sample.
    #[serde(default)]
    pub load: Option<In>,
    /// Strokes a cycle, 2 or 4 (default 4). A two-stroke fires every
    /// revolution, so for the same cylinders and rpm it is twice the pitch.
    #[serde(default)]
    pub stroke: Option<u32>,
    /// Exhaust pipe length, metres (0.05 to 10, default 1.2): the two
    /// exhaust delays sit at 0.87 and 1.33 x the round trip 2 length / 343 s
    /// (their times then wobble with the engine signal): longer is lower.
    #[serde(default)]
    pub exhaust_length: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): each
    /// cylinder's level error and timing and level jitter, and the noise.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Engine {
    const BLURB: &'static str = "piston engine from rpm (Farnell's car engine): uneven cylinder pulses at rpm/60 x cylinders/(stroke/2) Hz, transmission overtones, combustion noise, a warping two-delay exhaust, saturated; rpm and load are signals";
    fn example() -> serde_json::Value {
        serde_json::json!({"rpm": 3000, "cylinders": 4, "load": 0.5, "stroke": 4, "exhaust_length": 1.2, "seed": 1})
    }
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

        // After Farnell (Designing Sound ch. 45), preferred in an A/B (book
        // check 2): each cylinder is a 1 / (1 + k x^2) pulse on the crank
        // phase at its own offset, jittered by slow noise (uneven firing);
        // transmission overtones at 4, 8 and 16 x the cycle; combustion
        // noise riding the pulses; then a "warping" exhaust: two feedback
        // delays (the pipe and its return) whose times the engine signal
        // modulates, high-passed, saturated and low-passed.
        let cyc_per_rev = 2.0 / stroke as f32;
        let ncyl = cylinders as f32;
        let mut rng = cx.rng(self.seed);
        let level_error: Vec<f32> = (0..cylinders).map(|_| 1.0 + 0.06 * rng.signed()).collect();
        let jit_gain: Vec<f32> = (0..cylinders).map(|_| 0.02 * rng.signed()).collect();
        let lvl_gain: Vec<f32> = (0..cylinders).map(|_| 0.35 * rng.signed()).collect();
        let base = 2.0 * length / SOUND;
        let (d1, d2) = (0.87 * base, 1.33 * base);
        let mut l1 = DelayLine::new(((d1 * 1.3 + 0.002) * cx.sr).ceil() as usize + 4);
        let mut l2 = DelayLine::new(((d2 * 1.3 + 0.002) * cx.sr).ceil() as usize + 4);
        let (mut j1, mut j2) = (OnePole::default(), OnePole::default());
        let mut dc = OnePole::default();
        let mut cn = Svf::default();
        let (mut h1, mut h2) = (Svf::default(), Svf::default());
        let (mut o1, mut o2) = (Svf::default(), Svf::default());
        let mut noise = cx.rng(self.seed ^ 0x1A7A_CE00_D15C_0001);
        let mut phase = 0.0f64;
        let top = 0.36 * cx.sr;
        Ok(one(mono(cx.n, |i| {
            let l = clean(load.at(i), 0.0, 1.0);
            let r = clean(rpm.at(i), 0.0, MAX_RPM);
            phase = (phase + f64::from(r / 60.0 * cyc_per_rev / cx.sr)).fract();
            let p = phase as f32;
            let w = noise.signed();
            let j = 12.0 * j2.tick(j1.tick(w, 40.0, cx.sr).0, 40.0, cx.sr).0;
            let k = (150.0 + 300.0 * l) * (ncyl / 4.0).powi(2);
            let mut cyl = 0.0f32;
            for c in 0..cylinders as usize {
                let x = (p + c as f32 / ncyl + jit_gain[c] * j).rem_euclid(1.0) - 0.5;
                cyl += level_error[c] * (1.0 + lvl_gain[c] * j).max(0.2) / (1.0 + k * x * x);
            }
            let cyl = cyl * (0.25 + 0.75 * l) * 2.0 / ncyl - 0.1;
            let pulse4 = 1.0 / (1.0 + 40.0 * ((p * ncyl + 0.1).fract() - 0.5).powi(2));
            // Transmission overtones at 1, 2 and 4 x the firing rate.
            let ot = 0.05 * (TAU * (p * 4.0 * ncyl + 0.3).fract()).cos()
                + 0.2 * pulse4
                + 0.03 * (TAU * (p * 2.0 * ncyl + 0.7).fract()).cos();
            let comb = cn.tick(w, FilterMode::Bp, 1800.0, 0.7, 0.0, cx.sr) * cyl;
            let src = dc
                .tick(cyl + 0.6 * ot + (0.3 + 0.4 * l) * comb, 30.0, cx.sr)
                .1;
            let t1 = (d1 + 0.0015 * src).max(0.0005) * cx.sr;
            let t2 = (d2 - 0.0012 * src).max(0.0005) * cx.sr;
            let e1 = l1.read(t1);
            let e2 = l2.read(t2);
            l1.write(src + 0.55 * e1);
            l2.write(src + 0.5 * e2);
            let pipe = src + 0.8 * e1 + 0.8 * e2;
            let hp = h2.tick(
                h1.tick(pipe, FilterMode::Hp, 45.0, 0.7, 0.0, cx.sr),
                FilterMode::Hp,
                45.0,
                0.7,
                0.0,
                cx.sr,
            );
            let sat = (hp * (1.5 + 2.0 * l)).tanh();
            let tone = (2000.0 + 4000.0 * l).min(top);
            let a = o1.tick(sat, FilterMode::Lp, tone, 0.6, 0.0, cx.sr);
            // rpm 0 is silence; the level follows speed near standstill.
            0.6 * (r / 60.0).min(1.0) * o2.tick(a, FilterMode::Lp, tone, 0.6, 0.0, cx.sr)
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
    /// brush hiss and spark crackle, the bearing noise and the speed drift.
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
        // Speed wobble: white noise low-passed twice at 2 Hz and scaled so
        // the shaft speed drifts by about +-0.4 %, plus a 0.15 % once-a-turn
        // eccentricity. Every line follows it, so none is a perfect sine.
        let mut drift_rng = cx.rng(self.seed ^ 0xD81F_7000_0000_0004);
        let (mut drift_a, mut drift_b) = (OnePole::default(), OnePole::default());
        let mut bearing_rng = cx.rng(self.seed ^ 0xBEA2_1000_0000_0005);
        let mut bearing_bp = Svf::default();
        let mut spark_rng = cx.rng(self.seed ^ 0x5BA2_C000_0000_0006);
        let mut commutate = 0.0f64;
        let mut spark = 0.0f32;
        let carrier_hz = carrier;
        let tone = |ph: f64, k: f32| (TAU * (ph * f64::from(k)).fract() as f32).sin();
        Ok(one(mono(cx.n, |i| {
            let r = clean(rpm.at(i), 0.0, MAX_RPM);
            let l = clean(load.at(i), 0.0, 1.0);
            let w = clean(whine.at(i), 0.0, 1.0);
            let d = drift_a.tick(drift_rng.signed(), 2.0, cx.sr).0;
            let d = drift_b.tick(d, 2.0, cx.sr).0;
            let wob = 1.0 + 0.004 * (d * 60.0).clamp(-1.5, 1.5) + 0.0015 * tone(rotor, 1.0);
            let shaft = r / 60.0 * wob;
            let f_elec = shaft * poles as f32 / 2.0;
            let f_mesh = shaft * teeth as f32;
            rotor = (rotor + f64::from(shaft) / sr).fract();
            elec = (elec + f64::from(f_elec) / sr).fract();
            mesh = (mesh + f64::from(f_mesh) / sr).fract();
            pwm = (pwm + f64::from(carrier_hz) / sr).fract();
            // Brush sparks: one per commutator segment pass (twice the
            // electrical rate), each a click of random height that decays in
            // about 0.3 ms.
            commutate += f64::from(2.0 * f_elec) / sr;
            if commutate >= 1.0 {
                commutate = commutate.fract();
                spark += spark_rng.unit() * spark_rng.unit();
            }
            spark *= (-1.0 / (0.0003 * cx.sr)).exp();
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
            // Brush noise: a steady hiss and the sparks' crackle, high-passed.
            let raw = noise.signed() * (0.5 + 1.5 * spark);
            let hiss = brush_hp.tick(raw, FilterMode::Hp, 1500.0, 0.7, 0.0, cx.sr);
            s += 0.10 * (0.3 + 0.7 * l) * hiss;
            // Bearing noise: band-passed noise at 2.5 kHz, rising and falling
            // once a turn.
            let rumble = bearing_bp.tick(
                bearing_rng.signed(),
                FilterMode::Bp,
                2500.0,
                0.8,
                0.0,
                cx.sr,
            );
            s += 0.08 * (0.7 + 0.3 * tone(rotor, 1.0)) * rumble;
            0.5 * speed * s
        })))
    }
}
