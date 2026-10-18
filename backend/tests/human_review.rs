//! Human review of frozen payouts and escalation of conflicting verdicts. Requires a running
//! FalkorDB: docker compose up -d falkordb && cargo test --test human_review -- --ignored

mod common;

use common::TestGraph;
use diktiax_backend::actions::{ActionDispatcher, Webhook};
use diktiax_backend::agent::{ClaimInvestigator, PlannerConfig};
use diktiax_backend::db::queries::REVIEW_CONSTRAINT;
use diktiax_backend::db::FalkorGraphRepository;
use diktiax_backend::domain::{ReviewDecision, ReviewError, ReviewRequest};
use falkordb::{
    ConstraintType, EntityType, FalkorAsyncClient, FalkorClientBuilder, FalkorConnectionInfo,
    FalkorValue,
};
use futures_core::Stream;
use std::pin::Pin;
use std::sync::Arc;

const FRAUD_CLAIM: &str = "CLM-48291";
const VIN_CLAIM: &str = "CLM-55104";
const CLEAN_CLAIM: &str = "CLM-10022";
const RACERS: usize = 16;
const RACE_ROUNDS: usize = 20;

async fn session(graph: &TestGraph) -> (FalkorGraphRepository, ClaimInvestigator) {
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB running");
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::new(None).expect("client"));
    (
        repo.clone(),
        ClaimInvestigator::new(repo, dispatcher, PlannerConfig::RuleBased),
    )
}

async fn client(graph: &TestGraph) -> FalkorAsyncClient {
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client")
}

/// All rows of a query, through a client separate from the repository.
async fn query(graph: &TestGraph, cypher: &str) -> Vec<Vec<FalkorValue>> {
    let client = client(graph).await;
    let mut res = client
        .select_graph(&graph.name)
        .query(cypher)
        .execute()
        .await
        .expect("query");
    let mut rows = Vec::new();
    while let Some(row) = std::future::poll_fn(|cx| Pin::new(&mut res.data).poll_next(cx)).await {
        rows.push(row.expect("parse").into_iter().collect());
    }
    rows
}

async fn scalar(graph: &TestGraph, cypher: &str) -> FalkorValue {
    query(graph, cypher).await[0][0].clone()
}

async fn assigned(graph: &TestGraph, claim_id: &str) -> (String, i64) {
    let row = &query(
        graph,
        &format!(
            "MATCH (:Claim {{id: '{claim_id}'}})-[:ASSIGNED_INVESTIGATOR]->(i:Investigator) \
             RETURN i.id, i.open_cases"
        ),
    )
    .await[0];
    match (&row[0], &row[1]) {
        (FalkorValue::String(id), FalkorValue::I64(n)) => (id.clone(), *n),
        other => panic!("unexpected {other:?}"),
    }
}

fn request(decision: ReviewDecision, reviewer: &str) -> ReviewRequest {
    ReviewRequest {
        decision,
        reviewer: reviewer.into(),
        note: Some("checked the shared phone records".into()),
    }
}

/// The happy paths and every refusal: 404, 403 for another investigator, 409 for a claim that
/// is not frozen or was already reviewed; the write itself lands in one statement.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn review_confirms_or_releases_a_frozen_payout() {
    let graph = TestGraph::new("diktiax_review_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let dossier = investigator
        .investigate(FRAUD_CLAIM)
        .await
        .expect("dossier");
    assert_eq!(dossier.status, "FLAGGED_SIU_FRAUD");
    let (inv, open_before) = assigned(&graph, FRAUD_CLAIM).await;

    let confirm = request(ReviewDecision::ConfirmFraud, &inv);
    let other = request(ReviewDecision::Release, "INV-999");
    assert!(matches!(
        repo.review_claim("CLM-NOPE", &confirm).await,
        Err(ReviewError::NotFound(_))
    ));
    assert!(matches!(
        repo.review_claim(FRAUD_CLAIM, &other).await,
        Err(ReviewError::Forbidden(_))
    ));
    investigator.investigate(CLEAN_CLAIM).await.expect("clean");
    assert!(matches!(
        repo.review_claim(CLEAN_CLAIM, &confirm).await,
        Err(ReviewError::Conflict(_))
    ));

    let review = repo
        .review_claim(FRAUD_CLAIM, &confirm)
        .await
        .expect("review");
    assert_eq!(review.claim_status, "CONFIRMED_FRAUD");
    assert!(review.payout_frozen);
    assert_eq!(review.reviewer_id, inv);
    assert_eq!(review.investigation_id, dossier.certificate.certificate_id);
    let linked = scalar(
        &graph,
        &format!(
            "MATCH (:Investigator {{id: '{inv}'}})-[:REVIEWED]->(r:Review)-[:REVIEWS]->\
             (:Investigation {{id: '{}'}}) RETURN count(r)",
            review.investigation_id
        ),
    )
    .await;
    assert_eq!(linked, FalkorValue::I64(1));
    assert_eq!(assigned(&graph, FRAUD_CLAIM).await.1, open_before - 1);
    assert!(matches!(
        repo.review_claim(FRAUD_CLAIM, &confirm).await,
        Err(ReviewError::Conflict(_))
    ));

    investigator.investigate(VIN_CLAIM).await.expect("vin");
    let (vin_inv, _) = assigned(&graph, VIN_CLAIM).await;
    let mut release = request(ReviewDecision::Release, &vin_inv);
    release.note = None;
    let review = repo
        .review_claim(VIN_CLAIM, &release)
        .await
        .expect("release");
    assert_eq!(review.claim_status, "RELEASED_AFTER_REVIEW");
    assert!(!review.payout_frozen && review.note.is_none());
    let frozen = scalar(
        &graph,
        &format!("MATCH (c:Claim {{id: '{VIN_CLAIM}'}}) RETURN c.payout_frozen"),
    )
    .await;
    assert_eq!(frozen, FalkorValue::Bool(false));
    drop((repo, investigator));
    drop(graph);
}

