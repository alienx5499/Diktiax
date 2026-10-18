//! Deterministic, graph-driven planner. Every decision is derived from the CONTENT of prior
//! tool results: the explored neighbourhood decides which detectors are worth running, matched
//! evidence decides which shops get a centrality check, and the claim's algo.WCC ring component
//! decides which linked claims get corroboration runs.
//!
//! Detector gating only uses NECESSARY conditions of each detector's Cypher pattern that are
//! fully visible in a depth-3 neighbourhood, so a skipped detector could never have matched.

use super::planner::*;
use crate::domain::{FraudPatternMatch, RingComponent, SubgraphPayload};
use crate::tools::{
    DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE, DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING,
    EXPLORE_CLAIM_SUBGRAPH, LIST_CLAIM_SHOPS, RING_COMPONENTS, SHOP_CENTRALITY,
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Depth at which shared contacts, co-located accidents and VINs become visible (3 hops).
const EXPLORE_DEPTH: u8 = 3;

/// `remembered`: linked claims the latest prior verdict RELIED_ON. Corroboration runs on them
/// are skipped (recorded in `skipped`); calls on the target claim are never filtered, so the
/// score cannot change.
#[derive(Default)]
pub struct RuleBasedPlanner {
    remembered: Vec<String>,
    skipped: Vec<PlannedCall>,
}

impl RuleBasedPlanner {
    pub fn remembering(remembered: Vec<String>) -> Self {
        Self {
            remembered,
            skipped: Vec::new(),
        }
    }

    /// Corroboration calls not made because a prior verdict already relied on their claim.
    pub fn skipped(&self) -> &[PlannedCall] {
        &self.skipped
    }

    /// Drops calls on remembered linked claims (any planner's calls), recording each once.
    pub fn skip_remembered(&mut self, claim_id: &str, calls: Vec<PlannedCall>) -> Vec<PlannedCall> {
        let (skip, keep): (Vec<_>, Vec<_>) = calls.into_iter().partition(|c| {
            c.claim_arg()
                .is_some_and(|id| id != claim_id && self.remembered.iter().any(|r| r == id))
        });
        for call in skip {
            let seen = self
                .skipped
                .iter()
                .any(|s| s.tool == call.tool && s.claim_arg() == call.claim_arg());
            if !seen {
                self.skipped.push(call);
            }
        }
        keep
    }
}

impl Planner for RuleBasedPlanner {
    async fn plan(
        &mut self,
        claim_id: &str,
        history: &[ToolOutcome],
    ) -> Result<Vec<PlannedCall>, String> {
        Ok(self.skip_remembered(claim_id, next_calls(claim_id, history)))
    }
}

/// Pure policy: history in, next batch out.
pub fn next_calls(claim_id: &str, history: &[ToolOutcome]) -> Vec<PlannedCall> {
    let Some(explored) = outcome(history, EXPLORE_CLAIM_SUBGRAPH, claim_id) else {
        return vec![PlannedCall::new(
            EXPLORE_CLAIM_SUBGRAPH,
            json!({ "claim_id": claim_id, "depth": EXPLORE_DEPTH }),
            format!("Map the {EXPLORE_DEPTH}-hop neighbourhood of {claim_id} to decide which fraud detectors apply."),
        )];
    };
    // Without a neighbourhood there is nothing to reason about; the error stays in the trace.
    let Some(graph) = explored
        .result
        .as_ref()
        .ok()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    else {
        return Vec::new();
    };
    let view = GraphView::new(claim_id, &graph);

    let detectors: Vec<PlannedCall> = view
        .applicable_detectors()
        .into_iter()
        .filter(|call| outcome(history, &call.tool, claim_id).is_none())
        .collect();
    if !detectors.is_empty() {
        return detectors;
    }

    let matched = matches_for(history, claim_id);
    if matched.is_empty() {
        return Vec::new(); // clean neighbourhood: nothing further to corroborate
    }

    let mut shops = view.shops_in(&matched);
    if let Some(listed) = outcome(history, LIST_CLAIM_SHOPS, claim_id) {
        if let Ok(Value::Array(ids)) = &listed.result {
            shops.extend(ids.iter().filter_map(Value::as_str).map(str::to_string));
        }
    } else if shops.is_empty() {
        return vec![PlannedCall::new(
            LIST_CLAIM_SHOPS,
            json!({ "claim_id": claim_id }),
            "Matched patterns name no repair shop; list the claim's shops to check for a hub."
                .into(),
        )];
    }
    let centrality: Vec<PlannedCall> = shops
        .into_iter()
        .filter(|shop| !ran_with(history, SHOP_CENTRALITY, "shop_id", shop))
        .map(|shop| {
            let rationale = format!(
                "Shop {shop} appears in the fraud evidence; measure its claim concentration."
            );
            PlannedCall::new(SHOP_CENTRALITY, json!({ "shop_id": shop }), rationale)
        })
        .collect();
    if !centrality.is_empty() {
        return centrality;
    }

    let corroborate = corroboratable(history, claim_id);
    if corroborate.is_empty() {
        return Vec::new();
    }
    let Some(scope) = outcome(history, RING_COMPONENTS, claim_id) else {
        return vec![PlannedCall::new(
            RING_COMPONENTS,
            json!({ "claim_id": claim_id }),
            format!("Patterns matched on {claim_id}; find its WCC ring component to pick which linked claims to corroborate."),
        )];
    };
    match scope
        .result
        .as_ref()
        .ok()
        .and_then(|v| serde_json::from_value::<RingComponent>(v.clone()).ok())
    {
        Some(ring) => corroborate_in_ring(&ring, &matched, &corroborate, history),
        // WCC failed: fall back to the first claim linked through shared contacts.
        None => follow_linked_claim(&view, history),
    }
}

/// Matched target detectors worth re-running from a linked claim's side. Shared contacts is
/// symmetric, so re-running it from the peer adds nothing.
fn corroboratable<'h>(history: &'h [ToolOutcome], claim_id: &str) -> Vec<&'h ToolOutcome> {
    history
        .iter()
        .filter(|o| o.call.claim_arg() == Some(claim_id) && o.call.tool != DETECT_SHARED_CONTACTS)
        .filter(|o| {
            DETECTORS.contains(&o.call.tool.as_str()) && matches!(&o.result, Ok(v) if !v.is_null())
        })
        .collect()
}

