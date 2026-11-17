//! A dev-only HTTP bridge to the window's data path: `sounder --headless
//! --dev-ui PORT` serves the page and the same calls / bulk data the webview
//! gets, over plain HTTP on 127.0.0.1, so a browser (or an agent's browser)
//! can drive the REAL page against the REAL session. Not part of the product:
//! local only (Host / Origin checked, JSON-only POSTs), std only.
//!
//! - `GET /<host>/<path>?<query>`: [`protocol::respond`] (`/app/index.html`,
//!   `/fonts/<file>`, `/render/<doc>/spectrogram?...`)
//! - `POST /call` with `{name, args}`: [`api::call`], reply `{ok, result|error}`
//! - `GET /poll?since=N`: waits up to 20 s for the session's `rev` to pass N,
//!   answers `{rev}` (the page turns it into `sounder.changed`)
//!
//! The page switches to it with `?bridge=http://127.0.0.1:PORT`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use sounder::session::Session;

use super::player_iface::PlayerControl;
use super::{SessionBackend, api, protocol};
use crate::server::Cmd;

/// The session's revision, with a way to wait for it to move.
#[derive(Default)]
pub struct Rev {
    value: Mutex<u64>,
    moved: Condvar,
}

impl Rev {
    pub fn set(&self, rev: u64) {
        if let Ok(mut v) = self.value.lock() {
            *v = rev;
        }
        self.moved.notify_all();
    }

    fn wait_past(&self, since: u64, within: Duration) -> u64 {
        let Ok(g) = self.value.lock() else {
            return since;
        };
        match self.moved.wait_timeout_while(g, within, |v| *v <= since) {
            Ok((g, _)) => *g,
            Err(_) => since,
        }
    }
}

/// Starts the bridge on 127.0.0.1:`port` (its own threads).
pub fn serve(
    port: u16,
    tx: mpsc::Sender<Cmd>,
    player: Arc<dyn PlayerControl>,
    rev: Arc<Rev>,
) -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("--dev-ui: cannot listen on 127.0.0.1:{port}: {e}"))?;
    let bound = listener
        .local_addr()
        .map_err(|e| format!("--dev-ui: {e}"))?
        .port();
    let shared = api::Shared::new(player, tx.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, shared, rev) = (tx.clone(), Arc::clone(&shared), Arc::clone(&rev));
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            std::thread::spawn(move || {
                let _ = handle(stream, bound, &tx, &shared, &rev);
            });
        }
    });
    Ok(bound)
}

fn reply(s: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)
}

