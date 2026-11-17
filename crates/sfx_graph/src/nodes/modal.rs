//! Physical models: `modal` (a struck plate, bar, tube, membrane, string or
//! shell), `contact` (the force pulse of a strike) and `friction`
//! (stick-slip). Material constants and Hertz contact time come from foley
//! (`foley_synth::Material`, `hertz_time`); the plate mode table is computed
//! here from the same formulas as foley's `modal.rs` (those functions are
//! private there), for a `size` x `size` plate. The other shapes' tables are
//! below, each with its source.

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

/// Below this nothing is kept: a mode under 20 Hz is not heard.
const FLOOR_HZ: f32 = 20.0;

/// Most partials a table is asked for (bar roots, string harmonics, ring
/// orders): a cap on the work, far above what fits under 20 kHz.
const MAX_PARTIALS: usize = 200;

fn wave_speed(p: &MaterialProps) -> f32 {
    (p.young / (p.density * (1.0 - p.poisson * p.poisson))).sqrt()
}

/// One mode: Hz, T60 seconds, gain.
type Mode = (f32, f32, f32);

/// The highest mode frequency kept.
fn top_hz(sr: f32) -> f32 {
    max_hz(sr).min(20_000.0)
}

/// Seeded mistuning of the modes of a shape, as a fraction: a square plate
/// has degenerate (m, n) / (n, m) pairs, so it gets 1.5 %; the others only a
/// little, so their tables stay within a percent of the ideal ratios.
const PLATE_DETUNE: f32 = 0.015;
const SHAPE_DETUNE: f32 = 0.003;

/// The gentle high-frequency tilt (the highest of a dense table must not
/// crowd out the body). The plate also has its radiation term, below.
fn tilt(f: f32) -> f32 {
    1.0 / (1.0 + f / 8000.0)
}

/// What a plate, bar, tube, membrane, string or shell is, for the mode table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModalShape {
    /// A simply supported square plate, `size` a side (the default).
    #[default]
    Plate,
    /// A free-free bar of length `size` (xylophone key, tool, rod).
    Bar,
    /// A thin-walled tube of diameter `size`, struck on the wall (pipe, can).
    Tube,
    /// An ideal circular membrane of diameter `size` (drum skin).
    Membrane,
    /// A string of length `size`, with optional `stiffness`.
    #[serde(rename = "string")]
    Str,
    /// A bell / bowl / helmet: the church-bell partial series.
    Shell,
}

impl ModalShape {
    fn name(self) -> &'static str {
        match self {
            ModalShape::Plate => "plate",
            ModalShape::Bar => "bar",
            ModalShape::Tube => "tube",
            ModalShape::Membrane => "membrane",
            ModalShape::Str => "string",
            ModalShape::Shell => "shell",
        }
    }
}

