//! Model Context Protocol adapter: JSON-RPC over stdio, exposing the shared graph tools
//! (`crate::tools`) plus `investigate_claim`.

mod investigate;
mod protocol;
mod stdio;

pub use investigate::InvestigateClaim;
pub use stdio::serve_stdio;
