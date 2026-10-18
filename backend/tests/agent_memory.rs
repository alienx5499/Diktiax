//! Cross-session agent memory: verdicts recorded by one session are read back by a fresh
//! client (a new "session") on the same graph. Requires a running FalkorDB:
//!   docker compose up -d falkordb && cargo test --test agent_memory -- --ignored

mod common;

use claimgraph_backend::actions::{status_for, ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, InvestigationDossier, PlannerConfig};
use claimgraph_backend::db::queries::ASSIGNMENT_CONSTRAINT;
use claimgraph_backend::db::FalkorGraphRepository;
use claimgraph_backend::mcp::InvestigateClaim;
use common::TestGraph;
use falkordb::{
    ConstraintType, EntityType, FalkorAsyncClient, FalkorClientBuilder, FalkorConnectionInfo,
    FalkorValue,
};
use futures_core::Stream;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::Arc;

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

const RACE_CLAIM: &str = "CLM-48291";
const RACERS: usize = 32;
const RACE_ROUNDS: usize = 20;

/// `(claim edges, open_cases delta)` for RACE_CLAIM against a `before` snapshot.
async fn race_outcome(graph: &TestGraph, before: &HashMap<String, i64>) -> (i64, i64) {
    let edges = read_counts(
        graph,
        "MATCH (c:Claim)-[r:ASSIGNED_INVESTIGATOR]->(:Investigator) RETURN c.id, count(r)",
    )
    .await;
    let after = read_counts(graph, OPEN_CASES).await;
    let delta: i64 = after.iter().map(|(id, n)| n - before[id]).sum();
    println!("edges {edges:?} open_cases {before:?} -> {after:?}");
    (edges.get(RACE_CLAIM).copied().unwrap_or(0), delta)
}

/// The assignment statement alone, no process lock: 32 concurrent calls on one claim, 20 rounds
/// on a freshly seeded graph. Two clients give 16 sockets in flight: more than the server's 4
/// query threads, under its MAX_QUEUED_QUERIES of 25 (beyond it: "pending queries exceeded").
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_direct_assignment_creates_one_edge() {
    let names = vec!["Collusion Ring".to_string()];
    let graph = TestGraph::new("claimgraph_assign_race_it");
    let mut repos = Vec::new();
    for _ in 0..2 {
        repos.push(
            FalkorGraphRepository::connect(&graph.url, &graph.name)
                .await
                .expect("FalkorDB running"),
        );
    }
    for round in 0..RACE_ROUNDS {
        repos[0].seed(true).await.expect("seed"); // a fresh graph each round, same clients
        let before = read_counts(&graph, OPEN_CASES).await;

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(RACERS));
        let handles: Vec<_> = (0..RACERS)
            .map(|n| {
                let (repo, names, barrier) = (
                    repos[n % repos.len()].clone(),
                    names.clone(),
                    barrier.clone(),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    repo.assign_once(RACE_CLAIM, &names).await
                })
            })
            .collect();
        let mut reported = HashSet::new();
        for handle in handles {
            let assigned = handle.await.expect("task").expect("assign").expect("some");
            reported.insert(assigned.id);
        }

        let (edges, delta) = race_outcome(&graph, &before).await;
        assert_eq!(edges, 1, "round {round}: {edges} edges");
        assert_eq!(delta, 1, "round {round}: open_cases delta {delta}");
        assert_eq!(reported.len(), 1, "round {round}: {reported:?}");
    }
    drop(repos);
    drop(graph);
}

async fn has_assignment_constraint(client: &FalkorAsyncClient, graph: &TestGraph) -> bool {
    let (rel, prop) = ASSIGNMENT_CONSTRAINT;
    client
        .select_graph(&graph.name)
        .list_constraints()
        .await
        .expect("constraints")
        .data
        .iter()
        .any(|c| c.entity_type == EntityType::Edge && c.label == rel && c.properties == [prop])
}

