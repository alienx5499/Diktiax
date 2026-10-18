//! Investigation memory: verdict history, the chained Investigation write and RELIED_ON links.

use super::mapping::{chained_verdict, col, evidence_reuse, investigation_record, text};
use super::queries;
use super::repository::FalkorGraphRepository;
use crate::domain::{ChainedVerdict, EvidenceReuse, PriorVerdicts};
use chrono::Utc;
use falkordb::FalkorValue;

impl FalkorGraphRepository {
    /// Past verdicts for a claim in SUPERSEDES-chain order, head first; empty when never
    /// investigated.
    pub async fn investigation_history(
        &self,
        claim_id: &str,
    ) -> Result<Vec<ChainedVerdict>, String> {
        let rows = self
            .read_rows(
                queries::INVESTIGATION_HISTORY,
                vec![("claim_id", text(claim_id))],
            )
            .await?;
        if rows.is_empty() {
            return Err(format!("claim {claim_id} not found"));
        }
        // Row index is chain position: the only filtered row is the lone all-null one.
        rows.iter()
            .enumerate()
            .filter_map(|(position, row)| chained_verdict(row, position).transpose())
            .collect()
    }

    /// Verdicts on other claims that RELIED_ON this one, newest first; errors for an unknown claim.
    pub async fn evidence_reuse(&self, claim_id: &str) -> Result<Vec<EvidenceReuse>, String> {
        let rows = self
            .read_rows(queries::EVIDENCE_REUSE, vec![("claim_id", text(claim_id))])
            .await?;
        if rows.is_empty() {
            return Err(format!("claim {claim_id} not found"));
        }
        rows.iter()
            .filter_map(|row| evidence_reuse(row).transpose())
            .collect()
    }

    /// Count, newest verdict and its RELIED_ON claims from one read (PRIOR_VERDICTS), so they
    /// agree even while other runs record verdicts. `None` when never investigated.
    pub async fn prior_verdicts(&self, claim_id: &str) -> Result<Option<PriorVerdicts>, String> {
        let rows = self
            .read_rows(queries::PRIOR_VERDICTS, vec![("claim_id", text(claim_id))])
            .await?;
        let row = rows
            .first()
            .ok_or_else(|| format!("claim {claim_id} not found"))?;
        let Some(latest) = investigation_record(row)? else {
            return Ok(None);
        };
        let count: i64 = col(row, "prior")?;
        Ok(Some(PriorVerdicts {
            count: usize::try_from(count).map_err(|e| format!("column 'prior': {e}"))?,
            latest,
            relied_on: col(row, "relied_on")?,
        }))
    }

    /// Persists an Investigation node linked to the claim and to its previous verdict
    /// (RECORD_CHAINED_INVESTIGATION). A chain-constraint violation means a concurrent run took
    /// the head first; the statement is re-run, with a fresh `recorded_at`, so chain order and
    /// `recorded_at` order agree. Every failed attempt implies another run committed.
    pub async fn record_investigation(
        &self,
        claim_id: &str,
        decision: &str,
        risk_score: i32,
        certificate_id: &str,
    ) -> Result<(), String> {
        let params = || {
            vec![
                ("claim_id", text(claim_id)),
                ("decision", text(decision)),
                ("risk_score", FalkorValue::I64(i64::from(risk_score))),
                ("certificate_id", text(certificate_id)),
                ("recorded_at", text(&Utc::now().to_rfc3339())),
            ]
        };
        let mut attempt = 1;
        let rows = loop {
            match self
                .rows(queries::RECORD_CHAINED_INVESTIGATION, params())
                .await
            {
                Err(e) if is_chain_violation(&e) && attempt < CHAIN_ATTEMPTS => attempt += 1,
                other => break other?,
            }
        };
        if rows.is_empty() {
            return Err(format!("claim {claim_id} not found"));
        }
        Ok(())
    }

    /// `(investigation)-[:RELIED_ON]->(claim)` for each id in `claim_ids` that names an existing
    /// Claim; returns how many edges were created.
    pub async fn link_relied_on(
        &self,
        investigation_id: &str,
        claim_ids: &[String],
    ) -> Result<i64, String> {
        let params = vec![
            ("investigation_id", text(investigation_id)),
            (
                "claim_ids",
                FalkorValue::Array(claim_ids.iter().map(|id| text(id)).collect()),
            ),
        ];
        let rows = self.rows(queries::LINK_RELIED_ON, params).await?;
        rows.first().map_or(Ok(0), |row| col(row, "linked"))
    }
}

/// Attempts per record_investigation call. Each lost attempt means another run on the
/// same claim committed, so this caps simultaneous writers per claim at 64; raise it (or add
/// backoff) if more ever race.
const CHAIN_ATTEMPTS: usize = 64;

/// True for a violation of SUPERSEDES_CONSTRAINT or CHAIN_ROOT_CONSTRAINT. FalkorDB reports
/// "unique constraint violation, on edge of relationship-type SUPERSEDES" and
/// "unique constraint violation on node of type InvestigationChain".
fn is_chain_violation(error: &str) -> bool {
    error.contains("constraint violation")
        && (error.contains(queries::SUPERSEDES_CONSTRAINT.0)
            || error.contains(queries::CHAIN_ROOT_CONSTRAINT.0))
}

#[cfg(test)]
mod tests {
    use super::is_chain_violation;

    #[test]
    fn matches_only_chain_constraint_violations() {
        assert!(is_chain_violation(
            "FalkorDB query failed: unique constraint violation, on edge of relationship-type SUPERSEDES"
        ));
        assert!(is_chain_violation(
            "FalkorDB query failed: unique constraint violation on node of type InvestigationChain"
        ));
        assert!(!is_chain_violation(
            "FalkorDB query failed: constraint violation, on edge of relationship-type ASSIGNED_INVESTIGATOR"
        ));
        assert!(!is_chain_violation("connection refused"));
    }
}
