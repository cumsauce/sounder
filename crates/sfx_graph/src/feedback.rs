//! Feedback: a cycle in the graph is legal only if it passes through a
//! `delay` node (a loop needs at least one sample of delay to be causal).
//!
//! Strongly connected components are found with Tarjan's algorithm. A
//! component with a cycle is evaluated sample by sample, in dependency order,
//! with every `delay` read before it is written: a delay's output at sample
//! i depends only on its input at earlier samples, so the edge from a delay to
//! what it reads is cut when ordering the nodes. A cycle that does not pass
//! through a delay is an error naming the nodes.
//!
//! Nodes inside a loop need a per-sample (`tick`) form. Block nodes are
//! unchanged; the loop-capable ones below have a [`Ticker`] built from the
//! same DSP structs (`dsp.rs`). Nodes outside the loop (sources, envelopes
//! that read nothing from it) are evaluated as whole buffers first and read
//! sample by sample.

use std::collections::{BTreeSet, HashMap};

use crate::buf::{Buf, Sig};
use crate::dsp::{DelayLine, Follower, OnePole, Svf};
use crate::eval::{Parsed, Ports};
use crate::node::{In, Node};
use crate::nodes::{
    Allpass, Comb, ExprProg, ExprState, Filter, Follower as FollowerNode, Gain, MathOp, Mix, MixIn,
    Onepole, PoleMode, Remap, Shape, ShapeMode, Slew, To,
};
use crate::{db_to_gain, nodes};

/// Node types that may sit inside a feedback loop.
pub const LOOP_TYPES: &[&str] = &[
    "delay", "filter", "onepole", "comb", "allpass", "gain", "math", "mix", "shape", "remap",
    "slew", "follower", "expr",
];

/// The loops of a graph.
#[derive(Debug, Clone, Default)]
pub struct Loops {
    /// Each loop's members in evaluation order (dependencies first, delays
    /// read before they are written).
    pub groups: Vec<Vec<usize>>,
    /// The loop a node belongs to.
    pub of: Vec<Option<usize>>,
    /// The condensation depth of each node (0 = reads nothing); all members
    /// of a loop share one.
    pub depth: Vec<usize>,
}

struct Tarjan<'a> {
    deps: &'a [Vec<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    on: Vec<bool>,
    stack: Vec<usize>,
    next: usize,
    comps: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    fn visit(&mut self, v: usize) {
        self.index[v] = Some(self.next);
        self.low[v] = self.next;
        self.next += 1;
        self.stack.push(v);
        self.on[v] = true;
        for k in 0..self.deps[v].len() {
            let w = self.deps[v][k];
            match self.index[w] {
                None => {
                    self.visit(w);
                    self.low[v] = self.low[v].min(self.low[w]);
                }
                Some(iw) if self.on[w] => self.low[v] = self.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(self.low[v]) == self.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = self.stack.pop() {
                self.on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            self.comps.push(comp);
        }
    }
}

/// Finds the loops, checks each is legal, and orders their members; also
/// assigns every node its depth for the parallel levels.
pub fn find_loops(nodes: &[Parsed]) -> Result<Loops, String> {
    let idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.as_str(), i))
        .collect();
    let deps: Vec<Vec<usize>> = nodes
        .iter()
        .map(|p| {
            p.reads
                .iter()
                .filter_map(|r| idx.get(r.as_str()).copied())
                .collect()
        })
        .collect();
    let n = nodes.len();
    let mut t = Tarjan {
        deps: &deps,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        next: 0,
        comps: Vec::new(),
    };
    for v in 0..n {
        if t.index[v].is_none() {
            t.visit(v);
        }
    }
    // Tarjan emits a component after everything it reads: evaluation order.
    let comps = t.comps;
    let mut comp_of = vec![0usize; n];
    for (c, members) in comps.iter().enumerate() {
        for &m in members {
            comp_of[m] = c;
        }
    }
    let mut comp_depth = vec![0usize; comps.len()];
    let mut out = Loops {
        groups: Vec::new(),
        of: vec![None; n],
        depth: vec![0; n],
    };
    for (c, members) in comps.iter().enumerate() {
        let mut d = 0;
        for &m in members {
            for &w in &deps[m] {
                if comp_of[w] != c {
                    d = d.max(comp_depth[comp_of[w]] + 1);
                }
            }
        }
        comp_depth[c] = d;
        for &m in members {
            out.depth[m] = d;
        }
        let cyclic = members.len() > 1 || deps[members[0]].contains(&members[0]);
        if !cyclic {
            continue;
        }
        let order = order_loop(nodes, &deps, members)?;
        let g = out.groups.len();
        for &m in &order {
            out.of[m] = Some(g);
        }
        out.groups.push(order);
    }
    Ok(out)
}

