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
use crate::dsp::{FilterMode, Resonator, Svf, max_hz};
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
// Sources of every ratio table below. The references are the standard texts
// as cited from memory (chapter-level, not page-level: not re-checked against
// the books). The ratios themselves are not typed in: the unit tests below
// recompute them from the defining equations and compare with the published
// values (bar 2.757 / 5.404 / 8.933, membrane 1.593 / 2.136 / 2.295 / 2.653).
//
// - Plate: simply supported rectangular plate, f(m,n) ~ (m^2 + n^2) (Leissa,
//   "Vibration of Plates", 1969; Fletcher & Rossing, "The Physics of Musical
//   Instruments", 2nd ed. 1998, the chapter on plates). Foley's table, as before.
// - Bar: free-free Euler-Bernoulli bar. The frequency equation is
//   cos(x) cosh(x) = 1, roots x_k = beta_k L = 4.7300, 7.8532, 10.9956,
//   14.1372, then (k + 1/2) pi; f_k = x_k^2 / (2 pi) * (t / sqrt 12) * c / L^2
//   (Rayleigh, "Theory of Sound" vol. 1, bars; Fletcher & Rossing, the chapter on bars,
//   free bars; Blevins, "Formulas for Natural Frequency and Mode Shape",
//   the chapter on beams, free-free). The ratios
//   are 1 : 2.757 : 5.404 : 8.933. The roots are solved here, not typed in.
//   Mode shape for the strike weight: cosh(bx) + cos(bx) - s (sinh(bx) +
//   sin(bx)), s = (cosh b - cos b) / (sinh b - sin b) (same references).
// - Membrane: ideal circular membrane, f(m,n) = f(0,1) * j(m,n) / j(0,1),
//   j(m,n) the n-th zero of the Bessel function J_m (Abramowitz & Stegun
//   table 9.5; Fletcher & Rossing, the chapter on drums, ideal circular membrane):
//   1 : 1.594 : 2.136 : 2.296 : 2.653 : 2.918 for (0,1) (1,1)
//   (2,1) (0,2) (3,1) (1,2). The zeros are found here by root-finding J_m
//   (integral definition); the strike weight of mode (m,n) at radius r is
//   |J_m(j(m,n) r / R)|.
// - String: f_n = n f_1 sqrt(1 + B n^2), B the inharmonicity coefficient
//   from stiffness (Fletcher, "Normal vibration frequencies of a stiff piano
//   string", JASA 36, 1964); B = 0 is the harmonic series 1 : 2 : 3 : 4.
// - Tube: the in-plane flexural ("ovalling") modes of a thin circular ring
//   (the cross-section of a pipe or can wall), inextensional theory:
//   omega_n = sqrt(E I / (rho A)) / R^2 * n (n^2 - 1) / sqrt(n^2 + 1), n =
//   2, 3, ... (Den Hartog, "Mechanical Vibrations"; Blevins, the chapter on
//   rings, in-plane flexure; Love, "Mathematical Theory of Elasticity", the
//   thin ring): 1 : 2.828 : 5.423 : 8.771. Each n is
//   a degenerate cos / sin pair, struck with weights |cos n th|, |sin n th|.
// - Shell: the idealized church-bell partial series, relative to the prime:
//   hum 0.5, prime 1, tierce 1.2 (minor third), quint 1.5, nominal 2, then
//   2.5, 3, 4, 5, 6 (Rossing, "Science of Percussion Instruments" 2000, ch.
//   on bells; Fletcher & Rossing, the chapter on bells, the partials of a Hemony-style bell,
//   rounded here to the minor-third ideal). Bowls and helmets share the series here: an
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
            let k =
                props.thickness / 12f32.sqrt() * (props.young / props.density).sqrt() / (2.0 * PI);
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

/// The small end of the size range: the lowest mode sits this fraction of
/// the top of the band (18 kHz at 48 kHz). Below it the lowest mode is nearly
/// out of the band, and past it the table is empty.
const RANGE_TOP_FRACTION: f32 = 0.9;

