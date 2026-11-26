//! The nonlinear plate: `plate`. A struck thin plate whose modes are coupled
//! by the stretching of its middle surface, so a hard strike is not only
//! louder: the mode frequencies rise with the energy (and glide down as it
//! decays), and energy moves from the struck low modes into many higher ones
//! within tens of milliseconds (the crash of a cymbal, gong or thunder sheet)
//! before the higher modes' larger loss darkens it again.
//!
//! The mode table, the material and every shared field come from `modal`
//! (`Modal::prepare`), shape plate. The nonlinearity is a reduced form of the
//! modal von Karman plate (Ducceschi and Touze, "Simulations of nonlinear
//! plate dynamics: an accurate and efficient modal algorithm"; Bilbao,
//! "Numerical Sound Synthesis", the chapter on nonlinear plates): there the
//! nonlinear energy is a sum over in-plane stress modes of the square of a
//! quadratic form in the transverse modes, sum_n (q^T H_n q)^2. Here
//!
//! - one form is the diagonal tension sum_k k_k^2 q_k^2 (Berger's plate):
//!   every mode stiffens with the total stretching, the pitch glide;
//! - `STRESS_MODES` more are sparse random forms, each over pairs of modes
//!   whose wavenumbers k_p, k_q can meet a stress wavenumber K_n
//!   (|k_p - k_q| <= K_n <= k_p + k_q, k = sqrt(f) for a plate), weighted
//!   k_p^2 k_q^2 (the curvatures) and tapered above a quarter of the band:
//!   they carry energy between modes.
//!
//! The potential (kappa / 4) (s_0^2 + cascade sum_n s_n^2) is never negative,
//! and it is stepped with the scalar auxiliary variable method (Bilbao,
//! Ducceschi, Zama, "Explicit exactly energy-conserving methods for
//! Hamiltonian systems", JCP 2023): psi = sqrt(2 V + eps) becomes a state of
//! its own and the update is linearly implicit with a rank-one matrix, solved
//! exactly per sample (Sherman-Morrison) in O(modes + pairs). The discrete
//! energy, kinetic + linear potential + psi^2 / 2, is a sum of non-negative
//! terms that only the loss lowers and only the `excite` input raises, for
//! any `drive`: the state stays bounded. The linear part is each mode's
//! exact two-pole resonator written as a damped Verlet step (h^2 w < 4 for
//! every mode under the top of the band), so with `drive` 0 the node is
//! `modal`'s plate to rounding.
//!
//! Two numbers were set by measurement, not physics. Weighted k_p k_q (the
//! slopes) the forms moved almost nothing; weighted k_p^2 k_q^2 without the
//! taper, a hard strike sent its energy to the top of the band within 20 ms
//! and held it there for the whole render (the top modes' added stiffness
//! pushed them past what a sample step can carry). With the taper at a
//! quarter of the band the highs come and go with the strike.

use serde::Deserialize;

use super::{MaterialSpec, Modal, ModalShape, impulse_peak, level_gain, one};
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Default `modes` (`modal` allows 1..256).
const DEFAULT_MODES: usize = 128;

/// In-plane stress forms besides the tension, and the mode pairs drawn for
/// each (1152 pairs: about three quarters of the per-sample work at 256
/// modes).
const STRESS_MODES: usize = 24;
const PAIRS_PER_STRESS: usize = 48;

/// The tension's strength at `drive` 1: at the loudest strike's peak
/// stretching (s_0 = 1 by the normalization) the lowest mode's squared
/// frequency rises by this fraction.
const TENSION: f64 = 0.1;

/// The stress forms' share at `cascade` 1. At `drive` 1 on the test strike
/// 100 raised the 30-80 ms centroid 4 % over the linear plate and 400 60 %;
/// 1600 (`cascade` 4) at `drive` 8 still let the highs go within 100 ms.
const CASCADE: f64 = 400.0;

/// The forms' weights fall as 1 / (1 + (f / taper)^2) above this fraction
/// of the top of the band (5 kHz at 48 kHz; at twice that, `drive` 8 and
/// `cascade` 4 held the centroid at 3.5-7 kHz past 300 ms).
const TAPER: f64 = 0.25;

