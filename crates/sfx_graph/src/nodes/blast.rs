//! Blasts: `explosion` and `gunshot`, finished-recipe models in the style of
//! `thunder`. Each is a few physical fields in and tuned DSP inside, all
//! computed offline from the seed (nothing is read from another node except
//! the optional `debris` event stream).
//!
//! Shared structure (both nodes):
//!
//! * a Friedlander pressure pulse `p(t) = (1 - t/t+) exp(-t/t+)` (zero net
//!   area), the positive phase `t+` set by `size` / `caliber`;
//! * a sine thump whose pitch falls exponentially;
//! * a noise body: white noise through a three-pole low-pass whose cutoff
//!   falls with the same envelope that fades the noise;
//! * a tail: dense noise, low-passed, decaying more slowly;
//! * propagation: the whole blast arrives `distance / 343` s late (plus a
//!   fixed 2.5 ms lead so the output stage's 2 ms fade-in never eats the
//!   attack), is low-passed by air absorption (two biquad poles at
//!   `16 kHz / (1 + d / 40 m)`, never under 120 Hz) and loses level as
//!   `1 / (1 + d / 100 m)`;
//! * a final `tanh(0.7 x)` so the output is always inside +-1.
//!
//! `gunshot` adds the supersonic crack: an N-wave (a fast fall from +1 to -1
//! over a fraction of a millisecond) that rides the bullet, so it arrives at
//! `distance / (0.9 * muzzle velocity)`, ahead of the muzzle blast at
//! `distance / 343`.

use std::f32::consts::TAU;

use serde::Deserialize;

use super::one;
use crate::buf::{Buf, Event};
use crate::dsp::{Biquad, FilterMode, Svf, flush, max_hz};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// Speed of sound the delays use, m/s.
const SOUND_SPEED: f32 = 343.0;
/// Every blast starts this many seconds in, so the output stage's default
/// 2 ms fade-in does not touch the attack.
const LEAD: f32 = 0.0025;
/// Largest `distance`, metres (a 30 km blast arrives after 87 s).
const MAX_DISTANCE: f32 = 30_000.0;
/// Largest `debris` stream, events.
const MAX_DEBRIS: usize = 20_000;
/// Air low-pass distance scale of `explosion`, metres: the cutoff is
/// `16 kHz / (1 + d / this)`.
const EXPLOSION_AIR_M: f32 = 40.0;
/// The same for `gunshot`: 400 m keeps the mechanism and crack detail at
/// 50-120 m (cutoff 14.2 / 12.3 kHz; it was 7.1 / 4.0 kHz with 40 m, heard
/// as "lo-fi, no high end").
const GUNSHOT_AIR_M: f32 = 400.0;

/// Muzzle velocity of a gunshot, m/s, from the barrel length in metres: 380
/// m/s at 5 cm rising to 900 m/s for a very long barrel. `suppressed` caps
/// it at 310 (subsonic ammunition).
fn muzzle_velocity(barrel: f32, suppressed: bool) -> f32 {
    let v = 300.0 + 600.0 * (1.0 - (-barrel / 0.35).exp());
    if suppressed { v.min(310.0) } else { v }
}

/// The speed the crack is timed with: the bullet slows on the way.
fn mean_velocity(barrel: f32, suppressed: bool) -> f32 {
    0.9 * muzzle_velocity(barrel, suppressed)
}

fn friedlander(t: f32, tp: f32) -> f32 {
    let x = t / tp;
    (1.0 - x) * (-x).exp()
}

/// Three one-pole low-passes in series with a per-sample cutoff.
#[derive(Default)]
struct Lp3([f32; 3]);

impl Lp3 {
    /// Filters `x`, then scales by the noise-power compensation so band-
    /// limited noise keeps about the level of white noise at any cutoff.
    fn tick(&mut self, x: f32, fc: f32, sr: f32) -> f32 {
        let fc = fc.clamp(20.0, max_hz(sr));
        let a = 1.0 - (-TAU * fc / sr).exp();
        for s in &mut self.0 {
            *s = flush(*s + a * (x - *s));
        }
        self.0[2] * (0.5 * sr / fc).sqrt()
    }
}

/// Seconds to a sample shift, or None when the blast arrives after the end.
fn shift(delay: f32, cx: &Cx) -> Option<usize> {
    let s = (delay * cx.sr).round();
    (s < cx.n as f32).then_some(s as usize)
}

/// Air absorption and spreading loss for `distance` metres, then `dry`
/// added into `out` from sample `at`.
fn arrive(out: &mut [f32], dry: &mut [f32], at: usize, distance: f32, sr: f32, air_m: f32) {
    if distance > 0.0 {
        let fc = (16_000.0 / (1.0 + distance / air_m)).clamp(120.0, max_hz(sr));
        let mut a = Biquad::new();
        let mut b = Biquad::new();
        a.set(FilterMode::Lp, fc, 0.707, 0.0, sr);
        b.set(FilterMode::Lp, fc, 0.707, 0.0, sr);
        let g = 1.0 / (1.0 + distance / 100.0);
        for v in dry.iter_mut() {
            *v = b.tick(a.tick(*v)) * g;
        }
    }
    for (o, v) in out[at..].iter_mut().zip(dry.iter()) {
        *o += v;
    }
}

/// Clips the sum softly and removes any non-finite sample.
fn finish(out: &mut [f32]) {
    for v in out {
        *v = if v.is_finite() {
            (0.7 * *v).tanh()
        } else {
            0.0
        };
    }
}

fn check_distance(distance: f32) -> Result<f32, String> {
    if distance.is_finite() && (0.0..=MAX_DISTANCE).contains(&distance) {
        Ok(distance)
    } else {
        Err(format!(
            "distance: {distance}: give metres from 0 to {MAX_DISTANCE}"
        ))
    }
}

