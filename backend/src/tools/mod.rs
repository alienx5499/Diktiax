//! The canonical graph-tool catalog and its validated execution against FalkorDB.
//! Shared by the agent loop, the HTTP API and the MCP server.

mod catalog;
mod execute;

pub use catalog::{
    external_tool_definitions, is_graph_scan, tool_definitions, ToolDefinition, CONNECTION_PATH,
    DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE, DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING,
    EXPLORE_CLAIM_SUBGRAPH, INVESTIGATION_HISTORY, LIST_CLAIM_SHOPS, RING_COMMUNITIES,
    RING_COMPONENTS, SHOP_CENTRALITY, SHOP_INFLUENCE,
};
pub(crate) use catalog::{investigate_claim_definition, INVESTIGATE_CLAIM};
pub(crate) use execute::required_str;
pub use execute::{execute_external_tool, execute_tool};
