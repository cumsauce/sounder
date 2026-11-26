//! Big bodies as statistics: `structure` (a struck or rubbed panel or shell
//! too large to hear as partials) and `rumble` (the pitchless low roll that
//! follows a heavy event). Reference, physics and measurements:
//! `docs/nodes/structure.md`; tests in
//! `crates/sfx_analysis/tests/it/structure.rs`.
//!
//! A panel of an airliner has thousands of modes under 5 kHz, so the ear
//! hears shaped noise whose decay falls with frequency ("structural
//! reverberation"), not a set of partials: `modal` (96 modes at most) reads
//! as wood when damped and as a gong when not. `structure` builds the body's
//! impulse response as noise with a per-frequency decay (three STFT layers,
//! fine in frequency at the bottom, fine in time at the top), keeps the
//! lowest modes as discrete resonances only where they are sparser than one
//! per critical band, and convolves the excitation with it.

use std::f32::consts::PI;

use foley_synth::MaterialProps;
use realfft::RealFftPlanner;
use serde::Deserialize;

use super::{MaterialSpec, fft_convolve, level_gain, mono, non_negative, one, per_channel};
use crate::dsp::{Biquad, FilterMode, Rng, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound in air, m/s, and the density of air, kg/m3.
const C_AIR: f32 = 343.0;
const RHO_AIR: f32 = 1.21;

/// `size` limits, metres.
const MIN_SIZE: f32 = 0.05;
const MAX_SIZE: f32 = 200.0;
/// `thickness` limits, metres.
const MIN_THICKNESS: f32 = 0.0002;
const MAX_THICKNESS: f32 = 0.5;
/// `loss` multiplier limits.
const MIN_LOSS: f32 = 0.01;
const MAX_LOSS: f32 = 100.0;
/// `tilt` limit, dB per octave.
const MAX_TILT: f32 = 12.0;
/// Most discrete low modes (`low_modes`).
const MAX_LOW_MODES: usize = 96;
const DEFAULT_LOW_MODES: usize = 32;
/// Default edge (joint) absorption: a riveted or bolted seam takes about a
/// tenth of a bending wave's energy as it passes (the SEA texts give
/// 0.05..0.3 for built-up structures).
const DEFAULT_EDGE_LOSS: f32 = 0.1;

/// T60 bounds, seconds: no band rings longer than 20 s (as `modal`) or
/// shorter than 3 ms.
const T60_MAX: f32 = 20.0;
const T60_MIN: f32 = 0.003;

/// Nothing under this is built: a mode under 20 Hz is not heard.
const FLOOR_HZ: f32 = 20.0;

/// The STFT layers of the statistical part: FFT length and the crossover to
/// the next layer up (Hz; one octave wide, power complementary). The bottom
/// layer resolves 6 Hz bins at 48 kHz, the top one 5 ms of time.
const LAYERS: [usize; 3] = [8192, 1024, 256];
const CROSSOVERS: [f32; 2] = [350.0, 3000.0];

/// Where the statistical part's level is checked against the excitation:
/// below -100 dB of a layer's start its frames stop.
const LAYER_END: f32 = 1e-5;

/// Zwicker's critical bandwidth, Hz (Zwicker & Terhardt, JASA 68, 1980).
fn critical_band(f: f32) -> f32 {
    let k = f / 1000.0;
    25.0 + 75.0 * (1.0 + 1.4 * k * k).powf(0.69)
}

/// What is struck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StructureShape {
    /// A flat panel `size` x `size` (a wing skin, a wall, a deck).
    #[default]
    Plate,
    /// A closed thin cylinder of diameter `size` and `length` (a fuselage,
    /// a tank, a silo, a pipe). Alias `tube`, `cylinder`.
    #[serde(alias = "tube", alias = "cylinder")]
    Shell,
}

