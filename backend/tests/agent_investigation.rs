//! Live agent loop + actions test (rule-based planner). Requires a running FalkorDB:
//!   docker compose up -d falkordb && cargo test -- --ignored

mod common;

use claimgraph_backend::actions::{ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, PlannerConfig};
use claimgraph_backend::db::FalkorGraphRepository;
use common::TestGraph;

async fn setup() -> (FalkorGraphRepository, ClaimInvestigator, TestGraph) {
    let graph = TestGraph::new("claimgraph_agent_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB running");
    repo.seed(true).await.expect("seed");
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::new(None).expect("client"));
    (
        repo.clone(),
        ClaimInvestigator::new(repo, dispatcher, PlannerConfig::RuleBased),
        graph,
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn rule_based_investigation_end_to_end() {
    let (repo, investigator, _graph) = setup().await;

    let fraud = investigator
        .investigate("CLM-48291")
        .await
        .expect("fraud dossier");
    assert_eq!(fraud.planner, "rule-based");
    assert_eq!(fraud.risk_tier, "CRITICAL_FRAUD");
    let names: Vec<&str> = fraud
        .patterns_matched
        .iter()
        .map(|p| p.pattern_name.as_str())
        .collect();
    assert!(
        names.contains(&"Ghost Collision Ring")
            && names.contains(&"Adjuster-Shop-Clinic Kickback Triangle"),
        "{names:?}"
    );
    assert!(
        fraud
            .agent_trace
            .iter()
            .any(|s| s.tool == "shop_centrality"),
        "evidence shop gets a centrality check"
    );
    assert_eq!(
        fraud.certificate.topological_evidence.len(),
        fraud.agent_trace.len()
    );
    assert_eq!(fraud.certificate.cryptographic_digest.len(), 64);
    assert!(fraud.remediation.payout_frozen);
    assert_ne!(fraud.remediation.assigned_investigator, "unassigned");
    assert_eq!(
        fraud.remediation.webhook_target,
        "skipped: WEBHOOK_URL not set"
    );
    let stored = repo.get_claim("CLM-48291").await.unwrap().expect("claim");
    assert_eq!(stored.status, "FLAGGED_SIU_FRAUD");
    assert_eq!(fraud.status, stored.status);

    // Assignment is idempotent: re-investigating does not inflate the investigator's load.
    let names_owned: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    let before = repo
        .pick_siu_investigator(&names_owned)
        .await
        .unwrap()
        .expect("pool");
    investigator.investigate("CLM-48291").await.expect("again");
    let after = repo
        .pick_siu_investigator(&names_owned)
        .await
        .unwrap()
        .expect("pool");
    assert_eq!(before, after);

    let clean = investigator
        .investigate("CLM-10022")
        .await
        .expect("clean dossier");
    assert!(clean.patterns_matched.is_empty());
    assert_eq!(clean.risk_score, 0);
    assert_eq!(clean.status, "CLEARED_AUTOMATED");
    assert!(!clean.remediation.payout_frozen);
    assert!(
        clean.agent_trace.len() < fraud.agent_trace.len(),
        "clean claims need fewer graph hops"
    );
    assert!(
        clean
            .agent_trace
            .iter()
            .all(|s| s.tool != "detect_kickback_triangle"),
        "no clinic -> no kickback test"
    );

    let vin = investigator
        .investigate("CLM-55104")
        .await
        .expect("vin dossier");
    assert!(vin
        .patterns_matched
        .iter()
        .any(|p| p.pattern_name == "Cloned VIN Across Jurisdictions"));
    assert!(vin.agent_trace.iter().any(|s| s.tool == "list_claim_shops"));

    let missing = investigator
        .investigate("CLM-NOPE")
        .await
        .expect_err("unknown claim");
    assert!(missing.contains("not found"), "{missing}");
}
