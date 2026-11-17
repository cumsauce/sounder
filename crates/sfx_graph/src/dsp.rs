//! Per-sample DSP state: each piece is a small struct with a `tick`, so the
//! block evaluator loops over it and the phase 3 feedback evaluator (a cycle
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
}

impl Osc {
    pub fn new(wave: Wave, phase: f32) -> Osc {
        Osc {
            wave,
            phase: f64::from(phase.rem_euclid(1.0)),
            tri: 0.0,
        }
    }

    /// The next sample at `hz` (negative runs backwards) with pulse `width`.
    #[inline]
    pub fn tick(&mut self, hz: f32, width: f32, sr: f32) -> f32 {
        let dt = (hz / sr).clamp(-0.5, 0.5);
        let t = self.phase as f32;
        let adt = dt.abs();
        let out = match self.wave {
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
        };
        self.phase = (self.phase + f64::from(dt)).rem_euclid(1.0);
        out
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
