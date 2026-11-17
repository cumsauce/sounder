//! Evaluates a sound graph over whole buffers.
//!
//! Nodes run in dependency *levels*: every node in a level reads only
//! earlier levels, so a level's nodes run in parallel (rayon); each node is
//! itself a sequential loop, so results never depend on the thread count.
//! Results are memoized in a process-wide cache keyed by a Merkle hash (type,
//! resolved body, the keys of the nodes it reads, the length and sample rate,
//! the seed for random nodes, file mtimes), capped by bytes and evicting the
//! least recently used. Follows modeler's `src/material/eval.rs`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use serde_json::Value;

use crate::buf::{Buf, Sig};
use crate::node::{self, Cx, In, Node};
use crate::params::{self, Params};

/// Where and how long a graph is evaluated.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub sample_rate: u32,
    /// Samples every audio buffer has.
    pub n: usize,
    /// The document seed.
    pub seed: u64,
    pub looped: bool,
    /// Relative file paths resolve here.
    pub base_dir: Option<PathBuf>,
    /// The document's params (a `voices` patch reads them as defaults).
    pub params: Params,
    /// How `sound` nodes reach other documents.
    pub sounds: Option<crate::instance::SoundSource>,
    /// The documents being rendered, outermost first (instance cycle guard).
    pub stack: Vec<String>,
}

impl Default for Ctx {
    fn default() -> Ctx {
        Ctx {
            sample_rate: 48_000,
            n: 48_000,
            seed: 1,
            looped: false,
            base_dir: None,
            params: Params::new(),
            sounds: None,
            stack: Vec::new(),
        }
    }
}

/// One parsed node of a graph.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub name: String,
    pub ty: String,
    pub node: Node,
    /// The resolved body (params substituted), for cache keys.
    pub body: Value,
    /// Node names this one reads.
    pub reads: Vec<String>,
}

/// Parses and checks a graph: names unique, types known, fields valid,
/// every reference names a node, no cycles. Returns nodes in list order.
pub fn parse(nodes: &[Value], params: &Params) -> Result<Vec<Parsed>, String> {
    let mut out: Vec<Parsed> = Vec::new();
    for (k, raw) in nodes.iter().enumerate() {
        let v = params::resolve(raw, params).map_err(|e| format!("node #{k}: {e}"))?;
        let Value::Object(mut m) = v else {
            return Err(format!(
                "node #{k}: expected an object with \"type\" and \"name\""
            ));
        };
        let ty = match m.remove("type") {
            Some(Value::String(s)) => s,
            _ => {
                return Err(format!(
                    "node #{k}: missing \"type\" (one of: {})",
                    node::NODE_TYPES.join(", ")
                ));
            }
        };
        let name = match m.remove("name") {
            Some(Value::String(s)) if valid_name(&s) => s,
            Some(Value::String(s)) => {
                return Err(format!(
                    "node #{k} ({ty}): name `{s}`: use letters, digits and _ (no '.', no leading '$' or '=', not ending in .wav)"
                ));
            }
            _ => return Err(format!("node #{k} ({ty}): missing \"name\"")),
        };
        if out.iter().any(|p| p.name == name) {
            return Err(format!(
                "node #{k} ({ty} \"{name}\"): another node is already named `{name}`"
            ));
        }
        let body = Value::Object(m);
        let ctx = format!("node #{k} ({ty} \"{name}\")");
        let node = node::parse_body(&ty, &body, &ctx)?;
        let mut reads: Vec<String> = node::refs(&body).into_iter().map(|(n, _)| n).collect();
        reads.sort();
        reads.dedup();
        out.push(Parsed {
            name,
            ty,
            node,
            body,
            reads,
        });
    }
    let outputs: Vec<&str> = out
        .iter()
        .filter(|p| p.ty == "output")
        .map(|p| p.name.as_str())
        .collect();
    if outputs.len() > 1 {
        return Err(format!(
            "more than one output node ({}): a sound has exactly one; mix the others into it",
            outputs.join(", ")
        ));
    }
    for p in &out {
        for r in &p.reads {
            if !out.iter().any(|q| &q.name == r) {
                let names: Vec<&str> = out.iter().map(|q| q.name.as_str()).collect();
                let near = names
                    .iter()
                    .min_by_key(|n| node::edit_distance(r, n))
                    .filter(|n| node::edit_distance(r, n) <= 2)
                    .map(|n| format!(" (did you mean `{n}`?)"))
                    .unwrap_or_default();
                return Err(format!(
                    "node `{}` ({}): reads `{r}`, which is not a node{near} (nodes: {}; files end in .wav)",
                    p.name,
                    p.ty,
                    names.join(", ")
                ));
            }
        }
    }
    levels(&out)?;
    Ok(out)
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('.')
        && !s.starts_with('$')
        && !s.starts_with('=')
        && s != "mono"
        && s != "stereo"
}