/// A statistically dense struck or rubbed body.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Structure {
    /// The force: an impulse, a `contact` pulse, an `impact` or `rig`
    /// output for a hit; a `friction` stream or noise for a scrape or a
    /// tear. Convolved with the body's response, so anything works. Alias
    /// `input`.
    #[serde(alias = "input")]
    pub excite: In,
    /// stone wood metal glass ice earth granular plant soft snow, or
    /// {young, density, poisson, loss, thickness} (as `modal`).
    pub material: MaterialSpec,
    /// Side of the plate or diameter of the shell, metres (0.05..200; a
    /// number, `$param` or `=expr`, read once). Bigger reaches lower
    /// (panels radiate poorly below the frequency whose wavelength is twice
    /// their size), rings longer (the edges, where energy leaves, are
    /// further apart) and a shell's ring frequency falls. Alias `diameter`.
    #[serde(alias = "diameter")]
    pub size: In,
    /// Shell only: its length, metres (default 2 x `size`, 0.05..2000).
    #[serde(default)]
    pub length: Option<f32>,
    /// plate (default) or shell (alias tube, cylinder).
    #[serde(default)]
    pub shape: StructureShape,
    /// Skin or wall thickness, metres (default: the material's; 0.0002..0.5,
    /// read once). An airliner skin is 0.002, a ship's hull 0.015, a
    /// bridge girder web 0.03.
    #[serde(default)]
    pub thickness: Option<In>,
    /// Multiplier on the material's loss factor (default 1, 0.01..100): 2
    /// halves every decay, 0.5 doubles it.
    #[serde(default)]
    pub loss: Option<f32>,
    /// Absorption of a bending wave at an edge or joint, 0..1 (default
    /// 0.1): rivets, bolts and supports. It shortens small bodies far more
    /// than big ones (its loss goes as perimeter / area). 0 leaves only the
    /// material and radiation.
    #[serde(default)]
    pub edge_loss: Option<f32>,
    /// Spacing of the joints, frames or stiffeners that bound one bay,
    /// metres (default: the whole body, `size`; 0.05..200). A built-up
    /// structure loses its energy at its joints, so an airliner skin with
    /// frames every 0.5 m and stringers every 0.2 m decays as a bay of about
    /// that size, not as a 40 m panel: the edge loss uses perimeter / area
    /// = 4 / panel. With little material `loss` this makes the decay fall
    /// as 1 / sqrt(f) instead of 1 / f.
    #[serde(default)]
    pub panel: Option<f32>,
    /// dB per octave about 1 kHz on the response (default 0, -12..12):
    /// negative for a darker, heavier body. The normalization keeps the
    /// peak, so only the balance moves.
    #[serde(default)]
    pub tilt: Option<f32>,
    /// Most discrete low modes (default 32, 0..96). Modes are kept only
    /// below the frequency where the body has one mode per critical band;
    /// above it the response is statistical. 0 makes all of it statistical.
    #[serde(default)]
    pub low_modes: Option<usize>,
    /// Level in dB after the normalization (default 0, -60..60).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// noise of the response and the low modes' tuning and strike.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Structure {
    const BLURB: &'static str = "a body too big to hear as partials (an airliner panel or fuselage, a hull, a girder): the excite audio convolved with band-wise noise whose decay follows T60(f) = 2.2 / (eta(f) f) from material, size, thickness, shape (plate shell), loss, edge_loss and panel (joint spacing), plus the few lowest modes where they are sparse; works for one hit (impulse, contact, impact) or a scrape / tear stream; a unit impulse peaks at 0 dBFS";
    fn example() -> serde_json::Value {
        serde_json::json!({"excite": "contact1", "material": "metal", "size": 6.0,
            "thickness": 0.003, "shape": "plate"})
    }
}

/// The body in physical terms, everything the response is built from.
struct Body {
    shape: StructureShape,
    /// Plate side or shell diameter, m.
    size: f32,
    /// Shell length, m (plate: unused).
    length: f32,
    h: f32,
    /// Longitudinal (plate) wave speed, m/s.
    c_l: f32,
    /// Surface density, kg/m2.
    mass: f32,
    poisson: f32,
    area: f32,
    perimeter: f32,
    /// Perimeter / area of one bay, 1/m: what the edge loss sees.
    bay: f32,
    eta_m: f32,
    edge: f32,
    tilt_db: f32,
    /// The lowest mode of the body, Hz.
    lowest: f32,
}

impl Body {
    fn new(
        props: &MaterialProps,
        shape: StructureShape,
        size: f32,
        length: f32,
        panel: Option<f32>,
        edge: f32,
        tilt_db: f32,
    ) -> Body {
        let h = props.thickness;
        let c_l = (props.young / (props.density * (1.0 - props.poisson * props.poisson))).sqrt();
        let (area, perimeter) = match shape {
            StructureShape::Plate => (size * size, 4.0 * size),
            StructureShape::Shell => (PI * size * length, 2.0 * PI * size),
        };
        let mut b = Body {
            shape,
            size,
            length,
            h,
            c_l,
            mass: props.density * h,
            poisson: props.poisson,
            area,
            perimeter,
            bay: panel.map_or(perimeter / area, |p| 4.0 / p),
            eta_m: props.loss,
            edge,
            tilt_db,
            lowest: 0.0,
        };
        b.lowest = b.lowest_mode();
        b
    }

    /// The plate's modal density, modes per Hz: sqrt(3) S / (h c_L), the
    /// same at every frequency (Cremer, Heckl & Petersson, "Structure-Borne
    /// Sound", the chapter on plates).
    fn plate_density(&self) -> f32 {
        3f32.sqrt() * self.area / (self.h * self.c_l)
    }

    /// The shell's ring frequency, Hz: a longitudinal wave once round.
    fn ring_hz(&self) -> f32 {
        self.c_l / (PI * self.size)
    }