/// Tries to reproduce the race ASSIGNMENT_CONSTRAINT guards against: with the constraint
/// dropped, 32 concurrent assign_once calls per round (two clients), up to RACE_ROUNDS rounds,
/// stopping at the first duplicate edge. Observational: it reports what happened and fails only
/// on a broken setup. On FalkorDB v4.22.1 (4 query threads) 3 x 20 rounds never left more than
/// one edge, so the single MERGE statement is what serialised here; the constraint stays as the
/// guarantee, since FalkorDB does not document that one statement is atomic. Reseeding at the
/// end restores the constraint.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn without_the_constraint_concurrent_assignment_race_is_observed() {
    let names = vec!["Collusion Ring".to_string()];
    let graph = TestGraph::new("claimgraph_assign_dup_it");
    let mut repos = Vec::new();
    for _ in 0..2 {
        repos.push(
            FalkorGraphRepository::connect(&graph.url, &graph.name)
                .await
                .expect("FalkorDB running"),
        );
    }
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client");
    let (rel, prop) = ASSIGNMENT_CONSTRAINT;
    let mut per_round = Vec::new();
    for round in 0..RACE_ROUNDS {
        repos[0].seed(true).await.expect("seed");
        client
            .select_graph(&graph.name)
            .drop_constraint(ConstraintType::Unique, EntityType::Edge, rel, &[prop])
            .await
            .expect("drop constraint");
        assert!(!has_assignment_constraint(&client, &graph).await);
        let before = read_counts(&graph, OPEN_CASES).await;

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(RACERS));
        let handles: Vec<_> = (0..RACERS)
            .map(|n| {
                let (repo, names, barrier) = (
                    repos[n % repos.len()].clone(),
                    names.clone(),
                    barrier.clone(),
                );
                tokio::spawn(async move {
                    barrier.wait().await;
                    repo.assign_once(RACE_CLAIM, &names).await
                })
            })
            .collect();
        for handle in handles {
            handle.await.expect("task").expect("assign").expect("some");
        }
        let (edges, delta) = race_outcome(&graph, &before).await;
        assert!(
            edges >= 1 && delta == edges,
            "round {round}: {edges} edges, delta {delta}"
        );
        per_round.push(edges);
        if edges > 1 {
            break;
        }
    }
    let duplicates: i64 = per_round.iter().map(|e| e - 1).sum();
    println!(
        "WITHOUT CONSTRAINT: {} round(s) x {RACERS} calls, edges per round {per_round:?}, \
         {duplicates} duplicate edge(s)",
        per_round.len()
    );

    repos[0]
        .seed(true)
        .await
        .expect("reseed restores the constraint");
    assert!(has_assignment_constraint(&client, &graph).await);
    drop(repos);
    drop(graph);
}

/// End to end: 32 concurrent investigations of the same claim share one assignment.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_investigations_of_one_claim_assign_once() {
    let graph = TestGraph::new("claimgraph_same_claim_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let before = read_counts(&graph, OPEN_CASES).await;

    let handles: Vec<_> = (0..RACERS)
        .map(|_| {
            let investigator = investigator.clone();
            tokio::spawn(async move { investigator.investigate(RACE_CLAIM).await })
        })
        .collect();
    let mut reported = HashSet::new();
    for handle in handles {
        let dossier = handle.await.expect("task").expect("dossier");
        let assign = dossier
            .remediation
            .actions_log
            .iter()
            .find(|a| a.action_type == "ASSIGN_INVESTIGATOR")
            .expect("assign action");
        assert_eq!(assign.status, "CONFIRMED");
        reported.insert(dossier.remediation.assigned_investigator);
    }

    let investigations = read(
        &graph,
        "MATCH (:Claim {id: 'CLM-48291'})-[:HAS_INVESTIGATION]->(i:Investigation) RETURN count(i)",
    )
    .await;
    assert_eq!(investigations[0][0], FalkorValue::I64(RACERS as i64));
    let (edges, delta) = race_outcome(&graph, &before).await;
    assert_eq!(edges, 1);
    assert_eq!(delta, 1);
    assert_eq!(reported.len(), 1, "{reported:?}");

    drop((repo, investigator));
    drop(graph);
}

