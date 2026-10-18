//! Cypher statements. Every caller-supplied value is a `$param`; the only formatted value is
//! the clamped subgraph depth (see [`claim_subgraph`]).

pub const PING: &str = "RETURN 1 AS ok";

pub const INDEXES: &[&str] = &[
    "CREATE INDEX FOR (n:Claim) ON (n.id)",
    "CREATE INDEX FOR (n:Customer) ON (n.id)",
    "CREATE INDEX FOR (n:Vehicle) ON (n.id)",
    "CREATE INDEX FOR (n:VIN) ON (n.vin)",
    "CREATE INDEX FOR (n:Phone) ON (n.number)",
    "CREATE INDEX FOR (n:Address) ON (n.line)",
    "CREATE INDEX FOR (n:Accident) ON (n.id)",
    "CREATE INDEX FOR (n:Location) ON (n.id)",
    "CREATE INDEX FOR (n:RepairShop) ON (n.id)",
    "CREATE INDEX FOR (n:Clinic) ON (n.id)",
    "CREATE INDEX FOR (n:Adjuster) ON (n.id)",
    "CREATE INDEX FOR (n:Investigation) ON (n.id)",
    "CREATE INDEX FOR (n:Investigator) ON (n.id)",
];

pub const COUNT_NODES: &str = "MATCH (n) RETURN count(n) AS total";
pub const COUNT_EDGES: &str = "MATCH ()-[r]->() RETURN count(r) AS total";

const CLAIM_SUMMARY_RETURN: &str = "RETURN c.id AS claim_id, cust.name AS claimant_name, \
     c.amount_claimed AS amount, c.status AS status, c.date_filed AS filed_at";

pub fn list_claims() -> String {
    format!("MATCH (cust:Customer)-[:FILED_CLAIM]->(c:Claim) {CLAIM_SUMMARY_RETURN} ORDER BY c.date_filed DESC")
}

pub fn get_claim() -> String {
    format!(
        "MATCH (cust:Customer)-[:FILED_CLAIM]->(c:Claim {{id: $claim_id}}) {CLAIM_SUMMARY_RETURN}"
    )
}

/// Node and edge columns are id/labels/props scalars, not entities: see `read_rows`.
pub const CLAIM_NODE: &str = "MATCH (c:Claim {id: $claim_id}) \
     RETURN id(c) AS node_id, labels(c) AS node_labels, properties(c) AS node_props";

pub const CLAIM_SHOPS: &str =
    "MATCH (:Claim {id: $claim_id})-[:REPAIRED_AT]->(s:RepairShop) RETURN s.id AS shop_id ORDER BY shop_id";

pub const MIN_DEPTH: u8 = 1;
pub const MAX_DEPTH: u8 = 4;

/// Variable-length neighbourhood of a claim. `depth` is clamped to `MIN_DEPTH..=MAX_DEPTH`
/// before being formatted into the path bound: FalkorDB rejects a parameter there (`[*1..$depth]`
/// fails with "unhandled type in inlined properties" on v4.22.1, FalkorDB/FalkorDB#3350).
pub fn claim_subgraph(depth: u8) -> String {
    let depth = depth.clamp(MIN_DEPTH, MAX_DEPTH);
    format!(
        "MATCH p = (:Claim {{id: $claim_id}})-[*1..{depth}]-() \
         UNWIND relationships(p) AS r WITH DISTINCT r \
         WITH r, startNode(r) AS s, endNode(r) AS d \
         RETURN id(s) AS src_id, labels(s) AS src_labels, properties(s) AS src_props, \
                id(r) AS rel_id, type(r) AS rel_type, properties(r) AS rel_props, \
                id(d) AS dst_id, labels(d) AS dst_labels, properties(d) AS dst_props"
    )
}

/// Claims at the same accident location, repaired at the same shop, filed within a window.
pub const GHOST_COLLISION: &str = "MATCH (c:Claim {id: $claim_id})<-[:TRIGGERED_CLAIM]-(:Accident)-[:OCCURRED_AT]->(loc:Location), \
     (c)-[:REPAIRED_AT]->(shop:RepairShop), \
     (loc)<-[:OCCURRED_AT]-(:Accident)-[:TRIGGERED_CLAIM]->(other:Claim)-[:REPAIRED_AT]->(shop) \
     WHERE other.id <> c.id AND abs(other.filed_day - c.filed_day) <= $window_days \
     RETURN c.id AS claim_id, loc.id AS location_id, shop.id AS shop_id, \
     other.id AS linked_claim_id, abs(other.filed_day - c.filed_day) AS days_apart \
     ORDER BY days_apart, linked_claim_id";

