<p align="center"><img src="https://github.com/user-attachments/assets/64ac4e68-c1e4-4743-ad71-c44295e6e16b" alt="Diktiax logo" width="112"></p>

<h1 align="center">Diktiax</h1>

![Three crashes, one ring: the graph picks the next query](https://github.com/user-attachments/assets/f6b18adb-0742-46d3-ad0c-3e1b30dca7ef)

**An investigation agent that finds insurance fraud rings by walking a FalkorDB graph, acts on what it finds, and remembers it: every verdict joins a constraint-guarded chain in the graph that the agent, and any other agent over MCP, reads before investigating again.**

Track 01, Agents That Act on Connected Data, and Track 02, Agent Memory and Coordination: FalkorDB "Graph Hacks: Context for AI Agents".

![Rust](https://img.shields.io/badge/Rust-1.99_·_Axum_0.8-B7410E?style=flat-square&logo=rust&logoColor=white) ![FalkorDB](https://img.shields.io/badge/FalkorDB-v4.22.1-7466FF?style=flat-square) ![Next.js](https://img.shields.io/badge/Next.js-16.4-000000?style=flat-square&logo=nextdotjs&logoColor=white) ![React](https://img.shields.io/badge/React-19.3-149ECA?style=flat-square&logo=react&logoColor=white) ![TypeScript](https://img.shields.io/badge/TypeScript-6-3178C6?style=flat-square&logo=typescript&logoColor=white) ![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-4-06B6D4?style=flat-square&logo=tailwindcss&logoColor=white) ![Bun](https://img.shields.io/badge/Bun-1.3-000000?style=flat-square&logo=bun&logoColor=white) ![Groq](https://img.shields.io/badge/Groq-gpt--oss--120b-F55036?style=flat-square) ![Claude](https://img.shields.io/badge/Claude-MCP-D97757?style=flat-square&logo=anthropic&logoColor=white) ![Docker](https://img.shields.io/badge/Docker-compose-2496ED?style=flat-square&logo=docker&logoColor=white) ![Vercel](https://img.shields.io/badge/Vercel-frontend-000000?style=flat-square&logo=vercel&logoColor=white) ![Render](https://img.shields.io/badge/Render-backend-46E3B7?style=flat-square&logo=render&logoColor=white)

- **Live demo:** https://diktiax.vercel.app
- **API:** https://diktiax.onrender.com
- **Video (4K):** https://youtu.be/DLu3_uuH42A

https://github.com/user-attachments/assets/8bf9e6ba-1c15-4737-89f5-df575e0ed24d

> **Judges, the 60-second path**
> 1. Open https://diktiax.vercel.app (it can take up to a minute to wake). Click **CLM-48291**, the ring claim; CLM-20988, CLM-10022 and CLM-20417 are clean controls. One investigation per visitor every 15 s.
> 2. Video: [0:10](https://youtu.be/DLu3_uuH42A?t=10) the ring · [1:30](https://youtu.be/DLu3_uuH42A?t=90) the agent investigates · [1:45](https://youtu.be/DLu3_uuH42A?t=105) WCC picks the corroboration targets · [2:32](https://youtu.be/DLu3_uuH42A?t=152) 5,040 claims · [3:27](https://youtu.be/DLu3_uuH42A?t=207) the agent reads its past verdicts.
> 3. Headline: on 5,040 generated claims the graph agent froze 200 of 240 fraud claims and 0 honest ones (precision 1.00, recall 0.83, F1 0.91); per-claim rules flagged 2,789 honest drivers (0.04, 0.50, 0.08). The other 40 fraud claims and 169 honest ones went to audit. 17 ms median per investigation. A pre-registered re-run on a fresh seed gave the same picture: rules flagged 2,753 honest drivers, the graph froze 200 of 240 and 0 honest.
> 4. No browser? `curl -s https://diktiax.onrender.com/api/health` (expect `"falkordb":"ok"`; it also wakes the service), `curl -s -X POST https://diktiax.onrender.com/api/investigate/CLM-48291`, then `curl -s https://diktiax.onrender.com/api/claims/CLM-48291/investigations` to see the stored verdict. Investigate again after 15 s and the dossier's `memory` block shows the earlier verdict and the checks it skipped.
> 5. Proof: `cargo test --release --test scale_benchmark -- --ignored --nocapture` and `cargo test --release --test agent_memory -- --ignored --test-threads=1` in `backend/`. The hosted graph reseeds on every restart, so its investigation history starts empty.

## The problem

An SIU (Special Investigation Unit) analyst reviews claims one at a time. Organized fraud doesn't show up that way. Claim CLM-48291 is an ordinary $42,500 collision on its own. What gives it away is everything around it:

- two other claims had accidents at the same intersection, with no police report
- all three were repaired at the same body shop and treated at the same clinic
- all three were handled by the same adjuster
- two of the claimants share a phone number

None of that lives in any single record, and a vector search over claim text can't find it either: it returns claims that *read* alike, not claims that are *connected*. In a graph, each of those signals is one short traversal.

![Similar text vs shared connections: the same three claims in vector search and in the graph](https://github.com/user-attachments/assets/51da4099-e95e-4b19-a651-85cbf2bf6c23)

## What Diktiax does

Give it a claim id. The agent:

1. **Explores** the claim's neighbourhood in FalkorDB (3 hops).
2. **Decides which detectors to run** based on what that neighbourhood contains. With the rule-based planner, a detector whose pattern can't possibly match is skipped. With an LLM planner the model chooses, and any detector it skipped still runs before scoring.
3. **Runs Cypher detectors** for four fraud-ring patterns, then follows the evidence: shops named in a match get a centrality check, and linked claims get corroborated.
4. **Scores** the claim from detector results only. The LLM never sets the score.
5. **Acts**: writes the new claim status and an `Investigation` node back to the graph, freezes the payout, assigns an SIU investigator picked from the graph by specialty and workload, and optionally fires a webhook.

Every step is returned as a trace: tool, arguments, why it was chosen, what came back, and how long it took.

![Agent trace in the dashboard: each step with the planner's reason, arguments, result and Cypher](https://github.com/user-attachments/assets/8d74d5bc-75e7-48bb-9f78-e77f72a5b6dc)

![The evidence path highlighted on the claim's subgraph](https://github.com/user-attachments/assets/2aa91a63-9b03-424a-bb91-0d752b952550)

## How the graph drives the agent

![How the agent picks its next query](https://github.com/user-attachments/assets/dff36366-c272-4878-952c-f0bf7962c822)

Real runs with the rule-based planner (`cargo run --release -- --investigate <id>`):

**CLM-48291 (ring member): 8 tool calls → risk 100, CRITICAL_FRAUD, payout frozen, investigator assigned**

| # | Tool | Why the planner chose it | Result |
|---|------|--------------------------|--------|
| 1 | `explore_claim_subgraph` | Map the neighbourhood first | 24 nodes, 32 edges |
| 2 | `detect_ghost_collision` | LOC-441 hosts several accidents; claim repaired at SHOP-801 | match, +45 |
| 3 | `detect_shared_contacts` | Phone +1-555-019-8821 belongs to more than one customer | match, +25 |
| 4 | `detect_vin_cloning` | A VIN is in the neighbourhood | no match |
| 5 | `detect_kickback_triangle` | No police report; ADJ-04 and MED-201 attached | match, +40 |
| 6 | `shop_centrality` SHOP-801 | SHOP-801 appears in the fraud evidence | 1.71× the average shop, anomalous |
| 7-8 | detectors on CLM-39102 | Linked to CLM-48291 through the shared phone | match (corroboration, not scored) |

**CLM-10022 (clean claim): 3 tool calls → risk 0, cleared automatically**

The planner explores, runs two detectors, and stops. It never calls `detect_shared_contacts` (no phone or address is shared) or `detect_kickback_triangle` (there's a police report and no clinic). Different graph, different investigation.

## Track 02: memory and coordination

![Verdicts stay under the claim in the graph, so a new session or another agent over MCP reads the same certificate](https://github.com/user-attachments/assets/7518057e-6acc-4ec1-a181-8c226bb5c401)

![Re-investigating CLM-48291: the verdict comes first, and the memory panel shows the agent agrees with its earlier verdict and skipped 2 corroboration checks it already had evidence for](https://github.com/user-attachments/assets/9e6b947a-100c-4497-933c-ce9bbaf74525)

**The coordination problem: two agents, or two runs of one agent, must never assign the same claim twice, fork its verdict history, or redo work already decided.** Diktiax makes the graph the shared memory that guarantees this. The agent writes every verdict and assignment into FalkorDB; the next run, the SIU team's workload and any other agent over MCP all read the same state; and the guarantees are database constraints, not app locks. Each one has a test that passes:

| Guarantee | Test |
|---|---|
| 32 racing assignments of one claim leave exactly one investigator edge | [`concurrent_direct_assignment_creates_one_edge`](backend/tests/agent_memory.rs) |
| 16 concurrent investigations of one claim leave one linear `SUPERSEDES` chain (16 nodes, 15 links, one head) | [`concurrent_investigations_form_one_supersedes_chain`](backend/tests/agent_memory.rs) |
| 8 concurrent MCP `investigate_claim` calls on a new claim run the agent once; 7 get the stored verdict | [`concurrent_mcp_investigate_claim_runs_the_agent_once`](backend/tests/agent_memory.rs) |
| A re-run reads the last verdict, says whether it agrees, and skips corroboration it already relied on | [`second_run_remembers_and_skips_corroborated_work`](backend/tests/agent_memory.rs) |
| 8 concurrent re-runs each report memory consistent with the chain they read | [`concurrent_reinvestigations_report_consistent_memory`](backend/tests/agent_memory.rs) |

- **Verdicts outlive the session.** Every investigation writes an `Investigation` node (certificate id, decision, risk score, time) under the claim, and re-investigating adds another one, so the graph holds the full history. `GET /api/claims/{id}/investigations` and the MCP tool `investigation_history` return it newest first. [`verdicts_survive_across_sessions`](backend/tests/agent_memory.rs) investigates a claim, drops the client, connects a new one and checks the same certificate comes back, then investigates again and gets two entries.
- **Other agents pick up where Diktiax left off.** `investigation_history` is served over MCP and HTTP, so Claude or any MCP client can see what the agent already decided instead of re-running it. The agent's own planners never see this tool (a unit test checks it), so a past verdict can't stand in for fresh evidence.
- **Memory is consumed, not just stored.** `investigate_claim` over MCP reads the claim's history first. If a verdict is already recorded it returns that verdict (`reused: true`, certificate id, decision, risk score, time) without running the agent or writing anything; `force: true` runs a fresh investigation. A per-claim lock covers the check, the run and the write, so 8 concurrent calls on a never-investigated claim run the agent once and the other 7 get the reused verdict, while calls on different claims still run in parallel ([`concurrent_mcp_investigate_claim_runs_the_agent_once`](backend/tests/agent_memory.rs)). That lock lives in one process (the hosted backend runs a single instance); with several instances two of them could both run the agent, and the verdict chain would still stay linear, because that part is a database constraint. HTTP `POST /api/investigate`, which the dashboard uses, always runs a fresh investigation, but the agent still reads its memory first: one `PRIOR_VERDICTS` read returns the verdict count, the latest verdict and the claims it `RELIED_ON`. The dossier gains a `memory` block (`prior_investigations`, `latest_prior`, `agrees_with_latest`, `skipped_corroboration`, `tool_calls_saved`), the rationale says whether this run agrees, and corroboration on claims the last verdict already relied on is skipped, whichever planner (rule-based or LLM) asked for it (rule-based on the ring claim: 9 tool calls first, 7 on a re-run; same score). Scoring never reads a past verdict. Tests: [`second_run_remembers_and_skips_corroborated_work`](backend/tests/agent_memory.rs), [`concurrent_reinvestigations_report_consistent_memory`](backend/tests/agent_memory.rs).
- **Memory has structure.** Each new verdict on a claim `SUPERSEDES` the previous one, so a claim's history is a single linear chain, and it links with `RELIED_ON` to the linked claims its evidence came from. Two unique constraints (`SUPERSEDES.superseded_id` and one `InvestigationChain` start marker per claim) keep the chain linear under concurrency; [`concurrent_investigations_form_one_supersedes_chain`](backend/tests/agent_memory.rs) runs 16 investigations of one claim at once and checks 16 nodes, 15 links and one head, and [`chain_constraints_reject_a_fork`](backend/tests/agent_memory.rs) forces the exact write a lost race would make and checks it is rejected with nothing left behind.
- **Shared state with the SIU team.** Assigning an investigator raises their `open_cases` in the graph, and the next case goes to the investigator whose specialties best cover the matched patterns, least loaded first, so earlier investigations shape later assignments. A claim keeps its first investigator: re-investigating it reuses the existing assignment instead of picking again, so `open_cases` only rises once.
- **Consistent under concurrency.** [`concurrent_investigations_keep_shared_state_consistent`](backend/tests/agent_memory.rs) runs 16 investigations at once (all 8 claims twice) and checks there are exactly 16 verdicts, exactly one investigator on each of the 5 flagged claims, none on the 3 clean ones, and an `open_cases` increase of exactly 5. Writing it caught a real race (two concurrent runs could assign a claim twice). FalkorDB does not document that a single read-then-write statement is serialised under concurrency, so the fix is a database constraint: one `MERGE` statement plus a unique constraint on `ASSIGNED_INVESTIGATOR.claim_id`, created at seed time and awaited until it is enforced. [`concurrent_direct_assignment_creates_one_edge`](backend/tests/agent_memory.rs) and [`concurrent_investigations_of_one_claim_assign_once`](backend/tests/agent_memory.rs) run 32 concurrent writers on one claim and always leave exactly one edge. [`without_the_constraint_concurrent_assignment_race_is_observed`](backend/tests/agent_memory.rs) drops the constraint and races the statement alone: on FalkorDB v4.22.1 it left one edge in all 60 rounds we ran (1,920 calls), so we could not reproduce a duplicate with the current single `MERGE` statement; the constraint is kept as the guarantee.
- **Handoff between planners.** If the LLM planner fails mid-run, the rule-based planner continues from the same step history rather than starting over, and the dossier records where it took over.

A real MCP session (JSON-RPC over stdio against the `--mcp-server` binary, recorded on a throwaway graph): another agent asks for history, gets nothing, runs the investigation once, then gets the stored verdict back instead of a second run.

```text
→ tools/call investigation_history {"claim_id":"CLM-48291"}
← []
→ tools/call investigate_claim {"claim_id":"CLM-48291"}
← (full dossier, summarised) CRITICAL_FRAUD, risk 100 · certificate CERT-02987df7-5d82-4c98-ba3e-9a14ae379c13
→ tools/call investigate_claim {"claim_id":"CLM-48291"}
← {"reused":true,"claim_id":"CLM-48291","certificate_id":"CERT-02987df7-5d82-4c98-ba3e-9a14ae379c13","decision":"CRITICAL_FRAUD","risk_score":100,"recorded_at":"2026-10-17T21:39:00.563358+00:00"}
→ tools/call ring_components {"claim_id":"CLM-48291"}
← {"claim_id":"CLM-48291","claims":["CLM-39102","CLM-44730","CLM-48291"],"size":3}
```

The score never reads this history: on a re-run a past verdict can save corroboration work and is reported in the dossier's `memory` block, but it is never evidence for a new score.

## Graph data model

Source of truth: [`data/seed.cypher`](data/seed.cypher) (68 nodes, 84 relationships) and the ingest/write-back queries in [`backend/src/db/queries.rs`](backend/src/db/queries.rs).

![Diktiax graph model](https://github.com/user-attachments/assets/08832df0-c319-43d4-9fe0-6636edb16c99)

Dashed relationships are written by the agent.

| Label | Key properties | Why it's a node |
|-------|----------------|-----------------|
| `Claim` | `id`, `amount_claimed`, `status`, `filed_day` | The thing under investigation; `status` is overwritten by the agent |
| `Customer` | `id`, `name` | Claimant |
| `Phone`, `Address` | `number`, `line` | Merged on value, so customers who share one share the node: a shared contact is a graph path, not a string comparison |
| `Vehicle`, `VIN` | `id`, `jurisdiction`; `vin` | One `VIN` node with two vehicles pointing at it is the cloning signal |
| `Accident`, `Location` | `police_report_filed`; `id` | Where and how the loss happened |
| `RepairShop`, `Clinic`, `Adjuster` | `id`, `name` | The service providers that rings reuse |
| `Investigator` | `specialties`, `open_cases` | Seeded SIU staff the agent assigns from |
| `Investigation` | `decision`, `risk_score`, certificate id | The agent's recorded verdict |

| Relationship | From → To | Meaning |
|--------------|-----------|---------|
| `FILED_CLAIM` | `Customer` → `Claim` | Who filed the claim |
| `TRIGGERED_CLAIM` | `Accident` → `Claim` | The loss behind the claim |
| `OCCURRED_AT` | `Accident` → `Location` | Where it happened |
| `INVOLVED_IN` | `Vehicle` → `Accident` | Vehicles in the accident |
| `OWNS` | `Customer` → `Vehicle` | Vehicle ownership |
| `HAS_VIN` | `Vehicle` → `VIN` | Two vehicles on one `VIN` = cloning |
| `HAS_PHONE`, `LIVES_AT` | `Customer` → `Phone` / `Address` | Shared contacts |
| `REPAIRED_AT`, `TREATED_AT`, `ASSIGNED_TO` | `Claim` → `RepairShop` / `Clinic` / `Adjuster` | Service providers on the claim |
| `COLLUDES_WITH` | `RepairShop` → `Clinic` | Referral link (`referral_fee_pct`) used by the kickback check |
| `HAS_INVESTIGATION` | `Claim` → `Investigation` | Written by the agent: the verdict |
| `SUPERSEDES` | `Investigation` → `Investigation` | Written by the agent: the newer verdict on a claim points to the one it replaces (`superseded_id`, unique) |
| `RELIED_ON` | `Investigation` → `Claim` | Written by the agent: the linked claims the evidence came from |
| `ASSIGNED_INVESTIGATOR` | `Claim` → `Investigator` | Written by the agent: who works the case |

Seeded scenarios: a ghost-collision ring (CLM-48291, CLM-39102, CLM-44730), a cloned-VIN pair across CA and NV (CLM-55104, CLM-31205), and three clean controls (CLM-10022, CLM-20417, CLM-20988).

## The Cypher queries

Every query below is copied verbatim from [`backend/src/db/queries.rs`](backend/src/db/queries.rs) (line breaks added). Every value is passed as a parameter, never formatted into the query text; the one exception is the exploration depth: on FalkorDB v4.22.1 `[*1..$depth]` fails with "Encountered unhandled type in inlined properties" ([FalkorDB#3350](https://github.com/FalkorDB/FalkorDB/issues/3350)), so the depth is clamped to 1..4 in Rust and then inlined. Each detector returns the node ids that form the evidence path.

Each detector models a scheme insurance fraud investigators deal with in practice:

| Detector | Real-world scheme it models |
|---|---|
| Ghost collision ring | Staged or invented ("paper") accidents: a ring reuses one location and routes every claim to the same body shop |
| Shared contact network | Claimant rings: separate "strangers" on different claims who share a phone number or home address |
| Cloned VIN | Vehicle cloning: one VIN carried by two registrations (often in different states), each filing its own claim |
| Kickback triangle | Provider collusion: a body shop, clinic and adjuster who keep appearing together on unreported accidents, sometimes with a recorded referral arrangement |
| Shop concentration | A repair shop acting as a ring hub, with far more claims than its peers |

The data is generated, so these are the shapes of those schemes, not cases taken from real claim files.

**Explore the neighbourhood** (always the first step; the agent runs it with depth 3):

```cypher
MATCH p = (:Claim {id: $claim_id})-[*1..3]-()
UNWIND relationships(p) AS r WITH DISTINCT r
RETURN startNode(r) AS src, r AS rel, endNode(r) AS dst
```

**Ghost collision ring**: other claims at the same location, repaired at the same shop, within 90 days.
Path: `Claim ← Accident → Location ← Accident → Claim → RepairShop`, 5 hops. In SQL: claims joined to accidents, locations, accidents again, claims again and shops, a six-table self-join.

```cypher
MATCH (c:Claim {id: $claim_id})<-[:TRIGGERED_CLAIM]-(:Accident)-[:OCCURRED_AT]->(loc:Location),
      (c)-[:REPAIRED_AT]->(shop:RepairShop),
      (loc)<-[:OCCURRED_AT]-(:Accident)-[:TRIGGERED_CLAIM]->(other:Claim)-[:REPAIRED_AT]->(shop)
WHERE other.id <> c.id AND abs(other.filed_day - c.filed_day) <= $window_days
RETURN c.id AS claim_id, loc.id AS location_id, shop.id AS shop_id,
       other.id AS linked_claim_id, abs(other.filed_day - c.filed_day) AS days_apart
ORDER BY days_apart, linked_claim_id
```

**Shared contact network**: claimants of other claims who share this claimant's phone or address.
Path: `Claim ← Customer → Phone|Address ← Customer → Claim`, 4 hops. In SQL: the same chain written twice (phones, then addresses) and unioned.

```cypher
MATCH (c:Claim {id: $claim_id})<-[:FILED_CLAIM]-(me:Customer)-[link:HAS_PHONE|LIVES_AT]->(contact)
      <-[:HAS_PHONE|LIVES_AT]-(peer:Customer)-[:FILED_CLAIM]->(other:Claim)
WHERE peer.id <> me.id
RETURN c.id AS claim_id, me.id AS customer_id, type(link) AS via,
       coalesce(contact.number, contact.line) AS contact, peer.id AS peer_id,
       peer.name AS peer_name, other.id AS linked_claim_id
ORDER BY peer_id, linked_claim_id
```

**Cloned VIN**: other vehicle registrations carrying the same VIN that also produced claims.
Path: `Claim ← Accident ← Vehicle → VIN ← Vehicle → Accident → Claim`, 6 hops. In SQL: a seven-table chain through two vehicle registrations.

```cypher
MATCH (c:Claim {id: $claim_id})<-[:TRIGGERED_CLAIM]-(:Accident)<-[:INVOLVED_IN]-(v:Vehicle)-[:HAS_VIN]->(vin:VIN)
      <-[:HAS_VIN]-(clone:Vehicle)-[:INVOLVED_IN]->(:Accident)-[:TRIGGERED_CLAIM]->(other:Claim)
WHERE other.id <> c.id
RETURN c.id AS claim_id, v.id AS vehicle_id, v.jurisdiction AS jurisdiction, vin.vin AS vin,
       clone.id AS clone_vehicle_id, clone.jurisdiction AS clone_jurisdiction, other.id AS linked_claim_id
ORDER BY linked_claim_id
```

**Kickback triangle**: an unreported accident whose adjuster, shop and clinic recur together on other claims.
Pattern: one claim and another claim both pointing at the same `RepairShop`, `Clinic` and `Adjuster` (six edges matched at once). In SQL: three provider tables self-joined on two claims, then filtered.

```cypher
MATCH (a:Accident)-[:TRIGGERED_CLAIM]->(c:Claim {id: $claim_id}),
      (c)-[:REPAIRED_AT]->(shop:RepairShop), (c)-[:TREATED_AT]->(clinic:Clinic), (c)-[:ASSIGNED_TO]->(adj:Adjuster),
      (other:Claim)-[:REPAIRED_AT]->(shop), (other)-[:TREATED_AT]->(clinic), (other)-[:ASSIGNED_TO]->(adj)
WHERE other.id <> c.id AND a.police_report_filed = false
OPTIONAL MATCH (shop)-[ref:COLLUDES_WITH]->(clinic)
RETURN c.id AS claim_id, adj.id AS adjuster_id, shop.id AS shop_id, clinic.id AS clinic_id,
       other.id AS linked_claim_id, ref IS NOT NULL AS referral_link
ORDER BY linked_claim_id
```

**Shop concentration** (degree centrality): a shop's claim count against the average across all shops. 1.5× or more is flagged.

```cypher
MATCH (s:RepairShop) OPTIONAL MATCH (s)<-[:REPAIRED_AT]-(c:Claim)
RETURN s.id AS shop_id, s.name AS shop_name, count(c) AS degree
```

**Acting on the verdict** (write-back): set the claim's status, record the investigation as the new head of the claim's chain, link the evidence it relied on, and assign an investigator exactly once.

```cypher
MATCH (c:Claim {id: $claim_id}) SET c.status = $status RETURN c.id AS claim_id

MATCH (c:Claim {id: $claim_id})
OPTIONAL MATCH (c)-[:HAS_INVESTIGATION]->(prev:Investigation)
WHERE NOT ()-[:SUPERSEDES]->(prev)
WITH c, prev ORDER BY prev.recorded_at DESC, prev.id DESC LIMIT 1
CREATE (c)-[:HAS_INVESTIGATION]->(i:Investigation {id: $certificate_id, decision: $decision,
       risk_score: $risk_score, recorded_at: $recorded_at})
FOREACH (p IN CASE WHEN prev IS NULL THEN [] ELSE [prev] END |
  CREATE (i)-[:SUPERSEDES {superseded_id: p.id}]->(p))
FOREACH (root IN CASE WHEN prev IS NULL THEN [1] ELSE [] END |
  CREATE (:InvestigationChain {claim_id: c.id}))
RETURN i.id AS investigation_id

MATCH (i:Investigation {id: $investigation_id})
UNWIND $claim_ids AS cid
MATCH (l:Claim {id: cid})
CREATE (i)-[:RELIED_ON]->(l)
RETURN count(l) AS linked

MATCH (c:Claim {id: $claim_id})
OPTIONAL MATCH (c)-[:ASSIGNED_INVESTIGATOR]->(existing:Investigator)
WITH c, collect(existing)[0] AS existing
MATCH (i:Investigator)
WITH c, existing, i, size([p IN $patterns WHERE p IN i.specialties]) AS coverage
ORDER BY coverage DESC, i.open_cases ASC, i.id ASC LIMIT 1
WITH c, coalesce(existing, i) AS chosen
MERGE (c)-[r:ASSIGNED_INVESTIGATOR]->(chosen)
ON CREATE SET r.claim_id = c.id, r.assigned_at = $assigned_at,
  chosen.open_cases = chosen.open_cases + 1
RETURN chosen.id AS id, chosen.name AS name, chosen.specialties AS specialties,
  chosen.open_cases AS open_cases
```

FalkorDB does not document that a single read-then-write statement is serialised under concurrency (with the constraint dropped, 1,920 racing calls of the assignment statement on v4.22.1 never left a duplicate, but that is an observation, not a guarantee), so each of these writes is backed by a unique constraint created at startup and awaited until it is enforced: `ASSIGNED_INVESTIGATOR.claim_id`, `SUPERSEDES.superseded_id` and `InvestigationChain.claim_id`. A violation rolls the statement back and it is retried.

Runs on FalkorDB v4.22.1, the newest release of the 4.x line, pinned for reproducible demos.

## Why you can trust the verdict

![Evidence queries run on the read-only lane with a timeout; only ingest, the verdict and the assignment write](https://github.com/user-attachments/assets/b76d4a8f-cca3-468a-aa52-846ac1b04373)

- **Planners only choose which graph tools to call.** The score is computed by `FraudRiskEvaluator` ([`domain/evaluator.rs`](backend/src/domain/evaluator.rs)) from detector results on the target claim, capped at 100.
- **An LLM can't clear a claim by skipping checks.** With the Claude or Groq planner, any detector it didn't run is executed before scoring. Its closing text is labelled "not used for scoring".
- **Bounded:** the planner gets at most 12 tool calls; the coverage guard can add up to 4 detector runs after that. If the LLM API fails, the rule-based planner takes over and the dossier says so (`planner: rule-based (fallback: LLM rate-limited)`); the raw provider error goes to the server log only.
- **Read-only by construction:** exploration, the four detectors, shop centrality and the claim list run through `GRAPH.RO_QUERY` with a 5 s server-side timeout (`read_rows` in [`repository.rs`](backend/src/db/repository.rs)), so none of the evidence-gathering queries can write to the graph. Writes (ingest, the verdict write-back, investigator assignment, seeding) use `GRAPH.QUERY`, as does the startup and health `RETURN 1` ping, because `GRAPH.RO_QUERY` errors on a graph that doesn't exist yet.
- **Never cleared on bad data:** if exploring the claim or any detector on it fails, the score is only a lower bound (missing detectors can only add points). A bound that already reaches HIGH or CRITICAL still freezes the payout; anything lower becomes `INCONCLUSIVE` with status `NEEDS_MANUAL_REVIEW`, and the failed calls are named in the rationale. Only an exact `LOW_RISK` result clears a claim.
- **Checkable:** each investigation returns a certificate with a SHA-256 digest over the claim id and the ordered steps, so a saved dossier can be re-hashed later to spot edits (the server doesn't store or verify it yet). Each detector step carries the Cypher that produced it.

## Does the graph matter? (measured)

[`backend/tests/baseline.rs`](backend/tests/baseline.rs) runs classic single-claim red-flag rules (no police report, or amount ≥ $25,000, reading only the claim and its own accident) against the graph agent. It uses the seeded claims plus two constructed groups: ring members who file police reports and keep amounts small, and honest drivers whose claims carry red flags.

| Group | Claims | Per-claim rules flagged | Graph agent flagged |
|---|---|---|---|
| Seeded fraud (ring + cloned VIN) | 5 | 5/5 | 5/5 |
| Seeded clean controls | 3 | 0/3 | 0/3 |
| Evasive ring members (police report, small amounts) | 3 | 0/3 | 3/3 |
| Honest claims with red flags (no police / high amount) | 2 | 2/2 | 0/2 |

Fraud caught: rules 5/8, graph 8/8. Honest claims flagged: rules 2/5, graph 0/5. The labels were fixed by construction before anything ran.

An LLM that also reads one claim at a time does no better. [`scripts/llm_only_baseline.py`](scripts/llm_only_baseline.py) shows `openai/gpt-oss-120b` (Groq, temperature 0) each claim's own file: amount, damage type, accident description, police-report flag, location and shop. Over 3 runs with identical answers ([recorded output](scripts/llm_only_baseline_result.md)) it caught 5/8 fraud claims, missing all 3 evasive ring members, and flagged 1 of the 5 honest ones. Rules catch careless fraud; the graph also catches organised fraud that learned the rules, because the ring still reuses its phone, crash location and body shop. The groups are small and constructed, so read this as a demonstration of the mechanism, not an accuracy benchmark.

```bash
cd backend && cargo test --test baseline -- --ignored --nocapture
```

**At scale (5,040 generated claims).** [`backend/tests/scale_benchmark.rs`](backend/tests/scale_benchmark.rs) generates 4,800 honest claims spread over 300 shops, 400 locations, 150 clinics and 40 adjusters (with chance overlaps and shared household addresses), 25 tight fraud rings of 8 (10 of them evasive), and 5 loose rings that share only a crash location and a body shop. The answer key's SHA-256 is written before anything is scored ([results and key](scripts/scale/RESULTS.md)).

![Thirty fraud rings hidden among 5,040 generated claims](https://github.com/user-attachments/assets/95511d53-904c-468f-b023-c527063dbf73)

| Group | Claims | Single-claim rules flagged | Graph agent froze payout | Graph agent sent to audit |
|---|---|---|---|---|
| Tight rings | 120 | 120 | 120 | 0 |
| Evasive rings | 80 | 0 | 80 | 0 |
| Loose rings | 40 | 0 | 0 | 40 |
| Honest | 4,800 | 2,789 | 0 | 169 |

The graph agent froze 200 of 240 fraud claims and no honest ones; loose rings are a blind spot for freezing and go to audit instead, along with 3.5% of honest claims. An LLM reading a stratified 50-claim sample one claim at a time caught 3 of 24 fraud claims. Each investigation took 17 ms median (p95 23 ms) at this size. The evasive rings are generated with a police report and an amount between $3,000 and $12,000 ([`scale_benchmark.rs`](backend/tests/scale_benchmark.rs)), exactly the claims the per-claim rules can't flag, so the rules' 0 of 80 there is by construction: that group exists to show what a ring looks like once it learns the rules. The data is generated, so this measures the mechanism on rings built the way the detectors expect, not real-world accuracy. The answer key comes from the same seeded generator and is hashed before any claim is investigated: the hash shows the key wasn't edited after scoring, not that the generator wasn't tuned between runs.

**Pre-registered re-run.** The original answer key holds claim ids and labels only, so its hash is the same for any seed and proves little on its own. So the benchmark was run again on a fresh seed (20261019) as a pre-registration: the answer key, now bound to a SHA-256 of the whole generated dataset, was committed on its own before the results existed, and the results went into a later commit: key `9826efe` at 02:53 IST on 18 Oct, results `2cf0baa` at 03:13 IST; `git log -- scripts/scale/prereg/` shows the order. Same outcome: rules 120 of 240 fraud and 2,753 honest flagged; graph 200 of 240 frozen and 0 honest (178 honest to audit). Reproduce with `SCALE_SEED=20_261_019 cargo test --release --test scale_benchmark scale_benchmark -- --ignored --exact`; it fails if the key's hash differs from the committed one ([`scripts/scale/prereg/`](scripts/scale/prereg/)).

Reproduce it (about 90 s; needs FalkorDB on `localhost:6379`, uses its own graph):

```bash
cd backend && cargo test --release --test scale_benchmark -- --ignored --nocapture
shasum -a 256 ../scripts/scale/answer_key.json   # must match the hash in RESULTS.md
```

Query latency, from [`scripts/bench_queries.py`](scripts/bench_queries.py): FalkorDB's server-side execution time, 200 runs after 10 warm-ups, FalkorDB v4.22.1 in Docker on a laptop, demo graph (68 nodes):

| Query (on CLM-48291) | Median ms | p95 ms | Max ms |
|---|---|---|---|
| `explore_claim_subgraph` (3 hops) | 0.073 | 0.109 | 0.165 |
| `detect_ghost_collision` | 0.141 | 0.168 | 0.260 |
| `detect_shared_contacts` | 0.122 | 0.143 | 0.191 |
| `detect_vin_cloning` | 0.126 | 0.151 | 0.248 |
| `detect_kickback_triangle` | 0.157 | 0.187 | 0.265 |
| `shop_centrality` | 0.048 | 0.071 | 0.162 |

Three of the four detectors (ghost collision, shared contacts, VIN cloning) start from an indexed lookup of the claim, not a label scan. `GRAPH.EXPLAIN` on `detect_shared_contacts` (the 13 indexes are created at startup, see `INDEXES` in [`queries.rs`](backend/src/db/queries.rs)):

```text
Results
    Sort
        Project
            Conditional Traverse | (peer)->(other:Claim)
                Filter
                    Conditional Traverse | (contact)->(peer:Customer)
                        Conditional Traverse | (me)-[link:HAS_PHONE|:LIVES_AT]->(contact)
                            Conditional Traverse | (c)->(me:Customer)
                                Node By Index Scan | (c:Claim)
```

The plan seeks one `Claim` through the index and then only walks its relationships, so the cost grows with the claim's neighbourhood, not with the size of the graph.

`detect_kickback_triangle` is the exception: its pattern begins at `(a:Accident)`, so the planner opens with `Node By Label Scan | (a:Accident)` and filters to the claim afterwards. `shop_centrality` scans every `RepairShop` on purpose, since it compares degrees across all shops.

**Cross-checked with FalkorDB's built-in algorithms** ([`backend/tests/graph_algorithms.rs`](backend/tests/graph_algorithms.rs)). The detectors don't use these at runtime; the tests check that the ring the detectors find is also an isolated structure in the graph:

```cypher
CALL algo.WCC({nodeLabels: ['Customer','Claim','Phone','Address','RepairShop','Clinic'],
  relationshipTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT']})
YIELD node, componentId WITH node, componentId WHERE node:Claim
RETURN componentId, collect(node.id) AS claims
```

Weakly connected components put CLM-48291, CLM-39102 and CLM-44730 in one component with no other claim. `algo.pageRank(null, null)` ranks the ring's shop SHOP-801 first among repair shops (0.0234 vs 0.0210 for the next), a narrow margin on a 68-node graph. The labelled form `algo.pageRank('RepairShop', 'REPAIRED_AT')` gives every shop the same score, so the test uses the unlabelled one.

Entity resolution is tested too: [`ingest_merges_shared_contacts`](backend/tests/graph_repository.rs) files two new claims with different claimants and the same phone and address, then checks there is exactly one `Phone` and one `Address` node with two incoming edges each. A third claim with that phone still leaves one `Phone` node.

## FalkorDB features used

| Feature | Where | What it does here |
|---|---|---|
| `MERGE` entity resolution | [`queries.rs:99`](backend/src/db/queries.rs) · test `ingest_merges_shared_contacts` | Phones, addresses and VINs merge on value, so a reused phone links claims the moment one is filed |
| Parameterised multi-hop Cypher | [`queries.rs:59-93`](backend/src/db/queries.rs) | The four detectors (4 to 6 hops each); every value is a parameter |
| Bounded variable-length traversal | [`queries.rs:49`](backend/src/db/queries.rs) | `[*1..3]` neighbourhood that decides which detectors run (depth clamped and inlined, FalkorDB#3350) |
| `GRAPH.RO_QUERY` + timeout | [`repository.rs:79-89`](backend/src/db/repository.rs) | All evidence queries are read-only with a 5 s server-side timeout |
| Indexes + `GRAPH.EXPLAIN` | [`queries.rs:6`](backend/src/db/queries.rs) | 13 indexes plus 3 unique constraints that make concurrent writes safe; three of four detectors open with an index seek (plans above) |
| Write-back | [`queries.rs:118-160, 199`](backend/src/db/queries.rs) | Claim status, `Investigation` node, `MERGE`d `ASSIGNED_INVESTIGATOR` edge |
| Named graphs | [`tests/common/mod.rs`](backend/tests/common/mod.rs) | Each test run gets its own graph; the scale graph is named by its data hash (`scale_e03eb6b9`) |
| `algo.WCC`, `algo.pageRank` | [`graph_algorithms.rs:60,83`](backend/tests/graph_algorithms.rs) · [`queries.rs:219`](backend/src/db/queries.rs) | Test-time cross-checks on the demo graph. WCC is also live through the `ring_components` tool (read-only, 5 s timeout), which returns every claim in the target's component. The rule planner calls it once after a detector match and spreads its corroboration runs (one per matched detector, as before) across ring members the evidence names; scoring still reads only the target claim's detectors, and live shop centrality is a degree count ([`queries.rs:94`](backend/src/db/queries.rs)) |
| MCP | [`backend/src/mcp`](backend/src/mcp) | The same graph tools, plus `investigation_history` and `ring_components`, for Claude or any MCP client |

## Architecture

![Diktiax architecture: Vercel dashboard, Render container with the Rust agent and FalkorDB, Groq planner, Claude over MCP](https://github.com/user-attachments/assets/1ff17889-0d11-41e4-a3f9-9fcf21e3c726)

The agent, the HTTP API and the MCP server all use the same tool catalog, so Claude calls the same graph tools the agent calls, plus two that only outside agents get: `investigate_claim` and `investigation_history`. In production FalkorDB and the backend share one Render container, with FalkorDB bound to loopback; locally they run as two Docker services.

- **Backend:** Rust 1.99 (Axum 0.8, Tokio 1.53, `falkordb` crate 0.10, reqwest 0.13 with rustls). FalkorDB v4.22.1 is the only database.
- **Frontend:** Next.js 16.4 / React 19.3, TypeScript 6, Tailwind CSS 4, Bun. Agent trace timeline, live graph canvas, claim intake form.
- **Planner:** graph-driven rules by default; Claude (Anthropic Messages API tool use) when `ANTHROPIC_API_KEY` is set, otherwise any OpenAI-compatible chat-completions model (Groq by default) when `GROQ_API_KEY` is set. LLM failures fall back to the rules, and detectors the model skipped still run before scoring.

## Design decisions

| Decision | Where | Why |
|---|---|---|
| **Deterministic core, AI at the edge.** The LLM picks the next tool; it never sets the score | [`domain/evaluator.rs`](backend/src/domain/evaluator.rs), coverage guard in [`agent/investigator.rs`](backend/src/agent/investigator.rs) | A fraud verdict has to be explainable and repeatable. The same evidence always gives the same score |
| **Inward-only dependencies.** `domain` imports nothing from the crate; `db` depends on `domain`; `tools` and `actions` on `db`; `agent` on those; `api` and `mcp` sit on top | [`backend/src`](backend/src) | The scoring rules can be tested without a database, an LLM or HTTP |
| **Strategy:** one `Planner` trait, three implementations (rules, Claude, any OpenAI-compatible model) | [`agent/planner.rs`](backend/src/agent/planner.rs) | Adding a planner changes no existing code, and tests swap in fakes |
| **Decorator:** `FallbackPlanner<P>` wraps any planner and hands over to the rules when the LLM fails, with a generic label | [`agent/fallback_planner.rs`](backend/src/agent/fallback_planner.rs) | Failure handling lives in one place instead of in each planner |
| **One tool catalog** for the agent, the HTTP API and the MCP server | [`tools/catalog.rs`](backend/src/tools/catalog.rs) | Claude over MCP runs exactly the Cypher the agent runs |
| **Reads and writes split:** `read_rows` (`GRAPH.RO_QUERY` + timeout) vs `rows` (`GRAPH.QUERY`) | [`db/repository.rs`](backend/src/db/repository.rs) | Evidence gathering can't change the graph it's judging |
| **Cypher as named constants, values as parameters** | [`db/queries.rs`](backend/src/db/queries.rs) | Queries are reviewable in one file and injection-safe |
| **An error is not a no:** a failed detector makes the result `INCONCLUSIVE`, never `LOW_RISK` | [`agent/verdict.rs`](backend/src/agent/verdict.rs) | A database hiccup must never clear a claim |

Why there's no repository trait: the agent uses `FalkorGraphRepository` directly. A trait with a single implementation adds indirection with nothing to swap in, and the graph is meant to be load-bearing, not hidden behind an interface.

## Run it locally

Prerequisites: Docker, Rust (stable), bun.

```bash
docker compose up -d falkordb                     # FalkorDB on localhost:6379
cd backend && cargo run --release                 # API on :3001, seeds an empty graph
cd frontend && bun install && bun run dev         # dashboard on :3000 (second terminal)
```

Open http://localhost:3000, pick a claim and investigate it. To see detection on a claim nobody scripted, file a new one with phone `+1-555-019-8821`, location `LOC-441` and shop `SHOP-801`.

Command line:

```bash
cd backend
cargo run --release -- --investigate CLM-48291    # prints the full dossier as JSON
cargo run --release -- --seed                     # reset the graph (investigations write to it)
cargo test                                        # unit tests
cargo test -- --ignored --test-threads=1          # integration tests against live FalkorDB
```

Everything in containers: `docker compose -f docker-compose.full.yml up --build`, then open http://localhost:3000.

Optional environment (see [`.env.example`](.env.example)): `ANTHROPIC_API_KEY` switches to the Claude planner; without it, `GROQ_API_KEY` enables an LLM planner on Groq (`GROQ_MODEL` overrides `openai/gpt-oss-120b`, `LLM_BASE_URL` points it at another OpenAI-compatible endpoint such as OpenRouter, Gemini or Ollama); `LLM_FALLBACK_API_KEY` (plus optional `LLM_FALLBACK_BASE_URL` / `LLM_FALLBACK_MODEL`) adds a backup OpenAI-compatible provider that takes over if the primary is rate-limited; `WEBHOOK_URL` receives escalations, and `FALKORDB_URL` / `FALKORDB_GRAPH` point at another FalkorDB.

## Use it from Claude (MCP)

```bash
cd backend && cargo build --release
claude mcp add diktiax -e FALKORDB_URL=falkor://127.0.0.1:6379 -- "$PWD/target/release/claimgraph-backend" --mcp-server
```

This exposes the 9 read-only graph tools (including `ring_components`, the claim's `algo.WCC` component), `investigation_history` (read-only, past verdicts) and `investigate_claim`, which runs the full agent and **writes to the graph**. For Claude Desktop, use the same command and args in `claude_desktop_config.json`.

## Deploy

The backend runs on Render and the frontend on Vercel.

**Render, one container.** [`deploy/render/Dockerfile`](deploy/render/Dockerfile) packs FalkorDB (Redis + module, bound to loopback) and the backend into one image. The graph is in memory and re-seeded on every start, so a restart always comes back to the complete demo graph. On Render: New Web Service → this repo → Docker, Dockerfile path `deploy/render/Dockerfile`, health check `/api/health`, environment `GROQ_API_KEY`, `GROQ_MODEL`, `ADMIN_TOKEN`. An uptime pinger on `/api/health` keeps the service warm.

**Vercel:** project root `frontend/`, install command `bun install`, and `NEXT_PUBLIC_API_BASE=https://<render-domain>` set *before* building (it's inlined at build time).

The deployed demo is deliberately open: anyone can file and investigate claims on the synthetic graph. Only `POST /api/seed`, which wipes the graph, is locked, and it fails closed: it needs a matching `x-admin-token` header, and is refused outright if `ADMIN_TOKEN` isn't set (unless `DIKTIAX_DEV=1`, which `make run-backend` sets for local runs). Locally, Docker publishes every port on `127.0.0.1` only.

## API

| Endpoint | Purpose |
|----------|---------|
| `GET /api/claims` · `POST /api/claims` | List claims · file a new claim |
| `POST /api/investigate/:id` | Run the agent; returns score, trace, certificate, actions taken and subgraph |
| `GET /api/graph/:id` | Subgraph for the canvas |
| `GET /api/claims/:id/investigations` | Past verdicts for a claim, newest first (404 for an unknown claim) |
| `GET /api/mcp/tools` · `POST /api/mcp/call` | The graph tool catalog over HTTP |
| `POST /api/mcp/call` `{"name":"ring_components","arguments":{"claim_id":"CLM-48291"}}` | Claims in the same `algo.WCC` component, e.g. `{"claim_id":"CLM-48291","claims":["CLM-39102","CLM-44730","CLM-48291"],"size":3}` (404 for an unknown claim) |
| `POST /api/seed` · `GET /api/health` | Reset the demo graph (needs `x-admin-token`; local dev: `DIKTIAX_DEV=1`) · health check |

- `POST /api/investigate` is limited to one request per visitor every 15 s (HTTP 429 with `Retry-After`), so one visitor can't use up the LLM quota for everyone.
- `ring_components` over HTTP (`POST /api/mcp/call`) runs `algo.WCC` across the whole graph, so it is limited to one call per visitor every 3 s (HTTP 429 with `Retry-After`). Inside an investigation the agent runs it at most once, whatever the planner asks.

## Limitations

- The data is synthetic: the demo graph has 68 nodes and the scale benchmark uses 5,040 generated claims. That shows the mechanism on rings built the way the detectors expect, not real-world accuracy.
- Detector severity uses fixed thresholds, not learned weights.
- `detect_kickback_triangle` starts with a label scan of `Accident` (see the query plans above). Anchoring its pattern on `(c:Claim {id: $claim_id})` would make it an index seek; at 5,040 claims a full investigation still takes 17 ms median.
- The Claude and Groq planners' request handling is unit-tested; live LLM API calls are not part of the automated tests.
- The ghost-collision window is 90 days, so newly filed claims stop matching the seeded ring once its dates age out.

## AI usage disclosure

Developed with Claude Code as a coding assistant. The architecture, graph model, fraud patterns and scoring rules are my own design, and every result in this README was verified against a live FalkorDB.