/// The large end of the size range: the lowest mode sits here. A mode under
/// `FLOOR_HZ` is dropped, so this keeps the detuned lowest mode above it.
const RANGE_BOTTOM_HZ: f32 = 24.0;

/// `(lo, hi)` metres between which the lowest mode falls from 90 % of the top
/// of the band (18 kHz at 48 kHz) to 24 Hz, rounded inward to two figures.
/// Every size in it has at least its lowest mode in the band (a size just
/// outside the small end has none and is an error), and the table drops every
/// mode above the top, so nothing folds back past Nyquist.
fn size_range(shape: ModalShape, props: &MaterialProps, sr: f32) -> (f32, f32) {
    let (a, p) = lowest_mode(shape, props);
    let at = |f: f32| (a / f).powf(1.0 / p);
    let round = |v: f32, up: bool| {
        let k = 10f32.powf(1.0 - v.log10().floor());
        (if up { (v * k).ceil() } else { (v * k).floor() }) / k
    };
    (
        round(at(RANGE_TOP_FRACTION * top_hz(sr)), true),
        round(at(RANGE_BOTTOM_HZ), false),
    )
}

/// `v` to two significant figures, rounded up or down, as text.
fn sig2(v: f32, up: bool) -> String {
    let e = v.log10().floor();
    let k = 10f32.powf(1.0 - e);
    let r = (if up { (v * k).ceil() } else { (v * k).floor() }) / k;
    format!("{:.*}", (1.0 - e).max(0.0) as usize, r)
}