/// Claimants of other claims who share a phone number or address with this claimant.
pub const SHARED_CONTACTS: &str = "MATCH (c:Claim {id: $claim_id})<-[:FILED_CLAIM]-(me:Customer)-[link:HAS_PHONE|LIVES_AT]->(contact) \
     <-[:HAS_PHONE|LIVES_AT]-(peer:Customer)-[:FILED_CLAIM]->(other:Claim) \
     WHERE peer.id <> me.id \
     RETURN c.id AS claim_id, me.id AS customer_id, type(link) AS via, \
     coalesce(contact.number, contact.line) AS contact, peer.id AS peer_id, \
     peer.name AS peer_name, other.id AS linked_claim_id \
     ORDER BY peer_id, linked_claim_id";

/// Other vehicle registrations carrying the same VIN that also produced claims.
pub const VIN_CLONING: &str = "MATCH (c:Claim {id: $claim_id})<-[:TRIGGERED_CLAIM]-(:Accident)<-[:INVOLVED_IN]-(v:Vehicle)-[:HAS_VIN]->(vin:VIN) \
     <-[:HAS_VIN]-(clone:Vehicle)-[:INVOLVED_IN]->(:Accident)-[:TRIGGERED_CLAIM]->(other:Claim) \
     WHERE other.id <> c.id \
     RETURN c.id AS claim_id, v.id AS vehicle_id, v.jurisdiction AS jurisdiction, vin.vin AS vin, \
     clone.id AS clone_vehicle_id, clone.jurisdiction AS clone_jurisdiction, other.id AS linked_claim_id \
     ORDER BY linked_claim_id";

/// Unreported accident whose adjuster, shop and clinic recur together on other claims.
pub const KICKBACK_TRIANGLE: &str = "MATCH (a:Accident)-[:TRIGGERED_CLAIM]->(c:Claim {id: $claim_id}), \
     (c)-[:REPAIRED_AT]->(shop:RepairShop), (c)-[:TREATED_AT]->(clinic:Clinic), (c)-[:ASSIGNED_TO]->(adj:Adjuster), \
     (other:Claim)-[:REPAIRED_AT]->(shop), (other)-[:TREATED_AT]->(clinic), (other)-[:ASSIGNED_TO]->(adj) \
     WHERE other.id <> c.id AND a.police_report_filed = false \
     OPTIONAL MATCH (shop)-[ref:COLLUDES_WITH]->(clinic) \
     RETURN c.id AS claim_id, adj.id AS adjuster_id, shop.id AS shop_id, clinic.id AS clinic_id, \
     other.id AS linked_claim_id, ref IS NOT NULL AS referral_link \
     ORDER BY linked_claim_id";

pub const SHOP_DEGREES: &str = "MATCH (s:RepairShop) OPTIONAL MATCH (s)<-[:REPAIRED_AT]-(c:Claim) \
     RETURN s.id AS shop_id, s.name AS shop_name, count(c) AS degree";

/// Files a claim. Shared entities are MERGEd on natural keys with exactly the labels and
/// relationship types the detectors traverse.
pub const INGEST_CLAIM: &str = "MERGE (phone:Phone {number: $phone}) \
     MERGE (addr:Address {line: $address}) \
     MERGE (vin:VIN {vin: $vin}) \
     MERGE (loc:Location {id: $location_id}) ON CREATE SET loc.name = $location_id \
     MERGE (shop:RepairShop {id: $repair_shop_id}) ON CREATE SET shop.name = $repair_shop_id \
     CREATE (cust:Customer {id: $customer_id, name: $claimant_name}), \
     (veh:Vehicle {id: $vehicle_id, jurisdiction: 'UNVERIFIED'}), \
     (acc:Accident {id: $accident_id, timestamp: $filed_at, police_report_filed: $police_report}), \
     (c:Claim {id: $claim_id, date_filed: $filed_at, filed_day: $filed_day, amount_claimed: $amount, status: 'SUBMITTED'}), \
     (cust)-[:HAS_PHONE]->(phone), (cust)-[:LIVES_AT]->(addr), (cust)-[:OWNS]->(veh), \
     (veh)-[:HAS_VIN]->(vin), (veh)-[:INVOLVED_IN]->(acc), (acc)-[:OCCURRED_AT]->(loc), \
     (acc)-[:TRIGGERED_CLAIM]->(c), (cust)-[:FILED_CLAIM]->(c), (c)-[:REPAIRED_AT]->(shop) \
     FOREACH (cid IN CASE WHEN $clinic_id IS NULL THEN [] ELSE [$clinic_id] END | \
       MERGE (m:Clinic {id: cid}) MERGE (c)-[:TREATED_AT]->(m)) \
     FOREACH (aid IN CASE WHEN $adjuster_id IS NULL THEN [] ELSE [$adjuster_id] END | \
       MERGE (j:Adjuster {id: aid}) MERGE (c)-[:ASSIGNED_TO]->(j)) \
     RETURN c.id AS claim_id, cust.name AS claimant_name, c.amount_claimed AS amount, \
     c.status AS status, c.date_filed AS filed_at";

