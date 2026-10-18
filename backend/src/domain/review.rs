//! Human review of a frozen payout: the assigned investigator confirms the fraud or releases
//! the payout. A freeze is a hold; this review is the decision.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Claim statuses that hold a payout until a person reviews it.
pub const REVIEWABLE_STATUSES: [&str; 3] = [
    "FLAGGED_SIU_FRAUD",
    "PAYOUT_FROZEN",
    CONFLICTING_VERDICTS_STATUS,
];

/// Status of a claim whose re-investigation disagreed with its latest verdict.
pub const CONFLICTING_VERDICTS_STATUS: &str = "NEEDS_REVIEW_CONFLICTING_VERDICTS";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    ConfirmFraud,
    Release,
}

impl ReviewDecision {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "confirm_fraud" => Ok(Self::ConfirmFraud),
            "release" => Ok(Self::Release),
            other => Err(format!(
                "argument 'decision' must be 'confirm_fraud' or 'release' (got '{other}')"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfirmFraud => "confirm_fraud",
            Self::Release => "release",
        }
    }

    /// Claim status after the review.
    pub fn claim_status(self) -> &'static str {
        match self {
            Self::ConfirmFraud => "CONFIRMED_FRAUD",
            Self::Release => "RELEASED_AFTER_REVIEW",
        }
    }

    /// Confirmed fraud keeps the payout frozen; a release unfreezes it.
    pub fn payout_frozen(self) -> bool {
        self == Self::ConfirmFraud
    }
}

/// `POST /api/claims/{id}/review` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub decision: ReviewDecision,
    pub reviewer: String,
    #[serde(default)]
    pub note: Option<String>,
}

impl ReviewRequest {
    /// Reviewer trimmed and non-empty; a blank note becomes no note.
    pub fn normalized(self) -> Result<Self, String> {
        let reviewer = self.reviewer.trim().to_string();
        if reviewer.is_empty() {
            return Err("reviewer must not be empty".into());
        }
        let note = self
            .note
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        Ok(Self {
            decision: self.decision,
            reviewer,
            note,
        })
    }
}

/// A recorded `Review` node and the claim state it left behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub review_id: String,
    pub claim_id: String,
    pub decision: ReviewDecision,
    pub reviewer_id: String,
    pub reviewer_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub reviewed_at: String,
    /// The Investigation this review decided on (the claim's latest).
    pub investigation_id: String,
    pub claim_status: String,
    pub payout_frozen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewError {
    NotFound(String),
    /// The reviewer is not the claim's assigned investigator.
    Forbidden(String),
    /// Not frozen, or already reviewed.
    Conflict(String),
    Upstream(String),
}

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(m) | Self::Forbidden(m) | Self::Conflict(m) | Self::Upstream(m) => {
                f.write_str(m)
            }
        }
    }
}

/// What the graph says about a claim whose review statement matched nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGate {
    pub status: String,
    pub assigned: Vec<String>,
    pub reviews: i64,
}

impl ReviewGate {
    /// Why the review was refused: state conflicts (409) before the reviewer check (403).
    pub fn rejection(&self, claim_id: &str, reviewer: &str) -> ReviewError {
        if self.reviews > 0 {
            return ReviewError::Conflict(format!("claim {claim_id} was already reviewed"));
        }
        if !REVIEWABLE_STATUSES.contains(&self.status.as_str()) || self.assigned.is_empty() {
            return ReviewError::Conflict(format!(
                "claim {claim_id} has no frozen payout to review (status {})",
                self.status
            ));
        }
        if !self.assigned.iter().any(|a| a == reviewer) {
            return ReviewError::Forbidden(format!(
                "only the investigator assigned to claim {claim_id} may review it"
            ));
        }
        ReviewError::Conflict(format!(
            "claim {claim_id} has no recorded investigation to review"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(status: &str, assigned: &[&str], reviews: i64) -> ReviewGate {
        ReviewGate {
            status: status.into(),
            assigned: assigned.iter().map(|s| s.to_string()).collect(),
            reviews,
        }
    }

    #[test]
    fn decisions_map_to_status_and_freeze() {
        let confirm = ReviewDecision::parse("confirm_fraud").unwrap();
        assert_eq!(confirm.claim_status(), "CONFIRMED_FRAUD");
        assert!(confirm.payout_frozen());
        let release = ReviewDecision::parse("release").unwrap();
        assert_eq!(release.claim_status(), "RELEASED_AFTER_REVIEW");
        assert!(!release.payout_frozen());
        assert!(ReviewDecision::parse("approve").is_err());
    }

    #[test]
    fn request_needs_a_reviewer_and_drops_a_blank_note() {
        let req = ReviewRequest {
            decision: ReviewDecision::Release,
            reviewer: " INV-202 ".into(),
            note: Some("  ".into()),
        };
        let req = req.normalized().unwrap();
        assert_eq!((req.reviewer.as_str(), req.note), ("INV-202", None));
        let blank = ReviewRequest {
            decision: ReviewDecision::Release,
            reviewer: " ".into(),
            note: None,
        };
        assert!(blank.normalized().is_err());
    }

    #[test]
    fn gate_explains_each_refusal() {
        let reviewed = gate("CONFIRMED_FRAUD", &["INV-202"], 1);
        assert!(matches!(
            reviewed.rejection("C", "INV-202"),
            ReviewError::Conflict(_)
        ));
        let cleared = gate("CLEARED_AUTOMATED", &[], 0);
        assert!(matches!(
            cleared.rejection("C", "INV-202"),
            ReviewError::Conflict(_)
        ));
        let frozen = gate("FLAGGED_SIU_FRAUD", &["INV-202"], 0);
        assert!(matches!(
            frozen.rejection("C", "INV-999"),
            ReviewError::Forbidden(_)
        ));
    }
}