/// One corroboration run per matched detector (the same cap as following a single linked
/// claim). Candidates are ring members (same WCC component) that the evidence names, this
/// detector's own path first; each detector takes a member no earlier detector took, so the
/// runs spread across the ring. A detector already run on any candidate is done.
fn corroborate_in_ring(
    ring: &RingComponent,
    matched: &[FraudPatternMatch],
    detectors: &[&ToolOutcome],
    history: &[ToolOutcome],
) -> Vec<PlannedCall> {
    let claim_id = ring.claim_id.as_str();
    let mut taken: Vec<&str> = Vec::new();
    let mut calls = Vec::new();
    for o in detectors {
        let tool = o.call.tool.as_str();
        let own = o
            .result
            .as_ref()
            .ok()
            .and_then(|v| serde_json::from_value::<FraudPatternMatch>(v.clone()).ok())
            .map(|m| m.evidence_path)
            .unwrap_or_default();
        let mut candidates: Vec<&str> = Vec::new();
        for id in own
            .iter()
            .chain(matched.iter().flat_map(|m| &m.evidence_path))
        {
            let member = ring.claims.iter().find(|c| *c == id && *c != claim_id);
            if let Some(member) = member.filter(|m| !candidates.contains(&m.as_str())) {
                candidates.push(member);
            }
        }
        if candidates
            .iter()
            .any(|id| outcome(history, tool, id).is_some())
        {
            continue;
        }
        let Some(pick) = candidates
            .iter()
            .find(|id| !taken.contains(id))
            .or(candidates.first())
            .copied()
        else {
            continue;
        };
        taken.push(pick);
        let rationale = format!(
            "{pick} is in {claim_id}'s WCC ring component ({} claims) and named in the evidence; check whether {tool} also holds from its side.",
            ring.size
        );
        calls.push(PlannedCall::new(
            tool,
            json!({ "claim_id": pick }),
            rationale,
        ));
    }
    calls
}

/// Re-runs the matched ring detectors from the perspective of the first claim linked through
/// shared contacts, to corroborate the ring (corroboration is not scored for the target).
fn follow_linked_claim(view: &GraphView, history: &[ToolOutcome]) -> Vec<PlannedCall> {
    let claim_id = view.claim_id;
    let contacts = outcome(history, DETECT_SHARED_CONTACTS, claim_id)
        .and_then(|o| o.result.as_ref().ok())
        .and_then(|v| serde_json::from_value::<FraudPatternMatch>(v.clone()).ok());
    // Customer, contact and peer sit within 3 hops and are labelled in the view; the peer's
    // claim is either labelled `Claim` or lies beyond the explored depth.
    let Some(linked) = contacts.as_ref().and_then(|m| {
        m.evidence_path
            .iter()
            .find(|id| *id != claim_id && matches!(view.label(id), None | Some("Claim")))
    }) else {
        return Vec::new();
    };
    corroboratable(history, claim_id)
        .into_iter()
        .filter(|o| outcome(history, &o.call.tool, linked).is_none())
        .map(|o| {
            let rationale = format!(
                "{linked} is tied to {claim_id} through shared contacts; check whether {} also holds from its side.",
                o.call.tool
            );
            PlannedCall::new(&o.call.tool, json!({ "claim_id": linked }), rationale)
        })
        .collect()
}