    /// Modes per Hz at `f`. A shell has the plate's density above its ring
    /// frequency and far fewer below (the wall is stiff in its own plane):
    /// Szechenyi's approximation (J. Sound Vib. 19, 1971), as cited from
    /// memory and made continuous: 1.75 sqrt(nu) under nu = f / f_ring =
    /// 0.48, 1.21 to 0.83, then falling back to 1.
    fn density(&self, f: f32) -> f32 {
        let np = self.plate_density();
        match self.shape {
            StructureShape::Plate => np,
            StructureShape::Shell => {
                let nu = (f / self.ring_hz()).max(1e-6);
                let r = if nu < 0.48 {
                    1.75 * nu.sqrt()
                } else if nu < 0.83 {
                    1.75 * 0.48f32.sqrt()
                } else {
                    1.0 + (1.75 * 0.48f32.sqrt() - 1.0) * (0.83 / nu).powf(1.5)
                };
                np * r
            }
        }
    }

    /// The coincidence frequency, Hz: bending waves as fast as sound in air.
    fn coincidence(&self) -> f32 {
        C_AIR * C_AIR / (1.8 * self.h * self.c_l)
    }

    /// Under this the panel is smaller than half a wavelength in air and
    /// its sides cancel: c / (2 size).
    fn radiation_floor(&self) -> f32 {
        C_AIR / (2.0 * self.size)
    }

    /// Radiation efficiency (Maidanik's edge term below coincidence, made
    /// to fall as (f / f_floor)^2 under the size floor; 1 / sqrt(1 - fc/f)
    /// above, capped at 3).
    fn sigma(&self, f: f32) -> f32 {
        let fc = self.coincidence();
        let fl = self.radiation_floor();
        let small = (f / fl).powi(2) / (1.0 + (f / fl).powi(2));
        if f < fc {
            let edge = self.perimeter * C_AIR / (PI * PI * self.area * fc) * (f / fc).sqrt();
            (edge * small).min(1.0)
        } else {
            (1.0 / (1.0 - fc / f).max(1.0 / 9.0)).sqrt() * small
        }
    }

    /// The loss factor at `f`: the material's, radiation rho0 c0 sigma /
    /// (omega m''), and the edges c_g P alpha / (pi omega S), c_g the
    /// bending group speed (Lyon & DeJong, "Theory and Application of
    /// Statistical Energy Analysis", the chapter on damping).
    fn eta(&self, f: f32) -> f32 {
        let w = 2.0 * PI * f.max(1.0);
        let rad = RHO_AIR * C_AIR * self.sigma(f) / (w * self.mass);
        let kappa = self.h / 12f32.sqrt();
        let cg = 2.0 * (w * kappa * self.c_l).sqrt();
        let edge = cg * self.bay * self.edge / (PI * w);
        self.eta_m + rad + edge
    }

    /// T60 at `f`, seconds: 2.2 / (eta f).
    fn t60(&self, f: f32) -> f32 {
        (2.2 / (self.eta(f) * f.max(1.0))).clamp(T60_MIN, T60_MAX)
    }

    /// The response's power per Hz at the start, relative: the energy an
    /// impulse puts into a band goes as the modal density (the real part of
    /// the drive-point mobility), the sound it makes as the radiation
    /// efficiency's slope (sqrt(f / fc) under coincidence and the size
    /// floor; the edge-radiation constant is left out, or the band above
    /// coincidence would sit 30 dB over the rest and no real heavy hit's
    /// force reaches it), then `tilt`, and nothing below the lowest mode.
    fn power(&self, f: f32) -> f32 {
        let fc = self.coincidence();
        let fl = self.radiation_floor();
        let small = (f / fl).powi(2) / (1.0 + (f / fl).powi(2));
        let slope = (f.min(fc) / fc).sqrt();
        let lo = (f / self.lowest).powi(4) / (1.0 + (f / self.lowest).powi(4));
        let tilt = 10f32.powf(self.tilt_db * (f / 1000.0).log2() / 10.0);
        self.density(f) / self.plate_density() * small * slope * lo * tilt
    }

    /// Lowest mode, Hz.
    fn lowest_mode(&self) -> f32 {
        match self.shape {
            StructureShape::Plate => {
                let (a, b) = self.plate_sides();
                plate_k(self) * (1.0 / (a * a) + 1.0 / (b * b))
            }
            StructureShape::Shell => {
                let mut low = f32::INFINITY;
                for m in 1..=8 {
                    for n in 0..=80 {
                        low = low.min(self.shell_mode(m, n));
                    }
                }
                low
            }
        }
    }

    /// The plate as a rectangle of area size^2 and sides 1.15 : 1 / 1.15, so
    /// the (m, n) / (n, m) pairs of a square do not coincide.
    fn plate_sides(&self) -> (f32, f32) {
        (self.size * 1.15, self.size / 1.15)
    }

    /// Shell mode (m axial half waves, n circumferential waves), Hz, of a
    /// simply supported thin cylinder (Donnell's simplified frequency
    /// equation, Leissa, "Vibration of Shells", 1973): Omega^2 = (1 - nu^2)
    /// l^4 / (l^2 + n^2)^2 + beta^2 (l^2 + n^2)^2, l = m pi a / L, beta^2 =
    /// h^2 / (12 a^2), f = Omega f_ring.
    fn shell_mode(&self, m: u32, n: u32) -> f32 {
        let a = 0.5 * self.size;
        let l = m as f32 * PI * a / self.length;
        let s = l * l + (n * n) as f32;
        let beta2 = self.h * self.h / (12.0 * a * a);
        let om2 = (1.0 - self.poisson * self.poisson) * l.powi(4) / (s * s) + beta2 * s * s;
        om2.sqrt() * self.ring_hz()
    }

