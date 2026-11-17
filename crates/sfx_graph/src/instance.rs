//! How a render reaches other sounds: the `sound` node instances another
//! document (an open one, or one saved in the catalog), and `voices` renders
//! a sub-patch per event. The session hands each render a [`SoundSource`].

use std::fmt;
use std::sync::Arc;

use crate::SoundDoc;

/// Looks sounds up by name (open sounds first, then the catalog).
#[derive(Clone)]
pub struct SoundSource {
    names: Arc<Vec<String>>,
    get: Arc<dyn Fn(&str) -> Option<SoundDoc> + Send + Sync>,
}

impl SoundSource {
    /// `names` are what a lookup error lists; `get` finds one by name.
    pub fn new(
        names: Vec<String>,
        get: impl Fn(&str) -> Option<SoundDoc> + Send + Sync + 'static,
    ) -> SoundSource {
        SoundSource {
            names: Arc::new(names),
            get: Arc::new(get),
        }
    }

    pub fn get(&self, name: &str) -> Option<SoundDoc> {
        (self.get)(name)
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }
}

impl fmt::Debug for SoundSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SoundSource({} names)", self.names.len())
    }
}

/// How deep `sound` instances may nest.
pub const MAX_DEPTH: usize = 8;