/// The guard itself: seeding installs the unique constraint, and a second edge carrying the same
/// claim id is rejected with open_cases rolled back. A decoy edge from another claim holds
/// RACE_CLAIM's id, so the statement sees no assignment of its own and tries to create one.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn assignment_constraint_rejects_a_second_edge() {
    let graph = TestGraph::new("claimgraph_assign_guard_it");
    let (repo, _) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client");
    client
        .select_graph(&graph.name)
        .query(
            "MATCH (c:Claim {id: 'CLM-10022'}), (i:Investigator {id: 'INV-201'}) \
             CREATE (c)-[:ASSIGNED_INVESTIGATOR {claim_id: 'CLM-48291'}]->(i)",
        )
        .execute()
        .await
        .expect("decoy edge");
    let before = read_counts(&graph, OPEN_CASES).await;

    let err = repo
        .assign_once(RACE_CLAIM, &[])
        .await
        .expect_err("constraint violation");
    assert!(err.contains("constraint violation"), "{err}");
    assert_eq!(read_counts(&graph, OPEN_CASES).await, before);

    drop(repo);
    drop(graph);
}

/// The external `investigate_claim` over a fresh session on `graph`.
async fn mcp_tool(graph: &TestGraph) -> InvestigateClaim {
    let (repo, investigator) = session(graph).await;
    InvestigateClaim::new(repo, investigator)
}

async fn investigation_count(graph: &TestGraph, claim_id: &str) -> i64 {
    let rows = read(
        graph,
        &format!(
            "MATCH (:Claim {{id: '{claim_id}'}})-[:HAS_INVESTIGATION]->(i:Investigation) \
             RETURN count(i)"
        ),
    )
    .await;
    match rows[0][0] {
        FalkorValue::I64(n) => n,
        ref other => panic!("unexpected count {other:?}"),
    }
}

fn is_reused(response: &Value) -> bool {
    response["reused"] == json!(true)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn mcp_investigate_claim_reuses_the_verdict_unless_forced() {
    let graph = TestGraph::new("claimgraph_mcp_reuse_it");
    let (repo, _) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let tool = mcp_tool(&graph).await;
    let args = json!({ "claim_id": RACE_CLAIM });

    let first = tool.call(&args).await.expect("first");
    assert!(!is_reused(&first), "{first}");
    let certificate = first["certificate"]["certificate_id"].clone();
    assert_eq!(investigation_count(&graph, RACE_CLAIM).await, 1);

    // Second call: answered from history, the agent does not run and nothing is written.
    let second = tool.call(&args).await.expect("second");
    assert_eq!(
        second,
        json!({
            "reused": true,
            "claim_id": RACE_CLAIM,
            "certificate_id": certificate,
            "decision": "CRITICAL_FRAUD",
            "risk_score": 100,
            "recorded_at": second["recorded_at"],
        })
    );
    assert!(second["recorded_at"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));
    assert_eq!(investigation_count(&graph, RACE_CLAIM).await, 1);

    // force: true runs a fresh investigation; the next plain call reuses that newer verdict.
    let forced = tool
        .call(&json!({ "claim_id": RACE_CLAIM, "force": true }))
        .await
        .expect("forced");
    assert!(!is_reused(&forced), "{forced}");
    let newer = forced["certificate"]["certificate_id"].clone();
    assert_ne!(newer, certificate);
    assert_eq!(investigation_count(&graph, RACE_CLAIM).await, 2);
    let third = tool.call(&args).await.expect("third");
    assert!(
        is_reused(&third) && third["certificate_id"] == newer,
        "{third}"
    );

    let bad = tool
        .call(&json!({ "claim_id": RACE_CLAIM, "force": "yes" }))
        .await
        .expect_err("non-boolean force");
    assert!(bad.contains("force"), "{bad}");
    let missing = tool
        .call(&json!({ "claim_id": "CLM-NOPE" }))
        .await
        .expect_err("unknown claim");
    assert!(missing.contains("not found"), "{missing}");
    assert_eq!(tool.tracked_claims(), 0, "lock map must not grow");

    drop((repo, tool));
    drop(graph);
}

const MCP_RACERS: usize = 8;

/// 8 concurrent `investigate_claim` calls on a never-investigated claim run the agent once.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_mcp_investigate_claim_runs_the_agent_once() {
    let graph = TestGraph::new("claimgraph_mcp_race_it");
    let (repo, _) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let tool = Arc::new(mcp_tool(&graph).await);

    let barrier = Arc::new(tokio::sync::Barrier::new(MCP_RACERS));
    let handles: Vec<_> = (0..MCP_RACERS)
        .map(|_| {
            let (tool, barrier) = (tool.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                tool.call(&json!({ "claim_id": RACE_CLAIM })).await
            })
        })
        .collect();
    let mut responses = Vec::new();
    for handle in handles {
        responses.push(handle.await.expect("task").expect("response"));
    }

    let (reused, fresh): (Vec<_>, Vec<_>) = responses.iter().partition(|r| is_reused(r));
    let count = investigation_count(&graph, RACE_CLAIM).await;
    assert_eq!((fresh.len(), reused.len(), count), (1, 7, 1));
    let certificate = &fresh[0]["certificate"]["certificate_id"];
    assert!(reused.iter().all(|r| &r["certificate_id"] == certificate));
    assert_eq!(tool.tracked_claims(), 0, "lock map must not grow");

    drop((repo, tool));
    drop(graph);
}

