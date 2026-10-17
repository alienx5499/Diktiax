//! Validated execution of the graph tool catalog against the real FalkorDB repository.
//! Arguments are parsed into a `ToolCall` before any query runs, so validation is DB-free.

use super::catalog::{self, DEFAULT_DEPTH};
use crate::db::queries::{MAX_DEPTH, MIN_DEPTH};
use crate::db::FalkorGraphRepository;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCall {
    GetClaim { claim_id: String },
    ExploreSubgraph { claim_id: String, depth: u8 },
    GhostCollision { claim_id: String },
    SharedContacts { claim_id: String },
    VinCloning { claim_id: String },
    KickbackTriangle { claim_id: String },
    ClaimShops { claim_id: String },
    ShopCentrality { shop_id: String },
}

/// Reads a required, non-empty string argument (trimmed).
pub(crate) fn required_str(args: &Value, key: &str) -> Result<String, String> {
    let args = args.as_object().ok_or("arguments must be a JSON object")?;
    match args.get(key) {
        None | Some(Value::Null) => Err(format!("missing required argument '{key}'")),
        Some(Value::String(s)) if s.trim().is_empty() => {
            Err(format!("argument '{key}' must not be empty"))
        }
        Some(Value::String(s)) => Ok(s.trim().to_string()),
        Some(_) => Err(format!("argument '{key}' must be a string")),
    }
}

fn depth(args: &Value) -> Result<u8, String> {
    match args.get("depth") {
        None | Some(Value::Null) => Ok(DEFAULT_DEPTH),
        Some(v) => v
            .as_u64()
            .and_then(|d| u8::try_from(d).ok())
            .filter(|d| (MIN_DEPTH..=MAX_DEPTH).contains(d))
            .ok_or_else(|| {
                format!("argument 'depth' must be an integer between {MIN_DEPTH} and {MAX_DEPTH}")
            }),
    }
}

/// Pure argument validation: maps a tool name + arguments to a typed call.
pub fn parse_call(name: &str, args: &Value) -> Result<ToolCall, String> {
    let claim_id = || required_str(args, "claim_id");
    Ok(match name {
        catalog::GET_CLAIM => ToolCall::GetClaim {
            claim_id: claim_id()?,
        },
        catalog::EXPLORE_CLAIM_SUBGRAPH => ToolCall::ExploreSubgraph {
            claim_id: claim_id()?,
            depth: depth(args)?,
        },
        catalog::DETECT_GHOST_COLLISION => ToolCall::GhostCollision {
            claim_id: claim_id()?,
        },
        catalog::DETECT_SHARED_CONTACTS => ToolCall::SharedContacts {
            claim_id: claim_id()?,
        },
        catalog::DETECT_VIN_CLONING => ToolCall::VinCloning {
            claim_id: claim_id()?,
        },
        catalog::DETECT_KICKBACK_TRIANGLE => ToolCall::KickbackTriangle {
            claim_id: claim_id()?,
        },
        catalog::LIST_CLAIM_SHOPS => ToolCall::ClaimShops {
            claim_id: claim_id()?,
        },
        catalog::SHOP_CENTRALITY => ToolCall::ShopCentrality {
            shop_id: required_str(args, "shop_id")?,
        },
        unknown => return Err(format!("unknown tool '{unknown}'")),
    })
}

fn to_json<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| format!("failed to serialize tool result: {e}"))
}

