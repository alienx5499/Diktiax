//! Investigation output contract (mirrors `frontend/src/lib/api.ts`).

use super::planner::PlannedCall;
use crate::actions::TriageRemediationResult;
use crate::domain::{
    FraudPatternMatch, InvestigationRecord, PriorVerdicts, ProvenanceCertificate, SubgraphPayload,
};
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
    /// "claude:<model>" | "rule-based" | "rule-based (fallback: LLM rate-limited | LLM timed out | LLM unavailable)"
    pub planner: String,
    /// Present only when the claim had been investigated before this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<InvestigationMemory>,
}

/// What this run took from earlier verdicts on the same claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestigationMemory {
    /// Verdicts recorded before this run started.
    pub prior_investigations: usize,
    pub latest_prior: InvestigationRecord,
    pub agrees_with_latest: bool,
    /// Linked claims not re-checked because the latest prior verdict already RELIED_ON them.
    pub skipped_corroboration: Vec<String>,
    pub tool_calls_saved: usize,
}

impl InvestigationMemory {
    /// `None` on a first investigation, so its dossier is unchanged.
    pub fn new(
        prior: Option<PriorVerdicts>,
        decision: &str,
        skipped: &[PlannedCall],
    ) -> Option<Self> {
        let prior = prior?;
        let mut claims: Vec<String> = Vec::new();
        for id in skipped.iter().filter_map(PlannedCall::claim_arg) {
            if !claims.iter().any(|c| c == id) {
                claims.push(id.to_string());
            }
        }
        Some(Self {
            prior_investigations: prior.count,
            agrees_with_latest: prior.latest.decision == decision,
            latest_prior: prior.latest,
            skipped_corroboration: claims,
            tool_calls_saved: skipped.len(),
        })
    }

    /// Rationale line, e.g. "Memory: 1 earlier verdict; latest CERT-… was CRITICAL_FRAUD; this run agrees."
    pub fn note(&self) -> String {
        let n = self.prior_investigations;
        let mut note = format!(
            "Memory: {n} earlier verdict{}; latest {} was {}; this run {}.",
            if n == 1 { "" } else { "s" },
            self.latest_prior.certificate_id,
            self.latest_prior.decision,
            if self.agrees_with_latest {
                "agrees"
            } else {
                "differs"
            },
        );
        if self.tool_calls_saved > 0 {
            note.push_str(&format!(
                " Skipped {} corroboration call(s) on {} (already relied on by {}).",
                self.tool_calls_saved,
                self.skipped_corroboration.join(", "),
                self.latest_prior.certificate_id
            ));
        }
        note
    }
}