/// The error for a size that leaves no mode in the band: where its lowest
/// mode falls, the size range, and the material thickness that would fix it
/// (the lowest mode of every shape goes as the thickness).
fn no_mode_error(
    size: f32,
    shape: ModalShape,
    material: &MaterialSpec,
    props: &MaterialProps,
    sr: f32,
) -> String {
    let (lo, hi) = size_range(shape, props, sr);
    let (a, pw) = lowest_mode(shape, props);
    let lowest = a / size.powf(pw);
    let top = top_hz(sr);
    let what = match material {
        MaterialSpec::Name(n) => format!("the {n} {}", shape.name()),
        MaterialSpec::Props(_) => format!("this material's {}", shape.name()),
    };
    let why = if lowest > top {
        // Thinner material lowers every mode in proportion.
        let t = sig2(props.thickness * RANGE_TOP_FRACTION * top / lowest, false);
        format!(
            "its lowest mode would be {} Hz, above the top of the band ({top:.0} Hz): make the size bigger, or give a thinner material ({{young, density, poisson, loss, thickness}} with thickness {t} m or less, or set the `thickness` field to {t} or less)",
            sig2(lowest, true)
        )
    } else if lowest >= FLOOR_HZ {
        format!(
            "its lowest mode is {lowest:.1} Hz but the strike leaves no mode in the band: move `strike_position`"
        )
    } else {
        let t = sig2(props.thickness * RANGE_BOTTOM_HZ / lowest, true);
        format!(
            "its lowest mode would be {} Hz, under 20 Hz: make the size smaller, or give a thicker material (thickness {t} m or more, or set the `thickness` field to {t} or more)",
            sig2(lowest, false)
        )
    };
    format!(
        "size {size}: no mode of {what} falls between 20 Hz and the top of the band: {why}; for {what} try {lo} to {hi} m (lowest mode from {:.0} kHz down to {RANGE_BOTTOM_HZ:.0} Hz)",
        RANGE_TOP_FRACTION * top / 1000.0
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
    /// `f_min` / `f_max`: only modes inside the band are kept.
    f_min: Option<f32>,
    f_max: Option<f32>,
}

impl Table<'_> {
    /// The modes, lowest first. Empty when the table has no mode in
    /// 20 Hz .. the top of the band (the caller says what to do); an error
    /// when `f_min` / `f_max` leave none of the table's modes.
    fn modes(&self, rng: &mut crate::dsp::Rng) -> Result<Vec<Mode>, String> {
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
                    let (c, s) = (
                        (n as f32 * theta).cos().abs(),
                        (n as f32 * theta).sin().abs(),
                    );
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
        if self.f_min.is_none() && self.f_max.is_none() {
            return Ok(select(all, self.count, p));
        }
        // The band comes after the table (mistuning included) and before the
        // `modes` cap, so `modes` counts kept modes and the lowest half are the
        // lowest kept ones.
        let (lo, hi) = (self.f_min.unwrap_or(0.0), self.f_max.unwrap_or(f32::MAX));
        let (table_lo, table_hi) = all
            .iter()
            .fold((f32::MAX, 0.0f32), |(a, b), c| (a.min(c.0), b.max(c.0)));
        let total = all.len();
        all.retain(|c| c.0 >= lo && c.0 <= hi);
        let given = match (self.f_min, self.f_max) {
            (Some(a), Some(b)) => format!("f_min {a} / f_max {b}"),
            (Some(a), None) => format!("f_min {a}"),
            (None, Some(b)) => format!("f_max {b}"),
            (None, None) => String::new(),
        };
        if total == 0 {
            return Ok(Vec::new());
        }
        if all.is_empty() {
            return Err(format!(
                "{given}: no mode of this {} falls in the band; its {total} modes span {table_lo:.1} to {table_hi:.1} Hz at this size, material and thickness: widen the band, or change `size` / `f0` / `thickness`",
                self.shape.name()
            ));
        }
        let kept = select(all, self.count, p);
        if kept.is_empty() {
            return Err(format!(
                "{given}: the modes inside the band are not excited at this `strike_position` (the table spans {table_lo:.1} to {table_hi:.1} Hz): widen the band or move `strike_position`"
            ));
        }
        Ok(kept)
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
    /// start (a `$param` or `=expr` works). The usable range is where the
    /// lowest mode falls between 24 Hz and 90 % of the top of the band
    /// (18 kHz at 48 kHz): a size outside it is an error that names the range
    /// for the material and shape, and the thickness that would fix it.
    /// Modes above the top of the band are dropped, never folded back.
    /// Leave it out when `f0` is given.
    #[serde(default)]
    pub size: Option<In>,
    /// Frequency of the lowest mode of the shape, Hz (20..18000, a number,
    /// `$param` or `=expr`, read once): sets `size` so that the table's lowest
    /// mode sits here, from the shape's own table and the effective material
    /// and `thickness` (the seeded mistuning moves it by up to 1.5 % for a
    /// plate, 0.3 % for the others). Giving both `f0` and `size` is an error.
    /// `f_min` does not move it: it drops modes, not the size.
    #[serde(default)]
    pub f0: Option<In>,
    /// Keep only modes at or above this frequency, Hz (default none). Applied
    /// after the mode table and before the `modes` cap, so `modes` counts kept
    /// modes. A band with no mode in it is an error that gives the table's range.
    #[serde(default)]
    pub f_min: Option<f32>,
    /// Keep only modes at or below this frequency, Hz (default none); see `f_min`.
    #[serde(default)]
    pub f_max: Option<f32>,
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
    /// Multiplier on the material's loss (default 1, range 0.01..100):
    /// 0.5 rings twice as long, 2 half as long. T60 is still capped at 20 s
    /// per mode.
    #[serde(default)]
    pub loss: Option<f32>,
    /// Thickness of the plate, bar wall, tube wall, membrane or string
    /// stand-in, metres (default: the material's; range 0.0001..0.5). Every
    /// mode frequency goes as the thickness, so 0.0015 gives a coin, 0.003 a
    /// shard, 0.004 a key without a custom material. A number, `$param` or
    /// `=expr`, read once. The size range in the error and the hint use this
    /// thickness.
    #[serde(default)]
    pub thickness: Option<In>,
    /// dB per octave on the mode gains about the lowest mode (default 0,
    /// -12..12): positive lifts the upper modes (flattens a table that is
    /// all in its first mode), negative darkens. The output is normalized
    /// after, so only the balance moves.
    #[serde(default)]
    pub tilt: Option<f32>,
    /// Level of the output in dB after the normalization (default 0, -60..60):
    /// balances a body against a click without a `gain` node, and a macro can
    /// map it. Absent or 0 leaves the samples unchanged.
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Pitch glide of a hard strike, cents (default none, 0..1200): every
    /// mode starts this far sharp at the ring's loudest and settles as it
    /// decays (the pitch follows the level, as a struck plate's tension
    /// does). Left out or 0, the samples are unchanged.
    #[serde(default)]
    pub strike_glide: Option<f32>,
    /// A loose part buzzing while the ring is loud, 0..1 (default none): the
    /// part of the ring above a gap (the ring's level, its loudest 5 ms as a
    /// sine peak, at 0; 30 dB under it at 1) is
    /// added back high-passed above 1.5 kHz, so the buzz lasts longer as
    /// `rattle` rises and stops when the ring falls under the gap. Left out
    /// or 0, the samples are unchanged.
    #[serde(default)]
    pub rattle: Option<f32>,
    /// How noisy each mode rings, 0..1 (default none): each mode's clean
    /// ring is mixed (1 - noise : noise) with a band of white noise at the
    /// mode's frequency, `noise_width` wide, that follows the ring's own
    /// envelope and decay. At 1 no line is left, only noise bands placed and
    /// decaying like the modes: wood and stone knock, metal washes, nothing
    /// chimes. Left out or 0, the samples are unchanged.
    #[serde(default)]
    pub noise: Option<f32>,
    /// How wide each mode's noise band is, as a fraction of its frequency
    /// (default 0.25, 0.05..2; only with `noise`). Wider than the gaps
    /// between partials, the bands merge into a wash.
    #[serde(default)]
    pub noise_width: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// mistuning.
    #[serde(default)]
    pub seed: u64,
}

/// The ring's level envelope relative to its peak (0..1): instant attack,
/// 30 ms release.
fn level_envelope(x: &[f32], sr: f32) -> Vec<f32> {
    let k = (-1.0 / (0.03 * sr)).exp();
    let mut e = 0.0f32;
    let env: Vec<f32> = x
        .iter()
        .map(|v| {
            let a = v.abs();
            e = if a > e { a } else { e * k + a * (1.0 - k) };
            e
        })
        .collect();
    let top = env.iter().copied().fold(0.0f32, f32::max);
    if top > 0.0 {
        env.iter().map(|v| v / top).collect()
    } else {
        env
    }
}

/// One mode, retuned per sample by `cents x level`.
fn glide_one(x: &[f32], mode: (f32, f32, f32), level: &[f32], cents: f32, sr: f32) -> Vec<f32> {
    let (f, t60, g) = mode;
    let top = max_hz(sr);
    let w0 = 2.0 * PI * f.clamp(1.0, top) / sr;
    let r = (-(1000f32.ln()) / (t60.max(1e-4) * sr)).exp();
    let b0 = g * w0.sin();
    let (mut y1, mut y2) = (0.0f32, 0.0f32);
    (0..x.len())
        .map(|i| {
            let w = (w0 * 2f32.powf(cents * level[i] / 1200.0)).min(PI * 0.98);
            let y = crate::dsp::flush(b0 * x[i] + 2.0 * r * w.cos() * y1 - r * r * y2);
            y2 = y1;
            y1 = y;
            y
        })
        .collect()
}

/// `noise_width` when left out: each noise band a quarter of its mode's
/// frequency wide.
pub(crate) const DEFAULT_NOISE_WIDTH: f32 = 0.25;

/// Adds one mode to `out` as `(1 - depth) x ring + depth x` a band of white
/// noise at its frequency (`width` x f wide, scaled to a sine's power) that
/// follows the ring's envelope (a peak follower falling at the mode's own
/// decay rate). `white` is scratch at least the ring's length. Shared by
/// `modal` `noise`, `click` `noise` and `footstep` `noise`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_noise_band(
    out: &mut [f32],
    ring: &[f32],
    (f, t60): (f32, f32),
    depth: f32,
    width: f32,
    rng: &mut crate::dsp::Rng,
    white: &mut [f32],
    sr: f32,
) {
    let n = ring.len();
    // Its amplitude: a peak follower that falls at the mode's own decay
    // rate, so it holds the envelope between the sine's peaks.
    let fall = (-(1000f32.ln()) / (t60.max(1e-4) * sr)).exp();
    // A band of white noise at the mode's frequency, `width` x f wide,
    // scaled to the power of a sine of amplitude 1 (rms 1 / sqrt 2).
    for w in white.iter_mut().take(n) {
        *w = rng.signed();
    }
    let bw = (width * f).clamp(20.0, 0.2 * sr);
    let q = (f / bw).max(0.05);
    let mut svf = Svf::default();
    let band: Vec<f32> = white[..n]
        .iter()
        .map(|w| svf.tick(*w, FilterMode::Bp, f, q, 0.0, sr))
        .collect();
    let rms = (band
        .iter()
        .map(|b| f64::from(*b) * f64::from(*b))
        .sum::<f64>()
        / n.max(1) as f64)
        .sqrt() as f32;
    let scale = if rms > 0.0 {
        std::f32::consts::FRAC_1_SQRT_2 / rms
    } else {
        0.0
    };
    let mut env = 0.0f32;
    for i in 0..n {
        env = (env * fall).max(ring[i].abs());
        out[i] += (1.0 - depth) * ring[i] + depth * env * band[i] * scale;
    }
}

