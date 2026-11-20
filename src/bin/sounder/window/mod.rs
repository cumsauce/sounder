//! The window: a system webview (`wry`) on a `tao` event loop on
//! the main thread, showing the page embedded from `ui/`.
//!
//! Data flow: the page calls `sounder.call(name, args)` -> `window.ipc` ->
//! [`Window::run`]'s ipc handler -> a `Cmd::Exec` on the session thread
//! ([`api::call`], client `window`) -> the reply goes back through an
//! `EventLoopProxy` user event -> `sounder._reply(id, ...)`. When any command
//! (from any client) bumps the session's `rev`, the session thread wakes the
//! loop and the page gets `sounder.changed(rev)`; the page then asks for
//! `state`. Bulk data (spectrogram, wave, loudness) is fetched by the page
//! from the `sounder://` protocol ([`protocol`]). The player's events become
//! `sounder.played({...})`.

pub mod api;
pub mod dev;
pub mod player_iface;
pub mod protocol;

/// Bulk-data requests being served (bounded: each renders a spectrogram).
static INFLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use sfx_graph::Audio;
use sounder::session::Session;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::WindowBuilder;
use wry::WebViewBuilder;
use wry::http::{Response, header};

use self::player_iface::{PlayerControl, PlayerEvent};
use crate::server::{Cmd, OnChange};

/// Things the loop is told from other threads.
#[derive(Debug)]
pub enum Ui {
    /// The result of the page's call `id`, as JSON text `{ok, result|error}`.
    Reply { id: Value, payload: String },
    /// The session's `rev` moved.
    Changed(u64),
    /// The player's state, as JSON text.
    Played(String),
    /// Script to run in the page (the layout probe, `SOUNDER_PROBE`).
    Eval(String),
}

/// The window, before it runs. Create it on the main thread.
pub struct Window {
    event_loop: EventLoop<Ui>,
}

/// Where the render endpoints get audio: the session thread.
struct SessionBackend {
    tx: mpsc::Sender<Cmd>,
}

impl protocol::Backend for SessionBackend {
    fn audio(&self, doc: &str, node: Option<&str>) -> Result<Audio, String> {
        let (rtx, rrx) = mpsc::channel();
        let (doc, node) = (doc.to_string(), node.map(String::from));
        self.tx
            .send(Cmd::Exec(Box::new(move |s: &mut Session| {
                let _ = rtx.send(s.node_audio(Some(&doc), node.as_deref()));
            })))
            .map_err(|_| "the session thread is not running".to_string())?;
        rrx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| "timed out waiting for the session to render".to_string())?
    }
}

fn js_call(f: &str, args: &str) -> String {
    format!("window.sounder&&sounder.{f}&&sounder.{f}({args})")
}

impl Window {
    pub fn new() -> Window {
        Window {
            event_loop: EventLoopBuilder::<Ui>::with_user_event().build(),
        }
    }

    /// The hook that wakes the loop when the session's `rev` moves; give it
    /// to `run_session`.
    pub fn change_hook(&self) -> OnChange {
        let proxy = self.event_loop.create_proxy();
        Box::new(move |rev| {
            let _ = proxy.send_event(Ui::Changed(rev));
        })
    }

