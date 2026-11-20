//! `rig`: several bodies joined by contacts and springs, stepped together
//! per sample. The general form of `impact`: a hammer, a plate and a loose
//! screw; a ball, a membrane and a frame. Bodies are point masses or modal
//! objects (`modal`'s mode table, read at each joint's attachment point);
//! joints are a Hunt-Crossley contact or a spring with an optional gap (a
//! rattle). Vertical motion only.

use std::f32::consts::PI;

use foley_synth::hertz_time;
use serde::Deserialize;

use super::modal::{
    DEFAULT_NOISE_WIDTH, MaterialSpec, Modal, ModalShape, add_noise_band, level_gain,
};
use super::rig_waveguide::{RigWaveguide, WgBody};
use crate::buf::{Buf, Sig};
use crate::node::{Cx, Describe, Eval, In, Outs};

/// One body of a rig.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RigBody {
    /// Its name, for the joints.
    pub name: String,
    /// Mass, kg (0.0001..10000).
    pub mass: f32,
    /// Held in place (default false): a fixed body does not translate (a
    /// clamped plate, a bell on its stand); its modes still ring.
    #[serde(default)]
    pub fixed: bool,
    /// Starting speed, m/s (default 0; positive is up, so a thrown hammer
    /// coming down is negative).
    #[serde(default)]
    pub velocity: Option<f32>,
    /// A force on the body, newtons (a signal; positive is up).
    #[serde(default)]
    pub drive: Option<In>,
    /// A modal object (default none: a point mass): `modal`'s material.
    #[serde(default)]
    pub material: Option<MaterialSpec>,
    /// plate (default) bar tube membrane string shell.
    #[serde(default)]
    pub shape: ModalShape,
    /// Size, metres, as `modal` (or `f0`).
    #[serde(default)]
    pub size: Option<In>,
    /// Lowest mode, Hz, as `modal` (or `size`).
    #[serde(default)]
    pub f0: Option<In>,
    /// As `modal`.
    #[serde(default)]
    pub thickness: Option<In>,
    /// As `modal` (default 1).
    #[serde(default)]
    pub loss: Option<f32>,
    /// As `modal`, on the radiated levels (default 0).
    #[serde(default)]
    pub tilt: Option<f32>,
    /// As `modal`, 0..1.
    #[serde(default)]
    pub noise: Option<f32>,
    /// As `modal` (default 0.25).
    #[serde(default)]
    pub noise_width: Option<f32>,
    /// Most modes (default 24, 1..64).
    #[serde(default)]
    pub modes: Option<usize>,
    /// Level of this body in the output, dB (default 0): a modal body
    /// radiates; a point mass is silent.
    #[serde(default)]
    pub level_db: Option<f32>,
    /// A waveguide body (default none): a string or a tube; see
    /// `docs/nodes/rig.md`. Not with `material`.
    #[serde(default)]
    pub waveguide: Option<RigWaveguide>,
}

/// Which kind of joint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum JointKind {
    /// Hunt-Crossley: pushes `a` up off `b` only while they overlap.
    #[default]
    Contact,
    /// A spring and damper; with a `gap` it only pulls past the gap (a loose
    /// part rattling on its fixing).
    Spring,
    /// `a`'s velocity at `at_a` drives end 0 of the tube `b`, and the air
    /// pushes back on `a` (`strength`).
    Air,
}

