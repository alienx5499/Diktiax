#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClaimId(pub String);

impl ClaimId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudPatternMatch {
    pub pattern_name: String,
    pub severity: String,
    pub risk_points: i32,
    pub description: String,
    pub cypher_query: String,
    pub evidence_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceStep {
    pub step_number: usize,
    pub traversal_expression: String,
    pub cypher_source: String,
    pub nodes_implicated: Vec<String>,
    pub finding: String,
    pub risk_delta: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceCertificate {
    pub certificate_id: String,
    pub claim_id: String,
    pub generated_at: String,
    pub overall_decision: String,
    pub confidence_score: f64,
    pub final_risk_score: i32,
    pub execution_latency_ms: f64,
    pub topological_evidence: Vec<ProvenanceStep>,
    pub forensic_conclusion: String,
    pub cryptographic_digest: String,
}