/// Adds one tick per event into `dry`: a 2.5 ms exponentially decaying noise
/// burst, high-passed by a first difference, scaled by the event value.
fn add_debris(dry: &mut [f32], events: &[Event], rng: &mut crate::dsp::Rng, sr: f32) {
    let len = (0.015 * sr) as usize;
    let tau = 0.0025 * sr;
    for e in events {
        if !(e.t.is_finite() && e.t >= 0.0) {
            continue;
        }
        let start = (e.t * f64::from(sr)).round();
        if start >= dry.len() as f64 {
            continue;
        }
        let start = start as usize;
        let amp = 0.3 * if e.value.is_finite() { e.value } else { 0.0 };
        let mut prev = 0.0f32;
        for j in 0..len.min(dry.len() - start) {
            let w = rng.signed();
            let v = (w - 0.7 * prev) * (-(j as f32) / tau).exp();
            prev = w;
            dry[start + j] += amp * v;
        }
    }
}

// ---------------------------------------------------------------- explosion

/// An explosion: a Friedlander pulse, a falling sub thump, a noise body, a
/// dense decaying tail and a long fireball rumble (`add_fireball`), delayed
/// and darkened by distance.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Explosion {
    /// Fireball radius, metres, 0.1 ..= 100. Larger is longer, lower and
    /// has more sub bass: the pressure pulse lasts `3 ms * size`, the thump
    /// starts at `160 Hz * size^-0.35` (at most 160, at least 22) and falls
    /// to 0.3 of that, the tail lasts `0.3 s * size^0.5`.
    pub size: f32,
    /// Metres from the blast to the listener, 0 ..= 30000. The blast
    /// arrives `distance / 343` s later (plus a 2.5 ms lead), air takes the
    /// highs off and the level falls as `1 / (1 + distance / 100)`.
    pub distance: f32,
    /// An event stream node: a tick of flying debris at each event's time
    /// (seconds after the blast, so they are delayed with it), scaled by the
    /// event's value. At most 20000 events. Default: none.
    #[serde(default)]
    pub debris: Option<In>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Explosion {
    const BLURB: &'static str = "an explosion from its size and distance: pressure pulse, falling sub thump, noise body, tail, a long fireball rumble (Farnell), optional debris ticks from an event stream";
    fn example() -> serde_json::Value {
        serde_json::json!({"size": 5, "distance": 100, "debris": "ev", "seed": 1})
    }
}

impl Eval for Explosion {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let size = self.size;
        if !(size.is_finite() && (0.1..=100.0).contains(&size)) {
            return Err(format!(
                "size: {size}: give metres of fireball radius from 0.1 to 100"
            ));
        }
        let distance = check_distance(self.distance)?;
        let events = match &self.debris {
            Some(d) => {
                let e = cx.events("debris", d)?;
                if e.len() > MAX_DEBRIS {
                    return Err(format!(
                        "debris: {} events: at most {MAX_DEBRIS}; thin the stream (poisson rate, burst count)",
                        e.len()
                    ));
                }
                e
            }
            None => Vec::new(),
        };
        let mut out = vec![0.0f32; cx.n];
        if let Some(at) = shift(LEAD + distance / SOUND_SPEED, cx) {
            let mut dry = explosion_dry(cx, size, cx.n - at, &events, self.seed);
            arrive(&mut out, &mut dry, at, distance, cx.sr, EXPLOSION_AIR_M);
        }
        finish(&mut out);
        Ok(one(Buf::Audio(out)))
    }
}

fn explosion_dry(cx: &Cx, size: f32, len: usize, debris: &[Event], seed: u64) -> Vec<f32> {
    let sr = cx.sr;
    let mut body_rng = cx.rng(seed);
    let mut tail_rng = cx.rng(seed.wrapping_add(0x9E37_79B9));
    let mut debris_rng = cx.rng(seed.wrapping_add(0x2545_F491));
    let tp = 0.003 * size;
    let f0 = (160.0 * size.powf(-0.35)).clamp(22.0, 160.0);
    let f1 = (0.3 * f0).max(16.0);
    let tau_pitch = 0.06 * size.powf(0.3);
    let tau_thump = 0.05 * size.powf(0.45);
    let tau_body = 0.12 * size.powf(0.4);
    let fc_body = 5000.0 * size.powf(-0.4);
    let tau_tail = 0.3 * size.sqrt();
    let fc_tail = 500.0 * size.powf(-0.2);
    let mut phase = 0.0f32;
    let (mut lp_body, mut lp_tail) = (Lp3::default(), Lp3::default());
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let pulse = if t < 20.0 * tp {
            0.7 * friedlander(t, tp)
        } else {
            0.0
        };
        let fq = f1 + (f0 - f1) * (-t / tau_pitch).exp();
        phase = (phase + TAU * fq / sr).fract();
        let thump = 0.6 * (-t / tau_thump).exp() * (TAU * phase).sin();
        let eb = (-t / tau_body).exp();
        let body = 0.35 * eb * lp_body.tick(body_rng.signed(), (fc_body * eb).max(80.0), sr);
        let et = (-t / tau_tail).exp() * (1.0 - (-t / 0.02).exp());
        let tail = 0.3 * et * lp_tail.tick(tail_rng.signed(), fc_tail * (0.3 + 0.7 * et), sr);
        *y = pulse + thump + body + tail;
    }
    add_fireball(&mut dry, size, cx.rng(seed.wrapping_add(0x6A09_E667)), sr);
    add_debris(&mut dry, debris, &mut debris_rng, sr);
    dry
}

