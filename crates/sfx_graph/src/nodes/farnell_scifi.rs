//! Sci-fi nodes ported from the Pure Data patches of Andy Farnell's
//! *Designing Sound* (MIT Press): `droid` is R2D2/droids3.pd (the FM core is
//! droids2.pd), `transporter` SCIFI/transporter-TNG.pd, `torpedo`
//! SCIFI/photon-torpedo.pd, `phaser_beam` SCIFI/tos-phaser.pd. The patches
//! keep the book's Pd conventions (see `docs/porting-farnell.md`): `osc~`
//! and `cos~` are cosines taking cycles, `pow~` is right ^ left, `bp~` takes
//! centre and Q, `hip~` is the pre-0.44 one-pole. The patches run until
//! stopped, so each node takes a `length`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::{mono, one};
use crate::dsp::{DelayLine, Rng};
use crate::node::{Cx, Describe, Eval, Outs};

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

/// Pd's `line~`: `set(target, seconds)` ramps from where it is now.
#[derive(Default, Clone, Copy)]
struct Line {
    cur: f32,
    step: f32,
    left: u32,
}

impl Line {
    fn set(&mut self, target: f32, secs: f32, sr: f32) {
        let n = (secs * sr).round() as u32;
        if n == 0 {
            self.cur = target;
            self.left = 0;
        } else {
            self.step = (target - self.cur) / n as f32;
            self.left = n;
        }
    }
    fn tick(&mut self) -> f32 {
        if self.left > 0 {
            self.cur += self.step;
            self.left -= 1;
        }
        self.cur
    }
}

fn range(field: &str, x: f32, lo: f32, hi: f32, what: &str) -> Result<f32, String> {
    if x.is_finite() && (lo..=hi).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{field}: {x}: give {what} from {lo} to {hi}"))
    }
}

/// 1 inside `len` samples, 3 ms fade in, `fade` samples fade out, 0 after.
fn gate(i: usize, len: usize, fade: usize, sr: f32) -> f32 {
    if i >= len {
        return 0.0;
    }
    let fin = (i as f32 / (0.003 * sr)).min(1.0);
    let fout = ((len - i) as f32 / fade.max(1) as f32).min(1.0);
    fin * fout
}

// ------------------------------------------------------------------- droid

/// A chattering robot (R2D2/droids3.pd, the FM core of droids2.pd). A metro
/// ticks every `period` ms (halved one tick in three); each tick re-rolls,
/// with its own chance, the phasor's rate (0 to 0.6 Hz, times the carrier
/// factor 100 to 9100 cycles per phasor cycle: a carrier up to about 5 kHz),
/// the modulator (20 to 820 times the rate, Hz), the index (0.2 to 0.7
/// cycles) and the amplitude (0 or 1). Each change either jumps or glides
/// for one period (random 2), which is the babble: chirps, slides and rests.
/// The FM is `cos~(K phasor + index cos(2 pi mod))`, low-passed at 10 kHz and
/// high-passed at 100 Hz (each twice), times 0.06.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Droid {
    /// How fast it talks, 0 to 1 (default 0.9): the tick period is 100 +
    /// 600 (1 - chatter) ms (the patch's slider sits at 138 ms, 0.94).
    #[serde(default)]
    pub chatter: Option<f32>,
    /// Pitch factor, 0.25 to 4 (default 1): scales every frequency.
    #[serde(default)]
    pub pitch: Option<f32>,
    /// 0 to 1 (default 0.5, the patch's odds and spans): how often the
    /// pitch, modulator and index jump and how wide the index goes. 0 is a
    /// plain sine that rarely moves; 1 changes everything nearly every tick.
    #[serde(default)]
    pub mood: Option<f32>,
    /// Seconds of babble, 0.1 to 60 (default 2); the patch never stops.
    #[serde(default)]
    pub length: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Droid {
    const BLURB: &'static str = "a chattering robot (Farnell R2D2): random FM chirps, slides and rests on a metro; chatter = speed, pitch = scale, mood = how much changes; seeded";
    fn example() -> serde_json::Value {
        serde_json::json!({"chatter": 0.9, "pitch": 1, "mood": 0.5, "length": 2})
    }
}

