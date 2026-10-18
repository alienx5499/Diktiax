use serde::{Deserialize, Serialize};

/// Read model of a claim as stored in the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimSummary {
    pub claim_id: String,
    pub claimant_name: String,
    pub amount: f64,
    pub status: String,
    pub filed_at: String,
}

/// One past verdict the agent recorded for a claim (an `Investigation` node).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestigationRecord {
    pub certificate_id: String,
    pub decision: String,
    pub risk_score: i32,
    pub recorded_at: String,
}

/// One verdict in a claim's SUPERSEDES chain, as investigation history returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainedVerdict {
    #[serde(flatten)]
    pub record: InvestigationRecord,
    /// Certificate id of the verdict this one superseded; `None` at the root.
    pub supersedes: Option<String>,
    /// Claims this verdict RELIED_ON.
    pub relied_on: Vec<String>,
    /// 0 for the head (newest), counting back along the chain.
    pub position: usize,
}

/// A verdict on another claim that RELIED_ON the queried claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceReuse {
    pub claim_id: String,
    pub certificate_id: String,
    pub decision: String,
    pub recorded_at: String,
    /// True while no later verdict supersedes it.
    pub is_head: bool,
}

/// What the agent remembers about a claim before re-investigating it.
#[derive(Debug, Clone, PartialEq)]
pub struct PriorVerdicts {
    /// Verdicts recorded before this read.
    pub count: usize,
    pub latest: InvestigationRecord,
    /// Claims the latest verdict RELIED_ON.
    pub relied_on: Vec<String>,
}

/// Command payload for filing a brand-new claim into the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewClaim {
    pub claim_id: Option<String>,
    pub claimant_name: String,
    pub phone: String,
    pub address: String,
    pub vehicle_vin: String,
    pub location_id: String,
    pub repair_shop_id: String,
    pub clinic_id: Option<String>,
    pub adjuster_id: Option<String>,
    pub amount: f64,
    pub police_report: bool,
}

impl NewClaim {
    /// Pure validation: every required string non-blank, optional strings non-blank when
    /// present, amount finite and strictly positive.
    pub fn validate(&self) -> Result<(), String> {
        let required = [
            ("claimant_name", &self.claimant_name),
            ("phone", &self.phone),
            ("address", &self.address),
            ("vehicle_vin", &self.vehicle_vin),
            ("location_id", &self.location_id),
            ("repair_shop_id", &self.repair_shop_id),
        ];
        for (field, value) in required {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        let optional = [
            ("claim_id", &self.claim_id),
            ("clinic_id", &self.clinic_id),
            ("adjuster_id", &self.adjuster_id),
        ];
        for (field, value) in optional {
            if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
                return Err(format!("{field} must not be blank when provided"));
            }
        }
        if !self.amount.is_finite() || self.amount <= 0.0 {
            return Err(format!("amount must be > 0 (got {})", self.amount));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> NewClaim {
        NewClaim {
            claim_id: None,
            claimant_name: "Jane Roe".into(),
            phone: "+1-555-000-0000".into(),
            address: "1 Main St".into(),
            vehicle_vin: "VIN123".into(),
            location_id: "LOC-1".into(),
            repair_shop_id: "SHOP-1".into(),
            clinic_id: None,
            adjuster_id: None,
            amount: 1000.0,
            police_report: true,
        }
    }

    #[test]
    fn accepts_valid_claim() {
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn rejects_blank_required_field() {
        let claim = NewClaim {
            phone: "  ".into(),
            ..valid()
        };
        assert_eq!(claim.validate(), Err("phone must not be empty".into()));
    }

    #[test]
    fn rejects_blank_optional_field() {
        let claim = NewClaim {
            clinic_id: Some(String::new()),
            ..valid()
        };
        assert!(claim.validate().is_err());
    }

    #[test]
    fn rejects_non_positive_or_nan_amount() {
        assert!(NewClaim {
            amount: 0.0,
            ..valid()
        }
        .validate()
        .is_err());
        assert!(NewClaim {
            amount: -5.0,
            ..valid()
        }
        .validate()
        .is_err());
        assert!(NewClaim {
            amount: f64::NAN,
            ..valid()
        }
        .validate()
        .is_err());
    }
}
