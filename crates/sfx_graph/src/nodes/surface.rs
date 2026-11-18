//! Surface sounds (M6): `rolling` (the excitation of something rolling over a
//! rough surface), `cloth` (rustling fabric, paper, foliage) and `footstep`
//! (one heel-and-toe step on concrete, gravel, grass or wood). Tests in
//! `crates/sfx_analysis/tests/surface.rs`.

use std::f32::consts::PI;

use serde::Deserialize;

use super::{non_negative, one};
use crate::buf::Buf;
use crate::dsp::{Biquad, FilterMode, Resonator, Rng, Svf, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

// ------------------------------------------------------------------ helpers

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// A signal sample made safe: non-finite counts as `fallback`, then clamped.
fn safe(x: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if x.is_finite() {
        x.clamp(lo, hi)
    } else {
        fallback
    }
}

/// SplitMix64 finalizer: the same bits on every platform and thread count.
fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ------------------------------------------------------------------ rolling

/// How many surface features per second `rolling` may pass at most, as a
/// fraction of the sample rate (past a quarter of it the profile is read
/// faster than it can be represented).
const ROLLING_MAX_FEATURES: f32 = 0.25;
/// Fastest speed `rolling` follows, m/s (a fast car); level grows with speed.
const ROLLING_MAX_SPEED: f32 = 50.0;

/// Catmull-Rom spline through four points, at `x` in 0..1 between the
/// middle two; and its derivative with respect to `x`.
fn catmull(p: [f32; 4], x: f32) -> (f32, f32) {
    let [a, b, c, d] = p;
    let v = 0.5
        * (2.0 * b
            + (c - a) * x
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * x * x
            + (3.0 * b - a - 3.0 * c + d) * x * x * x);
    let dv = 0.5
        * ((c - a)
            + 2.0 * (2.0 * a - 5.0 * b + 4.0 * c - d) * x
            + 3.0 * (3.0 * b - a - 3.0 * c + d) * x * x);
    (v, dv)
}

/// A seeded random value in -1..1 at integer cell `i` of octave `octave`.
fn cell(base: u64, octave: u64, i: i64) -> f32 {
    let h = mix64(base ^ mix64(octave.wrapping_mul(0xA24B_AED4_963E_E407)) ^ (i as u64));
    (h >> 40) as f32 / 8_388_608.0 - 1.0
}

/// The surface that something rolls over, read at `speed`: a seeded random
/// height profile whose features are `size` metres across, differentiated
/// along the way (a contact force follows the slope of the surface times
/// the speed). The output is the excitation for `modal`, `resonators` or a
/// filter: silence at speed 0, level proportional to speed, centroid
/// proportional to speed / size.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rolling {
    /// Speed over the surface, m/s. A number or a signal (a `ramp` is a
    /// wheel that speeds up). Negative counts as its magnitude; 0 is exact
    /// silence. A number over 50 m/s, or whose `speed / size` passes 0.25 x
    /// the sample rate features a second (12000 /s at 48 kHz), is an error;
    /// a signal is clamped to those limits.
    pub speed: In,
    /// How rough the surface is, 0..1 (default 0.5). 0 is one smooth
    /// undulation of `size`; more adds finer grit at 1/3.7 and 1/13.7 of
    /// `size`, which brightens the output. A number outside 0..1 is an
    /// error.
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Size of the surface features, metres (default 0.002, range
    /// 0.0001..10): bigger features give a lower, slower rumble. A number
    /// outside the range is an error.
    #[serde(default)]
    pub size: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): another
    /// seed is another stretch of surface.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Rolling {
    const BLURB: &'static str = "the force of rolling over a rough surface at speed (silence at 0, level ~ speed, pitch of the grit ~ speed / size); excites modal or resonators";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 1.5, "roughness": 0.5, "size": 0.002, "seed": 1})
    }
}

