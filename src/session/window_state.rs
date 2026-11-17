//! The `state` object the window draws (see `design/window-api.md`): every
//! number comes from the same Rust that serves agents.
//!
//! Building it never blocks on a long computation it can skip: the default
//! checks and the macro sweeps are read from caches that `Session::check`
//! fills (the window asks for that in a separate command, see
//! `Session::window_needs_settle`), and are empty / null until then.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sfx_graph::{Macro, Params};

use super::{Annotation, Session, Sound};

/// The window is not an agent: it is left out of the agents list and feed.
pub const WINDOW_CLIENT: &str = "window";

/// An agent that called within this long is `live`.
const LIVE_FOR: Duration = Duration::from_secs(120);
/// A sound an agent called in this recently is `working`.
const WORKING_FOR: Duration = Duration::from_secs(20);

fn ago_coarse(t: Instant) -> String {
    let s = t.elapsed().as_secs();
    match s {
        0..=4 => "now".into(),
        5..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    }
}

fn ago_clock(t: Instant) -> String {
    let s = t.elapsed().as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

fn kind_name(k: sfx_graph::Kind) -> &'static str {
    match k {
        sfx_graph::Kind::Oneshot => "oneshot",
        sfx_graph::Kind::Loop => "loop",
    }
}

/// Node names a history label touches: `+a ~b -c` -> a, b (removed ones
/// are not in the graph any more).
fn label_nodes(label: &str) -> Vec<&str> {
    label
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|t| t.strip_prefix('+').or_else(|| t.strip_prefix('~')))
        .filter(|t| !t.is_empty())
        .collect()
}

fn macro_json(m: &Macro, result: Option<(bool, &str)>) -> Value {
    json!({
        "name": m.name,
        "value": m.value,
        "default": m.default_value(),
        "doc": m.doc,
        "claim": m.claim,
        "game": m.game,
        "map": m.map.iter().map(|e| json!({
            "node": e.node, "field": e.field, "from": e.from, "to": e.to,
            "curve": e.curve, "col": e.col, "index": e.index, "key": e.key,
        })).collect::<Vec<_>>(),
        "result": result.map(|(pass, detail)| json!({"pass": pass, "detail": detail})),
    })
}

fn comment_json(a: &Annotation) -> Value {
    json!({
        "id": a.id, "text": a.text, "by": a.by, "node": a.node,
        "time": a.time, "hz": a.hz, "resolved": a.resolved, "reply": a.reply,
    })
}

impl Session {
    /// The sound the window shows: `doc` when given (must be open), else the
    /// window client's active sound, else the newest open one.
    fn window_target(&self, doc: Option<&str>) -> Result<Option<String>, String> {
        if let Some(d) = doc {
            return self.target_name(Some(d)).map(Some);
        }
        Ok(self
            .active_name()
            .or_else(|| self.order.last().cloned())
            .filter(|n| self.sounds.contains_key(n)))
    }

    /// The human opened `name` in the window: nodes changed by others before
    /// now are no longer `unread`.
    pub fn window_mark_seen(&mut self, name: &str) {
        if let Some(s) = self.sounds.get(name) {
            self.window_seen
                .insert(name.to_string(), s.history.nodes.len());
            self.rev += 1;
        }
    }

    /// True when the default checks or a macro claim of `doc` are not
    /// computed yet: the window then asks for `Session::check` in a command
    /// of its own, so `state` never waits for the sweep.
    pub fn window_needs_settle(&self, doc: Option<&str>) -> Option<String> {
        let name = self.window_target(doc).ok().flatten()?;
        let s = self.sounds.get(&name)?;
        if s.doc.nodes.is_empty() {
            return None;
        }
        let hash = s.doc_hash();
        let checks_ok = self.check_cache.get(&name).is_some_and(|(h, _)| *h == hash);
        let macros_ok = s.doc.macros.iter().all(|m| {
            s.macro_results
                .get(&m.name)
                .is_some_and(|(h, ..)| *h == hash)
        });
        (!(checks_ok && macros_ok)).then_some(name)
    }

