use super::*;
use crate::agent::planner::PlannedCall;
use serde_json::json;

fn done(tool: &str, claim: &str, result: Result<Value, String>) -> ToolOutcome {
    ToolOutcome {
        call: PlannedCall::new(tool, json!({ "claim_id": claim }), String::new()),
        result,
        latency_ms: 2.0,
    }
}

fn pattern(name: &str, points: i32) -> Value {
    json!({ "pattern_name": name, "severity": "HIGH", "risk_points": points, "description": "d",
            "cypher_query": "MATCH (c)", "evidence_path": ["C1", "S1"] })
}

#[test]
fn only_target_detector_matches_are_scored_once() {
    let outcomes = vec![
        done("detect_ghost_collision", "C1", Ok(pattern("Ghost", 45))),
        done("detect_ghost_collision", "C1", Ok(pattern("Ghost", 45))),
        done("detect_ghost_collision", "C2", Ok(pattern("Ghost", 45))),
        done("detect_vin_cloning", "C1", Ok(Value::Null)),
        done("detect_kickback_triangle", "C1", Err("db".into())),
        done(
            "explore_claim_subgraph",
            "C1",
            Ok(pattern("NotADetector", 99)),
        ),
    ];
    let scored = scoring_patterns("C1", &outcomes).expect("valid");
    assert_eq!(scored.len(), 1);
    let steps = provenance_steps("C1", &outcomes);
    assert_eq!(
        steps.iter().map(|s| s.risk_delta).sum::<i32>(),
        45,
        "deltas sum to the raw score"
    );
    assert_eq!(steps[0].cypher_source, "MATCH (c)");
    assert!(steps[4].finding.starts_with("ERROR"));
}

#[test]
fn malformed_detector_result_fails_loudly() {
    let outcomes = vec![done(
        "detect_vin_cloning",
        "C1",
        Ok(json!({ "bogus": true })),
    )];
    assert!(scoring_patterns("C1", &outcomes).is_err());
}

#[test]
fn summaries_are_content_based() {
    assert_eq!(
        summarize(&done("detect_vin_cloning", "C1", Ok(Value::Null))),
        "no match"
    );
    assert!(
        summarize(&done("detect_vin_cloning", "C1", Ok(pattern("Vin", 50))))
            .starts_with("MATCH Vin")
    );
    let graph = json!({ "nodes": [], "edges": [] });
    assert_eq!(
        summarize(&done("explore_claim_subgraph", "C1", Ok(graph))),
        "0 nodes, 0 edges"
    );
}

#[test]
fn every_detector_step_carries_cypher_and_aligns_with_trace() {
    let outcomes = vec![
        done(
            "explore_claim_subgraph",
            "C1",
            Ok(json!({ "nodes": [], "edges": [] })),
        ),
        done("detect_ghost_collision", "C1", Ok(pattern("Ghost", 45))),
        done("detect_shared_contacts", "C1", Ok(Value::Null)),
        done("detect_vin_cloning", "C1", Err("db".into())),
        done("detect_kickback_triangle", "C2", Ok(Value::Null)),
        done("shop_centrality", "C1", Ok(json!({}))),
    ];
    let steps = provenance_steps("C1", &outcomes);
    let trace = agent_trace(&outcomes);
    assert_eq!(steps.len(), outcomes.len());
    assert_eq!(trace.len(), steps.len());
    for (s, t) in steps.iter().zip(&trace) {
        assert_eq!(s.step_number, t.step);
        assert_eq!(s.finding, t.result_summary);
    }
    assert_eq!(
        steps[0].cypher_source, "",
        "non-detector keeps empty cypher"
    );
    assert_eq!(
        steps[1].cypher_source, "MATCH (c)",
        "match keeps its own cypher"
    );
    assert_eq!(steps[2].cypher_source, queries::SHARED_CONTACTS);
    assert_eq!(steps[3].cypher_source, queries::VIN_CLONING);
    assert_eq!(steps[4].cypher_source, queries::KICKBACK_TRIANGLE);
    assert_eq!(steps[5].cypher_source, "");
}

#[test]
fn relied_on_candidates_skip_the_target_and_repeats() {
    let m = |path: &[&str]| FraudPatternMatch {
        pattern_name: "p".into(),
        severity: "HIGH".into(),
        risk_points: 1,
        description: "d".into(),
        cypher_query: "MATCH (c)".into(),
        evidence_path: path.iter().map(|s| s.to_string()).collect(),
    };
    let patterns = [m(&["C1", "S1", "C2"]), m(&["C1", "C2", "C3"])];
    assert_eq!(
        relied_on_candidates("C1", &patterns),
        vec!["S1".to_string(), "C2".into(), "C3".into()]
    );
    assert!(relied_on_candidates("C1", &[]).is_empty());
}
