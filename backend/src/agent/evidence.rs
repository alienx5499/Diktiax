//! Pure functions turning executed tool outcomes into scored evidence, trace summaries and
//! provenance steps. Only successful detector results for the TARGET claim are ever scored.

use super::dossier::AgentStep;
use super::planner::{ToolOutcome, DETECTORS};
use crate::db::queries;
use crate::domain::{
    CentralityReport, ClaimSummary, FraudPatternMatch, ProvenanceStep, SubgraphPayload,
};
use crate::mcp::{
    DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE, DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING,
    EXPLORE_CLAIM_SUBGRAPH, LIST_CLAIM_SHOPS, SHOP_CENTRALITY,
};
use serde_json::Value;

fn is_detector(tool: &str) -> bool {
    DETECTORS.contains(&tool)
}

/// Cypher each detector tool runs; empty for non-detector tools.
fn detector_cypher(tool: &str) -> &'static str {
    match tool {
        DETECT_GHOST_COLLISION => queries::GHOST_COLLISION,
        DETECT_SHARED_CONTACTS => queries::SHARED_CONTACTS,
        DETECT_VIN_CLONING => queries::VIN_CLONING,
        DETECT_KICKBACK_TRIANGLE => queries::KICKBACK_TRIANGLE,
        _ => "",
    }
}

/// Patterns that drive the risk score: detector matches on `claim_id`, deduplicated by name.
/// A detector result that does not deserialize is a contract violation and fails loudly.
pub fn scoring_patterns(
    claim_id: &str,
    outcomes: &[ToolOutcome],
) -> Result<Vec<FraudPatternMatch>, String> {
    let mut patterns: Vec<FraudPatternMatch> = Vec::new();
    for o in outcomes
        .iter()
        .filter(|o| is_detector(&o.call.tool) && o.call.claim_arg() == Some(claim_id))
    {
        let Ok(value) = &o.result else { continue };
        if value.is_null() {
            continue;
        }
        let m: FraudPatternMatch = serde_json::from_value(value.clone()).map_err(|e| {
            format!(
                "{} returned a malformed FraudPatternMatch: {e}",
                o.call.tool
            )
        })?;
        if !patterns.iter().any(|p| p.pattern_name == m.pattern_name) {
            patterns.push(m);
        }
    }
    Ok(patterns)
}