    /// The modes under `below` Hz, lowest first, at most `cap` (enumerated
    /// up to 8 x `cap` candidates and cut), each (Hz, strike weight).
    fn modes(&self, below: f32, cap: usize, rng: &mut Rng) -> Vec<f32> {
        let mut all: Vec<f32> = Vec::new();
        let limit = 8 * cap.max(1);
        match self.shape {
            StructureShape::Plate => {
                let (a, b) = self.plate_sides();
                let k = plate_k(self);
                'm: for m in 1..=400u32 {
                    let first = k * ((m * m) as f32 / (a * a) + 1.0 / (b * b));
                    if first > below {
                        break;
                    }
                    for n in 1..=400u32 {
                        let f = k * ((m * m) as f32 / (a * a) + (n * n) as f32 / (b * b));
                        if f > below {
                            break;
                        }
                        all.push(f);
                        if all.len() >= limit {
                            break 'm;
                        }
                    }
                }
            }
            StructureShape::Shell => {
                'outer: for m in 1..=200u32 {
                    let mut any = false;
                    for n in 0..=400u32 {
                        let f = self.shell_mode(m, n);
                        if f <= below {
                            any = true;
                            // n >= 1 is a cos / sin pair.
                            let copies = if n == 0 { 1 } else { 2 };
                            for _ in 0..copies {
                                all.push(f);
                            }
                            if all.len() >= limit {
                                break 'outer;
                            }
                        } else if n > 4 && f > 4.0 * below {
                            break;
                        }
                    }
                    if !any && self.shell_mode(m, 0) > below {
                        break;
                    }
                }
            }
        }
        // Real structures are not ideal: a little seeded mistuning, which
        // also splits the shell's degenerate pairs.
        for f in &mut all {
            *f *= 1.0 + 0.01 * rng.signed();
        }
        all.retain(|f| *f >= FLOOR_HZ && *f < below);
        all.sort_by(f32::total_cmp);
        all.truncate(cap);
        all
    }
}

/// pi / (2 sqrt 12) h c_L: the simply supported plate's f(m, n) is this
/// times (m/a)^2 + (n/b)^2.
fn plate_k(b: &Body) -> f32 {
    PI / (2.0 * 12f32.sqrt()) * b.h * b.c_l
}

/// The frequency where the body first has one mode per critical band (the
/// lowest at which it is heard as noise, not partials): 0 when it is
/// already that dense at 20 Hz, `top` when never.
fn statistical_from(b: &Body, top: f32) -> f32 {
    let steps = 200;
    for i in 0..=steps {
        let f = FLOOR_HZ * (top / FLOOR_HZ).powf(i as f32 / steps as f32);
        if b.density(f) * critical_band(f) >= 1.0 {
            return if i == 0 { 0.0 } else { f };
        }
    }
    top
}

/// Power-complementary crossover weight, rising from 0 an octave-wide
/// half below `fc` to 1 half an octave above.
fn rise(f: f32, fc: f32) -> f32 {
    if fc <= 0.0 {
        return 1.0;
    }
    let s = ((f / fc).log2() + 0.5).clamp(0.0, 1.0);
    (0.5 * PI * s).sin()
}

/// One standard normal pair (Box-Muller).
fn gauss2(rng: &mut Rng) -> (f32, f32) {
    let u = rng.unit().max(1e-12);
    let v = rng.unit();
    let r = (-2.0 * u.ln()).sqrt();
    (r * (2.0 * PI * v).cos(), r * (2.0 * PI * v).sin())
}

