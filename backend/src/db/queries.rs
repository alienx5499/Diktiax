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

pub const CLAIM_NODE: &str = "MATCH (c:Claim {id: $claim_id}) RETURN c AS node";

pub const CLAIM_SHOPS: &str =
    "MATCH (:Claim {id: $claim_id})-[:REPAIRED_AT]->(s:RepairShop) RETURN s.id AS shop_id ORDER BY shop_id";

pub const MIN_DEPTH: u8 = 1;
pub const MAX_DEPTH: u8 = 4;

/// Variable-length neighbourhood of a claim. `depth` is clamped to `MIN_DEPTH..=MAX_DEPTH`
/// before being formatted into the path bound (Cypher cannot parameterise path lengths).
pub fn claim_subgraph(depth: u8) -> String {
    let depth = depth.clamp(MIN_DEPTH, MAX_DEPTH);
    format!(
        "MATCH p = (:Claim {{id: $claim_id}})-[*1..{depth}]-() \
         UNWIND relationships(p) AS r WITH DISTINCT r \
         RETURN startNode(r) AS src, r AS rel, endNode(r) AS dst"
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

pub const RECORD_INVESTIGATION: &str = "MATCH (c:Claim {id: $claim_id}) \
     CREATE (c)-[:HAS_INVESTIGATION]->(i:Investigation {id: $certificate_id, decision: $decision, \
     risk_score: $risk_score, recorded_at: $recorded_at}) \
     RETURN i.id AS investigation_id";

/// SIU investigator whose specialties best cover the matched patterns, least loaded first.
pub const PICK_SIU_INVESTIGATOR: &str = "MATCH (i:Investigator) \
     WITH i, size([p IN $patterns WHERE p IN i.specialties]) AS coverage \
     RETURN i.id AS id, i.name AS name, i.specialties AS specialties, i.open_cases AS open_cases, coverage \
     ORDER BY coverage DESC, open_cases ASC, id ASC LIMIT 1";

/// Idempotent assignment: open_cases grows only when the edge is first created.
pub const ASSIGN_INVESTIGATOR: &str =
    "MATCH (c:Claim {id: $claim_id}), (i:Investigator {id: $investigator_id}) \
     MERGE (c)-[r:ASSIGNED_INVESTIGATOR]->(i) \
     ON CREATE SET r.assigned_at = $assigned_at, i.open_cases = i.open_cases + 1 \
     RETURN i.id AS id, i.name AS name, i.specialties AS specialties, i.open_cases AS open_cases";
