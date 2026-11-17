//! sounder: an agent-driven procedural audio tool (sibling of modeler).
//!
//! - [`sfx_graph`]: the synthesis core (phase 1).
//! - [`sfx_analysis`]: the senses (phase 1).
//! - This crate: history, the session (open sounds, per-agent active sound,
//!   comments, knob moves, autosave), the catalog, export and the friction
//!   log. The MCP server is the `sounder` binary (`src/bin/sounder`).

pub mod catalog;
pub mod export;
pub mod feedback;
pub mod history;
pub mod session;
pub mod store;

pub use sfx_analysis;
pub use sfx_graph;