/// One joint of a rig, between body `a` (above) and body `b` (below; or
/// "ground", rigid and fixed).
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RigJoint {
    /// contact (default) or spring.
    #[serde(default)]
    pub kind: JointKind,
    /// The upper body.
    pub a: String,
    /// The lower body, or "ground".
    pub b: String,
    /// Where on `a` it attaches, 0..1, as `modal`'s `strike_position`
    /// (default 0.3; a point mass ignores it).
    #[serde(default)]
    pub at_a: Option<f32>,
    /// Where on `b` it attaches (default 0.3).
    #[serde(default)]
    pub at_b: Option<f32>,
    /// Contact: the starting distance between them, metres (default 0,
    /// touching); a body dropped from 0.3 m onto another is `gap` 0.3.
    /// Spring: the free play either side of its rest length (default 0).
    #[serde(default)]
    pub gap: Option<f32>,
    /// Contact: hardness of the pair 0..1 (default 0.7, rubber to steel),
    /// as `contact`'s, when neither side has a material.
    #[serde(default)]
    pub hardness: Option<f32>,
    /// Contact: radius of curvature at the touch, metres (default 0.01).
    #[serde(default)]
    pub radius: Option<f32>,
    /// Contact: restitution 0..0.98 (default 0.5).
    #[serde(default)]
    pub bounce: Option<f32>,
    /// Spring: stiffness, N/m (default 1e4).
    #[serde(default)]
    pub stiffness: Option<f32>,
    /// Spring: damping, N s/m (default 1).
    #[serde(default)]
    pub damping: Option<f32>,
    /// Contact: what its damping acts on (default "body"): "body" damps the
    /// bodies' own motion, so a bounce keeps `bounce` while the modes ring
    /// on (and the rebound off a held body is held to `bounce`); "point"
    /// damps the full speed at the touch, vibration included, with no
    /// rebound cap (how the batch 27 / 28 rigs were heard).
    #[serde(default)]
    pub damp: Option<String>,
    /// Contact with `damp` "body": how a real rebound leaves (default
    /// "exact": at `bounce` x the landing speed; "cap": only limited to it,
    /// as the batch 30 wind chime was heard).
    #[serde(default)]
    pub rebound: Option<String>,
    /// Contact: speed of `a` along `b`'s surface, m/s (a signal, default 0):
    /// a bow drawn, a box dragged, a stick scraped.
    #[serde(default)]
    pub slide: Option<In>,
    /// Contact: `[static, dynamic]` friction coefficients (default none: no
    /// drag). With `slide`, a Stribeck stick-slip drag drives a sliding copy
    /// of both sides' modes (as `impact`'s `friction`): a bow sings near the
    /// `stribeck` speed, a faster slide scrapes.
    #[serde(default)]
    pub friction: Option<[f32; 2]>,
    /// Contact: the speed where friction falls from static to dynamic, m/s
    /// (default 0.1, 0.001..10).
    #[serde(default)]
    pub stribeck: Option<f32>,
    /// Air: the coupling, 0..20 (default 1).
    #[serde(default)]
    pub strength: Option<f32>,
}

/// Bodies and joints stepped together.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rig {
    /// The bodies (1..16).
    pub bodies: Vec<RigBody>,
    /// The joints (0..32).
    pub joints: Vec<RigJoint>,
    /// Gravity, m/s^2 (default 9.81; 0 for a hit in free space).
    #[serde(default)]
    pub gravity: Option<f32>,
    /// Substeps per sample at least (default 4, 1..64; raised so the
    /// shortest contact spans 20 substeps).
    #[serde(default)]
    pub oversample: Option<usize>,
    /// When it starts, seconds (default 0).
    #[serde(default)]
    pub at: Option<f32>,
    /// Output level, dB (default 0). The output peaks at -6 dBFS before it.
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// Mixed with the document seed and the node name (default 0).
    #[serde(default)]
    pub seed: u64,
}

impl Describe for Rig {
    const BLURB: &'static str = "bodies (point masses, modal objects) joined by contacts and springs (a gap makes a rattle), stepped together per sample: a hammer on a plate with a loose screw, a ball on a drum; vertical motion; port `force` is the largest contact force above the resting load; peaks at -6 dBFS";
    const WORDS: &'static [&'static str] = &["kind", "a", "b", "damp", "rebound", "ends"];
    fn example() -> serde_json::Value {
        serde_json::json!({
            "bodies": [
                {"name": "hammer", "mass": 0.4, "velocity": -3.0},
                {"name": "sheet", "mass": 2.0, "fixed": true, "material": "metal", "size": 0.5,
                 "thickness": 0.002, "loss": 0.4, "noise": 0.3},
                {"name": "screw", "mass": 0.005, "material": "metal", "shape": "bar", "f0": 2500,
                 "loss": 0.6, "level_db": -6}],
            "joints": [
                {"kind": "contact", "a": "hammer", "b": "sheet", "at_b": 0.4, "hardness": 0.9},
                {"kind": "spring", "a": "screw", "b": "sheet", "at_b": 0.8, "gap": 0.0002,
                 "stiffness": 2e5, "damping": 2}]
        })
    }
}