/// The fireball rumble, after Farnell (Designing Sound ch. 54), added in book
/// check 2 (the listener preferred its long rumble); see the body comment
/// for the patch. Level 1.2 (the dry sum is later soft-clipped).
fn add_fireball(dry: &mut [f32], size: f32, mut rng: crate::dsp::Rng, sr: f32) {
    // explosion-factory1.pd `fireball`: a line rises 0 -> 1 in 100 ms, then
    // falls to 0 over 8 s (here 8 s x (size / 2)^0.35); noise goes through a
    // band-pass (Q 1) and two one-pole low-passes, ALL at 40 + 100 x line
    // Hz, x 10, a 5 Hz high-pass; the level is line^4 smoothed by a 5 Hz
    // one-pole.
    let fall = 8.0 * (size / 2.0).powf(0.35);
    let mut bp = Svf::default();
    let (mut l1, mut l2) = (
        super::super::dsp::OnePole::default(),
        super::super::dsp::OnePole::default(),
    );
    let (mut hp, mut env_lp) = (
        super::super::dsp::OnePole::default(),
        super::super::dsp::OnePole::default(),
    );
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        if t > 0.1 + fall + 0.5 {
            break;
        }
        let line = if t < 0.1 {
            t / 0.1
        } else {
            (1.0 - (t - 0.1) / fall).max(0.0)
        };
        let fc = 40.0 + 100.0 * line;
        let x = bp.tick(rng.signed(), FilterMode::Bp, fc, 1.0, 0.0, sr);
        let x = l1.tick(x, fc, sr).0;
        let x = l2.tick(x, fc, sr).0;
        let x = hp.tick(10.0 * x, 5.0, sr).1;
        let e = env_lp.tick(line.powi(4), 5.0, sr).0;
        *y += 1.2 * e * x;
    }
}

// ------------------------------------------------------------------ gunshot

/// Largest `events` stream of a `gunshot`: rounds.
const MAX_ROUNDS: usize = 2_000;

/// A weapon preset: the defaults of caliber, barrel, action and punch, which
/// the explicit fields override.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Weapon {
    /// A 9 mm semi-automatic pistol: caliber 9, barrel 0.1 m, semi action,
    /// punch 0.45.
    Pistol,
    /// A .357 revolver: caliber 9.1, barrel 0.15 m, no action, punch 0.6.
    Revolver,
    /// A 9 mm sub-machine gun: caliber 9, barrel 0.2 m, semi action, punch 0.4.
    Smg,
    /// A 5.56 mm carbine: barrel 0.37 m, semi action, punch 0.5.
    Carbine,
    /// A 7.62 mm battle rifle: barrel 0.5 m, semi action, punch 0.6.
    Rifle,
    /// A .338 bolt rifle: caliber 8.6, barrel 0.7 m, no action, punch 0.8.
    Sniper,
    /// A 12 gauge shotgun: caliber 18.5, barrel 0.5 m, the shot at about 420
    /// m/s (a weak crack), no action, punch 1.
    Shotgun,
    /// A 7.62 mm machine gun: barrel 0.55 m, semi action, punch 0.7.
    Lmg,
}

/// (caliber mm, barrel m, action, punch, muzzle velocity cap m/s).
fn weapon_preset(w: Weapon) -> (f32, f32, Action, f32, Option<f32>) {
    match w {
        Weapon::Pistol => (9.0, 0.1, Action::Semi, 0.45, None),
        Weapon::Revolver => (9.1, 0.15, Action::None, 0.6, None),
        Weapon::Smg => (9.0, 0.2, Action::Semi, 0.4, None),
        Weapon::Carbine => (5.56, 0.37, Action::Semi, 0.5, None),
        Weapon::Rifle => (7.62, 0.5, Action::Semi, 0.6, None),
        Weapon::Sniper => (8.6, 0.7, Action::None, 0.8, None),
        Weapon::Shotgun => (18.5, 0.5, Action::None, 1.0, Some(420.0)),
        Weapon::Lmg => (7.62, 0.55, Action::Semi, 0.7, None),
    }
}

/// The mechanism heard with the shot.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// None (a revolver, a bolt or pump gun between shots).
    #[default]
    None,
    /// A slide or bolt carrier cycling: a clack 5-8 ms in (unlocking) and
    /// its return 35-55 ms in, about 20 dB under the blast.
    Semi,
}

/// The space the shot is heard in: the report tail.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GunSpace {
    /// Dry: no tail (a game adds the space, or exports `layer: tail`
    /// separately).
    #[default]
    None,
    /// Open ground: a dense report rolling for about a second, darkening
    /// from 5 kHz to 300 Hz.
    Open,
    /// Trees: a brighter, denser, slightly longer report (the trunks
    /// scatter the highs back).
    Forest,
    /// A street: facade slapbacks 35-160 ms in, then a 1.6 s report.
    Urban,
    /// A canyon or valley: late echoes at 0.4, 0.9 and 1.6 s over a 3.5 s
    /// dark report.
    Canyon,
    /// A room about 5 m long: a loud, short (0.5 s), bright reverberation.
    Room,
    /// A hall: a 1.8 s reverberation.
    Hall,
    /// A hangar or warehouse: a 3 s metallic reverberation with a flutter.
    Hangar,
}

/// What the node outputs: the whole shot, or one layer (they add back to
/// `all`).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GunLayer {
    /// Every layer.
    #[default]
    All,
    /// The muzzle blast and the punch.
    Close,
    /// The supersonic crack.
    Crack,
    /// The action.
    Mech,
    /// The report tail of the `space` (silent with `space: none`).
    Tail,
}