/// Adds one STFT layer of shaped, decaying noise to `ir`: bin k of frame j
/// is complex Gaussian of power `power(f) x weight(f)^2`, falling at the
/// band's T60 from frame to frame. Frames start before t = 0 so the
/// response is at full level from its first sample (what lands before 0 is
/// dropped). A decay faster than the window is clamped to what it can
/// show; each bin's level is corrected for the window's spread of the
/// decay, so the expected power at t = 0 is `power x weight^2` per Hz.
fn add_layer(
    ir: &mut [f32],
    n_fft: usize,
    weight: &dyn Fn(f32) -> f32,
    body: &Body,
    sr: f32,
    rng: &mut Rng,
) -> Result<(), String> {
    let len = ir.len();
    let hop = n_fft / 4;
    let bins = n_fft / 2 + 1;
    let top = max_hz(sr).min(20_000.0);
    let window: Vec<f32> = (0..n_fft)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n_fft as f32).cos())
        .collect();
    // Per bin: start amplitude and decay per hop.
    let mut amp = vec![0.0f32; bins];
    let mut per_hop = vec![0.0f32; bins];
    let half = 0.5 * n_fft as f32 / sr;
    for k in 1..bins - 1 {
        let f = k as f32 * sr / n_fft as f32;
        if !(FLOOR_HZ * 0.9..=top).contains(&f) {
            continue;
        }
        let w = weight(f);
        let p = body.power(f);
        if w <= 0.0 || p <= 0.0 {
            continue;
        }
        // Amplitude decay rate, 1/s, at most what half a window can show.
        let a = (1000f32.ln() / body.t60(f)).min(15.0 / half);
        // Expected power at t = 0 of unit frames centred at c_j = s_j +
        // n/2, s_j = -n + j hop (j = 1..4 cover t = 0), each scaled by
        // exp(-a c_j): sum of w(-s_j)^2 exp(-2 a c_j).
        let mut sum = 0.0f32;
        for j in 1..=4usize {
            let s = j as f32 * hop as f32 - n_fft as f32;
            let idx = (-s) as usize;
            if idx < n_fft {
                let c = (s + 0.5 * n_fft as f32) / sr;
                sum += window[idx] * window[idx] * (-2.0 * a * c).exp();
            }
        }
        // Unnormalized inverse FFT: per-bin power sr / (2 n) of the per-Hz
        // power gives that power per Hz in the time domain.
        let level = (p * sr / (2.0 * n_fft as f32)).sqrt() * w;
        amp[k] = level / sum.max(1e-12).sqrt();
        per_hop[k] = (-a * hop as f32 / sr).exp();
    }
    let start = amp.iter().copied().fold(0.0f32, f32::max);
    if start <= 0.0 {
        return Ok(());
    }
    // Frame j's centre is s_j + n/2; scale the amplitudes to frame 1's.
    let first_c = (hop as f32 - 0.5 * n_fft as f32) / sr;
    for k in 0..bins {
        if amp[k] > 0.0 {
            let a = -per_hop[k].ln() * sr / hop as f32;
            amp[k] *= (-a * first_c).exp();
        }
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let inv = planner.plan_fft_inverse(n_fft);
    let mut spec = inv.make_input_vec();
    let mut frame = inv.make_output_vec();
    let mut j = 1usize;
    loop {
        let s = j as isize * hop as isize - n_fft as isize;
        if s >= len as isize {
            break;
        }
        let mut loud = 0.0f32;
        for k in 0..bins {
            if amp[k] > 0.0 {
                let (re, im) = gauss2(rng);
                spec[k] = realfft::num_complex::Complex::new(re * amp[k], im * amp[k]);
                loud = loud.max(amp[k]);
                amp[k] *= per_hop[k];
            } else {
                spec[k] = realfft::num_complex::Complex::new(0.0, 0.0);
            }
        }
        if loud < LAYER_END * start {
            break;
        }
        spec[0].im = 0.0;
        spec[bins - 1].im = 0.0;
        inv.process(&mut spec, &mut frame)
            .map_err(|e| format!("structure: fft: {e}"))?;
        for (i, (x, w)) in frame.iter().zip(&window).enumerate() {
            let t = s + i as isize;
            if t >= 0 && (t as usize) < len {
                ir[t as usize] += x * w;
            }
        }
        j += 1;
    }
    Ok(())
}

/// The body's impulse response, `len` samples, peak 1 (before `gain`).
fn response(body: &Body, low_modes: usize, len: usize, sr: f32, rng: &mut Rng) -> Vec<f32> {
    let top = max_hz(sr).min(20_000.0);
    let mut ir = vec![0.0f32; len];
    // Where the statistical part takes over from the discrete modes.
    let mut fx = if low_modes == 0 {
        0.0
    } else {
        statistical_from(body, top)
    };
    let modes = if fx > 0.0 {
        let m = body.modes(fx * 2f32.sqrt(), low_modes, rng);
        // Capped: the statistical part starts at the last kept mode.
        if m.len() == low_modes
            && let Some(last) = m.last()
        {
            fx = fx.min(last / 2f32.sqrt());
        }
        m
    } else {
        Vec::new()
    };
    let stat = |f: f32| rise(f, fx);
    let layer_w = |f: f32, l: usize| -> f32 {
        let up = if l == 0 {
            1.0
        } else {
            rise(f, CROSSOVERS[l - 1])
        };
        let down = if l + 1 < LAYERS.len() {
            let r = rise(f, CROSSOVERS[l]);
            (1.0 - r * r).max(0.0).sqrt()
        } else {
            1.0
        };
        up * down
    };
    for (l, n_fft) in LAYERS.iter().enumerate() {
        let w = |f: f32| stat(f) * layer_w(f, l);
        // An FFT error is impossible for these fixed sizes; skip the layer.
        let _ = add_layer(&mut ir, *n_fft, &w, body, sr, rng);
    }
    // The discrete modes: each carries the energy of the 1 / n(f) Hz of
    // spectrum it stands for (amplitude sqrt(2 P / n)), times a random
    // strike weight (Rayleigh, mean square 1) and the crossover.
    for f in modes {
        if f >= top {
            continue;
        }
        let wd = {
            let r = stat(f);
            (1.0 - r * r).max(0.0).sqrt()
        };
        let strike = (-(rng.unit().max(1e-9)).ln()).sqrt();
        let amp = (2.0 * body.power(f) / body.density(f)).sqrt() * wd * strike;
        if amp <= 0.0 || !amp.is_finite() {
            continue;
        }
        let a = 1000f32.ln() / body.t60(f);
        let ring = ((body.t60(f) * 1.2 * sr) as usize).min(len);
        let (dr, di) = {
            let r = (-a / sr).exp();
            let w = 2.0 * PI * f / sr;
            (r * w.cos(), r * w.sin())
        };
        let (mut re, mut im) = (1.0f32, 0.0f32);
        for x in ir.iter_mut().take(ring) {
            *x += amp * im;
            let nr = re * dr - im * di;
            im = re * di + im * dr;
            re = nr;
        }
    }
    let peak = ir.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 && peak.is_finite() {
        for x in &mut ir {
            *x /= peak;
        }
    }
    ir
}