/// A body while it runs: its rigid motion and its modes.
struct Live {
    mass: f32,
    fixed: bool,
    y: f32,
    vy: f32,
    // Per mode: omega^2, 2 sigma, q, v, radiation.
    w2: Vec<f32>,
    s2: Vec<f32>,
    q: Vec<f32>,
    v: Vec<f32>,
    // The same modes moving along the surface, driven by friction.
    qt: Vec<f32>,
    vt: Vec<f32>,
    r: Vec<f32>,
    modal_mass: f32,
    modes: Vec<(f32, f32, f32)>,
    noise: Option<f32>,
    noise_width: f32,
    level: f32,
    wg: Option<WgBody>,
}

/// A joint while it runs: indices (None = ground) and the coupling weights
/// of each side's modes at its attachment point.
struct Join {
    kind: JointKind,
    a: usize,
    b: Option<usize>,
    wa: Vec<f32>,
    wb: Vec<f32>,
    gap: f32,
    k: f32,
    damping: f32,
    bounce: f32,
    alpha: f32,
    touching: bool,
    // The approach speed at the last touch, for the rebound cap.
    v_in: f32,
    // Substeps since it last let go: touches within 3 ms are chatter of the
    // same landing and keep its cap.
    apart: usize,
    point_damp: bool,
    exact_rebound: bool,
    slide: Option<Sig>,
    friction: Option<(f32, f32)>,
    vc: f32,
    ta: Option<usize>,
    tb: Option<usize>,
}

fn modal_of(b: &RigBody, pos: f32, seed: u64) -> Option<Modal> {
    b.material.as_ref().map(|m| Modal {
        excite: In::num(0.0),
        material: m.clone(),
        size: b.size.clone(),
        f0: b.f0.clone(),
        f_min: None,
        f_max: None,
        shape: b.shape,
        strike_position: Some(In::num(pos)),
        modes: Some(b.modes.unwrap_or(24)),
        stiffness: None,
        loss: b.loss,
        thickness: b.thickness.clone(),
        tilt: b.tilt,
        gain_db: None,
        strike_glide: None,
        rattle: None,
        noise: b.noise,
        noise_width: b.noise_width,
        seed,
    })
}

/// The coupling weights of `body`'s modes at `pos`, matched to `table` (the
/// body's modes at its first attachment) by the nearest frequency.
fn weights_at(
    body: &RigBody,
    pos: f32,
    seed: u64,
    table: &[(f32, f32, f32)],
    cx: &Cx,
) -> Result<Vec<f32>, String> {
    let Some(m) = modal_of(body, pos, seed) else {
        return Ok(Vec::new());
    };
    let here = m.prepare(cx)?.modes;
    let peak = here.iter().fold(0.0f32, |a, m| a.max(m.2.abs())).max(1e-12);
    Ok(table
        .iter()
        .map(|&(f, _, _)| {
            here.iter()
                .filter(|h| (h.0 / f - 1.0).abs() < 0.01)
                .map(|h| h.2 / peak)
                .next()
                .unwrap_or(0.0)
        })
        .collect())
}