    /// Opens the window and runs until it is closed.
    pub fn run(
        mut self,
        tx: mpsc::Sender<Cmd>,
        player: Arc<dyn PlayerControl>,
    ) -> Result<(), String> {
        let proxy = self.event_loop.create_proxy();
        let shared = api::Shared::new(Arc::clone(&player), tx.clone());
        install_player_events(&player, &shared, &proxy);

        let window = WindowBuilder::new()
            .with_title("sounder")
            .with_inner_size(LogicalSize::new(1360.0, 860.0))
            .with_window_icon(window_icon())
            .build(&self.event_loop)
            .map_err(|e| format!("could not open the window: {e}"))?;

        set_dock_icon();

        let backend = Arc::new(SessionBackend { tx: tx.clone() });
        let ipc_proxy = proxy.clone();
        let ipc_tx = tx.clone();
        let ipc_shared = Arc::clone(&shared);
        let builder = WebViewBuilder::new()
            .with_devtools(cfg!(debug_assertions))
            .with_asynchronous_custom_protocol("sounder".into(), move |_id, request, responder| {
                let backend = Arc::clone(&backend);
                let (method, host, path, query) = (
                    request.method().to_string(),
                    request.uri().host().unwrap_or("").to_string(),
                    request.uri().path().to_string(),
                    request.uri().query().unwrap_or("").to_string(),
                );
                // Renders can take a moment: off the UI thread, and not
                // unbounded in number.
                if INFLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 16 {
                    INFLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    let mut busy = Response::new(Vec::new());
                    *busy.status_mut() = wry::http::StatusCode::SERVICE_UNAVAILABLE;
                    responder.respond(busy);
                    return;
                }
                std::thread::spawn(move || {
                    let r = protocol::respond(&*backend, &method, &host, &path, &query);
                    let cache = if r.immutable {
                        "public, max-age=31536000, immutable"
                    } else {
                        "no-store"
                    };
                    let resp = Response::builder()
                        .status(r.status)
                        .header(header::CONTENT_TYPE, r.content_type)
                        .header(header::CACHE_CONTROL, cache)
                        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                        .body(r.body)
                        .unwrap_or_else(|_| Response::new(Vec::new()));
                    responder.respond(resp);
                    INFLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                });
            })
            .with_ipc_handler(move |req| {
                handle_ipc(req.body(), &ipc_tx, &ipc_shared, &ipc_proxy);
            })
            .with_url("sounder://app/index.html");

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let webview = builder
            .build(&window)
            .map_err(|e| format!("could not start the webview: {e}"))?;
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let webview = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            let vbox = window
                .default_vbox()
                .ok_or("no GTK box to put the webview in")?;
            builder
                .build_gtk(vbox)
                .map_err(|e| format!("could not start the webview: {e}"))?
        };

        // Dev aid: `SOUNDER_PROBE=/path/out.json` makes the window measure
        // its own rendered page a few seconds after it opens and write the
        // result there (see `PROBE_JS`), so the native webview can be checked
        // without a screenshot.
        if std::env::var_os("SOUNDER_PROBE").is_some() {
            let p = proxy.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(6));
                let _ = p.send_event(Ui::Eval(PROBE_JS.to_string()));
            });
        }
        self.event_loop.run_return(move |event, _, flow| {
            *flow = ControlFlow::Wait;
            let _keep = (&window, &webview);
            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => *flow = ControlFlow::Exit,
                Event::UserEvent(Ui::Reply { id, payload }) => {
                    let _ =
                        webview.evaluate_script(&js_call("_reply", &format!("{id}, {payload}")));
                }
                Event::UserEvent(Ui::Changed(rev)) => {
                    let _ = webview.evaluate_script(&js_call("changed", &rev.to_string()));
                }
                Event::UserEvent(Ui::Eval(js)) => {
                    let _ = webview.evaluate_script(&js);
                }
                Event::UserEvent(Ui::Played(json)) => {
                    let _ = webview.evaluate_script(&js_call("played", &json));
                }
                _ => {}
            }
        });
        Ok(())
    }
}

/// One page message `{id, name, args}` -> the session thread -> a reply event.
fn handle_ipc(
    body: &str,
    tx: &mpsc::Sender<Cmd>,
    shared: &Arc<api::Shared>,
    proxy: &EventLoopProxy<Ui>,
) {
    let msg: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sounder: window sent a message that is not JSON: {e}");
            return;
        }
    };
    if msg.get("name").and_then(Value::as_str) == Some("probe_report") {
        if let Some(path) = std::env::var_os("SOUNDER_PROBE") {
            let _ = std::fs::write(path, msg["args"].to_string());
        }
        return;
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let name = msg
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let args = msg.get("args").cloned().unwrap_or_else(|| json!({}));
    let (proxy, shared) = (proxy.clone(), Arc::clone(shared));
    let reply_proxy = proxy.clone();
    let reply_id = id.clone();
    let sent = tx.send(Cmd::Exec(Box::new(move |s: &mut Session| {
        let payload = match api::call(s, &shared, &name, &args) {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => json!({"ok": false, "error": error}),
        };
        let _ = proxy.send_event(Ui::Reply {
            id,
            payload: payload.to_string(),
        });
    })));
    if sent.is_err() {
        let payload = json!({"ok": false, "error": "the session thread is not running"});
        let _ = reply_proxy.send_event(Ui::Reply {
            id: reply_id,
            payload: payload.to_string(),
        });
    }
}