impl Structure {
    fn body(&self, cx: &Cx) -> Result<(Body, usize, Option<f32>), String> {
        let mut props = self.material.props()?;
        let loss = self.loss.unwrap_or(1.0);
        if !(loss.is_finite() && (MIN_LOSS..=MAX_LOSS).contains(&loss)) {
            return Err(format!(
                "loss: {loss}: give a multiplier on the material's loss in {MIN_LOSS}..{MAX_LOSS} (1 = as the material is, 2 halves every decay)"
            ));
        }
        props.loss *= loss;
        if let Some(t) = &self.thickness {
            let t = cx.mono("thickness", t)?.at(0);
            if !(t.is_finite() && (MIN_THICKNESS..=MAX_THICKNESS).contains(&t)) {
                return Err(format!(
                    "thickness: {t}: give metres in {MIN_THICKNESS}..{MAX_THICKNESS} (an airliner skin 0.002, a hull 0.015, a girder web 0.03)"
                ));
            }
            props.thickness = t;
        }
        let size = cx.mono("size", &self.size)?.at(0);
        if !(size.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&size)) {
            return Err(format!(
                "size: {size}: give metres in {MIN_SIZE}..{MAX_SIZE} (plate side or shell diameter; under about 0.3 m a `modal` body is the better node)"
            ));
        }
        let length = match (self.shape, self.length) {
            (StructureShape::Plate, Some(l)) => {
                return Err(format!(
                    "length: {l}: only the \"shell\" shape takes it (a plate is size x size): drop it or set shape \"shell\""
                ));
            }
            (StructureShape::Plate, None) => size,
            (StructureShape::Shell, None) => 2.0 * size,
            (StructureShape::Shell, Some(l))
                if l.is_finite() && (MIN_SIZE..=2000.0).contains(&l) =>
            {
                l
            }
            (StructureShape::Shell, Some(l)) => {
                return Err(format!("length: {l}: give metres in {MIN_SIZE}..2000"));
            }
        };
        let edge = self.edge_loss.unwrap_or(DEFAULT_EDGE_LOSS);
        if !(edge.is_finite() && (0.0..=1.0).contains(&edge)) {
            return Err(format!(
                "edge_loss: {edge}: give the share of a bending wave's energy an edge or joint absorbs, 0..1"
            ));
        }
        let panel = match self.panel {
            None => None,
            Some(p) if p.is_finite() && (MIN_SIZE..=MAX_SIZE).contains(&p) => Some(p),
            Some(p) => {
                return Err(format!(
                    "panel: {p}: give the joint spacing in metres, {MIN_SIZE}..{MAX_SIZE}"
                ));
            }
        };
        let tilt = self.tilt.unwrap_or(0.0);
        if !(tilt.is_finite() && (-MAX_TILT..=MAX_TILT).contains(&tilt)) {
            return Err(format!(
                "tilt: {tilt}: give dB per octave in {}..{MAX_TILT}",
                -MAX_TILT
            ));
        }
        let low = self.low_modes.unwrap_or(DEFAULT_LOW_MODES);
        if low > MAX_LOW_MODES {
            return Err(format!("low_modes: {low}: give 0..{MAX_LOW_MODES}"));
        }
        let gain = level_gain(self.gain_db)?;
        Ok((
            Body::new(&props, self.shape, size, length, panel, edge, tilt),
            low,
            gain,
        ))
    }
}