/// The lock is per claim: concurrent calls on 8 different claims each run their own investigation.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_mcp_investigate_claim_on_different_claims_each_run() {
    let graph = TestGraph::new("claimgraph_mcp_parallel_it");
    let (repo, _) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let tool = Arc::new(mcp_tool(&graph).await);

    let claims: Vec<&str> = FLAGGED.iter().chain(LOW_RISK.iter()).copied().collect();
    let barrier = Arc::new(tokio::sync::Barrier::new(claims.len()));
    let handles: Vec<_> = claims
        .iter()
        .map(|&id| {
            let (tool, barrier) = (tool.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                (id, tool.call(&json!({ "claim_id": id })).await)
            })
        })
        .collect();
    let mut certificates = HashSet::new();
    for handle in handles {
        let (id, result) = handle.await.expect("task");
        let dossier = result.unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(!is_reused(&dossier), "{id}: {dossier}");
        assert_eq!(dossier["claim_id"], id);
        certificates.insert(dossier["certificate"]["certificate_id"].to_string());
    }
    assert_eq!(certificates.len(), claims.len());
    for id in &claims {
        assert_eq!(investigation_count(&graph, id).await, 1, "{id}");
    }
    assert_eq!(tool.tracked_claims(), 0, "lock map must not grow");

    drop((repo, tool));
    drop(graph);
}

const CHAIN_RACERS: usize = 16;

fn text(value: &FalkorValue) -> String {
    match value {
        FalkorValue::String(s) => s.clone(),
        other => panic!("expected a string, got {other:?}"),
    }
}

/// Ids of RACE_CLAIM's investigations and its SUPERSEDES pairs `(newer, older)`, checking each
/// edge's `superseded_id` names its target.
async fn chain(graph: &TestGraph) -> (Vec<String>, Vec<(String, String)>) {
    let ids = read(
        graph,
        "MATCH (:Claim {id: 'CLM-48291'})-[:HAS_INVESTIGATION]->(i:Investigation) RETURN i.id",
    )
    .await
    .iter()
    .map(|row| text(&row[0]))
    .collect();
    let pairs = read(
        graph,
        "MATCH (:Claim {id: 'CLM-48291'})-[:HAS_INVESTIGATION]->(a:Investigation)\
         -[r:SUPERSEDES]->(b:Investigation) RETURN a.id, b.id, r.superseded_id",
    )
    .await
    .iter()
    .map(|row| {
        assert_eq!(row[1], row[2], "superseded_id must name the target");
        (text(&row[0]), text(&row[1]))
    })
    .collect();
    (ids, pairs)
}

