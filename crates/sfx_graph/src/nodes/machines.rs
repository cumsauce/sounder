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
        // Fig. 45.5: slow noise (white through two 20 Hz one-poles), read
        // back 5, 10, 15, 20 ms late per cylinder: x 0.5 into the phase
        // (timing) and x 10 added after the cosine (pulse position and width).
        let hist_len = ((0.005 * ncyl.max(4.0)) * cx.sr) as usize + 2;
        let mut hist = vec![0.0f32; hist_len];
        let taps: Vec<usize> = (0..cylinders as usize)
            .map(|c| (((c % 4) + 1) as f32 * 0.005 * cx.sr) as usize)
            .collect();
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
        let mut half = 0.0f64;
        let top = 0.36 * cx.sr;
        Ok(one(mono(cx.n, |i| {
            let l = clean(load.at(i), 0.0, 1.0);
            let r = clean(rpm.at(i), 0.0, MAX_RPM);
            phase = (phase + f64::from(r / 60.0 * cyc_per_rev / cx.sr)).fract();
            let p = phase as f32;
            half = (half + f64::from(r / 60.0 * cyc_per_rev / 2.0 / cx.sr)).fract();
            let q = half as f32;
            let w = noise.signed();
            let nz = j2.tick(j1.tick(w, 20.0, cx.sr).0, 20.0, cx.sr).0;
            hist[i % hist_len] = nz;
            // four-stroke-engine.pd: k = 3 (1 - speed) + 2 (narrower pulses
            // at low speed); each cylinder is 1 / (1 + (cos(phase - offset +
            // 0.5 n_a) * (k + 10 n_b))^2), n the slow noise read back 5..20 ms
            // late per cylinder: the noise moves the timing a little and the
            // pulse width a lot.
            let k = 3.0 * (1.0 - (r / 6000.0).min(1.0)) + 2.0;
            let mut cyl = 0.0f32;
            for c in 0..cylinders as usize {
                let d = hist[(i + hist_len - taps[c].min(hist_len - 1)) % hist_len];
                // cos~ on a half-rate phase: its two zeros a turn give each
                // cylinder one firing per cycle (the book's four cylinders at
                // offsets 0 .. 0.75 pair up on the same zeros).
                let x = (TAU * (q + 0.5 * d - c as f32 / (2.0 * ncyl))).cos() * (k + 10.0 * d);
                cyl += level_error[c] / (1.0 + x * x);
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
        let mut body_ph = 0.0f64;
        let (mut body_a, mut body_b) = (Svf::default(), Svf::default());
        let mut level = 0.0f32;
        let mut dc_out = OnePole::default();
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
            let torque = 0.3 + 0.7 * l;

            // After Farnell (Designing Sound ch. 44), preferred in an A/B
            // (book check 3). The rotor: brush noise (band-passed at 4 kHz)
            // plus a DC level, shaped by the quartic decay of each turn, so
            // every turn is a click.
            let nb = brush_hp.tick(noise.signed(), FilterMode::Bp, 4000.0, 1.0, 0.0, cx.sr);
            let turn = (1.0 - rotor as f32).powi(4);
            // motor2.pd rotor: (brush-level x noise + rotor-level) x drive^4,
            // levels 0.05 and 0.1 (fan5.pd's loadbang); load adds brush.
            let mut s = ((0.05 + 0.1 * l) * nb * (1.0 + spark) + 0.1) * turn;
            // Fig. 44.4: drive x 2, wrap, cos~, squared, + 1, reciprocal,
            // - 0.5 (a pulse at 4 x the phasor, half-width).
            let c = (TAU * ((2.0 * elec).fract() as f32)).cos();
            s += 0.3 * (1.0 / (1.0 + c * c) - 0.5) * (0.7 + 0.3 * l);
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
            // Fig. 44.6: cos~(amount x osc~ 178 + drive); Pd's cos~ takes
            // cycles, so the motor signal moves the phase by whole turns.
            let m = s * speed;
            body_ph = (body_ph + 178.0 / sr).fract();
            let fm = (TAU * (0.5 * (TAU * body_ph as f32).cos() + 4.0 * m)).cos();
            let fm = body_b.tick(
                body_a.tick(fm, FilterMode::Hp, 180.0, 0.7, 0.0, cx.sr),
                FilterMode::Hp,
                180.0,
                0.7,
                0.0,
                cx.sr,
            );
            level += 0.002 * (m.abs() - level);
            s = m + 0.5 * fm * level;
            let _ = (&mut bearing_bp, &mut bearing_rng);
            dc_out.tick(s, 20.0, cx.sr).1
        })))
    }
}

// ------------------------------------------------------------------- clock