impl Eval for Droid {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let chatter = range("chatter", self.chatter.unwrap_or(0.9), 0.0, 1.0, "0")?;
        let pitch = range("pitch", self.pitch.unwrap_or(1.0), 0.25, 4.0, "a factor")?;
        let mood = range("mood", self.mood.unwrap_or(0.5), 0.0, 1.0, "0")?;
        let length = range("length", self.length.unwrap_or(2.0), 0.1, 60.0, "seconds")?;
        let sr = cx.sr;
        let period = 0.1 + 0.6 * (1.0 - chatter);
        let (p_carrier, p_side) = (2.0 / 3.0 * mood, 0.5 * mood);
        let mut rng = cx.rng(self.seed);
        let mut interval = period;
        let mut next = 0.0f64;
        let (mut rate, mut carrier, mut amp, mut index, mut modf) = (
            Line::default(),
            Line::default(),
            Line::default(),
            Line::default(),
            Line::default(),
        );
        let (mut ph, mut pm) = (0.0f64, 0.0f64);
        let mut lop = [PdLop::new(10000.0, sr), PdLop::new(10000.0, sr)];
        let mut hip = [PdHip::new(100.0, sr), PdHip::new(100.0, sr)];
        let len_n = (length * sr) as usize;
        let fade = (0.02 * sr) as usize;
        Ok(one(mono(cx.n, |i| {
            while (i as f64) >= next {
                next += f64::from(interval * sr);
                // random 2: 0 jumps, 1 glides for one period.
                let glide = |rng: &mut Rng| {
                    if rng.unit() < 0.5 { period } else { 0.0 }
                };
                let t = glide(&mut rng);
                rate.set((rng.unit() * 1000.0).floor() / 1000.0, t, sr);
                if rng.unit() < p_carrier {
                    let t = glide(&mut rng);
                    carrier.set((rng.unit() * 9000.0).floor(), t, sr);
                }
                if rng.unit() < 0.5 {
                    let on = if rng.unit() < 0.5 { 1.0 } else { 0.0 };
                    let t = glide(&mut rng);
                    amp.set(on, t + 0.003, sr);
                }
                if rng.unit() < p_side {
                    let t = glide(&mut rng);
                    index.set((rng.unit() * 100.0).floor() / 200.0 * 2.0 * mood, t, sr);
                }
                if rng.unit() < p_side {
                    let t = glide(&mut rng);
                    modf.set((rng.unit() * 800.0).floor(), t, sr);
                }
                if rng.unit() < 1.0 / 3.0 {
                    interval = period / if rng.unit() < 0.5 { 1.0 } else { 2.0 };
                }
            }
            let f = 0.6 * pitch * rate.tick();
            let k = 100.0 + carrier.tick();
            let fm = (20.0 + modf.tick()) * f;
            let idx = 0.2 + index.tick();
            let a = amp.tick();
            ph = (ph + f64::from(f / sr)).rem_euclid(1.0);
            pm = (pm + f64::from(fm / sr)).rem_euclid(1.0);
            let arg = f64::from(k) * ph + f64::from(idx) * (std::f64::consts::TAU * pm).cos();
            let mut y = (std::f64::consts::TAU * arg).cos() as f32 * a;
            for l in &mut lop {
                y = l.tick(y);
            }
            for h in &mut hip {
                y = h.tick(y);
            }
            0.06 * y * gate(i, len_n, fade, sr)
        })))
    }
}

// ------------------------------------------------------------- transporter

