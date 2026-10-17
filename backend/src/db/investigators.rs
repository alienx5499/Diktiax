//! SIU investigator selection and assignment. Investigators are graph nodes, never constants.

use super::mapping::{col, text};
use super::queries;
use super::repository::FalkorGraphRepository;
use chrono::Utc;
use falkordb::{FalkorValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Investigator {
    pub id: String,
    pub name: String,
    pub specialties: Vec<String>,
    pub open_cases: i64,
}

fn investigator(row: &Row) -> Result<Investigator, String> {
    Ok(Investigator {
        id: col(row, "id")?,
        name: col(row, "name")?,
        specialties: col(row, "specialties")?,
        open_cases: col(row, "open_cases")?,
    })
}

/// True for a violation of ASSIGNMENT_CONSTRAINT. Through the client, FalkorDB reports it as
/// "...: constraint violation, on edge of relationship-type ASSIGNED_INVESTIGATOR".
fn is_assignment_violation(error: &str) -> bool {
    let (rel, prop) = queries::ASSIGNMENT_CONSTRAINT;
    error.contains("constraint violation") && (error.contains(rel) || error.contains(prop))
}

impl FalkorGraphRepository {
    /// Investigator covering the most `pattern_names`, ties broken by fewest open cases.
    /// `None` only when the graph holds no Investigator nodes.
    pub async fn pick_siu_investigator(
        &self,
        pattern_names: &[String],
    ) -> Result<Option<Investigator>, String> {
        let patterns = FalkorValue::Array(pattern_names.iter().map(|p| text(p)).collect());
        let rows = self
            .read_rows(queries::PICK_SIU_INVESTIGATOR, vec![("patterns", patterns)])
            .await?;
        rows.first().map(investigator).transpose()
    }

    /// The claim's investigator: the existing assignment if there is one, else the best pick for
    /// `pattern_names`, assigned (ASSIGN_ONCE; at most one edge per claim even across concurrent
    /// callers and processes). `None` only when the graph holds no Investigator nodes.
    pub async fn assign_once(
        &self,
        claim_id: &str,
        pattern_names: &[String],
    ) -> Result<Option<Investigator>, String> {
        let params = || {
            vec![
                ("claim_id", text(claim_id)),
                (
                    "patterns",
                    FalkorValue::Array(pattern_names.iter().map(|p| text(p)).collect()),
                ),
                ("assigned_at", text(&Utc::now().to_rfc3339())),
            ]
        };
        let rows = match self.rows(queries::ASSIGN_ONCE, params()).await {
            // A concurrent run committed first; its edge is visible now, so the re-run returns it.
            // One retry only: a second failure propagates.
            Err(e) if is_assignment_violation(&e) => {
                self.rows(queries::ASSIGN_ONCE, params()).await?
            }
            other => other?,
        };
        rows.first().map(investigator).transpose()
    }

    /// Liveness probe used by the health endpoint.
    pub async fn ping(&self) -> Result<(), String> {
        self.rows(queries::PING, vec![]).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::is_assignment_violation;

    #[test]
    fn matches_only_the_assignment_constraint_violation() {
        assert!(is_assignment_violation(
            "FalkorDB query failed: An error occurred while sending the request to Redis: \
             constraint violation, on edge of relationship-type ASSIGNED_INVESTIGATOR"
        ));
        assert!(!is_assignment_violation(
            "FalkorDB query failed: constraint violation on node of type Claim"
        ));
        assert!(!is_assignment_violation("connection refused"));
    }
}