/// Orders a loop's members, or explains why it is not a legal loop.
fn order_loop(
    nodes: &[Parsed],
    deps: &[Vec<usize>],
    members: &[usize],
) -> Result<Vec<usize>, String> {
    let set: BTreeSet<usize> = members.iter().copied().collect();
    // The edges that order evaluation: what each node reads, except that a
    // delay's own inputs are written after the others are computed.
    let reduced = |v: usize| -> Vec<usize> {
        if nodes[v].ty == "delay" {
            Vec::new()
        } else {
            deps[v]
                .iter()
                .copied()
                .filter(|w| set.contains(w))
                .collect()
        }
    };
    // Depth-first topological order with cycle reporting.
    let mut state: HashMap<usize, u8> = HashMap::new(); // 1 = on the path, 2 = done
    let mut order = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    fn go(
        v: usize,
        reduced: &dyn Fn(usize) -> Vec<usize>,
        state: &mut HashMap<usize, u8>,
        order: &mut Vec<usize>,
        path: &mut Vec<usize>,
    ) -> Result<(), Vec<usize>> {
        match state.get(&v) {
            Some(2) => return Ok(()),
            Some(1) => {
                let at = path.iter().position(|&p| p == v).unwrap_or(0);
                let mut cyc: Vec<usize> = path[at..].to_vec();
                cyc.push(v);
                return Err(cyc);
            }
            _ => {}
        }
        state.insert(v, 1);
        path.push(v);
        for w in reduced(v) {
            go(w, reduced, state, order, path)?;
        }
        path.pop();
        state.insert(v, 2);
        order.push(v);
        Ok(())
    }
    for &m in members {
        if let Err(cyc) = go(m, &reduced, &mut state, &mut order, &mut path) {
            let names: Vec<&str> = cyc.iter().map(|&j| nodes[j].name.as_str()).collect();
            return Err(format!(
                "nodes form a cycle without a delay: {}: a loop needs a `delay` node in it (feedback with no delay would depend on itself within one sample); put one where the signal returns, or use the feedback field of delay or comb",
                names.join(" -> ")
            ));
        }
    }
    for &m in members {
        let p = &nodes[m];
        if !LOOP_TYPES.contains(&p.ty.as_str()) {
            return Err(format!(
                "node `{}` ({}) is inside a feedback loop ({}) but has no per-sample form yet: loops can contain {}",
                p.name,
                p.ty,
                members
                    .iter()
                    .map(|&j| nodes[j].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                LOOP_TYPES.join(", ")
            ));
        }
    }
    Ok(order)
}

// ----------------------------------------------------------------- env

/// What a per-sample node sees: the loop's current values and the buffers
/// of everything outside the loop.
pub struct Env<'a> {
    vals: &'a [f32],
    names: &'a HashMap<&'a str, usize>,
    ext: &'a HashMap<String, Sig>,
}

impl Env<'_> {
    /// An input's value at sample `i`.
    #[inline]
    pub fn get(&self, i: usize, x: &In) -> f32 {
        match x {
            In::Num(v) => *v,
            In::Vec(v) => v.iter().sum::<f32>() / v.len().max(1) as f32,
            In::Name(s) => {
                let base = s.split('.').next().unwrap_or(s);
                match self.names.get(base) {
                    Some(&k) => self.vals[k],
                    None => self.ext.get(s).map_or(0.0, |sig| sig.at(i)),
                }
            }
        }
    }
}

