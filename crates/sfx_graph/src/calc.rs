//! One expression language for `"=..."` field expressions ([`crate::params`])
//! and the `expr` node's per-sample maths.
//!
//! Source text is compiled ONCE to flat stack bytecode ([`Program`]) and run
//! with [`Program::run`]: no parsing, allocation or hash lookups per sample.
//! All maths is f64.
//!
//! Grammar, loosest first: `c ? a : b`, `||`, `&&`, `== !=`, `< <= > >=`,
//! `+ -`, `* /`, unary `- !`, then numbers, names, `$param`, `f(args)` and
//! parentheses. Comparisons and logic give 1 or 0; a condition is true when
//! it is not 0. `?:`, `&&` and `||` evaluate both sides (no side effects, so
//! only `noise` in the untaken side costs a hash).
//!
//! Division is IEEE (`x / 0` is inf); the `expr` node turns non-finite
//! results into 0.

use std::collections::BTreeMap;

/// A compile error: what went wrong and where (byte offset into the source).
#[derive(Debug, Clone, PartialEq)]
pub struct CalcError {
    pub pos: usize,
    pub msg: String,
    /// What would have worked, e.g. ` (names: a, b)`; empty when nothing.
    pub hint: String,
}

impl CalcError {
    /// `msg at <pos> (hint)`, the form the `expr` node reports.
    pub fn at(&self) -> String {
        format!("{} at {}{}", self.msg, self.pos, self.hint)
    }
}

/// Where a program may read names from.
pub struct Scope<'a> {
    /// Per-run variables (slot = index into the `vars` slice given to `run`).
    pub vars: &'a [&'a str],
    /// Compile-time constants (`pi`, `sr`).
    pub consts: &'a [(&'a str, f64)],
    /// `$name` values (document parameters), when allowed.
    pub params: Option<&'a BTreeMap<String, f64>>,
    /// `noise()` needs a sample clock: only the `expr` node has one.
    pub noise: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Const(f64),
    Var(u16),
    Neg,
    Not,
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    /// (cond, a, b) -> a if cond != 0 else b.
    Select,
    Sin,
    Cos,
    Tan,
    Tanh,
    Exp,
    Log,
    Sqrt,
    Abs,
    Floor,
    Fract,
    Pow,
    Min,
    Max,
    Clamp,
    Mix,
    Step,
    Noise,
}

/// (name, args, op)
const FUNCS: &[(&str, usize, Op)] = &[
    ("sin", 1, Op::Sin),
    ("cos", 1, Op::Cos),
    ("tan", 1, Op::Tan),
    ("tanh", 1, Op::Tanh),
    ("exp", 1, Op::Exp),
    ("log", 1, Op::Log),
    ("sqrt", 1, Op::Sqrt),
    ("abs", 1, Op::Abs),
    ("floor", 1, Op::Floor),
    ("fract", 1, Op::Fract),
    ("pow", 2, Op::Pow),
    ("min", 2, Op::Min),
    ("max", 2, Op::Max),
    ("clamp", 3, Op::Clamp),
    ("mix", 3, Op::Mix),
    ("step", 2, Op::Step),
    ("noise", 1, Op::Noise),
];

/// The function names, for error messages and docs.
pub fn function_names() -> String {
    FUNCS.iter().map(|f| f.0).collect::<Vec<_>>().join(" ")
}

/// Per-run context: the sample clock behind `noise`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Clock {
    /// The node's seed (document seed mixed with the node's name and `seed`).
    pub seed: u64,
    /// Sample index.
    pub i: u64,
}

fn noise(seed_arg: f64, c: Clock) -> f64 {
    let h = crate::mix_seed(crate::mix_seed(c.seed, seed_arg.to_bits()), c.i);
    ((h >> 11) as f64 / (1u64 << 52) as f64) - 1.0
}

#[inline]
fn b(x: bool) -> f64 {
    if x { 1.0 } else { 0.0 }
}

