//! The window's calls (see `design/window-api.md`): a thin list over
//! `Session` methods. Every call runs on the session thread as client
//! `window`, so its edits are credited to it (history shows `by window`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use sfx_graph::DocEdit;
use sounder::catalog;
use sounder::session::{Annotation, Session, Travel};

use super::player_iface::PlayerControl;
use crate::server::Cmd;

/// The session client name of the window.
pub const CLIENT: &str = "window";
/// The author of the human's comments.
pub const HUMAN: &str = "you";
/// A pause in changes this long before the checks / macro sweeps run.
const SETTLE_AFTER: Duration = Duration::from_millis(400);

/// What the window's calls share besides the session.
pub struct Shared {
    pub player: Arc<dyn PlayerControl>,
    /// The node the player plays (None = the output).
    pub solo: Mutex<Option<String>>,
    pub tx: mpsc::Sender<Cmd>,
    settling: AtomicBool,
}

impl Shared {
    pub fn new(player: Arc<dyn PlayerControl>, tx: mpsc::Sender<Cmd>) -> Arc<Shared> {
        Arc::new(Shared {
            player,
            solo: Mutex::new(None),
            tx,
            settling: AtomicBool::new(false),
        })
    }

    fn solo(&self) -> Option<String> {
        self.solo.lock().ok().and_then(|s| s.clone())
    }