/// The invariant: RACE_CLAIM's `n` investigations form one linear SUPERSEDES chain. Exactly one
/// head (nothing supersedes it), no verdict superseded twice, none superseding two, n - 1 edges,
/// and walking from the head visits every investigation.
async fn assert_linear_chain(graph: &TestGraph, n: usize, context: &str) {
    let (ids, pairs) = chain(graph).await;
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for (newer, older) in &pairs {
        outgoing.entry(newer).or_default().push(older);
        incoming.entry(older).or_default().push(newer);
    }
    let heads: Vec<&String> = ids
        .iter()
        .filter(|id| !incoming.contains_key(id.as_str()))
        .collect();
    println!(
        "{context}: {} investigations, {} SUPERSEDES, heads {heads:?}",
        ids.len(),
        pairs.len()
    );
    assert_eq!(ids.len(), n, "{context}: investigations");
    assert!(
        incoming.values().all(|v| v.len() == 1),
        "{context}: a verdict superseded twice: {incoming:?}"
    );
    assert!(
        outgoing.values().all(|v| v.len() == 1),
        "{context}: a verdict superseding two: {outgoing:?}"
    );
    assert_eq!(heads.len(), 1, "{context}: heads {heads:?}");
    assert_eq!(pairs.len(), n - 1, "{context}: chain length");
    let mut seen = HashSet::new();
    let mut at = heads[0].as_str();
    while seen.insert(at) {
        match outgoing.get(at) {
            Some(next) => at = next[0],
            None => break,
        }
    }
    assert_eq!(seen.len(), n, "{context}: chain from the head is broken");
    let roots = read(
        graph,
        "MATCH (m:InvestigationChain {claim_id: 'CLM-48291'}) RETURN count(m)",
    )
    .await;
    assert_eq!(roots[0][0], FalkorValue::I64(1), "{context}: chain roots");
}

/// Two sessions (clients), 8 investigations each, all on one claim at once.
async fn two_sessions(graph: &TestGraph) -> Vec<(FalkorGraphRepository, ClaimInvestigator)> {
    let mut sessions = Vec::new();
    for _ in 0..2 {
        sessions.push(session(graph).await);
    }
    sessions[0].0.seed(true).await.expect("seed");
    sessions
}

/// End to end: 16 concurrent investigations of one claim leave one linear SUPERSEDES chain.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_investigations_form_one_supersedes_chain() {
    let graph = TestGraph::new("claimgraph_chain_it");
    let sessions = two_sessions(&graph).await;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(CHAIN_RACERS));
    let handles: Vec<_> = (0..CHAIN_RACERS)
        .map(|n| {
            let (investigator, barrier) = (sessions[n % 2].1.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                investigator.investigate(RACE_CLAIM).await
            })
        })
        .collect();
    for handle in handles {
        handle.await.expect("task").expect("dossier");
    }
    assert_linear_chain(&graph, CHAIN_RACERS, "investigations").await;
    drop(sessions);
    drop(graph);
}

/// The record statement alone, released together by a barrier: the tightest race on the head.
/// RACE_ROUNDS rounds, each on a freshly seeded graph.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_records_form_one_supersedes_chain() {
    let graph = TestGraph::new("claimgraph_chain_direct_it");
    let sessions = two_sessions(&graph).await;
    for round in 0..RACE_ROUNDS {
        sessions[0].0.seed(true).await.expect("seed");
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(CHAIN_RACERS));
        let handles: Vec<_> = (0..CHAIN_RACERS)
            .map(|n| {
                let (repo, barrier) = (sessions[n % 2].0.clone(), barrier.clone());
                tokio::spawn(async move {
                    barrier.wait().await;
                    let cert = format!("CERT-race-{n}");
                    repo.record_investigation(RACE_CLAIM, "CRITICAL_FRAUD", 100, &cert)
                        .await
                })
            })
            .collect();
        for handle in handles {
            handle.await.expect("task").expect("record");
        }
        assert_linear_chain(&graph, CHAIN_RACERS, &format!("records round {round}")).await;
    }
    drop(sessions);
    drop(graph);
}