/// `drive` and `cascade` limits.
const MAX_DRIVE: f32 = 10.0;
const MAX_CASCADE: f32 = 4.0;

/// Keeps psi = sqrt(2 V + eps) away from 0; V of a soft strike is ~1e-18.
const EPS: f64 = 1e-30;

/// A thin plate struck hard enough to stretch: `modal`'s plate whose modes
/// are coupled, so a harder strike brightens, crashes and glides in pitch.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plate {
    /// The strike: a `contact` pulse, an impulse, a noise burst, a
    /// `pattern` of hits. Its overall scale does not matter (the loudest
    /// strike is the one `drive` describes); a strike half as loud as the
    /// loudest is driven a quarter as hard.
    pub excite: In,
    /// stone wood metal glass ice earth granular plant soft snow, or
    /// {young, density, poisson, loss, thickness} (as `modal`; metal is the
    /// one that crashes).
    pub material: MaterialSpec,
    /// Side of the square plate, metres (as `modal`: pitch goes as
    /// 1 / size^2; the range for the material is in the error). Leave it out
    /// when `f0` is given.
    #[serde(default)]
    pub size: Option<In>,
    /// Frequency of the lowest mode, Hz (20..18000), instead of `size`.
    #[serde(default)]
    pub f0: Option<In>,
    /// Thickness, metres (0.0001..0.5, default the material's), as `modal`.
    #[serde(default)]
    pub thickness: Option<In>,
    /// Multiplier on the material's loss (0.01..100, default 1), as `modal`.
    #[serde(default)]
    pub loss: Option<f32>,
    /// Most modes (default 128, 1..256): the lowest half and the strongest
    /// of the rest. The nonlinearity needs room above the struck modes to
    /// move energy into; under about 32 there is little crash.
    #[serde(default)]
    pub modes: Option<usize>,
    /// Where it is struck, 0..1 (default 0.3; 0.5 is the centre), as `modal`.
    #[serde(default)]
    pub strike_position: Option<In>,
    /// dB per octave on the mode gains (default 0, -12..12), as `modal`.
    #[serde(default)]
    pub tilt: Option<f32>,
    /// How hard the plate is driven into its nonlinearity, 0..10 (default
    /// 1), for the loudest strike in `excite`. Alias `strength`. 0 is the
    /// linear plate (`modal`, to rounding); 0.25 is close to it; 1 a hard
    /// hit; higher a brighter, longer crash and a deeper glide. The strength
    /// goes as drive^2 x (the strike's level / the loudest's)^2.
    #[serde(default, alias = "strength")]
    pub drive: Option<f32>,
    /// How much of the nonlinearity moves energy between modes rather than
    /// only raising their pitch, 0..4 (default 1). 0 keeps the tension only
    /// (a glide, no crash).
    #[serde(default)]
    pub cascade: Option<f32>,
    /// Level of the output in dB (-60..60, default 0), after the
    /// normalization (a unit impulse at drive 0 peaks near 0 dBFS). It does
    /// not change how hard the plate is driven.
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0): the
    /// mistuning and the coupling.
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Plate {
    const BLURB: &'static str = "a thin plate struck hard enough to stretch (nonlinear, von Karman-like): modal's plate (material, size or f0, thickness, loss, modes, strike_position) whose modes are coupled, so drive (0 = linear) and the strike's level make it brighten into a crash within tens of ms, glide down in pitch and darken as it decays; cascade trades crash for glide; metal sheets, gongs, cymbals, thunder sheets, tearing structures";
    fn example() -> serde_json::Value {
        serde_json::json!({"excite": "contact1", "material": "metal", "size": 0.5,
            "thickness": 0.002, "drive": 1.5, "modes": 128})
    }
}

