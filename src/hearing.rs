//! `heard_as`: what an audio-text model hears in a sound, as a hint.
//!
//! The model runs in a Python worker (`tools/hearing/heard_as.py`) started
//! on the first `listen` and kept for the session: it loads the model once
//! and answers one JSON line per request. Nothing here is a check: the
//! model named the intended family first on 18 of 40 labelled cues, and
//! ranked a crate drop the listener called great as "a snare drum".
//!
//! The worker's Python is `SOUNDER_HEARING_PYTHON`, else
//! `~/.local/share/sounder-hearing/venv/bin/python` when it exists; the
//! script is `SOUNDER_HEARING_SCRIPT`, else the repo's copy. Without a
//! Python the hint is off and `listen` says so in one line;
//! `SOUNDER_HEARING=off` turns it off (the tests that drive the binary).

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde::Deserialize;
use sfx_graph::Audio;

/// The first reply (the model loading) may take this long.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Every later reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// One answer: the closest descriptions, and where the brief ranks.
#[derive(Clone, Debug, Deserialize)]
pub struct Heard {
    pub top: Vec<(String, f32)>,
    #[serde(default)]
    pub brief: Option<BriefRank>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BriefRank {
    pub score: f32,
    pub rank: usize,
    pub of: usize,
}

impl Heard {
    /// `heard_as (hint ...): a, b, c; the brief ranks r of n`.
    pub fn line(&self, model: &str) -> String {
        let top: Vec<String> = self
            .top
            .iter()
            .map(|(t, s)| format!("{t} {s:.2}"))
            .collect();
        let mut l = format!(
            "heard_as (a hint from {model}, not a check: it often misnames sounds the listener rated good): {}",
            top.join(", ")
        );
        if let Some(b) = &self.brief {
            l += &format!(
                "; the spec's brief ranks {} of {} (score {:.2})",
                b.rank, b.of, b.score
            );
        }
        l
    }
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    model: String,
}

/// The worker, started on first use.
#[derive(Default)]
pub struct Hearing {
    worker: Option<Worker>,
    /// Why the hint is off, once it is.
    off: Option<String>,
    /// Override the Python and the script (tests).
    pub python: Option<PathBuf>,
    pub script: Option<PathBuf>,
}

impl std::fmt::Debug for Hearing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hearing")
            .field("running", &self.worker.is_some())
            .field("off", &self.off)
            .finish()
    }
}

impl Drop for Hearing {
    fn drop(&mut self) {
        if let Some(w) = &mut self.worker {
            let _ = w.child.kill();
            let _ = w.child.wait();
        }
    }
}

impl Hearing {
    /// A hearing that is off, with the reason `listen` prints.
    pub fn off(reason: &str) -> Hearing {
        let mut h = Hearing::default();
        h.off = Some(reason.to_string());
        h
    }

    fn python(&self) -> Option<PathBuf> {
        if let Some(p) = &self.python {
            return Some(p.clone());
        }
        if let Some(p) = std::env::var_os("SOUNDER_HEARING_PYTHON") {
            return Some(PathBuf::from(p));
        }
        let home = std::env::var_os("HOME")?;
        let p = PathBuf::from(home).join(".local/share/sounder-hearing/venv/bin/python");
        p.exists().then_some(p)
    }

    fn script(&self) -> PathBuf {
        self.script
            .clone()
            .or_else(|| std::env::var_os("SOUNDER_HEARING_SCRIPT").map(PathBuf::from))
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/hearing/heard_as.py")
            })
    }

    fn start(&mut self) -> Result<(), String> {
        if self.python.is_none() && std::env::var_os("SOUNDER_HEARING").is_some_and(|v| v == "off")
        {
            return Err("SOUNDER_HEARING=off".into());
        }
        let Some(py) = self.python() else {
            return Err("no Python for the model: set SOUNDER_HEARING_PYTHON to a venv's python with torch, transformers and soundfile".into());
        };
        let script = self.script();
        let mut child = Command::new(&py)
            .arg("-I")
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", py.display()))?;
        let stdin = child.stdin.take().ok_or("worker has no stdin")?;
        let stdout = child.stdout.take().ok_or("worker has no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let ready = rx
            .recv_timeout(LOAD_TIMEOUT)
            .map_err(|_| format!("{} did not start (no ready line)", script.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&ready).map_err(|e| format!("worker said {ready:?}: {e}"))?;
        let model = v
            .get("ready")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("worker said {ready:?}, not ready"))?
            .to_string();
        self.worker = Some(Worker {
            child,
            stdin,
            lines: rx,
            model,
        });
        Ok(())
    }

    /// What the model hears in `audio` (and how `brief` ranks), with the
    /// model's name; Err(reason) when the hint is off.
    pub fn hear(&mut self, audio: &Audio, brief: &str) -> Result<(Heard, String), String> {
        if let Some(r) = &self.off {
            return Err(r.clone());
        }
        if self.worker.is_none()
            && let Err(e) = self.start()
        {
            self.off = Some(e.clone());
            return Err(e);
        }
        let path =
            std::env::temp_dir().join(format!("sounder_heard_as_{}.wav", std::process::id()));
        sfx_graph::wav::write(&path, audio, sfx_graph::wav::Bits::Float32)?;
        let req = serde_json::json!({"wav": path, "brief": brief});
        let w = self.worker.as_mut().ok_or("no worker")?;
        let sent = writeln!(w.stdin, "{req}").and_then(|()| w.stdin.flush());
        let reply = match sent {
            Ok(()) => w.lines.recv_timeout(REPLY_TIMEOUT).ok(),
            Err(_) => None,
        };
        let _ = std::fs::remove_file(&path);
        let Some(reply) = reply else {
            // A dead or stuck worker: stop it; the next listen starts anew.
            let _ = w.child.kill();
            let _ = w.child.wait();
            self.worker = None;
            return Err(
                "the model worker stopped answering (it restarts on the next listen)".into(),
            );
        };
        let v: serde_json::Value =
            serde_json::from_str(&reply).map_err(|e| format!("worker said {reply:?}: {e}"))?;
        if let Some(e) = v.get("error").and_then(serde_json::Value::as_str) {
            return Err(format!("the model worker failed: {e}"));
        }
        let h: Heard = serde_json::from_value(v).map_err(|e| format!("worker reply: {e}"))?;
        Ok((h, w.model.clone()))
    }
}