/// RELIED_ON points at the linked claims the detectors returned; a first investigation has no
/// SUPERSEDES and its dossier subgraph shows neither new edge; the second supersedes the first.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn reinvestigation_supersedes_and_relies_on_linked_claims() {
    let graph = TestGraph::new("claimgraph_memory_edges_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    let relied_on = |cert: String| {
        let graph = &graph;
        async move {
            read(
                graph,
                &format!(
                    "MATCH (:Investigation {{id: '{cert}'}})-[:RELIED_ON]->(c:Claim) RETURN c.id"
                ),
            )
            .await
            .into_iter()
            .map(|row| text(&row[0]))
            .collect::<HashSet<_>>()
        }
    };

    let first = investigator.investigate(RACE_CLAIM).await.expect("first");
    let first_cert = first.certificate.certificate_id.clone();
    let linked: HashSet<String> = first
        .patterns_matched
        .iter()
        .flat_map(|p| &p.evidence_path)
        .filter(|id| id.starts_with("CLM-") && id.as_str() != RACE_CLAIM)
        .cloned()
        .collect();
    let first_targets = relied_on(first_cert.clone()).await;
    println!("first RELIED_ON {first_targets:?}, detector-linked {linked:?}");
    assert!(!first_targets.is_empty());
    assert_eq!(first_targets, linked);
    let allowed: HashSet<String> = ["CLM-39102", "CLM-44730"].map(String::from).into();
    assert!(first_targets.is_subset(&allowed), "{first_targets:?}");
    let (_, pairs) = chain(&graph).await;
    assert!(pairs.is_empty(), "first run supersedes nothing: {pairs:?}");
    assert!(
        first
            .subgraph
            .edges
            .iter()
            .all(|e| e.label != "RELIED_ON" && e.label != "SUPERSEDES"),
        "first dossier subgraph unchanged"
    );

    let second = investigator.investigate(RACE_CLAIM).await.expect("second");
    let second_cert = second.certificate.certificate_id.clone();
    let (_, pairs) = chain(&graph).await;
    assert_eq!(pairs, vec![(second_cert.clone(), first_cert)]);
    assert_eq!(relied_on(second_cert).await, linked);

    drop((repo, investigator));
    drop(graph);
}

/// Runs a write statement through a client separate from the repository.
async fn write(graph: &TestGraph, query: &str) {
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("client");
    client
        .select_graph(&graph.name)
        .query(query)
        .execute()
        .await
        .expect("write");
}

async fn total_investigation_count(graph: &TestGraph) -> FalkorValue {
    read(graph, "MATCH (i:Investigation) RETURN count(i)").await[0][0].clone()
}

/// The guards themselves, without relying on timing: each decoy makes the statement attempt
/// exactly what a lost race would (a second chain start, or superseding an already superseded
/// verdict). The commit is rejected and rolled back, so no Investigation node is left behind.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn chain_constraints_reject_a_fork() {
    let graph = TestGraph::new("claimgraph_chain_guard_it");
    let (repo, _) = session(&graph).await;
    repo.seed(true).await.expect("seed");

    // A chain start already exists, but no verdict is visible: a racing first run.
    write(
        &graph,
        "CREATE (:InvestigationChain {claim_id: 'CLM-48291'})",
    )
    .await;
    let err = repo
        .record_investigation(RACE_CLAIM, "CRITICAL_FRAUD", 100, "CERT-second-root")
        .await
        .expect_err("second chain start");
    assert!(err.contains("constraint violation"), "{err}");
    assert_eq!(total_investigation_count(&graph).await, FalkorValue::I64(0));

    // The head is already superseded by an edge the statement cannot see as incoming (a decoy
    // on another node holds its superseded_id): a racing run that read a stale head.
    write(&graph, "MATCH (m:InvestigationChain) DELETE m").await;
    repo.record_investigation(RACE_CLAIM, "CRITICAL_FRAUD", 100, "CERT-head")
        .await
        .expect("first record");
    write(
        &graph,
        "MATCH (c:Claim {id: 'CLM-10022'}) CREATE (c)-[:HAS_INVESTIGATION]->(:Investigation \
         {id: 'CERT-decoy'})-[:SUPERSEDES {superseded_id: 'CERT-head'}]->(:Investigation {id: 'CERT-other'})",
    )
    .await;
    let before = total_investigation_count(&graph).await;
    let err = repo
        .record_investigation(RACE_CLAIM, "CRITICAL_FRAUD", 100, "CERT-fork")
        .await
        .expect_err("head superseded twice");
    assert!(err.contains("constraint violation"), "{err}");
    assert_eq!(total_investigation_count(&graph).await, before);

    drop(repo);
    drop(graph);
}