/// Labels and edges of the explored neighbourhood, indexed for gating decisions.
struct GraphView<'a> {
    claim_id: &'a str,
    graph: &'a SubgraphPayload,
    labels: HashMap<&'a str, &'a str>,
}

impl<'a> GraphView<'a> {
    fn new(claim_id: &'a str, graph: &'a SubgraphPayload) -> Self {
        let labels = graph
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        Self {
            claim_id,
            graph,
            labels,
        }
    }

    fn label(&self, id: &str) -> Option<&str> {
        self.labels.get(id).copied()
    }

    fn incoming(&self, node: &str, rel: &[&str]) -> usize {
        self.graph
            .edges
            .iter()
            .filter(|e| e.to == node && rel.contains(&e.label.as_str()))
            .count()
    }

    fn target_edges(&self, rel: &'static str) -> impl Iterator<Item = &'a str> + '_ {
        self.graph
            .edges
            .iter()
            .filter(move |e| e.from == self.claim_id && e.label == rel)
            .map(|e| e.to.as_str())
    }

    fn applicable_detectors(&self) -> Vec<PlannedCall> {
        let id = self.claim_id;
        let call = |tool: &str, why: String| PlannedCall::new(tool, json!({ "claim_id": id }), why);
        let mut calls = Vec::new();

        let shared_location = self
            .graph
            .nodes
            .iter()
            .find(|n| n.label == "Location" && self.incoming(&n.id, &["OCCURRED_AT"]) >= 2);
        if let (Some(loc), Some(shop)) = (shared_location, self.target_edges("REPAIRED_AT").next())
        {
            calls.push(call(DETECT_GHOST_COLLISION, format!(
                "Location {} hosts several accidents and the claim was repaired at {shop}: test for a staged ring.", loc.id
            )));
        }
        let shared_contact = self.graph.nodes.iter().find(|n| {
            matches!(n.label.as_str(), "Phone" | "Address")
                && self.incoming(&n.id, &["HAS_PHONE", "LIVES_AT"]) >= 2
        });
        if let Some(contact) = shared_contact {
            calls.push(call(
                DETECT_SHARED_CONTACTS,
                format!(
                    "{} '{}' is shared by several customers: test the contact network.",
                    contact.label, contact.title
                ),
            ));
        }
        if let Some(vin) = self.graph.nodes.iter().find(|n| n.label == "VIN") {
            calls.push(call(
                DETECT_VIN_CLONING,
                format!(
                    "Vehicle VIN {} is in the neighbourhood: check for cloned registrations.",
                    vin.id
                ),
            ));
        }
        let clinic = self.target_edges("TREATED_AT").next();
        let adjuster = self.target_edges("ASSIGNED_TO").next();
        if let (Some(clinic), Some(adjuster), true) = (clinic, adjuster, self.unreported_accident())
        {
            calls.push(call(DETECT_KICKBACK_TRIANGLE, format!(
                "No police report, and adjuster {adjuster} + clinic {clinic} are attached: test for a kickback triangle."
            )));
        }
        calls
    }

    /// The accident that triggered the target claim has `police_report_filed = false`.
    fn unreported_accident(&self) -> bool {
        self.graph
            .edges
            .iter()
            .filter(|e| e.to == self.claim_id && e.label == "TRIGGERED_CLAIM")
            .any(|e| {
                self.graph.nodes.iter().any(|n| {
                    n.id == e.from
                        && n.properties.get("police_report_filed") == Some(&Value::Bool(false))
                })
            })
    }

    /// RepairShop node ids appearing in matched evidence paths, first-seen order.
    fn shops_in(&self, matched: &[FraudPatternMatch]) -> Vec<String> {
        let mut shops: Vec<String> = Vec::new();
        for id in matched.iter().flat_map(|m| &m.evidence_path) {
            if self.label(id) == Some("RepairShop") && !shops.contains(id) {
                shops.push(id.clone());
            }
        }
        shops
    }
}

fn outcome<'h>(history: &'h [ToolOutcome], tool: &str, claim_id: &str) -> Option<&'h ToolOutcome> {
    history
        .iter()
        .find(|o| o.call.tool == tool && o.call.claim_arg() == Some(claim_id))
}

fn ran_with(history: &[ToolOutcome], tool: &str, key: &str, value: &str) -> bool {
    history.iter().any(|o| {
        o.call.tool == tool && o.call.arguments.get(key).and_then(Value::as_str) == Some(value)
    })
}

fn matches_for(history: &[ToolOutcome], claim_id: &str) -> Vec<FraudPatternMatch> {
    history
        .iter()
        .filter(|o| {
            DETECTORS.contains(&o.call.tool.as_str()) && o.call.claim_arg() == Some(claim_id)
        })
        .filter_map(|o| o.result.as_ref().ok())
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

#[cfg(test)]
#[path = "rule_planner_tests.rs"]
mod tests;
