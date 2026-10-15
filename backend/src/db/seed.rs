//! Seeding: indexes + data/seed.cypher (embedded at compile time, cwd-independent).

use super::mapping::col;
use super::queries;
use super::repository::FalkorGraphRepository;
use serde::{Deserialize, Serialize};
use tracing::info;

const SEED_CYPHER: &str = include_str!("../../../data/seed.cypher");

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
            return Ok(SeedReport {
                applied: false,
                nodes: self.count(queries::COUNT_NODES).await?,
                edges: self.count(queries::COUNT_EDGES).await?,
            });
        }
        self.create_indexes().await?;
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
    fn embedded_seed_is_a_single_create() {
        let stmts = seed_statements(SEED_CYPHER);
        assert_eq!(stmts.len(), 1);
        assert!(stmts[0].starts_with("CREATE"));
    }
}
