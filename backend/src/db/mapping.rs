//! FalkorDB row/value -> domain entity conversion. Every conversion error is propagated.

use crate::domain::{
    ChainedVerdict, ClaimSummary, EvidenceReuse, GraphEdge, GraphNode, InvestigationRecord,
};
use falkordb::{Edge, FalkorValue, FromFalkorValue, Node, Row};
use std::collections::HashMap;

/// Strictly typed column read with the column name in the error.
pub fn col<T: FromFalkorValue>(row: &Row, column: &str) -> Result<T, String> {
    row.try_get::<T>(column)
        .map_err(|e| format!("column '{column}': {e}"))
}

pub fn text(value: &str) -> FalkorValue {
    FalkorValue::String(value.to_string())
}

pub fn opt_text(value: Option<&str>) -> FalkorValue {
    value.map_or(FalkorValue::None, text)
}

pub fn claim_summary(row: &Row) -> Result<ClaimSummary, String> {
    Ok(ClaimSummary {
        claim_id: col(row, "claim_id")?,
        claimant_name: col(row, "claimant_name")?,
        amount: col(row, "amount")?,
        status: col(row, "status")?,
        filed_at: col(row, "filed_at")?,
    })
}

/// `None` for the all-null row PRIOR_VERDICTS or INVESTIGATION_HISTORY returns for a claim with
/// no investigations.
pub fn investigation_record(row: &Row) -> Result<Option<InvestigationRecord>, String> {
    let Some(certificate_id) = col::<Option<String>>(row, "certificate_id")? else {
        return Ok(None);
    };
    Ok(Some(InvestigationRecord {
        certificate_id,
        decision: col(row, "decision")?,
        risk_score: col(row, "risk_score")?,
        recorded_at: col(row, "recorded_at")?,
    }))
}

/// One INVESTIGATION_HISTORY row; `None` for the all-null row of a never-investigated claim.
pub fn chained_verdict(row: &Row, position: usize) -> Result<Option<ChainedVerdict>, String> {
    let Some(record) = investigation_record(row)? else {
        return Ok(None);
    };
    Ok(Some(ChainedVerdict {
        record,
        supersedes: col(row, "supersedes")?,
        relied_on: col(row, "relied_on")?,
        position,
    }))
}

/// One EVIDENCE_REUSE row; `None` for the all-null row of a claim nothing relied on.
pub fn evidence_reuse(row: &Row) -> Result<Option<EvidenceReuse>, String> {
    let Some(certificate_id) = col::<Option<String>>(row, "certificate_id")? else {
        return Ok(None);
    };
    Ok(Some(EvidenceReuse {
        claim_id: col(row, "claim_id")?,
        certificate_id,
        decision: col(row, "decision")?,
        recorded_at: col(row, "recorded_at")?,
        is_head: col(row, "is_head")?,
    }))
}

/// Node rebuilt from `<prefix>_id`, `<prefix>_labels`, `<prefix>_props` columns (see
/// `queries::claim_subgraph`), so the read-only lane never has to parse a node value.
pub fn node_columns(row: &Row, prefix: &str) -> Result<Node, String> {
    Ok(Node {
        entity_id: col(row, &format!("{prefix}_id"))?,
        labels: col(row, &format!("{prefix}_labels"))?,
        properties: col(row, &format!("{prefix}_props"))?,
    })
}

/// Edge rebuilt from `<prefix>_id`, `<prefix>_type`, `<prefix>_props` columns.
pub fn edge_columns(row: &Row, prefix: &str, src: i64, dst: i64) -> Result<Edge, String> {
    Ok(Edge {
        entity_id: col(row, &format!("{prefix}_id"))?,
        relationship_type: col(row, &format!("{prefix}_type"))?,
        src_node_id: src,
        dst_node_id: dst,
        properties: col(row, &format!("{prefix}_props"))?,
    })
}

