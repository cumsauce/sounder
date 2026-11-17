//! `sounder`: an MCP server for procedural sound effects.
//!
//! `sounder --headless` serves everything except `play` over stdio and exits
//! when stdin closes. The window (phase 5) is `sounder` without --headless.
//! The server runs on its own thread with its own tokio runtime; the session
//! lives on another and takes commands over a channel (the window will
//! drain that channel from its frame loop).

mod server;

use std::path::PathBuf;
use std::sync::mpsc;

use rmcp::ServiceExt;
use sounder::session::Session;
use sounder::store;

const USAGE: &str = "usage: sounder --headless [--project DIR] [--restore] [--feedback-report]
  --headless          serve MCP on stdio (the only mode until the window exists)
  --project DIR       the game project (default: the nearest ancestor with .sounder/)
  --restore           reopen the sounds left in the autosave
  --feedback-report   print the friction log's report and exit";

fn main() {
    let mut headless = false;
    let mut restore = false;
    let mut report = false;
    let mut print_dir = false;
    let mut project: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--headless" => headless = true,
            "--restore" => restore = true,
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
    if !headless {
        eprintln!(
            "sounder: the window arrives with phase 5; run `sounder --headless` for the MCP server"
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

    let (tx, rx) = mpsc::channel::<server::Cmd>();
    let done = server::Done::new();
    let server_done = done.clone();
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
            let server = match server::Tools::new(tx).serve(rmcp::transport::stdio()).await {
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
    let _session = server::run_headless(session, rx, done);
    store::release_lock_at(&lock, pid);
    let _ = server_thread.join();
}