// ------------------------------------------------------------ mode tables
//
// Sources of every ratio table below (from memory of the standard texts, and
// checked in the unit tests by recomputing them from the defining equations,
// not copied as numbers):
//
// - Plate: simply supported rectangular plate, f(m,n) ~ (m^2 + n^2) (Leissa,
//   "Vibration of Plates", 1969; Fletcher & Rossing, "The Physics of Musical
//   Instruments", 2nd ed. 1998, ch. 3). Foley's table, as before.
// - Bar: free-free Euler-Bernoulli bar. The frequency equation is
//   cos(x) cosh(x) = 1, roots x_k = beta_k L = 4.7300, 7.8532, 10.9956,
//   14.1372, then (k + 1/2) pi; f_k = x_k^2 / (2 pi) * (t / sqrt 12) * c / L^2
//   (Rayleigh, "Theory of Sound" s.170; Fletcher & Rossing s.2.19; Blevins,
//   "Formulas for Natural Frequency and Mode Shape", table 8-1). The ratios
//   are 1 : 2.757 : 5.404 : 8.933. The roots are solved here, not typed in.
//   Mode shape for the strike weight: cosh(bx) + cos(bx) - s (sinh(bx) +
//   sin(bx)), s = (cosh b - cos b) / (sinh b - sin b) (same references).
// - Membrane: ideal circular membrane, f(m,n) = f(0,1) * j(m,n) / j(0,1),
//   j(m,n) the n-th zero of the Bessel function J_m (Abramowitz & Stegun
//   table 9.5; Fletcher & Rossing s.3.? table "modes of an ideal circular
//   membrane"): 1 : 1.594 : 2.136 : 2.296 : 2.653 : 2.918 for (0,1) (1,1)
//   (2,1) (0,2) (3,1) (1,2). The zeros are found here by root-finding J_m
//   (integral definition); the strike weight of mode (m,n) at radius r is
//   |J_m(j(m,n) r / R)|.
// - String: f_n = n f_1 sqrt(1 + B n^2), B the inharmonicity coefficient
//   from stiffness (Fletcher, "Normal vibration frequencies of a stiff piano
//   string", JASA 36, 1964); B = 0 is the harmonic series 1 : 2 : 3 : 4.
// - Tube: the in-plane flexural ("ovalling") modes of a thin circular ring
//   (the cross-section of a pipe or can wall), inextensional theory:
//   omega_n = sqrt(E I / (rho A)) / R^2 * n (n^2 - 1) / sqrt(n^2 + 1), n =
//   2, 3, ... (Den Hartog, "Mechanical Vibrations"; Blevins table 8-?
//   "circular rings, in-plane flexure"; Love, "Mathematical Theory of
//   Elasticity" s.? for the thin ring): 1 : 2.828 : 5.423 : 8.771. Each n is
//   a degenerate cos / sin pair, struck with weights |cos n th|, |sin n th|.
// - Shell: the idealized church-bell partial series, relative to the prime:
//   hum 0.5, prime 1, tierce 1.2 (minor third), quint 1.5, nominal 2, then
//   2.5, 3, 4, 5, 6 (Rossing, "Science of Percussion Instruments" 2000, ch.
//   on bells; Fletcher & Rossing ch. 21 table of a Hemony-style bell, rounded
//   to the minor-third ideal). Bowls and helmets share the series here: an
//   honest simplification, the real ratios depend on the profile.
//
// Honest limits: no air loading, no membrane stiffness, no coupling of
// partials; `size` is a pitch knob, not a measurement.

/// The roots of cos(x) cosh(x) = 1 (free-free bar), `count` of them: found by
/// scanning for sign changes up to x = 40 and bisecting, then (k + 1/2) pi
/// (the error there is below e^-40).
fn bar_roots(count: usize) -> Vec<f64> {
    let g = |x: f64| x.cos() * x.cosh() - 1.0;
    let mut out = Vec::with_capacity(count);
    let (mut a, step) = (1.0f64, 0.01f64);
    while out.len() < count && a < 40.0 {
        let b = a + step;
        if (g(a) < 0.0) != (g(b) < 0.0) {
            let (mut lo, mut hi) = (a, b);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if (g(mid) < 0.0) == (g(lo) < 0.0) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            out.push(0.5 * (lo + hi));
        }
        a = b;
    }
    while out.len() < count {
        out.push((out.len() as f64 + 1.5) * std::f64::consts::PI);
    }
    out
}

/// |mode shape| of free-free bar mode with root `b` at `x0` (0..1 along the
/// bar). The shape is symmetric or antisymmetric about the middle, so x0 is
/// folded to 0..0.5. Past b = 25 the exact form loses all digits to
/// cancellation, so the standing-wave limit sqrt 2 cos(b x + pi/4) plus the
/// end term e^-bx is used (it agrees at x = 0: 2).
fn bar_shape(b: f64, x0: f64) -> f64 {
    let x = x0.min(1.0 - x0);
    if b <= 25.0 {
        let s = (b.cosh() - b.cos()) / (b.sinh() - b.sin());
        ((b * x).cosh() + (b * x).cos() - s * ((b * x).sinh() + (b * x).sin())).abs()
    } else {
        (2f64.sqrt() * (b * x + std::f64::consts::FRAC_PI_4).cos() + (-b * x).exp()).abs()
    }
}

