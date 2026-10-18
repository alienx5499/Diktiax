//! Agent loop: plan -> execute via MCP -> feed back, bounded by `MAX_STEPS`; then deterministic
//! scoring, provenance certificate, graph write-back actions and the dossier.

use super::claude_planner::{ClaudeConfig, ClaudePlanner};
use super::dossier::{InvestigationDossier, InvestigationMemory};
use super::evidence;
use super::fallback_planner::FallbackPlanner;
use super::openai_planner::{OpenAiConfig, OpenAiPlanner};
use super::planner::{
    repeated_ring_component, PlannedCall, Planner, ToolOutcome, DETECTORS, MAX_STEPS,
};
use super::rule_planner::RuleBasedPlanner;
use super::verdict;
use crate::actions::ActionDispatcher;
use crate::db::FalkorGraphRepository;
use crate::domain::{FraudRiskEvaluator, ProvenanceCertificate, ProvenanceDigestSigner};
use chrono::Utc;
use serde_json::json;
use std::time::Instant;
use tracing::info;

#[derive(Clone)]
pub enum PlannerConfig {
    RuleBased,
    Claude(ClaudeConfig),
    OpenAi(OpenAiConfig),
}

impl PlannerConfig {
    pub fn label(&self) -> String {
        match self {
            Self::RuleBased => "rule-based".into(),
            Self::Claude(cfg) => format!("claude:{}", cfg.model),
            Self::OpenAi(cfg) => cfg.label(),
        }
    }
}

#[derive(Clone)]
pub struct ClaimInvestigator {
    repo: FalkorGraphRepository,
    dispatcher: ActionDispatcher,
    planner: PlannerConfig,
}

impl ClaimInvestigator {
    pub fn new(
        repo: FalkorGraphRepository,
        dispatcher: ActionDispatcher,
        planner: PlannerConfig,
    ) -> Self {
        Self {
            repo,
            dispatcher,
            planner,
        }
    }

    pub fn planner_label(&self) -> String {
        self.planner.label()
    }

    /// Unknown claim -> `Err("claim <id> not found")`.
    pub async fn investigate(&self, claim_id: &str) -> Result<InvestigationDossier, String> {
        let started = Instant::now();
        let claim = self
            .repo
            .get_claim(claim_id)
            .await?
            .ok_or_else(|| format!("claim {claim_id} not found"))?;
        info!(
            "Investigating {claim_id} with planner {}",
            self.planner.label()
        );

        // One read: count, latest verdict and its RELIED_ON agree even under concurrent runs.
        let prior = self.repo.prior_verdicts(claim_id).await?;

        let mut outcomes: Vec<ToolOutcome> = Vec::new();
        let mut skipped = Vec::new();
        let remembered = prior
            .as_ref()
            .map(|p| p.relied_on.clone())
            .unwrap_or_default();
        let (planner_label, narrative, fallback_note) = match &self.planner {
            PlannerConfig::RuleBased => {
                let mut planner = RuleBasedPlanner::remembering(remembered);
                let narrative = self.run(&mut planner, claim_id, &mut outcomes).await?;
                skipped = planner.skipped().to_vec();
                (self.planner.label(), narrative, None)
            }
            PlannerConfig::Claude(cfg) => {
                let primary = ClaudePlanner::new(cfg.clone());
                self.run_llm(
                    primary,
                    None,
                    remembered,
                    claim_id,
                    &mut outcomes,
                    &mut skipped,
                )
                .await?
            }
            PlannerConfig::OpenAi(cfg) => {
                let primary = OpenAiPlanner::new(cfg.clone());
                let backup = cfg.backup.as_deref().cloned().map(OpenAiPlanner::new);
                self.run_llm(
                    primary,
                    backup,
                    remembered,
                    claim_id,
                    &mut outcomes,
                    &mut skipped,
                )
                .await?
            }
        };

        let patterns = evidence::scoring_patterns(claim_id, &outcomes)?;
        let mut assessment = FraudRiskEvaluator::evaluate(&patterns);
        let failures = verdict::graph_failures(claim_id, &outcomes);
        verdict::apply_graph_failures(&mut assessment, &failures);
        let mut summary_rationale = evidence::rationale(
            assessment.risk_score,
            &assessment.risk_tier,
            &patterns,
            outcomes.len(),
        );
        summary_rationale.push_str(&verdict::failure_note(assessment.risk_score, &failures));
        let memory = InvestigationMemory::new(prior, &assessment.risk_tier, &skipped);
        if let Some(memory) = &memory {
            summary_rationale.push_str(&format!("\n\n{}", memory.note()));
        }
        if let Some(note) = fallback_note {
            summary_rationale.push_str(&format!("\n\n{note}"));
        }
        if let Some(text) = narrative {
            summary_rationale.push_str(&format!(
                "\n\nPlanner narrative ({planner_label}, not used for scoring): {text}"
            ));
        }

        let steps = evidence::provenance_steps(claim_id, &outcomes);
        let certificate = ProvenanceCertificate {
            certificate_id: format!("CERT-{}", uuid::Uuid::new_v4()),
            claim_id: claim_id.to_string(),
            generated_at: Utc::now().to_rfc3339(),
            overall_decision: assessment.risk_tier.clone(),
            confidence_score: assessment.confidence,
            final_risk_score: assessment.risk_score,
            execution_latency_ms: elapsed_ms(started),
            cryptographic_digest: ProvenanceDigestSigner::compute_digest(claim_id, &steps),
            topological_evidence: steps,
            forensic_conclusion: summary_rationale.clone(),
        };

        let dispatched = self
            .dispatcher
            .dispatch(&claim, &assessment, &patterns, &certificate)
            .await?;
        let subgraph = self.repo.claim_subgraph(claim_id, 3).await?;
        // Written after the dossier subgraph is read, so a first investigation's subgraph is
        // unchanged; later investigations' subgraphs show these edges.
        self.repo
            .link_relied_on(
                &certificate.certificate_id,
                &evidence::relied_on_candidates(claim_id, &patterns),
            )
            .await?;
        let agent_trace = evidence::agent_trace(&outcomes);

        Ok(InvestigationDossier {
            claim_id: claim_id.to_string(),
            claim_amount: claim.amount,
            status: dispatched.claim_status,
            risk_score: assessment.risk_score,
            risk_tier: assessment.risk_tier,
            confidence: assessment.confidence,
            summary_rationale,
            patterns_matched: patterns,
            certificate,
            remediation: dispatched.remediation,
            subgraph,
            agent_trace,
            planner: planner_label,
            memory,
        })
    }

