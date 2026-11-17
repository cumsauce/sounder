//! The edit history: a tree of snapshots with a cursor. Copied from modeler's
//! `src/lib.rs` (`Doc`, `Snapshot`, `History`), generic over the document.
//!
//! Every mutation records a child of the current node; undo walks to the
//! parent, redo to the most recent child, jump lands anywhere. Navigation is
//! exact: nodes hold full documents, never inverse operations.

use serde::{Deserialize, Serialize};
use sfx_graph::SoundDoc;

/// A document wrapper for [`SoundDoc`] (the orphan rule keeps `Doc` off it).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct Step(pub SoundDoc);

impl Doc for Step {
    fn version(&self) -> u64 {
        self.0.version
    }
}

/// A document a [`History`] can snapshot: a model, or a material graph.
pub trait Doc: Clone + std::fmt::Debug + Default + Serialize + serde::de::DeserializeOwned {
    /// Drops what a recorded snapshot need not keep in memory (data that is
    /// re-derived on navigation).
    fn trim(&mut self) {}
    /// A counter that changes with every edit (shown in `history`).
    fn version(&self) -> u64;
    /// The copy written to a file: trimmed, and never nesting a history.
    fn for_file(&self) -> Self {
        let mut d = self.clone();
        d.trim();
        d
    }
}

/// See [`History::mark`].
#[derive(Clone, Copy, Debug)]
pub struct HistoryMark {
    len: usize,
    current: Option<usize>,
    children: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot<T: Doc> {
    /// The document at this node.
    pub model: T,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub label: String,
    /// Which client (agent, or "you" for the window) made this edit, when
    /// the call said so (several agents can share one server).
    pub author: Option<String>,
}

impl<T: Doc> Serialize for Snapshot<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let m = self.model.for_file();
        let mut st = s.serialize_struct("Snapshot", 4)?;
        st.serialize_field("model", &m)?;
        st.serialize_field("parent", &self.parent)?;
        st.serialize_field("children", &self.children)?;
        st.serialize_field("label", &self.label)?;
        if let Some(a) = &self.author {
            st.serialize_field("author", a)?;
        }
        st.end()
    }
}

impl<'de, T: Doc> Deserialize<'de> for Snapshot<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound = "T: Doc")]
        struct Raw<T> {
            model: T,
            #[serde(default)]
            parent: Option<usize>,
            #[serde(default)]
            children: Vec<usize>,
            #[serde(default)]
            label: String,
            #[serde(default)]
            author: Option<String>,
        }
        let raw = Raw::<T>::deserialize(d)?;
        Ok(Snapshot {
            model: raw.model,
            parent: raw.parent,
            children: raw.children,
            label: raw.label,
            author: raw.author,
        })
    }
}

/// The history of a session as a tree of snapshots with a cursor. Every
/// mutation records a child of the current node; undo walks to the parent,
/// redo to the most recent child, and jump lands anywhere. Navigation is O(1)
/// and exact: nodes hold full models, never inverse operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(bound = "T: Doc")]
pub struct History<T: Doc> {
    pub nodes: Vec<Snapshot<T>>,
    #[serde(default)]
    pub current: Option<usize>,
}

impl<T: Doc> Default for History<T> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            current: None,
        }
    }
}

impl<T: Doc> History<T> {
    /// True when nothing has been recorded yet (skipped in serialization).
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Records `model` as a child of the current node, makes it current and
    /// returns its id.
    ///
    pub fn record(&mut self, mut model: T, label: impl Into<String>) -> usize {
        model.trim();
        let id = self.nodes.len();
        self.nodes.push(Snapshot {
            model,
            parent: self.current,
            children: Vec::new(),
            label: label.into(),
            author: None,
        });
        if let Some(p) = self.current {
            self.nodes[p].children.push(id);
        }
        self.current = Some(id);
        id
    }

    /// A point to roll back to: the node count, the cursor and how many
    /// children the cursor had. History only appends, so this is enough.
    pub fn mark(&self) -> HistoryMark {
        HistoryMark {
            len: self.nodes.len(),
            current: self.current,
            children: self.current.map_or(0, |c| self.nodes[c].children.len()),
        }
    }