/// J_m(x) from its integral, (1 / 2 pi) int cos(m t - x sin t) dt over a
/// period: the rectangle rule is exact to rounding once there are more points
/// than m + x.
fn bessel_j(m: usize, x: f64) -> f64 {
    let n = 64 + 2 * (m + x.abs() as usize);
    let mut s = 0.0;
    for i in 0..n {
        let t = std::f64::consts::TAU * (i as f64 + 0.5) / n as f64;
        s += (m as f64 * t - x * t.sin()).cos();
    }
    s / n as f64
}

/// Highest order and zero of the Bessel table (j <= 40, m <= 40).
const BESSEL_MAX: usize = 40;

/// `zeros()[m]` = the zeros of J_m up to 40, ascending: the first few are
/// 2.4048 3.8317 5.1356 for m = 0 1 2 (first zero of each order).
fn bessel_zeros() -> &'static Vec<Vec<f64>> {
    static ZEROS: std::sync::OnceLock<Vec<Vec<f64>>> = std::sync::OnceLock::new();
    ZEROS.get_or_init(|| {
        (0..=BESSEL_MAX)
            .map(|m| {
                let mut out = Vec::new();
                // J_m(x) > 0 for x below its first zero, which is above m.
                let mut a = (m as f64).max(0.5);
                while a < BESSEL_MAX as f64 {
                    let b = a + 0.1;
                    let (ja, jb) = (bessel_j(m, a), bessel_j(m, b));
                    if (ja < 0.0) != (jb < 0.0) {
                        let (mut lo, mut hi, mut jlo) = (a, b, ja);
                        for _ in 0..50 {
                            let mid = 0.5 * (lo + hi);
                            let jm = bessel_j(m, mid);
                            if (jm < 0.0) == (jlo < 0.0) {
                                lo = mid;
                                jlo = jm;
                            } else {
                                hi = mid;
                            }
                        }
                        out.push(0.5 * (lo + hi));
                    }
                    a = b;
                }
                out
            })
            .collect()
    })
}

/// Thin-ring in-plane flexure factor n (n^2 - 1) / sqrt(n^2 + 1).
fn ring_factor(n: usize) -> f32 {
    let n = n as f32;
    n * (n * n - 1.0) / (n * n + 1.0).sqrt()
}

/// The church-bell partials relative to the prime (see the table notes).
const BELL: [f32; 10] = [0.5, 1.0, 1.2, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0];

/// The lowest mode of a shape as `(a, p)`: it sits at `a / size^p` Hz. The
/// same constants build the tables below, so the size hint cannot drift from
/// what renders.
fn lowest_mode(shape: ModalShape, props: &MaterialProps) -> (f32, f32) {
    let c = wave_speed(props);
    // The plate's (1,1) at one metre: pi/2 t c / sqrt 12 x 2.
    let plate = PI * props.thickness * c / 12f32.sqrt();
    match shape {
        ModalShape::Plate | ModalShape::Shell => {
            let a = if shape == ModalShape::Shell {
                0.5 * plate
            } else {
                plate
            };
            (a, 2.0)
        }
        ModalShape::Bar => {
            let k = props.thickness / 12f32.sqrt() * (props.young / props.density).sqrt()
                / (2.0 * PI);
            ((bar_roots(1)[0].powi(2)) as f32 * k, 2.0)
        }
        ModalShape::Tube => {
            let k = c * props.thickness / (12f32.sqrt() * 2.0 * PI);
            // R = size / 2: 1 / R^2 = 4 / size^2.
            (4.0 * k * ring_factor(2), 2.0)
        }
        ModalShape::Membrane | ModalShape::Str => (plate, 1.0),
    }
}

