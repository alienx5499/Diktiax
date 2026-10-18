//! FalkorDB built-in algorithms (v4.22.1) cross-check what the detectors find.
//! Live tests: `cargo test --test graph_algorithms -- --ignored`.

mod common;

use common::TestGraph;
use diktiax_backend::db::FalkorGraphRepository;
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
    let graph = TestGraph::new("diktiax_algo");
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

/// The live `ring_components` tool (same WCC projection, read-only path) returns the ring.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn ring_components_tool_returns_the_ghost_ring() {
    let graph = TestGraph::new("diktiax_ring_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    repo.seed(true).await.expect("seed");

    let result = diktiax_backend::tools::execute_external_tool(
        &repo,
        "ring_components",
        &serde_json::json!({ "claim_id": "CLM-48291" }),
    )
    .await
    .expect("ring_components");
    assert_eq!(
        result,
        serde_json::json!({
            "claim_id": "CLM-48291",
            "claims": ["CLM-39102", "CLM-44730", "CLM-48291"],
            "size": 3,
        })
    );

    let missing = repo.ring_component("CLM-NOPE").await.expect_err("unknown");
    assert!(missing.contains("not found"), "{missing}");

    drop(repo);
    drop(graph);
}

/// `connection_path` (algo.SPpaths): the ring's claims link in 2 hops through the shared shop and
/// the shared clinic (the shared phone is 4 hops, so not a shortest path); a clean control claim
/// is not connected within 6 hops; unknown ids are errors.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn connection_path_links_ring_claims_only() {
    let graph = TestGraph::new("diktiax_path_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    repo.seed(true).await.expect("seed");
    let call = |a: &'static str, b: &'static str| {
        let repo = repo.clone();
        async move {
            let args = serde_json::json!({ "claim_a": a, "claim_b": b });
            diktiax_backend::tools::execute_external_tool(&repo, "connection_path", &args).await
        }
    };

    let hop = |label: &str, id: &str| serde_json::json!({ "label": label, "id": id });
    let via = |label: &str, id: &str, rel: &str| {
        serde_json::json!({
            "nodes": [hop("Claim", "CLM-48291"), hop(label, id), hop("Claim", "CLM-39102")],
            "relationships": [rel, rel],
        })
    };
    assert_eq!(
        call("CLM-48291", "CLM-39102").await.expect("linked"),
        serde_json::json!({
            "claim_a": "CLM-48291",
            "claim_b": "CLM-39102",
            "max_hops": 6,
            "hops": 2,
            "paths": [
                via("Clinic", "MED-201", "TREATED_AT"),
                via("RepairShop", "SHOP-801", "REPAIRED_AT"),
            ],
        })
    );
    assert_eq!(
        call("CLM-48291", "CLM-10022").await.expect("unlinked"),
        serde_json::json!({
            "claim_a": "CLM-48291",
            "claim_b": "CLM-10022",
            "max_hops": 6,
            "hops": null,
            "paths": [],
        })
    );
    let missing = call("CLM-48291", "CLM-NOPE").await.expect_err("unknown");
    assert!(missing.contains("not found"), "{missing}");

    drop(repo);
    drop(graph);
}

/// `shop_influence` (algo.pageRank) on the seed graph: the ring's clinic MED-201 ranks first
/// (it also receives the shop's COLLUDES_WITH referral edge), the ring's shop SHOP-801 is the
/// top repair shop.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn shop_influence_ranks_the_ring_providers_first() {
    let graph = TestGraph::new("diktiax_influence_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    repo.seed(true).await.expect("seed");

    let ranked = diktiax_backend::tools::execute_external_tool(
        &repo,
        "shop_influence",
        &serde_json::json!({ "limit": 3 }),
    )
    .await
    .expect("shop_influence");
    let order: Vec<(&str, &str, i64)> = ranked
        .as_array()
        .expect("array")
        .iter()
        .map(|r| {
            (
                r["label"].as_str().unwrap(),
                r["id"].as_str().unwrap(),
                r["claims"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("Clinic", "MED-201", 3),
            ("RepairShop", "SHOP-801", 3),
            ("RepairShop", "SHOP-220", 2),
        ]
    );
    assert!(ranked[1]["score"].as_f64() > ranked[2]["score"].as_f64());

    let all = repo.shop_influence(50).await.expect("all");
    assert_eq!(all.len(), 6, "4 shops and 2 clinics");

    drop(repo);
    drop(graph);
}

/// `ring_communities` (algo.labelPropagation, RING_COMPONENT projection): on the seed graph the
/// communities match the WCC components exactly, so the ghost ring is its own community and the
/// two clean pairs are communities too. A claim in no shared community still gets its own.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn ring_communities_separate_the_ring() {
    let graph = TestGraph::new("diktiax_lpa_it");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    repo.seed(true).await.expect("seed");
    let call = |args: serde_json::Value| {
        let repo = repo.clone();
        async move {
            diktiax_backend::tools::execute_external_tool(&repo, "ring_communities", &args).await
        }
    };
    // Community ids are FalkorDB-internal, so compare everything else.
    let shape = |v: serde_json::Value| -> Vec<(Vec<String>, u64, Vec<String>)> {
        v.as_array()
            .expect("array")
            .iter()
            .map(|c| {
                assert!(c["community_id"].is_i64());
                let ids = |key: &str, field: &str| -> Vec<String> {
                    c[key]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.get(field).unwrap_or(x).as_str().unwrap().to_string())
                        .collect()
                };
                (
                    ids("claims", ""),
                    c["size"].as_u64().unwrap(),
                    ids("providers", "id"),
                )
            })
            .collect()
    };
    let s = |v: &str| v.split(' ').map(String::from).collect::<Vec<_>>();
    let ring = (s("CLM-39102 CLM-44730 CLM-48291"), 3, s("MED-201 SHOP-801"));

    assert_eq!(
        shape(call(serde_json::json!({})).await.expect("all")),
        [
            (s("CLM-20417 CLM-20988"), 2, s("MED-100 SHOP-220")),
            (s("CLM-31205 CLM-55104"), 2, s("SHOP-505")),
            ring.clone(),
        ]
    );
    let one = call(serde_json::json!({ "claim_id": "CLM-48291" })).await;
    assert_eq!(shape(one.expect("ring")), [ring]);
    let alone = call(serde_json::json!({ "claim_id": "CLM-10022" })).await;
    assert_eq!(
        shape(alone.expect("single")),
        [(s("CLM-10022"), 1, s("SHOP-100"))]
    );
    let missing = call(serde_json::json!({ "claim_id": "CLM-NOPE" })).await;
    assert!(missing.expect_err("unknown").contains("not found"));
    let bad = call(serde_json::json!({ "claim_id": 7 })).await;
    assert!(bad.expect_err("non-string").contains("argument 'claim_id'"));

    drop(repo);
    drop(graph);
}