impl Eval for Rolling {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono("speed", &self.speed)?;
        let rough = self.roughness.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&rough) {
            return Err(format!("roughness: {rough}: give a number from 0 to 1"));
        }
        let size = self.size.unwrap_or(0.002);
        if !(0.0001..=10.0).contains(&size) {
            return Err(format!(
                "size: {size}: give metres from 0.0001 to 10 (0.002 is fine grit, 0.02 a coarse road)"
            ));
        }
        let max_features = ROLLING_MAX_FEATURES * cx.sr;
        // The fastest speed that stays representable at this size.
        let v_max = (max_features * size).min(ROLLING_MAX_SPEED);
        if let Some(v) = speed.constant()
            && v.abs() > v_max
        {
            return Err(if v.abs() > ROLLING_MAX_SPEED {
                format!(
                    "speed: {v}: give a speed under {ROLLING_MAX_SPEED} m/s (a signal is clamped to it)"
                )
            } else {
                format!(
                    "speed: {v}: at size {size} m that passes {:.0} features a second, past {max_features:.0} (a quarter of the sample rate): give a speed under {v_max:.3} m/s or a bigger size",
                    v.abs() / size
                )
            });
        }
        let base = cx.rng(self.seed).next_u64();
        // Octave k reads the profile at 3.7^k x the position, weighted by
        // roughness^k.
        const SCALE: [f32; 3] = [1.0, 3.7, 13.7];
        let weight = [1.0, rough, rough * rough];
        // Normalized so speed 1 m/s gives about -14 dBFS rms at the
        // defaults (measured, tests/surface.rs).
        let gain = 0.235 / (1.0 + 0.8 * rough + 0.6 * rough * rough).sqrt();
        let mut pos = 0.0f64;
        let mut out = vec![0.0f32; cx.n];
        for (i, o) in out.iter_mut().enumerate() {
            let v = safe(speed.at(i).abs(), 0.0, v_max, 0.0);
            let dpos = f64::from(v / size / cx.sr);
            pos += dpos;
            // The derivative of the profile along the path, summed over the
            // octaves (each at its own scale), times the speed.
            let mut slope = 0.0f32;
            for k in 0..3 {
                if weight[k] == 0.0 {
                    continue;
                }
                let p = pos * f64::from(SCALE[k]);
                let c = p.floor();
                let x = (p - c) as f32;
                let c = c as i64;
                let pts = [
                    cell(base, k as u64, c - 1),
                    cell(base, k as u64, c),
                    cell(base, k as u64, c + 1),
                    cell(base, k as u64, c + 2),
                ];
                let (_, d) = catmull(pts, x);
                // d is per unit of that octave's position: per unit of the
                // base position it grows by SCALE[k], but the octave's
                // amplitude falls the same, so the weights set the balance.
                slope += weight[k] * d;
            }
            *o = gain * v * slope;
        }
        Ok(one(Buf::Audio(out)))
    }
}

// -------------------------------------------------------------------- cloth

/// Bursts per second at motion 1 and weight 0.5.
const CLOTH_RATE: f32 = 70.0;
/// Most motion the node follows (a larger value is clamped, or an error for
/// a number).
const CLOTH_MAX_MOTION: f32 = 4.0;
/// Largest burst size drawn (the power law is cut here), in units of the
/// smallest burst.
const CLOTH_MAX_SIZE: f32 = 40.0;
/// Exponent of the burst-size power law: P(size > s) ~ s^-ALPHA.
const CLOTH_ALPHA: f32 = 1.3;
/// Most bursts alive at once (a rate of 4 x 70 x 1.4 per second with a
/// longest life of about 0.3 s needs fewer than this).
const CLOTH_MAX_ACTIVE: usize = 512;
/// Crunch clusters drawn for a gravel step (the largest it can have) and
/// stones drawn for each.
const GRAVEL_CLUSTERS: usize = 13;
const GRAVEL_STONES: usize = 10;

/// Centre frequencies of the four bands bursts land in, Hz at motion 1 and
/// weight 0.5 (they move with sqrt(motion) and 2^(-1.2 (weight - 0.5))).
const CLOTH_BANDS: [f32; 4] = [600.0, 1500.0, 3500.0, 7500.0];

/// One burst size from the power law: `u` in 0..1 uniform.
pub(crate) fn cloth_size(u: f32) -> f32 {
    (1.0 - u.clamp(0.0, 0.999_999))
        .powf(-1.0 / CLOTH_ALPHA)
        .min(CLOTH_MAX_SIZE)
}

/// Rustling cloth, paper or foliage: short noise bursts whose sizes follow
/// a power law (many small, a few large), arriving at a rate that follows
/// `motion`, in four noise bands whose centres rise with `motion` and fall
/// with `weight`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cloth {
    /// How much the cloth is moving: 0 is still (exact silence), 1 a
    /// walking arm swing, up to 4. A number or a signal (a `wander` is
    /// fidgeting). A number outside 0..4 is an error; a signal is clamped.
    /// Raises the burst rate, level and band centres together.
    pub motion: In,
    /// Heaviness of the fabric, 0..1 (default 0.5): 0 is silk or paper
    /// (bright, short, frequent bursts), 1 is canvas or a coat (dull, long,
    /// fewer). A number outside 0..1 is an error.
    #[serde(default)]
    pub weight: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Cloth {
    const BLURB: &'static str = "rustling cloth/paper/foliage: power-law noise bursts whose rate, level and band follow motion; weight darkens and lengthens them";
    fn example() -> serde_json::Value {
        serde_json::json!({"motion": 1.0, "weight": 0.5, "seed": 1})
    }
}

struct Burst {
    band: usize,
    /// Two exponentials whose difference is the burst's envelope: a rise
    /// of `attack` seconds and a fall of `decay` (tens of ms each), so a
    /// burst swells and fades like a fold of fabric rubbing, not a tick.
    slow: f32,
    fast: f32,
    k_slow: f32,
    k_fast: f32,
}

/// Level of the continuous rubbing bed against the bursts, at motion 1.
const CLOTH_BED: f32 = 0.5;