    /// LLM planners: failures fall back to the backup provider (if any), then the rule-based
    /// policy instead of aborting, and the coverage guard runs every detector the model
    /// skipped. Returns (label, narrative, note).
    async fn run_llm<P: Planner + Send>(
        &self,
        primary: P,
        backup: Option<P>,
        remembered: Vec<String>,
        claim_id: &str,
        outcomes: &mut Vec<ToolOutcome>,
        skipped: &mut Vec<PlannedCall>,
    ) -> Result<(String, Option<String>, Option<String>), String> {
        let mut planner = FallbackPlanner::new(primary, backup).remembering(remembered);
        let narrative = self.run(&mut planner, claim_id, outcomes).await?;
        *skipped = planner.skipped().to_vec();
        self.coverage_guard(claim_id, outcomes).await;
        let primary = self.planner.label();
        Ok((
            planner.label(&primary),
            narrative,
            planner.failure_note(&primary),
        ))
    }

    /// Bounded plan/execute loop appending to `history`; returns the planner narrative.
    /// Planner errors abort loudly (LLM planners are wrapped in `FallbackPlanner`, which never errors).
    async fn run<P: Planner>(
        &self,
        planner: &mut P,
        claim_id: &str,
        history: &mut Vec<ToolOutcome>,
    ) -> Result<Option<String>, String> {
        while history.len() < MAX_STEPS {
            let calls = planner.plan(claim_id, history).await?;
            if calls.is_empty() {
                break;
            }
            for call in calls.into_iter().take(MAX_STEPS - history.len()) {
                let outcome = match repeated_ring_component(history, &call) {
                    Some(reused) => reused,
                    None => self.execute(call).await,
                };
                history.push(outcome);
            }
        }
        Ok(planner.narrative().map(str::to_string))
    }

    async fn execute(&self, call: PlannedCall) -> ToolOutcome {
        let started = Instant::now();
        let result = crate::tools::execute_tool(&self.repo, &call.tool, &call.arguments).await;
        let latency_ms = elapsed_ms(started);
        if let Err(e) = &result {
            info!("tool {} failed: {e}", call.tool);
        }
        ToolOutcome {
            call,
            result,
            latency_ms,
        }
    }

    /// An LLM must not be able to clear a claim by skipping detectors: any detector the model
    /// never ran on the target claim is executed before scoring (outside the planner budget).
    async fn coverage_guard(&self, claim_id: &str, outcomes: &mut Vec<ToolOutcome>) {
        for tool in DETECTORS {
            let covered = outcomes.iter().any(|o| {
                o.call.tool == tool && o.call.claim_arg() == Some(claim_id) && o.result.is_ok()
            });
            if !covered {
                let rationale = format!("Coverage guard: planner did not run {tool} on {claim_id}; executed before scoring.");
                outcomes.push(
                    self.execute(PlannedCall::new(
                        tool,
                        json!({ "claim_id": claim_id }),
                        rationale,
                    ))
                    .await,
                );
            }
        }
    }
}

/// Wall-clock milliseconds with microsecond precision; FalkorDB hops are sub-millisecond,
/// so integer millis would render most trace steps as "0 ms".
fn elapsed_ms(started: Instant) -> f64 {
    (started.elapsed().as_micros() as f64) / 1000.0
}
