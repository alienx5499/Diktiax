//! Seeding: indexes + data/seed.cypher (embedded at compile time, cwd-independent).

use super::mapping::col;
use super::queries;
use super::repository::FalkorGraphRepository;
use falkordb::{ConstraintStatus, ConstraintType, EntityType, FalkorDBError, WaitOptions};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::info;

const SEED_CYPHER: &str = include_str!("../../../data/seed.cypher");

/// Unique constraints every graph must carry before agents write to it: `(entity, (label, property))`.
const UNIQUE_CONSTRAINTS: [(EntityType, (&str, &str)); 3] = [
    (EntityType::Edge, queries::ASSIGNMENT_CONSTRAINT),
    (EntityType::Edge, queries::SUPERSEDES_CONSTRAINT),
    (EntityType::Node, queries::CHAIN_ROOT_CONSTRAINT),
];

/// Readiness wait for a unique constraint, used for both a fresh and an existing constraint.
/// Mirrors falkordb's WaitOptions defaults (50ms poll, x1.5 backoff, 1s cap).
const CONSTRAINT_TIMEOUT: Duration = Duration::from_secs(30);
const CONSTRAINT_POLL: Duration = Duration::from_millis(50);
const CONSTRAINT_MAX_POLL: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq, Eq)]
enum Readiness {
    Ready,
    Pending,
}

/// OPERATIONAL is ready, UNDER CONSTRUCTION is worth waiting for, FAILED is an error.
fn constraint_readiness(
    status: ConstraintStatus,
    (label, prop): (&str, &str),
) -> Result<Readiness, String> {
    match status {
        ConstraintStatus::Active => Ok(Readiness::Ready),
        ConstraintStatus::Pending => Ok(Readiness::Pending),
        ConstraintStatus::Failed => Err(constraint_failed_message(label, prop)),
    }
}