/// A small Schroeder reverb standing in for Pd's `rev3~ 100 95 5000 1`
/// (four damped combs and two allpasses, about 3 s).
struct Verb {
    combs: Vec<(Vec<f32>, usize, f32)>,
    aps: Vec<(Vec<f32>, usize)>,
    damp: f32,
}

impl Verb {
    fn new(sr: f32) -> Verb {
        let n = |ms: f32| ((ms * 0.001 * sr) as usize).max(2);
        Verb {
            combs: [29.7, 37.1, 41.1, 43.7]
                .iter()
                .map(|&ms| (vec![0.0; n(ms)], 0, 0.0))
                .collect(),
            aps: [5.0, 1.7].iter().map(|&ms| (vec![0.0; n(ms)], 0)).collect(),
            damp: (5000.0 * TAU / sr).clamp(0.0, 1.0),
        }
    }
    fn tick(&mut self, x: f32) -> f32 {
        let mut y = 0.0;
        for (buf, w, lp) in &mut self.combs {
            let out = buf[*w];
            *lp += self.damp * (out - *lp);
            buf[*w] = x + 0.93 * *lp;
            *w = (*w + 1) % buf.len();
            y += 0.25 * out;
        }
        for (buf, w) in &mut self.aps {
            let d = buf[*w];
            let v = y + 0.5 * d;
            buf[*w] = v;
            *w = (*w + 1) % buf.len();
            y = d - 0.5 * v;
        }
        y
    }
}

/// A beam-up shimmer (SCIFI/transporter-TNG.pd). Two squares multiply: 467.1
/// Hz and a second whose frequency hops between 3527 and -2973 Hz in step.
/// That feeds a three-tap feedback comb (0.21 per tap) whose delay slides
/// from 13 / 25 / 37 ms to 1 / 13 / 25 ms over `length` under a triangle
/// swell (0 to 2 and back), high-passed at 100 Hz into twelve very narrow
/// band-passes (Q 620 to 1200) on a whole-tone scale from 466 Hz, times 12,
/// into a reverb. The `activate` abstraction adds a 5 s decaying whistle
/// (a saw near 775 to 895 Hz with 30 Hz vibrato, a noise burst, clipped,
/// through three Q 13 bands) at once, direct and into the reverb.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transporter {
    /// Seconds of the swell, 1 to 30 (default 12, the patch's).
    #[serde(default)]
    pub length: Option<f32>,
    /// Scale of the twelve resonator notes, 0.5 to 2 (default 1: whole
    /// tones from 466 to 1661 Hz).
    #[serde(default)]
    pub pitch: Option<f32>,
    /// Level of the activation whistle, 0 to 2 (default 1).
    #[serde(default)]
    pub chime: Option<f32>,
    /// Level of the reverb, 0 to 2 (default 1).
    #[serde(default)]
    pub reverb: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Transporter {
    const BLURB: &'static str = "a beam-up shimmer (Farnell scifi): FSK buzz through a sliding feedback comb into twelve ringing resonators and a reverb, plus a decaying whistle; length = swell seconds";
    fn example() -> serde_json::Value {
        serde_json::json!({"length": 12, "pitch": 1, "chime": 1, "reverb": 1})
    }
}

