//! Physical models: `modal` (a struck plate), `contact` (the force pulse of a
//! strike) and `friction` (stick-slip). Material constants and Hertz contact
//! time come from foley (`foley_synth::Material`, `hertz_time`); the plate
//! mode table is computed here from the same formulas as foley's `modal.rs`
//! (those functions are private there), for a `size` x `size` plate.

use std::f32::consts::PI;

use foley_synth::{Material, MaterialProps, hertz_time};
use serde::Deserialize;

use super::{mono, non_negative, one, per_channel, positive};
use crate::dsp::{Resonator, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// A material: one of foley's names, or the plate's physical constants.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum MaterialSpec {
    /// stone wood metal glass ice earth granular plant soft snow.
    Name(String),
    Props(PlateProps),
}

/// A plate's constants.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlateProps {
    /// Young's modulus, Pa.
    pub young: f32,
    /// Density, kg/m3.
    pub density: f32,
    /// Poisson's ratio, 0..0.5.
    pub poisson: f32,
    /// Loss factor (damping): steel 0.004, wood 0.025.
    pub loss: f32,
    /// Plate thickness, m.
    pub thickness: f32,
}

impl MaterialSpec {
    fn props(&self) -> Result<MaterialProps, String> {
        match self {
            MaterialSpec::Name(n) => {
                let m = match n.to_ascii_lowercase().as_str() {
                    "stone" => Material::Stone,
                    "wood" => Material::Wood,
                    "metal" => Material::Metal,
                    "glass" => Material::Glass,
                    "ice" => Material::Ice,
                    "earth" => Material::Earth,
                    "granular" => Material::Granular,
                    "plant" => Material::Plant,
                    "soft" => Material::Soft,
                    "snow" => Material::Snow,
                    other => {
                        return Err(format!(
                            "material: unknown material `{other}`: use stone, wood, metal, glass, ice, earth, granular, plant, soft, snow, or {{young, density, poisson, loss, thickness}}"
                        ));
                    }
                };
                Ok(m.props())
            }
            MaterialSpec::Props(p) => {
                positive("material.young", p.young)?;
                positive("material.density", p.density)?;
                positive("material.thickness", p.thickness)?;
                non_negative("material.loss", p.loss)?;
                if !(0.0..0.5).contains(&p.poisson) {
                    return Err(format!(
                        "material.poisson: {}: give a ratio in 0..0.5",
                        p.poisson
                    ));
                }
                let mut m = Material::Stone.props();
                m.young = p.young;
                m.density = p.density;
                m.poisson = p.poisson;
                m.loss = p.loss;
                m.thickness = p.thickness;
                Ok(m)
            }
        }
    }
}

const SPEED_OF_SOUND: f32 = 343.0;

fn wave_speed(p: &MaterialProps) -> f32 {
    (p.young / (p.density * (1.0 - p.poisson * p.poisson))).sqrt()
}

/// One mode: Hz, T60 seconds, gain.
type Mode = (f32, f32, f32);