impl Eval for Cloth {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let motion = cx.mono("motion", &self.motion)?;
        if let Some(m) = motion.constant()
            && !(0.0..=CLOTH_MAX_MOTION).contains(&m)
        {
            return Err(format!(
                "motion: {m}: give a number from 0 to {CLOTH_MAX_MOTION} (0 is still, 1 a swinging arm)"
            ));
        }
        let weight = self.weight.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&weight) {
            return Err(format!("weight: {weight}: give a number from 0 to 1"));
        }
        let mut rng = cx.rng(self.seed);
        let sr = cx.sr;
        let rate_scale = lerp(1.4, 0.6, weight);
        let amp_scale = lerp(0.7, 1.3, weight);
        let tau_scale = lerp(1.0, 2.2, weight);
        let tone = 2f32.powf(-1.2 * (weight - 0.5));
        let mut active: Vec<Burst> = Vec::new();
        let mut filters: [Svf; 4] = Default::default();
        // Bed level follows motion smoothed over 40 ms (friction of the
        // whole garment), so the sound is continuous while it moves.
        let k_motion = 1.0 - (-1.0 / (0.04 * sr)).exp();
        let mut m_smooth = 0.0f32;
        let mut out = vec![0.0f32; cx.n];
        for (i, o) in out.iter_mut().enumerate() {
            let m = safe(motion.at(i), 0.0, CLOTH_MAX_MOTION, 0.0);
            m_smooth += k_motion * (m - m_smooth);
            if m == 0.0 && m_smooth < 1e-6 {
                m_smooth = 0.0;
            }
            // Thinned Poisson arrivals at the current rate.
            let rate = CLOTH_RATE * m * rate_scale;
            if rate > 0.0 && rng.unit() < rate / sr && active.len() < CLOTH_MAX_ACTIVE {
                let size = cloth_size(rng.unit());
                let r = rng.unit();
                // Big bursts fall in the low bands.
                let score = 3.0 * r - 1.6 * size.ln() / CLOTH_MAX_SIZE.ln();
                let band = score.round().clamp(0.0, 3.0) as usize;
                let decay = (0.025 * size.sqrt() * tau_scale).min(0.25);
                let attack = (0.35 * decay).max(0.006);
                let level = 0.05 * size.powf(0.35) * amp_scale * (0.4 + 0.6 * m.min(2.0));
                let (ks, kf) = ((-1.0 / (decay * sr)).exp(), (-1.0 / (attack * sr)).exp());
                active.push(Burst {
                    band,
                    slow: level,
                    fast: level,
                    k_slow: ks,
                    k_fast: kf,
                });
            }
            let mut target = [0.0f32; 4];
            for b in &mut active {
                target[b.band] += b.slow - b.fast;
                b.slow *= b.k_slow;
                b.fast *= b.k_fast;
            }
            active.retain(|b| b.slow > 1e-5);
            let bed = CLOTH_BED * 0.05 * amp_scale * m_smooth.min(CLOTH_MAX_MOTION).powf(0.75);
            // Band centres follow motion (octaves per unit) and weight.
            let shift = m_smooth.max(0.05).sqrt() * tone;
            let mut sum = 0.0;
            for k in 0..4 {
                let n = rng.signed();
                let hz = (CLOTH_BANDS[k] * shift).clamp(40.0, max_hz(sr));
                // Wide bands (q 0.6): rustle is broadband, a narrow band
                // rings as a pitched tick.
                sum += filters[k].tick(n, FilterMode::Bp, hz, 0.6, 0.0, sr) * (target[k] + bed);
            }
            *o = if m_smooth == 0.0 && active.is_empty() {
                0.0
            } else {
                sum
            };
        }
        Ok(one(Buf::Audio(out)))
    }
}

// ----------------------------------------------------------------- footstep

/// The ground under the foot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Ground {
    /// Hard and dead: a click and a short slab thud.
    Concrete,
    /// Loose stones: a chain of crunches, each a few small stone ticks.
    Gravel,
    /// Soft: a swish with a slow attack and a few stalk ticks, no pitch.
    Grass,
    /// A floor over a cavity: a low body mode under a short tick.
    Wood,
}

/// What is on the foot: sets how sharp the contact is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shoe {
    Hard,
    Sneaker,
    Boot,
    Bare,
}

impl Shoe {
    fn parse(s: &str) -> Result<Shoe, String> {
        match s {
            "hard" => Ok(Shoe::Hard),
            "sneaker" => Ok(Shoe::Sneaker),
            "boot" => Ok(Shoe::Boot),
            "bare" => Ok(Shoe::Bare),
            other => Err(format!(
                "shoe: \"{other}\": give hard (leather sole, heel click), sneaker (default), boot or bare"
            )),
        }
    }
    /// Contact pulse length factor.
    fn width(self) -> f32 {
        match self {
            Shoe::Hard => 0.6,
            Shoe::Sneaker => 1.0,
            Shoe::Boot => 1.2,
            Shoe::Bare => 2.0,
        }
    }
    /// Level of the sharp part (click, ticks).
    fn click(self) -> f32 {
        match self {
            Shoe::Hard => 1.4,
            Shoe::Sneaker => 0.8,
            Shoe::Boot => 1.0,
            Shoe::Bare => 0.35,
        }
    }
}