pub const SET_CLAIM_STATUS: &str =
    "MATCH (c:Claim {id: $claim_id}) SET c.status = $status RETURN c.id AS claim_id";

/// Records a verdict and links it to the claim's latest one: the Investigation node and
/// HAS_INVESTIGATION edge are exactly what a first investigation has always written, plus
/// `(new)-[:SUPERSEDES {superseded_id}]->(previous head)` when a head exists, or a disconnected
/// InvestigationChain marker when it does not (nothing the dossier subgraph can reach).
/// The head is the claim's investigation nothing supersedes yet (newest first when older,
/// unchained verdicts exist).
///
/// One statement is not guaranteed to serialise on FalkorDB v4.22.1 (see ASSIGN_ONCE), so racing
/// runs may read the same head. SUPERSEDES_CONSTRAINT (no verdict superseded twice) and
/// CHAIN_ROOT_CONSTRAINT (one chain start per claim) reject the loser's commit, which rolls back
/// whole; the caller re-runs it against the new head. Result: one linear chain per claim.
/// Measured: without the constraints, 208 rounds x 16 concurrent runs on one claim never forked,
/// and with them 21 rounds never hit a violation; this statement's read-to-commit window is too
/// short to lose in practice. The constraints are what make the invariant a guarantee, and
/// `chain_constraints_reject_a_fork` proves each one rejects the fork it exists for.
pub const RECORD_CHAINED_INVESTIGATION: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:HAS_INVESTIGATION]->(prev:Investigation) \
     WHERE NOT ()-[:SUPERSEDES]->(prev) \
     WITH c, prev ORDER BY prev.recorded_at DESC, prev.id DESC LIMIT 1 \
     CREATE (c)-[:HAS_INVESTIGATION]->(i:Investigation {id: $certificate_id, decision: $decision, \
     risk_score: $risk_score, recorded_at: $recorded_at}) \
     FOREACH (p IN CASE WHEN prev IS NULL THEN [] ELSE [prev] END | \
       CREATE (i)-[:SUPERSEDES {superseded_id: p.id}]->(p)) \
     FOREACH (root IN CASE WHEN prev IS NULL THEN [1] ELSE [] END | \
       CREATE (:InvestigationChain {claim_id: c.id})) \
     RETURN i.id AS investigation_id";

/// Unique `(relationship type, property)`: a verdict is superseded at most once.
pub const SUPERSEDES_CONSTRAINT: (&str, &str) = ("SUPERSEDES", "superseded_id");

/// Unique `(label, property)`: one chain start (first recorded verdict) per claim.
pub const CHAIN_ROOT_CONSTRAINT: (&str, &str) = ("InvestigationChain", "claim_id");

/// `(investigation)-[:RELIED_ON]->(claim)` for each listed id that is an existing Claim (other
/// ids, e.g. shops or phones from an evidence path, match nothing). Returns the edge count.
pub const LINK_RELIED_ON: &str = "MATCH (i:Investigation {id: $investigation_id}) \
     UNWIND $claim_ids AS cid \
     MATCH (l:Claim {id: cid}) \
     CREATE (i)-[:RELIED_ON]->(l) \
     RETURN count(l) AS linked";

/// Every recorded verdict for a claim, walked along the SUPERSEDES chain from the head (the
/// verdict nothing supersedes) back to the root, with the verdict each one supersedes and the
/// claims it RELIED_ON. Chain order is newest first, since each run supersedes the head it
/// read. Verdicts recorded before chaining existed are heads of one-entry chains, listed after
/// newer heads. `*0..1000` is a literal bound (a parameter is not accepted there on v4.22.1).
/// An unknown claim yields no rows; a known claim with no investigations yields one row of
/// nulls (OPTIONAL MATCH). Scalars and lists only, so it runs through the read-only lane.
pub const INVESTIGATION_HISTORY: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:HAS_INVESTIGATION]->(head:Investigation) \
     WHERE NOT ()-[:SUPERSEDES]->(head) \
     OPTIONAL MATCH p = (head)-[:SUPERSEDES*0..1000]->(i:Investigation) \
     OPTIONAL MATCH (i)-[:SUPERSEDES]->(prev:Investigation) \
     OPTIONAL MATCH (i)-[:RELIED_ON]->(l:Claim) \
     WITH head.recorded_at AS head_at, head.id AS head_id, length(p) AS depth, i, prev, \
     collect(l.id) AS relied_on \
     RETURN i.id AS certificate_id, i.decision AS decision, i.risk_score AS risk_score, \
     i.recorded_at AS recorded_at, prev.id AS supersedes, relied_on \
     ORDER BY head_at DESC, head_id DESC, depth";