impl Eval for Structure {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("excite", &self.excite)?;
        let (body, low, gain) = self.body(cx)?;
        // The response lasts as long as its longest band (the lowest),
        // and never longer than the render.
        let longest = (0..=40)
            .map(|i| {
                let f = FLOOR_HZ * 2f32.powf(i as f32 / 4.0);
                if f >= body.lowest * 0.8 {
                    body.t60(f)
                } else {
                    0.0
                }
            })
            .fold(T60_MIN, f32::max);
        let len = ((longest * cx.sr) as usize + 1).clamp(1, cx.n.max(1));
        let mut rng = cx.rng(self.seed);
        let mut ir = response(&body, low, len, cx.sr, &mut rng);
        if let Some(g) = gain {
            ir.iter_mut().for_each(|x| *x *= g);
        }
        let err = std::sync::Mutex::new(None);
        let out = per_channel(&input, cx.n, |x| match fft_convolve(x, &ir, cx.n) {
            Ok(mut y) => {
                y.iter_mut().for_each(|v| {
                    if !v.is_finite() {
                        *v = 0.0;
                    }
                });
                y
            }
            Err(e) => {
                if let Ok(mut slot) = err.lock() {
                    *slot = Some(e);
                }
                vec![0.0; cx.n]
            }
        });
        if let Some(e) = err.into_inner().ok().flatten() {
            return Err(e);
        }
        Ok(one(out))
    }
}

// ------------------------------------------------------------------ rumble

/// `rumble` limits.
const MIN_TOP: f32 = 20.0;
const MAX_TOP: f32 = 1000.0;
const MAX_FLUTTER_RATE: f32 = 60.0;
/// The flutter's swing at `flutter_depth` 1, dB either way.
const FLUTTER_DB: f32 = 9.0;
/// RMS of the band noise before the envelope: peaks land near 1.
const RUMBLE_RMS: f32 = 0.3;

/// The low roll after a heavy event: pitchless noise under `top`.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rumble {
    /// Highest frequency, Hz (default 120, 20..1000): a 4th-order low-pass
    /// on noise that falls 6 dB per octave above `bottom`, so nearly all
    /// the energy is under it (under 1 % above 2 x top in the tests).
    #[serde(default)]
    pub top: Option<f32>,
    /// Lowest frequency, Hz (default 25, 10..top / 1.5): a 2nd-order
    /// high-pass.
    #[serde(default)]
    pub bottom: Option<f32>,
    /// How fast the level wanders, Hz (default 12, 0..60): a smooth random
    /// gain (new targets at random times, about this many a second). 0 is
    /// a steady roll. Alias `rate`.
    #[serde(default, alias = "rate")]
    pub flutter_rate: Option<f32>,
    /// How far it wanders, 0..1 (default 0.5): at 1 the gain swings up to
    /// 9 dB either way. Alias `depth`.
    #[serde(default, alias = "depth")]
    pub flutter_depth: Option<f32>,
    /// Rise time, seconds (default 0.04, 0..5): a raised cosine from `at`,
    /// or the follower's attack with `follow`.
    #[serde(default)]
    pub onset: Option<f32>,
    /// Time to fall 60 dB after the onset, seconds (default 3, 0.05..30),
    /// or the follower's release with `follow`. Scaled by energy^0.25.
    #[serde(default)]
    pub decay: Option<f32>,
    /// How heavy the event was, relative (default 1, 0.01..100): the level
    /// goes as sqrt(energy) and the decay as energy^0.25 (4 is 6 dB louder
    /// and 41 % longer).
    #[serde(default)]
    pub energy: Option<f32>,
    /// Audio whose level the rumble follows (a peak follower with `onset`
    /// attack and `decay` release) instead of one hit at `at`: the rumble
    /// of a whole crash, or under a scrape. An input peak of 1 gives the
    /// level of one hit.
    #[serde(default)]
    pub follow: Option<In>,
    /// When the one hit starts, seconds (default 0; not with `follow`).
    #[serde(default)]
    pub at: Option<f32>,
    /// Level in dB (default 0, -60..60).
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Rumble {
    const BLURB: &'static str = "the low structural / ground rumble after a heavy event: pitchless noise under top (default 120 Hz) with a slow random flutter (flutter_rate, flutter_depth), onset and decay, scaled by energy; follow makes it track another signal's level";
    fn example() -> serde_json::Value {
        serde_json::json!({"top": 120, "flutter_rate": 12, "flutter_depth": 0.5,
            "onset": 0.04, "decay": 3.0, "energy": 1.0})
    }
}

/// A smooth random gain in dB-free form (-1..1): new targets at jittered
/// times about `rate` a second, joined by raised cosines.
fn flutter(n: usize, rate: f32, sr: f32, rng: &mut Rng) -> Vec<f32> {
    if rate <= 0.0 {
        return vec![0.0; n];
    }
    let mut out = Vec::with_capacity(n);
    let mut from = rng.signed();
    let mut to = rng.signed();
    let gap = |rng: &mut Rng| ((0.5 + rng.unit()) * sr / rate).max(1.0);
    let mut len = gap(rng);
    let mut pos = 0.0f32;
    for _ in 0..n {
        let s = (pos / len).min(1.0);
        out.push(from + (to - from) * (0.5 - 0.5 * (PI * s).cos()));
        pos += 1.0;
        if pos >= len {
            pos = 0.0;
            from = to;
            to = rng.signed();
            len = gap(rng);
        }
    }
    out
}