    /// The window's `state` (no `solo` / `playing` knowledge: the window
    /// layer fills those in). `doc` switches the window's open sound.
    pub fn window_state(&mut self, doc: Option<&str>) -> Result<Value, String> {
        let target = self.window_target(doc)?;
        if self.current.as_deref() == Some(WINDOW_CLIENT)
            && let Some(t) = &target
        {
            self.window_pin(t);
        }
        let now_live =
            |t: Option<Instant>, within: Duration| t.is_some_and(|t| t.elapsed() < within);

        // Each open sound's length and loudness (the grid's tiles show them):
        // from the render cache, rendering when it is stale; null when the
        // sound does not render yet.
        let opts = self.render_opts();
        let salt = self.generation();
        let mut stats: BTreeMap<String, (Option<f32>, Option<f32>)> = BTreeMap::new();
        for (n, s) in self.sounds.iter_mut() {
            stats.insert(
                n.clone(),
                s.rendered(&opts, salt)
                    .map(|r| (Some(r.audio.seconds()), Some(r.lufs)))
                    .unwrap_or((None, None)),
            );
        }

        // Sounds.
        let sounds: Vec<Value> = self
            .order
            .iter()
            .filter_map(|n| self.sounds.get(n).map(|s| (n, s)))
            .map(|(n, s)| {
                let by = s
                    .history
                    .current
                    .and_then(|c| s.history.nodes.get(c))
                    .and_then(|h| h.author.clone());
                let working = self.clients.iter().any(|(c, st)| {
                    c != WINDOW_CLIENT
                        && st.active.as_deref() == Some(n.as_str())
                        && now_live(st.last, WORKING_FOR)
                });
                json!({
                    "name": n,
                    "kind": kind_name(s.doc.kind),
                    "by": by,
                    "ago": self.touched.get(n).map_or_else(|| "—".to_string(), |t| ago_coarse(*t)),
                    "working": working,
                    "saved": !s.unsaved,
                    "open_comments": s.open_annotations().count(),
                    "seconds": stats.get(n).and_then(|x| x.0),
                    "lufs": stats.get(n).and_then(|x| x.1),
                })
            })
            .collect();

        // Agents and the feed come from the call log.
        let mut agents: BTreeMap<String, bool> = BTreeMap::new();
        for (c, st) in &self.clients {
            if c != WINDOW_CLIENT {
                agents.insert(c.clone(), now_live(st.last, LIVE_FOR));
            }
        }
        let agents: Vec<Value> = agents
            .into_iter()
            .map(|(name, live)| json!({"name": name, "live": live}))
            .collect();
        let feed: Vec<Value> = self
            .calls
            .iter()
            .rev()
            .filter(|c| c.client.as_deref() != Some(WINDOW_CLIENT))
            .take(30)
            .map(|c| {
                json!({
                    "agent": c.client.as_deref().unwrap_or("agent"),
                    "tool": c.tool,
                    "what": c.what.clone().or_else(|| c.doc.clone()).unwrap_or_default(),
                    "ago": ago_clock(c.at),
                    "doc": c.doc,
                })
            })
            .collect();

        let sound = target.clone().map(|n| self.window_sound(&n));
        // The top bar's label: the game project's folder, or the shared library.
        let project = self
            .project
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || "shared library".to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
        Ok(json!({
            "project": project,
            "rev": self.rev,
            "doc": target,
            "sounds": sounds,
            "agents": agents,
            "feed": feed,
            "sound": sound,
            "solo": null,
            "playing": false,
        }))
    }

    /// The `sound` object of `state`.
    fn window_sound(&mut self, name: &str) -> Value {
        let opts = self.render_opts();
        let salt = self.generation;
        let seen = *self
            .window_seen
            .entry(name.to_string())
            .or_insert_with(|| self.sounds.get(name).map_or(0, |s| s.history.nodes.len()));
        let (check_hit, desc_hit) = (
            self.check_cache.get(name).cloned(),
            self.desc_cache.get(name).cloned(),
        );
        let Some(sound) = self.sounds.get_mut(name) else {
            return Value::Null;
        };
        let hash = sound.doc_hash();
        let rendered = sound.rendered(&opts, salt).cloned();
        let d = &sound.doc;

        let nodes = window_nodes(sound, seen);
        let macros: Vec<Value> = d
            .macros
            .iter()
            .map(|m| {
                let r = sound
                    .macro_results
                    .get(&m.name)
                    .filter(|(h, ..)| *h == hash)
                    .map(|(_, pass, detail)| (*pass, detail.as_str()));
                macro_json(m, r)
            })
            .collect();
        let checks = match check_hit {
            Some((h, results)) if h == hash => serde_json::to_value(&results).unwrap_or(json!([])),
            _ => json!([]),
        };
        let mut out = json!({
            "name": d.name,
            "kind": kind_name(d.kind),
            "version": d.version,
            "seed": d.seed,
            "sample_rate": d.sample_rate,
            "duration": d.duration,
            "seconds": null,
            "peak_db": null,
            "lufs": null,
            "causal": null,
            "warnings": [],
            "nodes": nodes,
            "params": serde_json::to_value(&d.params).unwrap_or(json!({})),
            "macros": macros,
            "moves": sound.knob_moves.iter().map(|k| json!({"name": k.name, "from": k.from, "to": k.to})).collect::<Vec<_>>(),
            "comments": sound.annotations.iter().map(comment_json).collect::<Vec<_>>(),
            "history": sound.history.nodes.iter().enumerate().map(|(id, n)| json!({
                "id": id, "label": n.label, "by": n.author,
                "version": n.model.0.version,
                "current": sound.history.current == Some(id),
            })).collect::<Vec<_>>(),
            "checks": checks,
            "descriptors": null,
        });
        match &rendered {
            Ok(r) => {
                out["seconds"] = json!(r.audio.seconds());
                out["peak_db"] = json!(r.peak_db);
                out["lufs"] = json!(r.lufs);
                out["causal"] = json!(r.causal);
                out["warnings"] = json!(r.warnings);
                let desc = match desc_hit {
                    Some((h, g, v)) if h == hash && g == salt => v,
                    _ => {
                        let v = serde_json::to_value(sfx_analysis::describe(&r.audio))
                            .unwrap_or(Value::Null);
                        self.desc_cache
                            .insert(name.to_string(), (hash, salt, v.clone()));
                        v
                    }
                };
                out["descriptors"] = desc;
            }
            Err(e) => out["warnings"] = json!([format!("does not render yet: {e}")]),
        }
        out
    }
}