impl Eval for Transporter {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let length = range("length", self.length.unwrap_or(12.0), 1.0, 30.0, "seconds")?;
        let pitch = range("pitch", self.pitch.unwrap_or(1.0), 0.5, 2.0, "a factor")?;
        let chime = range("chime", self.chime.unwrap_or(1.0), 0.0, 2.0, "a level")?;
        let wet = range("reverb", self.reverb.unwrap_or(1.0), 0.0, 2.0, "a level")?;
        let sr = cx.sr;
        let mut noise = cx.rng(self.seed);
        // The delay line "x" and the resonators (Q by note, high to low).
        let mut line = DelayLine::new((0.06 * sr) as usize);
        let qs = [
            1200.0, 1100.0, 930.0, 910.0, 880.0, 840.0, 800.0, 760.0, 720.0, 680.0, 640.0, 620.0,
        ];
        let mut res: Vec<PdBp> = qs
            .iter()
            .enumerate()
            .map(|(k, q)| PdBp::new(466.164 * pitch * 1.12247f32.powi(k as i32), *q, sr))
            .collect();
        let mut hip = PdHip::new(100.0, sr);
        let mut verb = Verb::new(sr);
        // Pd's bp~ passes DC and the reverb piles it up: block it.
        let mut dc = PdHip::new(20.0, sr);
        let (mut p1, mut p2) = (0.0f32, 0.0f32);
        // activate: saw, vibrato, bands.
        let (mut ps, mut pv) = (0.0f32, 0.0f32);
        let mut lp30 = PdLop::new(30.0, sr);
        let mut bands = [
            PdBp::new(775.0, 13.0, sr),
            PdBp::new(1550.0, 13.0, sr),
            PdBp::new(2325.0, 13.0, sr),
        ];
        let square = |p: f32| if p < 0.5 { -0.5 } else { 0.5 };
        Ok(one(mono(cx.n, |i| {
            let t = i as f32 / sr;
            let v = (t / length).min(1.0);
            let d1 = (1.0 + 12.0 * (1.0 - v)) * 0.001 * sr;
            let step = 0.012 * sr;
            let taps = 0.3 * (line.read(d1) + line.read(d1 + step) + line.read(d1 + 2.0 * step));
            p1 = (p1 + 467.1 / sr).fract();
            let sq1 = square(p1);
            p2 = (p2 + (277.0 + 6500.0 * sq1) / sr).rem_euclid(1.0);
            line.write(sq1 * square(p2) + 0.7 * taps);
            let swell = 4.0 * v.min(1.0 - v);
            let x = hip.tick(taps * swell);
            let shimmer = 12.0 * res.iter_mut().map(|r| r.tick(x)).sum::<f32>();
            // activate: 1 in 10 ms then to 0 over 5 s; noise 1 for 10 ms
            // then to 0 over 120 ms.
            let a = if t < 0.01 {
                t / 0.01
            } else {
                (1.0 - (t - 0.01) / 5.0).max(0.0)
            };
            let b = if t < 0.01 {
                1.0
            } else {
                (1.0 - (t - 0.01) / 0.12).max(0.0)
            };
            let f = 775.0 + 120.0 * (1.0 - a) + 40.0 * (TAU * pv).cos() + 0.1 * noise.signed();
            pv = (pv + 30.0 / sr).fract();
            ps = (ps + f / sr).fract();
            let drive = 3.0 * (lp30.tick(a) * ps + noise.signed() * b);
            let drive = drive.clamp(-1.0, 1.0);
            let act = 0.5 * bands.iter_mut().map(|b| b.tick(drive)).sum::<f32>() * chime;
            dc.tick(0.5 * act + wet * verb.tick(shimmer + 0.1 * act))
        })))
    }
}

// ----------------------------------------------------------------- torpedo

/// A photon torpedo (SCIFI/photon-torpedo.pd). An envelope jumps to 1, falls
/// to 0 over 90 ms, rises to 0.89 at 151 ms and falls to 0 at 351 ms; a
/// cosine oscillator at 2700 E^7 Hz (Pd-extended's right ^ left) times E is
/// the zap. It feeds a feedback delay (0.98, high-passed at 23 Hz, low-passed
/// at 10 kHz) whose time grows from 1.2 to 241 ms over `sweep`: the stored
/// zap slides down in pitch. The tap goes out at 0.3, and through a
/// 1200 Hz low-pass into a four-line feedback delay network (79 / 128 / 207
/// / 49 ms, a Hadamard mix, feedback 0.415).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Torpedo {
    /// Top of the zap sweep, Hz, 200 to 8000 (default 2700).
    #[serde(default)]
    pub pitch: Option<f32>,
    /// Seconds the delay takes to slide from 1.2 to 241 ms, 0.5 to 20
    /// (default 5).
    #[serde(default)]
    pub sweep: Option<f32>,
    /// Feedback of the sliding delay, 0.5 to 0.995 (default 0.98, the
    /// patch's; rings about a minute, so `length` cuts it).
    #[serde(default)]
    pub ring: Option<f32>,
    /// Level of the reverb network, 0 to 2 (default 1).
    #[serde(default)]
    pub reverb: Option<f32>,
    /// Seconds before the end fade (the last quarter, 2 s at most), 1 to
    /// 30 (default 8).
    #[serde(default)]
    pub length: Option<f32>,
}

