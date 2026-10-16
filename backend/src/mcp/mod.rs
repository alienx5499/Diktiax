//! Model Context Protocol adapter: JSON-RPC over stdio, exposing the shared graph tools
//! (`crate::tools`) plus `investigate_claim`.

mod protocol;
mod stdio;

pub use stdio::serve_stdio;