/// Verdicts on OTHER claims that RELIED_ON `$claim_id`, newest first, each flagged `is_head`
/// when no later verdict supersedes it. An unknown claim yields no rows; a claim nothing
/// relied on yields one row of nulls. (`exists()` over a pattern always reported false here
/// on v4.22.1, hence the OPTIONAL MATCH for the superseding verdict.)
pub const EVIDENCE_REUSE: &str = "MATCH (target:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c:Claim)-[:HAS_INVESTIGATION]->(i:Investigation)-[:RELIED_ON]->(target) \
     WHERE c <> target \
     OPTIONAL MATCH (newer:Investigation)-[:SUPERSEDES]->(i) \
     RETURN c.id AS claim_id, i.id AS certificate_id, i.decision AS decision, \
     i.recorded_at AS recorded_at, newer IS NULL AS is_head \
     ORDER BY recorded_at DESC, certificate_id DESC";

/// The agent's memory of a claim in ONE statement (one consistent snapshot): how many verdicts
/// exist, the newest one, and the claims it RELIED_ON. A known claim with no investigations
/// yields `prior = 0` and nulls; an unknown claim yields no rows.
pub const PRIOR_VERDICTS: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:HAS_INVESTIGATION]->(i:Investigation) \
     WITH c, i ORDER BY i.recorded_at DESC, i.id DESC \
     WITH c, collect(i) AS chain \
     WITH size(chain) AS prior, chain[0] AS latest \
     OPTIONAL MATCH (latest)-[:RELIED_ON]->(l:Claim) \
     RETURN prior, latest.id AS certificate_id, latest.decision AS decision, \
     latest.risk_score AS risk_score, latest.recorded_at AS recorded_at, \
     collect(l.id) AS relied_on";

/// SIU investigator whose specialties best cover the matched patterns, least loaded first.
pub const PICK_SIU_INVESTIGATOR: &str = "MATCH (i:Investigator) \
     WITH i, size([p IN $patterns WHERE p IN i.specialties]) AS coverage \
     RETURN i.id AS id, i.name AS name, i.specialties AS specialties, i.open_cases AS open_cases, coverage \
     ORDER BY coverage DESC, open_cases ASC, id ASC LIMIT 1";

/// Check + pick + assign in one write statement: the existing assignment wins (its MERGE matches
/// and changes nothing), otherwise the PICK_SIU_INVESTIGATOR choice gets the edge and
/// open_cases + 1. No rows only when the graph holds no Investigator nodes (or no such claim).
///
/// FalkorDB does not document one statement as atomic under concurrency (without the constraint,
/// 60 rounds x 32 racing runs on v4.22.1 never left a duplicate edge, an observation only; see
/// `without_the_constraint_concurrent_assignment_race_is_observed`). ASSIGNMENT_CONSTRAINT
/// (seeded, `r.claim_id` set here) is the guard: a duplicate commit fails and rolls back,
/// open_cases included, and the caller re-runs the statement, which then finds the winner's edge.
/// With the constraint in place, 20 rounds x 32 racing calls never even produced a violation.
pub const ASSIGN_ONCE: &str = "MATCH (c:Claim {id: $claim_id}) \
     OPTIONAL MATCH (c)-[:ASSIGNED_INVESTIGATOR]->(existing:Investigator) \
     WITH c, collect(existing)[0] AS existing \
     MATCH (i:Investigator) \
     WITH c, existing, i, size([p IN $patterns WHERE p IN i.specialties]) AS coverage \
     ORDER BY coverage DESC, i.open_cases ASC, i.id ASC LIMIT 1 \
     WITH c, coalesce(existing, i) AS chosen \
     MERGE (c)-[r:ASSIGNED_INVESTIGATOR]->(chosen) \
     ON CREATE SET r.claim_id = c.id, r.assigned_at = $assigned_at, \
     chosen.open_cases = chosen.open_cases + 1 \
     RETURN chosen.id AS id, chosen.name AS name, chosen.specialties AS specialties, \
     chosen.open_cases AS open_cases";