/// The buzz's gain: at 2 it sat about 30 dB under a wood body (barely
/// there), at 8 about 18 dB under.
const RATTLE_GAIN: f32 = 8.0;

/// The buzz of `rattle`: the ring above the gap, high-passed at 1.5 kHz.
fn rattle_buzz(x: &[f32], rattle: f32, sr: f32) -> Vec<f32> {
    // The ring's level, not the sample peak: an impulse response's first
    // samples spike far above the ring that follows. The loudest 5 ms RMS,
    // as a sine's peak.
    let w = ((0.005 * sr) as usize).max(1);
    let mut acc = 0.0f64;
    let mut loud = 0.0f64;
    for i in 0..x.len() {
        acc += f64::from(x[i]) * f64::from(x[i]);
        if i >= w {
            acc -= f64::from(x[i - w]) * f64::from(x[i - w]);
        }
        loud = loud.max(acc.max(0.0) / w as f64);
    }
    let level = (2.0 * loud).sqrt() as f32;
    let gap = level * 10f32.powf(-30.0 * rattle / 20.0);
    let a = (-2.0 * PI * 1500.0 / sr).exp();
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    x.iter()
        .map(|v| {
            let over = (v.abs() - gap).max(0.0) * v.signum();
            // Two one-pole high-passes (12 dB/oct).
            lp1 = a * lp1 + (1.0 - a) * over;
            let h1 = over - lp1;
            lp2 = a * lp2 + (1.0 - a) * h1;
            RATTLE_GAIN * (h1 - lp2)
        })
        .collect()
}

