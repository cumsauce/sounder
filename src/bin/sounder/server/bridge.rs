//! The session lives on one thread; the MCP server (its own tokio runtime)
//! sends it closures. The window (a webview on the main thread) is one more
//! sender on the same channel, and learns of changes through `OnChange`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sounder::session::Session;
use std::sync::mpsc;

/// A unit of work for the session thread.
pub(crate) enum Cmd {
    Exec(Box<dyn FnOnce(&mut Session) + Send>),
}

/// Shared between the server thread and the session thread.
pub(crate) struct Done(pub(crate) AtomicBool);

impl Done {
    pub(crate) fn new() -> Arc<Done> {
        Arc::new(Done(AtomicBool::new(false)))
    }
    pub(crate) fn set(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub(crate) fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Called with the session's `rev` after a command changed anything anyone
/// can see (the window uses it to push `changed`).
pub(crate) type OnChange = Box<dyn FnMut(u64) + Send>;

/// Runs commands until `done` is set or every sender is gone; autosaves once
/// edits pause and on exit. Both modes use it: headless (the MCP server
/// alone) and the window (one more sender on `rx`, and `on_change`).
pub(crate) fn run_session(
    mut session: Session,
    rx: mpsc::Receiver<Cmd>,
    done: Arc<Done>,
    mut on_change: Option<OnChange>,
) -> Session {
    let mut seen_rev = session.rev();
    loop {
        let wait = session
            .time_to_autosave()
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(10), Duration::from_millis(200));
        match rx.recv_timeout(wait) {
            Ok(Cmd::Exec(f)) => {
                f(&mut session);
                if session.rev() != seen_rev {
                    seen_rev = session.rev();
                    if let Some(h) = on_change.as_mut() {
                        h(seen_rev);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        session.flush_autosave(false);
        if done.is_set() {
            break;
        }
    }
    session.flush_autosave(true);
    session
}