/// A gunshot: the open shot after Farnell (cartridge chirp, barrel wave and
/// noise through an 8-band weapon body, `blast_dry`), a low punch, the
/// action, an N-wave crack for a supersonic round, and the report of the
/// space it is fired in; one round or one per event.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gunshot {
    /// A preset (pistol, revolver, smg, carbine, rifle, sniper, shotgun,
    /// lmg): the defaults of caliber, barrel, action and punch. Give either
    /// this or `caliber`.
    #[serde(default)]
    pub weapon: Option<Weapon>,
    /// Bore diameter, millimetres, 2 ..= 60 (9 for a pistol, 5.56 a rifle,
    /// 12.7 a heavy machine gun). Larger is longer: the cartridge chirp,
    /// barrel wave and noise burst last `(caliber / 9)^0.3` times 93, 20 and
    /// 115 ms. Required unless `weapon` is given.
    #[serde(default)]
    pub caliber: Option<f32>,
    /// Barrel length, metres, 0.05 ..= 1.5 (default 0.4, or the weapon's).
    /// Longer gives a faster bullet (`300 + 600 * (1 - exp(-barrel / 0.35))`
    /// m/s: 380 at 5 cm, 708 at 40 cm) and a later barrel wave and noise
    /// burst (`12 ms * barrel / 0.4` after the chirp, 4..30 ms); the crack
    /// exists when 0.9 times that speed exceeds 343 m/s.
    #[serde(default)]
    pub barrel: Option<f32>,
    /// A suppressor on subsonic ammunition (default false): no crack and no
    /// pitched thump; a gas puff (noise, 200 Hz - 4 kHz, about 32 dB under
    /// the open shot) and the action: a bright clack 5-8 ms in and its
    /// return 35-55 ms in (seeded), with energy up to 8-12 kHz.
    #[serde(default)]
    pub suppressed: bool,
    /// The low punch of the muzzle blast, 0 ..= 1 (default 0.5, or the
    /// weapon's): a single atonal pressure swing and band-limited noise
    /// (50-250 Hz) lasting `30 ms * sqrt(caliber / 9)`. 0 is Farnell's shot
    /// alone. Ignored when `suppressed`.
    #[serde(default)]
    pub punch: Option<f32>,
    /// How hard the close shot is driven into saturation, 0 ..= 1 (default
    /// 0.7): a recorded shot overloads the microphone, so its crest is low
    /// (15-18 dB in the reference recordings against 23-25 dB clean). 0 is
    /// clean; the peak stays where it was.
    #[serde(default)]
    pub drive: Option<f32>,
    /// The mechanism: none or semi (default none, or the weapon's).
    #[serde(default)]
    pub action: Option<Action>,
    /// The space the shot is fired in (default none: dry): open, forest,
    /// urban, canyon, room, hall, hangar. Its report tail is the shot
    /// convolved with a response built for the space.
    #[serde(default)]
    pub space: GunSpace,
    /// Scales the space, 0.25 ..= 4 (default 1): the echo delays, and the
    /// tail length as `space_size^0.7`.
    #[serde(default)]
    pub space_size: Option<f32>,
    /// The tail level against the space's own, dB, -40 ..= 20 (default 0).
    #[serde(default)]
    pub tail_db: Option<f32>,
    /// The output: all (default), or one layer: close, crack, mech, tail.
    /// The layers add back to `all` (before the final soft clip).
    #[serde(default)]
    pub layer: GunLayer,
    /// An event stream node: one round at each event's time, scaled by its
    /// value (automatic fire: a `pattern` with `every: 60 / rpm`). Each round
    /// varies (seeded body, about +-0.5 dB) and the body sinks by up to 4 %
    /// as the barrel heats; the report rings under all of them. At most 2000
    /// rounds. Default: one round at the start.
    #[serde(default)]
    pub events: Option<In>,
    /// Metres from the gun to the listener, 0 ..= 30000 (default 0). The
    /// blast arrives after `distance / 343` s; the crack after `distance /
    /// (0.9 * muzzle velocity)`, so it leads the blast by the difference.
    /// The tail falls with distance more slowly than the shot.
    #[serde(default)]
    pub distance: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Gunshot {
    const BLURB: &'static str = "a gunshot: weapon preset (pistol revolver smg carbine rifle sniper shotgun lmg) or caliber + barrel; Farnell's blast through an 8-band weapon body, a low punch, the action, the supersonic N-wave crack, the report tail of a space (open forest urban canyon room hall hangar); layer all | close | crack | mech | tail; events for automatic fire; suppressor, distance";
    const WORDS: &'static [&'static str] = &["weapon", "action", "space", "layer"];
    fn example() -> serde_json::Value {
        serde_json::json!({"weapon": "pistol", "space": "urban", "distance": 10, "seed": 1})
    }
}

/// The resolved fields of one `gunshot`.
struct Gun {
    caliber: f32,
    barrel: f32,
    supp: bool,
    punch: f32,
    action: Action,
    vcap: Option<f32>,
}

impl Gunshot {
    fn resolve(&self) -> Result<Gun, String> {
        let preset = self.weapon.map(weapon_preset);
        let caliber = match (self.caliber, preset) {
            (Some(c), _) => c,
            (None, Some(p)) => p.0,
            (None, None) => {
                return Err(
                    "caliber: give the bore in millimetres (9 pistol, 5.56 rifle) or a weapon (pistol, rifle, shotgun, ...)".into(),
                );
            }
        };
        if !(caliber.is_finite() && (2.0..=60.0).contains(&caliber)) {
            return Err(format!(
                "caliber: {caliber}: give the bore in millimetres from 2 to 60 (9 pistol, 5.56 rifle)"
            ));
        }
        let barrel = self.barrel.or(preset.map(|p| p.1)).unwrap_or(0.4);
        if !(barrel.is_finite() && (0.05..=1.5).contains(&barrel)) {
            return Err(format!(
                "barrel: {barrel}: give the barrel length in metres from 0.05 to 1.5"
            ));
        }
        let punch = self.punch.or(preset.map(|p| p.3)).unwrap_or(0.5);
        if !(punch.is_finite() && (0.0..=1.0).contains(&punch)) {
            return Err(format!("punch: {punch}: give 0 to 1"));
        }
        Ok(Gun {
            caliber,
            barrel,
            supp: self.suppressed,
            punch,
            action: self.action.or(preset.map(|p| p.2)).unwrap_or_default(),
            vcap: preset.and_then(|p| p.4),
        })
    }
}