/// RACERS reviews of one frozen claim at once, from two clients: `(accepted, conflicts,
/// Review nodes, open_cases decrement)`.
async fn race_reviews(
    graph: &TestGraph,
    repos: &[FalkorGraphRepository],
) -> (usize, usize, i64, i64) {
    let (inv, before) = assigned(graph, FRAUD_CLAIM).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(RACERS));
    let handles: Vec<_> = (0..RACERS)
        .map(|n| {
            let (repo, barrier) = (repos[n % repos.len()].clone(), barrier.clone());
            let req = request(ReviewDecision::ConfirmFraud, &inv);
            tokio::spawn(async move {
                barrier.wait().await;
                repo.review_claim(FRAUD_CLAIM, &req).await
            })
        })
        .collect();
    let (mut accepted, mut conflicts) = (0, 0);
    for handle in handles {
        match handle.await.expect("task") {
            Ok(_) => accepted += 1,
            Err(ReviewError::Conflict(_)) => conflicts += 1,
            Err(e) => panic!("unexpected {e:?}"),
        }
    }
    let reviews = match scalar(graph, "MATCH (r:Review) RETURN count(r)").await {
        FalkorValue::I64(n) => n,
        other => panic!("{other:?}"),
    };
    (
        accepted,
        conflicts,
        reviews,
        before - assigned(graph, FRAUD_CLAIM).await.1,
    )
}

async fn two_repos(graph: &TestGraph) -> Vec<FalkorGraphRepository> {
    let mut repos = Vec::new();
    for _ in 0..2 {
        repos.push(session(graph).await.0);
    }
    repos
}

/// 16 simultaneous reviews of one claim: exactly one Review, 15 rejections, one case freed.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_reviews_leave_one_review() {
    let graph = TestGraph::new("diktiax_review_race_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    investigator
        .investigate(FRAUD_CLAIM)
        .await
        .expect("dossier");
    let outcome = race_reviews(&graph, &two_repos(&graph).await).await;
    println!("review race (accepted, conflicts, reviews, freed): {outcome:?}");
    assert_eq!(outcome, (1, RACERS - 1, 1, 1));
    drop((repo, investigator));
    drop(graph);
}

/// The guard itself, without timing: a Review for the claim already exists but the claim is
/// still frozen (what a racing review that read the old status attempts). REVIEW_CONSTRAINT
/// rejects the commit and the whole statement rolls back: status and open_cases untouched.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn review_constraint_rejects_a_second_review() {
    let graph = TestGraph::new("diktiax_review_guard_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    investigator
        .investigate(FRAUD_CLAIM)
        .await
        .expect("dossier");
    let (inv, open_before) = assigned(&graph, FRAUD_CLAIM).await;
    query(
        &graph,
        &format!("CREATE (:Review {{id: 'REV-decoy', claim_id: '{FRAUD_CLAIM}'}})"),
    )
    .await;

    let err = repo
        .review_claim(FRAUD_CLAIM, &request(ReviewDecision::Release, &inv))
        .await
        .expect_err("second review");
    assert!(
        matches!(err, ReviewError::Conflict(ref m) if m.contains("already reviewed")),
        "{err:?}"
    );
    let status = scalar(
        &graph,
        &format!("MATCH (c:Claim {{id: '{FRAUD_CLAIM}'}}) RETURN c.status"),
    )
    .await;
    assert_eq!(status, FalkorValue::String("FLAGGED_SIU_FRAUD".into()));
    assert_eq!(assigned(&graph, FRAUD_CLAIM).await.1, open_before);
    assert_eq!(
        scalar(&graph, "MATCH (r:Review) RETURN count(r)").await,
        FalkorValue::I64(1)
    );
    drop((repo, investigator));
    drop(graph);
}