/// Compiled bytecode.
#[derive(Debug, Clone)]
pub struct Program {
    code: Vec<Op>,
    /// Stack slots `run` needs.
    depth: usize,
}

impl Program {
    /// Stack slots to give [`Program::run`].
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The value if the program is a constant.
    pub fn constant(&self) -> Option<f64> {
        match self.code[..] {
            [Op::Const(x)] => Some(x),
            _ => None,
        }
    }

    /// Runs the program. `stack` must hold at least [`Program::depth`] slots
    /// (scratch; contents are not meaningful between runs) and `vars` the
    /// scope's variables.
    #[inline]
    pub fn run(&self, stack: &mut [f64], vars: &[f64], clock: Clock) -> f64 {
        let mut sp = 0usize;
        macro_rules! un {
            ($f:expr) => {{
                let x = stack[sp - 1];
                stack[sp - 1] = $f(x);
            }};
        }
        macro_rules! bin {
            ($f:expr) => {{
                sp -= 1;
                let (x, y) = (stack[sp - 1], stack[sp]);
                stack[sp - 1] = $f(x, y);
            }};
        }
        for op in &self.code {
            match *op {
                Op::Const(x) => {
                    stack[sp] = x;
                    sp += 1;
                }
                Op::Var(k) => {
                    stack[sp] = vars[k as usize];
                    sp += 1;
                }
                Op::Neg => un!(|x: f64| -x),
                Op::Not => un!(|x: f64| b(x == 0.0)),
                Op::Add => bin!(|x: f64, y: f64| x + y),
                Op::Sub => bin!(|x: f64, y: f64| x - y),
                Op::Mul => bin!(|x: f64, y: f64| x * y),
                Op::Div => bin!(|x: f64, y: f64| x / y),
                Op::Lt => bin!(|x: f64, y: f64| b(x < y)),
                Op::Le => bin!(|x: f64, y: f64| b(x <= y)),
                Op::Gt => bin!(|x: f64, y: f64| b(x > y)),
                Op::Ge => bin!(|x: f64, y: f64| b(x >= y)),
                Op::Eq => bin!(|x: f64, y: f64| b(x == y)),
                Op::Ne => bin!(|x: f64, y: f64| b(x != y)),
                Op::And => bin!(|x: f64, y: f64| b(x != 0.0 && y != 0.0)),
                Op::Or => bin!(|x: f64, y: f64| b(x != 0.0 || y != 0.0)),
                Op::Select => {
                    sp -= 2;
                    let (c, x, y) = (stack[sp - 1], stack[sp], stack[sp + 1]);
                    stack[sp - 1] = if c != 0.0 { x } else { y };
                }
                Op::Sin => un!(f64::sin),
                Op::Cos => un!(f64::cos),
                Op::Tan => un!(f64::tan),
                Op::Tanh => un!(f64::tanh),
                Op::Exp => un!(f64::exp),
                Op::Log => un!(f64::ln),
                Op::Sqrt => un!(f64::sqrt),
                Op::Abs => un!(f64::abs),
                Op::Floor => un!(f64::floor),
                Op::Fract => un!(|x: f64| x - x.floor()),
                Op::Pow => bin!(f64::powf),
                Op::Min => bin!(f64::min),
                Op::Max => bin!(f64::max),
                Op::Step => bin!(|edge: f64, x: f64| b(x >= edge)),
                Op::Clamp => {
                    sp -= 2;
                    let (x, lo, hi) = (stack[sp - 1], stack[sp], stack[sp + 1]);
                    stack[sp - 1] = x.max(lo).min(hi.max(lo));
                }
                Op::Mix => {
                    sp -= 2;
                    let (x, y, t) = (stack[sp - 1], stack[sp], stack[sp + 1]);
                    stack[sp - 1] = x + (y - x) * t;
                }
                Op::Noise => un!(|s: f64| noise(s, clock)),
            }
        }
        stack[0]
    }
}