/// Rule-based trace length of a first investigation on a freshly seeded graph (flagged claims
/// include the one ring_components call that picks the corroboration targets).
const FIRST_RUN_STEPS: [(&str, usize); 3] = [("CLM-48291", 9), ("CLM-55104", 8), ("CLM-10022", 3)];

fn calls_on<'a>(dossier: &'a InvestigationDossier, claim_id: &str) -> Vec<&'a str> {
    dossier
        .agent_trace
        .iter()
        .filter(|s| s.arguments.get("claim_id").and_then(Value::as_str) == Some(claim_id))
        .map(|s| s.tool.as_str())
        .collect()
}

/// A first investigation is exactly what it was before memory existed: no `memory` field in
/// the JSON, same number of steps.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn first_run_has_no_memory_and_unchanged_steps() {
    let graph = TestGraph::new("claimgraph_memory_first_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");
    for (claim_id, steps) in FIRST_RUN_STEPS {
        let dossier = investigator.investigate(claim_id).await.expect("dossier");
        assert!(dossier.memory.is_none(), "{claim_id}");
        let json = serde_json::to_value(&dossier).expect("json");
        assert!(json.get("memory").is_none(), "{claim_id}: memory omitted");
        assert!(!dossier.summary_rationale.contains("Memory:"));
        assert_eq!(
            dossier.agent_trace.len(),
            steps,
            "{claim_id} first-run steps"
        );
    }
    drop((repo, investigator));
    drop(graph);
}

/// The second run reads the first verdict, skips corroboration on the linked claims the first
/// verdict relied on (both ring members the WCC step picked), and still reaches the same score
/// from the target-claim detectors.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn second_run_remembers_and_skips_corroborated_work() {
    let graph = TestGraph::new("claimgraph_memory_second_it");
    let (repo, investigator) = session(&graph).await;
    repo.seed(true).await.expect("seed");

    let first = investigator.investigate(RACE_CLAIM).await.expect("first");
    let second = investigator.investigate(RACE_CLAIM).await.expect("second");
    let memory = second.memory.as_ref().expect("memory on re-investigation");
    println!(
        "first {} calls, second {} calls, memory {memory:?}",
        first.agent_trace.len(),
        second.agent_trace.len()
    );

    assert_eq!(memory.prior_investigations, 1);
    assert_eq!(
        memory.latest_prior.certificate_id,
        first.certificate.certificate_id
    );
    assert_eq!(memory.latest_prior.decision, first.risk_tier);
    assert_eq!(memory.latest_prior.risk_score, first.risk_score);
    assert!(memory.agrees_with_latest);
    assert_eq!(memory.skipped_corroboration, vec!["CLM-44730", "CLM-39102"]);
    let linked = ["CLM-44730", "CLM-39102"];
    let saved: usize = linked.iter().map(|id| calls_on(&first, id).len()).sum();
    assert_eq!(memory.tool_calls_saved, saved);
    assert!(memory.tool_calls_saved > 0);
    assert!(linked.iter().all(|id| calls_on(&second, id).is_empty()));
    assert_eq!(
        second.agent_trace.len(),
        first.agent_trace.len() - memory.tool_calls_saved
    );
    // Target-claim work and the verdict are untouched.
    assert_eq!(calls_on(&second, RACE_CLAIM), calls_on(&first, RACE_CLAIM));
    assert_eq!(second.risk_score, first.risk_score);
    assert_eq!(second.risk_tier, first.risk_tier);
    assert_eq!(second.patterns_matched.len(), first.patterns_matched.len());
    let note = format!(
        "Memory: 1 earlier verdict; latest {} was CRITICAL_FRAUD; this run agrees.",
        first.certificate.certificate_id
    );
    assert!(second.summary_rationale.contains(&note), "{note}");
    let json = serde_json::to_value(&second).expect("json");
    assert_eq!(json["memory"]["prior_investigations"], 1);
    assert_eq!(json["memory"]["tool_calls_saved"], memory.tool_calls_saved);

    drop((repo, investigator));
    drop(graph);
}

