//! FalkorDB-backed graph repository: connection, the read-only evidence lane and the two
//! query entry points. Its methods are split by concern across `claims.rs`,
//! `investigations.rs`, `graph_algorithms.rs`, `detectors.rs`, `investigators.rs` and `seed.rs`.

use super::queries;
use falkordb::{
    AsyncGraph, FalkorClientBuilder, FalkorConnectionInfo, FalkorResult, FalkorValue, QueryResult,
    Row, RowStream,
};
use futures_core::Stream;
use std::pin::Pin;
use std::time::Duration;
use tracing::{info, warn};

pub(crate) type Params<'a> = Vec<(&'a str, FalkorValue)>;

/// Server-side cap (ms) on read-only queries; the slowest detector at 5k claims is far below it.
const READ_QUERY_TIMEOUT_MS: i64 = 5_000;

/// Drains a query result into rows, failing on the first row that cannot be parsed.
async fn collect(result: FalkorResult<QueryResult<RowStream>>) -> Result<Vec<Row>, String> {
    let mut stream = result
        .map_err(|e| format!("FalkorDB query failed: {e}"))?
        .data;
    let mut rows = Vec::with_capacity(stream.len());
    while let Some(row) = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await {
        rows.push(row.map_err(|e| format!("FalkorDB row parse failed: {e}"))?);
    }
    Ok(rows)
}

/// FalkorDB user the evidence lane logs in as when `with_read_only_lane` succeeds.
pub const READER_USER: &str = "diktiax_reader";
pub const LANE_READ_ONLY: &str = "read-only user";
pub const LANE_SHARED: &str = "shared client";

fn connection_info(url: &str) -> Result<FalkorConnectionInfo, String> {
    url.try_into()
        .map_err(|e| format!("invalid FalkorDB URL '{url}': {e:?}"))
}

async fn open_graph(info: FalkorConnectionInfo, graph: &str) -> Result<AsyncGraph, String> {
    let address = info.address();
    let client = FalkorClientBuilder::new_async()
        .with_connection_info(info)
        .with_connect_timeout(Some(Duration::from_secs(3)))
        .build()
        .await
        .map_err(|e| format!("FalkorDB unreachable at {address}: {e}"))?;
    Ok(client.select_graph(graph))
}

async fn redis_connection(
    info: redis::ConnectionInfo,
) -> Result<redis::aio::MultiplexedConnection, String> {
    redis::Client::open(info)
        .map_err(|e| e.to_string())?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| e.to_string())
}

#[derive(Clone)]
pub struct FalkorGraphRepository {
    graph: AsyncGraph,
    /// Client for `read_rows`: the read-only user's when that lane is up, else `graph`.
    reader: AsyncGraph,
    evidence_lane: &'static str,
    graph_name: String,
}

impl FalkorGraphRepository {
    /// Connects and verifies the server answers a trivial query. No fallback of any kind.
    pub async fn connect(url: &str, graph: &str) -> Result<Self, String> {
        let graph_handle = open_graph(connection_info(url)?, graph).await?;
        let repo = Self {
            reader: graph_handle.clone(),
            graph: graph_handle,
            evidence_lane: LANE_SHARED,
            graph_name: graph.to_string(),
        };
        repo.rows(queries::PING, vec![])
            .await
            .map_err(|e| format!("FalkorDB ping failed at {url}: {e}"))?;
        info!("Connected to FalkorDB at {url}, graph '{graph}'");
        Ok(repo)
    }

    pub fn graph_name(&self) -> &str {
        &self.graph_name
    }

    /// `LANE_READ_ONLY` or `LANE_SHARED`, for `/api/health`.
    pub fn evidence_lane(&self) -> &'static str {
        self.evidence_lane
    }

    /// Creates or updates `user` so it may run only GRAPH.RO_QUERY on this graph (plus PING and
    /// `INFO server` for the client handshake), then routes `read_rows` through a client logged
    /// in as that user. Any failure (no ACL support, bad credentials, timeout) keeps the shared
    /// client with a warning: the site must never go down over this.
    pub async fn with_read_only_lane(mut self, url: &str, user: &str, password: &str) -> Self {
        let attempt = tokio::time::timeout(
            Duration::from_secs(5),
            self.open_reader(url, user, password),
        )
        .await
        .unwrap_or_else(|_| Err("timed out".into()));
        match attempt {
            Ok(reader) => {
                self.reader = reader;
                self.evidence_lane = LANE_READ_ONLY;
                info!("Evidence lane: FalkorDB user '{user}' (GRAPH.RO_QUERY only)");
            }
            Err(e) => {
                warn!("Evidence lane: read-only user unavailable ({e}); using the shared client")
            }
        }
        self
    }

    async fn open_reader(
        &self,
        url: &str,
        user: &str,
        password: &str,
    ) -> Result<AsyncGraph, String> {
        let FalkorConnectionInfo::Redis(admin) = connection_info(url)?;
        // `>pw` adds a password without dropping others, so a second process (e.g. the MCP
        // server next to the HTTP server) with its own per-process secret keeps working.
        // `~graph`: this graph's key only (FalkorDB flags RO_QUERY's key spec as RW, so `%R~` would
        // deny it); `-@all` then GRAPH.RO_QUERY back, so the command list is what keeps it read-only.
        redis::cmd("ACL")
            .arg("SETUSER")
            .arg(user)
            .arg("on")
            .arg(format!(">{password}"))
            .arg("resetkeys")
            .arg(format!("~{}", self.graph_name))
            .arg("resetchannels")
            .arg("-@all")
            .arg("+graph.ro_query")
            .arg("+ping")
            // The falkordb client reads `INFO server` once at connect (sentinel detection).
            .arg("+info|server")
            .exec_async(&mut redis_connection(admin.clone()).await?)
            .await
            .map_err(|e| format!("ACL SETUSER failed: {e}"))?;
        let settings = admin
            .redis_settings()
            .clone()
            .set_username(user)
            .set_password(password);
        let reader_info = admin.set_redis_settings(settings);
        // Login check that does not need the graph to exist yet.
        redis::cmd("PING")
            .exec_async(&mut redis_connection(reader_info.clone()).await?)
            .await
            .map_err(|e| format!("reader login failed: {e}"))?;
        open_graph(FalkorConnectionInfo::Redis(reader_info), &self.graph_name).await
    }

    /// Executes one Cypher statement (GRAPH.QUERY) with parameters and collects every row,
    /// failing on the first row that cannot be parsed. Use for writes, and for statements that
    /// must work on a graph that does not exist yet (GRAPH.QUERY creates it, RO_QUERY errors).
    pub(crate) async fn rows(&self, query: &str, params: Params<'_>) -> Result<Vec<Row>, String> {
        let mut graph = self.graph.clone();
        collect(graph.query(query).with_params(params).execute().await).await
    }

    /// Single entry point for pure reads: GRAPH.RO_QUERY with a server-side timeout, on the
    /// read-only user's client when that lane is up. Return scalars, maps and lists only: a
    /// node or edge value makes the falkordb crate refresh its schema with GRAPH.QUERY, which
    /// the read-only user is denied.
    pub(crate) async fn read_rows(
        &self,
        query: &str,
        params: Params<'_>,
    ) -> Result<Vec<Row>, String> {
        let mut graph = self.reader.clone();
        collect(
            graph
                .ro_query(query)
                .with_params(params)
                .with_timeout(READ_QUERY_TIMEOUT_MS)
                .execute()
                .await,
        )
        .await
    }

    pub(crate) fn graph_handle(&self) -> AsyncGraph {
        self.graph.clone()
    }
}