impl Eval for Gunshot {
    fn random(&self) -> bool {
        true
    }
    fn causal(&self) -> bool {
        false
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let g = self.resolve()?;
        let distance = check_distance(self.distance.unwrap_or(0.0))?;
        let size = self.space_size.unwrap_or(1.0);
        if !(size.is_finite() && (0.25..=4.0).contains(&size)) {
            return Err(format!("space_size: {size}: give 0.25 to 4"));
        }
        let drive = self.drive.unwrap_or(0.7);
        if !(drive.is_finite() && (0.0..=1.0).contains(&drive)) {
            return Err(format!("drive: {drive}: give 0 to 1"));
        }
        let tail_db = self.tail_db.unwrap_or(0.0);
        if !(tail_db.is_finite() && (-40.0..=20.0).contains(&tail_db)) {
            return Err(format!("tail_db: {tail_db}: give -40 to 20 dB"));
        }
        let rounds = match &self.events {
            Some(e) => {
                let ev = cx.events("events", e)?;
                if ev.len() > MAX_ROUNDS {
                    return Err(format!(
                        "events: {} rounds: at most {MAX_ROUNDS}; thin the stream (pattern count, poisson rate)",
                        ev.len()
                    ));
                }
                ev.into_iter()
                    .filter(|e| e.t.is_finite() && e.t >= 0.0 && e.value.is_finite())
                    .map(|e| (e.t as f32, e.value))
                    .collect()
            }
            None => vec![(0.0f32, 1.0f32)],
        };
        let sr = cx.sr;
        let n = cx.n;
        // Every layer at the gun, on the shot's own clock (the blast's
        // arrival is added by `arrive`).
        let mut close = vec![0.0f32; n];
        // The punch is kept apart: it is part of the close layer but does not
        // feed the report (a low end rolling through the tail was the biggest
        // difference from the recordings).
        let mut punch = vec![0.0f32; n];
        let mut mech = vec![0.0f32; n];
        // The crack on the listener's clock (it travels with the bullet).
        let mut crack = vec![0.0f32; n];
        // The crack on the gun's clock (44 ms after the trigger), only to
        // feed the report: its reflections carry the report's early highs.
        let mut crack_gun = vec![0.0f32; n];
        let mut vrng = cx.rng(self.seed.wrapping_add(0x51_7CC1));
        let vm = {
            let v = mean_velocity(g.barrel, g.supp);
            g.vcap.map_or(v, |c| v.min(0.9 * c))
        };
        let crack_lead = if vm > SOUND_SPEED && !g.supp {
            // guns1.pd adds the N-wave 44 ms after the trigger ("shockwave"),
            // so at the gun it never lands on top of the blast.
            Some((distance / vm).max(0.044))
        } else {
            None
        };
        for (i, &(t, value)) in rounds.iter().enumerate() {
            let Some(at) = shift(t, cx) else { continue };
            let len = n - at;
            let seed = self.seed.wrapping_add((i as u64).wrapping_mul(0x9E37_79B9));
            // One round is the seed's own shot; a stream varies each round a
            // little and sinks it as the barrel heats.
            let (tune, gain) = if self.events.is_some() {
                let heat = 1.0 - (0.002 * i as f32).min(0.04);
                (
                    heat * (1.0 + 0.015 * vrng.signed()),
                    value * 10f32.powf(0.5 * vrng.signed() / 20.0),
                )
            } else {
                (1.0, value)
            };
            let body = blast_dry(cx, g.caliber, g.barrel, g.supp, len, seed, tune);
            for (o, v) in close[at..].iter_mut().zip(&body) {
                *o += gain * v;
            }
            if !g.supp && g.punch > 0.0 {
                add_punch(&mut punch[at..], cx, g.caliber, gain * g.punch, seed);
            }
            if g.action == Action::Semi && !g.supp {
                let mut rng = cx.rng(seed.wrapping_add(0x2545_F491));
                let clicks = [
                    (0.005 + 0.003 * rng.unit(), 0.1),
                    (0.035 + 0.02 * rng.unit(), 0.07),
                ];
                let tau = 0.0005 * (g.caliber / 9.0).powf(0.3);
                add_clicks(&mut mech[at..], &clicks, 0.8 * gain, tau, &mut rng, sr);
            }
            if let Some(lead) = crack_lead
                && let Some(c) = shift(LEAD + t + lead, cx)
            {
                let wave = crack_dry(cx, g.caliber, vm / SOUND_SPEED, n - c);
                for (o, v) in crack[c..].iter_mut().zip(&wave) {
                    *o += gain * v;
                }
                if self.space != GunSpace::None
                    && let Some(cg) = shift(t + 0.044, cx)
                {
                    for (o, v) in crack_gun[cg..].iter_mut().zip(&wave) {
                        *o += gain * v;
                    }
                }
            }
        }
        // The report: the close shot through the space's response. The tail
        // is a diffuse field, so it falls with distance more slowly than the
        // shot (`arrive` takes `1 / (1 + d / 100)` off both).
        let mut tail = vec![0.0f32; n];
        if self.space != GunSpace::None {
            let mut rng = cx.rng(self.seed.wrapping_add(0x7A11));
            let ir = report_ir(self.space, size, sr, n, &mut rng);
            let src: Vec<f32> = close.iter().zip(&crack_gun).map(|(a, b)| a + b).collect();
            let wet = super::time::fft_convolve(&src, &ir, n)?;
            let g_tail = 10f32.powf(tail_db / 20.0) * (1.0 + distance / 100.0).sqrt();
            for (o, v) in tail.iter_mut().zip(&wet) {
                *o = g_tail * v;
            }
        }
        let keep = |l: GunLayer| self.layer == GunLayer::All || self.layer == l;
        for (c, p) in close.iter_mut().zip(&punch) {
            *c += p;
        }
        saturate(&mut close, drive);
        let mut src = vec![0.0f32; n];
        for (l, buf) in [
            (GunLayer::Close, &close),
            (GunLayer::Mech, &mech),
            (GunLayer::Tail, &tail),
        ] {
            if keep(l) {
                for (o, v) in src.iter_mut().zip(buf) {
                    *o += v;
                }
            }
        }
        let mut out = vec![0.0f32; n];
        if let Some(at) = shift(LEAD + distance / SOUND_SPEED, cx) {
            src.truncate(n - at);
            arrive(&mut out, &mut src, at, distance, sr, GUNSHOT_AIR_M);
        }
        if keep(GunLayer::Crack) && crack_lead.is_some() {
            arrive(&mut out, &mut crack, 0, distance, sr, GUNSHOT_AIR_M);
        }
        finish(&mut out);
        Ok(one(Buf::Audio(out)))
    }
}