/// A mechanical clock ticking, after Farnell (Designing Sound ch. 43), heard
/// in book check 3 as "very close up to a clock or car blinker": on a beat
/// grid (default 8 a second, clock-all1.pd's metro 125), a tick (four clicks
/// on steps 1 4 5 8 of a 3 ms burst, each ringing 6543 / 3245 / 1356 Hz at Q
/// 30 for 30 / 20 / 10 ms, square-law decay) on beats 1 and 5 of each 8, a
/// tock (7543 / 3988 / 2765 Hz, steps 1 2 4 6 8) on 3 and 7, the escapement
/// (eight 10 ms pings per beat alternating 10 and 8 kHz) and the hand (300 /
/// 3245 / 5356 Hz, 120 / 20 / 10 ms, exponential) on beat 1; mixed 0.1 /
/// 0.1 / 0.06 / 0.3, x 0.3, plus the case (bodyresonance.pd: 11.1 and 9.1 ms
/// loops through 900 and 1100 Hz band-passes, Q 3, fed back x 0.3).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    /// Beats a second, 1 to 40 (default 8). Ticks fall on 2 of every 8
    /// beats, tocks on 2 others, the escapement on every one.
    #[serde(default)]
    pub rate: Option<f32>,
    /// Case size, 0.25 to 4 (default 1): the case delays are 10 ms x size and
    /// its band-passes 1.2 kHz / size; bigger is lower and roomier.
    #[serde(default)]
    pub size: Option<f32>,
    /// Escapement level, 0 to 2 (default 1): the high 8-10 kHz chatter.
    #[serde(default)]
    pub escapement: Option<f32>,
    /// Seconds before the first beat (default 0.005).
    #[serde(default)]
    pub at: Option<f32>,
    /// Listening distance, metres, 0 to 50 (default 0: right up to the
    /// clock). Further away only the ticks carry: the escapement falls as
    /// `1 / (1 + (d / 0.3)^2)`, the tock and the hand clunk as
    /// `1 / (1 + (d / 1.5)^1.5)`, and everything is low-passed at
    /// `12 kHz / (1 + d / 3)` (one pole). Heard close (0 m) the listener
    /// called it "a mechanical thing"; a clock across a room is about 3 m.
    #[serde(default)]
    pub distance: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Clock {
    const BLURB: &'static str = "a mechanical clock (Farnell): tick and tock clusters through narrow bands, a high escapement chatter, a hand clunk, a small case resonance; rate = beats a second";
    fn example() -> serde_json::Value {
        serde_json::json!({"rate": 4, "size": 1, "escapement": 1, "distance": 3, "seed": 1})
    }
}

