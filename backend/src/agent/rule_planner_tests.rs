use super::*;
use serde_json::json;

fn node(id: &str, label: &str, props: Value) -> Value {
    json!({ "id": id, "label": label, "title": id, "group": "g", "properties": props })
}

fn edge(from: &str, to: &str, label: &str) -> Value {
    json!({ "id": format!("{from}-{label}-{to}"), "from": from, "to": to, "label": label, "properties": {} })
}

fn done(tool: &str, args: Value, result: Result<Value, String>) -> ToolOutcome {
    ToolOutcome {
        call: PlannedCall::new(tool, args, String::new()),
        result,
        latency_ms: 1.0,
    }
}

/// Ring-shaped neighbourhood: shared phone, co-located accident, clinic+adjuster, no police report.
fn ring_graph() -> Value {
    json!({
        "nodes": [
            node("C1", "Claim", json!({})), node("C2", "Claim", json!({})),
            node("A1", "Accident", json!({ "police_report_filed": false })), node("A2", "Accident", json!({})),
            node("L1", "Location", json!({})), node("S1", "RepairShop", json!({})),
            node("M1", "Clinic", json!({})), node("J1", "Adjuster", json!({})),
            node("U1", "Customer", json!({})), node("U2", "Customer", json!({})),
            node("P1", "Phone", json!({})), node("V1", "VIN", json!({})),
        ],
        "edges": [
            edge("A1", "C1", "TRIGGERED_CLAIM"), edge("A1", "L1", "OCCURRED_AT"), edge("A2", "L1", "OCCURRED_AT"),
            edge("C1", "S1", "REPAIRED_AT"), edge("C2", "S1", "REPAIRED_AT"),
            edge("C1", "M1", "TREATED_AT"), edge("C1", "J1", "ASSIGNED_TO"),
            edge("U1", "C1", "FILED_CLAIM"), edge("U1", "P1", "HAS_PHONE"), edge("U2", "P1", "HAS_PHONE"),
        ]
    })
}

/// Clean neighbourhood: unique contacts, police report, no clinic, single accident at location.
fn clean_graph() -> Value {
    json!({
        "nodes": [
            node("C9", "Claim", json!({})), node("A9", "Accident", json!({ "police_report_filed": true })),
            node("L9", "Location", json!({})), node("S9", "RepairShop", json!({})),
            node("U9", "Customer", json!({})), node("P9", "Phone", json!({})),
        ],
        "edges": [
            edge("A9", "C9", "TRIGGERED_CLAIM"), edge("A9", "L9", "OCCURRED_AT"), edge("C9", "S9", "REPAIRED_AT"),
            edge("U9", "C9", "FILED_CLAIM"), edge("U9", "P9", "HAS_PHONE"),
        ]
    })
}

fn tools(calls: &[PlannedCall]) -> Vec<&str> {
    calls.iter().map(|c| c.tool.as_str()).collect()
}

fn pattern(name: &str, path: &[&str]) -> Value {
    json!({ "pattern_name": name, "severity": "CRITICAL", "risk_points": 40, "description": "d",
            "cypher_query": "MATCH", "evidence_path": path })
}

#[test]
fn starts_by_exploring() {
    let calls = next_calls("C1", &[]);
    assert_eq!(tools(&calls), vec![EXPLORE_CLAIM_SUBGRAPH]);
    assert_eq!(calls[0].arguments["depth"], 3);
}

#[test]
fn ring_topology_selects_all_matching_detectors() {
    let h = vec![done(
        EXPLORE_CLAIM_SUBGRAPH,
        json!({ "claim_id": "C1" }),
        Ok(ring_graph()),
    )];
    assert_eq!(tools(&next_calls("C1", &h)), DETECTORS.to_vec());
}

#[test]
fn clean_topology_skips_detectors_whose_preconditions_are_absent() {
    let h = vec![done(
        EXPLORE_CLAIM_SUBGRAPH,
        json!({ "claim_id": "C9" }),
        Ok(clean_graph()),
    )];
    assert!(
        next_calls("C9", &h).is_empty(),
        "no shared location/contact, VIN, or clinic -> nothing to run"
    );
}