/// Soft saturation that keeps the peak: `tanh(g x / p) / tanh(g) * p`, `p`
/// the peak, `g` from 0.01 (clean) to 5 at `drive` 1.
fn saturate(x: &mut [f32], drive: f32) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if drive <= 0.0 || peak <= 0.0 {
        return;
    }
    let g = 5.0 * drive;
    let norm = peak / g.tanh();
    for v in x {
        *v = (g * *v / peak).tanh() * norm;
    }
}

/// The low punch of the muzzle blast: one Friedlander pressure swing with
/// a positive phase of `2.5 ms * sqrt(caliber / 9)` (energy near 60-100 Hz,
/// a single cycle, so atonal), plus noise band-limited to 50-250 Hz with a
/// 1 ms attack and a `30 ms * sqrt(caliber / 9)` decay. Noise, not a sine:
/// a pitched thump read as "a tuned drum" on the impacts.
fn add_punch(dry: &mut [f32], cx: &Cx, caliber: f32, level: f32, seed: u64) {
    let sr = cx.sr;
    let s = (caliber / 9.0).sqrt();
    let tp = 0.0025 * s;
    let tau = 0.03 * s;
    let mut rng = cx.rng(seed.wrapping_add(0x0B00_5711));
    let a50 = 1.0 - (-TAU * 50.0 / sr).exp();
    let (mut lp, mut lpp, mut h1, mut h2) = (Lp3::default(), Lp3::default(), 0.0f32, 0.0f32);
    let len = ((8.0 * tau * sr) as usize).min(dry.len());
    for (i, y) in dry.iter_mut().enumerate().take(len) {
        let t = i as f32 / sr;
        let rise = 1.0 - (-t / 0.0003).exp();
        let pulse = if t < 10.0 * tp {
            friedlander(t, tp)
        } else {
            0.0
        };
        let pulse = lpp.tick(rise * pulse, 600.0, sr) / (0.5 * sr / 600.0).sqrt();
        let env = (1.0 - (-t / 0.001).exp()) * (-t / tau).exp();
        let nz = lp.tick(rng.signed(), 250.0, sr);
        h1 = flush(h1 + a50 * (nz - h1));
        let x = nz - h1;
        h2 = flush(h2 + a50 * (x - h2));
        *y += level * (2.2 * pulse + 0.9 * env * (x - h2));
    }
}

/// Clicks of the action into `dry`: at each `(time s, level)`, a first-
/// differenced noise click with a `tau` decay and a 0.4 echo 1-2 ms later.
fn add_clicks(
    dry: &mut [f32],
    clicks: &[(f32, f32)],
    gain: f32,
    tau: f32,
    rng: &mut crate::dsp::Rng,
    sr: f32,
) {
    let len = dry.len();
    for &(at, level) in clicks {
        for (dt, l) in [(0.0, 1.0), (0.001 + 0.001 * rng.unit(), 0.4)] {
            let start = ((at + dt) * sr).round() as usize;
            let n = ((8.0 * tau * sr) as usize).min(len.saturating_sub(start));
            let mut prev = 0.0f32;
            for j in 0..n {
                let w = rng.signed();
                let v = (w - 0.85 * prev) * (-(j as f32) / (tau * sr)).exp();
                prev = w;
                dry[start + j] += gain * level * l * v;
            }
        }
    }
}