impl Eval for Clock {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let rate = self.rate.unwrap_or(8.0);
        if !(rate.is_finite() && (1.0..=40.0).contains(&rate)) {
            return Err(format!("rate: {rate}: give beats a second from 1 to 40"));
        }
        let size = self.size.unwrap_or(1.0);
        if !(size.is_finite() && (0.25..=4.0).contains(&size)) {
            return Err(format!("size: {size}: give 0.25 to 4 (1 = a mantel clock)"));
        }
        let esc = self.escapement.unwrap_or(1.0);
        if !(esc.is_finite() && (0.0..=2.0).contains(&esc)) {
            return Err(format!("escapement: {esc}: give 0 to 2"));
        }
        let at = super::non_negative("at", self.at.unwrap_or(0.005))?;
        let dist = self.distance.unwrap_or(0.0);
        if !(dist.is_finite() && (0.0..=50.0).contains(&dist)) {
            return Err(format!("distance: {dist}: give metres from 0 to 50"));
        }
        let far_esc = 1.0 / (1.0 + (dist / 0.3).powi(2));
        let far_tock = 1.0 / (1.0 + (dist / 1.5).powf(1.5));
        let air_hz = (12000.0 / (1.0 + dist / 3.0)).min(0.45 * cx.sr);
        let mut air = OnePole::default();
        let sr = cx.sr;
        // clock-all1.pd. Per beat (metro 125 ms = rate 8), `bangburst 3`
        // counts 0..9 every 3 ms; each part fires on some of those steps.
        // A click (mclick.pd) restarts all three of its band envelopes; a
        // band is noise (hip 200) through bp~ Q 30, times sqdec: a 1 ms rise
        // then (1 - t / T)^2 over T. mclick~ (the hand) decays
        // exponentially instead.
        struct Part {
            bands: [(f32, f32); 3],
            steps: &'static [usize],
            beats: &'static [usize],
            gain: f32,
            exp: bool,
        }
        let parts = [
            // clocktick: 6543 / 3245 / 1356 Hz, 30 / 20 / 10 ms, steps 1 4 5 8
            Part {
                bands: [(6543.0, 0.03), (3245.0, 0.02), (1356.0, 0.01)],
                steps: &[1, 4, 5, 8],
                beats: &[0, 4],
                gain: 0.1,
                exp: false,
            },
            // clocktick2: 7543 / 3988 / 2765 Hz, steps 1 2 4 6 8
            Part {
                bands: [(7543.0, 0.03), (3988.0, 0.02), (2765.0, 0.01)],
                steps: &[1, 2, 4, 6, 8],
                beats: &[2, 6],
                gain: 0.1,
                exp: false,
            },
            // clockhand: mclick~ 300 / 3245 / 5356 Hz, 120 / 20 / 10 ms, on
            // beat 0 at once
            Part {
                bands: [(300.0, 0.12), (3245.0, 0.02), (5356.0, 0.01)],
                steps: &[0],
                beats: &[0],
                gain: 0.3,
                exp: true,
            },
        ];
        let far = [1.0, far_tock, far_tock];
        // Triggers in samples, per part.
        let beat_s = 1.0 / rate;
        let mut trig: Vec<Vec<usize>> = vec![Vec::new(); parts.len()];
        let mut esc_on: Vec<(usize, f32)> = Vec::new();
        let mut b = 0usize;
        loop {
            let t = at + b as f32 * beat_s;
            if t * sr >= cx.n as f32 {
                break;
            }
            for (k, part) in parts.iter().enumerate() {
                if part.beats.contains(&(b % 8)) {
                    for st in part.steps {
                        trig[k].push(((t + 0.003 * *st as f32) * sr) as usize);
                    }
                }
            }
            // escapement.pd: sqdec 10 ms pings, 10 kHz on steps 1 3 6 8 and
            // 8 kHz on 2 4 5 7, every beat.
            for (st, hz) in [
                (1, 1e4),
                (2, 8e3),
                (3, 1e4),
                (4, 8e3),
                (5, 8e3),
                (6, 1e4),
                (7, 8e3),
                (8, 1e4),
            ] {
                esc_on.push((((t + 0.003 * st as f32) * sr) as usize, hz));
            }
            b += 1;
        }
        for v in trig.iter_mut() {
            v.sort_unstable();
        }
        esc_on.sort_by_key(|e| e.0);
        let mut noise = cx.rng(self.seed);
        let mut nhp = OnePole::default();
        // Per part, three band filters and their envelopes (age in samples).
        let mut filt: Vec<[Svf; 3]> = parts
            .iter()
            .map(|_| [Svf::default(), Svf::default(), Svf::default()])
            .collect();
        let mut age: Vec<usize> = vec![usize::MAX; parts.len()];
        let mut next = vec![0usize; parts.len()];
        let rise = (0.001 * sr).max(1.0);
        let sq = |age: usize, t60: f32| -> f32 {
            let a = age as f32;
            if a < rise {
                a / rise
            } else {
                let x = 1.0 - (a - rise) / (t60 * sr);
                if x > 0.0 { x * x } else { 0.0 }
            }
        };
        let ex =
            |age: usize, t60: f32| -> f32 { (-(age as f32) / (t60 * sr) * 1000.0f32.ln()).exp() };
        let (mut ei, mut eph, mut ehz, mut eage) = (0usize, 0.0f32, 1e4f32, usize::MAX);
        let esc_len = 0.01 * sr;
        // bodyresonance.pd: two loops, 1 / 0.09 and 1 / 0.11 ms (x size),
        // each through bp~ at 900 / 1100 Hz (/ size) Q 3, fed back x 0.3;
        // the two band-pass outputs are added to the dry sound.
        let (dl, dw) = (0.0111 * size * sr, 0.0091 * size * sr);
        let mut ll = DelayLine::new(dl.ceil() as usize + 4);
        let mut lw = DelayLine::new(dw.ceil() as usize + 4);
        let (mut bl, mut bw) = (Svf::default(), Svf::default());
        let (fl, fwh) = (
            (900.0 / size).min(0.45 * sr),
            (1100.0 / size).min(0.45 * sr),
        );
        Ok(one(mono(cx.n, |i| {
            let nz = nhp.tick(noise.signed(), 200.0, sr).1;
            let mut dry = 0.0;
            for (k, part) in parts.iter().enumerate() {
                while next[k] < trig[k].len() && trig[k][next[k]] <= i {
                    age[k] = 0;
                    next[k] += 1;
                }
                let mut click = 0.0;
                for (j, (hz, t)) in part.bands.iter().enumerate() {
                    let y = filt[k][j].tick(nz, FilterMode::Bp, hz.min(0.45 * sr), 30.0, 0.0, sr);
                    let e = if age[k] == usize::MAX {
                        0.0
                    } else if part.exp {
                        ex(age[k], *t)
                    } else {
                        sq(age[k], *t)
                    };
                    click += y * e;
                }
                dry += 3.0 * part.gain * far[k] * click;
                if age[k] != usize::MAX {
                    age[k] += 1;
                }
            }
            while ei < esc_on.len() && esc_on[ei].0 <= i {
                eage = 0;
                ehz = esc_on[ei].1.min(0.45 * sr);
                ei += 1;
            }
            if eage != usize::MAX && (eage as f32) < esc_len + rise {
                eph = (eph + ehz / sr).fract();
                dry += 0.06 * esc * far_esc * sq(eage, 0.01) * (TAU * eph).sin();
                eage += 1;
            }
            let dry = 0.3 * dry;
            let ol = bl.tick(ll.read(dl), FilterMode::Bp, fl, 3.0, 0.0, sr);
            let ow = bw.tick(lw.read(dw), FilterMode::Bp, fwh, 3.0, 0.0, sr);
            ll.write(dry + 0.3 * ol);
            lw.write(dry + 0.3 * ow);
            let y = dry + ol + ow;
            if dist > 0.0 {
                air.tick(y, air_hz, sr).0
            } else {
                y
            }
        })))
    }
}