/// Unique `(relationship type, property)`: at most one ASSIGNED_INVESTIGATOR edge per claim id.
pub const ASSIGNMENT_CONSTRAINT: (&str, &str) = ("ASSIGNED_INVESTIGATOR", "claim_id");

/// Claims weakly connected to `$claim_id` through shared customers, contacts, shops and clinics
/// (same projection as the WCC cross-check test). An unknown claim yields no rows.
/// Cost: algo.WCC labels the whole projected graph on every call, O(V+E), then keeps one
/// component; well under the read timeout on the demo graph.
pub const RING_COMPONENT: &str =
    "CALL algo.WCC({nodeLabels: ['Customer','Claim','Phone','Address','RepairShop','Clinic'], \
     relationshipTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT']}) \
     YIELD node, componentId WITH node, componentId WHERE node:Claim \
     WITH componentId, collect(node.id) AS claims WHERE $claim_id IN claims \
     RETURN claims";

/// Hop bound for CONNECTION_PATH (written into the query as maxLen).
pub const MAX_PATH_HOPS: usize = 6;

/// Up to 5 of the minimal-length paths (algo.SPpaths, any direction) between two claims over the
/// same relationship types as RING_COMPONENT, at most 6 hops. pathCount 0 asks for every minimal
/// path (pathCount n > 1 also returns longer ones). No rows when the claims are not connected
/// within the bound or either id is unknown.
pub const CONNECTION_PATH: &str = "MATCH (a:Claim {id: $claim_a}), (b:Claim {id: $claim_b}) \
     CALL algo.SPpaths({sourceNode: a, targetNode: b, \
     relTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT'], \
     relDirection: 'both', maxLen: 6, pathCount: 0}) YIELD path \
     RETURN [n IN nodes(path) | labels(n)[0]] AS labels, \
     [n IN nodes(path) | coalesce(n.id, n.number, n.line, n.vin)] AS ids, \
     [r IN relationships(path) | type(r)] AS rels LIMIT 5";

pub const COUNT_CLAIMS_BY_ID: &str =
    "MATCH (c:Claim) WHERE c.id IN $claim_ids RETURN count(c) AS found";

/// Repair shops and clinics ranked by whole-graph algo.pageRank, with their claim count.
/// The labelled form (one label, one relationship type) scores every shop equally, so it runs
/// over the whole graph and filters afterwards; O(V+E) per call.
pub const SHOP_INFLUENCE: &str = "CALL algo.pageRank(null, null) YIELD node, score \
     WITH node, score WHERE node:RepairShop OR node:Clinic \
     OPTIONAL MATCH (node)<-[:REPAIRED_AT|TREATED_AT]-(c:Claim) \
     RETURN labels(node)[0] AS label, node.id AS id, node.name AS name, score, count(c) AS claims \
     ORDER BY score DESC, id LIMIT $limit";

/// algo.labelPropagation over the RING_COMPONENT projection (the procedure accepts nodeLabels
/// and relationshipTypes). One row per community holding at least 2 claims, or, with
/// `$claim_id`, only that claim's community whatever its size (no rows for an unknown claim).
/// On v4.22.1 propagation follows edge direction, so shops and clinics land in communities of
/// their own, never with their claims: `providers` (`[label, id]`) are therefore the shops and
/// clinics the community's claims were repaired or treated at. Community ids are not stable
/// across graph changes. Cost: labels the whole projected graph on every call.
pub const RING_COMMUNITIES: &str =
    "CALL algo.labelPropagation({nodeLabels: ['Customer','Claim','Phone','Address','RepairShop','Clinic'], \
     relationshipTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT']}) \
     YIELD node, communityId WITH communityId, node WHERE node:Claim \
     WITH communityId, collect(node) AS members, collect(node.id) AS claims \
     WHERE ($claim_id IS NULL AND size(claims) >= 2) OR $claim_id IN claims \
     UNWIND members AS c OPTIONAL MATCH (c)-[:REPAIRED_AT|TREATED_AT]->(p) \
     WITH communityId, claims, \
     collect(DISTINCT CASE WHEN p IS NULL THEN NULL ELSE [labels(p)[0], p.id] END) AS providers \
     RETURN communityId AS community_id, claims, providers";

/// Unique `(label, property)`: at most one Review per claim.
pub const REVIEW_CONSTRAINT: (&str, &str) = ("Review", "claim_id");