/// The planner has nothing to gate on, so it stops; the investigator then reports the target
/// as INCONCLUSIVE / NEEDS_MANUAL_REVIEW, never cleared (see `verdict` tests).
#[test]
fn failed_exploration_stops_planning() {
    let h = vec![done(
        EXPLORE_CLAIM_SUBGRAPH,
        json!({ "claim_id": "C1" }),
        Err("boom".into()),
    )];
    assert!(next_calls("C1", &h).is_empty());
}

#[test]
fn matched_evidence_drives_centrality_then_linked_claim() {
    let c1 = json!({ "claim_id": "C1" });
    let mut h = vec![
        done(EXPLORE_CLAIM_SUBGRAPH, c1.clone(), Ok(ring_graph())),
        done(
            DETECT_GHOST_COLLISION,
            c1.clone(),
            Ok(pattern("Ghost", &["C1", "L1", "S1", "C2"])),
        ),
        done(
            DETECT_SHARED_CONTACTS,
            c1.clone(),
            Ok(pattern("Contacts", &["C1", "U1", "P1", "U2", "C2"])),
        ),
        done(DETECT_VIN_CLONING, c1.clone(), Ok(Value::Null)),
        done(DETECT_KICKBACK_TRIANGLE, c1.clone(), Err("db down".into())),
    ];
    let calls = next_calls("C1", &h);
    assert_eq!(tools(&calls), vec![SHOP_CENTRALITY]);
    assert_eq!(calls[0].arguments["shop_id"], "S1");

    h.push(done(
        SHOP_CENTRALITY,
        json!({ "shop_id": "S1" }),
        Ok(json!({})),
    ));
    assert_eq!(tools(&next_calls("C1", &h)), vec![RING_COMPONENTS]);
    h.push(done(RING_COMPONENTS, c1.clone(), Err("timeout".into())));
    let linked = next_calls("C1", &h);
    assert_eq!(
        tools(&linked),
        vec![DETECT_GHOST_COLLISION],
        "WCC failed: only matched, non-contact detectors are re-run on the contact peer"
    );
    assert_eq!(linked[0].arguments["claim_id"], "C2");

    h.push(done(
        DETECT_GHOST_COLLISION,
        json!({ "claim_id": "C2" }),
        Ok(Value::Null),
    ));
    assert!(next_calls("C1", &h).is_empty());
}

#[test]
fn evidence_without_shop_lists_claim_shops() {
    let c1 = json!({ "claim_id": "C1" });
    let mut h = vec![done(EXPLORE_CLAIM_SUBGRAPH, c1.clone(), Ok(ring_graph()))];
    for tool in DETECTORS {
        let result = if tool == DETECT_VIN_CLONING {
            pattern("Vin", &["C1", "V1", "X9"])
        } else {
            Value::Null
        };
        h.push(done(tool, c1.clone(), Ok(result)));
    }
    assert_eq!(tools(&next_calls("C1", &h)), vec![LIST_CLAIM_SHOPS]);
    h.push(done(LIST_CLAIM_SHOPS, c1, Ok(json!(["S1"]))));
    assert_eq!(tools(&next_calls("C1", &h)), vec![SHOP_CENTRALITY]);
}

