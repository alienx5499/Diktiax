//! Human review of a frozen payout (RECORD_REVIEW), with the refusal explained by REVIEW_GATE.

use super::mapping::{col, opt_text, text};
use super::queries;
use super::repository::FalkorGraphRepository;
use crate::domain::{
    ReviewDecision, ReviewError, ReviewGate, ReviewRecord, ReviewRequest, REVIEWABLE_STATUSES,
};
use chrono::Utc;
use falkordb::{FalkorValue, Row};

/// A person's decision on a frozen payout, in ONE statement: matches only when `$reviewer` is
/// the claim's assigned investigator and the claim is still held (`$reviewable`), then writes
/// the Review node, `(review)-[:REVIEWS]->(latest Investigation)`, `(investigator)-[:REVIEWED]->
/// (review)`, the new claim status and payout flag, and frees one of the investigator's cases.
/// No rows means nothing was written (REVIEW_GATE says why). REVIEW_CONSTRAINT rejects a second
/// Review for the claim, so racing reviews commit exactly one. Measured: without it, 80 rounds
/// x 16 racing reviews on v4.22.1 never left a second Review; the constraint is the guarantee.
const RECORD_REVIEW: &str =
    "MATCH (c:Claim {id: $claim_id})-[:ASSIGNED_INVESTIGATOR]->(inv:Investigator {id: $reviewer}) \
     WHERE c.status IN $reviewable \
     MATCH (c)-[:HAS_INVESTIGATION]->(i:Investigation) \
     WITH c, inv, i ORDER BY i.recorded_at DESC, i.id DESC LIMIT 1 \
     CREATE (inv)-[:REVIEWED]->(r:Review {id: $review_id, claim_id: c.id, decision: $decision, \
     reviewer: inv.id, note: $note, reviewed_at: $reviewed_at})-[:REVIEWS]->(i) \
     SET c.status = $status, c.payout_frozen = $payout_frozen, inv.open_cases = inv.open_cases - 1 \
     RETURN r.id AS review_id, c.id AS claim_id, r.decision AS decision, inv.id AS reviewer_id, \
     inv.name AS reviewer_name, r.note AS note, r.reviewed_at AS reviewed_at, \
     i.id AS investigation_id, c.status AS claim_status, c.payout_frozen AS payout_frozen";

/// Sets the claim status unless a person already reviewed the claim (a Review REVIEWS one of
/// its investigations), in one statement, so an automated re-investigation never overrides a
/// human decision. Returns the status the claim has afterwards and whether it was reviewed.
const SET_UNREVIEWED_CLAIM_STATUS: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:HAS_INVESTIGATION]->(:Investigation)<-[:REVIEWS]-(r:Review) \
     WITH c, count(r) AS reviews \
     FOREACH (_ IN CASE WHEN reviews = 0 THEN [1] ELSE [] END | SET c.status = $status) \
     RETURN c.status AS status, reviews > 0 AS reviewed";

/// Why RECORD_REVIEW matched nothing: claim status, assigned investigator ids, reviews so far.
/// An unknown claim yields no rows.
const REVIEW_GATE: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:ASSIGNED_INVESTIGATOR]->(inv:Investigator) \
     WITH c, collect(inv.id) AS assigned \
     OPTIONAL MATCH (r:Review {claim_id: c.id}) \
     RETURN c.status AS status, assigned, count(r) AS reviews";

fn review_record(row: &Row) -> Result<ReviewRecord, String> {
    let decision: String = col(row, "decision")?;
    Ok(ReviewRecord {
        review_id: col(row, "review_id")?,
        claim_id: col(row, "claim_id")?,
        decision: ReviewDecision::parse(&decision)?,
        reviewer_id: col(row, "reviewer_id")?,
        reviewer_name: col(row, "reviewer_name")?,
        note: col(row, "note")?,
        reviewed_at: col(row, "reviewed_at")?,
        investigation_id: col(row, "investigation_id")?,
        claim_status: col(row, "claim_status")?,
        payout_frozen: col(row, "payout_frozen")?,
    })
}