/// The pace as a number: 0 a creeping sneak, 0.5 a walk, 1 a run.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Pace {
    /// 0..1: 0 sneak, 0.5 walk, 1 run.
    Num(f32),
    /// `walk` (0.5), `run` (0.9) or `sneak` (0.1).
    Word(String),
}

impl Pace {
    fn value(&self) -> Result<f32, String> {
        match self {
            Pace::Num(x) if (0.0..=1.0).contains(x) => Ok(*x),
            Pace::Num(x) => Err(format!(
                "pace: {x}: give a number from 0 (sneak) to 1 (run), or walk, run, sneak"
            )),
            Pace::Word(w) => match w.as_str() {
                "walk" => Ok(0.5),
                "run" => Ok(0.9),
                "sneak" => Ok(0.1),
                other => Err(format!(
                    "pace: \"{other}\": give walk, run, sneak, or a number from 0 to 1"
                )),
            },
        }
    }
}

/// One step: a heel strike, then the toe coming down, on a surface. A
/// finished recipe in the style of `thunder`: pick a `surface`, the rest
/// have defaults. Each `seed` is another step: the heel-toe gap, the levels,
/// the resonances and the stones all move a little.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Footstep {
    /// concrete, gravel, grass or wood.
    pub surface: Ground,
    /// How heavy the walker is, 0..1 (default 0.5; a number). Continuous:
    /// more weight is louder, longer in contact and lower in colour (the
    /// centroid falls), and on gravel and wood adds stones / lowers the body.
    /// A number outside 0..1 is an error.
    #[serde(default)]
    pub weight: Option<f32>,
    /// How fast the foot moves: `walk` (default), `run`, `sneak`, or a
    /// number 0..1 (0 sneak, 0.5 walk, 1 run). Sets the heel-to-toe gap
    /// (140 ms at 0, 105 ms at 0.5, 70 ms at 1, each +-7 % by seed), the level and, on gravel,
    /// the crunch spacing.
    #[serde(default)]
    pub pace: Option<Pace>,
    /// What is on the foot: `sneaker` (default), `hard` (leather sole:
    /// sharper, with a heel click), `boot`, `bare` (soft, no click).
    #[serde(default)]
    pub shoe: Option<String>,
    /// Scales the resonant body frequency, 0.5..2 (default 1), independent
    /// of `weight`: wood's three body modes, concrete's low thud, gravel's
    /// thump and grass's soft low band all move by this factor (0.5 an
    /// octave down, 2 an octave up). The sharp upper part (ticks, scuffs)
    /// does not move. A number outside 0.5..2 is an error.
    #[serde(default)]
    pub body: Option<f32>,
    /// Balance of the continuous swish against the stalk / stone-tick layer,
    /// 0..1 (default 0.5, the unchanged recipe). Grass: the swish bands scale
    /// by `1 + 2 (swish - 0.5)` and the stalk ticks by `1 - 2 (swish - 0.5)`
    /// (0 is ticks only, at twice the level; 1 is swish only). Gravel: below
    /// 0.5 the stone ticks grow up to 1.6x; above it they fade to 0.4x and a
    /// swish bed (the grass band) fades in. Concrete and wood ignore it. A
    /// number outside 0..1 is an error.
    #[serde(default)]
    pub swish: Option<f32>,
    /// When the heel lands, seconds (default 0). The step lasts up to about
    /// 0.6 s after it; whatever passes the end of the sound is cut.
    #[serde(default)]
    pub at: Option<f32>,
    /// Wood only, 0..1 (default none): the board's resonances ring as bands
    /// of noise (as `modal` `noise`) instead of clean modes. A default wood
    /// step reads pitched (voiced 0.84 at 158 Hz); left out or 0, the
    /// samples are unchanged. Other surfaces ignore it.
    #[serde(default)]
    pub noise: Option<f32>,
    /// Wood only, 0..1 (default none): the sole scuffing the board between
    /// heel and toe (band-passed noise around 2.5 kHz that swells and fades
    /// across the roll). Left out or 0, the samples are unchanged.
    #[serde(default)]
    pub scuff: Option<f32>,
    /// Wood only, 0.25..2 (default 1): how long the board rings, a
    /// multiplier on its three body modes' decays. The default board puts 84 %
    /// of a step's energy under 200 Hz for 160 ms, heard as "slammed"; lower
    /// is a shorter, gentler step. Left out, the samples are unchanged.
    #[serde(default)]
    pub ring: Option<f32>,
    /// Wood only, 0..1 (default none): a board creaking under the weight, a
    /// stick-slip train (70-130 slips a second, seeded, each slip jittered)
    /// from 30-60 ms after the heel for 0.15-0.3 s through three of the
    /// board's resonances (noise bands too when `noise` is set). The level;
    /// give some steps a creak and others none for a walk. Left out or 0,
    /// the samples are unchanged.
    #[serde(default)]
    pub creak: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): another
    /// seed is another step (timing, levels, tuning, stones).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Footstep {
    const BLURB: &'static str = "one footstep: heel then toe on concrete, gravel, grass or wood, by weight, pace, shoe, body and swish (a finished recipe; seed for another step)";
    const WORDS: &'static [&'static str] = &["surface", "pace", "shoe"];
    fn example() -> serde_json::Value {
        serde_json::json!({"surface": "concrete", "weight": 0.5, "pace": "walk", "shoe": "sneaker", "at": 0.0, "seed": 1})
    }
}

