//! Named document parameters. A node field may be `"$cutoff"` (the
//! parameter's value) or an expression `"=$cutoff / 2 + 100"` (`+ - * /`,
//! parentheses, numbers, `$names`, plus the functions, comparisons and `?:` of
//! [`crate::calc`]; a leading `$` also starts an expression).
//! The document keeps each node's unresolved JSON (its template), so changing
//! a parameter re-resolves every node that reads it. Copied from modeler.

use crate::calc::{self, Clock, Scope};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub type Params = BTreeMap<String, f64>;

/// True when `s` is a parameter reference or expression.
fn is_expr(s: &str) -> bool {
    s.starts_with('$') || s.starts_with('=')
}

/// Keys of a node object whose strings are expression source for the node's
/// own compiler (`expr`'s `out` and `update`), never `$param` / `=expr`
/// fields: they are written without a leading `$` or `=`.
fn expr_text_keys(map: &serde_json::Map<String, Value>) -> &'static [&'static str] {
    if map.get("type").and_then(Value::as_str) == Some("expr") {
        &["out", "update"]
    } else {
        &[]
    }
}

/// `v` with every reference/expression replaced by its number.
pub fn resolve(v: &Value, params: &Params) -> Result<Value, String> {
    Ok(match v {
        Value::String(s) if is_expr(s) => {
            let x = eval(s.trim_start_matches('='), params)?;
            // Whole numbers come out as integers so counts (u32 fields) accept them.
            if x.fract() == 0.0 && x.abs() < 9.0e15 {
                Value::Number((x as i64).into())
            } else {
                serde_json::Number::from_f64(x)
                    .map(Value::Number)
                    .ok_or_else(|| format!("`{s}` is not a finite number ({x})"))?
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|x| resolve(x, params))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => {
            let skip = expr_text_keys(map);
            Value::Object(
                map.iter()
                    .map(|(k, x)| {
                        if skip.contains(&k.as_str()) {
                            Ok((k.clone(), x.clone()))
                        } else {
                            resolve(x, params).map(|r| (k.clone(), r))
                        }
                    })
                    .collect::<Result<_, _>>()?,
            )
        }
        other => other.clone(),
    })
}

/// Parameter names referenced anywhere in `v`.
pub fn refs(v: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect(v, &mut out);
    out
}

fn collect(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::String(s) if is_expr(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'$' {
                    let start = i + 1;
                    let mut j = start;
                    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                        j += 1;
                    }
                    out.insert(s[start..j].to_string());
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|x| collect(x, out)),
        Value::Object(map) => {
            let skip = expr_text_keys(map);
            map.iter()
                .filter(|(k, _)| !skip.contains(&k.as_str()))
                .for_each(|(_, x)| collect(x, out));
        }
        _ => {}
    }
}

/// Evaluates an expression over `params`: `+ - * /`, parentheses, numbers,
/// `$names`, `pi`, the functions of [`crate::calc`] (`sin`, `min`, `clamp`,
/// ...), comparisons, `&&`, `||`, `!` and `cond ? a : b`.
pub fn eval(src: &str, params: &Params) -> Result<f64, String> {
    let scope = Scope {
        vars: &[],
        consts: &[("pi", std::f64::consts::PI)],
        params: Some(params),
        noise: false,
    };
    let prog = calc::compile(src, &scope).map_err(|e| format!("{} in `{src}`", e.at()))?;
    let mut stack = vec![0.0; prog.depth()];
    Ok(prog.run(&mut stack, &[], Clock::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_refs_and_expressions() {
        let p: Params = [("h".to_string(), 2.0), ("w".to_string(), 0.5)].into();
        assert_eq!(eval("$h / 2 + 0.1", &p), Ok(1.1));
        assert_eq!(eval("-($h - $w) * 2", &p), Ok(-3.0));
        let v = json!({"size": [0.8, "$h", 0], "translation": [0, "=$h/2", 0], "name": "t"});
        assert_eq!(
            resolve(&v, &p),
            Ok(json!({"size": [0.8, 2, 0], "translation": [0, 1, 0], "name": "t"}))
        );
        assert_eq!(
            refs(&v),
            ["h".to_string()].into_iter().collect::<BTreeSet<_>>()
        );
        assert!(eval("$nope", &p).is_err_and(|e| e.contains("unknown parameter")));
        // Integral results stay integers (for counts).
        assert_eq!(resolve(&json!("=$h * 6"), &p), Ok(json!(12)));
    }

    #[test]
    fn expressions_have_functions_comparisons_and_ternary() {
        let p: Params = [("h".to_string(), 2.0), ("w".to_string(), 0.5)].into();
        assert_eq!(eval("max($h, 3) + min($h, $w)", &p), Ok(3.5));
        assert_eq!(eval("$h > 1 ? $h * 10 : 0", &p), Ok(20.0));
        assert_eq!(eval("clamp($h * 4, 0, 5)", &p), Ok(5.0));
        assert_eq!(eval("$h > 1 && $w < 1", &p), Ok(1.0));
        assert_eq!(eval("pow(2, 3) * sqrt(4)", &p), Ok(16.0));
        assert_eq!(resolve(&json!("=$h >= 2 ? 7 : 8"), &p), Ok(json!(7)));
        let e = eval("$h + sin(", &p).unwrap_err();
        assert!(e.contains("at ") && e.contains("$h + sin("), "{e}");
        assert!(eval("noise(1)", &p).is_err());
    }

    #[test]
    fn expr_nodes_keep_their_expression_text() {
        let p: Params = [("h".to_string(), 2.0)].into();
        let v = json!({"type": "expr", "name": "e", "inputs": {"a": "src"},
            "state": {"y1": "=$h"}, "out": "a * 2", "update": {"y1": "$a + y1"}});
        let r = resolve(&v, &p).unwrap();
        assert_eq!(r["out"], "a * 2");
        assert_eq!(r["update"]["y1"], "$a + y1");
        assert_eq!(r["state"]["y1"], json!(2));
        assert!(
            refs(&v).len() == 1 && refs(&v).contains("h"),
            "{:?}",
            refs(&v)
        );
    }
}