impl Eval for Rig {
    fn random(&self) -> bool {
        true
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        if !(1..=16).contains(&self.bodies.len()) {
            return Err(format!("bodies: {} given: give 1..16", self.bodies.len()));
        }
        if self.joints.len() > 32 {
            return Err(format!("joints: {} given: give 0..32", self.joints.len()));
        }
        let gravity = self.gravity.unwrap_or(9.81);
        if !(gravity.is_finite() && (0.0..=100.0).contains(&gravity)) {
            return Err(format!("gravity: {gravity}: give 0..100 m/s^2"));
        }
        let level = level_gain(self.gain_db)?;
        let index = |n: &str| self.bodies.iter().position(|b| b.name == n);
        for (i, b) in self.bodies.iter().enumerate() {
            if b.name == "ground" {
                return Err("body name `ground` is reserved for the fixed floor".into());
            }
            if self.bodies[..i].iter().any(|o| o.name == b.name) {
                return Err(format!("two bodies are named `{}`", b.name));
            }
            if !(b.mass.is_finite() && (0.0001..=10000.0).contains(&b.mass)) {
                return Err(format!(
                    "body `{}`: mass {}: give 0.0001..10000 kg",
                    b.name, b.mass
                ));
            }
            if b.material.is_none() && (b.size.is_some() || b.f0.is_some()) {
                return Err(format!(
                    "body `{}`: size / f0 without a material: give `material` for a modal body",
                    b.name
                ));
            }
            if b.waveguide.is_some() && b.material.is_some() {
                return Err(format!(
                    "body `{}`: a waveguide body takes no material",
                    b.name
                ));
            }
            if b.modes.is_some_and(|m| !(1..=64).contains(&m)) {
                return Err(format!("body `{}`: modes: give 1..64", b.name));
            }
        }
        // Each modal body's table is taken at its first attachment point.
        let first_at = |i: usize| -> f32 {
            for j in &self.joints {
                if index(&j.a) == Some(i) {
                    return j.at_a.unwrap_or(0.3);
                }
                if index(&j.b) == Some(i) {
                    return j.at_b.unwrap_or(0.3);
                }
            }
            0.3
        };
        let mut bodies: Vec<Live> = Vec::new();
        let mut drives: Vec<Option<Sig>> = Vec::new();
        for (i, b) in self.bodies.iter().enumerate() {
            let seed = self.seed ^ (i as u64 * 0x9e37);
            let (modes, noise, noise_width, tilt_db) = match modal_of(b, first_at(i), seed) {
                Some(m) => {
                    let p = m
                        .prepare(cx)
                        .map_err(|e| format!("body `{}`: {e}", b.name))?;
                    if p.modes.is_empty() {
                        return Err(format!(
                            "body `{}`: no mode in the audio band: change size / f0",
                            b.name
                        ));
                    }
                    (p.modes, p.noise_depth, p.noise_width, p.tilt_db)
                }
                None => (Vec::new(), None, None, 0.0),
            };
            let f_low = modes.first().map_or(1.0, |m| m.0);
            let peak = modes
                .iter()
                .fold(0.0f32, |a, m| a.max(m.2.abs()))
                .max(1e-12);
            bodies.push(Live {
                mass: b.mass,
                fixed: b.fixed || b.waveguide.is_some(),
                y: 0.0,
                vy: b.velocity.unwrap_or(0.0),
                w2: modes.iter().map(|m| (2.0 * PI * m.0).powi(2)).collect(),
                s2: modes
                    .iter()
                    .map(|m| 2.0 * 6.907_755 / m.1.max(1e-4))
                    .collect(),
                q: vec![0.0; modes.len()],
                v: vec![0.0; modes.len()],
                qt: vec![0.0; modes.len()],
                vt: vec![0.0; modes.len()],
                r: modes
                    .iter()
                    .map(|m| m.2 / peak * 10f32.powf(tilt_db * (m.0 / f_low).log2() / 20.0))
                    .collect(),
                modal_mass: b.mass / 4.0,
                modes,
                noise,
                noise_width: noise_width.unwrap_or(DEFAULT_NOISE_WIDTH),
                level: 10f32.powf(b.level_db.unwrap_or(0.0) / 20.0),
                wg: match &b.waveguide {
                    Some(w) => Some(w.live(&b.name, b.mass)?),
                    None => None,
                },
            });
            drives.push(match &b.drive {
                Some(d) => Some(
                    cx.mono("drive", d)
                        .map_err(|e| format!("body `{}`: {e}", b.name))?,
                ),
                None => None,
            });
        }
        let mut joins: Vec<Join> = Vec::new();
        let mut shortest = f32::INFINITY;
        for (n, j) in self.joints.iter().enumerate() {
            let a = index(&j.a).ok_or_else(|| format!("joint {n}: a: no body `{}`", j.a))?;
            let b =
                if j.b == "ground" {
                    None
                } else {
                    Some(index(&j.b).ok_or_else(|| {
                        format!("joint {n}: b: no body `{}` (or \"ground\")", j.b)
                    })?)
                };
            if Some(a) == b {
                return Err(format!("joint {n}: a and b are the same body"));
            }
            let pa = j.at_a.unwrap_or(0.3).clamp(0.02, 0.98);
            let pb = j.at_b.unwrap_or(0.3).clamp(0.02, 0.98);
            let seed_of = |i: usize| self.seed ^ (i as u64 * 0x9e37);
            let wa = weights_at(&self.bodies[a], pa, seed_of(a), &bodies[a].modes, cx)?;
            let wb = match b {
                Some(b) => weights_at(&self.bodies[b], pb, seed_of(b), &bodies[b].modes, cx)?,
                None => Vec::new(),
            };
            let gap = j.gap.unwrap_or(0.0);
            if !(gap.is_finite() && (0.0..=50.0).contains(&gap)) {
                return Err(format!("joint {n}: gap {gap}: give 0..50 m"));
            }
            let k = match j.kind {
                JointKind::Contact => {
                    let hard = j.hardness.unwrap_or(0.7);
                    if !(0.0..=1.0).contains(&hard) {
                        return Err(format!("joint {n}: hardness {hard}: give 0..1"));
                    }
                    // A side with a material contributes its own modulus.
                    let e_of = |i: Option<usize>| -> Result<(f32, f32), String> {
                        match i.and_then(|i| self.bodies[i].material.as_ref()) {
                            Some(m) => {
                                let p = m.props()?;
                                Ok((p.young, p.poisson))
                            }
                            None if i.is_none() => Ok((50e9, 0.25)),
                            None => Ok((1e6 * 2e5f32.powf(hard), 0.3)),
                        }
                    };
                    let (ea, na) = e_of(Some(a))?;
                    let (eb, nb) = e_of(b)?;
                    let e_star = 1.0 / ((1.0 - na * na) / ea + (1.0 - nb * nb) / eb);
                    let radius = j.radius.unwrap_or(0.01);
                    if !(radius.is_finite() && (0.0001..=1.0).contains(&radius)) {
                        return Err(format!("joint {n}: radius {radius}: give 0.0001..1 m"));
                    }
                    let m_a = bodies[a].mass;
                    let m_eff = b.map_or(m_a, |b| m_a * bodies[b].mass / (m_a + bodies[b].mass));
                    let v_in = (2.0 * gravity * gap)
                        .sqrt()
                        .max(bodies[a].vy.abs())
                        .max(0.1);
                    shortest = shortest.min(hertz_time(m_eff, radius, e_star, v_in));
                    4.0 / 3.0 * e_star * radius.sqrt()
                }
                JointKind::Air => {
                    if !b.is_some_and(|b| bodies[b].wg.as_ref().is_some_and(|w| w.is_tube())) {
                        return Err(format!("joint {n}: air: b must be a tube body"));
                    }
                    let s = j.strength.unwrap_or(1.0);
                    if !(s.is_finite() && (0.0..=20.0).contains(&s)) {
                        return Err(format!("joint {n}: strength {s}: give 0..20"));
                    }
                    s
                }
                JointKind::Spring => {
                    let k = j.stiffness.unwrap_or(1e4);
                    if !(k.is_finite() && k > 0.0 && k <= 1e9) {
                        return Err(format!("joint {n}: stiffness {k}: give 0..1e9 N/m"));
                    }
                    let m = bodies[a]
                        .mass
                        .min(b.map_or(f32::INFINITY, |b| bodies[b].mass));
                    shortest = shortest.min(2.0 * PI * (m / k).sqrt() / 2.0);
                    k
                }
            };
            if j.kind == JointKind::Spring
                && (j.slide.is_some() || j.friction.is_some() || j.stribeck.is_some())
            {
                return Err(format!(
                    "joint {n}: slide / friction / stribeck are for a contact joint, not a spring"
                ));
            }
            let slide = match &j.slide {
                Some(x) => Some(cx.mono("slide", x).map_err(|e| format!("joint {n}: {e}"))?),
                None => None,
            };
            let friction = match j.friction {
                None => None,
                Some([s_, d_]) => {
                    if !(s_.is_finite()
                        && d_.is_finite()
                        && (0.0..=3.0).contains(&s_)
                        && (0.0..=s_).contains(&d_))
                    {
                        return Err(format!(
                            "joint {n}: friction [{s_}, {d_}]: give [static, dynamic], 0..3, dynamic <= static"
                        ));
                    }
                    Some((s_, d_))
                }
            };
            let vc = j.stribeck.unwrap_or(0.1);
            if !(vc.is_finite() && (0.001..=10.0).contains(&vc)) {
                return Err(format!("joint {n}: stribeck {vc}: give 0.001..10 m/s"));
            }
            let point_damp = match j.damp.as_deref() {
                None | Some("body") => false,
                Some("point") => true,
                Some(o) => {
                    return Err(format!("joint {n}: damp {o:?}: give \"body\" or \"point\""));
                }
            };
            let exact_rebound = match j.rebound.as_deref() {
                None | Some("exact") => true,
                Some("cap") => false,
                Some(o) => {
                    return Err(format!(
                        "joint {n}: rebound {o:?}: give \"exact\" or \"cap\""
                    ));
                }
            };
            let bounce = j.bounce.unwrap_or(0.5);
            if !(0.0..=0.98).contains(&bounce) {
                return Err(format!("joint {n}: bounce {bounce}: give 0..0.98"));
            }
            let ta = bodies[a].wg.as_mut().map(|w| w.tap(pa));
            let tb = b.and_then(|b| bodies[b].wg.as_mut().map(|w| w.tap(pb)));
            joins.push(Join {
                kind: j.kind,
                a,
                b,
                wa,
                wb,
                gap,
                k,
                damping: j.damping.unwrap_or(1.0).max(0.0),
                bounce,
                alpha: 0.0,
                touching: false,
                v_in: 0.0,
                apart: usize::MAX / 2,
                // A waveguide has no rigid motion: damp the speed at the touch.
                point_damp: point_damp || ta.is_some() || tb.is_some(),
                exact_rebound,
                slide,
                friction,
                vc,
                ta,
                tb,
            });
        }
        let min_os = self.oversample.unwrap_or(4);
        if !(1..=64).contains(&min_os) {
            return Err(format!("oversample: {min_os}: give 1..64"));
        }
        let w_top = bodies
            .iter()
            .flat_map(|b| b.w2.iter())
            .fold(0.0f32, |a, w| a.max(w.sqrt()));
        let mut os = min_os.max((w_top / (1.2 * cx.sr)).ceil() as usize);
        if shortest.is_finite() {
            os = os.max((20.0 / (shortest * cx.sr)).ceil() as usize);
        }
        let os = os.min(64);
        let dt = 1.0 / (cx.sr * os as f32);
        for b in bodies.iter_mut() {
            if let Some(w) = b.wg.as_mut() {
                w.start(dt)?;
            }
        }
        let at = self.at.unwrap_or(0.0);
        if !(at.is_finite() && at >= 0.0) {
            return Err(format!("at: {at}: give seconds >= 0"));
        }
        let start = cx.at(at);
        let n = cx.n;
        let mut rings: Vec<Vec<Vec<f32>>> = bodies
            .iter()
            .map(|b| vec![vec![0.0; n]; b.modes.len()])
            .collect();
        let mut contact = vec![0.0f32; n];
        let mut touching = vec![0.0f32; n];
        let mut guided = vec![0.0f32; n];
        let mut forces = vec![0.0f32; bodies.len()];
        let mut modal_f: Vec<Vec<f32>> = bodies.iter().map(|b| vec![0.0; b.modes.len()]).collect();
        let mut drag_f: Vec<Vec<f32>> = bodies.iter().map(|b| vec![0.0; b.modes.len()]).collect();
        let tangential =
            |b: &Live, w: &[f32]| -> f32 { b.vt.iter().zip(w).map(|(v, w)| v * w).sum() };
        let disp = |b: &Live, w: &[f32], tap: Option<usize>| -> (f32, f32) {
            let (mut u, mut ud) = (b.y, b.vy);
            if let (Some(g), Some(t)) = (&b.wg, tap) {
                let (gu, gv) = g.at(t);
                u += gu;
                ud += gv;
            }
            for ((q, v), w) in b.q.iter().zip(&b.v).zip(w) {
                u += w * q;
                ud += w * v;
            }
            (u, ud)
        };
        for i in start..n {
            let pushes: Vec<f32> = drives
                .iter()
                .map(|d| d.as_ref().map_or(0.0, |d| d.at(i)))
                .collect();
            for _ in 0..os {
                forces.iter_mut().for_each(|f| *f = 0.0);
                modal_f
                    .iter_mut()
                    .for_each(|f| f.iter_mut().for_each(|x| *x = 0.0));
                drag_f
                    .iter_mut()
                    .for_each(|f| f.iter_mut().for_each(|x| *x = 0.0));
                for j in joins.iter_mut() {
                    let (ua, va) = disp(&bodies[j.a], &j.wa, j.ta);
                    let (ub, vb) = match j.b {
                        Some(b) => disp(&bodies[b], &j.wb, j.tb),
                        None => (0.0, 0.0),
                    };
                    // `a` sits `gap` above `b` at rest positions.
                    let sep = ua + j.gap - ub;
                    let vy_a = bodies[j.a].vy;
                    let rate = va - vb;
                    // The bodies' own motion, without their vibration: the
                    // contact's damping acts on this, so a bounce loses
                    // energy while the modes ring against a lossless spring
                    // (damping the full rate killed a resting pipe's lowest
                    // mode).
                    let rigid = if j.point_damp {
                        rate
                    } else {
                        bodies[j.a].vy - j.b.map_or(0.0, |b| bodies[b].vy)
                    };
                    let f = match j.kind {
                        JointKind::Contact => {
                            let d = -sep;
                            if d > 0.0 {
                                if !j.touching {
                                    j.touching = true;
                                    let v_in = (-rigid).max(0.02);
                                    if j.point_damp || j.apart as f32 * dt > 0.003 {
                                        j.alpha = 1.5 * (1.0 - j.bounce) / v_in;
                                        j.v_in = -rigid;
                                    }
                                }
                                j.apart = 0;
                                (j.k * d * d.sqrt() * (1.0 - j.alpha * rigid)).max(0.0)
                            } else {
                                j.apart = j.apart.saturating_add(1);
                                if j.touching {
                                    j.touching = false;
                                    // The modes are springs too and give their
                                    // energy back to the bounce (a pipe with
                                    // `bounce` 0 flew in `impact`): hold the
                                    // rebound of a free body off a held one to
                                    // `bounce` x the speed it came in at.
                                    let b_held = j.b.is_none_or(|b| bodies[b].fixed);
                                    if !j.point_damp && b_held && !bodies[j.a].fixed && j.v_in > 0.0
                                    {
                                        let vb = j.b.map_or(0.0, |b| bodies[b].vy);
                                        let target = j.bounce * j.v_in;
                                        let rel = bodies[j.a].vy - vb;
                                        // A real rebound (not chatter within
                                        // the landing) leaves at exactly
                                        // `bounce` x the landing speed: the
                                        // modes took most of a light ball's
                                        // bounce (a marble on a bowl stopped
                                        // after 3 landings at `bounce` 0.75).
                                        bodies[j.a].vy = if j.exact_rebound && rel > 0.3 * target {
                                            vb + target
                                        } else {
                                            bodies[j.a].vy.min(vb + target)
                                        };
                                    }
                                }
                                0.0
                            }
                        }
                        JointKind::Spring => {
                            // Free play of `gap` either side of rest.
                            let x = ua - ub;
                            let stretch = if x > j.gap {
                                x - j.gap
                            } else if x < -j.gap {
                                x + j.gap
                            } else {
                                0.0
                            };
                            let engaged = stretch != 0.0;
                            -(j.k * stretch) - if engaged { j.damping * rate } else { 0.0 }
                        }
                        JointKind::Air => match j.b.and_then(|b| bodies[b].wg.as_mut()) {
                            // Its vibration only: a body falling does not blow.
                            Some(w) => w.air(va - vy_a, j.k),
                            None => 0.0,
                        },
                    };
                    // A waveguide side gives way at its wave impedance: a
                    // contact stiffer than that cannot push it further
                    // than the overlap in one substep.
                    let f = match j.kind {
                        JointKind::Contact => {
                            let give = |b: &Live, t: Option<usize>| match (&b.wg, t) {
                                (Some(w), Some(_)) => w.give(-sep, dt),
                                _ => f32::INFINITY,
                            };
                            let g = give(&bodies[j.a], j.ta)
                                .min(j.b.map_or(f32::INFINITY, |b| give(&bodies[b], j.tb)));
                            f.min(g)
                        }
                        _ => f,
                    };
                    if j.kind == JointKind::Contact {
                        // Above the resting load of the upper body (its
                        // weight and a steady downward drive), so the port
                        // reads 0 once it rests.
                        let load = if bodies[j.a].fixed {
                            0.0
                        } else {
                            bodies[j.a].mass * gravity + (-pushes[j.a]).max(0.0)
                        };
                        contact[i] = contact[i].max(f - load);
                        touching[i] = touching[i].max(f);
                    }
                    // Friction along the surface: Stribeck on the slip
                    // between the slide and the two sides' sliding modes.
                    if let (Some((mu_s, mu_d)), true) = (j.friction, f > 0.0) {
                        let along = j.slide.as_ref().map_or(0.0, |x| x.at(i));
                        if along != 0.0 {
                            let vb = j.b.map_or(0.0, |b| tangential(&bodies[b], &j.wb));
                            let va = tangential(&bodies[j.a], &j.wa);
                            let slip = along - (vb - va);
                            // The compliance both sides offer the drag.
                            let soft = |bi: Option<usize>, w: &[f32]| {
                                bi.map_or(0.0, |b| {
                                    w.iter().map(|x| x * x).sum::<f32>()
                                        / bodies[b].modal_mass.max(1e-9)
                                })
                            };
                            let comp = (soft(Some(j.a), &j.wa) + soft(j.b, &j.wb)).max(1e-6);
                            let eps = (2.0 * dt * f * mu_s * comp).max(1e-3);
                            let r = slip / j.vc;
                            let drag =
                                (slip / eps).tanh() * (mu_d + (mu_s - mu_d) / (1.0 + r * r)) * f;
                            for (m, w) in drag_f[j.a].iter_mut().zip(&j.wa) {
                                *m -= w * drag;
                            }
                            if let Some(b) = j.b {
                                for (m, w) in drag_f[b].iter_mut().zip(&j.wb) {
                                    *m += w * drag;
                                }
                            }
                        }
                    }
                    // +f on a (up), -f on b.
                    forces[j.a] += f;
                    if let (Some(w), Some(t)) = (bodies[j.a].wg.as_mut(), j.ta) {
                        w.push(t, f);
                    }
                    for (m, w) in modal_f[j.a].iter_mut().zip(&j.wa) {
                        *m += w * f;
                    }
                    if let (Some(w), Some(t), true) = (
                        j.b.and_then(|b| bodies[b].wg.as_mut()),
                        j.tb,
                        j.kind != JointKind::Air,
                    ) {
                        w.push(t, -f);
                    }
                    if let Some(b) = j.b.filter(|_| j.kind != JointKind::Air) {
                        forces[b] -= f;
                        for (m, w) in modal_f[b].iter_mut().zip(&j.wb) {
                            *m -= w * f;
                        }
                    }
                }
                for (bi, b) in bodies.iter_mut().enumerate() {
                    if let Some(w) = b.wg.as_mut() {
                        w.step(dt);
                    } else if !b.fixed {
                        b.vy += dt * ((forces[bi] + pushes[bi]) / b.mass - gravity);
                        b.y += dt * b.vy;
                    }
                    for k in 0..b.modes.len() {
                        let a = modal_f[bi][k] / b.modal_mass - b.s2[k] * b.v[k] - b.w2[k] * b.q[k];
                        b.v[k] += dt * a;
                        b.q[k] += dt * b.v[k];
                        let at =
                            drag_f[bi][k] / b.modal_mass - b.s2[k] * b.vt[k] - b.w2[k] * b.qt[k];
                        b.vt[k] += dt * at;
                        b.qt[k] += dt * b.vt[k];
                    }
                }
            }
            for b in bodies.iter_mut() {
                if let Some(w) = b.wg.as_mut() {
                    guided[i] += w.radiate() * b.level;
                }
            }
            for (bi, b) in bodies.iter().enumerate() {
                for k in 0..b.modes.len() {
                    rings[bi][k][i] = b.r[k] * (b.v[k] + b.vt[k]) * b.level;
                }
            }
        }
        let mut out = vec![0.0f32; n];
        let mut nrng = crate::dsp::Rng::new(crate::mix_seed(cx.seed, self.seed ^ 0x0004_015e_u64));
        let mut white = vec![0.0f32; n];
        for (b, rs) in bodies.iter().zip(&rings) {
            for (ring, &(f, t60, _)) in rs.iter().zip(&b.modes) {
                match b.noise {
                    None => out.iter_mut().zip(ring).for_each(|(o, x)| *o += x),
                    Some(depth) => add_noise_band(
                        &mut out,
                        ring,
                        (f, t60),
                        depth,
                        b.noise_width,
                        &mut nrng,
                        &mut white,
                        cx.sr,
                    ),
                }
            }
        }
        out.iter_mut().zip(&guided).for_each(|(o, g)| *o += g);
        let peak = out.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        if !peak.is_finite() {
            return Err("the rig blew up: raise `oversample`, or soften a joint (lower stiffness or hardness)".into());
        }
        let scale = if peak > 0.0 { 0.5 / peak } else { 0.0 } * level.unwrap_or(1.0);
        out.iter_mut().for_each(|o| *o *= scale);
        let fmax = contact.iter().fold(0.0f32, |a, x| a.max(*x));
        if fmax > 0.0 {
            contact.iter_mut().for_each(|c| *c /= fmax);
        }
        let tmax = touching.iter().fold(0.0f32, |a, x| a.max(*x));
        if tmax > 0.0 {
            touching.iter_mut().for_each(|c| *c /= tmax);
        }
        Ok(vec![
            ("out", Buf::Audio(out)),
            ("force", Buf::Audio(contact)),
            ("contact", Buf::Audio(touching)),
        ])
    }
}
