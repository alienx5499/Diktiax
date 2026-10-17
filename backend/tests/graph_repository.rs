//! Live FalkorDB integration test. Requires a running server:
//!   docker compose up -d falkordb && cargo test -- --ignored
//! Uses FALKORDB_URL (default falkor://127.0.0.1:6379) and a dedicated test graph.

mod common;

use claimgraph_backend::db::FalkorGraphRepository;
use claimgraph_backend::domain::NewClaim;
use common::TestGraph;
use falkordb::{FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use futures_core::Stream;
use std::pin::Pin;

const RING_PHONE: &str = "+1-555-019-8821"; // shared by CUST-1001 / CUST-1002
const RING_LOCATION: &str = "LOC-441";
const RING_SHOP: &str = "SHOP-801";

async fn repo() -> (FalkorGraphRepository, TestGraph) {
    let graph = TestGraph::new("claimgraph_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    (repo, graph)
}

#[tokio::test]
async fn connect_fails_loudly_when_unreachable() {
    let err = FalkorGraphRepository::connect("falkor://127.0.0.1:1", "x").await;
    assert!(err.is_err(), "connecting to a closed port must fail");
}

// falkordb parses node/edge schema ids via block_in_place, which needs a multi-thread runtime.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn graph_repository_end_to_end() {
    let (repo, _graph) = repo().await;

    // Seeding is idempotent with reset.
    let first = repo.seed(true).await.expect("seed");
    let second = repo.seed(true).await.expect("reseed");
    assert!(first.applied && first.nodes > 0 && first.edges > 0);
    assert_eq!((first.nodes, first.edges), (second.nodes, second.edges));
    let skipped = repo.seed(false).await.expect("seed without reset");
    assert!(!skipped.applied);
    assert_eq!(skipped.nodes, first.nodes);

    let claims = repo.list_claims().await.expect("list");
    assert_eq!(claims.len(), 8);
    let target = repo
        .get_claim("CLM-48291")
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(target.claimant_name, "Victor Vance");
    assert_eq!(target.amount, 42500.0);
    assert!(repo.get_claim("CLM-NOPE").await.expect("get").is_none());

    // Ring 1: ghost collision + shared phone + kickback triangle.
    let ghost = repo
        .detect_ghost_collision("CLM-48291")
        .await
        .unwrap()
        .expect("ghost ring");
    assert_eq!(ghost.pattern_name, "Ghost Collision Ring");
    assert_eq!(ghost.severity, "CRITICAL");
    for id in [
        "CLM-48291",
        RING_LOCATION,
        RING_SHOP,
        "CLM-39102",
        "CLM-44730",
    ] {
        assert!(
            ghost.evidence_path.contains(&id.to_string()),
            "ghost evidence missing {id}"
        );
    }
    let contacts = repo
        .detect_shared_contacts("CLM-48291")
        .await
        .unwrap()
        .expect("shared phone");
    assert!(contacts.evidence_path.contains(&"CUST-1002".to_string()));
    assert!(contacts.evidence_path.contains(&RING_PHONE.to_string()));
    let kickback = repo
        .detect_kickback_triangle("CLM-48291")
        .await
        .unwrap()
        .expect("triangle");
    for id in ["ADJ-04", RING_SHOP, "MED-201", "CLM-39102"] {
        assert!(
            kickback.evidence_path.contains(&id.to_string()),
            "kickback evidence missing {id}"
        );
    }
    assert!(repo
        .detect_vin_cloning("CLM-48291")
        .await
        .unwrap()
        .is_none());

    // Ring 2: cross-state VIN clone.
    let vin = repo
        .detect_vin_cloning("CLM-55104")
        .await
        .unwrap()
        .expect("vin clone");
    assert_eq!(vin.severity, "CRITICAL");
    for id in ["1FTFW1ED4MFA88102", "VEH-88102-NV", "CLM-31205"] {
        assert!(
            vin.evidence_path.contains(&id.to_string()),
            "vin evidence missing {id}"
        );
    }

    // Clean claims trigger nothing.
    for clean in ["CLM-10022", "CLM-20417", "CLM-20988"] {
        assert!(
            repo.detect_ghost_collision(clean).await.unwrap().is_none(),
            "{clean} ghost"
        );
        assert!(
            repo.detect_shared_contacts(clean).await.unwrap().is_none(),
            "{clean} contacts"
        );
        assert!(
            repo.detect_vin_cloning(clean).await.unwrap().is_none(),
            "{clean} vin"
        );
        assert!(
            repo.detect_kickback_triangle(clean)
                .await
                .unwrap()
                .is_none(),
            "{clean} kickback"
        );
    }

    // Subgraph is real, deduplicated and depth-clamped.
    let sub = repo.claim_subgraph("CLM-48291", 2).await.expect("subgraph");
    let target_node = sub
        .nodes
        .iter()
        .find(|n| n.id == "CLM-48291")
        .expect("target node");
    assert_eq!(target_node.group, "target_claim");
    let mut ids: Vec<_> = sub.nodes.iter().map(|n| n.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), sub.nodes.len(), "nodes deduplicated");
    assert!(sub
        .edges
        .iter()
        .any(|e| e.label == "REPAIRED_AT" && e.to == RING_SHOP));
    let clamped = repo
        .claim_subgraph("CLM-48291", 200)
        .await
        .expect("clamped");
    let four = repo.claim_subgraph("CLM-48291", 4).await.expect("depth 4");
    assert_eq!(clamped.nodes.len(), four.nodes.len());
    assert!(repo.claim_subgraph("CLM-NOPE", 2).await.is_err());

    // Centrality before ingest.
    assert_eq!(
        repo.claim_shops("CLM-48291").await.unwrap(),
        vec![RING_SHOP.to_string()]
    );
    let before = repo.shop_centrality(RING_SHOP).await.expect("centrality");
    assert!(before.degree as f64 > before.baseline);

    // Validation rejects bad input.
    let bad = NewClaim {
        amount: 0.0,
        ..new_claim()
    };
    assert!(repo.ingest_claim(bad).await.is_err());

    // A brand-new, unscripted claim reusing the ring's burner phone, location and shop.
    let created = repo.ingest_claim(new_claim()).await.expect("ingest");
    assert!(created.claim_id.starts_with("CLM-"));
    assert_eq!(created.status, "SUBMITTED");
    let fresh = repo
        .detect_shared_contacts(&created.claim_id)
        .await
        .unwrap()
        .expect("detected");
    assert!(
        fresh.evidence_path.contains(&"CUST-1001".to_string()),
        "{:?}",
        fresh.evidence_path
    );
    let after = repo.shop_centrality(RING_SHOP).await.expect("centrality");
    assert_eq!(after.degree, before.degree + 1);
    assert!(after.degree as f64 > after.baseline && after.is_anomalous);

    // Commands write to the graph and fail loudly on missing claims.
    repo.set_claim_status(&created.claim_id, "FLAGGED_SIU")
        .await
        .expect("status");
    let updated = repo
        .get_claim(&created.claim_id)
        .await
        .unwrap()
        .expect("exists");
    assert_eq!(updated.status, "FLAGGED_SIU");
    repo.record_investigation(&created.claim_id, "CRITICAL_FRAUD", 95, "CERT-IT-1")
        .await
        .expect("record");
    assert!(repo.set_claim_status("CLM-NOPE", "X").await.is_err());
    assert!(repo
        .record_investigation("CLM-NOPE", "X", 1, "CERT-X")
        .await
        .is_err());
    let dup = NewClaim {
        claim_id: Some(created.claim_id.clone()),
        ..new_claim()
    };
    assert!(
        repo.ingest_claim(dup).await.is_err(),
        "duplicate claim id rejected"
    );
}

