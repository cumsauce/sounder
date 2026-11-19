//! Per-sample DSP state: each piece is a small struct with a `tick`, so the
//! block evaluator loops over it and the feedback evaluator (a cycle
//! through a `delay`, run sample by sample) can reuse the same code.

use std::f32::consts::PI;

pub use foley_dsp::Rng;

/// Zero for anything below audibility: a filter ringing down in silence would
/// otherwise reach subnormal floats (copied from foley's `filter.rs`).
#[inline]
pub fn flush(y: f32) -> f32 {
    if y.abs() < 1e-20 { 0.0 } else { y }
}

/// The highest frequency a filter is tuned to: just under Nyquist.
#[inline]
pub fn max_hz(sr: f32) -> f32 {
    0.49 * sr
}

// ------------------------------------------------------------- oscillators

/// polyBLEP: the band-limited step correction at phase `t` (0..1) for a
/// phase increment `dt`.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Tri,
    Pulse,
}

/// A band-limited oscillator (polyBLEP saw, square and pulse; the triangle
/// is the leaky integral of the band-limited square).
#[derive(Debug, Clone)]
pub struct Osc {
    pub wave: Wave,
    phase: f64,
    tri: f32,
    /// The phase modulation of the previous sample (cycles), for its slope.
    pm: f32,
    started: bool,
}

impl Osc {
    pub fn new(wave: Wave, phase: f32) -> Osc {
        Osc {
            wave,
            phase: f64::from(phase.rem_euclid(1.0)),
            tri: 0.0,
            pm: 0.0,
            started: false,
        }
    }

    /// The next sample at `hz` (negative runs backwards) with pulse `width`.
    #[inline]
    pub fn tick(&mut self, hz: f32, width: f32, sr: f32) -> f32 {
        let dt = (hz / sr).clamp(-0.5, 0.5);
        let out = self.shape(self.phase as f32, dt.abs(), width);
        self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
        out
    }