/// The couplings: the tension weights and the sparse stress forms.
struct Coupling {
    /// k_k^2 = f_k / f_1 (the tension form's diagonal).
    c0: Vec<f64>,
    /// Mode pairs and weights of every stress form, form by form.
    p: Vec<u32>,
    q: Vec<u32>,
    h: Vec<f64>,
    /// Form n's pairs are `starts[n]..starts[n + 1]`.
    starts: Vec<usize>,
}

impl Coupling {
    /// `ratio` is f_k / f_1, ascending; `f_low` f_1 and `top` the top of the
    /// band, Hz.
    fn new(ratio: &[f64], f_low: f64, top: f64, rng: &mut Rng) -> Coupling {
        let n = ratio.len();
        let k: Vec<f64> = ratio.iter().map(|r| r.sqrt()).collect();
        let (k_lo, k_hi) = (k[0], k[n - 1]);
        let (mut p, mut q, mut h) = (Vec::new(), Vec::new(), Vec::new());
        let mut starts = vec![0];
        let norm = 1.0 / (PAIRS_PER_STRESS as f64).sqrt();
        let corner = TAPER * top / f_low;
        let taper = |j: usize| 1.0 / (1.0 + (ratio[j] / corner).powi(2));
        if n > 1 {
            for s in 0..STRESS_MODES {
                // Stress wavenumbers spread geometrically from the lowest
                // mode's to twice the highest's (the sum of two).
                let t = (s as f64 + 0.5) / STRESS_MODES as f64;
                let big_k = k_lo * (2.0 * k_hi / k_lo).powf(t);
                for _ in 0..PAIRS_PER_STRESS {
                    let a = (rng.next_u64() % n as u64) as usize;
                    // Partners whose wavenumber closes the triangle.
                    let lo = k.partition_point(|v| *v < (big_k - k[a]).abs());
                    let hi = k.partition_point(|v| *v <= big_k + k[a]);
                    if hi <= lo {
                        continue;
                    }
                    let b = lo + (rng.next_u64() % (hi - lo) as u64) as usize;
                    let w = (0.5 + 0.5 * f64::from(rng.unit()))
                        * ratio[a]
                        * ratio[b]
                        * taper(a)
                        * taper(b)
                        * norm;
                    let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
                    p.push(a as u32);
                    q.push(b as u32);
                    h.push(sign * w);
                }
                starts.push(p.len());
            }
        }
        Coupling {
            c0: ratio.to_vec(),
            p,
            q,
            h,
            starts,
        }
    }
}

/// One mode's per-sample constants (see `step`).
struct ModeCoef {
    /// 1 + sigma h and 1 - sigma h.
    a: f64,
    am: f64,
    /// 2 - h^2 w.
    lin: f64,
    /// Input and output weights.
    inp: f64,
    out: f64,
}