/// `(lo, hi)` metres between which the lowest mode falls from 10 kHz (or half
/// the top of the band, if that is lower) to 40 Hz, rounded inward to
/// two figures.
fn size_range(shape: ModalShape, props: &MaterialProps, sr: f32) -> (f32, f32) {
    let (a, p) = lowest_mode(shape, props);
    let at = |f: f32| (a / f).powf(1.0 / p);
    let round = |v: f32, up: bool| {
        let k = 10f32.powf(1.0 - v.log10().floor());
        (if up { (v * k).ceil() } else { (v * k).floor() }) / k
    };
    (
        round(at(10_000f32.min(0.5 * top_hz(sr))), true),
        round(at(40.0), false),
    )
}

/// One candidate mode before selection: Hz and strike weight.
type Cand = (f32, f32);

/// The plate's candidates: every (m, n) with a mode in 20 Hz .. top, as
/// `k (m^2 + n^2)`, weighted by the strike (`x0`, `y0`) and the radiation.
fn plate_candidates(
    p: &MaterialProps,
    size: f32,
    x0: f32,
    y0: f32,
    sr: f32,
    rng: &mut crate::dsp::Rng,
) -> Vec<Cand> {
    let c = wave_speed(p);
    let k = PI / 2.0 * p.thickness * c / 12f32.sqrt() / (size * size);
    let coincidence = SPEED_OF_SOUND * SPEED_OF_SOUND / (1.8 * p.thickness * c);
    let top = top_hz(sr);
    let mut all: Vec<Cand> = Vec::new();
    'm: for m in 1..=200u32 {
        let mut any = false;
        for n in 1..=200u32 {
            let f = k * ((m * m + n * n) as f32);
            if f > top {
                break;
            }
            any = true;
            if f < FLOOR_HZ {
                continue;
            }
            let shape = (m as f32 * PI * x0).sin() * (n as f32 * PI * y0).sin();
            // How well the mode radiates, and a gentle tilt so the highest
            // of a dense table do not crowd out the body.
            let w = shape.abs() * (f / (f + coincidence)).sqrt() / (1.0 + f / 8000.0);
            // Real blocks are not ideal plates: a little seeded mistuning.
            all.push((f * (1.0 + PLATE_DETUNE * rng.signed()), w));
        }
        if !any {
            break 'm;
        }
    }
    all
}

/// Keeps `count`: the lowest half (so the fundamental and its neighbours are
/// always there: a crowded table of a big plate must not lose its pitch),
/// then the strongest of the rest; drops those under 1e-4 of the strongest;
/// gives each a T60 from the loss and a gain relative to the strongest.
fn select(mut all: Vec<Cand>, count: usize, p: &MaterialProps) -> Vec<Mode> {
    let strongest = all.iter().fold(0.0f32, |m, a| m.max(a.1)).max(1e-12);
    all.retain(|a| a.1 > 1e-4 * strongest);
    all.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)));
    let low = count.div_ceil(2).min(all.len());
    let mut rest = all.split_off(low);
    rest.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    rest.truncate(count - low);
    all.extend(rest);
    all.sort_by(|a, b| a.0.total_cmp(&b.0));
    all.into_iter()
        .map(|(f, w)| {
            // Amplitude e-fold rate pi f eta, as foley: T60 = ln(1000) / rate.
            let t60 = (1000f32.ln() / (PI * f * p.loss.max(1e-5))).clamp(0.002, 20.0);
            (f, t60, w / strongest)
        })
        .collect()
}

/// What a strike and a shape need to build a table.
struct Table<'a> {
    shape: ModalShape,
    props: &'a MaterialProps,
    size: f32,
    /// `strike_position`, clamped to 0.02..0.98.
    pos: f32,
    stiffness: f32,
    count: usize,
    sr: f32,
}

