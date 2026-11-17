//! The session lives on one thread; the MCP server (its own tokio runtime)
//! sends it closures. The window (phase 5, a webview on the main thread) will
//! be one more sender on the same channel.

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

/// Runs commands until the server is done; autosaves once edits pause and on exit.
pub(crate) fn run_headless(
    mut session: Session,
    rx: mpsc::Receiver<Cmd>,
    done: Arc<Done>,
) -> Session {
    loop {
        let wait = session
            .time_to_autosave()
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(10), Duration::from_millis(200));
        match rx.recv_timeout(wait) {
            Ok(Cmd::Exec(f)) => f(&mut session),
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