/// The graph for the window: evaluation order, reads, raw fields, the
/// fields macros drive (`effective`), unread and comment counts, depth.
fn window_nodes(sound: &Sound, seen: usize) -> Vec<Value> {
    let d = &sound.doc;
    let names: Vec<&str> = d
        .nodes
        .iter()
        .filter_map(|n| n.get("name").and_then(Value::as_str))
        .collect();
    // The graph as evaluated (params resolved, macros applied).
    let graph = d.graph(&BTreeMap::new()).ok();
    let parsed = graph
        .as_ref()
        .and_then(|g| sfx_graph::eval::parse(g, &Params::new()).ok());
    let mut depth = vec![0usize; d.nodes.len()];
    if let Some(p) = &parsed
        && let Ok(levels) = sfx_graph::eval::levels(p)
    {
        for (lv, members) in levels.iter().enumerate() {
            for i in members {
                if let Some(slot) = depth.get_mut(*i) {
                    *slot = lv;
                }
            }
        }
    }
    // Nodes others changed since the window last looked.
    let mut unread: Vec<&str> = Vec::new();
    for n in sound.history.nodes.iter().skip(seen) {
        if n.author.as_deref() != Some(WINDOW_CLIENT) {
            unread.extend(label_nodes(&n.label));
        }
    }
    let mut order: Vec<usize> = (0..d.nodes.len()).collect();
    order.sort_by_key(|i| (depth[*i], *i));
    order
        .into_iter()
        .map(|i| {
            let raw = &d.nodes[i];
            let name = raw.get("name").and_then(Value::as_str).unwrap_or("?");
            let ty = raw.get("type").and_then(Value::as_str).unwrap_or("?");
            let mut fields = raw.clone();
            if let Some(o) = fields.as_object_mut() {
                o.remove("type");
                o.remove("name");
            }
            let reads: Vec<String> = match &parsed {
                Some(p) => p[i].reads.clone(),
                None => sfx_graph::node::refs(&fields)
                    .into_iter()
                    .map(|(n, _)| n)
                    .filter(|n| names.contains(&n.as_str()))
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            };
            let mut effective = serde_json::Map::new();
            for m in &d.macros {
                for e in m.map.iter().filter(|e| e.node == name) {
                    if let Some(v) = graph
                        .as_ref()
                        .and_then(|g| g.get(i))
                        .and_then(|n| n.get(&e.field))
                    {
                        effective.insert(e.field.clone(), v.clone());
                    }
                }
            }
            json!({
                "name": name,
                "type": ty,
                "reads": reads,
                "fields": fields,
                "effective": effective,
                "unread": unread.contains(&name),
                "open_comments": sound.open_annotations().filter(|a| a.node.as_deref() == Some(name)).count(),
                "depth": depth[i],
            })
        })
        .collect()
}

impl Session {
    /// Makes `name` the sound the window client works in (not any agent's).
    pub fn window_pin(&mut self, name: &str) {
        if self.sounds.contains_key(name) {
            self.clients
                .entry(WINDOW_CLIENT.to_string())
                .or_default()
                .active = Some(name.to_string());
        }
    }

    /// The part of `sound` a knob move changes: macros, the human's moves
    /// and the headline numbers. Cheap: no descriptors.
    pub fn window_knob_result(&mut self, doc: Option<&str>) -> Result<Value, String> {
        let opts = self.render_opts();
        let salt = self.generation;
        let sound = self.sound(doc)?;
        let hash = sound.doc_hash();
        let rendered = sound.rendered(&opts, salt).cloned()?;
        let macros: Vec<Value> = sound
            .doc
            .macros
            .iter()
            .map(|m| {
                let r = sound
                    .macro_results
                    .get(&m.name)
                    .filter(|(h, ..)| *h == hash)
                    .map(|(_, pass, detail)| (*pass, detail.as_str()));
                macro_json(m, r)
            })
            .collect();
        Ok(json!({
            "macros": macros,
            "moves": sound.knob_moves.iter().map(|k| json!({"name": k.name, "from": k.from, "to": k.to})).collect::<Vec<_>>(),
            "seconds": rendered.audio.seconds(),
            "peak_db": rendered.peak_db,
            "lufs": rendered.lufs,
        }))
    }
}
