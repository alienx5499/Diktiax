//! Live FalkorDB tests for the read-only evidence lane (ACL user limited to GRAPH.RO_QUERY).
//!   docker compose up -d falkordb && cargo test --test read_only_lane -- --ignored
//! Each test uses its own graph and its own uniquely named ACL user, both deleted on drop.

mod common;

use claimgraph_backend::db::{FalkorGraphRepository, LANE_READ_ONLY, LANE_SHARED};
use common::TestGraph;
use falkordb::FalkorConnectionInfo;

fn redis_info(url: &str) -> redis::ConnectionInfo {
    let FalkorConnectionInfo::Redis(info) = url.try_into().expect("FalkorDB URL");
    info
}

/// Deletes the test ACL user on drop (never touches the default user, never ACL SAVE).
struct AclUser {
    url: String,
    name: String,
}

impl Drop for AclUser {
    fn drop(&mut self) {
        let deleted = redis::Client::open(redis_info(&self.url))
            .and_then(|c| c.get_connection())
            .and_then(|mut c| {
                redis::cmd("ACL")
                    .arg("DELUSER")
                    .arg(&self.name)
                    .exec(&mut c)
            });
        if let Err(e) = deleted {
            eprintln!("warning: failed to delete ACL user '{}': {e}", self.name);
        }
    }
}

async fn seeded() -> (FalkorGraphRepository, TestGraph) {
    let graph = TestGraph::new("claimgraph_acl");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB must be running for ignored integration tests");
    repo.seed(true).await.expect("seed");
    (repo, graph)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn reader_user_is_denied_writes_and_delete() {
    let (repo, graph) = seeded().await;
    let user = AclUser {
        url: graph.url.clone(),
        name: format!("diktiax_reader_it_{}", uuid::Uuid::new_v4().simple()),
    };
    let password = uuid::Uuid::new_v4().simple().to_string();
    let repo = repo
        .with_read_only_lane(&graph.url, &user.name, &password)
        .await;
    assert_eq!(repo.evidence_lane(), LANE_READ_ONLY);

    // Every evidence path still works through the reader, including the subgraph.
    assert_eq!(repo.list_claims().await.expect("list").len(), 8);
    let sub = repo.claim_subgraph("CLM-48291", 2).await.expect("subgraph");
    assert!(!sub.nodes.is_empty() && !sub.edges.is_empty());
    assert!(repo
        .detect_ghost_collision("CLM-48291")
        .await
        .expect("detector")
        .is_some());

    // Raw commands as the reader: RO_QUERY passes; writes, delete and other graphs are NOPERM.
    let info = redis_info(&graph.url);
    let settings = info
        .redis_settings()
        .clone()
        .set_username(&user.name)
        .set_password(&password);
    let mut reader = redis::Client::open(info.set_redis_settings(settings))
        .expect("client")
        .get_multiplexed_async_connection()
        .await
        .expect("reader login");
    let run = |args: &[&str]| {
        let mut cmd = redis::cmd(args[0]);
        cmd.arg(&args[1..]);
        cmd
    };
    let name = graph.name.as_str();
    run(&["GRAPH.RO_QUERY", name, "MATCH (c:Claim) RETURN count(c)"])
        .exec_async(&mut reader)
        .await
        .expect("GRAPH.RO_QUERY must succeed");
    for args in [
        &[
            "GRAPH.QUERY",
            name,
            "CREATE (:Claim {id: 'CLM-WRITE-PROBE'})",
        ][..],
        &["GRAPH.DELETE", name][..],
        &["GRAPH.RO_QUERY", "claimgraph", "RETURN 1"][..],
    ] {
        let err = run(args)
            .exec_async(&mut reader)
            .await
            .expect_err("must be denied");
        eprintln!("{} {} as reader -> {err}", args[0], args[1]);
        assert_eq!(err.code(), Some("NOPERM"), "{args:?}: {err}");
    }

    // Nothing changed, and the main client still writes.
    assert!(repo
        .get_claim("CLM-WRITE-PROBE")
        .await
        .expect("get")
        .is_none());
    assert_eq!(repo.list_claims().await.expect("list").len(), 8);
    repo.ping().await.expect("main client still works");
}

/// Every graph-algorithm query returns only scalars, maps and lists, so it runs as the reader.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn graph_algorithms_run_through_the_reader() {
    let (repo, graph) = seeded().await;
    let user = AclUser {
        url: graph.url.clone(),
        name: format!("diktiax_reader_it_{}", uuid::Uuid::new_v4().simple()),
    };
    let password = uuid::Uuid::new_v4().simple().to_string();
    let repo = repo
        .with_read_only_lane(&graph.url, &user.name, &password)
        .await;
    assert_eq!(repo.evidence_lane(), LANE_READ_ONLY);

    let influence = repo.shop_influence(50).await.expect("pageRank");
    assert_eq!(influence.len(), 6);
    assert_eq!(influence[0].id, "MED-201");
    let communities = repo.ring_communities(None).await.expect("labelPropagation");
    assert_eq!(communities.len(), 3);
    let ring = repo.ring_communities(Some("CLM-48291")).await.expect("one");
    assert_eq!(ring[0].size, 3);
    assert_eq!(repo.ring_component("CLM-48291").await.expect("WCC").size, 3);
    let path = repo.connection_path("CLM-48291", "CLM-39102").await;
    assert_eq!(path.expect("SPpaths").hops, Some(2));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn failed_acl_setup_falls_back_to_shared_client() {
    let (repo, graph) = seeded().await;
    // Redis rejects usernames with spaces, so ACL SETUSER fails: an injected setup failure.
    let repo = repo
        .with_read_only_lane(&graph.url, "not a valid user", "pw")
        .await;
    assert_eq!(repo.evidence_lane(), LANE_SHARED);
    assert_eq!(repo.list_claims().await.expect("list").len(), 8);

    // Unreachable admin endpoint: same fallback, reads keep working.
    let repo = repo
        .with_read_only_lane("falkor://127.0.0.1:1", "diktiax_reader_unused", "pw")
        .await;
    assert_eq!(repo.evidence_lane(), LANE_SHARED);
    assert!(!repo
        .claim_subgraph("CLM-48291", 1)
        .await
        .expect("subgraph")
        .nodes
        .is_empty());
}
