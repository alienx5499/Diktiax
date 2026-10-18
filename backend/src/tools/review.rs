//! `review_claim`, on the stdio MCP server only (`--mcp-server`, run by a local operator): another
//! agent, or a person through an MCP client, acts as the assigned investigator and decides a
//! frozen payout. Never offered to the agent's planners or over the public HTTP tool endpoint.

use super::catalog::{claim_id_property, tool, ToolDefinition};
use super::execute::required_str;
use crate::db::FalkorGraphRepository;
use crate::domain::{ReviewDecision, ReviewRequest};
use serde_json::{json, Value};

pub const REVIEW_CLAIM: &str = "review_claim";

pub fn review_claim_definition() -> ToolDefinition {
    tool(
        REVIEW_CLAIM,
        "Human review of a frozen payout: as the investigator assigned to the claim, confirm the \
         fraud (status CONFIRMED_FRAUD, payout stays frozen) or release the payout (status \
         RELEASED_AFTER_REVIEW). One review per claim; errors if the claim is not frozen, was \
         already reviewed, or the reviewer is not its assigned investigator. Returns {review_id, \
         claim_id, decision, reviewer_id, reviewer_name, note, reviewed_at, investigation_id, \
         claim_status, payout_frozen}.",
        json!({
            "claim_id": claim_id_property(),
            "decision": { "type": "string", "enum": ["confirm_fraud", "release"] },
            "reviewer": {
                "type": "string",
                "minLength": 1,
                "description": "Investigator id of the reviewer, e.g. 'INV-202'."
            },
            "note": { "type": "string", "description": "Optional reason for the decision." }
        }),
        &["claim_id", "decision", "reviewer"],
    )
}

/// `{claim_id, decision, reviewer, note?}` -> claim id and a normalized request.
fn parse_review_args(args: &Value) -> Result<(String, ReviewRequest), String> {
    let claim_id = required_str(args, "claim_id")?;
    let decision = ReviewDecision::parse(&required_str(args, "decision")?)?;
    let reviewer = required_str(args, "reviewer")?;
    let note = match args.get("note") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err("argument 'note' must be a string".into()),
    };
    let request = ReviewRequest {
        decision,
        reviewer,
        note,
    };
    Ok((claim_id, request.normalized()?))
}

pub async fn review_claim(repo: &FalkorGraphRepository, args: &Value) -> Result<Value, String> {
    let (claim_id, request) = parse_review_args(args)?;
    let record = repo
        .review_claim(&claim_id, &request)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(record).map_err(|e| format!("failed to serialize review: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_args_are_validated() {
        let ok = json!({ "claim_id": "C", "decision": "release", "reviewer": "INV-1" });
        let (id, req) = parse_review_args(&ok).unwrap();
        assert_eq!((id.as_str(), req.decision), ("C", ReviewDecision::Release));
        let bad = json!({ "claim_id": "C", "decision": "approve", "reviewer": "INV-1" });
        assert!(parse_review_args(&bad)
            .unwrap_err()
            .contains("argument 'decision'"));
        let no_reviewer = json!({ "claim_id": "C", "decision": "release" });
        assert!(parse_review_args(&no_reviewer).is_err());
    }
}