/// Compiles `src` against `scope`.
pub fn compile(src: &str, scope: &Scope) -> Result<Program, CalcError> {
    let mut p = Compiler {
        src,
        s: src.as_bytes(),
        i: 0,
        scope,
        code: Vec::new(),
        depth: 0,
        max: 0,
    };
    p.ternary()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err_here(format!("unexpected `{}`", &src[p.i..])));
    }
    Ok(Program {
        code: p.code,
        depth: p.max.max(1),
    })
}

struct Compiler<'a> {
    src: &'a str,
    s: &'a [u8],
    i: usize,
    scope: &'a Scope<'a>,
    code: Vec<Op>,
    depth: usize,
    max: usize,
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

impl Compiler<'_> {
    fn err_here(&self, msg: String) -> CalcError {
        CalcError {
            pos: self.i,
            msg,
            hint: String::new(),
        }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    /// Consumes `tok` (after whitespace) if it is next.
    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.src[self.i..].starts_with(tok) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }
    /// Like `eat` but does not take `tok` when it is the start of a longer
    /// operator in `longer` (`<` vs `<=`, `!` vs `!=`).
    fn eat_not_followed(&mut self, tok: &str, longer: &str) -> bool {
        self.ws();
        let rest = &self.src[self.i..];
        if rest.starts_with(tok) && !rest.starts_with(longer) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    fn push(&mut self, n: usize) {
        self.depth += n;
        self.max = self.max.max(self.depth);
    }

    fn emit_const(&mut self, x: f64) {
        self.code.push(Op::Const(x));
        self.push(1);
    }

    /// Emits an operator taking `n` operands, folding it when they are all
    /// constants (never `noise`, which reads the clock).
    fn emit(&mut self, op: Op, n: usize) {
        let len = self.code.len();
        let foldable = op != Op::Noise
            && len >= n
            && self.code[len - n..]
                .iter()
                .all(|o| matches!(o, Op::Const(_)));
        self.depth = self.depth + 1 - n;
        if foldable {
            let mut tmp = Program {
                code: self.code[len - n..].to_vec(),
                depth: n,
            };
            tmp.code.push(op);
            let mut stack = [0.0; 4];
            let v = tmp.run(&mut stack, &[], Clock::default());
            self.code.truncate(len - n);
            self.code.push(Op::Const(v));
        } else {
            self.code.push(op);
        }
    }

    fn ternary(&mut self) -> Result<(), CalcError> {
        self.or()?;
        if self.eat("?") {
            self.ternary()?;
            if !self.eat(":") {
                return Err(self.err_here("expected `:` in `cond ? a : b`".into()));
            }
            self.ternary()?;
            self.emit(Op::Select, 3);
        }
        Ok(())
    }
    fn or(&mut self) -> Result<(), CalcError> {
        self.and()?;
        while self.eat("||") {
            self.and()?;
            self.emit(Op::Or, 2);
        }
        Ok(())
    }
    fn and(&mut self) -> Result<(), CalcError> {
        self.equality()?;
        while self.eat("&&") {
            self.equality()?;
            self.emit(Op::And, 2);
        }
        Ok(())
    }
    fn equality(&mut self) -> Result<(), CalcError> {
        self.comparison()?;
        loop {
            let op = if self.eat("==") {
                Op::Eq
            } else if self.eat("!=") {
                Op::Ne
            } else {
                return Ok(());
            };
            self.comparison()?;
            self.emit(op, 2);
        }
    }
    fn comparison(&mut self) -> Result<(), CalcError> {
        self.additive()?;
        loop {
            let op = if self.eat("<=") {
                Op::Le
            } else if self.eat(">=") {
                Op::Ge
            } else if self.eat("<") {
                Op::Lt
            } else if self.eat(">") {
                Op::Gt
            } else {
                return Ok(());
            };
            self.additive()?;
            self.emit(op, 2);
        }
    }
    fn additive(&mut self) -> Result<(), CalcError> {
        self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            self.term()?;
            self.emit(if c == b'+' { Op::Add } else { Op::Sub }, 2);
        }
        Ok(())
    }
    fn term(&mut self) -> Result<(), CalcError> {
        self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            self.unary()?;
            self.emit(if c == b'*' { Op::Mul } else { Op::Div }, 2);
        }
        Ok(())
    }
    fn unary(&mut self) -> Result<(), CalcError> {
        if self.eat("-") {
            self.unary()?;
            self.emit(Op::Neg, 1);
            Ok(())
        } else if self.eat_not_followed("!", "!=") {
            self.unary()?;
            self.emit(Op::Not, 1);
            Ok(())
        } else {
            self.primary()
        }
    }

    fn names(&self) -> String {
        let mut v: Vec<&str> = self.scope.vars.to_vec();
        v.extend(self.scope.consts.iter().map(|c| c.0));
        v.join(", ")
    }

    fn primary(&mut self) -> Result<(), CalcError> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                self.ternary()?;
                if self.peek() != Some(b')') {
                    return Err(self.err_here("missing `)`".into()));
                }
                self.i += 1;
                Ok(())
            }
            Some(b'$') => {
                let at = self.i;
                self.i += 1;
                let start = self.i;
                while self.i < self.s.len() && ident_char(self.s[self.i]) {
                    self.i += 1;
                }
                let name = &self.src[start..self.i];
                let Some(params) = self.scope.params else {
                    return Err(CalcError {
                        pos: at,
                        msg: format!(
                            "`${name}`: document parameters are not available here; read a node through `inputs` instead"
                        ),
                        hint: String::new(),
                    });
                };
                match params.get(name) {
                    Some(&x) => {
                        self.emit_const(x);
                        Ok(())
                    }
                    None => {
                        let known: Vec<&str> = params.keys().map(String::as_str).collect();
                        Err(CalcError {
                            pos: at,
                            msg: format!("unknown parameter `${name}`"),
                            hint: format!(
                                " (known: {})",
                                if known.is_empty() {
                                    "none - set them with sound_ops params (or inputs)".to_string()
                                } else {
                                    known.join(", ")
                                }
                            ),
                        })
                    }
                }
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len() {
                    let c = self.s[self.i];
                    let exp_sign = (c == b'+' || c == b'-')
                        && self.i > start
                        && matches!(self.s[self.i - 1], b'e' | b'E')
                        && self.s[start..self.i - 1]
                            .iter()
                            .all(|d| d.is_ascii_digit() || *d == b'.');
                    if c.is_ascii_digit() || c == b'.' || c == b'e' || c == b'E' || exp_sign {
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                match self.src[start..self.i].parse::<f64>() {
                    Ok(x) => {
                        self.emit_const(x);
                        Ok(())
                    }
                    Err(_) => Err(CalcError {
                        pos: start,
                        msg: format!("bad number `{}`", &self.src[start..self.i]),
                        hint: String::new(),
                    }),
                }
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.i;
                while self.i < self.s.len() && ident_char(self.s[self.i]) {
                    self.i += 1;
                }
                let name = &self.src[start..self.i];
                if self.peek() == Some(b'(') {
                    return self.call(name, start);
                }
                if let Some(k) = self.scope.vars.iter().position(|v| *v == name) {
                    self.code.push(Op::Var(k as u16));
                    self.push(1);
                    Ok(())
                } else if let Some(c) = self.scope.consts.iter().find(|c| c.0 == name) {
                    self.emit_const(c.1);
                    Ok(())
                } else {
                    let hint = if FUNCS.iter().any(|f| f.0 == name) {
                        format!(" (`{name}` is a function: write `{name}(...)`)")
                    } else {
                        String::new()
                    };
                    Err(CalcError {
                        pos: start,
                        msg: format!("unknown name `{name}`{hint}"),
                        hint: format!(" (names: {})", self.names()),
                    })
                }
            }
            Some(c) => Err(self.err_here(format!(
                "expected a number, name or `(`, found `{}`",
                self.src[self.i..].chars().next().unwrap_or(c as char)
            ))),
            None => {
                Err(self.err_here("expression ends early: expected a number, name or `(`".into()))
            }
        }
    }

    fn call(&mut self, name: &str, start: usize) -> Result<(), CalcError> {
        let Some(&(_, argc, op)) = FUNCS.iter().find(|f| f.0 == name) else {
            return Err(CalcError {
                pos: start,
                msg: format!("unknown function `{name}`"),
                hint: format!(" (functions: {})", function_names()),
            });
        };
        if op == Op::Noise && !self.scope.noise {
            return Err(CalcError {
                pos: start,
                msg: "`noise` needs a sample clock: it is only available in an `expr` node".into(),
                hint: String::new(),
            });
        }
        self.i += 1; // (
        let mut got = 0;
        if self.peek() != Some(b')') {
            loop {
                self.ternary()?;
                got += 1;
                if !self.eat(",") {
                    break;
                }
            }
        }
        if self.peek() != Some(b')') {
            return Err(self.err_here(format!("missing `)` after the arguments of `{name}`")));
        }
        self.i += 1;
        if got != argc {
            return Err(CalcError {
                pos: start,
                msg: format!(
                    "`{name}` takes {argc} argument{}, got {got}",
                    if argc == 1 { "" } else { "s" }
                ),
                hint: String::new(),
            });
        }
        self.emit(op, argc);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, vars: &[(&str, f64)]) -> Result<f64, CalcError> {
        let names: Vec<&str> = vars.iter().map(|v| v.0).collect();
        let vals: Vec<f64> = vars.iter().map(|v| v.1).collect();
        let sc = Scope {
            vars: &names,
            consts: &[("pi", std::f64::consts::PI)],
            params: None,
            noise: true,
        };
        let p = compile(src, &sc)?;
        let mut st = vec![0.0; p.depth()];
        Ok(p.run(&mut st, &vals, Clock { seed: 7, i: 3 }))
    }

    fn v(src: &str) -> f64 {
        run(src, &[("x", 0.5), ("y", -2.0)]).unwrap_or_else(|e| panic!("{src}: {}", e.at()))
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(v("1 + 2 * 3"), 7.0);
        assert_eq!(v("(1 + 2) * 3"), 9.0);
        assert_eq!(v("-x * 4"), -2.0);
        assert_eq!(v("10 - 4 - 3"), 3.0);
        assert_eq!(v("8 / 4 / 2"), 1.0);
        assert_eq!(v("1e-3 * 1000"), 1.0);
        assert_eq!(v("pi"), std::f64::consts::PI);
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(v("x < 1"), 1.0);
        assert_eq!(v("x <= 0.5"), 1.0);
        assert_eq!(v("x > 0.5"), 0.0);
        assert_eq!(v("x >= 0.5"), 1.0);
        assert_eq!(v("x == 0.5"), 1.0);
        assert_eq!(v("x != 0.5"), 0.0);
        assert_eq!(v("x > 0 && y < 0"), 1.0);
        assert_eq!(v("x < 0 || y < 0"), 1.0);
        assert_eq!(v("x < 0 || y > 0"), 0.0);
        assert_eq!(v("!0"), 1.0);
        assert_eq!(v("!x"), 0.0);
        assert_eq!(v("!x == 0"), 1.0, "! binds tighter than ==");
        assert_eq!(v("1 + 1 == 2"), 1.0, "arithmetic binds tighter than ==");
    }

    #[test]
    fn ternary_is_right_associative_and_loosest() {
        assert_eq!(v("x > 0 ? 10 : 20"), 10.0);
        assert_eq!(v("x < 0 ? 10 : 20"), 20.0);
        assert_eq!(v("y > 0 ? 1 : x > 0 ? 2 : 3"), 2.0);
        assert_eq!(v("x > 0 ? y < 0 ? 1 : 2 : 3"), 1.0);
        assert_eq!(v("1 + (x > 0 ? 1 : 0) * 2"), 3.0);
        // The untaken side may be NaN.
        assert_eq!(v("y > 0 ? log(y) : 4"), 4.0);
    }

    #[test]
    fn every_function() {
        let close = |src: &str, want: f64| {
            let got = v(src);
            assert!((got - want).abs() < 1e-12, "{src}: {got} vs {want}");
        };
        close("sin(pi / 2)", 1.0);
        close("cos(0)", 1.0);
        close("tan(pi / 4)", 1.0);
        close("tanh(0.5)", 0.5f64.tanh());
        close("exp(1)", std::f64::consts::E);
        close("log(exp(2))", 2.0);
        close("pow(2, 10)", 1024.0);
        close("pow(x, 2)", 0.25);
        close("sqrt(16)", 4.0);
        close("abs(y)", 2.0);
        close("min(x, y)", -2.0);
        close("max(x, y)", 0.5);
        close("floor(2.7)", 2.0);
        close("floor(-2.5)", -3.0);
        close("fract(2.75)", 0.75);
        close("fract(-0.25)", 0.75);
        close("clamp(5, 0, 1)", 1.0);
        close("clamp(-5, 0, 1)", 0.0);
        close("clamp(x, 0, 1)", 0.5);
        close("mix(10, 20, 0.25)", 12.5);
        close("step(0.5, 0.4)", 0.0);
        close("step(0.5, 0.5)", 1.0);
    }

    #[test]
    fn noise_is_deterministic_bounded_and_varies() {
        let sc = Scope {
            vars: &[],
            consts: &[],
            params: None,
            noise: true,
        };
        let p = compile("noise(1)", &sc).unwrap();
        let mut st = vec![0.0; p.depth()];
        let mut run = |seed, i| p.run(&mut st, &[], Clock { seed, i });
        let xs: Vec<f64> = (0..1000).map(|i| run(5, i)).collect();
        assert!(xs.iter().all(|x| (-1.0..1.0).contains(x)));
        assert!(xs.windows(2).any(|w| w[0] != w[1]));
        let mean = xs.iter().sum::<f64>() / 1000.0;
        assert!(mean.abs() < 0.1, "mean {mean}");
        assert_eq!(run(5, 17), xs[17]);
        assert_ne!(run(6, 17), xs[17], "a different node seed differs");
        let q = compile("noise(2)", &sc).unwrap();
        assert_ne!(q.run(&mut st, &[], Clock { seed: 5, i: 17 }), xs[17]);
    }

    #[test]
    fn constants_fold() {
        let sc = Scope {
            vars: &["x"],
            consts: &[],
            params: None,
            noise: true,
        };
        assert_eq!(compile("2 * 3 + 1", &sc).unwrap().constant(), Some(7.0));
        assert_eq!(compile("1 < 2 ? 4 : 5", &sc).unwrap().constant(), Some(4.0));
        assert_eq!(compile("x * 3 + 1", &sc).unwrap().constant(), None);
    }

    #[test]
    fn errors_have_positions() {
        let e = |src: &str| run(src, &[("a", 1.0), ("y1", 0.0)]).unwrap_err();
        let u = e("a + foo");
        assert_eq!(u.pos, 4);
        assert!(u.msg.contains("unknown name `foo`"), "{u:?}");
        assert!(u.hint.contains("names: a, y1, pi"), "{u:?}");
        assert!(e("sin(a").msg.contains("missing `)`"));
        assert!(e("pow(a)").msg.contains("takes 2 arguments, got 1"));
        assert!(e("blah(a)").msg.contains("unknown function `blah`"));
        assert_eq!(e("a +").pos, 3);
        assert!(e("a b").msg.contains("unexpected `b`"));
        assert_eq!(e("a ? 1").pos, 5);
        assert!(e("sin").msg.contains("is a function"));
        assert!(e("$p").msg.contains("not available"));
        let sc = Scope {
            vars: &[],
            consts: &[],
            params: None,
            noise: false,
        };
        assert!(compile("noise(1)", &sc).unwrap_err().msg.contains("expr"));
    }
}