fn constraint_failed_message(label: &str, prop: &str) -> String {
    format!(
        "unique constraint on {label}.{prop} FAILED to build: existing {label} entities violate \
         it (duplicate {prop}, or entities created before {prop} was stored); reseed the graph to fix"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedReport {
    /// False when `reset` was false and the graph already held data (nothing written).
    pub applied: bool,
    pub nodes: i64,
    pub edges: i64,
}

/// Statements of a seed script: full-line `//` comments removed, split on `;`.
fn seed_statements(script: &str) -> Vec<String> {
    let body: String = script
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    body.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl FalkorGraphRepository {
    /// Loads the seed graph. `reset = true` deletes the graph first (idempotent);
    /// `reset = false` only seeds an empty graph.
    pub async fn seed(&self, reset: bool) -> Result<SeedReport, String> {
        if reset {
            self.delete_graph().await?;
        } else if self.count(queries::COUNT_NODES).await? > 0 {
            self.ensure_unique_constraints().await?;
            return Ok(SeedReport {
                applied: false,
                nodes: self.count(queries::COUNT_NODES).await?,
                edges: self.count(queries::COUNT_EDGES).await?,
            });
        }
        self.create_indexes().await?;
        self.ensure_unique_constraints().await?;
        for statement in seed_statements(SEED_CYPHER) {
            self.rows(&statement, vec![])
                .await
                .map_err(|e| format!("seed statement failed: {e}"))?;
        }
        let report = SeedReport {
            applied: true,
            nodes: self.count(queries::COUNT_NODES).await?,
            edges: self.count(queries::COUNT_EDGES).await?,
        };
        info!(
            "Seeded graph '{}': {} nodes, {} edges",
            self.graph_name(),
            report.nodes,
            report.edges
        );
        Ok(report)
    }

    async fn delete_graph(&self) -> Result<(), String> {
        match self.graph_handle().delete().await {
            Ok(()) => Ok(()),
            // Deleting a graph that does not exist yet is the desired end state.
            Err(e) if e.to_string().contains("empty key") => Ok(()),
            Err(e) => Err(format!(
                "failed to delete graph '{}': {e}",
                self.graph_name()
            )),
        }
    }

    async fn create_indexes(&self) -> Result<(), String> {
        for statement in queries::INDEXES {
            match self.rows(statement, vec![]).await {
                Ok(_) => {}
                Err(e) if e.contains("already indexed") || e.contains("already exists") => {}
                Err(e) => return Err(format!("index creation failed ({statement}): {e}")),
            }
        }
        Ok(())
    }

    async fn ensure_unique_constraints(&self) -> Result<(), String> {
        for (entity, key) in UNIQUE_CONSTRAINTS {
            self.ensure_unique_constraint(entity, key).await?;
        }
        Ok(())
    }

    /// Creates the constraint unless present, then waits until it is OPERATIONAL, whether
    /// just created or found UNDER CONSTRUCTION (bounded by CONSTRAINT_TIMEOUT). Runs on every
    /// seed call, so graphs seeded before the constraint existed get it at the next startup.
    /// Fails (instead of running unguarded) on FAILED (existing data breaks it) or on timeout.
    async fn ensure_unique_constraint(
        &self,
        entity: EntityType,
        (rel, prop): (&str, &str),
    ) -> Result<(), String> {
        let mut graph = self.graph_handle();
        let deadline = Instant::now() + CONSTRAINT_TIMEOUT;
        let mut delay = CONSTRAINT_POLL;
        loop {
            let status = graph
                .list_constraints()
                .await
                .map_err(|e| format!("listing constraints failed: {e}"))?
                .data
                .into_iter()
                .find(|c| {
                    c.constraint_type == ConstraintType::Unique
                        && c.entity_type == entity
                        && c.label == rel
                        && c.properties == [prop]
                })
                .map(|c| c.status);
            let Some(status) = status else {
                return graph
                    .create_unique_constraint_op(entity, rel, &[prop])
                    .wait_with(WaitOptions::with_timeout(CONSTRAINT_TIMEOUT))
                    .await
                    .map_err(|e| match e {
                        FalkorDBError::ConstraintFailed { .. } => {
                            constraint_failed_message(rel, prop)
                        }
                        e => format!("creating unique constraint on {rel}.{prop} failed: {e}"),
                    });
            };
            if constraint_readiness(status, (rel, prop))? == Readiness::Ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "unique constraint on {rel}.{prop} still UNDER CONSTRUCTION after {CONSTRAINT_TIMEOUT:?}"
                ));
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 3 / 2).min(CONSTRAINT_MAX_POLL);
        }
    }

    async fn count(&self, query: &str) -> Result<i64, String> {
        let rows = self.rows(query, vec![]).await?;
        let row = rows
            .first()
            .ok_or_else(|| format!("count query returned no row: {query}"))?;
        col(row, "total")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_splits_statements() {
        let stmts =
            seed_statements("// header\nCREATE (a)\n// mid\n, (b);\n\nMATCH (n) RETURN n;\n");
        assert_eq!(
            stmts,
            vec![
                "CREATE (a)\n, (b)".to_string(),
                "MATCH (n) RETURN n".to_string()
            ]
        );
    }

    #[test]
    fn constraint_status_maps_to_ready_wait_or_error() {
        let key = queries::SUPERSEDES_CONSTRAINT;
        assert_eq!(
            constraint_readiness(ConstraintStatus::Active, key),
            Ok(Readiness::Ready)
        );
        assert_eq!(
            constraint_readiness(ConstraintStatus::Pending, key),
            Ok(Readiness::Pending)
        );
        let err = constraint_readiness(ConstraintStatus::Failed, key).unwrap_err();
        assert!(
            err.contains("reseed") && err.contains("SUPERSEDES"),
            "{err}"
        );
    }

    #[test]
    fn embedded_seed_is_a_single_create() {
        let stmts = seed_statements(SEED_CYPHER);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].starts_with("CREATE"));
    }
}