/// Runs an already-validated call against FalkorDB.
pub async fn run(repo: &FalkorGraphRepository, call: ToolCall) -> Result<Value, String> {
    match call {
        ToolCall::GetClaim { claim_id } => match repo.get_claim(&claim_id).await? {
            Some(claim) => to_json(claim),
            None => Err(format!("claim {claim_id} not found")),
        },
        ToolCall::ExploreSubgraph { claim_id, depth } => {
            to_json(repo.claim_subgraph(&claim_id, depth).await?)
        }
        ToolCall::GhostCollision { claim_id } => {
            to_json(repo.detect_ghost_collision(&claim_id).await?)
        }
        ToolCall::SharedContacts { claim_id } => {
            to_json(repo.detect_shared_contacts(&claim_id).await?)
        }
        ToolCall::VinCloning { claim_id } => to_json(repo.detect_vin_cloning(&claim_id).await?),
        ToolCall::KickbackTriangle { claim_id } => {
            to_json(repo.detect_kickback_triangle(&claim_id).await?)
        }
        ToolCall::ClaimShops { claim_id } => to_json(repo.claim_shops(&claim_id).await?),
        ToolCall::ShopCentrality { shop_id } => to_json(repo.shop_centrality(&shop_id).await?),
    }
}

/// Validates then executes a catalog tool. Detectors yield JSON null when no pattern matches.
pub async fn execute_tool(
    repo: &FalkorGraphRepository,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    run(repo, parse_call(name, args)?).await
}

/// Entry point for external callers (MCP, /api/mcp/call): `execute_tool` plus the external-only
/// tools, which the agent loop (it calls `execute_tool`) can therefore never run.
pub async fn execute_external_tool(
    repo: &FalkorGraphRepository,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if name == catalog::INVESTIGATION_HISTORY {
        let claim_id = required_str(args, "claim_id")?;
        return to_json(repo.investigation_history(&claim_id).await?);
    }
    if name == catalog::RING_COMPONENTS {
        let claim_id = required_str(args, "claim_id")?;
        return to_json(repo.ring_component(&claim_id).await?);
    }
    execute_tool(repo, name, args).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_catalog_tool_parses_with_valid_args() {
        for def in catalog::tool_definitions() {
            let args = if def.name == catalog::SHOP_CENTRALITY {
                json!({ "shop_id": "SHOP-1" })
            } else {
                json!({ "claim_id": "CLM-1" })
            };
            assert!(
                parse_call(&def.name, &args).is_ok(),
                "{} failed to parse",
                def.name
            );
        }
    }

    #[test]
    fn agent_loop_cannot_parse_external_only_tools() {
        for name in [catalog::INVESTIGATION_HISTORY, catalog::RING_COMPONENTS] {
            assert!(
                parse_call(name, &json!({ "claim_id": "C" })).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn unknown_tool_is_rejected() {
        assert_eq!(
            parse_call("nope", &json!({})),
            Err("unknown tool 'nope'".into())
        );
    }

    #[test]
    fn missing_empty_or_non_string_claim_id_is_rejected() {
        assert!(parse_call("get_claim", &json!({}))
            .unwrap_err()
            .contains("missing required argument 'claim_id'"));
        assert!(parse_call("get_claim", &json!({ "claim_id": "  " }))
            .unwrap_err()
            .contains("must not be empty"));
        assert!(parse_call("get_claim", &json!({ "claim_id": 7 }))
            .unwrap_err()
            .contains("must be a string"));
        assert!(parse_call("get_claim", &json!("CLM-1"))
            .unwrap_err()
            .contains("JSON object"));
        assert!(
            parse_call("shop_centrality", &json!({ "claim_id": "CLM-1" }))
                .unwrap_err()
                .contains("'shop_id'")
        );
    }

    #[test]
    fn depth_defaults_and_bounds() {
        let call = |d: Value| {
            parse_call(
                "explore_claim_subgraph",
                &json!({ "claim_id": "C", "depth": d }),
            )
        };
        assert_eq!(
            parse_call("explore_claim_subgraph", &json!({ "claim_id": "C" })),
            Ok(ToolCall::ExploreSubgraph {
                claim_id: "C".into(),
                depth: 2
            })
        );
        assert_eq!(
            call(json!(4)),
            Ok(ToolCall::ExploreSubgraph {
                claim_id: "C".into(),
                depth: 4
            })
        );
        for bad in [json!(9), json!(0), json!(-1), json!(2.5), json!("2")] {
            assert!(call(bad.clone()).is_err(), "depth {bad} should be rejected");
        }
    }
}
