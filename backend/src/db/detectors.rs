//! Fraud-pattern detectors and shop centrality. Every result is derived from the rows the
//! graph returns; zero rows means no match.

use super::mapping::{col, text};
use super::queries;
use super::repository::FalkorGraphRepository;
use crate::domain::{CentralityReport, FraudPatternMatch};
use falkordb::{FalkorValue, Row};

/// Claims at the same location + shop within this many days count as one staged ring.
pub const GHOST_WINDOW_DAYS: i64 = 90;

/// Appends `id` unless already present, preserving first-seen order.
fn push_unique(path: &mut Vec<String>, id: String) {
    if !path.contains(&id) {
        path.push(id);
    }
}

/// Distinct values of a string column across all rows, in first-seen order.
fn distinct(rows: &[Row], column: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for row in rows {
        push_unique(&mut out, col(row, column)?);
    }
    Ok(out)
}

fn pattern(
    name: &str,
    (severity, risk_points): (&str, i32),
    description: String,
    cypher: &str,
    evidence_path: Vec<String>,
) -> Option<FraudPatternMatch> {
    Some(FraudPatternMatch {
        pattern_name: name.into(),
        severity: severity.into(),
        risk_points,
        description,
        cypher_query: cypher.into(),
        evidence_path,
    })
}

impl FalkorGraphRepository {
    async fn detector_rows(
        &self,
        query: &str,
        claim_id: &str,
        extra: Vec<(&str, FalkorValue)>,
    ) -> Result<Vec<Row>, String> {
        let mut params = vec![("claim_id", text(claim_id))];
        params.extend(extra);
        self.rows(query, params).await
    }

    pub async fn detect_ghost_collision(
        &self,
        claim_id: &str,
    ) -> Result<Option<FraudPatternMatch>, String> {
        let window = vec![("window_days", FalkorValue::I64(GHOST_WINDOW_DAYS))];
        let rows = self
            .detector_rows(queries::GHOST_COLLISION, claim_id, window)
            .await?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let locations = distinct(&rows, "location_id")?;
        let shops = distinct(&rows, "shop_id")?;
        let mut path = vec![col::<String>(first, "claim_id")?];
        path.extend(locations.iter().chain(&shops).cloned());
        let base = path.len();
        let mut closest = i64::MAX;
        for row in &rows {
            push_unique(&mut path, col(row, "linked_claim_id")?);
            closest = closest.min(col(row, "days_apart")?);
        }
        let linked = path.len() - base;
        let grade = if linked >= 2 {
            ("CRITICAL", 45)
        } else {
            ("HIGH", 35)
        };
        let description = format!(
            "{linked} other claim(s) staged at location {} and repaired at {} \
             within {GHOST_WINDOW_DAYS} days (closest {closest} days apart).",
            locations.join(", "),
            shops.join(", ")
        );
        Ok(pattern(
            "Ghost Collision Ring",
            grade,
            description,
            queries::GHOST_COLLISION,
            path,
        ))
    }

    pub async fn detect_shared_contacts(
        &self,
        claim_id: &str,
    ) -> Result<Option<FraudPatternMatch>, String> {
        let rows = self
            .detector_rows(queries::SHARED_CONTACTS, claim_id, vec![])
            .await?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let mut path = vec![
            col::<String>(first, "claim_id")?,
            col(first, "customer_id")?,
        ];
        let mut peers: Vec<String> = Vec::new();
        let mut links: Vec<String> = Vec::new();
        for row in &rows {
            let contact: String = col(row, "contact")?;
            let peer_id: String = col(row, "peer_id")?;
            let via = match col::<String>(row, "via")?.as_str() {
                "HAS_PHONE" => "phone",
                _ => "address",
            };
            let link = format!(
                "{} ({peer_id}) shares {via} '{contact}'",
                col::<String>(row, "peer_name")?
            );
            if !links.contains(&link) {
                links.push(link);
            }
            push_unique(&mut path, contact);
            push_unique(&mut peers, peer_id.clone());
            push_unique(&mut path, peer_id);
            push_unique(&mut path, col(row, "linked_claim_id")?);
        }
        let grade = if peers.len() >= 2 {
            ("CRITICAL", 35)
        } else {
            ("HIGH", 25)
        };
        let description = format!(
            "Claimant shares contact points with {} other claimant(s): {}.",
            peers.len(),
            links.join("; ")
        );
        Ok(pattern(
            "Shared Contact Network",
            grade,
            description,
            queries::SHARED_CONTACTS,
            path,
        ))
    }

