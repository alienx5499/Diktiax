//! External `investigate_claim`: reads the claim's investigation history first and returns the
//! latest recorded verdict instead of re-running the agent, unless the caller passes `force`.

use crate::agent::ClaimInvestigator;
use crate::db::FalkorGraphRepository;
use crate::tools::required_str;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type ClaimLock = Arc<tokio::sync::Mutex<()>>;

pub struct InvestigateClaim {
    repo: FalkorGraphRepository,
    investigator: ClaimInvestigator,
    /// One lock per claim currently being handled; entries are removed when no caller holds them,
    /// so the map stays bounded by the number of in-flight claims.
    locks: Mutex<HashMap<String, ClaimLock>>,
}

impl InvestigateClaim {
    pub fn new(repo: FalkorGraphRepository, investigator: ClaimInvestigator) -> Self {
        Self {
            repo,
            investigator,
            locks: Mutex::default(),
        }
    }

    /// `{claim_id, force?}`. Without `force`, an existing verdict comes back as
    /// `{reused: true, claim_id, certificate_id, decision, risk_score, recorded_at}` with nothing
    /// written; otherwise the full dossier of a fresh investigation.
    pub async fn call(&self, arguments: &Value) -> Result<Value, String> {
        let claim_id = required_str(arguments, "claim_id")?;
        let force = match arguments.get("force") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err("argument 'force' must be a boolean".into()),
        };
        let lock = self.claim_lock(&claim_id);
        // Held across history check -> investigation -> record, so concurrent callers on the
        // same claim see the first caller's verdict instead of investigating again.
        let outcome = {
            let _guard = lock.lock().await;
            self.investigate_or_reuse(&claim_id, force).await
        };
        self.release(&claim_id, lock);
        outcome
    }

    async fn investigate_or_reuse(&self, claim_id: &str, force: bool) -> Result<Value, String> {
        if !force {
            if let Some(head) = self.repo.investigation_history(claim_id).await?.first() {
                let latest = &head.record;
                return Ok(json!({
                    "reused": true,
                    "claim_id": claim_id,
                    "certificate_id": latest.certificate_id,
                    "decision": latest.decision,
                    "risk_score": latest.risk_score,
                    "recorded_at": latest.recorded_at,
                }));
            }
        }
        let dossier = self.investigator.investigate(claim_id).await?;
        serde_json::to_value(dossier).map_err(|e| format!("failed to serialize dossier: {e}"))
    }

    fn claim_lock(&self, claim_id: &str) -> ClaimLock {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        locks.entry(claim_id.to_string()).or_default().clone()
    }

    /// Drops the map entry once the last caller for this claim is done. Clones are only taken
    /// under the map mutex, so the count cannot rise while it is checked here.
    fn release(&self, claim_id: &str, lock: ClaimLock) {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        if Arc::strong_count(&lock) == 2 {
            locks.remove(claim_id);
        }
    }

    pub fn tracked_claims(&self) -> usize {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}
