//! Cross-session agent memory: verdicts recorded by one session are read back by a fresh
//! client (a new "session") on the same graph. Requires a running FalkorDB:
//!   docker compose up -d falkordb && cargo test --test agent_memory -- --ignored

mod common;

use claimgraph_backend::actions::{status_for, ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, PlannerConfig};
use claimgraph_backend::db::FalkorGraphRepository;
use common::TestGraph;
use falkordb::{FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use futures_core::Stream;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;

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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn verdicts_survive_across_sessions() {
    let graph = TestGraph::new("claimgraph_memory_it");

    let first_certificate = {
        let (repo, investigator) = session(&graph).await;
        repo.seed(true).await.expect("seed");
        assert!(repo
            .investigation_history("CLM-48291")
            .await
            .expect("history")
            .is_empty());
        let dossier = investigator
            .investigate("CLM-48291")
            .await
            .expect("dossier");
        dossier.certificate.certificate_id
    }; // first session's repository, investigator and client are dropped here

    let (repo, investigator) = session(&graph).await;
    let history = repo
        .investigation_history("CLM-48291")
        .await
        .expect("history");
    assert_eq!(history.len(), 1, "{history:?}");
    assert_eq!(history[0].decision, "CRITICAL_FRAUD");
    assert_eq!(history[0].risk_score, 100);
    assert_eq!(history[0].certificate_id, first_certificate);

    let second = investigator.investigate("CLM-48291").await.expect("again");
    let history = repo
        .investigation_history("CLM-48291")
        .await
        .expect("history");
    assert_eq!(history.len(), 2, "{history:?}");
    assert_eq!(history[0].certificate_id, second.certificate.certificate_id);
    assert_eq!(history[1].certificate_id, first_certificate);
    assert_ne!(history[0].certificate_id, history[1].certificate_id);
    assert!(history[0].recorded_at > history[1].recorded_at);

    let missing = repo
        .investigation_history("CLM-NOPE")
        .await
        .expect_err("unknown claim");
    assert!(missing.contains("not found"), "{missing}");

    drop((repo, investigator));
    drop(graph); // deletes the graph
}

const FLAGGED: [&str; 5] = [
    "CLM-48291",
    "CLM-39102",
    "CLM-44730",
    "CLM-55104",
    "CLM-31205",
];
const LOW_RISK: [&str; 3] = ["CLM-10022", "CLM-20417", "CLM-20988"];

/// All rows of a read-only query, through a client separate from the repository.
async fn read(graph: &TestGraph, query: &str) -> Vec<Vec<FalkorValue>> {
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client");
    let mut res = client
        .select_graph(&graph.name)
        .ro_query(query)
        .execute()
        .await
        .expect("query");
    let mut rows = Vec::new();
    while let Some(row) = std::future::poll_fn(|cx| Pin::new(&mut res.data).poll_next(cx)).await {
        rows.push(row.expect("parse").into_iter().collect());
    }
    rows
}

/// `(string, int)` rows, e.g. `RETURN id, count`.
async fn read_counts(graph: &TestGraph, query: &str) -> HashMap<String, i64> {
    read(graph, query)
        .await
        .into_iter()
        .map(|row| match (&row[0], &row[1]) {
            (FalkorValue::String(k), FalkorValue::I64(n)) => (k.clone(), *n),
            other => panic!("unexpected row {other:?} for {query}"),
        })
        .collect()
}

const OPEN_CASES: &str = "MATCH (i:Investigator) RETURN i.id, i.open_cases";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_investigations_keep_shared_state_consistent() {
    let graph = TestGraph::new("claimgraph_concurrency_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let before = read_counts(&graph, OPEN_CASES).await;

    let handles: Vec<_> = FLAGGED
        .iter()
        .chain(LOW_RISK.iter())
        .flat_map(|id| [*id, *id])
        .map(|id| {
            let investigator = investigator.clone();
            tokio::spawn(async move { (id, investigator.investigate(id).await) })
        })
        .collect();
    assert_eq!(handles.len(), 16);

    // (a) every run is Ok, and decides like a single run would.
    let mut certificates = HashSet::new();
    for handle in handles {
        let (id, result) = handle.await.expect("task");
        let dossier = result.unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(dossier.claim_id, id);
        if FLAGGED.contains(&id) {
            assert!(
                status_for(&dossier.risk_tier).1,
                "{id}: {}",
                dossier.risk_tier
            );
        } else {
            assert_eq!(dossier.risk_tier, "LOW_RISK", "{id}");
            assert_eq!(dossier.remediation.assigned_investigator, "unassigned");
        }
        certificates.insert(dossier.certificate.certificate_id);
    }

    // (b) two Investigation nodes per claim, 16 distinct certificates.
    let investigations = read_counts(
        &graph,
        "MATCH (c:Claim)-[:HAS_INVESTIGATION]->(i:Investigation) RETURN c.id, count(i)",
    )
    .await;
    assert_eq!(investigations.len(), 8, "{investigations:?}");
    assert!(
        investigations.values().all(|&n| n == 2),
        "{investigations:?}"
    );
    assert_eq!(certificates.len(), 16);
    let stored = read(
        &graph,
        "MATCH (i:Investigation) RETURN count(DISTINCT i.id)",
    )
    .await;
    assert_eq!(stored[0][0], FalkorValue::I64(16));

    // Read every count first so a failure still prints the full picture.
    let edges = read_counts(
        &graph,
        "MATCH (c:Claim)-[r:ASSIGNED_INVESTIGATOR]->(:Investigator) RETURN c.id, count(r)",
    )
    .await;
    let assigned = read(
        &graph,
        "MATCH (c:Claim)-[r:ASSIGNED_INVESTIGATOR]->(i:Investigator) \
         RETURN c.id, i.id, id(r) ORDER BY c.id",
    )
    .await;
    let after = read_counts(&graph, OPEN_CASES).await;
    let delta: i64 = after.iter().map(|(id, n)| n - before[id]).sum();
    println!("edges per claim {edges:?}\nedges {assigned:?}");
    println!("open_cases before {before:?} after {after:?} delta {delta}");

    // (c) MERGE keeps one assignment edge per claim, even with both runs racing.
    assert_eq!(
        edges.keys().map(String::as_str).collect::<HashSet<_>>(),
        FLAGGED.into_iter().collect::<HashSet<_>>(),
        "{edges:?}"
    );
    assert!(edges.values().all(|&n| n == 1), "{edges:?}");

    // (d) open_cases grew once per assigned claim, not once per run.
    assert_eq!(delta, edges.len() as i64);

    drop((repo, investigator));
    drop(graph);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn sequential_reinvestigation_reuses_the_assignment() {
    let graph = TestGraph::new("claimgraph_reassign_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let before = read_counts(&graph, OPEN_CASES).await;

    let mut reported = Vec::new();
    for _ in 0..3 {
        let dossier = investigator.investigate("CLM-48291").await.expect("run");
        let assign = dossier
            .remediation
            .actions_log
            .iter()
            .find(|a| a.action_type == "ASSIGN_INVESTIGATOR")
            .expect("assign action");
        assert_eq!(assign.status, "CONFIRMED");
        reported.push(dossier.remediation.assigned_investigator);
    }
    assert!(reported.iter().all(|r| r == &reported[0]), "{reported:?}");

    let edges = read_counts(
        &graph,
        "MATCH (c:Claim)-[r:ASSIGNED_INVESTIGATOR]->(:Investigator) RETURN c.id, count(r)",
    )
    .await;
    assert_eq!(edges.get("CLM-48291"), Some(&1), "{edges:?}");
    let after = read_counts(&graph, OPEN_CASES).await;
    let delta: i64 = after.iter().map(|(id, n)| n - before[id]).sum();
    assert_eq!(delta, 1);

    drop((repo, investigator));
    drop(graph);
}
