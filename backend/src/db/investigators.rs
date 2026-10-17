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

    /// Writes `(Claim)-[:ASSIGNED_INVESTIGATOR]->(Investigator)` and returns the updated investigator.
    pub async fn assign_investigator(
        &self,
        claim_id: &str,
        investigator_id: &str,
    ) -> Result<Investigator, String> {
        let params = vec![
            ("claim_id", text(claim_id)),
            ("investigator_id", text(investigator_id)),
            ("assigned_at", text(&Utc::now().to_rfc3339())),
        ];
        let rows = self.rows(queries::ASSIGN_INVESTIGATOR, params).await?;
        let row = rows.first().ok_or_else(|| {
            format!("claim {claim_id} or investigator {investigator_id} not found")
        })?;
        investigator(row)
    }

    /// Liveness probe used by the health endpoint.
    pub async fn ping(&self) -> Result<(), String> {
        self.rows(queries::PING, vec![]).await.map(|_| ())
    }
}