/// True for a violation of REVIEW_CONSTRAINT ("unique constraint violation on node of type Review").
fn is_review_violation(error: &str) -> bool {
    let (label, _) = queries::REVIEW_CONSTRAINT;
    error.contains("constraint violation") && error.contains(&format!("type {label}"))
}

impl FalkorGraphRepository {
    /// Writes `status` unless the claim was already reviewed; `Some(kept status)` when the
    /// human decision stood. Unknown claim -> `Err("claim <id> not found")`.
    pub async fn set_status_unless_reviewed(
        &self,
        claim_id: &str,
        status: &str,
    ) -> Result<Option<String>, String> {
        let params = vec![("claim_id", text(claim_id)), ("status", text(status))];
        let rows = self.rows(SET_UNREVIEWED_CLAIM_STATUS, params).await?;
        let row = rows
            .first()
            .ok_or_else(|| format!("claim {claim_id} not found"))?;
        let reviewed: bool = col(row, "reviewed")?;
        Ok(if reviewed {
            Some(col(row, "status")?)
        } else {
            None
        })
    }

    /// Records the assigned investigator's decision on the claim's frozen payout. Unknown claim
    /// -> NotFound; not frozen or already reviewed (including a racing review that committed
    /// first) -> Conflict; someone other than the assigned investigator -> Forbidden.
    pub async fn review_claim(
        &self,
        claim_id: &str,
        request: &ReviewRequest,
    ) -> Result<ReviewRecord, ReviewError> {
        let decision = request.decision;
        let reviewable = REVIEWABLE_STATUSES.iter().map(|s| text(s)).collect();
        let params = vec![
            ("claim_id", text(claim_id)),
            ("reviewer", text(&request.reviewer)),
            ("reviewable", FalkorValue::Array(reviewable)),
            ("review_id", text(&format!("REV-{}", uuid::Uuid::new_v4()))),
            ("decision", text(decision.as_str())),
            ("note", opt_text(request.note.as_deref())),
            ("reviewed_at", text(&Utc::now().to_rfc3339())),
            ("status", text(decision.claim_status())),
            ("payout_frozen", FalkorValue::Bool(decision.payout_frozen())),
        ];
        let rows = match self.rows(RECORD_REVIEW, params).await {
            Err(e) if is_review_violation(&e) => {
                return Err(ReviewError::Conflict(format!(
                    "claim {claim_id} was already reviewed"
                )))
            }
            other => other.map_err(ReviewError::Upstream)?,
        };
        match rows.first() {
            Some(row) => review_record(row).map_err(ReviewError::Upstream),
            None => Err(self
                .review_rejection(claim_id, &request.reviewer)
                .await
                .unwrap_or_else(ReviewError::Upstream)),
        }
    }

    /// Why RECORD_REVIEW matched nothing; `Err` only when the gate read itself fails.
    async fn review_rejection(
        &self,
        claim_id: &str,
        reviewer: &str,
    ) -> Result<ReviewError, String> {
        let rows = self
            .read_rows(REVIEW_GATE, vec![("claim_id", text(claim_id))])
            .await?;
        let Some(row) = rows.first() else {
            return Ok(ReviewError::NotFound(format!("claim {claim_id} not found")));
        };
        let gate = ReviewGate {
            status: col(row, "status")?,
            assigned: col(row, "assigned")?,
            reviews: col(row, "reviews")?,
        };
        Ok(gate.rejection(claim_id, reviewer))
    }
}

#[cfg(test)]
mod tests {
    use super::is_review_violation;

    #[test]
    fn matches_only_the_review_constraint_violation() {
        assert!(is_review_violation(
            "FalkorDB query failed: unique constraint violation on node of type Review"
        ));
        assert!(!is_review_violation(
            "FalkorDB query failed: unique constraint violation on node of type InvestigationChain"
        ));
        assert!(!is_review_violation("connection refused"));
    }
}