/// Everything a surface recipe needs to know about the step.
struct Step {
    sr: f32,
    w: f32,
    pace: f32,
    shoe: Shoe,
    /// Body frequency factor and swish balance (`body`, `swish`).
    body: f32,
    swish: f32,
    /// `noise` (wood's resonances as noise bands), when set.
    noise: Option<f32>,
    /// `scuff` and `creak` (wood), when set.
    scuff: Option<f32>,
    creak: Option<f32>,
    /// `ring` (wood): the body modes' decay multiplier, when set.
    ring: Option<f32>,
    /// Heel landing and toe landing, seconds, with their strengths.
    heel: (f32, f32),
    toe: (f32, f32),
    rng: Rng,
}

/// Adds a half-sine pulse of `width` seconds starting at `t`, scaled to a
/// unit sum (so a wider pulse has the same low-frequency content, less
/// highs), times `amp`.
fn add_pulse(buf: &mut [f32], sr: f32, t: f32, width: f32, amp: f32) {
    let len = ((width * sr).round() as usize).clamp(2, 4096);
    let start = (t * sr).round() as usize;
    let shape: Vec<f32> = (0..len)
        .map(|k| (PI * (k as f32 + 0.5) / len as f32).sin())
        .collect();
    let sum: f32 = shape.iter().sum();
    for (k, s) in shape.iter().enumerate() {
        if let Some(v) = buf.get_mut(start + k) {
            *v += amp * s / sum;
        }
    }
}

/// A burst of band-passed noise: `attack` seconds of raised-cosine rise,
/// then an exponential fall of time constant `decay`; normalized so `amp`
/// is about the rms of the band at full envelope.
#[allow(clippy::too_many_arguments)]
fn add_noise(
    buf: &mut [f32],
    rng: &mut Rng,
    sr: f32,
    t: f32,
    hz: f32,
    q: f32,
    amp: f32,
    attack: f32,
    decay: f32,
) {
    let start = (t * sr).round() as usize;
    if start >= buf.len() {
        return;
    }
    let len = (((attack + 7.0 * decay) * sr) as usize).clamp(4, 96_000);
    let mut f = Biquad::new();
    f.set(FilterMode::Bp, hz, q, 0.0, sr);
    // White noise of rms 0.577 through a band of hz/q out of sr/2.
    let norm = ((sr * 0.5 / (hz.max(1.0) / q.max(0.1))).sqrt() / 0.577).clamp(1.0, 80.0);
    let a = attack * sr;
    let fall = (-1.0 / (decay.max(1e-4) * sr)).exp();
    let mut tail = 1.0f32;
    for k in 0..len {
        let kf = k as f32;
        let env = if a > 0.5 && kf < a {
            let s = (0.5 * PI * kf / a).sin();
            s * s
        } else {
            let e = tail;
            tail *= fall;
            e
        };
        let v = f.tick(rng.signed()) * env * amp * norm;
        if let Some(b) = buf.get_mut(start + k) {
            *b += v;
        }
    }
}

/// Runs the buffer through resonators (hz, decay s, gain).
fn ring(buf: &[f32], modes: &[(f32, f32, f32)], sr: f32) -> Vec<f32> {
    let mut bank: Vec<Resonator> = modes
        .iter()
        .map(|(hz, d, g)| Resonator::new(*hz, *d, *g, sr))
        .collect();
    buf.iter()
        .map(|x| bank.iter_mut().map(|r| r.tick(*x)).sum())
        .collect()
}

fn add(out: &mut [f32], x: &[f32], gain: f32) {
    for (o, v) in out.iter_mut().zip(x) {
        *o += v * gain;
    }
}

impl Step {
    /// Wood's resonances: clean modes, or with `noise` bands of noise.
    fn wood_ring(&mut self, buf: &[f32], modes: &[(f32, f32, f32)], sr: f32) -> Vec<f32> {
        let Some(depth) = self.noise else {
            return ring(buf, modes, sr);
        };
        let mut out = vec![0.0f32; buf.len()];
        let mut white = vec![0.0f32; buf.len()];
        for &(hz, d, g) in modes {
            let mut r = Resonator::new(hz, d, g, sr);
            let y: Vec<f32> = buf.iter().map(|x| r.tick(*x)).collect();
            super::modal::add_noise_band(
                &mut out,
                &y,
                (hz, d),
                depth,
                super::modal::DEFAULT_NOISE_WIDTH,
                &mut self.rng,
                &mut white,
                sr,
            );
        }
        out
    }

    /// Brightness multiplier: heavier steps are darker.
    fn tilt(&self) -> f32 {
        lerp(1.25, 0.75, self.w)
    }

    /// Contact pulse width, seconds: from `light` to `heavy` ms across
    /// weight, scaled by the shoe.
    fn width(&self, light: f32, heavy: f32) -> f32 {
        lerp(light, heavy, self.w) * 0.001 * self.shoe.width()
    }

