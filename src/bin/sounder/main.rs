//! `sounder`: an MCP server for procedural sound effects.
//!
//! `sounder --headless` serves everything except `play` over stdio and exits
//! when stdin closes. `sounder` (window feature) also opens the window, on
//! the main thread. The MCP server runs on its own thread with its own tokio
//! runtime; the session lives on another and takes commands over a channel
//! (the window is one more sender on it).

mod server;
#[cfg(feature = "window")]
mod window;

use std::path::PathBuf;
use std::sync::mpsc;

use rmcp::ServiceExt;
use sounder::session::Session;
use sounder::store;

const USAGE: &str =
    "usage: sounder [--headless [--audio]] [--project DIR] [--restore] [--feedback-report]
  (no flag)           the window (needs the default `window` feature) and the MCP server on stdio
  --headless          serve MCP on stdio without opening the window
  --audio             with --headless: also open the speakers so `play` works
  --project DIR       the game project (default: the nearest ancestor with .sounder/)
  --restore           reopen the sounds left in the autosave
  --feedback-report   print the friction log's report and exit
  --dev-ui PORT       with --headless (window build): serve the page and its data over HTTP on
                      127.0.0.1:PORT so a browser can drive the real page (dev only)";

fn main() {
    let mut headless = false;
    let mut restore = false;
    let mut report = false;
    let mut audio = false;
    let mut print_dir = false;
    let mut dev_ui: Option<u16> = None;
    let mut project: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--headless" => headless = true,
            "--restore" => restore = true,
            "--dev-ui" => match args.next().and_then(|p| p.parse::<u16>().ok()) {
                Some(p) => dev_ui = Some(p),
                None => {
                    eprintln!("sounder: --dev-ui needs a port number\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "--audio" => audio = true,
            "--feedback-report" => report = true,
            "--print-catalog-dir" => print_dir = true,
            "--project" => match args.next() {
                Some(p) => project = Some(PathBuf::from(p)),
                None => {
                    eprintln!("sounder: --project needs a folder\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("sounder: unknown argument `{other}`\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let project = project.or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| store::find_project(&d))
    });
    let mut session = Session::new(project.clone());
    if print_dir {
        match session.library_dir() {
            Ok(d) => println!("{}", d.display()),
            Err(e) => {
                eprintln!("sounder: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if report {
        println!(
            "{}",
            sounder::feedback::report(&sounder::feedback::read(), None)
        );
        return;
    }
    #[cfg(not(feature = "window"))]
    if !headless {
        eprintln!(
            "sounder: this build has no window (the `window` feature is off); run `sounder --headless` for the MCP server"
        );
        std::process::exit(2);
    }

    // One autosaving server per library: a second live one writes its own slot.
    let lib = match session.library_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("sounder: {e}");
            std::process::exit(1);
        }
    };
    let lock = lib.join("session.lock");
    let pid = std::process::id();
    match store::acquire_lock_at(&lock, pid) {
        Ok(store::LockOutcome::Owner) => {}
        Ok(store::LockOutcome::HeldBy(other)) => {
            let slot = std::env::var("SOUNDER_SLOT")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("pid{pid}"));
            eprintln!(
                "sounder: autosave SLOT {slot}: server pid {other} holds the session lock in this library, so this one autosaves to session.{slot}.json"
            );
            session.slot = Some(slot);
        }
        Err(e) => {
            eprintln!("sounder: {e}");
            session.autosave_ok = false;
        }
    }
    if let Some(root) = &project {
        store::ensure_project_gitignore(root);
    }
    if restore {
        match session.restore() {
            Ok(0) => {}
            Ok(n) => eprintln!("restored {n} sound(s) from the autosave"),
            Err(e) => eprintln!("sounder: could not restore: {e}"),
        }
    }

    // The window plays through the same player the MCP `play` tool uses; a
    // missing device is not fatal there (the window just cannot play), but it
    // is for `--audio`, which asked for it.
    let window_mode = cfg!(feature = "window") && !headless;
    let player = start_audio(audio || window_mode, audio);
    #[cfg(feature = "window")]
    let window_player = player.clone();
    let (tx, rx) = mpsc::channel::<server::Cmd>();
    #[cfg(feature = "window")]
    let window_tx = tx.clone();
    #[cfg(feature = "window")]
    let dev_tx = tx.clone();
    #[cfg(feature = "window")]
    let window_player_for_dev = player.clone();
    let done = server::Done::new();
    // Headless, the MCP client going away ends the session; with the window
    // open only closing the window does.
    let server_done = if headless {
        done.clone()
    } else {
        server::Done::new()
    };
    let server_thread = std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("sounder: could not start the MCP runtime: {e}");
                server_done.set();
                return;
            }
        };
        rt.block_on(async move {
            let server = match server::Tools::new(tx, player)
                .serve(rmcp::transport::stdio())
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("sounder: MCP server failed to start: {e}");
                    server_done.set();
                    return;
                }
            };
            let why = match server.waiting().await {
                Ok(_) => "stdin closed (MCP client gone)".to_string(),
                Err(e) => format!("MCP server stopped: {e}"),
            };
            eprintln!("sounder: exit: {why}");
            server_done.set();
        });
    });
    #[cfg(feature = "window")]
    if !headless {
        // The window owns the main thread; the session runs on its own.
        let win = window::Window::new();
        let hook = win.change_hook();
        let session_done = done.clone();
        let session_thread =
            std::thread::spawn(move || server::run_session(session, rx, session_done, Some(hook)));
        // Without a device `play` says so (the window's Play button errors).
        let player: std::sync::Arc<dyn sounder::player::PlayerControl> =
            window_player.unwrap_or_else(|| std::sync::Arc::new(window::player_iface::NullPlayer));
        let shown = win.run(window_tx, player);
        done.set();
        let _ = session_thread.join();
        store::release_lock_at(&lock, pid);
        if let Err(e) = shown {
            eprintln!("sounder: {e}");
            std::process::exit(1);
        }
        // The MCP server thread is blocked reading stdin: leave without it.
        std::process::exit(0);
    }
    // Dev only: the real page over HTTP (see window/dev.rs).
    #[cfg(feature = "window")]
    let dev_hook: Option<server::OnChange> = match dev_ui {
        Some(port) => {
            let rev = std::sync::Arc::new(window::dev::Rev::default());
            let player: std::sync::Arc<dyn sounder::player::PlayerControl> = window_player_for_dev
                .unwrap_or_else(|| std::sync::Arc::new(window::player_iface::NullPlayer));
            let port = match window::dev::serve(port, dev_tx, player, std::sync::Arc::clone(&rev)) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("sounder: {e}");
                    std::process::exit(1);
                }
            };
            eprintln!(
                "sounder: dev ui at http://127.0.0.1:{port}/app/index.html?bridge=http://127.0.0.1:{port}"
            );
            Some(Box::new(move |r| rev.set(r)))
        }
        None => None,
    };
    #[cfg(not(feature = "window"))]
    let dev_hook: Option<server::OnChange> = {
        if dev_ui.is_some() {
            eprintln!("sounder: --dev-ui needs a build with the `window` feature");
            std::process::exit(2);
        }
        None
    };
    let _session = server::run_session(session, rx, done, dev_hook);
    store::release_lock_at(&lock, pid);
    let _ = server_thread.join();
}

/// The speakers for `play`, when `--audio` asked for them.
#[cfg(feature = "audio")]
fn start_audio(
    want: bool,
    fatal: bool,
) -> Option<std::sync::Arc<dyn sounder::player::PlayerControl>> {
    if !want {
        return None;
    }
    match sounder::player::Player::start() {
        Ok(p) => Some(std::sync::Arc::new(p)),
        Err(e) => {
            eprintln!("sounder: {e}");
            if fatal {
                std::process::exit(1);
            }
            None
        }
    }
}

#[cfg(not(feature = "audio"))]
fn start_audio(
    _want: bool,
    fatal: bool,
) -> Option<std::sync::Arc<dyn sounder::player::PlayerControl>> {
    if fatal {
        eprintln!("sounder: --audio needs a build with the `audio` feature (this one has none)");
        std::process::exit(2);
    }
    None
}