    /// Drops every node recorded since `mark` and restores the cursor.
    pub fn rollback(&mut self, mark: &HistoryMark) {
        self.nodes.truncate(mark.len);
        self.current = mark.current;
        if let Some(c) = mark.current
            && let Some(n) = self.nodes.get_mut(c)
        {
            n.children.truncate(mark.children);
        }
    }

    /// Moves the cursor to the parent and returns its id and model (owned, so
    /// the caller can hand it back into a model that owns this history).
    pub fn undo(&mut self) -> Option<(usize, T)> {
        let parent = self.nodes[self.current?].parent?;
        self.current = Some(parent);
        Some((parent, self.nodes[parent].model.clone()))
    }

    /// Moves the cursor to the most recent child and returns its id and model.
    pub fn redo(&mut self) -> Option<(usize, T)> {
        let id = *self.nodes[self.current?].children.last()?;
        self.current = Some(id);
        Some((id, self.nodes[id].model.clone()))
    }

    /// Moves the cursor to any node in the tree and returns its id and model.
    pub fn jump(&mut self, id: usize) -> Result<(usize, T), String> {
        let Some(node) = self.nodes.get(id) else {
            return Err(format!("no history node #{id} (0..{})", self.nodes.len()));
        };
        self.current = Some(id);
        Ok((id, node.model.clone()))
    }

    /// Drops old history: keeps the newest `keep` nodes, plus the cursor, the
    /// root and every branch point (a node with 2+ children), and relinks each
    /// survivor to its nearest surviving ancestor. Node ids are renumbered
    /// (oldest first). Returns how many nodes were removed.
    pub fn prune(&mut self, keep: usize) -> usize {
        let n = self.nodes.len();
        if n <= keep {
            return 0;
        }
        let mut kept = vec![false; n];
        for flag in kept.iter_mut().skip(n - keep) {
            *flag = true;
        }
        for (i, node) in self.nodes.iter().enumerate() {
            if node.children.len() > 1 || node.parent.is_none() {
                kept[i] = true;
            }
        }
        if let Some(c) = self.current {
            kept[c] = true;
        }
        // New index for each survivor.
        let mut remap = vec![usize::MAX; n];
        let mut next = 0;
        for i in 0..n {
            if kept[i] {
                remap[i] = next;
                next += 1;
            }
        }
        let nearest_kept = |mut i: usize, nodes: &[Snapshot<T>]| -> Option<usize> {
            while let Some(p) = nodes[i].parent {
                if kept[p] {
                    return Some(remap[p]);
                }
                i = p;
            }
            None
        };
        let parents: Vec<Option<usize>> = (0..n)
            .map(|i| {
                if kept[i] {
                    nearest_kept(i, &self.nodes)
                } else {
                    None
                }
            })
            .collect();
        let old = std::mem::take(&mut self.nodes);
        let mut nodes: Vec<Snapshot<T>> = Vec::with_capacity(next);
        for (i, mut node) in old.into_iter().enumerate() {
            if kept[i] {
                node.parent = parents[i];
                node.children.clear();
                nodes.push(node);
            }
        }
        for i in 0..nodes.len() {
            if let Some(p) = nodes[i].parent {
                nodes[p].children.push(i);
            }
        }
        self.current = self.current.map(|c| remap[c]);
        self.nodes = nodes;
        n - next
    }

    /// One line per node: id, parent, label, model version and the cursor.
    pub fn describe(&self) -> String {
        if self.nodes.is_empty() {
            return "history is empty".into();
        }
        let mut lines = Vec::new();
        for (id, node) in self.nodes.iter().enumerate() {
            let cursor = if self.current == Some(id) { " <-" } else { "" };
            let parent = node
                .parent
                .map(|p| format!("parent #{p}"))
                .unwrap_or_else(|| "root".into());
            let label = if node.label.is_empty() {
                "(no label)".to_string()
            } else {
                format!("`{}`", node.label)
            };
            let by = node
                .author
                .as_deref()
                .map(|a| format!(" by {a}"))
                .unwrap_or_default();
            lines.push(format!(
                "#{id} {parent}, {label}{by}, v{}{cursor}",
                node.model.version()
            ));
        }
        lines.join("\n")
    }
}