const MEMORY_RACERS: usize = 8;

/// 8 re-investigations at once: the SUPERSEDES chain stays linear and each run's memory block
/// is one consistent snapshot: its count names a real prefix of the chain, its latest verdict is
/// the newest in that prefix, and what it skipped is what that verdict relied on.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn concurrent_reinvestigations_report_consistent_memory() {
    let graph = TestGraph::new("claimgraph_memory_race_it");
    let sessions = two_sessions(&graph).await;
    let first = sessions[0].1.investigate(RACE_CLAIM).await.expect("first");
    let barrier = Arc::new(tokio::sync::Barrier::new(MEMORY_RACERS));
    let handles: Vec<_> = (0..MEMORY_RACERS)
        .map(|n| {
            let (investigator, barrier) = (sessions[n % 2].1.clone(), barrier.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                investigator.investigate(RACE_CLAIM).await
            })
        })
        .collect();
    let mut dossiers = Vec::new();
    for handle in handles {
        dossiers.push(handle.await.expect("task").expect("dossier"));
    }
    assert_linear_chain(&graph, MEMORY_RACERS + 1, "memory race").await;
    let seen: Vec<(usize, usize)> = dossiers
        .iter()
        .filter_map(|d| d.memory.as_ref())
        .map(|m| (m.prior_investigations, m.tool_calls_saved))
        .collect();
    println!("memory race (prior, saved): {seen:?}");

    let mut oldest_first = sessions[0]
        .0
        .investigation_history(RACE_CLAIM)
        .await
        .expect("history");
    oldest_first.reverse();
    let relied_on = |cert: &str| {
        let (graph, cert) = (&graph, cert.to_string());
        async move {
            read(
                graph,
                &format!(
                    "MATCH (:Investigation {{id: '{cert}'}})-[:RELIED_ON]->(c:Claim) RETURN c.id"
                ),
            )
            .await
            .into_iter()
            .map(|row| text(&row[0]))
            .collect::<HashSet<_>>()
        }
    };
    for dossier in &dossiers {
        let memory = dossier.memory.as_ref().expect("memory");
        let own = oldest_first
            .iter()
            .position(|r| r.certificate_id == dossier.certificate.certificate_id)
            .expect("own verdict recorded");
        let prior = memory.prior_investigations;
        assert!((1..=own).contains(&prior), "prior {prior}, own index {own}");
        assert_eq!(memory.latest_prior, oldest_first[prior - 1], "{memory:?}");
        assert!(memory.agrees_with_latest);
        assert_eq!(dossier.risk_score, first.risk_score);
        let skipped: HashSet<String> = memory.skipped_corroboration.iter().cloned().collect();
        assert!(
            skipped.is_subset(&relied_on(&memory.latest_prior.certificate_id).await),
            "{memory:?}"
        );
        let saved: usize = memory
            .skipped_corroboration
            .iter()
            .map(|id| calls_on(&first, id).len())
            .sum();
        assert_eq!(memory.tool_calls_saved, saved, "{memory:?}");
        assert_eq!(
            dossier.agent_trace.len(),
            first.agent_trace.len() - saved,
            "{memory:?}"
        );
    }
    drop(sessions);
    drop(graph);
}
