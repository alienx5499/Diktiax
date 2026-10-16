//! Baseline: per-claim red-flag rules vs the graph agent, on the seeded claims plus two
//! constructed groups. Prints a markdown table. Requires a running FalkorDB:
//!   cargo test --test baseline -- --ignored --nocapture

mod common;

use claimgraph_backend::actions::{status_for, ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, PlannerConfig};
use claimgraph_backend::db::FalkorGraphRepository;
use claimgraph_backend::domain::NewClaim;
use common::TestGraph;

const SEEDED_FRAUD: [&str; 5] = [
    "CLM-48291",
    "CLM-39102",
    "CLM-44730",
    "CLM-55104",
    "CLM-31205",
];
const SEEDED_CLEAN: [&str; 3] = ["CLM-10022", "CLM-20417", "CLM-20988"];
const HIGH_AMOUNT: f64 = 25_000.0;

/// What a single-claim reviewer sees: the claim and its own accident, nothing shared.
/// Flags on the classic red flags: no police report, or a high amount.
async fn per_claim_rules_flag(repo: &FalkorGraphRepository, claim_id: &str) -> bool {
    let g = repo.claim_subgraph(claim_id, 1).await.expect("subgraph");
    let prop = |label: &str, key: &str| {
        g.nodes
            .iter()
            .find(|n| n.label == label && (label != "Claim" || n.id == claim_id))
            .and_then(|n| n.properties.get(key).cloned())
    };
    let amount = prop("Claim", "amount_claimed")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let police = prop("Accident", "police_report_filed").and_then(|v| v.as_bool());
    police == Some(false) || amount >= HIGH_AMOUNT
}

async fn graph_agent_flags(investigator: &ClaimInvestigator, claim_id: &str) -> bool {
    let dossier = investigator.investigate(claim_id).await.expect("dossier");
    status_for(&dossier.risk_tier).1
}

/// `who` = [claimant name, phone, address, VIN, location id, repair shop id].
fn claim(who: [&str; 6], amount: f64, police: bool) -> NewClaim {
    let [name, phone, address, vin, loc, shop] = who;
    NewClaim {
        claim_id: None,
        claimant_name: name.into(),
        phone: phone.into(),
        address: address.into(),
        vehicle_vin: vin.into(),
        location_id: loc.into(),
        repair_shop_id: shop.into(),
        clinic_id: None,
        adjuster_id: None,
        amount,
        police_report: police,
    }
}

struct Group {
    name: &'static str,
    fraud: bool,
    ids: Vec<String>,
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB (docker compose up -d falkordb)"]
async fn per_claim_rules_vs_graph_agent() {
    let graph = TestGraph::new("claimgraph_baseline");
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB running");
    repo.seed(true).await.expect("seed");

    // Ring members who learned the rules: police report filed, ordinary amounts. They still reuse
    // the ring's phone, crash location and body shop.
    let mut evasive = Vec::new();
    for (i, amount) in [6_400.0, 7_900.0, 5_200.0].into_iter().enumerate() {
        let (name, address, vin) = (
            format!("Evasive {i}"),
            format!("{i}0 Evasive St"),
            format!("EVASIVEVIN{i}"),
        );
        let who = [
            name.as_str(),
            "+1-555-019-8821",
            address.as_str(),
            vin.as_str(),
            "LOC-441",
            "SHOP-801",
        ];
        let c = claim(who, amount, true);
        evasive.push(repo.ingest_claim(c).await.expect("ingest").claim_id);
    }
    // Honest drivers with classic red flags: a hit-and-run with no police report, a big repair.
    // Nothing shared with anyone.
    let mut red_flag = Vec::new();
    for (i, (amount, police)) in [(31_000.0, false), (27_500.0, true)]
        .into_iter()
        .enumerate()
    {
        let owned = [
            format!("Honest {i}"),
            format!("+1-555-777-000{i}"),
            format!("{i} Honest Ave"),
            format!("HONESTVIN{i}"),
            format!("LOC-90{i}"),
            format!("SHOP-90{i}"),
        ];
        let c = claim(owned.each_ref().map(String::as_str), amount, police);
        red_flag.push(repo.ingest_claim(c).await.expect("ingest").claim_id);
    }

    let groups = [
        Group {
            name: "Seeded fraud (ring + cloned VIN)",
            fraud: true,
            ids: SEEDED_FRAUD.map(String::from).to_vec(),
        },
        Group {
            name: "Seeded clean controls",
            fraud: false,
            ids: SEEDED_CLEAN.map(String::from).to_vec(),
        },
        Group {
            name: "Evasive ring members (police report, small amounts)",
            fraud: true,
            ids: evasive,
        },
        Group {
            name: "Honest claims with red flags (no police / high amount)",
            fraud: false,
            ids: red_flag,
        },
    ];

    // Rules read the graph before the agent writes verdicts, so they see the original data.
    let mut rule_hits = Vec::new();
    for g in &groups {
        let mut n = 0;
        for id in &g.ids {
            n += per_claim_rules_flag(&repo, id).await as usize;
        }
        rule_hits.push(n);
    }
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::new(None).expect("client"));
    let investigator = ClaimInvestigator::new(repo, dispatcher, PlannerConfig::RuleBased);
    let mut agent_hits = Vec::new();
    for g in &groups {
        let mut n = 0;
        for id in &g.ids {
            n += graph_agent_flags(&investigator, id).await as usize;
        }
        agent_hits.push(n);
    }

    println!("\n| Group | Claims | Per-claim rules flagged | Graph agent flagged |");
    println!("|---|---|---|---|");
    for (i, g) in groups.iter().enumerate() {
        let total = g.ids.len();
        println!(
            "| {} | {total} | {}/{total} | {}/{total} |",
            g.name, rule_hits[i], agent_hits[i]
        );
    }
    let (mut tp_r, mut fp_r, mut tp_a, mut fp_a) = (0, 0, 0, 0);
    for (i, g) in groups.iter().enumerate() {
        if g.fraud {
            tp_r += rule_hits[i];
            tp_a += agent_hits[i]
        } else {
            fp_r += rule_hits[i];
            fp_a += agent_hits[i]
        }
    }
    println!("\nFraud caught: rules {tp_r}/8, graph {tp_a}/8. Honest claims flagged: rules {fp_r}/5, graph {fp_a}/5.");

    // The claims the README and blog make about this table.
    assert_eq!(agent_hits, vec![5, 0, 3, 0], "graph agent");
    assert_eq!(
        rule_hits[2], 0,
        "per-claim rules miss the evasive ring members"
    );
    assert_eq!(
        rule_hits[3], 2,
        "per-claim rules flag both honest red-flag claims"
    );
}