/// Dependency levels: level 0 reads nothing; each node sits one level after
/// the deepest node it reads. A feedback loop (legal only through a `delay`,
/// see [`crate::feedback`]) is one unit: its members share a level.
pub fn levels(nodes: &[Parsed]) -> Result<Vec<Vec<usize>>, String> {
    let loops = crate::feedback::find_loops(nodes)?;
    let max = loops.depth.iter().max().copied().unwrap_or(0);
    let mut lv = vec![Vec::new(); if nodes.is_empty() { 0 } else { max + 1 }];
    for (i, d) in loops.depth.iter().enumerate() {
        lv[*d].push(i);
    }
    Ok(lv)
}

/// A node's outputs: (port, buffer), the first being the default.
pub type Ports = Arc<Vec<(&'static str, Arc<Buf>)>>;

/// The result of evaluating a graph.
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub nodes: Vec<Parsed>,
    pub ports: HashMap<String, Ports>,
    /// Nodes actually computed (cache misses) by this evaluation.
    pub runs: usize,
}

impl Evaluated {
    /// The output node's name, if the graph has one.
    pub fn output_name(&self) -> Option<&str> {
        self.nodes
            .iter()
            .find(|p| p.ty == "output")
            .map(|p| p.name.as_str())
    }

    /// A node's default (or named) port.
    pub fn buf(&self, name: &str) -> Result<Arc<Buf>, String> {
        match resolve(&self.ports, &In::Name(name.into()))? {
            Sig::Buf(b) => Ok(b),
            _ => Err(format!("`{name}` is not a node")),
        }
    }
}

pub(crate) fn resolve(ports: &HashMap<String, Ports>, i: &In) -> Result<Sig, String> {
    match i {
        In::Num(x) => Ok(Sig::Const(*x)),
        In::Vec(v) => match v.as_slice() {
            [x] => Ok(Sig::Const(*x)),
            [l, r] => Ok(Sig::ConstStereo([*l, *r])),
            _ => Err(format!("{v:?}: give a number or a stereo pair [l, r]")),
        },
        In::Name(s) => {
            let (n, p) = s.split_once('.').unwrap_or((s, ""));
            let Some(outs) = ports.get(n) else {
                return Err(format!("`{n}` is not a node"));
            };
            if outs.is_empty() {
                return Err(format!("`{n}` has no outputs to read"));
            }
            if p.is_empty() {
                return Ok(Sig::Buf(outs[0].1.clone()));
            }
            match outs.iter().find(|(q, _)| *q == p) {
                Some((_, b)) => Ok(Sig::Buf(b.clone())),
                None => {
                    let names: Vec<&str> = outs.iter().map(|(q, _)| *q).collect();
                    Err(format!(
                        "`{n}` has no port `{p}` (ports: {})",
                        names.join(", ")
                    ))
                }
            }
        }
    }
}

// ------------------------------------------------------------------ cache

/// Cached node results are capped at this many bytes (LRU eviction).
const CACHE_BYTES: usize = 512 << 20;

