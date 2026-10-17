//! Write-back actions after scoring: claim status, Investigation record, SIU assignment chosen
//! from the graph, and an optional webhook. Graph write failures propagate.

use super::webhook::{Webhook, WebhookOutcome};
use crate::db::{FalkorGraphRepository, Investigator};
use crate::domain::evaluator::FraudRiskAssessment;
use crate::domain::{ClaimSummary, FraudPatternMatch, ProvenanceCertificate};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tracing::info;

const OPERATOR: &str = "Diktiax Autonomous Agent";

// One process-wide lock; the backend runs as a single instance. Multiple instances would need the check+assign in one Cypher statement.
static ASSIGN_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: String,
    pub claim_id: String,
    pub action_type: String,
    pub details: String,
    pub executed_at: String,
    pub status: String,
    pub operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageRemediationResult {
    pub claim_id: String,
    pub payout_frozen: bool,
    pub frozen_amount: f64,
    pub assigned_investigator: String,
    pub investigator_role: String,
    pub webhook_dispatched: bool,
    pub webhook_target: String,
    pub actions_log: Vec<ActionRecord>,
}

pub struct DispatchOutcome {
    /// Status written to the claim node.
    pub claim_status: String,
    pub remediation: TriageRemediationResult,
}

/// Tier for an investigation whose target-claim graph evidence is incomplete (tool errors).
pub const INCONCLUSIVE_TIER: &str = "INCONCLUSIVE";

/// Claim status by risk tier; `true` = escalate (freeze payout + SIU assignment + webhook).
/// Only an explicit `LOW_RISK` clears; `INCONCLUSIVE` or any unknown tier goes to manual review.
pub fn status_for(risk_tier: &str) -> (&'static str, bool) {
    match risk_tier {
        "CRITICAL_FRAUD" => ("FLAGGED_SIU_FRAUD", true),
        "HIGH_SUSPICION" => ("PAYOUT_FROZEN", true),
        "MEDIUM_RISK" => ("FLAGGED_AUDIT", false),
        "LOW_RISK" => ("CLEARED_AUTOMATED", false),
        _ => ("NEEDS_MANUAL_REVIEW", false),
    }
}

#[derive(Clone)]
pub struct ActionDispatcher {
    repo: FalkorGraphRepository,
    webhook: Webhook,
}

fn record(claim_id: &str, action_type: &str, details: String, status: &str) -> ActionRecord {
    ActionRecord {
        id: uuid::Uuid::new_v4().to_string(),
        claim_id: claim_id.to_string(),
        action_type: action_type.into(),
        details,
        executed_at: Utc::now().to_rfc3339(),
        status: status.into(),
        operator: OPERATOR.into(),
    }
}

impl ActionDispatcher {
    pub fn new(repo: FalkorGraphRepository, webhook: Webhook) -> Self {
        Self { repo, webhook }
    }

    /// The claim's investigator: the existing assignment if there is one, else pick + assign.
    /// `None` only when the graph holds no Investigator nodes.
    async fn assign_once(
        &self,
        id: &str,
        names: &[String],
    ) -> Result<Option<Investigator>, String> {
        let _guard = ASSIGN_LOCK.lock().await;
        if let Some(existing) = self.repo.claim_assignment(id).await? {
            return Ok(Some(existing));
        }
        match self.repo.pick_siu_investigator(names).await? {
            Some(pick) => self.repo.assign_investigator(id, &pick.id).await.map(Some),
            None => Ok(None),
        }
    }