impl Describe for Torpedo {
    const BLURB: &'static str = "a photon torpedo (Farnell scifi): a 2700 Hz to 0 zap fed into a feedback delay whose time slides 1 to 241 ms (the sound slides down), plus a reverb network; length cuts the long ring";
    fn example() -> serde_json::Value {
        serde_json::json!({"pitch": 2700, "sweep": 5, "ring": 0.98, "reverb": 1, "length": 8})
    }
}

impl Eval for Torpedo {
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let pitch = range("pitch", self.pitch.unwrap_or(2700.0), 200.0, 8000.0, "Hz")?;
        let sweep = range("sweep", self.sweep.unwrap_or(5.0), 0.5, 20.0, "seconds")?;
        let ring = range("ring", self.ring.unwrap_or(0.98), 0.5, 0.995, "a gain")?;
        let wet = range("reverb", self.reverb.unwrap_or(1.0), 0.0, 2.0, "a level")?;
        let length = range("length", self.length.unwrap_or(8.0), 1.0, 30.0, "seconds")?;
        let sr = cx.sr;
        let mut line = DelayLine::new((0.3 * sr) as usize);
        let mut hip = PdHip::new(23.0, sr);
        let mut lop = PdLop::new(10000.0, sr);
        let mut rev_in = PdLop::new(1200.0, sr);
        let ms = |t: f32| t * 0.001 * sr;
        let (na, nb, nc, nd) = (
            ms(79.0).round(),
            ms(79.0 * 1.618).round(),
            ms(79.0 * 2.618).round(),
            ms(79.0 / 1.618).round(),
        );
        let mk = || DelayLine::new((0.25 * sr) as usize);
        let (mut da, mut db, mut dc, mut dd) = (mk(), mk(), mk(), mk());
        let mut phase = 0.0f64;
        let len_n = (length * sr) as usize;
        let fade = ((length * 0.25).min(2.0) * sr) as usize;
        // The envelope of the zap: vline~ "0, 1 0 0, 0 90 0, 0.89 60 91,
        // 0 200 151".
        let env = |t: f32| {
            if t < 0.09 {
                1.0 - t / 0.09
            } else if t < 0.091 {
                0.0
            } else if t < 0.151 {
                0.89 * (t - 0.091) / 0.06
            } else if t < 0.351 {
                0.89 * (1.0 - (t - 0.151) / 0.2)
            } else {
                0.0
            }
        };
        Ok(one(mono(cx.n, |i| {
            let t = i as f32 / sr;
            let e = env(t);
            phase = (phase + f64::from(pitch * e.powi(7) / sr)).rem_euclid(1.0);
            let zap = (std::f64::consts::TAU * phase).cos() as f32 * e;
            let v = (t / sweep).min(1.0);
            let tap = hip.tick(line.read(ms(1.2 + 240.0 * v)));
            line.write(zap + ring * lop.tick(tap));
            let direct = 0.3 * tap;
            let x = rev_in.tick(direct);
            let (a, b, c, d) = (da.read(na), db.read(nb), dc.read(nc), dd.read(nd));
            let (s7, s8) = (x + a, x + b);
            let (s11, s12, s13, s14) = (s7 + s8, c + d, s7 - s8, c - d);
            let fb = 0.415;
            da.write(fb * (s11 + s12));
            db.write(fb * (s13 + s14));
            dc.write(fb * (s11 - s12));
            dd.write(fb * (s13 - s14));
            let rev = 0.5 * (s7 + s8);
            (direct + wet * rev) * gate(i, len_n, fade, sr)
        })))
    }
}