/// A node's per-sample form.
pub trait Ticker {
    /// The output at sample `i`, from this sample's inputs.
    fn out(&mut self, i: usize, env: &Env) -> f32;
    /// After every node of the loop has its value for sample `i`: a delay
    /// writes its input here.
    fn commit(&mut self, _i: usize, _env: &Env) {}
}

struct Build<'a> {
    sr: f32,
    n: usize,
    ext: &'a HashMap<String, Sig>,
    members: &'a HashMap<&'a str, usize>,
}

impl Build<'_> {
    /// The longest value of a signal field in seconds (or Hz when `hz`),
    /// scanning an outside buffer; a signal from inside the loop needs a
    /// stated maximum.
    fn longest(&self, field: &str, x: &In, hz: bool) -> Result<f32, String> {
        let conv = |v: f32| {
            if hz {
                self.sr / v.max(1.0)
            } else {
                v.max(0.0) * self.sr
            }
        };
        match x {
            In::Num(v) => Ok(conv(*v)),
            In::Vec(v) => Ok(conv(v.first().copied().unwrap_or(0.0))),
            In::Name(s) => {
                let base = s.split('.').next().unwrap_or(s);
                if self.members.contains_key(base) {
                    return Err(format!(
                        "{field}: `{s}` is inside the loop, so the delay length cannot be sized from it: use a number or a node outside the loop"
                    ));
                }
                match self.ext.get(s) {
                    Some(sig) => Ok((0..self.n.max(1))
                        .map(|i| conv(sig.at(i)))
                        .fold(1.0, f32::max)),
                    None => Ok(1.0),
                }
            }
        }
    }
}

fn clamp_line(samples: f32, sr: f32) -> usize {
    (samples.ceil() as usize + 2).min(60 * sr as usize)
}

struct DelayT {
    line: DelayLine,
    input: In,
    time: In,
    fb: Option<In>,
    sr: f32,
    back: f32,
}

impl Ticker for DelayT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let d = (env.get(i, &self.time) * self.sr).max(1.0);
        self.back = self.line.read(d - 1.0);
        self.back
    }
    fn commit(&mut self, i: usize, env: &Env) {
        let fb = self
            .fb
            .as_ref()
            .map_or(0.0, |f| env.get(i, f))
            .clamp(-0.99, 0.99);
        self.line.write(env.get(i, &self.input) + fb * self.back);
    }
}

struct FilterT {
    svf: Svf,
    n: Filter,
    sr: f32,
}

impl Ticker for FilterT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let q = self
            .n
            .q
            .as_ref()
            .map_or(std::f32::consts::FRAC_1_SQRT_2, |q| env.get(i, q));
        let g = self.n.gain_db.as_ref().map_or(0.0, |g| env.get(i, g));
        self.svf.tick(
            env.get(i, &self.n.input),
            self.n.mode.mode(),
            env.get(i, &self.n.freq),
            q,
            g,
            self.sr,
        )
    }
}

struct OnepoleT {
    f: OnePole,
    n: Onepole,
    sr: f32,
}

impl Ticker for OnepoleT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let (lp, hp) = self
            .f
            .tick(env.get(i, &self.n.input), env.get(i, &self.n.freq), self.sr);
        if matches!(self.n.mode, PoleMode::Hp) {
            hp
        } else {
            lp
        }
    }
}

struct GainT {
    n: Gain,
    db: bool,
}

impl Ticker for GainT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let x = env.get(i, &self.n.input);
        if self.db {
            x * self
                .n
                .db
                .as_ref()
                .map_or(1.0, |d| db_to_gain(env.get(i, d)))
        } else {
            x * self.n.gain.as_ref().map_or(1.0, |g| env.get(i, g))
        }
    }
}

struct MathT {
    n: nodes::Math,
}