/// Single integer from a read-only query, through a client separate from the repository.
async fn count(graph: &TestGraph, query: &str) -> i64 {
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
        .expect("count");
    let row = std::future::poll_fn(|cx| Pin::new(&mut res.data).poll_next(cx))
        .await
        .expect("row")
        .expect("parse");
    match row.get_at(0) {
        Some(&FalkorValue::I64(n)) => n,
        other => panic!("unexpected count {other:?} for {query}"),
    }
}

/// (nodes, relationships), independent of `SeedReport`.
async fn graph_counts(graph: &TestGraph) -> (i64, i64) {
    (
        count(graph, "MATCH (n) RETURN count(n)").await,
        count(graph, "MATCH ()-[r]->() RETURN count(r)").await,
    )
}

/// Reset-and-reseed yields the same graph: the seed is one CREATE after deleting the graph,
/// so this proves determinism, not MERGE-style idempotence.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn reseed_is_deterministic() {
    let (repo, graph) = repo().await;
    repo.seed(true).await.expect("seed");
    let first = graph_counts(&graph).await;
    repo.seed(true).await.expect("reseed");
    let second = graph_counts(&graph).await;
    assert_eq!(first, second, "reseeding must not change the graph");
    assert_eq!(first, (68, 84), "README: 68 nodes, 84 relationships");
    // graph is deleted when `graph` drops
}