fn handle(
    mut s: TcpStream,
    port: u16,
    tx: &mpsc::Sender<Cmd>,
    shared: &Arc<api::Shared>,
    rev: &Arc<Rev>,
) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?.take(16 * 1024));
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut it = line.split_whitespace();
    let (method, target) = (it.next().unwrap_or(""), it.next().unwrap_or("/"));
    let (method, target) = (method.to_string(), target.to_string());
    let mut len = 0usize;
    let (mut host_ok, mut origin_ok, mut json_ok) = (false, true, false);
    let same = |v: &str| {
        let v = v.trim().trim_start_matches("http://");
        v == "127.0.0.1"
            || v == "localhost"
            || v == format!("127.0.0.1:{port}")
            || v == format!("localhost:{port}")
    };
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 {
            return reply(&mut s, 431, "text/plain", b"headers too large");
        }
        if h.trim().is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = lower.strip_prefix("host:") {
            host_ok = same(v);
        } else if let Some(v) = lower.strip_prefix("origin:") {
            origin_ok = same(v);
        } else if let Some(v) = lower.strip_prefix("content-type:") {
            json_ok = v.trim().starts_with("application/json");
        }
    }
    // Only the page this bridge serves may talk to it: a Host check stops
    // DNS rebinding, an Origin check and a JSON-only POST stop other sites.
    if !host_ok || !origin_ok {
        return reply(&mut s, 403, "text/plain", b"forbidden: host or origin");
    }
    let (method, target) = (method.as_str(), target.as_str());
    if method == "POST" && !json_ok {
        return reply(
            &mut s,
            415,
            "text/plain",
            b"POST needs Content-Type: application/json",
        );
    }
    r.get_mut().set_limit(len.min(1 << 20) as u64);
    let mut body = vec![0u8; len.min(1 << 20)];
    r.read_exact(&mut body)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if method == "OPTIONS" {
        return reply(&mut s, 204, "text/plain", b"");
    }
    match (method, path) {
        ("POST", "/call") => {
            let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let name = req["name"].as_str().unwrap_or("").to_string();
            let args = req.get("args").cloned().unwrap_or(json!({}));
            let (rtx, rrx) = mpsc::channel();
            let sh = Arc::clone(shared);
            let sent = tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
                let _ = rtx.send(api::call(s, &sh, &name, &args));
            })));
            let out = match sent
                .map_err(|_| "the session is gone".to_string())
                .and_then(|()| {
                    rrx.recv_timeout(Duration::from_secs(120))
                        .map_err(|_| "the call timed out".to_string())?
                }) {
                Ok(v) => json!({"ok": true, "result": v}),
                Err(e) => json!({"ok": false, "error": e}),
            };
            reply(&mut s, 200, "application/json", out.to_string().as_bytes())
        }
        ("GET", "/poll") => {
            let since = query
                .split('&')
                .find_map(|p| p.strip_prefix("since="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let now = rev.wait_past(since, Duration::from_secs(20));
            reply(
                &mut s,
                200,
                "application/json",
                json!({"rev": now}).to_string().as_bytes(),
            )
        }
        ("GET", p) => {
            let p = p.trim_start_matches('/');
            let (host, rest) = p.split_once('/').unwrap_or((p, ""));
            let be = SessionBackend { tx: tx.clone() };
            let resp = protocol::respond(&be, "GET", host, &format!("/{rest}"), query);
            reply(&mut s, resp.status, resp.content_type, &resp.body)
        }
        _ => reply(&mut s, 405, "text/plain", b"method not allowed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Done, run_session};
    use crate::window::player_iface::NullPlayer;
    use serde_json::json;
    use sfx_graph::DocEdit;
    use sounder::session::Session;

    fn http(port: u16, req: &str) -> (u16, String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.write_all(req.as_bytes()).expect("send");
        let mut all = Vec::new();
        s.read_to_end(&mut all).expect("read");
        let split = all
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("headers")
            + 4;
        let head = String::from_utf8_lossy(&all[..split]).to_string();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        (status, head, all[split..].to_vec())
    }

    fn post(port: u16, path: &str, body: &str) -> (u16, String, Vec<u8>) {
        http(
            port,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    #[test]
    fn other_hosts_origins_and_plain_posts_are_refused() {
        let (tx, _rx) = mpsc::channel();
        let port = serve(0, tx, Arc::new(NullPlayer), Arc::new(Rev::default())).expect("serve");
        let (st, _, _) = http(
            port,
            "GET /app/index.html HTTP/1.1\r\nHost: evil.example\r\n\r\n",
        );
        assert_eq!(st, 403);
        let (st, _, _) = http(
            port,
            "GET /app/index.html HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\n\r\n",
        );
        assert_eq!(st, 403);
        let (st, _, _) = http(
            port,
            &format!(
                "POST /call HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\n{{}}"
            ),
        );
        assert_eq!(st, 415);
    }

    #[test]
    fn the_bridge_serves_the_page_the_calls_the_bulk_data_and_rev_changes() {
        let dir = std::env::temp_dir().join(format!("sounder_dev_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let mut session = Session::new(Some(dir));
        session
            .new_sound("tone", sfx_graph::Kind::Oneshot)
            .expect("new");
        session
            .apply_ops(
                None,
                vec![
                    json!({"type": "osc", "name": "o", "freq": 440, "amp": 0.5}),
                    json!({"type": "decay", "name": "e", "time": 0.2}),
                    json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
                    json!({"type": "output", "name": "out", "input": "m"}),
                ],
                DocEdit::default(),
            )
            .expect("build");
        let (tx, rx) = mpsc::channel();
        let rev = Arc::new(Rev::default());
        let hook_rev = Arc::clone(&rev);
        let done = Done::new();
        let session_done = done.clone();
        let t = std::thread::spawn(move || {
            run_session(
                session,
                rx,
                session_done,
                Some(Box::new(move |r| hook_rev.set(r))),
            )
        });
        let port = serve(0, tx.clone(), Arc::new(NullPlayer), Arc::clone(&rev)).expect("serve");

        // The page.
        let (st, head, body) = http(
            port,
            "GET /app/index.html HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert_eq!(st, 200, "{head}");
        assert!(
            head.contains("text/html") && !head.contains("Access-Control-Allow-Origin"),
            "{head}"
        );
        assert!(String::from_utf8_lossy(&body).contains("<html"));
        // A call, as the page makes it.
        let (st, _, body) = post(port, "/call", r#"{"name":"state","args":{}}"#);
        assert_eq!(st, 200);
        let v: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["result"]["doc"], "tone");
        assert_eq!(
            v["result"]["sound"]["nodes"].as_array().map(Vec::len),
            Some(4)
        );
        // A call that fails answers ok: false with the text.
        let (_, _, body) = post(
            port,
            "/call",
            r#"{"name":"move_knob","args":{"name":"nope","value":1}}"#,
        );
        let v: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap_or("").contains("nope"), "{v}");
        // Bulk data: 3 arrays of w floats for the wave.
        let (st, _, body) = http(
            port,
            "GET /render/tone/wave?w=10 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert_eq!((st, body.len()), (200, 3 * 10 * 4));
        // An unknown route 404s with a reason.
        let (st, _, body) = http(port, "GET /nowhere/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        assert_eq!(st, 404, "{}", String::from_utf8_lossy(&body));
        // poll: returns at once when the rev is already past `since`, and
        // wakes when a change moves it.
        let (_, _, body) = http(
            port,
            "GET /poll?since=0 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        );
        let r0 = serde_json::from_slice::<Value>(&body).expect("json")["rev"]
            .as_u64()
            .expect("rev");
        assert!(r0 >= 1, "{r0}");
        let waiter = std::thread::spawn(move || {
            let (_, _, body) = http(
                port,
                &format!("GET /poll?since={r0} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
            );
            serde_json::from_slice::<Value>(&body).expect("json")["rev"]
                .as_u64()
                .expect("rev")
        });
        std::thread::sleep(Duration::from_millis(200));
        let (_, _, body) = post(port, "/call", r#"{"name":"set_seed","args":{"seed":7}}"#);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).expect("json")["ok"],
            true
        );
        assert!(
            waiter.join().expect("poll") > r0,
            "the poll woke with a higher rev"
        );
        done.set();
        drop(tx);
        let _ = t.join();
    }
}