/// Business key of a node: `id`, or the natural key of a shared contact/VIN node.
pub fn node_key(node: &Node) -> Result<String, String> {
    for key in ["id", "number", "line", "vin"] {
        if let Some(value) = node.properties.get(key) {
            return String::from_falkor_value(value.clone())
                .map_err(|e| format!("node property '{key}': {e}"));
        }
    }
    Err(format!(
        "node {} ({:?}) has no id/number/line/vin property",
        node.entity_id, node.labels
    ))
}

pub fn graph_node(node: &Node, target_claim_id: &str) -> Result<GraphNode, String> {
    let id = node_key(node)?;
    let label = node
        .labels
        .first()
        .cloned()
        .ok_or_else(|| format!("node {id} has no label"))?;
    let group = match label.as_str() {
        "Claim" if id == target_claim_id => "target_claim".to_string(),
        "Claim" => "linked_claim".to_string(),
        other => snake_case(other),
    };
    let title = ["name", "line", "number", "vin"]
        .iter()
        .find_map(|k| match node.properties.get(*k) {
            Some(FalkorValue::String(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| id.clone());
    Ok(GraphNode {
        id,
        label,
        title,
        group,
        properties: properties_json(&node.properties)?,
    })
}

pub fn graph_edge(edge: &Edge, from: String, to: String) -> Result<GraphEdge, String> {
    Ok(GraphEdge {
        id: format!("e{}", edge.entity_id),
        from,
        to,
        label: edge.relationship_type.clone(),
        properties: properties_json(&edge.properties)?,
    })
}

fn properties_json(props: &HashMap<String, FalkorValue>) -> Result<serde_json::Value, String> {
    props
        .iter()
        .map(|(k, v)| Ok((k.clone(), value_json(v)?)))
        .collect::<Result<serde_json::Map<_, _>, String>>()
        .map(serde_json::Value::Object)
}

fn value_json(value: &FalkorValue) -> Result<serde_json::Value, String> {
    use serde_json::Value as J;
    Ok(match value {
        FalkorValue::None => J::Null,
        FalkorValue::Bool(b) => J::Bool(*b),
        FalkorValue::I64(i) => J::from(*i),
        FalkorValue::F64(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .ok_or_else(|| format!("non-finite float property {f}"))?,
        FalkorValue::String(s) => J::String(s.clone()),
        FalkorValue::Array(items) => {
            J::Array(items.iter().map(value_json).collect::<Result<_, _>>()?)
        }
        FalkorValue::Map(map) => properties_json(map)?,
        other => return Err(format!("unsupported property value {other:?}")),
    })
}

/// `RepairShop` -> `repair_shop`, `VIN` -> `vin`.
fn snake_case(label: &str) -> String {
    let mut out = String::with_capacity(label.len() + 4);
    let mut prev_lower = false;
    for ch in label.chars() {
        if ch.is_uppercase() && prev_lower {
            out.push('_');
        }
        prev_lower = ch.is_lowercase();
        out.extend(ch.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_matches_frontend_groups() {
        assert_eq!(snake_case("RepairShop"), "repair_shop");
        assert_eq!(snake_case("Customer"), "customer");
        assert_eq!(snake_case("VIN"), "vin");
    }

    #[test]
    fn node_without_key_is_an_error() {
        let node = Node {
            entity_id: 7,
            labels: vec!["X".into()],
            properties: HashMap::new(),
        };
        assert!(node_key(&node).is_err());
    }

    #[test]
    fn claim_nodes_are_grouped_by_target() {
        let mut props = HashMap::new();
        props.insert("id".to_string(), text("CLM-1"));
        let node = Node {
            entity_id: 1,
            labels: vec!["Claim".into()],
            properties: props,
        };
        assert_eq!(
            graph_node(&node, "CLM-1").map(|n| n.group),
            Ok("target_claim".into())
        );
        assert_eq!(
            graph_node(&node, "CLM-2").map(|n| n.group),
            Ok("linked_claim".into())
        );
    }
}
