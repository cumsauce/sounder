//! The MCP server: tool args, the tool router and the session bridge.

mod args;
mod bridge;
mod tools;

pub(crate) use bridge::OnChange;
pub(crate) use bridge::{Cmd, Done, run_session};
pub(crate) use tools::Tools;
