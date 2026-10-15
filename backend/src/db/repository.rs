//! FalkorDB-backed graph repository: connection, claim reads and claim writes.
//! Seeding lives in `seed.rs`, detectors and centrality in `detectors.rs`.

use super::mapping::{claim_summary, col, graph_edge, graph_node, node_key, opt_text, text};
use super::queries;
use crate::domain::{ClaimSummary, NewClaim, SubgraphPayload};
use chrono::Utc;
use falkordb::{
    AsyncGraph, Edge, FalkorClientBuilder, FalkorConnectionInfo, FalkorValue, Node, Row,
};
use futures_core::Stream;
use std::collections::HashSet;
use std::pin::Pin;
use std::time::Duration;
use tracing::info;

pub(crate) type Params<'a> = Vec<(&'a str, FalkorValue)>;

#[derive(Clone)]
pub struct FalkorGraphRepository {
    graph: AsyncGraph,
    graph_name: String,
}

impl FalkorGraphRepository {
    /// Connects and verifies the server answers a trivial query. No fallback of any kind.
    pub async fn connect(url: &str, graph: &str) -> Result<Self, String> {
        let info: FalkorConnectionInfo = url
            .try_into()
            .map_err(|e| format!("invalid FalkorDB URL '{url}': {e:?}"))?;
        let client = FalkorClientBuilder::new_async()
            .with_connection_info(info)
            .with_connect_timeout(Some(Duration::from_secs(3)))
            .build()
            .await
            .map_err(|e| format!("FalkorDB unreachable at {url}: {e}"))?;
        let repo = Self {
            graph: client.select_graph(graph),
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

    /// Executes one Cypher statement with parameters and collects every row, failing on the
    /// first row that cannot be parsed.
    pub(crate) async fn rows(&self, query: &str, params: Params<'_>) -> Result<Vec<Row>, String> {
        let mut graph = self.graph.clone();
        let result = graph
            .query(query)
            .with_params(params)
            .execute()
            .await
            .map_err(|e| format!("FalkorDB query failed: {e}"))?;
        let mut stream = result.data;
        let mut rows = Vec::with_capacity(stream.len());
        while let Some(row) = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await {
            rows.push(row.map_err(|e| format!("FalkorDB row parse failed: {e}"))?);
        }
        Ok(rows)
    }

    pub(crate) fn graph_handle(&self) -> AsyncGraph {
        self.graph.clone()
    }

    pub async fn list_claims(&self) -> Result<Vec<ClaimSummary>, String> {
        self.rows(&queries::list_claims(), vec![])
            .await?
            .iter()
            .map(claim_summary)
            .collect()
    }

    pub async fn get_claim(&self, claim_id: &str) -> Result<Option<ClaimSummary>, String> {
        let rows = self
            .rows(&queries::get_claim(), vec![("claim_id", text(claim_id))])
            .await?;
        rows.first().map(claim_summary).transpose()
    }

    pub async fn claim_shops(&self, claim_id: &str) -> Result<Vec<String>, String> {
        self.rows(queries::CLAIM_SHOPS, vec![("claim_id", text(claim_id))])
            .await?
            .iter()
            .map(|row| col(row, "shop_id"))
            .collect()
    }

    /// Deduplicated neighbourhood of a claim up to `depth` hops (clamped to 1..=4).
    pub async fn claim_subgraph(
        &self,
        claim_id: &str,
        depth: u8,
    ) -> Result<SubgraphPayload, String> {
        let params = || vec![("claim_id", text(claim_id))];
        let rows = self.rows(&queries::claim_subgraph(depth), params()).await?;
        let mut payload = SubgraphPayload::default();
        let mut seen_nodes = HashSet::new();
        let mut seen_edges = HashSet::new();

        if rows.is_empty() {
            let claim = self.rows(queries::CLAIM_NODE, params()).await?;
            let row = claim
                .first()
                .ok_or_else(|| format!("claim {claim_id} not found"))?;
            payload
                .nodes
                .push(graph_node(&col::<Node>(row, "node")?, claim_id)?);
            return Ok(payload);
        }

        for row in &rows {
            let src: Node = col(row, "src")?;
            let dst: Node = col(row, "dst")?;
            let rel: Edge = col(row, "rel")?;
            for node in [&src, &dst] {
                if seen_nodes.insert(node.entity_id) {
                    payload.nodes.push(graph_node(node, claim_id)?);
                }
            }
            if seen_edges.insert(rel.entity_id) {
                payload
                    .edges
                    .push(graph_edge(&rel, node_key(&src)?, node_key(&dst)?)?);
            }
        }
        Ok(payload)
    }

    /// Files a new claim, linking it to shared Phone/Address/VIN/Location/RepairShop nodes.
    pub async fn ingest_claim(&self, claim: NewClaim) -> Result<ClaimSummary, String> {
        claim.validate()?;
        let claim_id = match claim.claim_id.as_deref() {
            Some(id) => id.trim().to_string(),
            None => format!(
                "CLM-{}",
                uuid::Uuid::new_v4().simple().to_string()[..8].to_uppercase()
            ),
        };
        if self.get_claim(&claim_id).await?.is_some() {
            return Err(format!("claim {claim_id} already exists"));
        }
        let now = Utc::now();
        let filed_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (customer_id, vehicle_id, accident_id) = (
            format!("CUST-{claim_id}"),
            format!("VEH-{claim_id}"),
            format!("ACC-{claim_id}"),
        );
        let params: Params = vec![
            ("claim_id", text(&claim_id)),
            ("customer_id", text(&customer_id)),
            ("vehicle_id", text(&vehicle_id)),
            ("accident_id", text(&accident_id)),
            ("claimant_name", text(claim.claimant_name.trim())),
            ("phone", text(claim.phone.trim())),
            ("address", text(claim.address.trim())),
            ("vin", text(claim.vehicle_vin.trim())),
            ("location_id", text(claim.location_id.trim())),
            ("repair_shop_id", text(claim.repair_shop_id.trim())),
            (
                "clinic_id",
                opt_text(claim.clinic_id.as_deref().map(str::trim)),
            ),
            (
                "adjuster_id",
                opt_text(claim.adjuster_id.as_deref().map(str::trim)),
            ),
            ("amount", FalkorValue::F64(claim.amount)),
            ("police_report", FalkorValue::Bool(claim.police_report)),
            ("filed_at", text(&filed_at)),
            (
                "filed_day",
                FalkorValue::I64(now.timestamp().div_euclid(86_400)),
            ),
        ];
        let rows = self.rows(queries::INGEST_CLAIM, params).await?;
        let row = rows
            .first()
            .ok_or_else(|| format!("ingest of {claim_id} returned no row"))?;
        let summary = claim_summary(row)?;
        info!(
            "Ingested claim {} into graph '{}'",
            summary.claim_id, self.graph_name
        );
        Ok(summary)
    }

    pub async fn set_claim_status(&self, claim_id: &str, status: &str) -> Result<(), String> {
        if status.trim().is_empty() {
            return Err("status must not be empty".into());
        }
        let params = vec![("claim_id", text(claim_id)), ("status", text(status))];
        let rows = self.rows(queries::SET_CLAIM_STATUS, params).await?;
        if rows.is_empty() {
            return Err(format!("claim {claim_id} not found"));
        }
        Ok(())
    }

    /// Persists an Investigation node linked to the claim.
    pub async fn record_investigation(
        &self,
        claim_id: &str,
        decision: &str,
        risk_score: i32,
        certificate_id: &str,
    ) -> Result<(), String> {
        let params = vec![
            ("claim_id", text(claim_id)),
            ("decision", text(decision)),
            ("risk_score", FalkorValue::I64(i64::from(risk_score))),
            ("certificate_id", text(certificate_id)),
            ("recorded_at", text(&Utc::now().to_rfc3339())),
        ];
        let rows = self.rows(queries::RECORD_INVESTIGATION, params).await?;
        if rows.is_empty() {
            return Err(format!("claim {claim_id} not found"));
        }
        Ok(())
    }
}