/// The modes of a simply supported `size` x `size` plate struck at
/// (`x0`, `y0`), strongest `count`, lowest first.
fn plate_modes(
    p: &MaterialProps,
    size: f32,
    x0: f32,
    y0: f32,
    count: usize,
    sr: f32,
    rng: &mut crate::dsp::Rng,
) -> Vec<Mode> {
    let c = wave_speed(p);
    let k = PI / 2.0 * p.thickness * c / 12f32.sqrt() / (size * size);
    let coincidence = SPEED_OF_SOUND * SPEED_OF_SOUND / (1.8 * p.thickness * c);
    let top = max_hz(sr).min(20_000.0);
    let mut all: Vec<(f32, f32)> = Vec::new();
    'm: for m in 1..=200u32 {
        let mut any = false;
        for n in 1..=200u32 {
            let f = k * ((m * m + n * n) as f32);
            if f > top {
                break;
            }
            any = true;
            if f < 20.0 {
                continue;
            }
            let shape = (m as f32 * PI * x0).sin() * (n as f32 * PI * y0).sin();
            // How well the mode radiates, and a gentle tilt so the highest
            // of a dense table do not crowd out the body.
            let w = shape.abs() * (f / (f + coincidence)).sqrt() / (1.0 + f / 8000.0);
            // Real blocks are not ideal plates: a little seeded mistuning.
            all.push((f * (1.0 + 0.015 * rng.signed()), w));
        }
        if !any {
            break 'm;
        }
    }
    all.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    all.truncate(count);
    let strongest = all.first().map_or(1.0, |a| a.1).max(1e-12);
    let mut out: Vec<Mode> = all
        .into_iter()
        .filter(|(_, w)| *w > 1e-4 * strongest)
        .map(|(f, w)| {
            // Amplitude e-fold rate pi f eta, as foley: T60 = ln(1000) / rate.
            let t60 = (1000f32.ln() / (PI * f * p.loss.max(1e-5))).clamp(0.002, 20.0);
            (f, t60, w / strongest)
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// A struck plate: the `excite` audio rings a bank of modes computed from a
/// material and a size.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Modal {
    /// The strike: a `contact` pulse, an impulse, a noise burst, a `friction`
    /// stream.
    pub excite: In,
    /// stone wood metal glass ice earth granular plant soft snow, or
    /// {young, density, poisson, loss, thickness}.
    pub material: MaterialSpec,
    /// Side of the (square) plate, metres. Bigger rings lower (pitch goes as
    /// 1 / size^2). Read once, at the start (a `$param` or `=expr` works).
    pub size: In,
    /// Where the plate is struck, 0..1 (default 0.3). 0.5 is the centre,
    /// which silences every mode with a node there.
    #[serde(default)]
    pub strike_position: Option<In>,
    /// Most modes kept (default 48, 1..256).
    #[serde(default)]
    pub modes: Option<usize>,
    /// Mixed with the document seed and the node name (default 0): the
    /// mistuning.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Modal {
    const BLURB: &'static str = "a struck plate: excite audio rings modes computed from material, size and strike position (bigger = lower)";
    fn example() -> serde_json::Value {
        serde_json::json!({"excite": "contact1", "material": "wood", "size": 0.5,
            "strike_position": 0.3, "modes": 48})
    }
}

impl Eval for Modal {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("excite", &self.excite)?;
        let props = self.material.props()?;
        let size = positive("size", cx.mono("size", &self.size)?.at(0))?;
        let pos = cx
            .mono_or("strike_position", &self.strike_position, 0.3)?
            .at(0)
            .clamp(0.02, 0.98);
        let count = self.modes.unwrap_or(48);
        if !(1..=256).contains(&count) {
            return Err(format!("modes: {count}: give 1..256"));
        }
        let mut rng = cx.rng(self.seed);
        // The point on the plate: x is the position, y follows it so that 0.5
        // is the centre in both.
        let modes = plate_modes(&props, size, pos, 0.25 + 0.5 * pos, count, cx.sr, &mut rng);
        if modes.is_empty() {
            return Err(format!(
                "size {size}: no mode falls between 20 Hz and the top of the band (material and size set the pitch): try a size between about 0.05 and 5 m"
            ));
        }
        Ok(one(per_channel(&input, cx.n, |x| {
            let mut out = vec![0.0f32; x.len()];
            for &(f, t60, g) in &modes {
                let mut r = Resonator::new(f, t60, g, cx.sr);
                for (o, v) in out.iter_mut().zip(x) {
                    *o += r.tick(*v);
                }
            }
            out
        })))
    }
}

/// The force pulse of one strike: a half-sine as long as the Hertz contact.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    /// Impact speed, m/s (a signal: read at `at`).
    pub speed: In,
    /// Mass of the striker, kg. Heavier = longer, duller.
    pub mass: In,
    /// What is struck (a name or constants, as `modal`).
    pub material: MaterialSpec,
    /// Striker hardness 0..1 (default 0.7): 0 rubber (long, dull) to 1 steel.
    #[serde(default)]
    pub hardness: Option<f32>,
    /// When the pulse starts, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
}

impl Describe for Contact {
    const BLURB: &'static str = "the force pulse of a strike (Hertz contact time: heavier, slower, softer = longer, duller); excites modal";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": 1.5, "mass": 0.2, "material": "wood", "hardness": 0.7, "at": 0.0})
    }
}

