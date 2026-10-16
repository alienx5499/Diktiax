//! The canonical graph-tool catalog and its validated execution against FalkorDB.
//! Shared by the agent loop, the HTTP API and the MCP server.

mod catalog;
mod execute;

pub(crate) use catalog::{investigate_claim_definition, INVESTIGATE_CLAIM};
pub use catalog::{
    tool_definitions, ToolDefinition, DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE,
    DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING, EXPLORE_CLAIM_SUBGRAPH, LIST_CLAIM_SHOPS,
    SHOP_CENTRALITY,
};
pub use execute::execute_tool;
pub(crate) use execute::required_str;
