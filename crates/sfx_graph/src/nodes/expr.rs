//! `expr`: per-sample maths with state, compiled once to bytecode.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::mono;
use super::one;
use crate::buf::Sig;
use crate::calc::{self, CalcError, Clock, Program, Scope};
use crate::node::{Cx, Eval, In, Outs};

/// Per-sample maths. `out` is the sample; `update` assigns new state after
/// each sample (every update reads the OLD state). Written without a leading
/// `=` or `$`. Variables: `t` (seconds), `sr`, `pi`, the `inputs` names (read
/// mono at this sample) and the `state` names. Functions: sin cos tan tanh
/// exp log pow sqrt abs min max floor fract clamp mix step noise(seed);
/// operators + - * / < <= > >= == != && || ! and `c ? a : b`. A non-finite
/// result (log of a negative, x / 0) becomes 0, in `out` and in state.
#[derive(Clone, Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expr {
    /// Named signals this node reads: {"a": "node", ...}.
    #[serde(default)]
    pub inputs: BTreeMap<String, In>,
    /// State variables and their starting values: {"y1": 0}.
    #[serde(default)]
    pub state: BTreeMap<String, f64>,
    /// The output sample, e.g. "tanh(a * 3)".
    pub out: String,
    /// New state values after each sample: {"y1": "y1 + 0.1 * (a - y1)"}.
    #[serde(default)]
    pub update: BTreeMap<String, String>,
    /// Extra seed for `noise` (mixed with the document seed and node name).
    #[serde(default)]
    pub seed: u64,
}

/// A compiled `expr` body: bytecode for `out` and each state update.
#[derive(Debug, Clone)]
pub struct ExprProg {
    out: Program,
    /// (state index, program)
    updates: Vec<(usize, Program)>,
    init: Vec<f64>,
    n_inputs: usize,
    depth: usize,
}

/// Names that variables may not take.
const RESERVED: &[&str] = &["t", "sr", "pi"];

fn ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

fn fail(field: &str, src: &str, e: CalcError) -> String {
    format!("{field} `{src}`: {}", e.at())
}

impl ExprProg {
    /// Compiles `out` / `update` over `inputs` (names, in the order
    /// [`ExprState::tick`] is given their values) and the `state` names with
    /// their start values. Errors name the field, the expression and the
    /// position.
    pub fn compile(
        inputs: &[&str],
        state: &BTreeMap<String, f64>,
        out: &str,
        update: &BTreeMap<String, String>,
        sr: f64,
    ) -> Result<ExprProg, String> {
        let state_names: Vec<&str> = state.keys().map(String::as_str).collect();
        for (kind, names) in [("inputs", inputs), ("state", &state_names[..])] {
            for n in names {
                if !ident(n) {
                    return Err(format!("{kind}: name `{n}`: use letters, digits and _"));
                }
                if RESERVED.contains(n) {
                    return Err(format!(
                        "{kind}: name `{n}` is built in ({}): rename it",
                        RESERVED.join(", ")
                    ));
                }
            }
        }
        if let Some(n) = inputs.iter().find(|n| state.contains_key(**n)) {
            return Err(format!(
                "`{n}` is both an input and a state variable: rename one"
            ));
        }
        let mut vars: Vec<&str> = vec!["t"];
        vars.extend(inputs);
        vars.extend(&state_names);
        let consts = [("pi", std::f64::consts::PI), ("sr", sr)];
        let scope = Scope {
            vars: &vars,
            consts: &consts,
            params: None,
            noise: true,
        };
        let outp = calc::compile(out.strip_prefix('=').unwrap_or(out), &scope)
            .map_err(|e| fail("out", out, e))?;
        let mut depth = outp.depth();
        let mut updates = Vec::new();
        for (k, src) in update {
            let Some(idx) = state_names.iter().position(|n| n == k) else {
                return Err(format!(
                    "update: `{k}` is not a state variable (state: {}): declare it in `state` with a start value",
                    if state_names.is_empty() {
                        "none".to_string()
                    } else {
                        state_names.join(", ")
                    }
                ));
            };
            let p = calc::compile(src.strip_prefix('=').unwrap_or(src), &scope)
                .map_err(|e| fail(&format!("update.{k}"), src, e))?;
            depth = depth.max(p.depth());
            updates.push((idx, p));
        }
        Ok(ExprProg {
            out: outp,
            updates,
            init: state.values().copied().collect(),
            n_inputs: inputs.len(),
            depth,
        })
    }

    /// Fresh per-sample state for one run. `seed` is the node's seed
    /// ([`Cx::rng`]'s mix of document seed, node name and own seed).
    pub fn start(&self, seed: u64) -> ExprState {
        let mut vars = vec![0.0; 1 + self.n_inputs];
        vars.extend(&self.init);
        ExprState {
            vars,
            stack: vec![0.0; self.depth],
            next: vec![0.0; self.updates.len()],
            clock: Clock { seed, i: 0 },
            n_inputs: self.n_inputs,
        }
    }
}

/// The running state of one `expr`: the state variables plus scratch, all
/// preallocated. [`ExprState::tick`] allocates nothing.
#[derive(Debug, Clone)]
pub struct ExprState {
    /// t, inputs..., state...
    vars: Vec<f64>,
    stack: Vec<f64>,
    next: Vec<f64>,
    clock: Clock,
    n_inputs: usize,
}

#[inline]
fn finite(x: f64) -> f64 {
    if x.is_finite() { x } else { 0.0 }
}

impl ExprState {
    /// One sample: `out` at time `t` with `inputs` (one value per input name,
    /// in compile order), then the state update.
    #[inline]
    pub fn tick(&mut self, prog: &ExprProg, t: f64, inputs: &[f64]) -> f64 {
        self.vars[0] = t;
        self.vars[1..1 + self.n_inputs].copy_from_slice(inputs);
        let y = finite(prog.out.run(&mut self.stack, &self.vars, self.clock));
        for (slot, (_, p)) in self.next.iter_mut().zip(&prog.updates) {
            *slot = finite(p.run(&mut self.stack, &self.vars, self.clock));
        }
        for (v, (idx, _)) in self.next.iter().zip(&prog.updates) {
            self.vars[1 + self.n_inputs + idx] = *v;
        }
        self.clock.i += 1;
        y
    }
}

fn mentions_noise(e: &Expr) -> bool {
    e.out.contains("noise") || e.update.values().any(|u| u.contains("noise"))
}

impl Eval for Expr {
    fn random(&self) -> bool {
        mentions_noise(self)
    }
    fn eval(&self, cx: &Cx) -> Result<Outs, String> {
        let names: Vec<&str> = self.inputs.keys().map(String::as_str).collect();
        let prog = ExprProg::compile(&names, &self.state, &self.out, &self.update, cx.sr as f64)?;
        let sigs: Vec<Sig> = self
            .inputs
            .iter()
            .map(|(k, i)| cx.mono(&format!("inputs.{k}"), i))
            .collect::<Result<_, _>>()?;
        let mut st = prog.start(crate::mix_seed(cx.seed, self.seed));
        let mut vals = vec![0.0f64; sigs.len()];
        let sr = cx.sr as f64;
        Ok(one(mono(cx.n, |i| {
            for (v, s) in vals.iter_mut().zip(&sigs) {
                *v = s.at(i) as f64;
            }
            st.tick(&prog, i as f64 / sr, &vals) as f32
        })))
    }
}