struct Entry {
    ports: Ports,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct Cache {
    map: HashMap<u64, Entry>,
    bytes: usize,
    clock: u64,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn cache_get(k: u64) -> Option<Ports> {
    let mut g = CACHE.lock().ok()?;
    let c = g.as_mut()?;
    c.clock += 1;
    let now = c.clock;
    let e = c.map.get_mut(&k)?;
    e.used = now;
    Some(e.ports.clone())
}

fn cache_put(k: u64, v: Ports) {
    let bytes: usize = v.iter().map(|(_, b)| b.bytes()).sum();
    if bytes > CACHE_BYTES / 4 {
        return;
    }
    if let Ok(mut g) = CACHE.lock() {
        let c = g.get_or_insert_with(Cache::default);
        c.clock += 1;
        while c.bytes + bytes > CACHE_BYTES {
            let Some(old) = c.map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k) else {
                break;
            };
            if let Some(e) = c.map.remove(&old) {
                c.bytes -= e.bytes;
            }
        }
        let used = c.clock;
        if let Some(e) = c.map.insert(
            k,
            Entry {
                ports: v,
                bytes,
                used,
            },
        ) {
            c.bytes -= e.bytes;
        }
        c.bytes += bytes;
    }
}

/// Drops every cached node result (tests, memory pressure).
pub fn clear_cache() {
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

/// The seed a node sees: the document seed mixed with its name.
pub fn node_seed(doc_seed: u64, name: &str) -> u64 {
    crate::mix_seed(doc_seed, crate::hash_str(name))
}

/// Evaluates `nodes` with `params`; `cached: false` bypasses the memo cache
/// (used to prove a cached result matches a fresh one).
pub fn evaluate(
    nodes: &[Value],
    params: &Params,
    ctx: &Ctx,
    cached: bool,
) -> Result<Evaluated, String> {
    let parsed = parse(nodes, params)?;
    let lv = levels(&parsed)?;
    let loops = crate::feedback::find_loops(&parsed)?;
    let mut keys: HashMap<String, u64> = HashMap::new();
    let mut ports: HashMap<String, Ports> = HashMap::new();
    let mut runs = 0usize;
    let sr = ctx.sample_rate as f32;
    for level in lv {
        // One job per node, or per loop (its first member stands for it).
        let level: Vec<usize> = level
            .into_iter()
            .filter(|&i| loops.of[i].is_none_or(|g| loops.groups[g][0] == i))
            .collect();
        let node_hash = |i: usize| {
            let p = &parsed[i];
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (
                p.ty.as_str(),
                p.body.to_string(),
                ctx.sample_rate,
                ctx.n,
                ctx.looped,
            )
                .hash(&mut h);
            if p.node.random() {
                (ctx.seed, p.name.as_str()).hash(&mut h);
            }
            // An instance also keys on the instanced document itself.
            if p.ty == "sound"
                && let (Some(src), Some(name)) =
                    (&ctx.sounds, p.body.get("sound").and_then(Value::as_str))
            {
                src.get(name)
                    .and_then(|d| serde_json::to_string(&d).ok())
                    .hash(&mut h);
            }
            // A file-reading node also keys on its file's path and mtime, so
            // editing the WAV re-renders.
            for f in ["file", "ir"] {
                if let Some(path) = p.body.get(f).and_then(Value::as_str)
                    && node::is_file(path)
                {
                    let full = match &ctx.base_dir {
                        Some(b) => b.join(path),
                        None => PathBuf::from(path),
                    };
                    full.hash(&mut h);
                    std::fs::metadata(&full)
                        .and_then(|m| m.modified())
                        .ok()
                        .hash(&mut h);
                }
            }
            h
        };
        // A job: the node indices it computes with their keys.
        let jobs: Vec<Vec<(usize, u64)>> = level
            .iter()
            .map(|&i| match loops.of[i] {
                None => {
                    let mut h = node_hash(i);
                    for r in &parsed[i].reads {
                        keys.get(r).copied().unwrap_or(0).hash(&mut h);
                    }
                    vec![(i, h.finish())]
                }
                Some(g) => {
                    // A loop's members key together: their own bodies plus
                    // every outside signal they read.
                    let members = &loops.groups[g];
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    for &m in members {
                        node_hash(m).finish().hash(&mut h);
                        parsed[m].name.hash(&mut h);
                        for r in &parsed[m].reads {
                            if !members.iter().any(|&o| parsed[o].name == *r) {
                                keys.get(r).copied().unwrap_or(0).hash(&mut h);
                            }
                        }
                    }
                    let base = h.finish();
                    members
                        .iter()
                        .map(|&m| {
                            let mut hm = std::collections::hash_map::DefaultHasher::new();
                            (base, parsed[m].name.as_str()).hash(&mut hm);
                            (m, hm.finish())
                        })
                        .collect()
                }
            })
            .collect();
        type Done = Vec<(usize, u64, Ports, bool)>;
        let done: Vec<Result<Done, String>> = jobs
            .par_iter()
            .map(|job| -> Result<Done, String> {
                if cached {
                    let hits: Vec<Option<Ports>> = job.iter().map(|&(_, k)| cache_get(k)).collect();
                    if hits.iter().all(Option::is_some) {
                        return Ok(job
                            .iter()
                            .zip(hits)
                            .filter_map(|(&(i, k), h)| h.map(|h| (i, k, h, false)))
                            .collect());
                    }
                }
                let first = job[0].0;
                if loops.of[first].is_some() {
                    let members: Vec<usize> = job.iter().map(|&(i, _)| i).collect();
                    let outs = crate::feedback::eval_loop(&members, &parsed, &ports, sr, ctx.n, ctx.seed)?;
                    return Ok(job
                        .iter()
                        .zip(outs)
                        .map(|(&(i, k), o)| {
                            if cached {
                                cache_put(k, o.clone());
                            }
                            (i, k, o, true)
                        })
                        .collect());
                }
                let (i, k) = job[0];
                let p = &parsed[i];
                let get = |x: &In| resolve(&ports, x);
                let cx = Cx {
                    sr,
                    n: ctx.n,
                    seed: node_seed(ctx.seed, &p.name),
                    looped: ctx.looped,
                    base_dir: ctx.base_dir.as_deref(),
                    params: &ctx.params,
                    sounds: ctx.sounds.as_ref(),
                    stack: &ctx.stack,
                    get: &get,
                };
                let outs = p
                    .node
                    .eval(&cx)
                    .map_err(|e| format!("node `{}` ({}): {e}", p.name, p.ty))?;
                for (port, b) in &outs {
                    if !matches!(b, Buf::Events(_)) && b.len() != ctx.n {
                        return Err(format!(
                            "node `{}` ({}): port `{port}` made {} samples, expected {} (a bug in the node: report it)",
                            p.name,
                            p.ty,
                            b.len(),
                            ctx.n
                        ));
                    }
                }
                let outs: Ports =
                    Arc::new(outs.into_iter().map(|(n, b)| (n, Arc::new(b))).collect());
                if cached {
                    cache_put(k, outs.clone());
                }
                Ok(vec![(i, k, outs, true)])
            })
            .collect();
        for d in done {
            for (i, k, outs, ran) in d? {
                runs += usize::from(ran);
                keys.insert(parsed[i].name.clone(), k);
                ports.insert(parsed[i].name.clone(), outs);
            }
        }
    }
    Ok(Evaluated {
        nodes: parsed,
        ports,
        runs,
    })
}
