use serde::{Deserialize, Serialize};

/// Node of a claim neighbourhood, shaped for the frontend graph canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub title: String,
    pub group: String,
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubgraphPayload {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Degree centrality of one repair shop relative to the mean claim degree of all shops.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CentralityReport {
    pub shop_id: String,
    pub shop_name: String,
    /// Number of claims repaired at this shop.
    pub degree: i64,
    /// Mean claim degree across all shops (including this one).
    pub baseline: f64,
    pub shop_count: usize,
    /// degree / baseline; 0.0 when the baseline is 0.
    pub score: f64,
    pub is_anomalous: bool,
}

impl CentralityReport {
    /// Score at or above which a shop's claim concentration is anomalous.
    pub const ANOMALY_THRESHOLD: f64 = 1.5;

    /// Pure computation from the shop's degree and the degrees of every shop in the graph.
    pub fn compute(shop_id: String, shop_name: String, degree: i64, all_degrees: &[i64]) -> Self {
        let shop_count = all_degrees.len();
        let baseline = if shop_count == 0 {
            0.0
        } else {
            all_degrees.iter().sum::<i64>() as f64 / shop_count as f64
        };
        let score = if baseline > 0.0 {
            degree as f64 / baseline
        } else {
            0.0
        };
        Self {
            shop_id,
            shop_name,
            degree,
            baseline,
            shop_count,
            score,
            is_anomalous: score >= Self::ANOMALY_THRESHOLD,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_is_degree_over_mean() {
        let r = CentralityReport::compute("S1".into(), "Shop".into(), 3, &[3, 1, 1, 2]);
        assert_eq!(r.shop_count, 4);
        assert!((r.baseline - 1.75).abs() < 1e-9);
        assert!((r.score - 3.0 / 1.75).abs() < 1e-9);
        assert!(r.is_anomalous);
    }

    #[test]
    fn zero_baseline_yields_zero_score() {
        let r = CentralityReport::compute("S1".into(), "Shop".into(), 0, &[0, 0]);
        assert_eq!(r.score, 0.0);
        assert!(!r.is_anomalous);
        let empty = CentralityReport::compute("S1".into(), "Shop".into(), 0, &[]);
        assert_eq!(empty.baseline, 0.0);
    }
}
