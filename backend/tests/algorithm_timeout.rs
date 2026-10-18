//! Does the read lane's server-side TIMEOUT stop the graph-algorithm queries? Runs each query
//! constant on a ~22k-node graph with a 1 ms timeout (the read lane uses 5 000 ms). Every one
//! fails with the server's timeout error (the client has no response timeout). FalkorDB checks
//! the deadline between operations, not inside a procedure: a single algo.* call runs to the
//! end before the error (labelPropagation errs after about its full run time).
//! Live test: `cargo test --test algorithm_timeout -- --ignored`.

mod common;

use common::TestGraph;
use diktiax_backend::db::queries;
use falkordb::{FalkorClientBuilder, FalkorConnectionInfo, FalkorValue};
use std::time::Instant;

/// 20 000 claims, each repaired at one of 2 000 shops and filed by one of 5 000 customers.
/// labelPropagation rejects a projection label or relationship type the graph lacks, so one of
/// each other projected kind exists too.
const BUILD: [&str; 5] = [
    "CREATE (:Phone {number: 'P'})<-[:HAS_PHONE]-(:Customer {id: 'X'})-[:LIVES_AT]->\
     (:Address {line: 'A'}), (:Claim {id: 'CX'})-[:TREATED_AT]->(:Clinic {id: 'M'})",
    "UNWIND range(1, 2000) AS i CREATE (:RepairShop {id: 'S' + toString(i)})",
    "UNWIND range(1, 5000) AS i CREATE (:Customer {id: 'U' + toString(i)})",
    "CREATE INDEX FOR (n:RepairShop) ON (n.id)",
    "CREATE INDEX FOR (n:Customer) ON (n.id)",
];
const LINK: &str = "UNWIND range(1, 20000) AS i \
     MATCH (s:RepairShop {id: 'S' + toString(i % 2000 + 1)}), \
     (u:Customer {id: 'U' + toString(i % 5000 + 1)}) \
     CREATE (u)-[:FILED_CLAIM]->(:Claim {id: 'C' + toString(i)})-[:REPAIRED_AT]->(s)";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn algorithm_queries_obey_the_read_timeout() {
    let graph = TestGraph::new("diktiax_timeout_it");
    let info: FalkorConnectionInfo = graph.url.as_str().try_into().expect("url");
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .build()
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    let mut g = client.select_graph(&graph.name);
    for q in BUILD.iter().chain([&LINK]) {
        g.query(q).execute().await.expect("build");
    }

    let s = |v: &str| FalkorValue::String(v.into());
    let cases = [
        ("WCC", queries::RING_COMPONENT, vec![("claim_id", s("C1"))]),
        // SPpaths is left out: bounded to 6 hops, it finishes in ~2 ms, too close to a 1 ms limit
        // to assert on reliably.
        (
            "pageRank",
            queries::SHOP_INFLUENCE,
            vec![("limit", FalkorValue::I64(10))],
        ),
        (
            "labelPropagation",
            queries::RING_COMMUNITIES,
            vec![("claim_id", s("C1"))],
        ),
    ];
    for (name, query, params) in cases {
        let run = |timeout: i64| {
            let (mut g, params) = (g.clone(), params.clone());
            async move {
                let start = Instant::now();
                let res = g
                    .ro_query(query)
                    .with_params(params)
                    .with_timeout(timeout)
                    .execute()
                    .await
                    .map(|_| ());
                (res, start.elapsed())
            }
        };
        let (full, full_time) = run(60_000).await;
        full.unwrap_or_else(|e| panic!("{name} without a tight timeout: {e}"));
        let (tight, tight_time) = run(1).await;
        let err = tight.expect_err(name).to_string();
        eprintln!("{name}: {full_time:?} untimed, 1 ms timeout -> '{err}' after {tight_time:?}");
        assert!(err.contains("timed out"), "{name}: {err}");
    }
}