/// The linear gain of an optional `gain_db` (-60..60): None for absent or 0,
/// so the default path multiplies by nothing and stays bit-identical.
fn level_gain(gain_db: Option<f32>) -> Result<Option<f32>, String> {
    match gain_db {
        None => Ok(None),
        Some(g) if g.is_finite() && g.abs() <= 60.0 => Ok((g != 0.0).then(|| 10f32.powf(g / 20.0))),
        Some(g) => Err(format!(
            "gain_db: {g}: give dB between -60 and 60 (0 = unchanged)"
        )),
    }
}

/// `loss` multiplier limits and the `tilt` limit (dB per octave).
const MIN_LOSS: f32 = 0.01;
const MAX_LOSS: f32 = 100.0;
const MAX_TILT: f32 = 12.0;
/// `thickness` limits, metres.
const MIN_THICKNESS: f32 = 0.0001;
const MAX_THICKNESS: f32 = 0.5;
/// `f0` limits, Hz.
const MIN_F0: f32 = 20.0;
const MAX_F0: f32 = 18_000.0;

impl Describe for Modal {
    const BLURB: &'static str = "a struck object: excite audio rings modes computed from material, size, shape (plate bar tube membrane string shell) and strike position (bigger = lower); thickness (m) overrides the material's; f0 (Hz) sets size from the lowest mode; f_min / f_max keep only modes in a band; loss scales the ring time, tilt (dB/oct) the mode balance; a unit impulse peaks near 0 dBFS";
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
        let mut props = self.material.props()?;
        let loss = self.loss.unwrap_or(1.0);
        if !(loss.is_finite() && (MIN_LOSS..=MAX_LOSS).contains(&loss)) {
            return Err(format!(
                "loss: {loss}: give a multiplier on the material's loss in {MIN_LOSS}..{MAX_LOSS} (1 = as the material is, 0.5 rings twice as long)"
            ));
        }
        props.loss *= loss;
        if let Some(t) = &self.thickness {
            let t = cx.mono("thickness", t)?.at(0);
            if !(t.is_finite() && (MIN_THICKNESS..=MAX_THICKNESS).contains(&t)) {
                return Err(format!(
                    "thickness: {t}: give metres in {MIN_THICKNESS}..{MAX_THICKNESS} (a coin 0.0015, a shard 0.003, a key 0.004; leave it out for the material's own)"
                ));
            }
            props.thickness = t;
        }
        let gain = level_gain(self.gain_db)?;
        let tilt_db = self.tilt.unwrap_or(0.0);
        if !(tilt_db.is_finite() && (-MAX_TILT..=MAX_TILT).contains(&tilt_db)) {
            return Err(format!(
                "tilt: {tilt_db}: give dB per octave in {}..{MAX_TILT} (0 = as the table is)",
                -MAX_TILT
            ));
        }
        let size = match (&self.size, &self.f0) {
            (Some(_), Some(_)) => {
                return Err("size and f0 are both given: `f0` sets the size so the lowest mode sits at that frequency, so give one of them".into());
            }
            (None, None) => {
                return Err("give `size` (metres) or `f0` (Hz of the lowest mode)".into());
            }
            (Some(s), None) => positive("size", cx.mono("size", s)?.at(0))?,
            (None, Some(f)) => {
                let f0 = cx.mono("f0", f)?.at(0);
                if !(f0.is_finite() && (MIN_F0..=MAX_F0).contains(&f0)) {
                    return Err(format!(
                        "f0: {f0}: give the lowest mode in Hz, {MIN_F0}..{MAX_F0}"
                    ));
                }
                // The lowest mode sits at a / size^p.
                let (a, pw) = lowest_mode(self.shape, &props);
                (a / f0).powf(1.0 / pw)
            }
        };
        for (name, v) in [("f_min", self.f_min), ("f_max", self.f_max)] {
            if let Some(v) = v
                && !(v.is_finite() && v > 0.0)
            {
                return Err(format!("{name}: {v}: give a frequency in Hz above 0"));
            }
        }
        if let (Some(a), Some(b)) = (self.f_min, self.f_max)
            && a >= b
        {
            return Err(format!(
                "f_min {a} is not below f_max {b}: give a band, f_min < f_max"
            ));
        }
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
            f_min: self.f_min,
            f_max: self.f_max,
        };
        let mut modes = table.modes(&mut rng)?;
        if modes.is_empty() {
            return Err(no_mode_error(
                size,
                self.shape,
                &self.material,
                &props,
                cx.sr,
            ));
        }
        let unit = |field: &str, v: Option<f32>| -> Result<Option<f32>, String> {
            match v {
                None => Ok(None),
                Some(x) if x.is_finite() && (0.0..=1.0).contains(&x) => Ok((x > 0.0).then_some(x)),
                Some(x) => Err(format!("{field}: {x}: give 0..1")),
            }
        };
        let noise_depth = unit("noise", self.noise)?;
        let noise_width = match self.noise_width {
            None => None,
            Some(w) if w.is_finite() && (0.05..=2.0).contains(&w) => Some(w),
            Some(w) => {
                return Err(format!(
                    "noise_width: {w}: give a fraction of each mode's frequency, 0.05..2"
                ));
            }
        };
        if tilt_db != 0.0 {
            // dB per octave about the lowest kept mode; the normalization
            // below sets the level, so only the balance changes.
            let f0 = modes[0].0;
            for m in &mut modes {
                m.2 *= 10f32.powf(tilt_db * (m.0 / f0).log2() / 20.0);
            }
        }
        // A unit impulse peaks at 0 dBFS (the raw sum of many modes was +22).
        let peak = impulse_peak(&modes, cx.sr);
        if peak > 1e-12 {
            let scale = 1.0 / peak;
            for m in &mut modes {
                m.2 *= scale;
            }
        }
        if let Some(g) = gain {
            for m in &mut modes {
                m.2 *= g;
            }
        }
        let glide = match self.strike_glide {
            None => None,
            Some(c) if c.is_finite() && (0.0..=1200.0).contains(&c) => (c > 0.0).then_some(c),
            Some(c) => {
                return Err(format!(
                    "strike_glide: {c}: give cents 0..1200 (100 = a semitone)"
                ));
            }
        };
        let rattle = match self.rattle {
            None => None,
            Some(r) if r.is_finite() && (0.0..=1.0).contains(&r) => (r > 0.0).then_some(r),
            Some(r) => return Err(format!("rattle: {r}: give 0..1")),
        };
        let noise_seed = crate::mix_seed(cx.seed, self.seed ^ 0x0004_015e_u64);
        Ok(one(per_channel(&input, cx.n, |x| {
            let clean = |m: (f32, f32, f32)| -> Vec<f32> {
                let mut r = Resonator::new(m.0, m.1, m.2, cx.sr);
                x.iter().map(|v| r.tick(*v)).collect()
            };
            // The glide follows the level of the plain ring (all modes).
            let level = glide.map(|_| {
                let mut sum = vec![0.0f32; x.len()];
                for &m in &modes {
                    for (o, y) in sum.iter_mut().zip(clean(m)) {
                        *o += y;
                    }
                }
                level_envelope(&sum, cx.sr)
            });
            // One mode's ring: retuned by the glide when there is one.
            let ring_of = |m: (f32, f32, f32)| -> Vec<f32> {
                match (glide, &level) {
                    (Some(c), Some(l)) => glide_one(x, m, l, c, cx.sr),
                    _ => clean(m),
                }
            };
            let mut out = vec![0.0f32; x.len()];
            match noise_depth {
                None => {
                    for &m in &modes {
                        for (o, y) in out.iter_mut().zip(ring_of(m)) {
                            *o += y;
                        }
                    }
                }
                Some(depth) => {
                    let mut rng = crate::dsp::Rng::new(noise_seed);
                    let width = noise_width.unwrap_or(DEFAULT_NOISE_WIDTH);
                    let mut white = vec![0.0f32; x.len()];
                    for &(f, t60, g) in &modes {
                        let ring = ring_of((f, t60, g));
                        add_noise_band(
                            &mut out,
                            &ring,
                            (f, t60),
                            depth,
                            width,
                            &mut rng,
                            &mut white,
                            cx.sr,
                        );
                    }
                }
            }
            if let Some(r) = rattle {
                let buzz = rattle_buzz(&out, r, cx.sr);
                for (o, b) in out.iter_mut().zip(buzz) {
                    *o += b;
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
    /// Level of the pulse in dB (default 0, -60..60): balances a body against
    /// a click without a `gain` node, and a macro can map it. Absent or 0
    /// leaves the samples unchanged.
    #[serde(default)]
    pub gain_db: Option<f32>,
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
        let level = level_gain(self.gain_db)?;
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
        let mut norm = PI / 2.0 * area / len;
        if let Some(g) = level {
            norm *= g;
        }
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
            f_min: None,
            f_max: None,
        }
        .modes(&mut rng)
        .unwrap()
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
        println!("membrane j01 {:.4}, ratios {ratios:.4?}", z[0][0]);
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
                    let f = m
                        .first()
                        .unwrap_or_else(|| panic!("{name} {shape:?} {size}"))
                        .0;
                    let want = a / size.powf(pw);
                    assert!(
                        f < last,
                        "{name} {shape:?} size {size}: {f} Hz after {last}"
                    );
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
                // At the top of the range the lowest mode is RANGE_BOTTOM_HZ
                // (24 Hz; the mistuning moves it by under 2 %) or more.
                assert!(last >= 23.0, "{name} {shape:?}: {last}");
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