impl Ticker for MathT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let a = env.get(i, &self.n.a);
        let b = self.n.b.as_ref().map_or(0.0, |b| env.get(i, b));
        let t = self.n.t.as_ref().map_or(0.0, |t| env.get(i, t));
        match self.n.op {
            MathOp::Add => a + b,
            MathOp::Sub => a - b,
            MathOp::Mul => a * b,
            MathOp::Div => {
                if b.abs() < 1e-12 {
                    0.0
                } else {
                    a / b
                }
            }
            MathOp::Min => a.min(b),
            MathOp::Max => a.max(b),
            MathOp::Pow => a.signum() * a.abs().powf(b),
            MathOp::Abs => a.abs(),
            MathOp::Mix => a + (b - a) * t,
        }
    }
}

struct MixT {
    inputs: Vec<(In, Option<In>)>,
}

impl Ticker for MixT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        self.inputs
            .iter()
            .map(|(x, g)| env.get(i, x) * g.as_ref().map_or(1.0, |g| env.get(i, g)))
            .sum()
    }
}

struct ShapeT {
    n: Shape,
    levels: f32,
}

impl Ticker for ShapeT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let u = env.get(i, &self.n.input) * self.n.drive.as_ref().map_or(1.0, |d| env.get(i, d));
        match self.n.mode {
            ShapeMode::Tanh => u.tanh(),
            ShapeMode::Clip => u.clamp(-1.0, 1.0),
            ShapeMode::Fold => ((u + 1.0).rem_euclid(4.0) - 2.0).abs() - 1.0,
            ShapeMode::Bitcrush => (u * self.levels).round() / self.levels,
            ShapeMode::Rectify => u.abs(),
        }
    }
}

struct RemapT {
    n: Remap,
}

impl Ticker for RemapT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let [a, b] = self.n.from;
        let [c, d] = self.n.to;
        let y = c + (env.get(i, &self.n.input) - a) / (b - a) * (d - c);
        if self.n.clamp {
            y.clamp(c.min(d), c.max(d))
        } else {
            y
        }
    }
}

struct SlewT {
    n: Slew,
    y: Option<f32>,
    up: f32,
    down: f32,
}

impl Ticker for SlewT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let x = env.get(i, &self.n.input);
        let y = self.y.unwrap_or(x);
        let y = y + (x - y).clamp(-self.down, self.up);
        self.y = Some(y);
        y
    }
}

struct FollowerT {
    n: FollowerNode,
    f: Follower,
    sr: f32,
}

impl Ticker for FollowerT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        self.f.tick(
            env.get(i, &self.n.input).abs(),
            self.n.attack.unwrap_or(0.005),
            self.n.release.unwrap_or(0.1),
            self.sr,
        )
    }
}

struct ExprT {
    prog: ExprProg,
    st: ExprState,
    inputs: Vec<In>,
    vals: Vec<f64>,
    sr: f64,
}

impl Ticker for ExprT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        for (v, x) in self.vals.iter_mut().zip(&self.inputs) {
            *v = f64::from(env.get(i, x));
        }
        self.st.tick(&self.prog, i as f64 / self.sr, &self.vals) as f32
    }
}

struct CombT {
    n: Comb,
    line: DelayLine,
    lp: f32,
    sr: f32,
}

impl Ticker for CombT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let d = match (&self.n.freq, &self.n.time) {
            (Some(f), _) => self.sr / env.get(i, f).max(1.0),
            (None, Some(t)) => env.get(i, t) * self.sr,
            (None, None) => 1.0,
        };
        let back = self.line.read((d - 1.0).max(0.0));
        let damp = self.n.damp.unwrap_or(0.0).clamp(0.0, 0.99);
        self.lp = back + damp * (self.lp - back);
        let y =
            env.get(i, &self.n.input) + env.get(i, &self.n.feedback).clamp(-0.999, 0.999) * self.lp;
        self.line.write(y);
        y
    }
}

struct AllpassT {
    n: Allpass,
    line: DelayLine,
    g: f32,
    sr: f32,
}