/// Turns the player's events into `sounder.played({playing, pos, loop, span, node})`.
fn install_player_events(
    player: &Arc<dyn PlayerControl>,
    shared: &Arc<api::Shared>,
    proxy: &EventLoopProxy<Ui>,
) {
    #[derive(Default)]
    struct Last {
        looped: bool,
        span: Option<[f32; 2]>,
    }
    let (proxy, shared) = (proxy.clone(), Arc::clone(shared));
    let last = Mutex::new(Last::default());
    player.set_event_handler(Box::new(move |e| {
        let Ok(mut last) = last.lock() else { return };
        let (playing, pos) = match e {
            PlayerEvent::Started { pos, looped, span } => {
                last.looped = looped;
                last.span = span;
                (true, pos)
            }
            PlayerEvent::Stopped { pos } => (false, pos),
            PlayerEvent::Looped => (true, last.span.map_or(0.0, |s| s[0])),
            PlayerEvent::Seeked { pos } => (shared.player.playing(), pos),
        };
        let node = shared.solo.lock().ok().and_then(|s| s.clone());
        let j = json!({
            "playing": playing, "pos": pos, "loop": last.looped,
            "span": last.span, "node": node,
        });
        let _ = proxy.send_event(Ui::Played(j.to_string()));
    }));
}

/// Measures the rendered page: the engine, fonts, canvas sizes, and the rect
/// and computed style of the elements that carry the layout. Posts it back as
/// the `probe_report` message.
const PROBE_JS: &str = r#"(() => {
  const sels = ['body','.top','.where','.seed','#hudbar','.plot','#base','.dock','.knob','.right','.tabs','.sect','.field','.cmt','.chip','.tbtn','.gnode','#hearing'];
  const out = { ua: navigator.userAgent, dpr: devicePixelRatio, inner: [innerWidth, innerHeight], fonts: [...document.fonts].filter(f => f.status === 'loaded').map(f => f.family + ' ' + f.weight), els: {}, errors: window.__errs || [] };
  for (const s of sels) {
    const e = document.querySelector(s); if (!e) { out.els[s] = null; continue; }
    const r = e.getBoundingClientRect(), c = getComputedStyle(e);
    out.els[s] = { x: +r.x.toFixed(1), y: +r.y.toFixed(1), w: +r.width.toFixed(1), h: +r.height.toFixed(1), font: c.fontFamily.split(',')[0] + ' ' + c.fontSize + ' ' + c.fontWeight, color: c.color, bg: c.backgroundColor };
  }
  out.canvas = [...document.querySelectorAll('canvas')].slice(0, 6).map(c => [c.id || c.className, c.width, c.height]);
  out.paint = [...document.querySelectorAll('canvas')].slice(0, 6).map(c => {
    try { const x = c.getContext('2d'), d = x.getImageData(0, 0, c.width, c.height).data; let lit = 0, sum = 0; for (let i = 0; i < d.length; i += 4 * 17) { if (d[i + 3] > 0 && (d[i] + d[i + 1] + d[i + 2]) > 30) lit++; sum += d[i] + d[i + 1] + d[i + 2]; } return [c.id || c.className, lit, Math.round(sum / (d.length / 68))]; } catch (e) { return [c.id, 'err ' + e]; }
  });
  out.state = typeof ST !== 'undefined' && ST ? { doc: ST.doc, sounds: (ST.sounds || []).length, nodes: ST.sound && ST.sound.nodes.length } : null;
  window.ipc.postMessage(JSON.stringify({ name: 'probe_report', args: out }));
})()"#;

/// The app icon (a struck sound spiralling in, in the spectrogram's palette;
/// `icon.svg` is its source).
const ICON_PNG: &[u8] = include_bytes!("icon.png");

/// The icon as a tao window icon (the title bar / taskbar off macOS).
fn window_icon() -> Option<tao::window::Icon> {
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    tao::window::Icon::from_rgba(buf, info.width, info.height).ok()
}

/// macOS takes the Dock icon from the application, not the window.
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {}
