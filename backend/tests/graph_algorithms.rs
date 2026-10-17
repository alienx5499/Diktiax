//! FalkorDB built-in algorithms (v4.22.1) cross-check what the detectors find.
//! Live tests: `cargo test --test graph_algorithms -- --ignored`.

mod common;

use claimgraph_backend::db::FalkorGraphRepository;
use common::TestGraph;
use falkordb::{FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use futures_core::Stream;
use std::pin::Pin;

const WCC: &str =
    "CALL algo.WCC({nodeLabels: ['Customer','Claim','Phone','Address','RepairShop','Clinic'], \
     relationshipTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT']}) \
     YIELD node, componentId WITH node, componentId WHERE node:Claim \
     RETURN componentId, collect(node.id) AS claims";

// The labelled form ('RepairShop', 'REPAIRED_AT') scores every shop equally; whole-graph is needed.
const PAGERANK: &str = "CALL algo.pageRank(null, null) YIELD node, score \
     WITH node, score WHERE node:RepairShop RETURN node.id, score ORDER BY score DESC";

/// Seeds a fresh test graph and returns all rows of a read-only query against it.
async fn seeded_rows(query: &str) -> Vec<Vec<FalkorValue>> {
    let graph = TestGraph::new("claimgraph_algo");
    FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests")
        .seed(true)
        .await
        .expect("seed");
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

fn string(v: &FalkorValue) -> String {
    match v {
        FalkorValue::String(s) => s.clone(),
        other => panic!("expected string, got {other:?}"),
    }
}

/// WCC over the contact/claim edges puts the ghost ring's three claims in their own component.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn wcc_isolates_the_ghost_ring() {
    let ring = ["CLM-39102", "CLM-44730", "CLM-48291"];
    let components: Vec<Vec<String>> = seeded_rows(WCC)
        .await
        .iter()
        .map(|row| match &row[1] {
            FalkorValue::Array(ids) => {
                let mut ids: Vec<String> = ids.iter().map(string).collect();
                ids.sort();
                ids
            }
            other => panic!("expected list, got {other:?}"),
        })
        .collect();
    assert!(
        components.iter().any(|c| c == &ring),
        "no component is exactly the ring: {components:?}"
    );
}

/// Whole-graph PageRank ranks the ring's shop SHOP-801 above every other shop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn pagerank_ranks_the_ring_shop_first() {
    let rows = seeded_rows(PAGERANK).await;
    let score = |row: &Vec<FalkorValue>| match row[1] {
        FalkorValue::F64(s) => s,
        ref other => panic!("expected score, got {other:?}"),
    };
    assert_eq!(rows.len(), 4, "seed has 4 shops");
    assert_eq!(string(&rows[0][0]), "SHOP-801");
    assert!(
        score(&rows[0]) > score(&rows[1]),
        "SHOP-801 must lead strictly"
    );
}
