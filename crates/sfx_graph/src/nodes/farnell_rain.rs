//! `rain`, from Farnell's RAIN patches (*Designing Sound*): `surface` leaf
//! is rain-on-leaf.pd, water is rainonwater.pd (raindrops.pd triggering a
//! rising-pitch bubble voice), window is rainwindow.pd (raindrops.pd through
//! glasswindow.pd) and ground is rain1.pd (three cpraingen.pd + cpulse.pd).
//! See `docs/nodes/farnell_rain.md`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::one;
use crate::buf::Buf;
use crate::dsp::Rng;
use crate::node::{Cx, Describe, Eval, Outs};

// The Pd filters, copied from `farnell.rs` (they are private there).

/// Pd's `bp~` (centre Hz, Q).
#[derive(Default, Clone)]
struct PdBp {
    c1: f32,
    c2: f32,
    gain: f32,
    x1: f32,
    x2: f32,
}

impl PdBp {
    // Pd's d_filter.c writes pi as 3.14159; keep it so the coefficients
    // match Pd's bit for bit.
    #[allow(clippy::approx_constant)]
    fn new(f: f32, q: f32, sr: f32) -> PdBp {
        // Pd's polynomial cosine, which is 0 above a quarter of the rate.
        let qcos = |f: f32| {
            if (-(0.5 * 3.14159)..=0.5 * 3.14159).contains(&f) {
                let g = f * f;
                ((g * g * g * (-1.0 / 720.0) + g * g * (1.0 / 24.0)) - g * 0.5) + 1.0
            } else {
                0.0
            }
        };
        let f = if f < 0.001 { 10.0 } else { f };
        let omega = f * (2.0 * 3.14159) / sr;
        let mut omr = if q < 0.001 { 1.0 } else { omega / q };
        if omr > 1.0 {
            omr = 1.0;
        }
        let r = 1.0 - omr;
        PdBp {
            c1: 2.0 * qcos(omega) * r,
            c2: -r * r,
            gain: 2.0 * omr * (omr + r * omega),
            x1: 0.0,
            x2: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let o = x + self.c1 * self.x1 + self.c2 * self.x2;
        self.x2 = self.x1;
        self.x1 = o;
        self.gain * o
    }
}

/// Pd's `hip~` before 0.44 (no gain normalisation).
#[derive(Default, Clone)]
struct PdHip {
    coef: f32,
    last: f32,
}

impl PdHip {
    fn new(f: f32, sr: f32) -> PdHip {
        PdHip {
            coef: (1.0 - f * TAU / sr).clamp(0.0, 1.0),
            last: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        if self.coef >= 1.0 {
            return x;
        }
        let new = x + self.coef * self.last;
        let out = new - self.last;
        self.last = new;
        out
    }
}

/// Pd's `lop~`.
#[derive(Default, Clone)]
struct PdLop {
    k: f32,
    y: f32,
}

impl PdLop {
    fn new(f: f32, sr: f32) -> PdLop {
        PdLop {
            k: (f * TAU / sr).clamp(0.0, 1.0),
            y: 0.0,
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// What the drops land on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RainSurface {
    /// rain-on-leaf.pd: sparse soft ticks, high-passed at 500 Hz twice.
    #[default]
    Leaf,
    /// rainonwater.pd: every drop starts a rising-pitch bubble.
    Water,
    /// rainwindow.pd: drop spikes ring a feedback network of ringmodulated
    /// bands near 2 kHz (glasswindow.pd).
    Window,
    /// rain1.pd: three random streams of 0.3 ms clicks, high-passed at 900 Hz.
    Ground,
}

/// Rain drops on a surface.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rain {
    /// `leaf`, `water`, `window` or `ground` (default `leaf`).
    #[serde(default)]
    pub surface: RainSurface,
    /// Drops a second, 0.1 to 500. Unset: the patch's own rate (measured in
    /// docs/nodes/farnell_rain.md). `leaf`, `water` and `window` move the
    /// patch's threshold until that many drops a second pass it, so about 80
    /// a second is the most they reach; `ground` scales its random delays.
    #[serde(default)]
    pub intensity: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
    /// Render a buffer that wraps with no seam. Unset: yes in a `kind: loop`
    /// document, no otherwise.
    #[serde(default, rename = "loop")]
    pub looped: Option<bool>,
}

impl Describe for Rain {
    const BLURB: &'static str = "rain (Farnell's RAIN patches): drops on a surface, leaf / water (rising bubbles) / window (ringing glass) / ground (clicks); intensity = drops a second; loop: true wraps without a seam";
    const WORDS: &'static [&'static str] = &["surface"];
    fn example() -> serde_json::Value {
        serde_json::json!({"surface": "leaf", "intensity": 8, "seed": 1})
    }
}

/// raindrops.pd's gaussianoise 0.4: Box-Muller with the radius drawn from
/// 0.6..1 (so it never exceeds 1.011).
fn gauss(rng: &mut Rng) -> f32 {
    let r = 0.6 + 0.4 * rng.signed().abs();
    (-2.0 * r.ln()).max(0.0).sqrt() * (TAU * rng.signed()).cos()
}

/// raindrops.pd up to its `max~`: gaussian noise through `bp~ centre 0.4`
/// and `lop~ 500` (L); a sine at 40 + 80 L Hz times 10 L^2.
fn drop_signal(rng: &mut Rng, n: usize, sr: f32, centre: f32, wrap: bool) -> Vec<f32> {
    let noise: Vec<f32> = (0..n).map(|_| gauss(rng)).collect();
    let mut bp = PdBp::new(centre, 0.4, sr);
    let mut lop = PdLop::new(500.0, sr);
    let mut l = vec![0.0f32; n];
    for _ in 0..if wrap { 2 } else { 1 } {
        for (o, &g) in l.iter_mut().zip(&noise) {
            *o = lop.tick(bp.tick(g));
        }
    }
    let mut phase = 0.0f64;
    l.iter()
        .map(|&v| {
            let x = (TAU * phase as f32).sin() * 10.0 * v * v;
            phase = (phase + f64::from((40.0 + 80.0 * v) / sr)).rem_euclid(1.0);
            x
        })
        .collect()
}

/// How many separate runs of `x` stay above `level`.
fn runs_above(x: &[f32], level: f32) -> usize {
    let mut count = 0;
    let mut up = false;
    for &v in x {
        if v > level && !up {
            count += 1;
        }
        up = v > level;
    }
    count
}

/// The `max~` threshold at which `x` has `target` drops a second (a drop
/// is a run above `thr + margin`); `thr0` when no target is given.
fn threshold(x: &[f32], thr0: f32, margin: f32, target: Option<f32>, secs: f32) -> f32 {
    let Some(target) = target else { return thr0 };
    let top = x.iter().cloned().fold(0.0f32, f32::max);
    let (mut lo, mut hi) = (0.0f32, top);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if runs_above(x, mid + margin) as f32 / secs > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// rainonwater.pd's voice: one bubble at a time, restarted by each drop.
/// A random r in 0..999 sets the length (r / 7 ms), the pitch rise (2 + r /
/// 100, as rise ^ ramp: Pd-extended's `pow~` is right ^ left), the base
/// (400 + r / 5 Hz) and the level (r / 1000); the ramp reaches the pitch 3
/// ms late, the envelope is (1 - ramp)^4 through `lop~ 12`, the sum goes
/// through `lop~ 1000` and 0.1.
fn bubbles(events: &[usize], rng: &mut Rng, n: usize, sr: f32, wrap: bool) -> Vec<f32> {
    let hist_n = ((0.003 * sr).round() as usize).max(1);
    let mut hist = vec![0.0f32; hist_n];
    let mut env_lop = PdLop::new(12.0, sr);
    let mut out_lop = PdLop::new(1000.0, sr);
    let (mut since, mut len, mut base, mut rise, mut gain) =
        (f32::MAX, 0.0f32, 0.0f32, 2.0f32, 0.0f32);
    let mut phase = 0.0f64;
    // One draw per drop, the same in both passes.
    let draws: Vec<f32> = events
        .iter()
        .map(|_| (rng.unit() * 1000.0).floor())
        .collect();
    let mut out = vec![0.0f32; n];
    for _ in 0..if wrap { 2 } else { 1 } {
        let mut next = 0;
        for (i, o) in out.iter_mut().enumerate() {
            while next < events.len() && events[next] == i {
                let r = draws[next];
                since = 0.0;
                len = r / 7.0 * 0.001 * sr;
                base = 400.0 + r / 5.0;
                rise = 2.0 + r / 100.0;
                gain = r / 1000.0;
                next += 1;
            }
            let v = if len <= 0.0 {
                1.0
            } else {
                (since / len).min(1.0)
            };
            since += 1.0;
            let slot = i % hist_n;
            let vd = hist[slot];
            hist[slot] = v;
            let env = env_lop.tick((1.0 - v).powi(4));
            let f = base * rise.powf(vd);
            phase = (phase + f64::from(f / sr)).rem_euclid(1.0);
            *o = 0.1 * out_lop.tick(env * (TAU * phase as f32).cos() * gain);
        }
    }
    out
}

/// glasswindow.pd: two delay lines (3.7 and 4.2 ms) each read twice at
/// 0.61; each read goes through `bp~` (Q 2.3, centres 2007 / 1994 / 1986 /
/// 1969 Hz) and a ringmodulating `osc~` (254 / 669 / 443 / 551 Hz) back into
/// one of the lines, so the lines feed each other; the output is the two
/// reads.
fn glass(input: &[f32], sr: f32, wrap: bool) -> Vec<f32> {
    let ring = 2048usize;
    let (mut la, mut lb) = (vec![0.0f32; ring], vec![0.0f32; ring]);
    let (da, db) = (
        ((0.0037 * sr).round() as usize).clamp(1, ring - 1),
        ((0.0042 * sr).round() as usize).clamp(1, ring - 1),
    );
    let mut bp = [2007.0f32, 1994.0, 1986.0, 1969.0].map(|f| PdBp::new(f, 2.3, sr));
    let freqs = [254.0f32, 669.0, 443.0, 551.0];
    let mut ph = [0.0f64; 4];
    let mut out = vec![0.0f32; input.len()];
    let mut t = 0usize;
    for _ in 0..if wrap { 2 } else { 1 } {
        for (o, &x) in out.iter_mut().zip(input) {
            let a = 0.61 * la[(t + ring - da) % ring];
            let b = 0.61 * lb[(t + ring - db) % ring];
            let mut ringmod = [0.0f32; 4];
            for (k, src) in [a, a, b, b].into_iter().enumerate() {
                ph[k] = (ph[k] + f64::from(freqs[k] / sr)).rem_euclid(1.0);
                ringmod[k] = bp[k].tick(src) * (TAU * ph[k] as f32).cos();
            }
            // Boxes 8 and 10 throw into b, 9 and 11 into a.
            lb[t % ring] = x + ringmod[0] + ringmod[2];
            la[t % ring] = x + ringmod[1] + ringmod[3];
            *o = a + b;
            t += 1;
        }
    }
    out
}

/// The three cpraingen.pd generators of rain1.pd: (range, spread, base).
const GENS: [(u32, u32, f32); 3] = [(60, 12, 0.2), (50, 12, 0.15), (70, 12, 0.4)];

/// cpraingen's wait in ms before the next drop for a draw r (the patch's
/// `+ 2.718` kept as written).
#[allow(clippy::approx_constant)]
fn wait_ms(r: u32) -> f32 {
    40.0 + ((r as f32 + 7.0) / 15.0 + 2.718).exp()
}

/// A generator's mean drops a second.
fn natural_rate(range: u32) -> f32 {
    let mean = (0..range).map(wait_ms).sum::<f32>() / range as f32;
    1000.0 / mean
}

/// rain1.pd: three cpraingen, each a random wait then cpulse (a parabolic
/// hump of 0.15..0.51 ms, height sqrt(length + 0.1)); the sum x 0.2 through
/// `hip~ 900`. `scale` stretches the waits.
fn ground(rng: &mut Rng, n: usize, sr: f32, scale: f32, wrap: bool) -> Vec<f32> {
    let mut out = vec![0.0f32; n];
    for (range, spread, base) in GENS {
        let draw = |rng: &mut Rng| wait_ms((rng.unit() * range as f32) as u32) * scale;
        // Start part-way into the first wait so the stream is stationary.
        let first = draw(rng);
        let mut t = f64::from(rng.unit() * first);
        while t < f64::from(n as f32 / sr * 1000.0) {
            let len_ms = base + (rng.unit() * spread as f32).floor() / 100.0;
            let len = (len_ms * 0.001 * sr).max(1.0);
            let amp = (len_ms + 0.1).sqrt() * 0.2;
            let start = (t * 0.001 * f64::from(sr)) as usize;
            for j in 0..len.ceil() as usize {
                let v = 1.0 - j as f32 / len;
                let at = start + j;
                let y = amp * (1.0 - 4.0 * (v - 0.5) * (v - 0.5));
                if at < n {
                    out[at] += y;
                } else if wrap {
                    out[at % n] += y;
                }
            }
            t += f64::from(draw(rng));
        }
    }
    let mut hip = PdHip::new(900.0, sr);
    for _ in 0..if wrap { 2 } else { 1 } {
        for v in out.iter_mut() {
            *v = hip.tick(*v);
        }
    }
    out
}

impl Eval for Rain {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if let Some(r) = self.intensity
            && !(r.is_finite() && (0.1..=500.0).contains(&r))
        {
            return Err(format!(
                "intensity: {r}: give drops a second from 0.1 to 500"
            ));
        }
        let (n, sr) = (cx.n, cx.sr);
        let secs = n as f32 / sr;
        let wrap = self.looped.unwrap_or(cx.looped);
        let mut rng = cx.rng(self.seed);
        let out = match self.surface {
            RainSurface::Leaf => {
                // rain-on-leaf.pd: raindrops.pd with bp~ 50, max~ 0.35.
                let x = drop_signal(&mut rng, n, sr, 50.0, wrap);
                let thr = threshold(&x, 0.35, 0.0, self.intensity, secs);
                let (mut h1, mut h2) = (PdHip::new(500.0, sr), PdHip::new(500.0, sr));
                let y: Vec<f32> = x.iter().map(|&v| 0.5 * (v.max(thr) - thr)).collect();
                let mut out = vec![0.0; n];
                for _ in 0..if wrap { 2 } else { 1 } {
                    for (o, &v) in out.iter_mut().zip(&y) {
                        *o = h2.tick(h1.tick(v));
                    }
                }
                out
            }
            RainSurface::Water => {
                // rainonwater.pd: raindrops 20 0.1, threshold~ 0.01 on its
                // output (so a drop is a run of x above thr + 0.02), 1 ms
                // debounce.
                let x = drop_signal(&mut rng, n, sr, 20.0, wrap);
                let thr = threshold(&x, 0.1, 0.02, self.intensity, secs);
                let gap = (0.001 * sr) as usize;
                let mut events: Vec<usize> = Vec::new();
                let mut up = false;
                for (i, &v) in x.iter().enumerate() {
                    let hi = 0.5 * (v.max(thr) - thr) > 0.01;
                    if hi && !up && events.last().is_none_or(|&l| i - l > gap) {
                        events.push(i);
                    }
                    up = hi;
                }
                bubbles(&events, &mut rng, n, sr, wrap)
            }
            RainSurface::Window => {
                // rainwindow.pd: raindrops 70 0.2, hip~ 9000, x 10, glasswindow.
                let x = drop_signal(&mut rng, n, sr, 70.0, wrap);
                let thr = threshold(&x, 0.2, 0.0, self.intensity, secs);
                let mut hip = PdHip::new(9000.0, sr);
                let y: Vec<f32> = x
                    .iter()
                    .map(|&v| 10.0 * hip.tick(0.5 * (v.max(thr) - thr)))
                    .collect();
                glass(&y, sr, wrap)
            }
            RainSurface::Ground => {
                let natural: f32 = GENS.iter().map(|g| natural_rate(g.0)).sum();
                let scale = self.intensity.map_or(1.0, |r| natural / r);
                ground(&mut rng, n, sr, scale, wrap)
            }
        };
        Ok(one(Buf::Audio(out)))
    }
}