    fn impacts(&self) -> [(f32, f32); 2] {
        [self.heel, self.toe]
    }

    fn concrete(&mut self, n: usize) -> Vec<f32> {
        let sr = self.sr;
        let tilt = self.tilt();
        let mut force = vec![0.0f32; n];
        let width = self.width(0.7, 1.7);
        for (t, a) in self.impacts() {
            add_pulse(&mut force, sr, t, width, a);
        }
        let jitter = 1.0 + 0.04 * self.rng.signed();
        let f = tilt * jitter;
        // A dead slab: a low thud and short mid modes.
        let modes = [
            (120.0 / tilt.sqrt() * jitter * self.body, 0.05, 0.45),
            (640.0 * f, 0.07, 0.6),
            (1180.0 * f, 0.06, 0.55),
            (1900.0 * f, 0.05, 0.5),
            (2800.0 * f, 0.04, 0.4),
        ];
        let mut out = ring(&force, &modes, sr);
        // The sole's scuff (a short click and a longer gritty tail).
        let click = self.shoe.click();
        for (t, a) in self.impacts() {
            let r = self.rng.signed();
            let hz = 1800.0 * tilt * (1.0 + 0.05 * r);
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                t,
                hz,
                0.9,
                0.55 * a * click,
                0.0,
                0.007,
            );
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                t,
                hz * 0.8,
                0.7,
                0.1 * a,
                0.0,
                0.045,
            );
        }
        out
    }

    fn gravel(&mut self, n: usize) -> Vec<f32> {
        let sr = self.sr;
        let tilt = self.tilt();
        let mut out = vec![0.0f32; n];
        // The low thump of heel and toe.
        let mut force = vec![0.0f32; n];
        let width = self.width(2.5, 4.5);
        for (t, a) in self.impacts() {
            add_pulse(&mut force, sr, t, width, a);
        }
        let thump = ring(&force, &[(95.0 / tilt.sqrt() * self.body, 0.05, 0.5)], sr);
        add(&mut out, &thump, 1.0);
        // The crunch chain: clusters spaced 30+ ms apart from the heel on;
        // the toe is the cluster nearest its landing. Every random number is
        // drawn for the largest chain, whatever the weight, so that moving
        // `weight` or `pace` changes the step smoothly and never reshuffles
        // it: weight only fades the extra clusters and stones in.
        let spacing = lerp(0.046, 0.0365, self.pace);
        let count = lerp(10.0, 12.5, self.w);
        let mut times = vec![self.heel.0];
        for _ in 1..GRAVEL_CLUSTERS {
            let last = times.last().copied().unwrap_or(0.0);
            times.push(last + spacing * lerp(0.85, 1.4, self.rng.unit()));
        }
        let toe_k = (1..times.len())
            .min_by(|&a, &b| {
                (times[a] - self.toe.0)
                    .abs()
                    .total_cmp(&(times[b] - self.toe.0).abs())
            })
            .unwrap_or(0);
        if toe_k > 0 {
            times[toe_k] = times[toe_k].max(times[toe_k - 1] + 0.032);
        }
        let click = self.shoe.click().max(0.5);
        // Stone ticks: 1.6x at swish 0, 1x at 0.5, 0.4x at 1.
        let tick_gain = 1.0 - (self.swish - 0.5) * 1.2;
        for (k, &t) in times.iter().enumerate() {
            let fade = (count - k as f32).clamp(0.0, 1.0);
            let fall = (-(t - self.heel.0) / 0.45).exp();
            let jitter = lerp(0.8, 1.0, self.rng.unit());
            let strength = if k == 0 {
                self.heel.1
            } else if k == toe_k {
                self.toe.1
            } else {
                0.6 * fall * jitter
            } * fade;
            let stones = 4.0 + 4.0 * self.rng.unit() + 2.0 * self.w;
            for j in 0..GRAVEL_STONES {
                let on = (stones - j as f32).clamp(0.0, 1.0);
                let dt = self.rng.unit() * 0.008;
                let hz = 1700.0 * (4.5f32).powf(self.rng.unit()) * tilt;
                let q = lerp(2.0, 5.0, self.rng.unit());
                let decay = lerp(0.0018, 0.005, self.rng.unit());
                let amp =
                    strength * on * lerp(0.35, 1.0, self.rng.unit()) * 0.5 * click * tick_gain;
                add_noise(&mut out, &mut self.rng, sr, t + dt, hz, q, amp, 0.0, decay);
            }
        }
        // The swish bed (only above swish 0.5, drawn last so nothing else
        // moves): the grass band under heel and toe.
        let bed = (self.swish - 0.5).max(0.0) * 2.0;
        if bed > 0.0 {
            let attack = lerp(0.034, 0.046, self.w) * lerp(1.2, 0.8, self.pace);
            for (t, a, long) in [
                (self.heel.0, self.heel.1, 1.0),
                (self.toe.0, self.toe.1, 0.8),
            ] {
                let r = self.rng.signed();
                add_noise(
                    &mut out,
                    &mut self.rng,
                    sr,
                    t,
                    3100.0 * tilt * (1.0 + 0.06 * r),
                    0.55,
                    0.35 * a * bed,
                    attack * long,
                    lerp(0.04, 0.065, self.w),
                );
            }
        }
        out
    }

    fn grass(&mut self, n: usize) -> Vec<f32> {
        let sr = self.sr;
        let tilt = self.tilt();
        let mut out = vec![0.0f32; n];
        let attack = lerp(0.034, 0.046, self.w) * lerp(1.2, 0.8, self.pace);
        let swish_gain = 1.0 + (self.swish - 0.5) * 2.0;
        let tick_gain = 1.0 - (self.swish - 0.5) * 2.0;
        // The swishes: wide noise bands, slow rise, heel then toe.
        for (t, a, long) in [
            (self.heel.0, self.heel.1, 1.0),
            (self.toe.0, self.toe.1, 0.8),
        ] {
            let r = self.rng.signed();
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                t,
                3100.0 * tilt * (1.0 + 0.06 * r),
                0.55,
                0.9 * a * swish_gain,
                attack * long,
                lerp(0.04, 0.065, self.w),
            );
        }
        // The soft body: low noise, slow rise, no pitch.
        for (t, a) in self.impacts() {
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                t,
                140.0 * self.body,
                0.5,
                0.55 * a * lerp(0.7, 1.4, self.w),
                attack,
                0.05,
            );
        }
        // Stalks snapping: a few quiet ticks after each landing (all 13
        // drawn, the weight fades the extra ones in).
        let ticks = 4.0 + 6.0 * self.rng.unit() + 3.0 * self.w;
        for j in 0..13 {
            let on = (ticks - j as f32).clamp(0.0, 1.0);
            let base = if self.rng.unit() < 0.6 {
                self.heel.0
            } else {
                self.toe.0
            };
            let t = base + 0.01 + self.rng.unit() * 0.18;
            let hz = lerp(2200.0, 6000.0, self.rng.unit()) * tilt;
            let amp =
                lerp(0.06, 0.2, self.rng.unit()) * self.shoe.click().max(0.5) * on * tick_gain;
            add_noise(&mut out, &mut self.rng, sr, t, hz, 1.5, amp, 0.0, 0.0025);
        }
        out
    }

    /// A board creaking under the weight: a jittered stick-slip train
    /// through three board resonances (see `Footstep::creak`).
    fn board_creak(&mut self, n: usize, f0: f32, level: f32) -> Vec<f32> {
        let sr = self.sr;
        let start = self.heel.0 + 0.03 + 0.03 * self.rng.unit();
        let len = 0.15 + 0.15 * self.rng.unit();
        let rate = 70.0 + 60.0 * self.rng.unit();
        let mut force = vec![0.0f32; n];
        let mut t = start;
        while t < start + len {
            let u = (t - start) / len;
            // Swells and fades over the creak.
            let env = (std::f32::consts::PI * u).sin();
            let k = (t * sr).round() as usize;
            if k < n {
                force[k] += env * (1.0 + 0.3 * self.rng.signed());
            }
            t += (1.0 + 0.25 * self.rng.signed()) / rate;
        }
        let modes = [
            (f0 * 1.9, 0.05, 1.0),
            (f0 * 4.3, 0.035, 0.7),
            (f0 * 7.6, 0.025, 0.5),
        ];
        let mut y = self.wood_ring(&force, &modes, sr);
        let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let g = if peak > 0.0 {
            0.5 * level * self.heel.1 / peak
        } else {
            0.0
        };
        for v in &mut y {
            *v *= g;
        }
        y
    }

    fn wood(&mut self, n: usize) -> Vec<f32> {
        let sr = self.sr;
        let tilt = self.tilt();
        // Body: a long contact pulse into low modes.
        let mut body_force = vec![0.0f32; n];
        let width = self.width(4.0, 6.0);
        for (t, a) in self.impacts() {
            add_pulse(&mut body_force, sr, t, width, a);
        }
        // `ring` scales the board's decays; 1 (the default) multiplies by
        // exactly one, so old renders hold.
        let rk = self.ring.unwrap_or(1.0);
        let f0 = lerp(215.0, 105.0, self.w) * (1.0 + 0.04 * self.rng.signed()) * self.body;
        let body = self.wood_ring(
            &body_force,
            &[
                (f0, lerp(0.15, 0.24, self.w) * rk, 1.0),
                (f0 * 1.74, 0.1 * rk, 0.3),
                (f0 * 2.9, 0.06 * rk, 0.12),
            ],
            sr,
        );
        let mut out = body;
        // The tick: a sharp short pulse into the board's upper modes.
        let mut tick_force = vec![0.0f32; n];
        let w2 = self.width(0.3, 0.7);
        for (t, a) in self.impacts() {
            add_pulse(&mut tick_force, sr, t, w2, a);
        }
        let click = self.shoe.click();
        let top = self.wood_ring(
            &tick_force,
            &[
                (780.0 * tilt, 0.012, 1.0),
                (1350.0 * tilt, 0.008, 0.9),
                (2200.0 * tilt, 0.005, 0.8),
            ],
            sr,
        );
        add(&mut out, &top, click);
        for (t, a) in self.impacts() {
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                t,
                2600.0 * tilt,
                0.9,
                0.6 * a * click,
                0.0,
                0.003,
            );
        }
        if let Some(sc) = self.scuff {
            // The roll: swells from the heel to the middle, fades by the toe.
            let (heel, toe) = (self.heel, self.toe);
            let half = (0.5 * (toe.0 - heel.0)).max(0.01);
            add_noise(
                &mut out,
                &mut self.rng,
                sr,
                heel.0 + 0.008,
                2500.0 * tilt,
                0.7,
                0.35 * sc * heel.1 * click,
                half,
                half / 3.0,
            );
        }
        if let Some(cr) = self.creak {
            let creak = self.board_creak(n, f0, cr);
            add(&mut out, &creak, 1.0);
        }
        out
    }
}

