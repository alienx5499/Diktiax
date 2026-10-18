//! A person's review is final, and a claim held for conflicting verdicts always has someone to
//! review it. Requires a running FalkorDB:
//!   docker compose up -d falkordb && cargo test --test review_outcomes -- --ignored

mod common;

use common::TestGraph;
use diktiax_backend::actions::{ActionDispatcher, Webhook};
use diktiax_backend::agent::{ClaimInvestigator, InvestigationDossier, PlannerConfig};
use diktiax_backend::db::FalkorGraphRepository;
use diktiax_backend::domain::{ReviewDecision, ReviewRequest};
use falkordb::{FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use futures_core::Stream;
use std::pin::Pin;

async fn session(graph: &TestGraph) -> (FalkorGraphRepository, ClaimInvestigator) {
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB running");
    repo.seed(true).await.expect("seed");
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::new(None).expect("client"));
    (
        repo.clone(),
        ClaimInvestigator::new(repo, dispatcher, PlannerConfig::RuleBased),
    )
}

/// First column of every row of a query, through a separate client.
async fn column(graph: &TestGraph, cypher: &str) -> Vec<FalkorValue> {
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client");
    let mut res = client
        .select_graph(&graph.name)
        .query(cypher)
        .execute()
        .await
        .expect("query");
    let mut values = Vec::new();
    while let Some(row) = std::future::poll_fn(|cx| Pin::new(&mut res.data).poll_next(cx)).await {
        values.extend(row.expect("parse").into_iter().take(1));
    }
    values
}

async fn assigned_id(graph: &TestGraph, claim_id: &str) -> Option<String> {
    let ids = column(
        graph,
        &format!(
            "MATCH (:Claim {{id: '{claim_id}'}})-[:ASSIGNED_INVESTIGATOR]->(i:Investigator) RETURN i.id"
        ),
    )
    .await;
    match ids.first() {
        Some(FalkorValue::String(id)) => Some(id.clone()),
        _ => None,
    }
}

fn has_action(dossier: &InvestigationDossier, action: &str) -> bool {
    dossier
        .remediation
        .actions_log
        .iter()
        .any(|a| a.action_type == action)
}

/// Review, then re-investigate: the automated run records its verdict but keeps the reviewed
/// status and payout state, for both decisions.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn reinvestigation_keeps_the_reviewed_status() {
    let graph = TestGraph::new("diktiax_review_final_it");
    let (repo, investigator) = session(&graph).await;
    for (claim_id, decision, frozen) in [
        ("CLM-48291", ReviewDecision::ConfirmFraud, true),
        ("CLM-55104", ReviewDecision::Release, false),
    ] {
        investigator.investigate(claim_id).await.expect("first");
        let reviewer = assigned_id(&graph, claim_id).await.expect("assigned");
        let request = ReviewRequest {
            decision,
            reviewer,
            note: None,
        };
        repo.review_claim(claim_id, &request).await.expect("review");

        let again = investigator.investigate(claim_id).await.expect("again");
        assert_eq!(again.status, decision.claim_status(), "{claim_id}");
        assert_eq!(again.remediation.payout_frozen, frozen, "{claim_id}");
        assert!(has_action(&again, "KEEP_REVIEWED_STATUS"), "{claim_id}");
        assert!(!has_action(&again, "SET_CLAIM_STATUS"), "{claim_id}");
        assert!(!has_action(&again, "FREEZE_PAYOUT"), "{claim_id}");
        let stored = column(
            &graph,
            &format!("MATCH (c:Claim {{id: '{claim_id}'}}) RETURN c.status"),
        )
        .await;
        assert_eq!(
            stored,
            vec![FalkorValue::String(decision.claim_status().into())]
        );
        let history = repo.investigation_history(claim_id).await.expect("history");
        assert_eq!(
            history.len(),
            2,
            "{claim_id}: the verdict is still recorded"
        );
    }
    drop((repo, investigator));
    drop(graph);
}

/// A conflict on a claim that was never escalated (MEDIUM_RISK, then LOW_RISK) still gets an
/// investigator, so the held claim has someone to review it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn escalated_conflict_always_has_an_investigator() {
    let graph = TestGraph::new("diktiax_review_assign_it");
    let (repo, investigator) = session(&graph).await;
    column(
        &graph,
        "MATCH (:Vehicle {id: 'VEH-88102-NV'})-[r:HAS_VIN]->() DELETE r",
    )
    .await;
    let first = investigator.investigate("CLM-55104").await.expect("first");
    assert_eq!(first.risk_tier, "MEDIUM_RISK");
    assert_eq!(assigned_id(&graph, "CLM-55104").await, None);

    column(
        &graph,
        "MATCH (:Customer {id: 'CUST-1004'})-[r:HAS_PHONE|LIVES_AT]->() DELETE r",
    )
    .await;
    let second = investigator.investigate("CLM-55104").await.expect("second");
    assert_eq!(second.risk_tier, "LOW_RISK");
    assert_eq!(second.status, "NEEDS_REVIEW_CONFLICTING_VERDICTS");
    assert!(has_action(&second, "ASSIGN_INVESTIGATOR"));
    assert!(!second.remediation.payout_frozen);
    let reviewer = assigned_id(&graph, "CLM-55104").await.expect("assigned");
    let request = ReviewRequest {
        decision: ReviewDecision::Release,
        reviewer,
        note: None,
    };
    let review = repo
        .review_claim("CLM-55104", &request)
        .await
        .expect("review");
    assert_eq!(review.claim_status, "RELEASED_AFTER_REVIEW");
    drop((repo, investigator));
    drop(graph);
}