impl Table<'_> {
    /// The modes, lowest first.
    fn modes(&self, rng: &mut crate::dsp::Rng) -> Vec<Mode> {
        let (p, size, pos, sr) = (self.props, self.size, self.pos, self.sr);
        let top = top_hz(sr);
        let (a, power) = lowest_mode(self.shape, p);
        let mut all: Vec<Cand> = Vec::new();
        match self.shape {
            ModalShape::Plate => {
                // The point on the plate: x is the position, y follows it so
                // that 0.5 is the centre in both.
                all = plate_candidates(p, size, pos, 0.25 + 0.5 * pos, sr, rng);
            }
            ModalShape::Bar => {
                let k = a / bar_roots(1)[0].powi(2) as f32;
                for b in bar_roots(MAX_PARTIALS) {
                    let f = (b * b) as f32 * k / (size * size);
                    if f > top {
                        break;
                    }
                    if f < FLOOR_HZ {
                        continue;
                    }
                    let w = bar_shape(b, f64::from(pos)) as f32 * tilt(f);
                    all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), w));
                }
            }
            ModalShape::Tube => {
                // f(n) = a / size^2 * ring_factor(n) / ring_factor(2).
                let theta = 2.0 * PI * pos;
                for n in 2..=MAX_PARTIALS.min(60) {
                    let f = a / (size * size) * ring_factor(n) / ring_factor(2);
                    if f > top {
                        break;
                    }
                    if f < FLOOR_HZ {
                        continue;
                    }
                    // A degenerate cos / sin pair, split a little.
                    let (c, s) = ((n as f32 * theta).cos().abs(), (n as f32 * theta).sin().abs());
                    all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), c * tilt(f)));
                    all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), s * tilt(f)));
                }
            }
            ModalShape::Membrane => {
                let zeros = bessel_zeros();
                let f01 = a / size.powf(power);
                let j01 = zeros[0][0];
                for (m, zs) in zeros.iter().enumerate() {
                    for &j in zs {
                        let f = f01 * (j / j01) as f32;
                        if f > top {
                            break;
                        }
                        if f < FLOOR_HZ {
                            continue;
                        }
                        // Struck at radius `pos` R, on the cos(m th) axis.
                        let w = bessel_j(m, j * f64::from(pos)).abs() as f32 * tilt(f);
                        all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), w));
                    }
                }
            }
            ModalShape::Str => {
                let f1 = a / size.powf(power);
                for n in 1..=MAX_PARTIALS {
                    let nf = n as f32;
                    let f = nf * f1 * (1.0 + self.stiffness * nf * nf).sqrt();
                    if f > top {
                        break;
                    }
                    if f < FLOOR_HZ {
                        continue;
                    }
                    // Struck at `pos` of the length; a hammer's 1 / n slope.
                    let w = (nf * PI * pos).sin().abs() / nf * tilt(f);
                    all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), w));
                }
            }
            ModalShape::Shell => {
                let prime = 2.0 * a / (size * size);
                for r in BELL {
                    let f = prime * r;
                    if f > top {
                        break;
                    }
                    if f < FLOOR_HZ {
                        continue;
                    }
                    all.push((f * (1.0 + SHAPE_DETUNE * rng.signed()), tilt(f)));
                }
            }
        }
        select(all, self.count, p)
    }
}

/// How many samples of the unit-impulse response are searched for its peak
/// (170 ms at 48 kHz: a quarter period of 20 Hz is 12 ms).
const NORM_LEN: usize = 8192;

/// The peak of the bank's response to a unit impulse (1.0 at sample 0).
fn impulse_peak(modes: &[Mode], sr: f32) -> f32 {
    let mut bank: Vec<Resonator> = modes
        .iter()
        .map(|&(f, t60, g)| Resonator::new(f, t60, g, sr))
        .collect();
    let mut peak = 0.0f32;
    for i in 0..NORM_LEN {
        let x = if i == 0 { 1.0 } else { 0.0 };
        let y: f32 = bank.iter_mut().map(|r| r.tick(x)).sum();
        peak = peak.max(y.abs());
    }
    peak
}