    pub async fn dispatch(
        &self,
        claim: &ClaimSummary,
        assessment: &FraudRiskAssessment,
        patterns: &[FraudPatternMatch],
        certificate: &ProvenanceCertificate,
    ) -> Result<DispatchOutcome, String> {
        let id = claim.claim_id.as_str();
        let (status, escalate) = status_for(&assessment.risk_tier);
        let mut log = Vec::new();

        self.repo.set_claim_status(id, status).await?;
        log.push(record(
            id,
            "SET_CLAIM_STATUS",
            format!("Claim status {} -> {status}", claim.status),
            "CONFIRMED",
        ));
        self.repo
            .record_investigation(
                id,
                &assessment.risk_tier,
                assessment.risk_score,
                &certificate.certificate_id,
            )
            .await?;
        log.push(record(
            id,
            "RECORD_INVESTIGATION",
            format!(
                "Investigation {} ({}, {}/100) linked to claim",
                certificate.certificate_id, assessment.risk_tier, assessment.risk_score
            ),
            "CONFIRMED",
        ));

        let mut result = TriageRemediationResult {
            claim_id: id.to_string(),
            payout_frozen: false,
            frozen_amount: 0.0,
            assigned_investigator: "unassigned".into(),
            investigator_role: String::new(),
            webhook_dispatched: false,
            webhook_target: "skipped: claim not escalated".into(),
            actions_log: Vec::new(),
        };

        if escalate {
            result.payout_frozen = true;
            result.frozen_amount = claim.amount;
            log.push(record(
                id,
                "FREEZE_PAYOUT",
                format!(
                    "Payout of {:.2} frozen (claim status {status})",
                    claim.amount
                ),
                "CONFIRMED",
            ));
            info!(
                "[ACTION] {id}: payout {:.2} frozen, status {status}",
                claim.amount
            );

            let names: Vec<String> = patterns.iter().map(|p| p.pattern_name.clone()).collect();
            match self.assign_once(id, &names).await? {
                Some(assigned) => {
                    log.push(record(
                        id,
                        "ASSIGN_INVESTIGATOR",
                        format!(
                            "Assigned {} ({}) - specialties: {}; open cases now {}",
                            assigned.name,
                            assigned.id,
                            assigned.specialties.join(", "),
                            assigned.open_cases
                        ),
                        "CONFIRMED",
                    ));
                    info!("[ACTION] {id}: assigned investigator {}", assigned.id);
                    result.assigned_investigator = assigned.name;
                    result.investigator_role =
                        format!("SIU investigator ({})", assigned.specialties.join(", "));
                }
                None => log.push(record(
                    id,
                    "ASSIGN_INVESTIGATOR",
                    "No Investigator nodes in the graph".into(),
                    "FAILED",
                )),
            }

            let payload = json!({
                "claim_id": id,
                "status": status,
                "risk_score": assessment.risk_score,
                "risk_tier": assessment.risk_tier,
                "certificate_id": certificate.certificate_id,
                "cryptographic_digest": certificate.cryptographic_digest,
                "patterns": names,
                "assigned_investigator": result.assigned_investigator,
            });
            let (dispatched, target, outcome) = match self.webhook.send(&payload).await {
                WebhookOutcome::Skipped(reason) => {
                    (false, format!("skipped: {reason}"), "SKIPPED".to_string())
                }
                WebhookOutcome::Delivered { target, status } => {
                    (true, target, format!("HTTP {status}"))
                }
                WebhookOutcome::Failed { target, error } => {
                    (false, target, format!("FAILED: {error}"))
                }
            };
            let details = if dispatched || outcome.starts_with("FAILED") {
                format!("Evidence summary POSTed to {target}")
            } else {
                target.clone()
            };
            log.push(record(id, "DISPATCH_WEBHOOK", details, &outcome));
            result.webhook_dispatched = dispatched;
            result.webhook_target = target;
        }

        result.actions_log = log;
        Ok(DispatchOutcome {
            claim_status: status.to_string(),
            remediation: result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_low_risk_clears() {
        assert_eq!(status_for("LOW_RISK"), ("CLEARED_AUTOMATED", false));
        assert_eq!(
            status_for(INCONCLUSIVE_TIER),
            ("NEEDS_MANUAL_REVIEW", false)
        );
        assert_eq!(status_for("garbage"), ("NEEDS_MANUAL_REVIEW", false));
        assert_eq!(status_for("CRITICAL_FRAUD"), ("FLAGGED_SIU_FRAUD", true));
    }
}