impl Ticker for AllpassT {
    fn out(&mut self, i: usize, env: &Env) -> f32 {
        let d = (env.get(i, &self.n.time) * self.sr).max(1.0);
        let back = self.line.read(d - 1.0);
        let w = env.get(i, &self.n.input) + self.g * back;
        self.line.write(w);
        back - self.g * w
    }
}

fn ticker(node: &Node, b: &Build, node_seed: u64) -> Result<Box<dyn Ticker>, String> {
    let sr = b.sr;
    Ok(match node {
        Node::Delay(n) => {
            let secs = match n.max {
                Some(m) => m.max(0.0) * sr,
                None => b.longest("time", &n.time, false)?,
            };
            Box::new(DelayT {
                line: DelayLine::new(clamp_line(secs, sr)),
                input: n.input.clone(),
                time: n.time.clone(),
                fb: n.feedback.clone(),
                sr,
                back: 0.0,
            })
        }
        Node::Filter(n) => {
            if let Some(s) = n.slope
                && s != 12.0
            {
                return Err(format!(
                    "slope: {s}: a loop runs one 12 dB section; use slope 12 inside it and put the steeper filter after the loop"
                ));
            }
            Box::new(FilterT {
                svf: Svf::default(),
                n: n.clone(),
                sr,
            })
        }
        Node::Onepole(n) => Box::new(OnepoleT {
            f: OnePole::default(),
            n: n.clone(),
            sr,
        }),
        Node::Gain(n) => {
            let db = match (&n.db, &n.gain) {
                (Some(_), None) => true,
                (None, Some(_)) => false,
                _ => return Err("give exactly one of db or gain".into()),
            };
            Box::new(GainT { n: n.clone(), db })
        }
        Node::Math(n) => {
            if n.b.is_none() && !matches!(n.op, MathOp::Abs) {
                return Err("b: this op needs a second operand b".into());
            }
            if n.t.is_none() && matches!(n.op, MathOp::Mix) {
                return Err("t: mix needs t (0 = a, 1 = b)".into());
            }
            Box::new(MathT { n: n.clone() })
        }
        Node::Mix(n) => mix_ticker(n)?,
        Node::Shape(n) => Box::new(ShapeT {
            n: n.clone(),
            levels: 2f32.powf(n.bits.unwrap_or(8.0).clamp(1.0, 24.0) - 1.0),
        }),
        Node::Remap(n) => {
            if (n.from[1] - n.from[0]).abs() < 1e-12 {
                return Err("from: the two ends are equal; give a range [a, b] with a != b".into());
            }
            Box::new(RemapT { n: n.clone() })
        }
        Node::Slew(n) => {
            let step = |t: f32| {
                if t <= 0.0 {
                    f32::INFINITY
                } else {
                    1.0 / (t * sr)
                }
            };
            Box::new(SlewT {
                n: n.clone(),
                y: None,
                up: step(n.up),
                down: step(n.down),
            })
        }
        Node::Follower(n) => Box::new(FollowerT {
            n: n.clone(),
            f: Follower::new(),
            sr,
        }),
        Node::Expr(n) => {
            let names: Vec<&str> = n.inputs.keys().map(String::as_str).collect();
            let prog = ExprProg::compile(&names, &n.state, &n.out, &n.update, f64::from(sr))?;
            let st = prog.start(crate::mix_seed(node_seed, n.seed));
            Box::new(ExprT {
                prog,
                st,
                inputs: n.inputs.values().cloned().collect(),
                vals: vec![0.0; n.inputs.len()],
                sr: f64::from(sr),
            })
        }
        Node::Comb(n) => {
            let secs = match (&n.freq, &n.time) {
                (Some(f), None) => b.longest("freq", f, true)?,
                (None, Some(t)) => b.longest("time", t, false)?,
                _ => return Err(
                    "give exactly one of freq (Hz: a pitched resonance) or time (seconds: an echo)"
                        .into(),
                ),
            };
            Box::new(CombT {
                n: n.clone(),
                line: DelayLine::new(clamp_line(secs, sr)),
                lp: 0.0,
                sr,
            })
        }
        Node::Allpass(n) => {
            let secs = b.longest("time", &n.time, false)?;
            Box::new(AllpassT {
                n: n.clone(),
                line: DelayLine::new(clamp_line(secs, sr)),
                g: n.feedback.unwrap_or(0.5).clamp(-0.99, 0.99),
                sr,
            })
        }
        _ => return Err("has no per-sample form".into()),
    })
}

