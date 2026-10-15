//! Investigation output contract (mirrors `frontend/src/lib/api.ts`).

use crate::actions::TriageRemediationResult;
use crate::domain::{FraudPatternMatch, ProvenanceCertificate, SubgraphPayload};
use serde::{Deserialize, Serialize};

/// One executed tool call as shown in the explainable decision path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub step: usize,
    pub tool: String,
    pub arguments: serde_json::Value,
    pub rationale: String,
    pub result_summary: String,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationDossier {
    pub claim_id: String,
    pub claim_amount: f64,
    /// Status written to the claim node by the action dispatcher.
    pub status: String,
    pub risk_score: i32,
    pub risk_tier: String,
    pub confidence: f64,
    pub summary_rationale: String,
    pub patterns_matched: Vec<FraudPatternMatch>,
    pub certificate: ProvenanceCertificate,
    pub remediation: TriageRemediationResult,
    pub subgraph: SubgraphPayload,
    pub agent_trace: Vec<AgentStep>,
    /// "claude:<model>" | "rule-based" | "rule-based (fallback: claude:<model> failed: <error>)"
    pub planner: String,
}