/// The response of a space to a shot: noise that builds up over `build`
/// and falls 60 dB over `t60`, low-passed from `hf0` down to `hf1` over
/// the first half of it (air and foliage take the highs first), with discrete slapbacks
/// (facades, canyon walls) and a 40 Hz high-pass, scaled to unit energy
/// times the space's level. Not a generic reverb: an outdoor report is a
/// rolling, darkening field of reflections from the ground, trees and
/// buildings.
fn report_ir(space: GunSpace, size: f32, sr: f32, n: usize, rng: &mut crate::dsp::Rng) -> Vec<f32> {
    // (t60 s, predelay s, build s, hf0 Hz, hf1 Hz, level dB, slaps [(s, gain)])
    let (t60, pre, build, hf0, hf1, level_db, slaps): (
        f32,
        f32,
        f32,
        f32,
        f32,
        f32,
        &[(f32, f32)],
    ) = match space {
        GunSpace::None => return Vec::new(),
        GunSpace::Open => (1.8, 0.02, 0.03, 14000.0, 1000.0, -4.0, &[]),
        GunSpace::Forest => (1.4, 0.008, 0.04, 16000.0, 1500.0, 1.0, &[]),
        GunSpace::Urban => (
            1.6,
            0.012,
            0.02,
            6000.0,
            600.0,
            -2.0,
            &[(0.035, 0.5), (0.07, 0.35), (0.11, 0.3), (0.16, 0.25)],
        ),
        GunSpace::Canyon => (
            3.5,
            0.05,
            0.3,
            4000.0,
            250.0,
            -3.0,
            &[(0.4, 0.5), (0.9, 0.35), (1.6, 0.25)],
        ),
        GunSpace::Room => (0.5, 0.002, 0.005, 9000.0, 2500.0, 0.0, &[]),
        GunSpace::Hall => (1.8, 0.01, 0.02, 7000.0, 1500.0, -1.0, &[]),
        GunSpace::Hangar => (
            3.0,
            0.02,
            0.04,
            5000.0,
            1000.0,
            0.0,
            &[(0.06, 0.3), (0.12, 0.2)],
        ),
    };
    let t60 = t60 * size.powf(0.7);
    let (pre, build) = (pre * size, build * size);
    let last_slap = slaps.last().map_or(0.0, |s| s.0 * size);
    let len = (((pre + 1.2 * t60).max(last_slap + 0.3)) * sr) as usize;
    let len = len.min(n.max(1));
    // The excitation: decaying noise, plus a short burst at each slapback
    // (a 3 ms noise grain at the diffuse level times its gain x 6).
    let mut x = vec![0.0f32; len];
    let p0 = (pre * sr) as usize;
    for (i, v) in x.iter_mut().enumerate().skip(p0) {
        let t = (i - p0) as f32 / sr;
        let env = (1.0 - (-t / build.max(1e-4)).exp()) * 10f32.powf(-3.0 * t / t60);
        *v = env * rng.signed();
    }
    for &(d, gain) in slaps {
        let start = (d * size * sr) as usize;
        let level = 6.0 * gain * 10f32.powf(-3.0 * d * size / t60);
        let g_len = (0.003 * sr) as usize;
        for j in 0..g_len.min(len.saturating_sub(start)) {
            let w = (-(j as f32) / (0.0008 * sr)).exp();
            x[start + j] += level * w * rng.signed();
        }
    }
    // Darkening: the low-pass falls from hf0 to hf1 over t60.
    let mut lp = Lp3::default();
    let mut hp = 0.0f32;
    let a40 = 1.0 - (-TAU * 150.0 / sr).exp();
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let fc = hf0 * (hf1 / hf0).powf((2.0 * t / t60).min(1.0));
        let y = lp.tick(*v, fc, sr) / (0.5 * sr / fc).sqrt();
        hp = flush(hp + a40 * (y - hp));
        *v = y - hp;
    }
    let energy: f32 = x.iter().map(|v| v * v).sum();
    if energy > 0.0 {
        let g = 10f32.powf(level_db / 20.0) / energy.sqrt();
        for v in &mut x {
            *v *= g;
        }
    }
    x
}

/// The open shot, after Farnell's fig. 53.9 (Designing Sound ch. 53),
/// preferred in an A/B as "perfectly like an smg" in a burst. With
/// `s = caliber / 9`: the cartridge chirp `cos(2 pi ((1.05 x)^64 - 0.25))`,
/// x falling 1 -> 0 over `93 ms * s^0.3` (about 15 kHz down to a few hundred
/// Hz at constant level; Pd's pow~ is left ^ right); the barrel wave, four
/// cycles of `x cos(2 pi ((2 x)^2 - 0.25))` over `20 ms * s^0.3`, both
/// `12 ms * barrel / 0.4` (4..30 ms) after the chirp, and the noise burst 7
/// ms after that: `(1 - t / T)^8` over `115 ms * s^0.3` of guns1.pd's
/// `nwavenoise` (a triangle FM'd by band-passed noise); all three through the weapon body, eight
/// parallel band-passes (2225, 647, 1278, 1329, 2615, 500, 1682, 2243 Hz,
/// Q 4.56; all divided by `s^0.3`: a bigger gun is longer and lower), then x0.9 x `(0.4 / barrel)^0.5` (a short barrel is louder,
/// so it saturates harder), tanh, x0.8.
fn blast_dry(
    cx: &Cx,
    caliber: f32,
    barrel: f32,
    supp: bool,
    len: usize,
    seed: u64,
    tune: f32,
) -> Vec<f32> {
    if supp {
        return suppressed_dry(cx, caliber, len, seed);
    }
    let sr = cx.sr;
    let mut rng = cx.rng(seed);
    // Each seed's weapon body sits a little apart (+-3 % per band), so
    // seeds and rounds differ; `tune` sinks the whole bank (a hot barrel).
    let mut jrng = cx.rng(seed.wrapping_add(0x00B0_D7A1));
    let k = (caliber / 9.0).powf(0.3) / tune;
    let (chirp, bt, nt) = (0.093 * k, 0.02 * k, 0.115 * k);
    let gap = (0.012 * barrel / 0.4).clamp(0.004, 0.03);
    // "Shorter barrels produce a louder sound for the same calibre": the
    // body is driven `(0.4 / barrel)^0.5` harder into the tanh (0.5..3).
    let loud = (0.4 / barrel).sqrt().clamp(0.5, 3.0);
    let mut bank: Vec<Biquad> = [
        2225.0f32, 647.0, 1278.0, 1329.0, 2615.0, 500.0, 1682.0, 2243.0,
    ]
    .iter()
    .map(|&f| {
        let mut q = Biquad::new();
        let f = f * (1.0 + 0.03 * jrng.signed());
        q.set(FilterMode::Bp, (f / k).min(0.45 * sr), 4.56, 0.0, sr);
        q
    })
    .collect();
    let wave = |ph: f32| (TAU * (ph.fract() - 0.25)).cos();
    let mut nbp = Biquad::new();
    nbp.set(FilterMode::Bp, 3000.0f32.min(0.45 * sr), 2.0, 0.0, sr);
    let mut nhp = crate::dsp::OnePole::default();
    let mut tri_ph = 0.0f32;
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let mut e = 0.0;
        if t < chirp {
            e += wave((1.05 * (1.0 - t / chirp)).powi(64));
        }
        let u = t - gap;
        if (0.0..bt).contains(&u) {
            let x = 1.0 - u / bt;
            e += x * wave((2.0 * x).powi(2));
        }
        // guns1.pd `noiseburst`: the envelope jumps to 1 7 ms after its
        // trigger and falls as (1 - t / T)^8; the source is `nwavenoise`, a
        // triangle whose frequency is 100 Hz + 5000 x (noise band-passed at
        // 3 kHz, Q 2, high-passed at 40 Hz): a buzz, not a hiss.
        let fm = nhp.tick(nbp.tick(rng.signed()), 40.0, sr).1;
        tri_ph = (tri_ph + (100.0 + 5000.0 * fm) / sr).rem_euclid(1.0);
        let tri = 2.0 * (tri_ph - 1.0).max(-tri_ph) + 0.5;
        let un = u - 0.007;
        if (0.0..nt).contains(&un) {
            e += (1.0 - un / nt).powi(8) * tri;
        }
        let body: f32 = bank.iter_mut().map(|q| q.tick(e)).sum();
        *y = 0.8 * (0.9 * loud * body).tanh();
    }
    dry
}