#[tokio::test]
async fn remembered_linked_claim_skips_corroboration_only() {
    let c1 = json!({ "claim_id": "C1" });
    let h = vec![
        done(EXPLORE_CLAIM_SUBGRAPH, c1.clone(), Ok(ring_graph())),
        done(
            DETECT_GHOST_COLLISION,
            c1.clone(),
            Ok(pattern("Ghost", &["C1", "L1", "S1", "C2"])),
        ),
        done(
            DETECT_SHARED_CONTACTS,
            c1.clone(),
            Ok(pattern("Contacts", &["C1", "U1", "P1", "U2", "C2"])),
        ),
        done(DETECT_VIN_CLONING, c1.clone(), Ok(Value::Null)),
        done(DETECT_KICKBACK_TRIANGLE, c1.clone(), Ok(Value::Null)),
        done(SHOP_CENTRALITY, json!({ "shop_id": "S1" }), Ok(json!({}))),
    ];
    // Target-claim calls pass through even when the target itself is "remembered".
    let mut planner = RuleBasedPlanner::remembering(vec!["C1".into(), "C2".into()]);
    assert_eq!(
        tools(&planner.plan("C1", &h[..1]).await.unwrap()),
        DETECTORS.to_vec()
    );
    assert_eq!(
        tools(&planner.plan("C1", &h).await.unwrap()),
        vec![RING_COMPONENTS],
        "the ring is re-read: a member that joined since the last verdict would be corroborated"
    );
    let mut h = h;
    h.push(done(RING_COMPONENTS, c1, ring(&["C1", "C2"])));
    for _ in 0..2 {
        assert!(planner.plan("C1", &h).await.unwrap().is_empty());
    }
    assert_eq!(tools(planner.skipped()), vec![DETECT_GHOST_COLLISION]);
    assert_eq!(planner.skipped()[0].claim_arg(), Some("C2"));

    let mut fresh = RuleBasedPlanner::default();
    assert_eq!(
        tools(&fresh.plan("C1", &h).await.unwrap()),
        vec![DETECT_GHOST_COLLISION]
    );
    assert!(fresh.skipped().is_empty());
}

fn ring(claims: &[&str]) -> Result<Value, String> {
    Ok(json!({ "claim_id": "C1", "claims": claims, "size": claims.len() }))
}

/// Ghost and kickback matched; the ring component decides who corroborates each of them.
fn ring_history(component: Result<Value, String>) -> Vec<ToolOutcome> {
    let c1 = json!({ "claim_id": "C1" });
    vec![
        done(EXPLORE_CLAIM_SUBGRAPH, c1.clone(), Ok(ring_graph())),
        done(
            DETECT_GHOST_COLLISION,
            c1.clone(),
            Ok(pattern("Ghost", &["C1", "L1", "S1", "C2", "C3"])),
        ),
        done(
            DETECT_SHARED_CONTACTS,
            c1.clone(),
            Ok(pattern("Contacts", &["C1", "U1", "P1", "U2", "C2"])),
        ),
        done(DETECT_VIN_CLONING, c1.clone(), Ok(Value::Null)),
        done(
            DETECT_KICKBACK_TRIANGLE,
            c1.clone(),
            Ok(pattern("Kickback", &["C1", "J1", "S1", "M1", "C2", "C4"])),
        ),
        done(SHOP_CENTRALITY, json!({ "shop_id": "S1" }), Ok(json!({}))),
        done(RING_COMPONENTS, c1, component),
    ]
}

fn claim_args(calls: &[PlannedCall]) -> Vec<(&str, &str)> {
    calls
        .iter()
        .map(|c| (c.tool.as_str(), c.claim_arg().unwrap_or("")))
        .collect()
}

#[test]
fn ring_component_spreads_corroboration_across_members() {
    let mut h = ring_history(ring(&["C1", "C2", "C3"]));
    let calls = next_calls("C1", &h);
    assert_eq!(
        claim_args(&calls),
        vec![
            (DETECT_GHOST_COLLISION, "C2"),
            (DETECT_KICKBACK_TRIANGLE, "C3")
        ],
        "C4 is outside the component; C3 is the next untaken ring member"
    );
    for call in calls {
        h.push(done(&call.tool, call.arguments, Ok(Value::Null)));
    }
    assert!(
        next_calls("C1", &h).is_empty(),
        "one run per matched detector"
    );
}

#[test]
fn ring_component_limits_corroboration_to_its_members() {
    let calls = next_calls("C1", &ring_history(ring(&["C1", "C4"])));
    assert_eq!(
        claim_args(&calls),
        vec![
            (DETECT_GHOST_COLLISION, "C4"),
            (DETECT_KICKBACK_TRIANGLE, "C4")
        ],
        "C2 and C3 are outside the component; both detectors share its one evidence member"
    );
    assert!(next_calls("C1", &ring_history(ring(&["C1"]))).is_empty());
}