/// Rings `x` through the coupled modes. `kappa` is the per-sample
/// nonlinear strength (0: linear), `mu` the cascade share.
fn ring(x: &[f32], coef: &[ModeCoef], cp: &Coupling, kappa: f64, mu: f64) -> Vec<f32> {
    let n = coef.len();
    let forms = cp.starts.len() - 1;
    let mut cur = vec![0.0f64; n];
    let mut prev = vec![0.0f64; n];
    let mut next = vec![0.0f64; n];
    let mut grad = vec![0.0f64; n];
    let mut s = vec![0.0f64; forms];
    let mut psi = EPS.sqrt();
    let nonlinear = kappa > 0.0;
    let mut out = Vec::with_capacity(x.len());
    for &input in x {
        let input = f64::from(input);
        if !nonlinear {
            let mut y = 0.0;
            for k in 0..n {
                let c = &coef[k];
                let v = (c.lin * cur[k] - c.am * prev[k] + c.inp * input) / c.a;
                prev[k] = cur[k];
                cur[k] = v;
                y += c.out * v;
            }
            out.push(y as f32);
            continue;
        }
        // The potential and its gradient at the current state.
        let mut s0 = 0.0;
        for k in 0..n {
            s0 += cp.c0[k] * cur[k] * cur[k];
        }
        let mut ss = 0.0;
        for f in 0..forms {
            let mut v = 0.0;
            for i in cp.starts[f]..cp.starts[f + 1] {
                v += cp.h[i] * cur[cp.p[i] as usize] * cur[cp.q[i] as usize];
            }
            s[f] = v;
            ss += v * v;
        }
        let pot = 0.25 * kappa * (s0 * s0 + mu * ss);
        for k in 0..n {
            grad[k] = kappa * s0 * cp.c0[k] * cur[k];
        }
        for f in 0..forms {
            let w = 0.5 * kappa * mu * s[f];
            for i in cp.starts[f]..cp.starts[f + 1] {
                let (a, b) = (cp.p[i] as usize, cp.q[i] as usize);
                let wh = w * cp.h[i];
                grad[a] += wh * cur[b];
                grad[b] += wh * cur[a];
            }
        }
        // g = grad V / psi(state); the step is
        //   a x+ = lin x - am x- + u - g (psi + (g.x+ - g.x-) / 4),
        // a rank-one system in x+.
        let psi_ref = (2.0 * pot + EPS).sqrt();
        // The auxiliary variable drifts from sqrt(2 V) (it is integrated,
        // not computed). Above it, it would push harder than the potential
        // does (once a late burst of highs at 300 ms); below 0 it would pull
        // the wrong way. Clamping it into 0..sqrt(2 V) only ever lowers
        // psi^2 / 2, so the energy bound still holds.
        psi = psi.clamp(0.0, psi_ref);
        let inv = 1.0 / psi_ref;
        let mut g1 = 0.0;
        for k in 0..n {
            grad[k] *= inv;
            g1 += grad[k] * prev[k];
        }
        let base = psi - 0.25 * g1;
        let (mut gc, mut gg) = (0.0, 0.0);
        for k in 0..n {
            let c = &coef[k];
            let v = (c.lin * cur[k] - c.am * prev[k] + c.inp * input - grad[k] * base) / c.a;
            next[k] = v;
            gc += grad[k] * v;
            gg += grad[k] * grad[k] / c.a;
        }
        let z = gc / (1.0 + 0.25 * gg);
        let mut y = 0.0;
        for k in 0..n {
            let v = next[k] - 0.25 * grad[k] * z / coef[k].a;
            prev[k] = cur[k];
            cur[k] = v;
            y += coef[k].out * v;
        }
        psi += 0.5 * (z - g1);
        if !y.is_finite() {
            // Only an input beyond f64 gets here: start again from rest.
            cur.iter_mut().for_each(|v| *v = 0.0);
            prev.iter_mut().for_each(|v| *v = 0.0);
            psi = EPS.sqrt();
            y = 0.0;
        }
        out.push(y as f32);
    }
    out
}

/// The largest tension sum s_0 = sum_k c0_k q_k^2 of the linear ring of `x`.
fn linear_stretch_peak(x: &[f32], coef: &[ModeCoef], c0: &[f64]) -> f64 {
    let n = coef.len();
    let mut cur = vec![0.0f64; n];
    let mut prev = vec![0.0f64; n];
    let mut top = 0.0f64;
    for &input in x {
        let input = f64::from(input);
        let mut s0 = 0.0;
        for k in 0..n {
            let c = &coef[k];
            let v = (c.lin * cur[k] - c.am * prev[k] + c.inp * input) / c.a;
            prev[k] = cur[k];
            cur[k] = v;
            s0 += c0[k] * v * v;
        }
        top = top.max(s0);
    }
    if top.is_finite() { top } else { 0.0 }
}