/// One-line, human-readable digest of a tool result for the agent trace.
pub fn summarize(o: &ToolOutcome) -> String {
    let value = match &o.result {
        Err(e) => return format!("ERROR: {e}"),
        Ok(v) => v,
    };
    let tool = o.call.tool.as_str();
    if is_detector(tool) {
        return match serde_json::from_value::<FraudPatternMatch>(value.clone()) {
            Ok(m) => format!(
                "MATCH {} ({}, +{}): {}",
                m.pattern_name, m.severity, m.risk_points, m.description
            ),
            Err(_) => "no match".into(),
        };
    }
    match tool {
        EXPLORE_CLAIM_SUBGRAPH => match serde_json::from_value::<SubgraphPayload>(value.clone()) {
            Ok(g) => format!("{} nodes, {} edges", g.nodes.len(), g.edges.len()),
            Err(_) => "unparseable subgraph".into(),
        },
        SHOP_CENTRALITY => match serde_json::from_value::<CentralityReport>(value.clone()) {
            Ok(r) => format!(
                "{} degree {} vs mean {:.2} (score {:.2}{})",
                r.shop_id,
                r.degree,
                r.baseline,
                r.score,
                if r.is_anomalous {
                    ", ANOMALOUS hub"
                } else {
                    ""
                }
            ),
            Err(_) => "unparseable centrality report".into(),
        },
        LIST_CLAIM_SHOPS => format!("shops: {}", value),
        _ => match serde_json::from_value::<ClaimSummary>(value.clone()) {
            Ok(c) => format!(
                "{} filed by {} for {:.2}, status {}",
                c.claim_id, c.claimant_name, c.amount, c.status
            ),
            Err(_) => truncate(&value.to_string(), 200),
        },
    }
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

/// What an LLM planner sees of a tool result: the one-line digest, plus `label:id` for every node
/// of an explored subgraph so it can pick shops and linked claims. Capped in size.
/// ponytail: raw JSON blew Groq's free-tier tokens/minute budget (every turn resends the history);
/// scoring and the trace still read the full result.
pub fn compact(o: &ToolOutcome) -> String {
    const MAX_CHARS: usize = 1500;
    if let (EXPLORE_CLAIM_SUBGRAPH, Ok(v)) = (o.call.tool.as_str(), &o.result) {
        if let Ok(g) = serde_json::from_value::<SubgraphPayload>(v.clone()) {
            let nodes: Vec<String> = g
                .nodes
                .iter()
                .map(|n| format!("{}:{}", n.label, n.id))
                .collect();
            let text = format!(
                "{} nodes, {} edges: {}",
                g.nodes.len(),
                g.edges.len(),
                nodes.join(", ")
            );
            return truncate(&text, MAX_CHARS);
        }
    }
    truncate(&summarize(o), MAX_CHARS)
}

/// Explainable decision path: one entry per executed call, index-aligned with
/// `provenance_steps` (the frontend pairs them by position).
pub fn agent_trace(outcomes: &[ToolOutcome]) -> Vec<AgentStep> {
    outcomes
        .iter()
        .enumerate()
        .map(|(i, o)| AgentStep {
            step: i + 1,
            tool: o.call.tool.clone(),
            arguments: o.call.arguments.clone(),
            rationale: o.call.rationale.clone(),
            result_summary: summarize(o),
            latency_ms: o.latency_ms,
        })
        .collect()
}

/// Provenance step per executed call. Only scored matches carry a non-zero risk delta, so
/// the deltas sum to the raw (pre-cap) risk score. Every detector step (match, no match or
/// error) carries the Cypher it ran; other tools carry an empty `cypher_source`.
pub fn provenance_steps(claim_id: &str, outcomes: &[ToolOutcome]) -> Vec<ProvenanceStep> {
    let mut credited: Vec<String> = Vec::new();
    outcomes
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let args = o.call.arguments.to_string();
            let matched = o
                .result
                .as_ref()
                .ok()
                .filter(|_| is_detector(&o.call.tool))
                .and_then(|v| serde_json::from_value::<FraudPatternMatch>(v.clone()).ok());
            let (expr, cypher, nodes, delta) = match &matched {
                Some(m) => {
                    // Every target match is scored (deduplicated by name) - credit its first occurrence.
                    let first_credit =
                        o.call.claim_arg() == Some(claim_id) && !credited.contains(&m.pattern_name);
                    if first_credit {
                        credited.push(m.pattern_name.clone());
                    }
                    let delta = if first_credit { m.risk_points } else { 0 };
                    (
                        m.evidence_path.join(" -> "),
                        m.cypher_query.clone(),
                        m.evidence_path.clone(),
                        delta,
                    )
                }
                None => (
                    format!("{}({args})", o.call.tool),
                    detector_cypher(&o.call.tool).to_string(),
                    shop_node(o),
                    0,
                ),
            };
            ProvenanceStep {
                step_number: i + 1,
                traversal_expression: expr,
                cypher_source: cypher,
                nodes_implicated: nodes,
                finding: summarize(o),
                risk_delta: delta,
            }
        })
        .collect()
}

fn shop_node(o: &ToolOutcome) -> Vec<String> {
    match (
        o.call.tool.as_str(),
        o.call.arguments.get("shop_id").and_then(Value::as_str),
    ) {
        (SHOP_CENTRALITY, Some(shop)) => vec![shop.to_string()],
        _ => Vec::new(),
    }
}

/// Deterministic rationale built only from scored patterns and the assessment.
pub fn rationale(
    score: i32,
    tier: &str,
    patterns: &[FraudPatternMatch],
    tool_calls: usize,
) -> String {
    if patterns.is_empty() {
        // An inconclusive run had failed calls, so "nothing matched" must not read as a clean result.
        let scope = if tier == crate::actions::INCONCLUSIVE_TIER {
            "in the calls that succeeded"
        } else {
            "after"
        };
        return format!(
            "Risk {score}/100 ({tier}): no fraud topology matched {scope} {tool_calls} graph tool call(s)."
        );
    }
    let findings: Vec<String> = patterns
        .iter()
        .map(|p| format!("{} (+{}): {}", p.pattern_name, p.risk_points, p.description))
        .collect();
    format!(
        "Risk {score}/100 ({tier}) from {} graph pattern(s). {}",
        patterns.len(),
        findings.join(" ")
    )
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
