//! sounder: an agent-driven procedural audio tool (sibling of modeler).
//!
//! Phase 1 is the core only: [`sfx_graph`] (synthesis) and [`sfx_analysis`]
//! (the senses). The MCP server, session, history and catalog arrive in
//! phase 2 and live in this crate.

pub use sfx_analysis;
pub use sfx_graph;