// ------------------------------------------------------------- phaser_beam

/// A handheld beam weapon (SCIFI/tos-phaser.pd). Four cosine oscillators,
/// each wobbled by a slower one (1360 +- 80 at 32 Hz and 1390 +- 200 at 28
/// Hz at 0.3; 2446 +- 443 at 47 Hz and 2586 +- 346 at 36 Hz at 0.7), plus
/// white noise high-passed at 2 kHz, through two Q 7 band-passes at 2600 and
/// 1300 Hz, clipped, high-passed at 1200 Hz, times 0.4 (the patch's "kill"
/// toggle; "stun" is 0.2).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaserBeam {
    /// Seconds the beam lasts, 0.05 to 30 (default 1); the patch holds
    /// until switched off.
    #[serde(default)]
    pub length: Option<f32>,
    /// `kill` (level 0.4, default) or `stun` (0.2, 6 dB quieter).
    #[serde(default)]
    pub mode: Option<String>,
    /// Pitch factor, 0.25 to 4 (default 1): scales every frequency.
    #[serde(default)]
    pub pitch: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for PhaserBeam {
    const BLURB: &'static str = "a hand-held beam weapon (Farnell scifi): four wobbling cosines and hissy noise through two band-passes; mode kill | stun, pitch scales, length = seconds";
    fn example() -> serde_json::Value {
        serde_json::json!({"length": 1, "mode": "kill", "pitch": 1})
    }
}

impl Eval for PhaserBeam {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let length = range("length", self.length.unwrap_or(1.0), 0.05, 30.0, "seconds")?;
        let pitch = range("pitch", self.pitch.unwrap_or(1.0), 0.25, 4.0, "a factor")?;
        let level = match self.mode.as_deref().unwrap_or("kill") {
            "kill" => 0.4,
            "stun" => 0.2,
            other => return Err(format!("mode: {other}: give kill or stun")),
        };
        let sr = cx.sr;
        let mut noise = cx.rng(self.seed);
        let mut nhip = PdHip::new(2000.0 * pitch, sr);
        let mut bp = [
            PdBp::new(2600.0 * pitch, 7.0, sr),
            PdBp::new(1300.0 * pitch, 7.0, sr),
        ];
        let mut out_hip = PdHip::new(1200.0 * pitch, sr);
        // (carrier centre, wobble depth, wobble rate, level).
        let voices = [
            (1360.0, 80.0, 32.0, 0.3),
            (1390.0, 200.0, 28.0, 0.3),
            (2446.0, 443.0, 47.0, 0.7),
            (2586.0, 346.0, 36.0, 0.7),
        ];
        let mut ph = [[0.0f64; 2]; 4];
        let len_n = (length * sr) as usize;
        let fade = (0.01 * sr) as usize;
        Ok(one(mono(cx.n, |i| {
            let mut s = 0.0f32;
            for (v, (c, depth, rate, g)) in voices.iter().enumerate() {
                let wob = (std::f64::consts::TAU * ph[v][1]).cos() as f32;
                ph[v][1] = (ph[v][1] + f64::from(rate * pitch / sr)).rem_euclid(1.0);
                let hz = (c + depth * wob) * pitch;
                ph[v][0] = (ph[v][0] + f64::from(hz / sr)).rem_euclid(1.0);
                s += g * (std::f64::consts::TAU * ph[v][0]).cos() as f32;
            }
            let x = s + nhip.tick(noise.signed());
            let y = (bp[0].tick(x) + bp[1].tick(x)).clamp(-1.0, 1.0);
            level * out_hip.tick(y) * gate(i, len_n, fade, sr)
        })))
    }
}