    fn set_solo(&self, node: Option<String>) {
        if let Ok(mut s) = self.solo.lock() {
            *s = node;
        }
    }
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{k}` is missing (a string)"))
}

fn num_arg(a: &Value, k: &str) -> Result<f64, String> {
    a.get(k)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("`{k}` is missing (a number)"))
}

/// `[a, b]`, or a single number as `[a, a]`; absent / null is None.
fn range_arg(a: &Value, k: &str) -> Result<Option<[f32; 2]>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let v = n.as_f64().unwrap_or(0.0) as f32;
            Ok(Some([v, v]))
        }
        Some(Value::Array(v)) if v.len() == 2 => match (v[0].as_f64(), v[1].as_f64()) {
            (Some(x), Some(y)) => Ok(Some([x as f32, y as f32])),
            _ => Err(format!("`{k}` must be [from, to] numbers")),
        },
        Some(_) => Err(format!("`{k}` must be [from, to] numbers")),
    }
}

/// Runs one window call on the session thread.
pub fn call(s: &mut Session, sh: &Arc<Shared>, name: &str, args: &Value) -> Result<Value, String> {
    let pin = match name {
        "state" | "select_sound" => args.get("doc").and_then(Value::as_str),
        _ => None,
    };
    let _ = s.begin_call(Some(CLIENT), pin, None);
    let r = run(s, sh, name, args);
    s.end_call(Some(CLIENT));
    r
}

fn run(s: &mut Session, sh: &Arc<Shared>, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "state" => state(s, sh, a.get("doc").and_then(Value::as_str)),
        "select_sound" => {
            let doc = str_arg(a, "doc")?.to_string();
            s.window_pin(&doc);
            s.window_mark_seen(&doc);
            sh.set_solo(None);
            state(s, sh, Some(&doc))
        }
        "move_knob" => {
            s.move_knob(None, str_arg(a, "name")?, num_arg(a, "value")?)?;
            s.window_knob_result(None)
        }
        "reset_knobs" => {
            let defaults: Vec<(String, f64)> = s
                .sound(None)?
                .doc
                .macros
                .iter()
                .map(|m| (m.name.clone(), m.default_value()))
                .collect();
            for (n, v) in defaults {
                s.move_knob(None, &n, v)?;
            }
            s.window_knob_result(None)
        }
        "set_field" => set_field(s, a),
        "tool" => crate::server::window_tool(
            s,
            str_arg(a, "name")?,
            a.get("args").cloned().unwrap_or_else(|| json!({})),
        ),
        "layout" => {
            let patch = a
                .get("layout")
                .ok_or("`layout` is missing ({nodes, rigs})")?;
            Ok(json!({"changed": s.set_layout(None, patch)?}))
        }
        "node_types" => Ok(node_types()),
        "set_seed" => {
            let seed = a
                .get("seed")
                .and_then(Value::as_u64)
                .ok_or("`seed` is missing (a whole number)")?;
            let edit = DocEdit {
                seed: Some(seed),
                ..DocEdit::default()
            };
            let r = s.apply_ops(None, Vec::new(), edit)?;
            Ok(json!({"label": r.label, "changed": r.changed}))
        }
        "add_annotation" => {
            let text = str_arg(a, "text")?.trim().to_string();
            if text.is_empty() {
                return Err("a comment needs some text".into());
            }
            let note = Annotation {
                id: 0,
                text,
                by: Some(HUMAN.into()),
                node: a.get("node").and_then(Value::as_str).map(String::from),
                time: range_arg(a, "time")?,
                hz: range_arg(a, "hz")?,
                resolved: false,
                reply: None,
                resolved_by: None,
            };
            let line = s.add_annotation(None, note)?;
            Ok(json!({"added": line}))
        }
        "jump" => {
            let id = a
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("`id` is missing (a history step number)")? as usize;
            // The human may undo an agent's step: force.
            let msg = s.travel(None, Travel::Jump(id), true)?;
            Ok(json!({"moved": msg}))
        }
        "solo" => {
            let node = match a.get("node") {
                None | Some(Value::Null) => None,
                Some(Value::String(n)) => {
                    let sound = s.sound(None)?;
                    let known = sound
                        .doc
                        .nodes
                        .iter()
                        .any(|x| x.get("name").and_then(Value::as_str) == Some(n.as_str()));
                    if !known {
                        return Err(format!("no node named `{n}` to solo"));
                    }
                    Some(n.clone())
                }
                Some(_) => return Err("`node` must be a node name or null".into()),
            };
            sh.set_solo(node.clone());
            Ok(json!({"solo": node}))
        }
        "play" => {
            let from = a.get("from").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let span = range_arg(a, "span")?;
            let looped = a.get("loop").and_then(Value::as_bool).unwrap_or(false);
            let solo = sh.solo();
            let audio = s.node_audio(None, solo.as_deref())?;
            sh.player.play(Arc::new(audio), from, span, looped)?;
            Ok(json!({"playing": true, "node": solo}))
        }
        "stop" => {
            sh.player.stop();
            Ok(json!({"playing": false}))
        }
        "seek" => {
            // The player applies a seek asynchronously: answer with the
            // requested time (its `Seeked` event confirms).
            let t = num_arg(a, "t")? as f32;
            sh.player.seek(t);
            Ok(json!({"pos": t}))
        }
        "catalog_list" => {
            let lib = s.library_dir()?;
            Ok(serde_json::to_value(catalog::list(&lib)).unwrap_or(json!([])))
        }
        "catalog_approve" => {
            // The human's verdict by ear; agents have no call that sets it.
            let n = str_arg(a, "name")?.to_string();
            let approved = a.get("approved").and_then(|v| v.as_bool()).unwrap_or(true);
            let lib = s.library_dir()?;
            catalog::set_approved(&lib, &n, approved)?;
            s.mark_dirty();
            Ok(serde_json::to_value(catalog::list(&lib)).unwrap_or(json!([])))
        }
        "catalog_load" => {
            let n = str_arg(a, "name")?.to_string();
            let lib = s.library_dir()?;
            // Already open: just show it.
            if s.sounds.contains_key(&n) {
                s.window_pin(&n);
                s.window_mark_seen(&n);
            } else {
                let doc = catalog::load(&lib, &n)?;
                s.open_doc(doc, &n, false)?;
            }
            let opened = s.sound(Some(&n)).map(|x| x.doc.name.clone())?;
            s.window_pin(&opened);
            sh.set_solo(None);
            state(s, sh, Some(&opened))
        }
        other => Err(format!(
            "unknown window call `{other}` (state, select_sound, move_knob, reset_knobs, set_field, set_seed, add_annotation, jump, solo, play, stop, seek, catalog_list, catalog_load, catalog_approve)"
        )),
    }
}

/// The `state` object with the window layer's `solo` / `playing`; also
/// arranges for the checks and macro sweeps to be computed soon.
fn state(s: &mut Session, sh: &Arc<Shared>, doc: Option<&str>) -> Result<Value, String> {
    let mut st = s.window_state(doc)?;
    let solo = sh.solo().filter(|n| {
        st["sound"]["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|x| x["name"] == json!(n)))
    });
    if solo.is_none() {
        sh.set_solo(None);
    }
    // A soloed node's own length / level, for the stats chips.
    st["solo_stats"] = solo
        .as_deref()
        .and_then(|n| s.node_audio(doc, Some(n)).ok())
        .map(|a| {
            let d = sfx_analysis::describe(&a);
            json!({
                "seconds": a.len() as f32 / a.sample_rate.max(1) as f32,
                "peak_db": d.peak_db,
                "lufs": d.lufs,
            })
        })
        .unwrap_or(Value::Null);
    st["solo"] = json!(solo);
    st["playing"] = json!(sh.player.playing());
    schedule_settle(s, sh, doc);
    Ok(st)
}

/// Runs the default checks (which include the macro sweeps) in a command of
/// their own once changes pause, so `state` never waits for them. They bump
/// `rev`, so the page asks again and gets them.
fn schedule_settle(s: &Session, sh: &Arc<Shared>, doc: Option<&str>) {
    let Some(name) = s.window_needs_settle(doc) else {
        return;
    };
    if sh.settling.swap(true, Ordering::SeqCst) {
        return;
    }
    let rev0 = s.rev();
    let sh2 = Arc::clone(sh);
    std::thread::spawn(move || {
        std::thread::sleep(SETTLE_AFTER);
        let sh3 = Arc::clone(&sh2);
        let _ = sh2.tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
            sh3.settling.store(false, Ordering::SeqCst);
            if !s.sounds.contains_key(&name) {
                return;
            }
            if s.rev() == rev0 {
                // A sound that does not render has nothing to check.
                let _ = s.check(Some(&name), None);
            } else {
                // Still changing: look again after the next pause.
                schedule_settle(s, &sh3, Some(&name));
            }
        })));
    });
}

/// Every node type with its blurb, an example and the JSON schema of its fields: the window builds
/// its add-node list and field editors from these (the same text `describe_op` serves agents).
fn node_types() -> Value {
    let types: Vec<Value> = sfx_graph::NODE_TYPES
        .iter()
        .map(|t| {
            json!({
                "type": t,
                "blurb": sfx_graph::blurb(t),
                "example": sfx_graph::example(t),
                "schema": sfx_graph::node::schema(t).map(|s| serde_json::to_value(s).unwrap_or(Value::Null)),
            })
        })
        .collect();
    Value::Array(types)
}

/// One field of one node through `apply_ops`: one history step `~node`.
/// A null value removes the field (back to the node's default).
fn set_field(s: &mut Session, a: &Value) -> Result<Value, String> {
    let node = str_arg(a, "node")?;
    let field = str_arg(a, "field")?;
    let value = a.get("value").cloned().unwrap_or(Value::Null);
    let sound = s.sound(None)?;
    let Some(old) = sound
        .doc
        .nodes
        .iter()
        .find(|x| x.get("name").and_then(Value::as_str) == Some(node))
    else {
        let names: Vec<&str> = sound
            .doc
            .nodes
            .iter()
            .filter_map(|x| x.get("name").and_then(Value::as_str))
            .collect();
        return Err(format!(
            "no node named `{node}` (nodes: {})",
            names.join(", ")
        ));
    };
    if field == "type" || field == "name" {
        return Err(format!("`{field}` is not an editable field"));
    }
    let mut item: Map<String, Value> = old.as_object().cloned().unwrap_or_default();
    if value.is_null() {
        item.remove(field);
    } else {
        item.insert(field.to_string(), value);
    }
    let r = s.apply_ops(None, vec![Value::Object(item)], DocEdit::default())?;
    Ok(json!({
        "label": r.label,
        "changed": r.changed,
        "seconds": r.rendered.audio.seconds(),
        "peak_db": r.rendered.peak_db,
        "lufs": r.rendered.lufs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Done, run_session};
    use crate::window::player_iface::{FakeCall, FakePlayer, PlayerEvent};
    use sfx_graph::{Claim, Macro, MapCurve, MapEntry, Measure};
    use std::sync::atomic::AtomicUsize;

    struct Rig {
        tx: mpsc::Sender<Cmd>,
        sh: Arc<Shared>,
        player: Arc<FakePlayer>,
        done: Arc<Done>,
        join: Option<std::thread::JoinHandle<Session>>,
        changes: Arc<AtomicUsize>,
        last_rev: Arc<AtomicUsize>,
    }

    impl Rig {
        fn new(tag: &str) -> Rig {
            let dir = std::env::temp_dir().join(format!("sounder_w_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            let mut s = Session::new(Some(dir));
            s.autosave_ok = false;
            let (tx, rx) = mpsc::channel();
            let done = Done::new();
            let (changes, last_rev) =
                (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
            let (c2, r2) = (Arc::clone(&changes), Arc::clone(&last_rev));
            let hook: crate::server::OnChange = Box::new(move |rev| {
                c2.fetch_add(1, Ordering::SeqCst);
                r2.store(rev as usize, Ordering::SeqCst);
            });
            let d2 = Arc::clone(&done);
            let join = std::thread::spawn(move || run_session(s, rx, d2, Some(hook)));
            let player = Arc::new(FakePlayer::default());
            let sh = Shared::new(Arc::clone(&player) as Arc<dyn PlayerControl>, tx.clone());
            Rig {
                tx,
                sh,
                player,
                done,
                join: Some(join),
                changes,
                last_rev,
            }
        }

        /// A window call over the real bridge.
        fn call(&self, name: &str, args: Value) -> Result<Value, String> {
            let (rtx, rrx) = mpsc::channel();
            let (sh, name) = (Arc::clone(&self.sh), name.to_string());
            let _ = self.tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
                let _ = rtx.send(call(s, &sh, &name, &args));
            })));
            rrx.recv_timeout(Duration::from_secs(60)).unwrap()
        }

        /// Runs `f` on the session thread as the agent `look`.
        fn agent<T: Send + 'static>(
            &self,
            f: impl FnOnce(&mut Session) -> T + Send + 'static,
        ) -> T {
            let (rtx, rrx) = mpsc::channel();
            let _ = self.tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
                let _ = s.begin_call(Some("look"), Some("zap"), None);
                let r = f(s);
                s.end_call(Some("look"));
                let _ = rtx.send(r);
            })));
            rrx.recv_timeout(Duration::from_secs(60)).unwrap()
        }

        /// Opens `zap` (noise -> lp -> output) with a brightness knob.
        fn with_sound(self) -> Rig {
            self.agent(|s| {
                s.new_sound("zap", sfx_graph::Kind::Oneshot).unwrap();
                let graph = vec![
                    json!({"type": "noise", "name": "n", "amp": 0.3}),
                    json!({"type": "decay", "name": "e", "time": 0.2}),
                    json!({"type": "math", "name": "m", "op": "mul", "a": "n", "b": "e"}),
                    json!({"type": "filter", "name": "lp", "input": "m", "mode": "lp", "freq": 1000}),
                    json!({"type": "output", "name": "out", "input": "lp", "lufs": -20}),
                ];
                let knob = Macro {
                    name: "brightness".into(),
                    value: 0.5,
                    default: None,
                    doc: "cutoff".into(),
                    claim: Some(Claim(Measure::Centroid, 1)),
                    game: false,
                    map: vec![MapEntry {
                        node: "lp".into(),
                        field: "freq".into(),
                        from: 300.0,
                        to: 8000.0,
                        curve: MapCurve::Exp,
                        param: None,
                        col: None,
                        index: None,
                        key: None,
                    }],
                };
                let edit = DocEdit {
                    macros: Some(vec![knob]),
                    ..DocEdit::default()
                };
                s.apply_ops(None, graph, edit).unwrap();
            });
            self
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            self.done.set();
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn summary_text(rig: &Rig) -> String {
        rig.agent(|s| s.summary(None).unwrap().0)
    }

    #[test]
    fn the_window_edits_through_the_agents_tools() {
        let rig = Rig::new("tool").with_sound();
        let r = rig
            .call(
                "tool",
                json!({"name": "sound_ops", "args": {"ops": [
                    {"type": "gain", "name": "g", "input": "lp", "db": -3},
                    {"type": "output", "name": "out", "input": "g", "lufs": -20}]}}),
            )
            .unwrap();
        assert_eq!(r["json"]["label"], "+g ~out", "{r}");
        let st = rig.call("state", json!({})).unwrap();
        let h = st["sound"]["history"].as_array().unwrap();
        assert_eq!(h.last().unwrap()["label"], "+g ~out");
        assert_eq!(h.last().unwrap()["by"], CLIENT);
        // the server's checks apply: a second output is refused and nothing changes
        let err = rig
            .call("tool", json!({"name": "sound_ops", "args": {"ops": [{"type": "output", "name": "out2", "input": "g"}]}}))
            .unwrap_err();
        assert!(err.contains("output"), "{err}");
        assert_eq!(
            rig.call("state", json!({})).unwrap()["sound"]["history"]
                .as_array()
                .unwrap()
                .len(),
            h.len()
        );
        // only the editing tools are open to the window
        let err = rig
            .call("tool", json!({"name": "listen", "args": {}}))
            .unwrap_err();
        assert!(err.contains("cannot call `listen`"), "{err}");
        // a new sound through new_sound
        rig.call(
            "tool",
            json!({"name": "new_sound", "args": {"name": "hum"}}),
        )
        .unwrap();
        let st = rig.call("state", json!({})).unwrap();
        assert!(
            st["sounds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["name"] == "hum")
        );
    }

    #[test]
    fn layout_is_a_history_step_that_keeps_the_render() {
        let rig = Rig::new("layout").with_sound();
        let before = rig.call("state", json!({})).unwrap()["sound"].clone();
        let r = rig
            .call(
                "layout",
                json!({"layout": {"nodes": {"lp": [400, 120], "ghost": [1, 2]}}}),
            )
            .unwrap();
        assert_eq!(r["changed"], true);
        let st = rig.call("state", json!({})).unwrap()["sound"].clone();
        // a node that does not exist is dropped; the rest is kept
        assert_eq!(st["layout"], json!({"nodes": {"lp": [400.0, 120.0]}}));
        assert_eq!(
            st["history"].as_array().unwrap().last().unwrap()["label"],
            "layout"
        );
        assert_eq!(st["version"], before["version"]);
        assert_eq!(st["hash"], before["hash"]);
        // the same patch again changes nothing
        let r = rig
            .call("layout", json!({"layout": {"nodes": {"lp": [400, 120]}}}))
            .unwrap();
        assert_eq!(r["changed"], false);
        // deleting the node drops its place
        rig.call(
            "tool",
            json!({"name": "sound_ops", "args": {"delete_macros": ["brightness"], "ops": [
                {"type": "output", "name": "out", "input": "m", "lufs": -20},
                {"type": "delete", "name": "lp"}]}}),
        )
        .unwrap();
        let st = rig.call("state", json!({})).unwrap()["sound"].clone();
        assert_eq!(st["layout"], json!({}));
        let err = rig
            .call("layout", json!({"layout": {"nodes": {"lp": "x"}}}))
            .unwrap_err();
        assert!(err.contains("[x, y]"), "{err}");
    }

    #[test]
    fn node_types_serve_every_type_with_its_schema() {
        let rig = Rig::new("types");
        let r = rig.call("node_types", json!({})).unwrap();
        let types = r.as_array().unwrap();
        assert_eq!(types.len(), sfx_graph::NODE_TYPES.len());
        let f = types.iter().find(|t| t["type"] == "filter").unwrap();
        assert!(f["schema"]["properties"]["freq"].is_object(), "{f}");
        assert_eq!(f["example"]["type"], "filter");
        assert!(!f["blurb"].as_str().unwrap().is_empty());
    }

    #[test]
    fn move_knob_reaches_the_agents_summary() {
        let rig = Rig::new("knob").with_sound();
        let r = rig
            .call("move_knob", json!({"name": "brightness", "value": 0.8}))
            .unwrap();
        assert_eq!(r["moves"][0]["name"], "brightness");
        assert_eq!(r["moves"][0]["to"], 0.8);
        assert!(r["seconds"].as_f64().unwrap() > 0.0);
        // The window does not clear moves: state still shows them.
        let st = rig.call("state", json!({})).unwrap();
        assert_eq!(st["sound"]["moves"].as_array().unwrap().len(), 1);
        // The agent sees the move, and reading it clears it.
        let text = summary_text(&rig);
        assert!(text.contains("brightness 0.50 -> 0.80"), "{text}");
        let st = rig.call("state", json!({})).unwrap();
        assert!(st["sound"]["moves"].as_array().unwrap().is_empty());
        // A knob move is not a history step.
        assert_eq!(st["sound"]["history"].as_array().unwrap().len(), 2);
        // Reset puts it back.
        let r = rig.call("reset_knobs", json!({})).unwrap();
        assert_eq!(r["macros"][0]["value"], 0.5);
        let err = rig
            .call("move_knob", json!({"name": "nope", "value": 1}))
            .unwrap_err();
        assert!(err.contains("no macro named `nope`"), "{err}");
    }

    #[test]
    fn add_annotation_is_an_open_comment_for_the_agent() {
        let rig = Rig::new("note").with_sound();
        rig.call(
            "add_annotation",
            json!({"text": "too hissy", "node": "lp", "time": [0.0, 0.07], "hz": [2500, 9000]}),
        )
        .unwrap();
        let text = summary_text(&rig);
        assert!(text.contains("open comments (1)"), "{text}");
        assert!(
            text.contains("too hissy") && text.contains("by you"),
            "{text}"
        );
        let st = rig.call("state", json!({})).unwrap();
        let c = &st["sound"]["comments"][0];
        assert_eq!(
            (c["by"].as_str(), c["node"].as_str()),
            (Some("you"), Some("lp"))
        );
        assert_eq!(
            c["time"][1].as_f64().map(|v| (v * 100.0).round()),
            Some(7.0)
        );
        assert_eq!(st["sounds"][0]["open_comments"], 1);
        let node = st["sound"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == "lp")
            .unwrap()
            .clone();
        assert_eq!(node["open_comments"], 1);
        assert!(rig.call("add_annotation", json!({"text": "  "})).is_err());
        let err = rig
            .call("add_annotation", json!({"text": "x", "node": "ghost"}))
            .unwrap_err();
        assert!(err.contains("no node named `ghost`"), "{err}");
    }

    #[test]
    fn set_field_is_one_history_step_by_window() {
        let rig = Rig::new("field").with_sound();
        let r = rig
            .call(
                "set_field",
                json!({"node": "e", "field": "time", "value": 0.4}),
            )
            .unwrap();
        assert_eq!(r["label"], "~e");
        let st = rig.call("state", json!({})).unwrap();
        let h = st["sound"]["history"].as_array().unwrap();
        let last = h.last().unwrap();
        assert_eq!(last["label"], "~e");
        assert_eq!(last["by"], "window");
        assert_eq!(last["current"], true);
        let e = st["sound"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == "e")
            .unwrap()
            .clone();
        assert_eq!(e["fields"]["time"], 0.4);
        // A bad value is the same plain-English error an agent would get,
        // and makes no history step.
        let before = h.len();
        let err = rig
            .call(
                "set_field",
                json!({"node": "e", "field": "time", "value": "fast"}),
            )
            .unwrap_err();
        assert!(err.contains("time"), "{err}");
        let err = rig
            .call(
                "set_field",
                json!({"node": "zzz", "field": "time", "value": 1}),
            )
            .unwrap_err();
        assert!(err.contains("no node named `zzz`"), "{err}");
        let st = rig.call("state", json!({})).unwrap();
        assert_eq!(st["sound"]["history"].as_array().unwrap().len(), before);
        // Seed is a step too.
        rig.call("set_seed", json!({"seed": 7})).unwrap();
        let st = rig.call("state", json!({})).unwrap();
        assert_eq!(st["sound"]["seed"], 7);
        // The agent sees whose step it was; undoing the window's is allowed.
        let hist = rig.agent(|s| s.history_text(None, 10).unwrap());
        assert!(hist.contains("by window"), "{hist}");
    }

    #[test]
    fn jump_may_revert_an_agents_step() {
        let rig = Rig::new("jump").with_sound();
        rig.agent(|s| {
            s.apply_ops(
                None,
                vec![json!({"type": "decay", "name": "e", "time": 0.5})],
                DocEdit::default(),
            )
            .unwrap();
        });
        let st = rig.call("state", json!({})).unwrap();
        let h = st["sound"]["history"].as_array().unwrap();
        let target = h[h.len() - 2]["id"].as_u64().unwrap();
        rig.call("jump", json!({"id": target})).unwrap();
        let st = rig.call("state", json!({})).unwrap();
        let cur = st["sound"]["history"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["current"] == true)
            .unwrap()
            .clone();
        assert_eq!(cur["id"].as_u64(), Some(target));
        assert!(rig.call("jump", json!({"id": 999})).is_err());
    }

    #[test]
    fn an_agents_edit_bumps_rev_and_wakes_the_hook() {
        let rig = Rig::new("rev").with_sound();
        let st = rig.call("state", json!({})).unwrap();
        let rev0 = st["rev"].as_u64().unwrap();
        let hooks0 = rig.changes.load(Ordering::SeqCst);
        assert!(hooks0 >= 1, "opening and building the sound moved rev");
        rig.agent(|s| {
            s.apply_ops(
                None,
                vec![json!({"type": "decay", "name": "e", "time": 0.3})],
                DocEdit::default(),
            )
            .unwrap();
        });
        // The hook runs on the session thread right after the command.
        for _ in 0..100 {
            if rig.changes.load(Ordering::SeqCst) > hooks0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(rig.changes.load(Ordering::SeqCst) > hooks0);
        let st = rig.call("state", json!({})).unwrap();
        assert!(st["rev"].as_u64().unwrap() > rev0);
        // The hook was told the same rev the next state reports (or later:
        // the settle command may already have bumped it).
        assert!(rig.last_rev.load(Ordering::SeqCst) as u64 <= st["rev"].as_u64().unwrap());
        // Reading state alone changes nothing.
        let hooks = rig.changes.load(Ordering::SeqCst);
        let a = rig.call("state", json!({})).unwrap()["rev"].clone();
        let b = rig.call("state", json!({})).unwrap()["rev"].clone();
        assert_eq!(a, b);
        assert!(rig.changes.load(Ordering::SeqCst) <= hooks + 1);
    }

    #[test]
    fn checks_and_macro_results_settle_after_state() {
        let rig = Rig::new("settle").with_sound();
        let st = rig.call("state", json!({})).unwrap();
        assert!(st["sound"]["macros"][0]["result"].is_null());
        assert!(st["sound"]["checks"].as_array().unwrap().is_empty());
        // The window's own follow-up command computes them and bumps rev.
        let mut st = st;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(100));
            st = rig.call("state", json!({})).unwrap();
            if !st["sound"]["macros"][0]["result"].is_null() {
                break;
            }
        }
        let res = &st["sound"]["macros"][0]["result"];
        assert!(res["pass"].is_boolean(), "{st}");
        assert!(
            res["detail"].as_str().unwrap().contains("centroid"),
            "{res}"
        );
        assert!(!st["sound"]["checks"].as_array().unwrap().is_empty());
        let c = &st["sound"]["checks"][0];
        assert!(c["name"].is_string() && c["pass"].is_boolean() && c["detail"].is_string());
    }

    #[test]
    fn solo_play_stop_seek_talk_to_the_player() {
        let rig = Rig::new("play").with_sound();
        let events: Arc<Mutex<Vec<PlayerEvent>>> = Arc::default();
        let e2 = Arc::clone(&events);
        rig.player
            .set_event_handler(Box::new(move |e| e2.lock().unwrap().push(e)));
        assert!(rig.call("solo", json!({"node": "ghost"})).is_err());
        rig.call("solo", json!({"node": "lp"})).unwrap();
        let st = rig.call("state", json!({})).unwrap();
        assert_eq!(st["solo"], "lp");
        let r = rig
            .call(
                "play",
                json!({"from": 0.05, "span": [0.0, 0.1], "loop": true}),
            )
            .unwrap();
        assert_eq!(r["node"], "lp");
        rig.call("seek", json!({"t": 0.02})).unwrap();
        rig.call("stop", json!({})).unwrap();
        let calls = rig.player.calls();
        match &calls[0] {
            FakeCall::Play {
                samples,
                from,
                span,
                looped,
            } => {
                assert!(*samples > 1000);
                assert_eq!((*from, *span, *looped), (0.05, Some([0.0, 0.1]), true));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(calls[1], FakeCall::Seek(0.02));
        assert_eq!(calls[2], FakeCall::Stop);
        assert_eq!(events.lock().unwrap().len(), 3);
        // Solo is dropped when switching sound; null clears it.
        rig.call("solo", json!({"node": null})).unwrap();
        assert!(rig.call("state", json!({})).unwrap()["solo"].is_null());
    }

    #[test]
    fn selecting_and_loading_sounds() {
        let rig = Rig::new("sel").with_sound();
        rig.agent(|s| {
            s.new_sound("other", sfx_graph::Kind::Loop).unwrap();
        });
        let st = rig.call("select_sound", json!({"doc": "zap"})).unwrap();
        assert_eq!(st["doc"], "zap");
        let st = rig.call("state", json!({"doc": "other"})).unwrap();
        assert_eq!(st["doc"], "other");
        assert_eq!(st["sound"]["kind"], "loop");
        // The window's choice sticks for its next call, and an agent's
        // active sound is untouched.
        assert_eq!(rig.call("state", json!({})).unwrap()["doc"], "other");
        let agent_doc = rig.agent(|s| s.target_name(None).unwrap());
        assert_eq!(agent_doc, "zap");
        let err = rig.call("state", json!({"doc": "ghost"})).unwrap_err();
        assert!(err.contains("not open"), "{err}");
        // Catalog: save one through the session, then list and load it.
        rig.agent(|s| {
            let lib = s.library_dir().unwrap();
            let doc = s.sound(Some("zap")).unwrap().doc.clone();
            let entry = sounder::catalog::CatalogEntry {
                name: "zap".into(),
                description: "a zap".into(),
                tags: vec![],
                prompt: None,
                kind: doc.kind,
                length: 0.3,
                lufs: -20.0,
                macros: vec![],
                role: None,
                material: None,
                approved: false,
                builtin: false,
            };
            sounder::catalog::save(&lib, &doc, entry).unwrap();
            s.close_sound("zap", true).unwrap();
        });
        let list = rig.call("catalog_list", json!({})).unwrap();
        assert_eq!(list[0]["name"], "zap");
        let st = rig.call("catalog_load", json!({"name": "zap"})).unwrap();
        assert_eq!(st["doc"], "zap");
        assert!(rig.call("catalog_load", json!({"name": "ghost"})).is_err());
        assert!(
            rig.call("fly", json!({}))
                .unwrap_err()
                .contains("unknown window call")
        );
    }
}