// --------------------------------------------------------------------- fan

/// A fan or ventilation unit, after Farnell (Designing Sound ch. 46; book
/// check 3 heard the 24 Hz version as "a bigger machine propeller", so the
/// default blade rate is higher): blade pulses `1 / (1 + (4 cos)^2)` at
/// twice `rate` x `speed`, band-passed noise chopped by the pulses, a small
/// motor hum, then a duct (two feedback delays, 9 ms x0.3 and 23 ms x0.4).
/// A closer port of fan5.pd (its motor inside, coupled 22 / 70 ms duct) was
/// rated worse by ear, so this adaptation stays.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fan {
    /// Spin, 0 to 1 (a node for spin-up / down, e.g. a `ramp`): scales the
    /// blade rate and the level; 0 is silence.
    pub speed: In,
    /// Blade passes a second at full speed are twice this, 2 to 200
    /// (default 30: 60 a second). Lower reads as a bigger fan or propeller.
    #[serde(default)]
    pub rate: Option<f32>,
    /// Centre of the air noise, Hz, 200 to 8000 (default 1500).
    #[serde(default)]
    pub air: Option<f32>,
    /// Duct echo level, 0 to 1 (default 0.5); 0 is a fan in the open.
    #[serde(default)]
    pub duct: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Fan {
    const BLURB: &'static str = "a fan or ventilation unit (Farnell): blade pulses at 2 x rate x speed, air noise chopped by the blades, a motor hum, a duct echo; speed is a 0..1 signal";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 1, "rate": 30, "air": 1500, "duct": 0.5, "seed": 1})
    }
}

impl Eval for Fan {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono("speed", &self.speed)?;
        if let Some(s) = speed.constant()
            && !(0.0..=1.0).contains(&s)
        {
            return Err(format!("speed: {s}: give 0 to 1 (a signal is clamped)"));
        }
        let rate = self.rate.unwrap_or(30.0);
        if !(rate.is_finite() && (2.0..=200.0).contains(&rate)) {
            return Err(format!(
                "rate: {rate}: give 2 to 200 (blade passes are twice this)"
            ));
        }
        let air = self.air.unwrap_or(1500.0);
        if !(air.is_finite() && (200.0..=8000.0).contains(&air)) {
            return Err(format!("air: {air}: give Hz from 200 to 8000"));
        }
        let duct = self.duct.unwrap_or(0.5);
        if !(duct.is_finite() && (0.0..=1.0).contains(&duct)) {
            return Err(format!("duct: {duct}: give 0 to 1"));
        }
        let sr = cx.sr;
        let mut noise = cx.rng(self.seed);
        let mut bp = Svf::default();
        let mut dc = OnePole::default();
        let (d1, d2) = (0.009 * sr, 0.023 * sr);
        let mut l1 = DelayLine::new(d1.ceil() as usize + 4);
        let mut l2 = DelayLine::new(d2.ceil() as usize + 4);
        let mut p = 0.0f64;
        Ok(one(mono(cx.n, |i| {
            let s = clean(speed.at(i), 0.0, 1.0);
            p = (p + f64::from(rate * s / sr)).fract();
            let ph = p as f32;
            let c = 4.0 * (TAU * ph).cos();
            let pulse = 1.0 / (1.0 + c * c);
            let nb = bp.tick(
                noise.signed(),
                FilterMode::Bp,
                air.min(0.45 * sr),
                1.0,
                0.0,
                sr,
            );
            let h = (8.0 * ph).fract();
            let hum = s * (0.05 * (TAU * h).sin() + 0.03 * (1.0 - h).powi(4));
            let fan = (0.1 * pulse + 0.4 * nb * pulse + 0.2 * hum) * s;
            let x = dc.tick(fan, 20.0, sr).1;
            let a = l1.read(d1);
            l1.write(x + 0.3 * a);
            let b = l2.read(d2);
            l2.write(x + 0.4 * b);
            x + duct * (a + b)
        })))
    }
}