impl Eval for Plate {
    fn random(&self) -> bool {
        true
    }
    /// The loudest strike in the whole input sets the reference (lookahead).
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let input = cx.audio("excite", &self.excite)?;
        let count = self.modes.unwrap_or(DEFAULT_MODES);
        let drive = self.drive.unwrap_or(1.0);
        if !(drive.is_finite() && (0.0..=MAX_DRIVE).contains(&drive)) {
            return Err(format!(
                "drive: {drive}: give 0..{MAX_DRIVE} (0 = the linear plate, 1 = a hard hit)"
            ));
        }
        let cascade = self.cascade.unwrap_or(1.0);
        if !(cascade.is_finite() && (0.0..=MAX_CASCADE).contains(&cascade)) {
            return Err(format!(
                "cascade: {cascade}: give 0..{MAX_CASCADE} (0 = pitch glide only, 1 = the default crash)"
            ));
        }
        // The mode table and every shared check are `modal`'s.
        let modal = Modal {
            excite: self.excite.clone(),
            material: self.material.clone(),
            size: self.size.clone(),
            f0: self.f0.clone(),
            f_min: None,
            f_max: None,
            shape: ModalShape::Plate,
            strike_position: self.strike_position.clone(),
            modes: Some(count),
            stiffness: None,
            loss: self.loss,
            thickness: self.thickness.clone(),
            tilt: self.tilt,
            gain_db: None,
            strike_glide: None,
            rattle: None,
            noise: None,
            noise_width: None,
            seed: self.seed,
        };
        let prepared = modal.prepare(cx)?;
        let out_gain = f64::from(level_gain(self.gain_db)?.unwrap_or(1.0));
        let mut modes = prepared.modes;
        if prepared.tilt_db != 0.0 {
            let f0 = modes[0].0;
            for m in &mut modes {
                m.2 *= 10f32.powf(prepared.tilt_db * (m.0 / f0).log2() / 20.0);
            }
        }
        let peak = impulse_peak(&modes, cx.sr);
        let scale = if peak > 1e-12 { 1.0 / peak } else { 1.0 };
        let sr = f64::from(cx.sr);
        let f_low = f64::from(modes[0].0);
        let ratio: Vec<f64> = modes.iter().map(|m| f64::from(m.0) / f_low).collect();
        let mut coef: Vec<ModeCoef> = modes
            .iter()
            .zip(&ratio)
            .map(|(&(f, t60, g), &rk)| {
                let theta = std::f64::consts::TAU * f64::from(f) / sr;
                let r = (-(1000f64.ln()) / (f64::from(t60).max(1e-4) * sr)).exp();
                let sigma = (1.0 - r * r) / (1.0 + r * r);
                let a = 1.0 + sigma;
                // Strike and radiation share the gain: their product is
                // `modal`'s, so the linear ring is the same. The state is a
                // displacement (1 / f of the velocity that is heard).
                let w = f64::from(g * scale).max(0.0).sqrt();
                ModeCoef {
                    a,
                    am: 1.0 - sigma,
                    lin: 2.0 * r * theta.cos() * a,
                    inp: a * w * theta.sin() / rk,
                    out: out_gain * w * rk,
                }
            })
            .collect();
        let mut rng = Rng::new(crate::mix_seed(cx.seed, self.seed ^ 0x0091_a7e5));
        let cp = Coupling::new(
            &ratio,
            f_low,
            f64::from(crate::dsp::max_hz(cx.sr).min(20_000.0)),
            &mut rng,
        );
        // The loudest strike sets the reference: its linear ring stretches
        // the plate to s_0 = 1 inside the model, whatever the excite's
        // scale, so `drive` alone says how hard that strike is and a softer
        // strike in the same input is softer in proportion.
        let channels = input.channels(cx.n);
        let stretch = channels
            .iter()
            .map(|c| linear_stretch_peak(c, &coef, &cp.c0))
            .fold(0.0f64, f64::max);
        let pre = if stretch > 0.0 {
            1.0 / stretch.sqrt()
        } else {
            1.0
        };
        for c in &mut coef {
            c.inp *= pre;
            c.out /= pre;
        }
        // Per-sample units: the lowest mode's (h w_1)^2 sets the scale, so
        // `drive` means the same at every size and sample rate.
        let theta1 = std::f64::consts::TAU * f_low / sr;
        let kappa = TENSION * f64::from(drive * drive) * theta1 * theta1;
        let mu = CASCADE * f64::from(cascade);
        Ok(one(crate::buf::Buf::from_channels(
            channels
                .iter()
                .map(|x| ring(x, &coef, &cp, kappa, mu))
                .collect(),
        )))
    }
}