    pub async fn detect_vin_cloning(
        &self,
        claim_id: &str,
    ) -> Result<Option<FraudPatternMatch>, String> {
        let rows = self
            .detector_rows(queries::VIN_CLONING, claim_id, vec![])
            .await?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let vin: String = col(first, "vin")?;
        let jurisdiction: String = col(first, "jurisdiction")?;
        let mut path = vec![
            col::<String>(first, "claim_id")?,
            col(first, "vehicle_id")?,
            vin.clone(),
        ];
        let mut jurisdictions = vec![jurisdiction];
        let clones = distinct(&rows, "clone_vehicle_id")?;
        for row in &rows {
            push_unique(&mut jurisdictions, col(row, "clone_jurisdiction")?);
            push_unique(&mut path, col(row, "clone_vehicle_id")?);
            push_unique(&mut path, col(row, "linked_claim_id")?);
        }
        let cross_state = jurisdictions.len() > 1;
        let grade = if cross_state {
            ("CRITICAL", 50)
        } else {
            ("HIGH", 35)
        };
        let description = format!(
            "VIN {vin} is registered on {} vehicle record(s) with {} other claim(s); jurisdictions: {}.",
            clones.len() + 1,
            rows.len(),
            jurisdictions.join(", ")
        );
        Ok(pattern(
            "Cloned VIN Across Jurisdictions",
            grade,
            description,
            queries::VIN_CLONING,
            path,
        ))
    }

    pub async fn detect_kickback_triangle(
        &self,
        claim_id: &str,
    ) -> Result<Option<FraudPatternMatch>, String> {
        let rows = self
            .detector_rows(queries::KICKBACK_TRIANGLE, claim_id, vec![])
            .await?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let adjusters = distinct(&rows, "adjuster_id")?;
        let shops = distinct(&rows, "shop_id")?;
        let clinics = distinct(&rows, "clinic_id")?;
        let referral: bool = col(first, "referral_link")?;
        let mut path = vec![col::<String>(first, "claim_id")?];
        path.extend(adjusters.iter().chain(&shops).chain(&clinics).cloned());
        let base = path.len();
        for row in &rows {
            push_unique(&mut path, col(row, "linked_claim_id")?);
        }
        let recurring = path.len() - base;
        let grade = if referral {
            ("CRITICAL", 40)
        } else {
            ("HIGH", 30)
        };
        let description = format!(
            "No police report; adjuster {}, shop {} and clinic {} recur together on \
             {recurring} other claim(s){}.",
            adjusters.join(", "),
            shops.join(", "),
            clinics.join(", "),
            if referral {
                "; shop has a COLLUDES_WITH referral link to the clinic"
            } else {
                ""
            }
        );
        Ok(pattern(
            "Adjuster-Shop-Clinic Kickback Triangle",
            grade,
            description,
            queries::KICKBACK_TRIANGLE,
            path,
        ))
    }

    /// Degree centrality of a shop versus the mean claim degree of all shops in the graph.
    pub async fn shop_centrality(&self, shop_id: &str) -> Result<CentralityReport, String> {
        let rows = self.rows(queries::SHOP_DEGREES, vec![]).await?;
        let mut degrees = Vec::with_capacity(rows.len());
        let mut target = None;
        for row in &rows {
            let id: String = col(row, "shop_id")?;
            let degree: i64 = col(row, "degree")?;
            degrees.push(degree);
            if id == shop_id {
                target = Some((
                    col::<Option<String>>(row, "shop_name")?.unwrap_or_else(|| id.clone()),
                    degree,
                ));
            }
        }
        let (name, degree) = target.ok_or_else(|| format!("repair shop {shop_id} not found"))?;
        Ok(CentralityReport::compute(
            shop_id.to_string(),
            name,
            degree,
            &degrees,
        ))
    }
}