    /// The waveform at phase `t` (0..1) for a phase step of `adt` cycles per
    /// sample (the polyBLEP width), pulse `width`.
    #[inline]
    fn shape(&mut self, t: f32, adt: f32, width: f32) -> f32 {
        match self.wave {
            Wave::Sine => (2.0 * PI * t).sin(),
            Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, adt),
            Wave::Square | Wave::Pulse | Wave::Tri => {
                let w = if self.wave == Wave::Pulse {
                    width.clamp(0.01, 0.99)
                } else {
                    0.5
                };
                let mut v = if t < w { 1.0 } else { -1.0 };
                v += poly_blep(t, adt);
                v -= poly_blep((t - w).rem_euclid(1.0), adt);
                if self.wave == Wave::Tri {
                    // Leaky integration: 4 * dt keeps the amplitude near 1.
                    self.tri = flush(4.0 * adt * v + (1.0 - 0.5 * adt) * self.tri);
                    self.tri
                } else {
                    v
                }
            }
        }
    }

    /// The waveform with no band-limiting at phase `t` (the value either
    /// side of a sync reset; the triangle counts as its square).
    fn naive(&self, t: f32, width: f32) -> f32 {
        match self.wave {
            Wave::Sine => (2.0 * PI * t).sin(),
            Wave::Saw => 2.0 * t - 1.0,
            Wave::Pulse => {
                if t < width.clamp(0.01, 0.99) {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Square | Wave::Tri => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    /// Like [`Osc::tick`] with phase modulation `pm` (cycles, added to the
    /// phase) and hard sync: `sync` is the fraction (0..1) of this sample
    /// step at which the master crossed zero upwards, when it did. Returns
    /// `(sample, fix)`: `fix` is the polyBLEP correction the PREVIOUS sample
    /// needs (add it there; always 0 unless `sync` is set, and 0 for the
    /// triangle, which folds it into this sample's integrator input).
    #[inline]
    pub fn tick_ex(
        &mut self,
        hz: f32,
        width: f32,
        sr: f32,
        pm: f32,
        sync: Option<f32>,
    ) -> (f32, f32) {
        let dt = (hz / sr).clamp(-0.5, 0.5);
        // A first sample has no previous modulation to differ from.
        let dpm = if self.started { pm - self.pm } else { 0.0 };
        self.started = true;
        self.pm = pm;
        // Cycles per sample the waveform's own phase moves, modulation included.
        let adt = (dt + dpm).clamp(-0.5, 0.5).abs();
        let Some(f) = sync else {
            let t = (self.phase as f32 + pm).rem_euclid(1.0);
            let out = self.shape(t, adt, width);
            self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
            return (out, 0.0);
        };
        let f = f.clamp(0.0, 1.0);
        // The step the reset makes, and the band-limited correction for the
        // sample before it (distance f ahead of the step) and the one after
        // (1 - f behind): the polyBLEP residuals of a unit-width kernel.
        let before = (self.phase as f32 + f * dt + pm).rem_euclid(1.0);
        let step = self.naive(pm.rem_euclid(1.0), width) - self.naive(before, width);
        let fix_prev = 0.5 * step * (1.0 - f) * (1.0 - f);
        let fix_now = -0.5 * step * f * f;
        self.phase = (f64::from((1.0 - f) * dt)).rem_euclid(1.0);
        let t = (self.phase as f32 + pm).rem_euclid(1.0);
        if self.wave == Wave::Tri {
            // The square's edge at 0.5 gets its usual correction; the reset
            // step is folded into this sample's integrator input.
            let mut v = if t < 0.5 { 1.0 } else { -1.0 };
            v -= poly_blep((t - 0.5).rem_euclid(1.0), adt);
            v += fix_prev + fix_now;
            self.tri = flush(4.0 * adt * v + (1.0 - 0.5 * adt) * self.tri);
            self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
            return (self.tri, 0.0);
        }
        let mut out = self.naive(t, width) + fix_now;
        match self.wave {
            Wave::Square | Wave::Pulse => {
                let w = if self.wave == Wave::Pulse {
                    width.clamp(0.01, 0.99)
                } else {
                    0.5
                };
                out -= poly_blep((t - w).rem_euclid(1.0), adt);
            }
            Wave::Saw | Wave::Sine | Wave::Tri => {}
        }
        self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
        (out, fix_prev)
    }
}

// ------------------------------------------------------------------ filters

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Lp,
    Hp,
    Bp,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

/// Andrew Simper's trapezoidal (TPT) state-variable filter: stable under
/// audio-rate modulation of frequency and Q.
#[derive(Debug, Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// One sample through the filter tuned to `hz`, `q`, `gain_db` (peak and
    /// shelves only).
    #[inline]
    pub fn tick(
        &mut self,
        x: f32,
        mode: FilterMode,
        hz: f32,
        q: f32,
        gain_db: f32,
        sr: f32,
    ) -> f32 {
        let hz = hz.clamp(1.0, max_hz(sr));
        let q = q.max(0.025);
        let w = (PI * hz / sr).tan();
        let a = 10f32.powf(gain_db / 40.0);
        let (g, k, m) = match mode {
            FilterMode::Lp => (w, 1.0 / q, [0.0, 0.0, 1.0]),
            FilterMode::Hp => (w, 1.0 / q, [1.0, -1.0 / q, -1.0]),
            FilterMode::Bp => (w, 1.0 / q, [0.0, 1.0, 0.0]),
            FilterMode::Notch => (w, 1.0 / q, [1.0, -1.0 / q, 0.0]),
            FilterMode::Peak => {
                let k = 1.0 / (q * a);
                (w, k, [1.0, k * (a * a - 1.0), 0.0])
            }
            FilterMode::LowShelf => {
                let k = 1.0 / q;
                (w / a.sqrt(), k, [1.0, k * (a - 1.0), a * a - 1.0])
            }
            FilterMode::HighShelf => {
                let k = 1.0 / q;
                (w * a.sqrt(), k, [a * a, k * (1.0 - a) * a, 1.0 - a * a])
            }
        };
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        m[0] * x + m[1] * v1 + m[2] * v2
    }
}

/// A one-pole TPT filter (6 dB an octave).
#[derive(Debug, Clone, Default)]
pub struct OnePole {
    s: f32,
}

impl OnePole {
    /// (low-pass, high-pass) of `x` at `hz`.
    #[inline]
    pub fn tick(&mut self, x: f32, hz: f32, sr: f32) -> (f32, f32) {
        let g = (PI * hz.clamp(0.01, max_hz(sr)) / sr).tan();
        let g = g / (1.0 + g);
        let v = (x - self.s) * g;
        let lp = v + self.s;
        self.s = flush(lp + v);
        (lp, x - lp)
    }
}

/// A two-pole resonator ringing at `hz` that falls 60 dB in `decay` seconds;
/// an impulse of 1 starts it at amplitude `gain`.
#[derive(Debug, Clone)]
pub struct Resonator {
    b0: f32,
    a1: f32,
    a2: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    pub fn new(hz: f32, decay: f32, gain: f32, sr: f32) -> Resonator {
        let w = 2.0 * PI * hz.clamp(1.0, max_hz(sr)) / sr;
        let r = (-(1000f32.ln()) / (decay.max(1e-4) * sr)).exp();
        Resonator {
            b0: gain * w.sin(),
            a1: 2.0 * r * w.cos(),
            a2: -r * r,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = flush(self.b0 * x + self.a1 * self.y1 + self.a2 * self.y2);
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

// -------------------------------------------------------------------- delay

/// A ring buffer read at a fractional delay (cubic Hermite interpolation).
#[derive(Debug, Clone)]
pub struct DelayLine {
    buf: Vec<f32>,
    w: usize,
}

impl DelayLine {
    /// Holds up to `max` samples of delay.
    pub fn new(max: usize) -> DelayLine {
        DelayLine {
            buf: vec![0.0; max.max(4) + 4],
            w: 0,
        }
    }

    pub fn max(&self) -> usize {
        self.buf.len() - 4
    }

    /// The sample written `d` samples ago (`d` >= 1 reads before writing; 0
    /// reads the last write).
    #[inline]
    pub fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = d.clamp(0.0, self.max() as f32);
        let di = d.floor();
        let f = d - di;
        let base = self.w + n - 1 - di as usize;
        let at = |k: isize| self.buf[(base as isize - k).rem_euclid(n as isize) as usize];
        let (xm1, x0, x1, x2) = (at(-1), at(0), at(1), at(2));
        let c0 = x0;
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + c0
    }

    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.w] = flush(x);
        self.w = (self.w + 1) % self.buf.len();
    }
}

// ---------------------------------------------------------------- dynamics

/// A level follower in dB with separate attack and release times.
#[derive(Debug, Clone)]
pub struct Follower {
    pub env: f32,
}

impl Follower {
    pub fn new() -> Follower {
        Follower { env: 0.0 }
    }

    /// Follows `x` (a linear level >= 0) with `attack` / `release` seconds.
    #[inline]
    pub fn tick(&mut self, x: f32, attack: f32, release: f32, sr: f32) -> f32 {
        let t = if x > self.env { attack } else { release };
        let c = if t <= 0.0 {
            0.0
        } else {
            (-1.0 / (t * sr)).exp()
        };
        self.env = flush(x + c * (self.env - x));
        self.env
    }
}

impl Default for Follower {
    fn default() -> Self {
        Follower::new()
    }
}

/// A lookahead peak limiter over a whole buffer (acausal): the gain reaches
/// its floor before a peak arrives (1.5 ms lookahead) and recovers over
/// `release` seconds; it never lets a sample above `ceiling` through.
/// Takes each sample's peak over the channels; returns the gain per sample.
pub fn limiter_gain(peak_per_sample: &[f32], ceiling: f32, release: f32, sr: f32) -> Vec<f32> {
    let n = peak_per_sample.len();
    let look = ((0.0015 * sr) as usize).max(1);
    let need: Vec<f32> = peak_per_sample
        .iter()
        .map(|p| if *p > ceiling { ceiling / p } else { 1.0 })
        .collect();
    // m[j] = min(need[j ..= j + look]) (monotonic deque, scanning backwards).
    let mut m = vec![1.0f32; n];
    let mut dq: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for j in (0..n).rev() {
        while dq.back().is_some_and(|&k| need[k] >= need[j]) {
            dq.pop_back();
        }
        dq.push_back(j);
        while dq.front().is_some_and(|&k| k > j + look) {
            dq.pop_front();
        }
        m[j] = dq.front().map_or(1.0, |&k| need[k]);
    }
    // A box average of m over [i - look + 1, i]: every window in it covers
    // sample i, so the average is <= need[i] (no overshoot) yet ramps
    // smoothly into each peak.
    let mut avg = vec![1.0f32; n];
    let mut sum = 0.0f64;
    for i in 0..n {
        sum += f64::from(m[i]);
        if i >= look {
            sum -= f64::from(m[i - look]);
        }
        let count = look.min(i + 1);
        let pre = (look - count) as f64; // samples before the start, at gain 1
        avg[i] = (((sum + pre) / look as f64) as f32).min(need[i]);
    }
    // Release: rise back towards 1 no faster than `release`, never above avg.
    let rel = 1.0 - (-1.0 / (release.max(1e-3) * sr)).exp();
    let mut g = 1.0f32;
    for a in &mut avg {
        g = (g + (1.0 - g) * rel).min(*a);
        *a = g;
    }
    avg
}

// ------------------------------------------------------------------ curves

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Exp,
    Smooth,
}

/// Interpolates from `a` to `b` at `x` (0..1) along `curve`. `Exp` moves
/// geometrically when both ends are positive (pitch sweeps), else falls back
/// to a quadratic ease-out.
#[inline]
pub fn interp(a: f32, b: f32, x: f32, curve: Curve) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match curve {
        Curve::Linear => a + (b - a) * x,
        Curve::Smooth => a + (b - a) * (0.5 - 0.5 * (PI * x).cos()),
        Curve::Exp => {
            if a > 0.0 && b > 0.0 {
                a * (b / a).powf(x)
            } else {
                let e = 1.0 - (1.0 - x) * (1.0 - x);
                a + (b - a) * e
            }
        }
    }
}