impl Eval for Contact {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let props = self.material.props()?;
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let start = cx.at(at);
        let speed = positive("speed", cx.mono("speed", &self.speed)?.at(start))?;
        let mass = positive("mass", cx.mono("mass", &self.mass)?.at(start))?;
        let hardness = self.hardness.unwrap_or(0.7);
        if !(0.0..=1.0).contains(&hardness) {
            return Err(format!("hardness: {hardness}: give 0..1"));
        }
        // The striker: a ball of density 2000 kg/m3 whose modulus runs from
        // 1 MPa (rubber) to 200 GPa (steel) with `hardness`.
        let radius = (3.0 * mass / (4.0 * PI * 2000.0)).cbrt();
        let e_striker = 1e6 * (2e5f32).powf(hardness);
        let nu = 0.3f32;
        let e_star = 1.0
            / ((1.0 - nu * nu) / e_striker + (1.0 - props.poisson * props.poisson) / props.young);
        let tau = hertz_time(mass, radius, e_star, speed).clamp(2.0 / cx.sr, 0.25);
        // Unit area is a 0.1 kg strike at 1 m/s; the impulse is mass x speed.
        let area = mass * speed / 0.1;
        let len = (tau * cx.sr).round().max(2.0);
        let norm = PI / 2.0 * area / len;
        Ok(one(mono(cx.n, |i| {
            if i < start {
                return 0.0;
            }
            let k = (i - start) as f32 + 0.5;
            if k < len {
                norm * (PI * k / len).sin()
            } else {
                0.0
            }
        })))
    }
}

/// Stick-slip: a sawtooth force whose rate follows the speed. Feed it to a
/// `modal` for creaks, squeaks and scrapes.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Friction {
    /// Sliding speed, 0.. (a signal: a ramp makes a creak that rises). Slip
    /// rate in Hz = speed x `grain`; 0 is silence.
    pub speed: In,
    /// Normal force, linear amplitude (a signal).
    pub pressure: In,
    /// 0..1 (default 0): seeded jitter in each slip's timing and size, plus
    /// noise. 0 is a clean saw; 1 a scrape.
    #[serde(default)]
    pub roughness: Option<f32>,
    /// Slips a second at speed 1 (default 200).
    #[serde(default)]
    pub grain: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

/// The stick-slip waveform at phase `ph` (0..1): a linear climb (stick) over
/// 90 % of the cycle, then a smooth drop (slip).
fn stick_slip(ph: f32) -> f32 {
    const STICK: f32 = 0.9;
    if ph < STICK {
        2.0 * ph / STICK - 1.0
    } else {
        let u = (ph - STICK) / (1.0 - STICK);
        1.0 - 2.0 * 0.5 * (1.0 - (PI * u).cos())
    }
}

impl Describe for Friction {
    const BLURB: &'static str = "stick-slip sawtooth whose rate follows speed and level follows pressure (creaks, squeaks, scrapes); excites modal";
    fn example() -> serde_json::Value {
        serde_json::json!({"speed": "sweep", "pressure": 0.5, "roughness": 0.2, "grain": 200})
    }
}

impl Eval for Friction {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let speed = cx.mono("speed", &self.speed)?;
        let pressure = cx.mono("pressure", &self.pressure)?;
        let rough = self.roughness.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&rough) {
            return Err(format!("roughness: {rough}: give 0..1"));
        }
        let grain = positive("grain", self.grain.unwrap_or(200.0))?;
        let mut rng = cx.rng(self.seed);
        let top = max_hz(cx.sr) / 2.0;
        let mut ph = 0.0f32;
        let mut cycle_rate = 1.0f32; // per-cycle timing factor
        let mut cycle_amp = 1.0f32;
        let mut y1 = 0.0f32; // a soft low-pass keeps the noise from hissing
        Ok(one(mono(cx.n, |i| {
            let v = speed.at(i);
            let v = if v.is_finite() { v.abs() } else { 0.0 };
            let hz = (v * grain).min(top);
            ph += hz * cycle_rate / cx.sr;
            if ph >= 1.0 {
                ph -= ph.floor();
                cycle_rate = 1.0 + 0.5 * rough * rng.signed();
                cycle_amp = 1.0 + 0.4 * rough * rng.signed();
            }
            // Standing still makes no sound; the gate avoids a click.
            let gate = (v / 0.01).min(1.0);
            let noise = rng.signed();
            y1 += 0.5 * (noise - y1);
            let p = pressure.at(i);
            let p = if p.is_finite() { p } else { 0.0 };
            let s = stick_slip(ph.clamp(0.0, 0.999_999)) * cycle_amp + 0.5 * rough * y1;
            s * p * gate
        })))
    }
}