fn mix_ticker(n: &Mix) -> Result<Box<dyn Ticker>, String> {
    if matches!(n.to, Some(To::Stereo)) {
        return Err("to: stereo: loops are mono; mix to stereo after the loop".into());
    }
    let mut inputs = Vec::new();
    for (k, m) in n.inputs.iter().enumerate() {
        match m {
            MixIn::Name(s) => inputs.push((In::Name(s.clone()), None)),
            MixIn::Full(f) => {
                if f.pan.is_some() || f.at.is_some_and(|a| a != 0.0) {
                    return Err(format!(
                        "inputs[{k}]: pan and at are not available inside a loop: pan / offset after the loop"
                    ));
                }
                inputs.push((f.input.clone(), f.gain.clone()));
            }
        }
    }
    Ok(Box::new(MixT { inputs }))
}

/// Evaluates one loop sample by sample: returns each member's buffer in the
/// order of `members` (as [`Loops::groups`] gives them).
pub fn eval_loop(
    members: &[usize],
    parsed: &[Parsed],
    ports: &HashMap<String, Ports>,
    sr: f32,
    n: usize,
    seed: u64,
) -> Result<Vec<Ports>, String> {
    let names: HashMap<&str, usize> = members
        .iter()
        .enumerate()
        .map(|(k, &m)| (parsed[m].name.as_str(), k))
        .collect();
    // The outside signals the members read.
    let mut ext: HashMap<String, Sig> = HashMap::new();
    for &m in members {
        for (name, port) in crate::node::refs_typed(&parsed[m].ty, &parsed[m].body) {
            if names.contains_key(name.as_str()) || !ports.contains_key(&name) {
                continue;
            }
            let key = if port.is_empty() {
                name.clone()
            } else {
                format!("{name}.{port}")
            };
            if let std::collections::hash_map::Entry::Vacant(slot) = ext.entry(key.clone()) {
                let sig = crate::eval::resolve(ports, &In::Name(key))
                    .map_err(|e| format!("node `{}` ({}): {e}", parsed[m].name, parsed[m].ty))?;
                slot.insert(sig);
            }
        }
    }
    let build = Build {
        sr,
        n,
        ext: &ext,
        members: &names,
    };
    let mut tickers: Vec<Box<dyn Ticker>> = Vec::with_capacity(members.len());
    for &m in members {
        let p = &parsed[m];
        tickers.push(
            ticker(&p.node, &build, crate::eval::node_seed(seed, &p.name))
                .map_err(|e| format!("node `{}` ({}): {e}", p.name, p.ty))?,
        );
    }
    let delays: Vec<usize> = members
        .iter()
        .enumerate()
        .filter(|(_, m)| parsed[**m].ty == "delay")
        .map(|(k, _)| k)
        .collect();
    let mut vals = vec![0.0f32; members.len()];
    let mut bufs: Vec<Vec<f32>> = vec![Vec::with_capacity(n); members.len()];
    for i in 0..n {
        for k in 0..members.len() {
            let v = {
                let env = Env {
                    vals: &vals,
                    names: &names,
                    ext: &ext,
                };
                tickers[k].out(i, &env)
            };
            vals[k] = v;
        }
        let env = Env {
            vals: &vals,
            names: &names,
            ext: &ext,
        };
        for &k in &delays {
            tickers[k].commit(i, &env);
        }
        for k in 0..members.len() {
            bufs[k].push(vals[k]);
        }
    }
    Ok(bufs
        .into_iter()
        .map(|b| std::sync::Arc::new(vec![("out", std::sync::Arc::new(Buf::Audio(b)))]))
        .collect())
}