// ============================================================ shared kit
//
// Pieces several nodes share.

// ------------------------------------------------------------------ biquad

/// A direct biquad (transposed direct form II, `f64` state) with the RBJ
/// cookbook coefficients, for cascades (steeper slopes) and EQ. Use [`Svf`]
/// when the frequency or Q is modulated at audio rate: swapping a biquad's
/// coefficients every sample can click.
#[derive(Debug, Clone)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
}

impl Default for Biquad {
    /// A pass-through.
    fn default() -> Self {
        Biquad {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }
}

impl Biquad {
    /// A pass-through until [`Biquad::set`] is called.
    pub fn new() -> Biquad {
        Biquad::default()
    }

    /// Tunes the filter (the state is kept). `hz` is clamped to 1 Hz ..
    /// just under Nyquist, `q` to 0.025 ..= 1000, `gain_db` to +-60 (peak
    /// and shelves only). `Bp` has 0 dB gain at `hz` (constant peak gain);
    /// shelves use `q` as the RBJ shelf Q (0.707 is the maximally flat
    /// shelf). A non-finite argument falls back to 1 kHz / Q 0.707 / 0 dB.
    pub fn set(&mut self, mode: FilterMode, hz: f32, q: f32, gain_db: f32, sr: f32) {
        let fin = |x: f32, d: f32| if x.is_finite() { x } else { d };
        let hz = f64::from(fin(hz, 1000.0).clamp(1.0, max_hz(sr)));
        let q = f64::from(fin(q, 0.707).clamp(0.025, 1000.0));
        let gain_db = f64::from(fin(gain_db, 0.0).clamp(-60.0, 60.0));
        let w0 = std::f64::consts::TAU * hz / f64::from(sr);
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let a = 10f64.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match mode {
            FilterMode::Lp => (
                (1.0 - cs) / 2.0,
                1.0 - cs,
                (1.0 - cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            FilterMode::Hp => (
                (1.0 + cs) / 2.0,
                -(1.0 + cs),
                (1.0 + cs) / 2.0,
                1.0 + alpha,
                -2.0 * cs,
                1.0 - alpha,
            ),
            FilterMode::Bp => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterMode::Notch => (1.0, -2.0 * cs, 1.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            FilterMode::Peak => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
            FilterMode::LowShelf => {
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + t),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - t),
                    (a + 1.0) + (a - 1.0) * cs + t,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - t,
                )
            }
            FilterMode::HighShelf => {
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + t),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - t),
                    (a + 1.0) - (a - 1.0) * cs + t,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - t,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    /// One sample through the filter.
    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let x = f64::from(x);
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        self.s1 = flush64(self.s1);
        self.s2 = flush64(self.s2);
        y as f32
    }