/// Ingest MERGEs contact nodes: two new customers sharing a phone and address share one node each.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn ingest_merges_shared_contacts() {
    const PHONE: &str = "+1-555-777-0142"; // not in data/seed.cypher
    const ADDRESS: &str = "77 Shared Lane, Fremont, CA";
    let (repo, graph) = repo().await;
    repo.seed(true).await.expect("seed");
    let claim = |name: &str, vin: &str| NewClaim {
        claimant_name: name.into(),
        phone: PHONE.into(),
        address: ADDRESS.into(),
        vehicle_vin: vin.into(),
        ..new_claim()
    };
    repo.ingest_claim(claim("First Sharer", "2C3CDXCT0EH000011"))
        .await
        .expect("ingest 1");
    repo.ingest_claim(claim("Second Sharer", "2C3CDXCT0EH000012"))
        .await
        .expect("ingest 2");

    let phones = format!("MATCH (p:Phone {{number: '{PHONE}'}}) RETURN count(p)");
    let addrs = format!("MATCH (a:Address {{line: '{ADDRESS}'}}) RETURN count(a)");
    let phone_edges =
        format!("MATCH (:Customer)-[r:HAS_PHONE]->(:Phone {{number: '{PHONE}'}}) RETURN count(r)");
    let addr_edges =
        format!("MATCH (:Customer)-[r:LIVES_AT]->(:Address {{line: '{ADDRESS}'}}) RETURN count(r)");
    assert_eq!(count(&graph, &phones).await, 1, "one Phone node");
    assert_eq!(count(&graph, &addrs).await, 1, "one Address node");
    assert_eq!(count(&graph, &phone_edges).await, 2, "2 HAS_PHONE edges");
    assert_eq!(count(&graph, &addr_edges).await, 2, "2 LIVES_AT edges");

    // A third claim reusing the phone still lands on the same node.
    repo.ingest_claim(claim("First Sharer", "2C3CDXCT0EH000011"))
        .await
        .expect("ingest 3");
    assert_eq!(count(&graph, &phones).await, 1, "Phone still merged");
    assert_eq!(count(&graph, &phone_edges).await, 3);
}

fn new_claim() -> NewClaim {
    NewClaim {
        claim_id: None,
        claimant_name: "Unscripted Demo Claimant".into(),
        phone: RING_PHONE.into(),
        address: "1 Nowhere Rd, Oakland, CA".into(),
        vehicle_vin: "2C3CDXCT0EH000001".into(),
        location_id: RING_LOCATION.into(),
        repair_shop_id: RING_SHOP.into(),
        clinic_id: None,
        adjuster_id: None,
        amount: 27500.0,
        police_report: false,
    }
}
