//! Model Context Protocol integration: the canonical graph-tool catalog (shared with the
//! agent loop), its validated execution against FalkorDB, and a stdio MCP server.

mod catalog;
mod protocol;
mod stdio;
mod tools;

pub use catalog::{
    tool_definitions, ToolDefinition, DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE,
    DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING, EXPLORE_CLAIM_SUBGRAPH, LIST_CLAIM_SHOPS,
    SHOP_CENTRALITY,
};
pub use stdio::serve_stdio;
pub use tools::execute_tool;