    /// Clears the state.
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// The linear gain of the current tuning at `hz`.
    pub fn magnitude(&self, hz: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num =
            (self.b0 + self.b1 * c1 + self.b2 * c2).powi(2) + (self.b1 * s1 + self.b2 * s2).powi(2);
        let den =
            (1.0 + self.a1 * c1 + self.a2 * c2).powi(2) + (self.a1 * s1 + self.a2 * s2).powi(2);
        (num / den.max(1e-300)).sqrt() as f32
    }
}

#[inline]
fn flush64(y: f64) -> f64 {
    if y.abs() < 1e-30 { 0.0 } else { y }
}

// ------------------------------------------------------------- oversampler

/// A ring of the last `n` pushed samples, `get(0)` the newest.
#[derive(Debug, Clone)]
struct Hist {
    buf: Vec<f32>,
    pos: usize,
    mask: usize,
}

impl Hist {
    fn new(n: usize) -> Hist {
        let len = n.max(1).next_power_of_two();
        Hist {
            buf: vec![0.0; len],
            pos: 0,
            mask: len - 1,
        }
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }

    #[inline]
    fn get(&self, i: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(i) & self.mask]
    }
}

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0f64, 1.0f64);
    let h = x * x / 4.0;
    for k in 1..60 {
        term *= h / (k as f64 * k as f64);
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

/// The odd taps of a Kaiser-windowed half-band low-pass of `4 m + 3` taps
/// (centre tap 0.5, DC gain 1), as the branch a polyphase stage uses:
/// `taps[i]` is the tap at offset `2 i - (2 m + 1)`.
fn halfband_taps(m: usize) -> Vec<f32> {
    let c = 2 * m + 1;
    let beta = 8.96; // ~90 dB stop band
    let i0b = bessel_i0(beta);
    let mut t: Vec<f64> = (0..=c)
        .map(|i| {
            let k = 2.0 * i as f64 - c as f64; // odd
            let win = bessel_i0(beta * (1.0 - (k / c as f64).powi(2)).max(0.0).sqrt()) / i0b;
            (std::f64::consts::FRAC_PI_2 * k).sin() / (std::f64::consts::PI * k) * win
        })
        .collect();
    let s: f64 = t.iter().sum();
    for v in &mut t {
        *v *= 0.5 / s;
    }
    t.into_iter().map(|v| v as f32).collect()
}

/// The `m` (taps = `4 m + 3`) of a half-band whose transition is `df` wide
/// (as a fraction of the sample rate it runs at) for ~90 dB.
fn halfband_m(df: f64) -> usize {
    let n = (82.0 / (2.285 * std::f64::consts::TAU * df)).ceil() + 1.0;
    (((n - 3.0) / 4.0).ceil().max(1.0)) as usize
}

#[derive(Debug, Clone)]
struct HalfUp {
    taps: Vec<f32>,
    m: usize,
    hx: Hist,
}

impl HalfUp {
    fn new(m: usize) -> HalfUp {
        HalfUp {
            taps: halfband_taps(m),
            m,
            hx: Hist::new(2 * m + 3),
        }
    }

    /// One input sample in, two (in time order) out at twice the rate.
    #[inline]
    fn push(&mut self, x: f32) -> (f32, f32) {
        self.hx.push(x);
        let mut a = 0.0;
        for (i, t) in self.taps.iter().enumerate() {
            a += t * self.hx.get(i);
        }
        (2.0 * a, self.hx.get(self.m))
    }
}

#[derive(Debug, Clone)]
struct HalfDown {
    taps: Vec<f32>,
    m: usize,
    he: Hist,
    ho: Hist,
}

impl HalfDown {
    fn new(m: usize) -> HalfDown {
        HalfDown {
            taps: halfband_taps(m),
            m,
            he: Hist::new(2 * m + 3),
            ho: Hist::new(m + 2),
        }
    }

    /// Two input samples (time order) in, one out at half the rate.
    #[inline]
    fn push(&mut self, even: f32, odd: f32) -> f32 {
        self.he.push(even);
        let mut z = 0.5 * self.ho.get(self.m);
        for (i, t) in self.taps.iter().enumerate() {
            z += t * self.he.get(i);
        }
        self.ho.push(odd);
        z
    }
}

#[derive(Debug, Clone)]
struct OsStage {
    up: HalfUp,
    down: HalfDown,
    /// This stage's delay in input-rate samples (up and down together).
    latency: f32,
}

/// Runs a nonlinearity at 1x, 2x, 4x or 8x the sample rate to keep the
/// harmonics it makes from folding back: every doubling is a Kaiser-windowed
/// half-band FIR (about 90 dB stop band, flat to 0.45 of the input rate: 21.6 kHz at 48 kHz),
/// run as a polyphase stage so the zero taps cost nothing. Linear phase; the
/// delay is [`Oversampler::latency`] samples (a fractional number for 4x and
/// 8x). Factor 1 calls `f` once on the input.
#[derive(Debug, Clone)]
pub struct Oversampler {
    factor: usize,
    stages: Vec<OsStage>,
}

impl Oversampler {
    /// `factor` is 1, 2, 4 or 8; anything else is an error naming the limit.
    pub fn new(factor: usize) -> Result<Oversampler, String> {
        let n = match factor {
            1 => 0,
            2 => 1,
            4 => 2,
            8 => 3,
            _ => return Err(format!("oversample: {factor}: give 1, 2, 4 or 8")),
        };
        let stages = (1..=n)
            .map(|s| {
                // The signal band is 0.45 of the input rate, so at this
                // stage's output rate it ends at 0.4 / 2^s of it.
                let fp = 0.45 / f64::from(1u32 << s);
                let m = halfband_m(0.5 - 2.0 * fp);
                OsStage {
                    up: HalfUp::new(m),
                    down: HalfDown::new(m),
                    latency: (2 * m + 1) as f32 / (1u32 << (s - 1)) as f32,
                }
            })
            .collect();
        Ok(Oversampler { factor, stages })
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    /// The delay in samples at the input rate.
    pub fn latency(&self) -> f32 {
        self.stages.iter().map(|s| s.latency).sum()
    }

    /// One input sample through `f` run `factor` times at the raised rate.
    #[inline]
    pub fn process(&mut self, x: f32, mut f: impl FnMut(f32) -> f32) -> f32 {
        Self::run(&mut self.stages, x, &mut f)
    }

    fn run<F: FnMut(f32) -> f32>(stages: &mut [OsStage], x: f32, f: &mut F) -> f32 {
        match stages.split_first_mut() {
            None => f(x),
            Some((st, rest)) => {
                let (a, b) = st.up.push(x);
                let ya = Self::run(rest, a, f);
                let yb = Self::run(rest, b, f);
                st.down.push(ya, yb)
            }
        }
    }
}

// ----------------------------------------------------------------- hilbert

/// Allpass coefficients (in `z^-2`) of the in-phase chain of [`Hilbert`].
pub const HILBERT_A: [f64; 5] = [
    0.15030967283332752,
    0.7068190928936895,
    0.9326428889150189,
    0.9855949703492775,
    0.9982168543298119,
];
/// The quadrature chain (followed by one sample of delay).
pub const HILBERT_B: [f64; 4] = [
    0.4538302067877107,
    0.8566155331642705,
    0.9687635109754964,
    0.9936994265086011,
];

/// A pair of allpass chains whose outputs stay 90 degrees apart (`q` lags
/// `i`, as the sine of a cosine) over 20 Hz to 20 kHz at 48 kHz, within 0.6
/// degrees (measured); both have exactly unit gain. For single-sideband
/// shifting and envelope detection. Coefficients were fitted for 48 kHz: at
/// another rate the covered band scales with it (about 0.0004 to 0.42 of
/// the rate). State is `f64` because the lowest poles sit at 0.998.
#[derive(Debug, Clone)]
pub struct Hilbert {
    a: [[f64; 2]; 5],
    b: [[f64; 2]; 4],
    q_delay: f64,
}

impl Default for Hilbert {
    fn default() -> Self {
        Hilbert::new()
    }
}

impl Hilbert {
    pub fn new() -> Hilbert {
        Hilbert {
            a: [[0.0; 2]; 5],
            b: [[0.0; 2]; 4],
            q_delay: 0.0,
        }
    }

    /// One allpass `(c - z^-2) / (1 - c z^-2)`: `y = c x + d2`, with the two
    /// delays holding `-x + c y`.
    #[inline]
    fn section(c: f64, s: &mut [f64; 2], x: f64) -> f64 {
        let y = c * x + s[1];
        s[1] = s[0];
        s[0] = flush64(-x + c * y);
        y
    }

    /// (in-phase, quadrature) for one input sample.
    #[inline]
    pub fn tick(&mut self, x: f32) -> (f32, f32) {
        let mut i = f64::from(x);
        for (c, s) in HILBERT_A.iter().zip(&mut self.a) {
            i = Self::section(*c, s, i);
        }
        let mut q = f64::from(x);
        for (c, s) in HILBERT_B.iter().zip(&mut self.b) {
            q = Self::section(*c, s, q);
        }
        let q_out = self.q_delay;
        self.q_delay = q;
        (i as f32, q_out as f32)
    }
}

// --------------------------------------------------------------- lagrange3

impl DelayLine {
    /// The sample written `d` samples ago by third-order Lagrange
    /// interpolation: unlike linear interpolation it keeps the high end of
    /// a modulated delay (gain error 0.5 dB at a fifth of the sample rate at
    /// half a sample, against 1.8 dB for linear). Exact at integer delays.
    /// At half a sample it is the same as [`DelayLine::read`]'s Hermite; the
    /// two differ for other fractions.
    /// `d` follows [`DelayLine::read`] (1 or more reads before the write
    /// that follows; clamped to the buffer). Between 0 and 1 the four taps
    /// extrapolate, so give a feedback loop at least one sample of delay.
    #[inline]
    pub fn read_lagrange3(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let d = if d.is_finite() { d } else { 0.0 };
        let d = d.clamp(0.0, self.max() as f32);
        // Four taps at delays start .. start + 3, d between the middle two.
        let start = (d.floor() as usize).saturating_sub(1);
        let t = d - start as f32;
        let at = |k: usize| self.buf[(self.w + n - 1 - start - k) % n];
        let l0 = -(t - 1.0) * (t - 2.0) * (t - 3.0) / 6.0;
        let l1 = t * (t - 2.0) * (t - 3.0) / 2.0;
        let l2 = -t * (t - 1.0) * (t - 3.0) / 2.0;
        let l3 = t * (t - 1.0) * (t - 2.0) / 6.0;
        l0 * at(0) + l1 * at(1) + l2 * at(2) + l3 * at(3)
    }
}

// ------------------------------------------------------------------ smooth

/// A one-pole smoother for control values (a click-free parameter jump):
/// the output closes 63 % of the gap to the target every `time` seconds.
/// The state is `f64` so a long time constant still reaches its target.
#[derive(Debug, Clone)]
pub struct Smooth {
    value: f64,
}

impl Smooth {
    /// Starts at `value` (no glide on the first sample).
    pub fn new(value: f32) -> Smooth {
        Smooth {
            value: f64::from(value),
        }
    }

    /// The current output.
    pub fn value(&self) -> f32 {
        self.value as f32
    }

    /// Moves towards `target` with time constant `time` seconds (0 or less
    /// jumps). A non-finite target is ignored.
    #[inline]
    pub fn tick(&mut self, target: f32, time: f32, sr: f32) -> f32 {
        if !target.is_finite() {
            return self.value as f32;
        }
        let target = f64::from(target);
        if time <= 0.0 || !time.is_finite() {
            self.value = target;
        } else {
            let c = (-1.0 / (f64::from(time) * f64::from(sr))).exp();
            self.value = flush64(target + c * (self.value - target));
        }
        self.value as f32
    }
}

#[cfg(test)]
mod kit_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use realfft::RealFftPlanner;

    const SR: f32 = 48_000.0;

    fn sine(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| {
                (std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR)).sin() as f32
            })
            .collect()
    }

    /// Hann-windowed power spectrum.
    fn power(x: &[f32]) -> Vec<f32> {
        let n = x.len();
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(n);
        let mut inp: Vec<f32> = x
            .iter()
            .enumerate()
            .map(|(i, v)| v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()))
            .collect();
        let mut out = fft.make_output_vec();
        fft.process(&mut inp, &mut out).unwrap();
        out.iter().map(|z| z.norm_sqr()).collect()
    }

    fn run_biquad(mode: FilterMode, hz: f32, q: f32, g: f32, x: &[f32]) -> Vec<f32> {
        let mut b = Biquad::new();
        b.set(mode, hz, q, g, SR);
        x.iter().map(|v| b.tick(*v)).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn db(g: f32) -> f32 {
        20.0 * g.max(1e-9).log10()
    }

    #[test]
    fn biquad_modes_have_the_cookbook_gains() {
        let mut b = Biquad::new();
        let mut at = |mode, hz, q, g, probe| {
            b.set(mode, hz, q, g, SR);
            db(b.magnitude(probe, SR))
        };
        let lp = at(
            FilterMode::Lp,
            1000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            1000.0,
        );
        let lp_far = at(
            FilterMode::Lp,
            1000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            8000.0,
        );
        let hp = at(
            FilterMode::Hp,
            1000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            1000.0,
        );
        let hp_low = at(
            FilterMode::Hp,
            1000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            125.0,
        );
        let bp = at(FilterMode::Bp, 2000.0, 4.0, 0.0, 2000.0);
        let notch = at(FilterMode::Notch, 2000.0, 4.0, 0.0, 2000.0);
        let notch_far = at(FilterMode::Notch, 2000.0, 4.0, 0.0, 200.0);
        let peak = at(FilterMode::Peak, 3000.0, 2.0, 6.0, 3000.0);
        let low_dc = at(
            FilterMode::LowShelf,
            500.0,
            std::f32::consts::FRAC_1_SQRT_2,
            6.0,
            20.0,
        );
        let low_hi = at(
            FilterMode::LowShelf,
            500.0,
            std::f32::consts::FRAC_1_SQRT_2,
            6.0,
            10_000.0,
        );
        let hi_top = at(
            FilterMode::HighShelf,
            4000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            -9.0,
            20_000.0,
        );
        let hi_low = at(
            FilterMode::HighShelf,
            4000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            -9.0,
            100.0,
        );
        println!(
            "lp {lp:.2} (far {lp_far:.1}), hp {hp:.2} (low {hp_low:.1}), bp {bp:.3}, notch {notch:.1} (far {notch_far:.3}), peak {peak:.3}, low shelf {low_dc:.2}/{low_hi:.3}, high shelf {hi_top:.2}/{hi_low:.3}"
        );
        assert!((lp + 3.01).abs() < 0.05 && lp_far < -30.0);
        assert!((hp + 3.01).abs() < 0.05 && hp_low < -30.0);
        assert!(bp.abs() < 0.01);
        assert!(notch < -60.0 && notch_far.abs() < 0.1);
        assert!((peak - 6.0).abs() < 0.01);
        assert!((low_dc - 6.0).abs() < 0.05 && low_hi.abs() < 0.05);
        assert!((hi_top + 9.0).abs() < 0.05 && hi_low.abs() < 0.05);
    }

    #[test]
    fn biquad_tick_matches_its_magnitude() {
        for (mode, hz, probe) in [
            (FilterMode::Lp, 2000.0, 4000.0),
            (FilterMode::Hp, 2000.0, 1000.0),
            (FilterMode::Peak, 1000.0, 1000.0),
            (FilterMode::HighShelf, 3000.0, 9000.0),
        ] {
            let mut b = Biquad::new();
            b.set(mode, hz, 1.2, 7.0, SR);
            let y: Vec<f32> = sine(probe, 24_000).iter().map(|v| b.tick(*v)).collect();
            let measured = rms(&y[12_000..]) * std::f32::consts::SQRT_2;
            let predicted = b.magnitude(probe, SR);
            println!("{mode:?} at {probe} Hz: tick {measured:.4}, magnitude {predicted:.4}");
            assert!((measured / predicted - 1.0).abs() < 0.002);
        }
    }

    #[test]
    fn a_biquad_cascade_is_steeper() {
        let x = sine(4000.0, 24_000);
        let one = run_biquad(
            FilterMode::Lp,
            1000.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            &x,
        );
        let mut b = [Biquad::new(), Biquad::new()];
        for s in &mut b {
            s.set(
                FilterMode::Lp,
                1000.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                SR,
            );
        }
        let two: Vec<f32> = x
            .iter()
            .map(|v| {
                let first = b[0].tick(*v);
                b[1].tick(first)
            })
            .collect();
        let (a, c) = (db(rms(&one[12_000..])), db(rms(&two[12_000..])));
        println!("4 kHz through lp 1 kHz: one {a:.1} dB, two {c:.1} dB");
        // (rms is 3 dB under the peak gain.)
        assert!(((c + 3.01) - 2.0 * (a + 3.01)).abs() < 0.5);
    }

    #[test]
    fn biquads_stay_finite_at_the_extremes() {
        let noise: Vec<f32> = {
            let mut r = Rng::new(7);
            (0..20_000)
                .map(|_| (r.next_u64() >> 40) as f32 / (1u64 << 23) as f32 - 1.0)
                .collect()
        };
        for mode in [
            FilterMode::Lp,
            FilterMode::Hp,
            FilterMode::Bp,
            FilterMode::Notch,
            FilterMode::Peak,
            FilterMode::LowShelf,
            FilterMode::HighShelf,
        ] {
            for (hz, q, g) in [
                (0.0, 0.0, -100.0),
                (1.0, 1000.0, 60.0),
                (23_999.0, 1000.0, -60.0),
                (1e9, 1e9, 1e9),
                (f32::NAN, f32::NAN, f32::NAN),
                (20.0, 0.025, 60.0),
            ] {
                let y = run_biquad(mode, hz, q, g, &noise);
                assert!(y.iter().all(|v| v.is_finite()), "{mode:?} {hz} {q} {g}");
            }
        }
    }

    /// Energy off the harmonic grid of `f0`, relative to on it, in dB.
    fn alias_db_to(x: &[f32], f0: f32, top: f32) -> f32 {
        let p = power(x);
        let n = x.len();
        let bin_hz = SR / n as f32;
        let (mut on, mut off) = (0.0f64, 0.0f64);
        for (k, v) in p.iter().enumerate().skip(4) {
            if k as f32 * bin_hz > top {
                break;
            }
            let h = (k as f32 * bin_hz / f0).round();
            let near = h >= 1.0 && (k as f32 * bin_hz - h * f0).abs() <= 4.0 * bin_hz;
            if near {
                on += f64::from(*v);
            } else {
                off += f64::from(*v);
            }
        }
        (10.0 * (off / on).log10()) as f32
    }

    /// Off-grid energy (re on-grid) of a hard-clipped 5 kHz sine through an
    /// oversampler of `factor`, below `top` Hz.
    fn clip_alias(factor: usize, top: f32) -> f32 {
        let x = sine(5000.0, 1 << 15);
        let clip = |v: f32| (v * 8.0).clamp(-1.0, 1.0);
        let mut os = Oversampler::new(factor).unwrap();
        let y: Vec<f32> = x.iter().map(|v| os.process(*v, clip)).collect();
        alias_db_to(&y[2048..], 5000.0, top)
    }

    #[test]
    fn oversampling_cuts_aliasing_of_a_clipped_sine() {
        let base = clip_alias(1, 20_000.0);
        let mut got = Vec::new();
        for (factor, need) in [(2usize, 12.0f32), (4, 22.0), (8, 40.0)] {
            let less = base - clip_alias(factor, 20_000.0);
            let full = clip_alias(1, 24_000.0) - clip_alias(factor, 24_000.0);
            println!(
                "{factor}x: {less:.1} dB less off-grid energy below 20 kHz ({full:.1} dB over the whole band)"
            );
            got.push(less);
            assert!(less >= need, "{factor}x: only {less:.1} dB less");
        }
        assert!(got[0] < got[1] && got[1] < got[2]);
    }

    #[test]
    fn the_oversampler_is_a_clean_delay_when_f_is_identity() {
        for factor in [1usize, 2, 4, 8] {
            let mut os = Oversampler::new(factor).unwrap();
            let lat = os.latency();
            let x = sine(1000.0, 8000);
            let y: Vec<f32> = x.iter().map(|v| os.process(*v, |s| s)).collect();
            // The same sine, `lat` samples later: compare with a fractional shift.
            let shifted: Vec<f32> = (0..8000)
                .map(|i| {
                    let t = (i as f64 - f64::from(lat)) / f64::from(SR);
                    (std::f64::consts::TAU * 1000.0 * t).sin() as f32
                })
                .collect();
            let err: Vec<f32> = y[1000..]
                .iter()
                .zip(&shifted[1000..])
                .map(|(a, b)| a - b)
                .collect();
            let e = db(rms(&err));
            println!("{factor}x: latency {lat:.2} samples, identity error {e:.1} dB");
            assert!(e < -60.0, "{factor}x: error {e} dB");
        }
    }

    #[test]
    fn the_oversampler_refuses_odd_factors() {
        let e = Oversampler::new(3).unwrap_err();
        assert!(e.contains("give 1, 2, 4 or 8"), "{e}");
    }

    #[test]
    fn hilbert_is_in_quadrature_across_the_band() {
        // Analytic response of the two chains at 48 kHz.
        let phase = |hz: f64| {
            let w = std::f64::consts::TAU * hz / f64::from(SR);
            let z2 = num(-2.0 * w);
            let chain = |cs: &[f64]| {
                cs.iter().fold((1.0, 0.0), |acc, c| {
                    let nu = (c - z2.0, -z2.1);
                    let de = (1.0 - c * z2.0, -c * z2.1);
                    mul(acc, div(nu, de))
                })
            };
            let a = chain(&HILBERT_A);
            let b = mul(chain(&HILBERT_B), num(-w));
            let r = div(a, b);
            (r.1.atan2(r.0).to_degrees(), (r.0 * r.0 + r.1 * r.1).sqrt())
        };
        fn num(a: f64) -> (f64, f64) {
            (a.cos(), a.sin())
        }
        fn mul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
            (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
        }
        fn div(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
            let d = b.0 * b.0 + b.1 * b.1;
            ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
        }
        let mut worst = 0.0f64;
        for k in 0..=2000 {
            let hz = 20.0 * (1000.0f64).powf(f64::from(k) / 2000.0);
            let (deg, mag) = phase(hz);
            worst = worst.max((deg - 90.0).abs());
            assert!((mag - 1.0).abs() < 1e-9);
        }
        println!("hilbert: worst phase error {worst:.3} degrees over 20 Hz - 20 kHz");
        assert!(worst < 1.0, "{worst}");
    }

    #[test]
    fn hilbert_ticks_a_sine_into_a_sine_and_a_cosine() {
        for hz in [20.0f32, 100.0, 1000.0, 7000.0, 15_000.0, 20_000.0] {
            let n = 96_000;
            let x = sine(hz, n);
            let mut h = Hilbert::new();
            let (mut ci, mut si, mut cq, mut sq) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for (k, v) in x.iter().enumerate() {
                let (i, q) = h.tick(*v);
                if k < n / 2 {
                    continue;
                }
                let w = std::f64::consts::TAU * f64::from(hz) * k as f64 / f64::from(SR);
                ci += f64::from(i) * w.cos();
                si += f64::from(i) * w.sin();
                cq += f64::from(q) * w.cos();
                sq += f64::from(q) * w.sin();
            }
            // Phase of each output against sin(wt): atan2(cos part, sin part).
            let pi = ci.atan2(si).to_degrees();
            let pq = cq.atan2(sq).to_degrees();
            let diff = (pi - pq).rem_euclid(360.0);
            let ratio = (cq.hypot(sq) / ci.hypot(si)).abs();
            println!("hilbert {hz} Hz: i - q phase {diff:.2} degrees, |q|/|i| {ratio:.4}");
            assert!((diff - 90.0).abs() < 1.0, "{hz}: {diff}");
            assert!((ratio - 1.0).abs() < 0.01, "{hz}: {ratio}");
        }
    }

    #[test]
    fn lagrange_reads_exact_integers_and_keeps_the_highs() {
        let mut d = DelayLine::new(64);
        let x = sine(9600.0, 400); // a fifth of the sample rate
        for v in &x[..200] {
            d.write(*v);
        }
        // Integer delay: the sample itself.
        assert!((d.read_lagrange3(10.0) - x[199 - 10]).abs() < 1e-6);
        let (mut e_l, mut e_h, mut e_lin) = (0.0f32, 0.0f32, 0.0f32);
        for k in 200..400 {
            // Delay 10.5 reads 10.5 samples before the last write (sample k - 1).
            let want = (std::f64::consts::TAU * 9600.0 * ((k - 1) as f64 - 10.5) / f64::from(SR))
                .sin() as f32;
            let lin = 0.5 * (d.read(10.0) + d.read(11.0));
            e_l = e_l.max((d.read_lagrange3(10.5) - want).abs());
            e_h = e_h.max((d.read(10.5) - want).abs());
            e_lin = e_lin.max((lin - want).abs());
            d.write(x[k]);
        }
        println!(
            "9.6 kHz at delay 10.5: max error lagrange3 {e_l:.4}, hermite {e_h:.4}, linear {e_lin:.4}"
        );
        assert!(e_l < 0.06 && e_l < e_lin / 3.0);
        // Out-of-range and non-finite delays are safe.
        for dd in [-5.0, 0.3, 1e9, f32::NAN, f32::INFINITY] {
            assert!(d.read_lagrange3(dd).is_finite());
        }
    }

    #[test]
    fn smooth_reaches_63_percent_in_one_time_constant() {
        let mut s = Smooth::new(0.0);
        let mut v = 0.0;
        for _ in 0..4800 {
            v = s.tick(1.0, 0.1, SR);
        }
        println!("smooth after one time constant: {v:.4}");
        assert!((v - 0.632).abs() < 0.01);
        assert_eq!(Smooth::new(0.0).tick(5.0, 0.0, SR), 5.0);
        let mut s = Smooth::new(2.0);
        assert_eq!(s.tick(f32::NAN, 0.1, SR), 2.0);
        for _ in 0..200_000 {
            v = s.tick(1.0, 0.01, SR);
        }
        assert!((v - 1.0).abs() < 1e-6);
    }
}