/// A struck object: the `excite` audio rings a bank of modes computed from a
/// material, a size and a shape. A unit impulse (or a `contact` of area 1)
/// peaks at 0 dBFS, whatever the shape, size and material.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Modal {
    /// The strike: a `contact` pulse, an impulse, a noise burst, a `friction`
    /// stream.
    pub excite: In,
    /// stone wood metal glass ice earth granular plant soft snow, or
    /// {young, density, poisson, loss, thickness}.
    pub material: MaterialSpec,
    /// Side of the plate, length of the bar or string, diameter of the tube
    /// or membrane, metres; the bell's size. Bigger rings lower (pitch goes
    /// as 1 / size^2; for membrane and string as 1 / size). Read once, at the
    /// start (a `$param` or `=expr` works). An out-of-range size is an error
    /// that names the range for the material and shape.
    pub size: In,
    /// What is struck: plate (default) bar tube membrane string shell.
    #[serde(default)]
    pub shape: ModalShape,
    /// Where it is struck, 0..1 (default 0.3). Plate: 0.5 is the centre,
    /// which silences every mode with a node there. Bar, string: along the
    /// length. Membrane: radius, 0 the centre. Tube: around the wall. Shell:
    /// ignored (a bell is struck on the lip).
    #[serde(default)]
    pub strike_position: Option<In>,
    /// Most modes kept (default 48, 1..256). The lowest half of them are
    /// always the lowest modes of the object, the rest the strongest.
    #[serde(default)]
    pub modes: Option<usize>,
    /// String only: the inharmonicity B in f_n = n f_1 sqrt(1 + B n^2),
    /// 0..1 (default 0: exactly harmonic; a piano string is about 0.0004).
    #[serde(default)]
    pub stiffness: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// mistuning.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Modal {
    const BLURB: &'static str = "a struck object: excite audio rings modes computed from material, size, shape (plate bar tube membrane string shell) and strike position (bigger = lower); a unit impulse peaks near 0 dBFS";
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
        let stiffness = match self.stiffness {
            None => 0.0,
            Some(b) if self.shape != ModalShape::Str => {
                return Err(format!(
                    "stiffness: {b}: only the \"string\" shape takes it (this is \"{}\"): drop it or set shape \"string\"",
                    self.shape.name()
                ));
            }
            Some(b) if b.is_finite() && (0.0..=1.0).contains(&b) => b,
            Some(b) => return Err(format!("stiffness: {b}: give 0..1")),
        };
        let mut rng = cx.rng(self.seed);
        let table = Table {
            shape: self.shape,
            props: &props,
            size,
            pos,
            stiffness,
            count,
            sr: cx.sr,
        };
        let mut modes = table.modes(&mut rng);
        if modes.is_empty() {
            let (lo, hi) = size_range(self.shape, &props, cx.sr);
            let what = match &self.material {
                MaterialSpec::Name(n) => format!("{n} {}", self.shape.name()),
                MaterialSpec::Props(_) => format!("this material's {}", self.shape.name()),
            };
            return Err(format!(
                "size {size}: no mode of the {what} falls between 20 Hz and the top of the band: for {what} try {lo} to {hi} m (lowest mode from 10 kHz down to 40 Hz)"
            ));
        }
        // A unit impulse peaks at 0 dBFS (the raw sum of many modes was +22).
        let peak = impulse_peak(&modes, cx.sr);
        if peak > 1e-12 {
            let scale = 1.0 / peak;
            for m in &mut modes {
                m.2 *= scale;
            }
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


#[cfg(test)]
mod tests {
    use super::*;

    const MATERIALS: [&str; 10] = [
        "stone", "wood", "metal", "glass", "ice", "earth", "granular", "plant", "soft", "snow",
    ];
    const SHAPES: [ModalShape; 6] = [
        ModalShape::Plate,
        ModalShape::Bar,
        ModalShape::Tube,
        ModalShape::Membrane,
        ModalShape::Str,
        ModalShape::Shell,
    ];

    fn props(name: &str) -> MaterialProps {
        MaterialSpec::Name(name.into()).props().unwrap()
    }

    fn table(shape: ModalShape, p: &MaterialProps, size: f32, pos: f32) -> Vec<Mode> {
        let mut rng = crate::dsp::Rng::new(1);
        Table {
            shape,
            props: p,
            size,
            pos,
            stiffness: 0.0,
            count: 48,
            sr: 48_000.0,
        }
        .modes(&mut rng)
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a / b - 1.0).abs() < rel
    }

    #[test]
    fn the_tables_come_out_of_their_equations() {
        let r = bar_roots(6);
        let ratios: Vec<f64> = r.iter().map(|x| x * x / (r[0] * r[0])).collect();
        println!("bar roots {r:.4?}, ratios {ratios:.4?}");
        assert!(close(r[0], 4.7300, 1e-4) && close(r[1], 7.8532, 1e-4));
        for (got, want) in ratios.iter().zip([1.0, 2.757, 5.404, 8.933]) {
            assert!(close(*got, want, 5e-4), "{got} vs {want}");
        }
        let z = bessel_zeros();
        let ratios = [z[0][0], z[1][0], z[2][0], z[0][1], z[3][0]].map(|j| j / z[0][0]);
        println!(
            "membrane j01 {:.4}, ratios {ratios:.4?}",
            z[0][0]
        );
        for (got, want) in ratios.iter().zip([1.0, 1.594, 2.136, 2.295, 2.653]) {
            assert!(close(*got, want, 1e-3), "{got} vs {want}");
        }
        let ring: Vec<f32> = (2..=5).map(|n| ring_factor(n) / ring_factor(2)).collect();
        println!("ring ratios {ring:.4?}");
        for (got, want) in ring.iter().zip([1.0, 2.828, 5.423, 8.771]) {
            assert!(close(f64::from(*got), want, 1e-3), "{got} vs {want}");
        }
    }

    /// The lowest mode of every material and shape falls with size over the
    /// range the hint gives, and sits where `lowest_mode` says.
    #[test]
    fn the_lowest_mode_falls_with_size_for_every_material_and_shape() {
        for name in MATERIALS {
            let p = props(name);
            for shape in SHAPES {
                let (lo, hi) = size_range(shape, &p, 48_000.0);
                assert!(lo < hi, "{name} {shape:?}: {lo} .. {hi}");
                let (a, pw) = lowest_mode(shape, &p);
                let mut last = f32::MAX;
                let mut size = lo;
                let mut steps = 0;
                while size <= hi {
                    let m = table(shape, &p, size, 0.3);
                    let f = m.first().unwrap_or_else(|| panic!("{name} {shape:?} {size}")).0;
                    let want = a / size.powf(pw);
                    assert!(f < last, "{name} {shape:?} size {size}: {f} Hz after {last}");
                    assert!(
                        (f / want - 1.0).abs() < 0.02,
                        "{name} {shape:?} size {size}: lowest {f} Hz, table {want} Hz"
                    );
                    last = f;
                    size *= 1.12;
                    steps += 1;
                }
                println!(
                    "{name} {}: sizes {lo} .. {hi} m, {steps} steps, lowest mode at the end {last:.1} Hz",
                    shape.name()
                );
                assert!(steps >= 2);
                // At the top of the range the lowest mode is 40 Hz or more.
                assert!(last >= 38.0, "{name} {shape:?}: {last}");
            }
        }
    }

    #[test]
    fn the_reported_wood_sizes_keep_their_fundamental() {
        let p = props("wood");
        for (size, want) in [(0.5, 425.0), (0.8, 166.0), (1.4, 54.2)] {
            let m = table(ModalShape::Plate, &p, size, 0.3);
            println!("wood {size} m: lowest {:.1} Hz (table {want})", m[0].0);
            assert!((m[0].0 / want - 1.0).abs() < 0.03, "{size}: {}", m[0].0);
        }
    }
}
