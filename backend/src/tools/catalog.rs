//! Canonical catalog of FalkorDB graph tools, shared by the in-process agent loop and the
//! MCP server. Descriptions are written for an LLM choosing which graph query to run next.

use crate::db::queries::{MAX_DEPTH, MIN_DEPTH};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub const GET_CLAIM: &str = "get_claim";
pub const EXPLORE_CLAIM_SUBGRAPH: &str = "explore_claim_subgraph";
pub const DETECT_GHOST_COLLISION: &str = "detect_ghost_collision";
pub const DETECT_SHARED_CONTACTS: &str = "detect_shared_contacts";
pub const DETECT_VIN_CLONING: &str = "detect_vin_cloning";
pub const DETECT_KICKBACK_TRIANGLE: &str = "detect_kickback_triangle";
pub const LIST_CLAIM_SHOPS: &str = "list_claim_shops";
pub const SHOP_CENTRALITY: &str = "shop_centrality";
/// MCP-only: runs the full autonomous investigation (not part of the agent's own catalog).
pub const INVESTIGATE_CLAIM: &str = "investigate_claim";
/// External-only (MCP + /api/mcp): never offered to the agent's own planners.
pub const INVESTIGATION_HISTORY: &str = "investigation_history";

pub const DEFAULT_DEPTH: u8 = 2;

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        }),
    }
}

fn claim_id_only(name: &str, description: &str) -> ToolDefinition {
    tool(
        name,
        description,
        json!({ "claim_id": claim_id_property() }),
        &["claim_id"],
    )
}

fn claim_id_property() -> Value {
    json!({ "type": "string", "minLength": 1, "description": "Claim identifier, e.g. 'CLM-48291'." })
}

const DETECTOR_RETURNS: &str =
    "Returns a FraudPatternMatch {pattern_name, severity (HIGH|CRITICAL), \
     risk_points, description, cypher_query, evidence_path (ordered node ids forming the proof)} \
     or null when the graph shows no such pattern for this claim.";

/// The FalkorDB graph tools, in the order an investigation typically uses them.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        claim_id_only(
            GET_CLAIM,
            "Fetch the core record of an insurance claim from the graph. Use first to confirm the \
             claim exists before deeper analysis. Returns {claim_id, claimant_name, amount, status, \
             filed_at}; errors if the claim id is unknown.",
        ),
        tool(
            EXPLORE_CLAIM_SUBGRAPH,
            "Traverse the graph neighbourhood around a claim (Customer, Phone, Address, Vehicle, VIN, \
             Accident, Location, RepairShop, Clinic, Adjuster and linked claims). Use to see who and \
             what a claim is connected to when you need raw context beyond the targeted detectors. \
             Returns {nodes:[{id,label,title,group,properties}], edges:[{id,from,to,label,properties}]}. \
             Larger depth reaches further but returns more data.",
            json!({
                "claim_id": claim_id_property(),
                "depth": {
                    "type": "integer",
                    "minimum": MIN_DEPTH,
                    "maximum": MAX_DEPTH,
                    "default": DEFAULT_DEPTH,
                    "description": "Number of hops to traverse from the claim (1-4, default 2)."
                }
            }),
            &["claim_id"],
        ),
        claim_id_only(
            DETECT_GHOST_COLLISION,
            &format!(
                "Detect a staged 'ghost collision' ring: other claims reported at the same accident \
                 location AND repaired at the same shop within a {}-day window. Use when a claim's \
                 location or repair shop looks reused. {DETECTOR_RETURNS}",
                crate::db::GHOST_WINDOW_DAYS
            ),
        ),
        claim_id_only(
            DETECT_SHARED_CONTACTS,
            &format!(
                "Detect a shared-contact network: the claimant shares a phone number or street address \
                 with other claimants who filed their own claims (synthetic identities / organised \
                 ring). Use when identity overlap is suspected. {DETECTOR_RETURNS}"
            ),
        ),
        claim_id_only(
            DETECT_VIN_CLONING,
            &format!(
                "Detect VIN cloning: the claim's vehicle VIN is registered on other vehicle records that \
                 also have claims, possibly across jurisdictions (cloned or salvage-washed vehicles). \
                 Use when the vehicle identity is in doubt. {DETECTOR_RETURNS}"
            ),
        ),
        claim_id_only(
            DETECT_KICKBACK_TRIANGLE,
            &format!(
                "Detect an adjuster-shop-clinic kickback triangle: a claim without a police report whose \
                 adjuster, repair shop and medical clinic recur together on other claims, optionally with \
                 a shop-to-clinic referral (COLLUDES_WITH) link. Use when service providers look \
                 coordinated. {DETECTOR_RETURNS}"
            ),
        ),
        claim_id_only(
            LIST_CLAIM_SHOPS,
            "List the repair shop ids linked to a claim. Use to obtain shop_id values for \
             shop_centrality. Returns a JSON array of shop id strings (may be empty).",
        ),
        tool(
            SHOP_CENTRALITY,
            "Measure how concentrated claims are at one repair shop: its claim degree versus the mean \
             degree of all shops. Use after list_claim_shops to check whether a shop is an anomalous \
             fraud hub. Returns {shop_id, shop_name, degree, baseline, shop_count, score, \
             is_anomalous (score >= 1.5)}; errors if the shop is unknown.",
            json!({
                "shop_id": { "type": "string", "minLength": 1, "description": "Repair shop identifier, e.g. 'SHOP-801'." }
            }),
            &["shop_id"],
        ),
    ]
}

/// Tools for external agents (MCP, /api/mcp): the planner catalog plus external-only memory
/// tools. The agent's planners use `tool_definitions()` and never see these extras.
pub fn external_tool_definitions() -> Vec<ToolDefinition> {
    let mut tools = tool_definitions();
    tools.push(claim_id_only(
        INVESTIGATION_HISTORY,
        "Past verdicts the Diktiax agent recorded for this claim, newest first: certificate id, \
         decision, risk score, time. Use it to continue from earlier work instead of \
         re-investigating.",
    ));
    tools
}

/// Definition of the MCP-only end-to-end investigation tool.
pub fn investigate_claim_definition() -> ToolDefinition {
    claim_id_only(
        INVESTIGATE_CLAIM,
        "Run Diktiax's full autonomous investigation on a claim: runs the graph detectors, scores \
         risk, issues a provenance certificate and EXECUTES remediation (high-risk claims get their \
         payout frozen, status changed in the graph and an SIU investigator assigned). Use when you \
         want a complete, acted-upon verdict rather than read-only graph queries. Returns the \
         investigation dossier.",
    )
}
