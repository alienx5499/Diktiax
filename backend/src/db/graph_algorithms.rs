//! Graph algorithms behind the scan tools: algo.WCC, algo.SPpaths, algo.pageRank and
//! algo.labelPropagation.

use super::mapping::{col, opt_text, text};
use super::queries;
use super::repository::FalkorGraphRepository;
use crate::domain::{
    ConnectionPath, InfluenceRank, PathNode, PathTrace, RingCommunity, RingComponent,
};
use falkordb::FalkorValue;

impl FalkorGraphRepository {
    /// The claim's weakly connected component (algo.WCC); errors if the claim is unknown.
    pub async fn ring_component(&self, claim_id: &str) -> Result<RingComponent, String> {
        let rows = self
            .read_rows(queries::RING_COMPONENT, vec![("claim_id", text(claim_id))])
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| format!("claim {claim_id} not found"))?;
        let mut claims: Vec<String> = col(row, "claims")?;
        claims.sort();
        Ok(RingComponent {
            claim_id: claim_id.to_string(),
            size: claims.len(),
            claims,
        })
    }

    /// Shortest paths (algo.SPpaths) between two claims, sorted; empty when not connected within
    /// `MAX_PATH_HOPS`. Errors if either claim is unknown.
    pub async fn connection_path(
        &self,
        claim_a: &str,
        claim_b: &str,
    ) -> Result<ConnectionPath, String> {
        let rows = self
            .read_rows(
                queries::CONNECTION_PATH,
                vec![("claim_a", text(claim_a)), ("claim_b", text(claim_b))],
            )
            .await?;
        if rows.is_empty() {
            let ids = FalkorValue::Array(vec![text(claim_a), text(claim_b)]);
            let found = self
                .read_rows(queries::COUNT_CLAIMS_BY_ID, vec![("claim_ids", ids)])
                .await?;
            let found: i64 = found
                .first()
                .map(|r| col(r, "found"))
                .transpose()?
                .unwrap_or(0);
            if found < 2 {
                return Err(format!("claim {claim_a} or {claim_b} not found"));
            }
        }
        let mut paths = rows
            .iter()
            .map(|row| {
                let labels: Vec<String> = col(row, "labels")?;
                let ids: Vec<String> = col(row, "ids")?;
                Ok(PathTrace {
                    nodes: labels
                        .into_iter()
                        .zip(ids)
                        .map(|(label, id)| PathNode { label, id })
                        .collect(),
                    relationships: col(row, "rels")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        // SPpaths returns ties in no fixed order.
        paths.sort_by(|a, b| {
            let key = |p: &PathTrace| p.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
            key(a).cmp(&key(b))
        });
        Ok(ConnectionPath {
            claim_a: claim_a.to_string(),
            claim_b: claim_b.to_string(),
            max_hops: queries::MAX_PATH_HOPS,
            hops: paths.first().map(|p| p.relationships.len()),
            paths,
        })
    }

    /// Repair shops and clinics by whole-graph algo.pageRank, highest first.
    pub async fn shop_influence(&self, limit: u8) -> Result<Vec<InfluenceRank>, String> {
        self.read_rows(
            queries::SHOP_INFLUENCE,
            vec![("limit", FalkorValue::I64(i64::from(limit)))],
        )
        .await?
        .iter()
        .map(|row| {
            Ok(InfluenceRank {
                label: col(row, "label")?,
                id: col(row, "id")?,
                name: col(row, "name")?,
                score: col(row, "score")?,
                claims: col(row, "claims")?,
            })
        })
        .collect()
    }

    /// algo.labelPropagation communities with at least 2 claims, or only `claim_id`'s community
    /// (whatever its size). Sorted by first claim id; errors if `claim_id` is unknown.
    pub async fn ring_communities(
        &self,
        claim_id: Option<&str>,
    ) -> Result<Vec<RingCommunity>, String> {
        let rows = self
            .read_rows(
                queries::RING_COMMUNITIES,
                vec![("claim_id", opt_text(claim_id))],
            )
            .await?;
        if let (Some(id), true) = (claim_id, rows.is_empty()) {
            return Err(format!("claim {id} not found"));
        }
        let mut communities = rows
            .iter()
            .map(|row| {
                let mut claims: Vec<String> = col(row, "claims")?;
                claims.sort();
                let mut providers = col::<Vec<Vec<String>>>(row, "providers")?
                    .into_iter()
                    .map(|pair| match <[String; 2]>::try_from(pair) {
                        Ok([label, id]) => Ok(PathNode { label, id }),
                        Err(pair) => Err(format!("provider entry {pair:?} is not [label, id]")),
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                providers.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(RingCommunity {
                    community_id: col(row, "community_id")?,
                    size: claims.len(),
                    claims,
                    providers,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        communities.sort_by(|a, b| a.claims.cmp(&b.claims));
        Ok(communities)
    }
}