/// Tries to reproduce the race REVIEW_CONSTRAINT guards against: with the constraint dropped,
/// RACERS concurrent reviews per round, up to RACE_ROUNDS rounds, stopping at the first round
/// that leaves more than one Review. Observational: it reports and fails only on a broken setup.
/// On FalkorDB v4.22.1, 4 x 20 rounds (1,280 reviews) never left a second Review.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn without_the_constraint_concurrent_review_race_is_observed() {
    let graph = TestGraph::new("diktiax_review_dup_it");
    let (repo, investigator) = session(&graph).await;
    let repos = two_repos(&graph).await;
    let client = client(&graph).await;
    let (label, prop) = REVIEW_CONSTRAINT;
    let mut per_round = Vec::new();
    for _ in 0..RACE_ROUNDS {
        repo.seed(true).await.expect("seed");
        client
            .select_graph(&graph.name)
            .drop_constraint(ConstraintType::Unique, EntityType::Node, label, &[prop])
            .await
            .expect("drop constraint");
        investigator
            .investigate(FRAUD_CLAIM)
            .await
            .expect("dossier");
        let (accepted, conflicts, reviews, freed) = race_reviews(&graph, &repos).await;
        assert!(accepted >= 1 && accepted + conflicts == RACERS);
        assert_eq!((reviews, freed), (accepted as i64, accepted as i64));
        per_round.push(reviews);
        if reviews > 1 {
            break;
        }
    }
    println!(
        "WITHOUT CONSTRAINT: {} round(s) x {RACERS} reviews, Reviews per round {per_round:?}",
        per_round.len()
    );
    repo.seed(true)
        .await
        .expect("reseed restores the constraint");
    drop((repo, investigator, repos));
    drop(graph);
}

/// Memory changes the outcome: once the cloned VIN behind a verdict is gone, the
/// re-investigation drops a tier (HIGH_SUSPICION 75 -> MEDIUM_RISK 25), so the claim is held for
/// a person instead of the new verdict replacing the old one. An agreeing re-investigation
/// carries no escalation.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn disagreeing_reinvestigation_escalates_to_human_review() {
    let graph = TestGraph::new("diktiax_review_conflict_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let first = investigator.investigate(VIN_CLAIM).await.expect("first");
    let agree = investigator.investigate(VIN_CLAIM).await.expect("agree");
    let json = serde_json::to_value(&agree).expect("json");
    assert!(json["memory"].get("escalated").is_none());
    assert!(json["memory"].get("escalation_reason").is_none());
    assert_eq!(agree.status, first.status);

    query(
        &graph,
        "MATCH (:Vehicle {id: 'VEH-88102-NV'})-[r:HAS_VIN]->() DELETE r",
    )
    .await;
    let third = investigator.investigate(VIN_CLAIM).await.expect("third");
    println!(
        "tiers: {} ({}) -> {} ({})",
        first.risk_tier, first.risk_score, third.risk_tier, third.risk_score
    );
    assert_ne!(third.risk_tier, first.risk_tier);
    let memory = third.memory.as_ref().expect("memory");
    assert!(memory.escalated && !memory.agrees_with_latest);
    assert!(memory
        .escalation_reason
        .as_deref()
        .is_some_and(|r| r.contains(&first.risk_tier)));
    assert_eq!(third.status, "NEEDS_REVIEW_CONFLICTING_VERDICTS");
    assert!(third
        .remediation
        .actions_log
        .iter()
        .any(|a| a.action_type == "ESCALATE_TO_HUMAN_REVIEW"));
    let stored = scalar(
        &graph,
        &format!("MATCH (c:Claim {{id: '{VIN_CLAIM}'}}) RETURN c.status"),
    )
    .await;
    assert_eq!(
        stored,
        FalkorValue::String("NEEDS_REVIEW_CONFLICTING_VERDICTS".into())
    );

    // The held claim goes to its assigned investigator.
    let (inv, _) = assigned(&graph, VIN_CLAIM).await;
    let review = repo
        .review_claim(VIN_CLAIM, &request(ReviewDecision::ConfirmFraud, &inv))
        .await
        .expect("review");
    assert_eq!(review.investigation_id, third.certificate.certificate_id);
    drop((repo, investigator));
    drop(graph);
}