impl Eval for Footstep {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let weight = self.weight.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&weight) {
            return Err(format!(
                "weight: {weight}: give a number from 0 (light) to 1 (heavy)"
            ));
        }
        let pace = match &self.pace {
            Some(p) => p.value()?,
            None => 0.5,
        };
        let shoe = Shoe::parse(self.shoe.as_deref().unwrap_or("sneaker"))?;
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let body = self.body.unwrap_or(1.0);
        if !(0.5..=2.0).contains(&body) {
            return Err(format!(
                "body: {body}: give a number from 0.5 (an octave down) to 2 (an octave up), default 1"
            ));
        }
        let swish = self.swish.unwrap_or(0.5);
        if !(0.0..=1.0).contains(&swish) {
            return Err(format!(
                "swish: {swish}: give a number from 0 (ticks only) to 1 (swish only), default 0.5"
            ));
        }
        let noise = match self.noise {
            None => None,
            Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => (x > 0.0).then_some(x),
            Some(x) => return Err(format!("noise: {x}: give 0..1 (wood only)")),
        };
        let unit = |field: &str, v: Option<f32>| -> Result<Option<f32>, String> {
            match v {
                None => Ok(None),
                Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => Ok((x > 0.0).then_some(x)),
                Some(x) => Err(format!("{field}: {x}: give 0..1 (wood only)")),
            }
        };
        let scuff = unit("scuff", self.scuff)?;
        let creak = unit("creak", self.creak)?;
        let ring = match self.ring {
            None => None,
            Some(x) if x.is_finite() && (0.25..=2.0).contains(&x) => Some(x),
            Some(x) => {
                return Err(format!(
                    "ring: {x}: give 0.25..2 (1 = as before; wood only)"
                ));
            }
        };
        let mut rng = cx.rng(self.seed);
        // Per-step variation, drawn first so every surface sees the same
        // numbers for the same seed: the gap, the two strengths.
        let gap = (0.14 - 0.07 * pace) * (1.0 + 0.07 * rng.signed());
        let level_db = 6.0 * (weight - 0.5) + 4.0 * (pace - 0.5);
        let level = 10f32.powf(level_db / 20.0);
        let heel_a = level * 10f32.powf(1.5 * rng.signed() / 20.0);
        let toe_a = level * 0.65 * 10f32.powf(1.5 * rng.signed() / 20.0);
        let mut step = Step {
            sr: cx.sr,
            w: weight,
            pace,
            shoe,
            body,
            swish,
            noise,
            scuff,
            creak,
            ring,
            heel: (at, heel_a),
            toe: (at + gap, toe_a),
            rng,
        };
        // Per-surface trim so a default step peaks near -12 dBFS.
        let trim_db = match self.surface {
            Ground::Concrete => -15.8,
            Ground::Gravel => -14.8,
            Ground::Grass => -23.6,
            Ground::Wood => -17.0,
        };
        let mut out = match self.surface {
            Ground::Concrete => step.concrete(cx.n),
            Ground::Gravel => step.gravel(cx.n),
            Ground::Grass => step.grass(cx.n),
            Ground::Wood => step.wood(cx.n),
        };
        let trim = 10f32.powf(trim_db / 20.0);
        for v in &mut out {
            *v = if v.is_finite() { *v * trim } else { 0.0 };
        }
        out.truncate(cx.n);
        Ok(one(Buf::Audio(out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloth_sizes_follow_the_power_law() {
        // P(size > s) = s^-1.3: the log-log slope of the survival function
        // over s = 1.5..10 is within 10 % of -1.3.
        let n = 200_000;
        let sizes: Vec<f32> = (0..n)
            .map(|i| cloth_size((i as f32 + 0.5) / n as f32))
            .collect();
        let surv = |s: f32| sizes.iter().filter(|x| **x > s).count() as f32 / n as f32;
        let (a, b) = (1.5f32, 10.0f32);
        let slope = (surv(b).ln() - surv(a).ln()) / (b.ln() - a.ln());
        println!("cloth size survival slope {slope:.3} (want -1.3)");
        assert!((slope + CLOTH_ALPHA).abs() < 0.13, "{slope}");
        assert!(sizes.iter().all(|s| *s >= 1.0 && *s <= CLOTH_MAX_SIZE));
    }
}