impl Eval for Rumble {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let sr = cx.sr;
        let top = self.top.unwrap_or(120.0);
        if !(top.is_finite() && (MIN_TOP..=MAX_TOP).contains(&top) && top < max_hz(sr)) {
            return Err(format!("top: {top}: give Hz in {MIN_TOP}..{MAX_TOP}"));
        }
        let bottom = self.bottom.unwrap_or(25.0f32.min(top / 1.5));
        if !(bottom.is_finite() && bottom >= 10.0 && bottom <= top / 1.5) {
            return Err(format!(
                "bottom: {bottom}: give Hz from 10 to top / 1.5 ({:.1})",
                top / 1.5
            ));
        }
        let rate = self.flutter_rate.unwrap_or(12.0);
        if !(rate.is_finite() && (0.0..=MAX_FLUTTER_RATE).contains(&rate)) {
            return Err(format!(
                "flutter_rate: {rate}: give Hz in 0..{MAX_FLUTTER_RATE}"
            ));
        }
        let depth = self.flutter_depth.unwrap_or(0.5);
        if !(depth.is_finite() && (0.0..=1.0).contains(&depth)) {
            return Err(format!("flutter_depth: {depth}: give 0..1"));
        }
        let onset = self.onset.unwrap_or(0.04);
        if !(onset.is_finite() && (0.0..=5.0).contains(&onset)) {
            return Err(format!("onset: {onset}: give seconds in 0..5"));
        }
        let decay = self.decay.unwrap_or(3.0);
        if !(decay.is_finite() && (0.05..=30.0).contains(&decay)) {
            return Err(format!(
                "decay: {decay}: give seconds to fall 60 dB, 0.05..30"
            ));
        }
        let energy = self.energy.unwrap_or(1.0);
        if !(energy.is_finite() && (0.01..=100.0).contains(&energy)) {
            return Err(format!("energy: {energy}: give 0.01..100 (1 = one hit)"));
        }
        if self.follow.is_some() && self.at.is_some() {
            return Err("at and follow are both given: `follow` makes the rumble track a signal, `at` places one hit; give one".into());
        }
        let at = non_negative("at", self.at.unwrap_or(0.0))?;
        let gain = level_gain(self.gain_db)?.unwrap_or(1.0);
        let decay = decay * energy.powf(0.25);
        let level = energy.sqrt() * gain;
        let mut rng = cx.rng(self.seed);
        let n = cx.n;

        // The band: white noise, -6 dB / octave above `bottom` (a one-pole
        // low-pass there), a 2nd-order high-pass at `bottom`, a 4th-order
        // Butterworth low-pass at `top`.
        let mut hp = Biquad::new();
        hp.set(
            FilterMode::Hp,
            bottom,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            sr,
        );
        let mut lp1 = Biquad::new();
        lp1.set(FilterMode::Lp, top, 0.5412, 0.0, sr);
        let mut lp2 = Biquad::new();
        lp2.set(FilterMode::Lp, top, 1.3066, 0.0, sr);
        let k = (-2.0 * PI * bottom / sr).exp();
        let mut tilt = 0.0f32;
        // A second of settling so the filters start in their steady state.
        let warm = sr as usize;
        let mut band: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n + warm {
            tilt = k * tilt + (1.0 - k) * rng.signed();
            let y = lp2.tick(lp1.tick(hp.tick(tilt)));
            if i >= warm {
                band.push(y);
            }
        }
        let rms = (band.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / n.max(1) as f64).sqrt()
            as f32;
        let scale = if rms > 0.0 { RUMBLE_RMS / rms } else { 0.0 };
        let wobble = flutter(n, rate, sr, &mut rng);

        let env: Vec<f32> = match &self.follow {
            Some(f) => {
                let x = cx.audio("follow", f)?;
                let chans = x.channels(n);
                let att = if onset > 0.0 {
                    (-1.0 / (onset / 3.0 * sr)).exp()
                } else {
                    0.0
                };
                let rel = (-(1000f32.ln()) / (decay * sr)).exp();
                let mut e = 0.0f32;
                (0..n)
                    .map(|i| {
                        let a = chans
                            .iter()
                            .map(|c| c.get(i).copied().unwrap_or(0.0).abs())
                            .fold(0.0f32, f32::max);
                        let a = if a.is_finite() { a } else { 0.0 };
                        e = if a > e {
                            att * e + (1.0 - att) * a
                        } else {
                            rel * e
                        };
                        e
                    })
                    .collect()
            }
            None => {
                let start = cx.at(at);
                let rise_n = onset * sr;
                let fall = 1000f32.ln() / (decay * sr);
                (0..n)
                    .map(|i| {
                        if i < start {
                            return 0.0;
                        }
                        let t = (i - start) as f32;
                        if t < rise_n {
                            0.5 - 0.5 * (PI * t / rise_n).cos()
                        } else {
                            (-(t - rise_n) * fall).exp()
                        }
                    })
                    .collect()
            }
        };
        let out = mono(n, |i| {
            let g = 10f32.powf(FLUTTER_DB * depth * wobble[i] / 20.0);
            let y = band[i] * scale * env[i] * g * level;
            if y.is_finite() { y } else { 0.0 }
        });
        Ok(one(out))
    }
}