/// A suppressed shot: no sine thump (a pitched thump with the gas muffled
/// read as "a metal sheet"), a gas puff of noise band-limited to 200 Hz -
/// 4 kHz, and the action: a bright clack (slide / bolt unlocking) 5-8 ms in
/// and its return 35-55 ms in, each a first-differenced noise click with a
/// 0.5 ms decay and a smaller echo 1-2 ms later, so energy reaches 8-10 kHz.
fn suppressed_dry(cx: &Cx, caliber: f32, len: usize, seed: u64) -> Vec<f32> {
    let sr = cx.sr;
    let mut puff_rng = cx.rng(seed);
    let mut act_rng = cx.rng(seed.wrapping_add(0x2545_F491));
    let s = caliber / 9.0;
    let gain = 0.04;
    let tau_puff = 0.018 * s.sqrt();
    let attack = 0.0004;
    let (mut lp, mut hp_state) = (Lp3::default(), 0.0f32);
    let a_hp = 1.0 - (-TAU * 200.0 / sr).exp();
    let fc_puff = (4000.0 * s.powf(-0.2)).min(max_hz(sr));
    let mut dry = vec![0.0f32; len];
    for (i, y) in dry.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let env = (1.0 - (-t / attack).exp()) * (-t / tau_puff).exp();
        let n = lp.tick(puff_rng.signed(), fc_puff, sr);
        hp_state = flush(hp_state + a_hp * (n - hp_state));
        *y = gain * 0.8 * env * (n - hp_state);
    }
    // Round 2: body (round 1 read as "a small hit"): a noise thump
    // band-limited to 80-400 Hz (two one-pole high-passes at 80 Hz, the
    // Lp3 at 400 Hz), 1 ms attack, a 25 ms x sqrt(caliber / 9) decay. Noise,
    // not a pitched sine (the old sine thump read as "a metal sheet").
    let mut thump_rng = cx.rng(seed.wrapping_add(0x7F4A_7C15));
    let tau_th = 0.025 * s.sqrt();
    let a80 = 1.0 - (-TAU * 80.0 / sr).exp();
    let (mut lpt, mut h1, mut h2) = (Lp3::default(), 0.0f32, 0.0f32);
    let th_len = ((8.0 * tau_th * sr) as usize).min(len);
    for (i, y) in dry.iter_mut().enumerate().take(th_len) {
        let t = i as f32 / sr;
        let env = (1.0 - (-t / 0.001).exp()) * (-t / tau_th).exp();
        let n = lpt.tick(thump_rng.signed(), 400.0, sr);
        h1 = flush(h1 + a80 * (n - h1));
        let x = n - h1;
        h2 = flush(h2 + a80 * (x - h2));
        *y += gain * 2.5 * env * (x - h2);
    }
    // The action, scaled by the caliber (a heavier slide clacks louder and a
    // little longer), placed with seeded jitter.
    let clicks = [
        (0.005 + 0.003 * act_rng.unit(), 0.1),
        (0.035 + 0.02 * act_rng.unit(), 0.07),
    ];
    let tau = 0.0005 * s.powf(0.3);
    for (at, level) in clicks {
        for (dt, l) in [(0.0, 1.0), (0.001 + 0.001 * act_rng.unit(), 0.4)] {
            let start = ((at + dt) * sr).round() as usize;
            let n = ((8.0 * tau * sr) as usize).min(len.saturating_sub(start));
            let mut prev = 0.0f32;
            for j in 0..n {
                let w = act_rng.signed();
                let v = (w - 0.85 * prev) * (-(j as f32) / (tau * sr)).exp();
                prev = w;
                dry[start + j] += gain * level * l * v;
            }
        }
    }
    dry
}

/// The N-wave: +1 falling linearly to -1 over `T`, `T` = 0.12 ms times
/// sqrt(caliber / 9), then 0; amplitude from the Mach number; one pole at
/// 14 kHz takes the edge off the two steps.
fn crack_dry(cx: &Cx, caliber: f32, mach: f32, len: usize) -> Vec<f32> {
    let sr = cx.sr;
    let width = (1.2e-4 * (caliber / 9.0).sqrt() * sr).max(3.0);
    let amp = 0.9 * ((mach - 1.0) / 0.5).clamp(0.4, 1.0);
    let a = 1.0 - (-TAU * 14_000.0f32.min(max_hz(sr)) / sr).exp();
    let mut y = 0.0f32;
    (0..len)
        .map(|i| {
            let x = i as f32;
            let w = if x < width {
                amp * (1.0 - 2.0 * x / width)
            } else {
                0.0
            };
            y = flush(y + a * (w - y));
            y
        })
        .collect()
}
