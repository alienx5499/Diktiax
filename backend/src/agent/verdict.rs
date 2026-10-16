//! Fail-inconclusive guard: a graph error on the TARGET claim (exploration or any detector)
//! means the evidence is incomplete, so a low score must never become an automated clearance.

use super::planner::{ToolOutcome, DETECTORS};
use crate::actions::INCONCLUSIVE_TIER;
use crate::domain::evaluator::FraudRiskAssessment;
use crate::domain::SubgraphPayload;
use crate::tools::EXPLORE_CLAIM_SUBGRAPH;

/// Exploration / detector tools whose failure on the target claim leaves a gap in the evidence.
fn is_required(tool: &str) -> bool {
    tool == EXPLORE_CLAIM_SUBGRAPH || DETECTORS.contains(&tool)
}

/// Usable result: `Ok`, and for exploration also a parseable subgraph (the planner gates on it).
fn usable(o: &ToolOutcome) -> bool {
    match &o.result {
        Err(_) => false,
        Ok(v) if o.call.tool == EXPLORE_CLAIM_SUBGRAPH => {
            serde_json::from_value::<SubgraphPayload>(v.clone()).is_ok()
        }
        Ok(_) => true,
    }
}

/// "tool: error" for every required tool on `claim_id` that never produced a usable result.
/// A failure later recovered by a successful rerun of the same tool (coverage guard) is not a gap.
/// Calls on linked claims and `shop_centrality` are not scored, so they never appear here.
pub fn graph_failures(claim_id: &str, outcomes: &[ToolOutcome]) -> Vec<String> {
    let on_target = |o: &&ToolOutcome| o.call.claim_arg() == Some(claim_id);
    let mut failures: Vec<String> = Vec::new();
    let mut failed_tools: Vec<&str> = Vec::new();
    for o in outcomes
        .iter()
        .filter(on_target)
        .filter(|o| is_required(&o.call.tool) && !usable(o))
    {
        let recovered = outcomes
            .iter()
            .filter(on_target)
            .any(|s| s.call.tool == o.call.tool && usable(s));
        // One entry per tool: a coverage-guard rerun that fails again is the same gap.
        if recovered || failed_tools.contains(&o.call.tool.as_str()) {
            continue;
        }
        failed_tools.push(&o.call.tool);
        let error = match &o.result {
            Err(e) => e.as_str(),
            Ok(_) => "unparseable subgraph",
        };
        failures.push(format!("{}: {error}", o.call.tool));
    }
    failures
}

/// With graph failures the score is only a LOWER bound (missing detectors can only add points).
/// A bound that already escalates (HIGH/CRITICAL) stands; anything lower becomes INCONCLUSIVE
/// with zero confidence, which the dispatcher maps to `NEEDS_MANUAL_REVIEW` (no clear, no freeze).
pub fn apply_graph_failures(assessment: &mut FraudRiskAssessment, failures: &[String]) {
    if failures.is_empty() || crate::actions::status_for(&assessment.risk_tier).1 {
        return;
    }
    assessment.risk_tier = INCONCLUSIVE_TIER.to_string();
    assessment.confidence = 0.0;
}

/// Rationale suffix naming each failure; empty when the evidence is complete.
pub fn failure_note(score: i32, failures: &[String]) -> String {
    if failures.is_empty() {
        return String::new();
    }
    format!(
        "\n\nIncomplete graph evidence ({} failed call(s) on the target claim): {}. \
         Score {score} is a lower bound; the claim was not cleared.",
        failures.len(),
        failures.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::planner::PlannedCall;
    use crate::domain::FraudRiskEvaluator;
    use crate::tools::{DETECT_KICKBACK_TRIANGLE, DETECT_VIN_CLONING, SHOP_CENTRALITY};
    use serde_json::{json, Value};

    fn done(tool: &str, args: Value, result: Result<Value, String>) -> ToolOutcome {
        ToolOutcome {
            call: PlannedCall::new(tool, args, String::new()),
            result,
            latency_ms: 1.0,
        }
    }

    fn c(id: &str) -> Value {
        json!({ "claim_id": id })
    }

    fn graph() -> Value {
        json!({ "nodes": [], "edges": [] })
    }

    #[test]
    fn failed_exploration_is_inconclusive_not_cleared() {
        let outcomes = vec![done(EXPLORE_CLAIM_SUBGRAPH, c("C1"), Err("boom".into()))];
        let failures = graph_failures("C1", &outcomes);
        assert_eq!(failures, vec!["explore_claim_subgraph: boom"]);
        let mut a = FraudRiskEvaluator::evaluate(&[]);
        apply_graph_failures(&mut a, &failures);
        assert_eq!(a.risk_tier, INCONCLUSIVE_TIER);
        assert_eq!(a.confidence, 0.0);
        assert_eq!(
            crate::actions::status_for(&a.risk_tier),
            ("NEEDS_MANUAL_REVIEW", false)
        );
    }

    #[test]
    fn unparseable_exploration_counts_as_failure() {
        let outcomes = vec![done(EXPLORE_CLAIM_SUBGRAPH, c("C1"), Ok(json!(42)))];
        assert_eq!(graph_failures("C1", &outcomes).len(), 1);
    }

    #[test]
    fn target_detector_error_is_a_gap_unless_rerun_succeeds() {
        let mut outcomes = vec![
            done(EXPLORE_CLAIM_SUBGRAPH, c("C1"), Ok(graph())),
            done(DETECT_VIN_CLONING, c("C1"), Err("db down".into())),
        ];
        assert_eq!(
            graph_failures("C1", &outcomes),
            vec!["detect_vin_cloning: db down"]
        );
        outcomes.push(done(DETECT_VIN_CLONING, c("C1"), Ok(Value::Null)));
        assert!(graph_failures("C1", &outcomes).is_empty());
    }

    #[test]
    fn repeated_failure_of_one_tool_is_one_gap() {
        let outcomes = vec![
            done(EXPLORE_CLAIM_SUBGRAPH, c("C1"), Ok(graph())),
            done(DETECT_VIN_CLONING, c("C1"), Err("db down".into())),
            done(DETECT_VIN_CLONING, c("C1"), Err("db down".into())),
        ];
        assert_eq!(
            graph_failures("C1", &outcomes),
            vec!["detect_vin_cloning: db down"]
        );
    }

    #[test]
    fn linked_claim_and_centrality_errors_do_not_block_verdict() {
        let outcomes = vec![
            done(EXPLORE_CLAIM_SUBGRAPH, c("C1"), Ok(graph())),
            done(DETECT_KICKBACK_TRIANGLE, c("C2"), Err("x".into())),
            done(SHOP_CENTRALITY, json!({ "shop_id": "S1" }), Err("y".into())),
        ];
        let failures = graph_failures("C1", &outcomes);
        assert!(failures.is_empty());
        let mut a = FraudRiskEvaluator::evaluate(&[]);
        apply_graph_failures(&mut a, &failures);
        assert_eq!(a.risk_tier, "LOW_RISK");
    }

    #[test]
    fn escalating_lower_bound_stands_despite_failures() {
        let mut a = FraudRiskEvaluator::evaluate(&[]);
        a.risk_tier = "CRITICAL_FRAUD".into();
        apply_graph_failures(&mut a, &["detect_vin_cloning: x".into()]);
        assert_eq!(a.risk_tier, "CRITICAL_FRAUD");
    }
}
