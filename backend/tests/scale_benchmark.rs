//! Scale benchmark: 5,040 generated claims (4,800 honest, 25 tight and 5 loose hidden fraud
//! rings), scored by single-claim rules and by the graph agent. Asserts the headline counts. Writes the answer key (and its SHA-256, computed before any
//! scoring), a stratified sample for the LLM-only script, and a results table to scripts/scale/.
//! Requires a running FalkorDB:
//!   cargo test --release --test scale_benchmark -- --ignored --nocapture
//!
//! Pre-registered run with another seed (key committed before the results):
//!   SCALE_SEED=<n> cargo test --release --test scale_benchmark prereg_key -- --ignored
//!   (commit scripts/scale/prereg/answer_key.*), then
//!   SCALE_SEED=<n> cargo test --release --test scale_benchmark scale_benchmark -- --ignored --nocapture
//! With SCALE_SEED set, output goes to scripts/scale/prereg/, the run fails unless the key hash
//! equals the committed answer_key.sha256, and the default-seed headline asserts are skipped.

mod common;

use claimgraph_backend::actions::{status_for, ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, PlannerConfig};
use claimgraph_backend::db::FalkorGraphRepository;
use claimgraph_backend::domain::NewClaim;
use common::TestGraph;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Instant;

const HONEST: usize = 4_800;
const RINGS: usize = 25;
const RING_SIZE: usize = 8;
const EVASIVE_RINGS: usize = 10; // 40%: police reports filed, small amounts
/// Loose rings share only a crash location and a body shop: no shared phones, clinics or
/// adjusters, police reports filed. A hard case the detectors are not built to escalate.
const LOOSE_RINGS: usize = 5;
const SHOPS: u64 = 300;
const LOCATIONS: u64 = 400;
const CLINICS: u64 = 150;
const ADJUSTERS: u64 = 40;
const HIGH_AMOUNT: f64 = 25_000.0;
const OUT: &str = "../scripts/scale";
const PREREG_OUT: &str = "../scripts/scale/prereg";
const DEFAULT_SEED: u64 = 20_261_018;

/// `SCALE_SEED` if set (pre-registered mode), else None.
fn prereg_seed() -> Option<u64> {
    std::env::var("SCALE_SEED").ok().map(|s| {
        s.replace('_', "")
            .parse()
            .expect("SCALE_SEED must be an integer")
    })
}

/// Deterministic generator (SplitMix64) so the dataset is identical on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

#[derive(Clone)]
struct Labelled {
    claim: NewClaim,
    fraud: bool,
    evasive: bool,
    group: String,
}

fn generate(seed: u64) -> Vec<Labelled> {
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    // Honest population: unique people, random providers. ~2% are households sharing an address,
    // and some draw the same shop and location by chance: realistic look-alikes.
    for i in 0..HONEST {
        let household = if rng.chance(2) { i / 2 } else { 100_000 + i };
        out.push(Labelled {
            claim: NewClaim {
                claim_id: Some(format!("H-{i:05}")),
                claimant_name: format!("Honest {i}"),
                phone: format!("+1-555-1{i:06}"),
                address: format!("{household} Main St"),
                vehicle_vin: format!("HVIN{i:07}"),
                location_id: format!("LOC-{}", rng.below(LOCATIONS)),
                repair_shop_id: format!("SHOP-{}", rng.below(SHOPS)),
                clinic_id: rng
                    .chance(40)
                    .then(|| format!("MED-{}", rng.below(CLINICS))),
                adjuster_id: Some(format!("ADJ-{}", rng.below(ADJUSTERS))),
                amount: 1_500.0 + rng.below(38_000) as f64,
                police_report: rng.chance(70),
            },
            fraud: false,
            evasive: false,
            group: "honest".into(),
        });
    }
    // Fraud rings: each reuses one location, shop, clinic and adjuster, and members share phones.
    for r in 0..RINGS {
        let evasive = r < EVASIVE_RINGS;
        let (loc, shop) = (
            format!("LOC-R{r}-{}", rng.below(LOCATIONS)),
            format!("SHOP-R{r}"),
        );
        let (clinic, adj) = (format!("MED-R{r}"), format!("ADJ-{}", rng.below(ADJUSTERS)));
        for m in 0..RING_SIZE {
            out.push(Labelled {
                claim: NewClaim {
                    claim_id: Some(format!("R{r:02}-{m}")),
                    claimant_name: format!("Ring {r} member {m}"),
                    phone: format!("+1-555-9{r:02}{:02}", m / 3),
                    address: format!("{} Ring Rd {r}", m % 4),
                    vehicle_vin: format!("RVIN{r:02}{m:02}"),
                    location_id: loc.clone(),
                    repair_shop_id: shop.clone(),
                    clinic_id: Some(clinic.clone()),
                    adjuster_id: Some(adj.clone()),
                    amount: if evasive {
                        3_000.0 + rng.below(9_000) as f64
                    } else {
                        26_000.0 + rng.below(30_000) as f64
                    },
                    police_report: evasive,
                },
                fraud: true,
                evasive,
                group: if evasive { "evasive ring" } else { "ring" }.into(),
            });
        }
    }
    for r in 0..LOOSE_RINGS {
        let (loc, shop) = (format!("LOC-L{r}"), format!("SHOP-L{r}"));
        for m in 0..RING_SIZE {
            out.push(Labelled {
                claim: NewClaim {
                    claim_id: Some(format!("L{r:02}-{m}")),
                    claimant_name: format!("Loose ring {r} member {m}"),
                    phone: format!("+1-555-7{r:02}{m:03}"),
                    address: format!("{m} Loose Ln {r}"),
                    vehicle_vin: format!("LVIN{r:02}{m:02}"),
                    location_id: loc.clone(),
                    repair_shop_id: shop.clone(),
                    clinic_id: rng
                        .chance(40)
                        .then(|| format!("MED-{}", rng.below(CLINICS))),
                    adjuster_id: Some(format!("ADJ-{}", rng.below(ADJUSTERS))),
                    amount: 4_000.0 + rng.below(12_000) as f64,
                    police_report: true,
                },
                fraud: true,
                evasive: false,
                group: "loose ring".into(),
            });
        }
    }
    out
}

fn rules_flag(c: &NewClaim) -> bool {
    !c.police_report || c.amount >= HIGH_AMOUNT
}

#[derive(Default)]
struct Tally {
    tp: usize,
    fp: usize,
    fn_: usize,
    evasive_caught: usize,
}
impl Tally {
    fn add(&mut self, l: &Labelled, flagged: bool) {
        match (l.fraud, flagged) {
            (true, true) => {
                self.tp += 1;
                self.evasive_caught += l.evasive as usize;
            }
            (false, true) => self.fp += 1,
            (true, false) => self.fn_ += 1,
            _ => {}
        }
    }
    fn row(&self, name: &str, evasive_total: usize) -> String {
        let p = self.tp as f64 / (self.tp + self.fp).max(1) as f64;
        let r = self.tp as f64 / (self.tp + self.fn_).max(1) as f64;
        let f1 = if p + r > 0.0 {
            2.0 * p * r / (p + r)
        } else {
            0.0
        };
        format!(
            "| {name} | {} / {} | {}/{evasive_total} | {} | {p:.2} | {r:.2} | {f1:.2} |",
            self.tp,
            self.tp + self.fn_,
            self.evasive_caught,
            self.fp
        )
    }
}

/// Answer key JSON and its SHA-256 (hex). Labels alone are fixed by claim id and identical for
/// every seed, so a pre-registered key also records the seed and the SHA-256 of the full
/// generated claims: the hash then commits to the exact dataset that gets scored. The default
/// key keeps its original format so the committed files stay reproducible.
fn answer_key(data: &[Labelled], prereg_seed: Option<u64>) -> (String, String) {
    let labels: Vec<_> = data
        .iter()
        .map(|l| json!({"claim_id": l.claim.claim_id, "fraud": l.fraud, "evasive": l.evasive}))
        .collect();
    let key = match prereg_seed {
        Some(seed) => {
            let claims: Vec<_> = data.iter().map(|l| &l.claim).collect();
            let dataset = serde_json::to_vec(&claims).expect("dataset json");
            json!({"seed": seed, "dataset_sha256": hex::encode(Sha256::digest(dataset)), "labels": labels})
        }
        None => json!(labels),
    };
    let key_json = serde_json::to_string_pretty(&key).expect("key json");
    let key_hash = hex::encode(Sha256::digest(key_json.as_bytes()));
    (key_json, key_hash)
}

fn write_key(out: &str, key_json: &str, key_hash: &str) {
    std::fs::write(format!("{out}/answer_key.json"), key_json).expect("write key");
    std::fs::write(
        format!("{out}/answer_key.sha256"),
        format!("{key_hash}  answer_key.json\n"),
    )
    .expect("write hash");
}

/// Pre-registration step: writes only the answer key for `SCALE_SEED` to scripts/scale/prereg/.
/// No graph, no scoring.
#[test]
#[ignore = "pre-registration step; needs SCALE_SEED"]
fn prereg_key() {
    let seed = prereg_seed().expect("set SCALE_SEED");
    assert_ne!(seed, DEFAULT_SEED, "pre-register a new seed");
    std::fs::create_dir_all(PREREG_OUT).expect("output dir");
    let (key_json, key_hash) = answer_key(&generate(seed), Some(seed));
    write_key(PREREG_OUT, &key_json, &key_hash);
    println!("seed {seed}: answer key SHA-256 {key_hash}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a live FalkorDB; generates 5,040 claims (about 95 s)"]
async fn scale_benchmark() {
    let prereg = prereg_seed();
    let out = if prereg.is_some() { PREREG_OUT } else { OUT };
    std::fs::create_dir_all(out).expect("output dir");
    let data = generate(prereg.unwrap_or(DEFAULT_SEED));

    // 1. Fix the answer key before anything is scored. Pre-registered mode never rewrites the
    // key: it must match the committed hash.
    let (key_json, key_hash) = answer_key(&data, prereg);
    if let Some(seed) = prereg {
        let committed = std::fs::read_to_string(format!("{out}/answer_key.sha256"))
            .expect("pre-registered answer_key.sha256 (run prereg_key and commit it first)");
        let committed = committed.split_whitespace().next().unwrap_or_default();
        assert_eq!(
            key_hash, committed,
            "seed {seed}: answer key differs from the pre-registered hash"
        );
    } else {
        write_key(out, &key_json, &key_hash);
    }

    // 2. Load into a throwaway graph (seed(true) drops any leftover one; deleted on drop).
    // Named after the answer-key hash: the graph is identified by the data it was built from.
    let graph = TestGraph::named(format!("scale_{}", &key_hash[..8]));
    let repo = FalkorGraphRepository::connect(&graph.url, &graph.name)
        .await
        .expect("FalkorDB running");
    repo.seed(true).await.expect("seed");
    let load = Instant::now();
    for l in &data {
        repo.ingest_claim(l.claim.clone()).await.expect("ingest");
    }
    let load_s = load.elapsed().as_secs_f64();

    // 3. Single-claim rules.
    let (mut rules, mut graph_t) = (Tally::default(), Tally::default());
    let rules_flags: Vec<bool> = data.iter().map(|l| rules_flag(&l.claim)).collect();
    for (l, &f) in data.iter().zip(&rules_flags) {
        rules.add(l, f);
    }
    let mut graph_flags = Vec::with_capacity(data.len());
    let mut graph_audit = Vec::with_capacity(data.len());

    // 4. Graph agent (rule-based planner) on every claim; verdicts are written back, as in production.
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::new(None).expect("client"));
    let investigator = ClaimInvestigator::new(repo, dispatcher, PlannerConfig::RuleBased);
    let mut ms = Vec::with_capacity(data.len());
    for l in &data {
        let id = l.claim.claim_id.as_deref().expect("id");
        let t = Instant::now();
        let dossier = investigator.investigate(id).await.expect("dossier");
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let flagged = status_for(&dossier.risk_tier).1;
        graph_flags.push(flagged);
        graph_audit.push(dossier.risk_tier == "MEDIUM_RISK");
        graph_t.add(l, flagged);
    }
    ms.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let pct = |p: f64| ms[((ms.len() as f64 - 1.0) * p) as usize];

    // 5. Stratified sample for the LLM-only script: 8 ring, 8 evasive, 8 loose ring, 26 honest.
    let mut sample = Vec::new();
    for (group, n) in [
        ("ring", 8),
        ("evasive ring", 8),
        ("loose ring", 8),
        ("honest", 26),
    ] {
        let picks: Vec<_> = data.iter().filter(|l| l.group == group).collect();
        let step = picks.len() / n;
        for l in picks.iter().step_by(step).take(n) {
            let c = &l.claim;
            sample.push(json!({"group": group, "fraud": l.fraud, "claim": {
                "claim_id": c.claim_id, "amount_usd": c.amount, "police_report_filed": c.police_report,
                "location": c.location_id, "repair_shop": c.repair_shop_id,
                "clinic": c.clinic_id, "adjuster": c.adjuster_id}}));
        }
    }
    // The LLM sample belongs to the default run only.
    if prereg.is_none() {
        std::fs::write(
            format!("{out}/llm_sample.json"),
            serde_json::to_string_pretty(&sample).expect("sample"),
        )
        .expect("write sample");
    }

    let (mut rule_hits, mut agent_hits, mut audit_hits) = (vec![], vec![], vec![]);
    let mut by_group = String::from("| Group | Claims | Single-claim rules flagged | Graph agent flagged (payout frozen) | Graph agent sent to audit |\n|---|---|---|---|---|\n");
    for g in ["ring", "evasive ring", "loose ring", "honest"] {
        let idx: Vec<usize> = (0..data.len()).filter(|&i| data[i].group == g).collect();
        let r = idx.iter().filter(|&&i| rules_flags[i]).count();
        let a = idx.iter().filter(|&&i| graph_flags[i]).count();
        let au = idx.iter().filter(|&&i| graph_audit[i]).count();
        by_group.push_str(&format!("| {g} | {} | {r} | {a} | {au} |\n", idx.len()));
        rule_hits.push(r);
        agent_hits.push(a);
        audit_hits.push(au);
    }
    let evasive_total = EVASIVE_RINGS * RING_SIZE;
    let table = format!(
        "| Method | Fraud caught | Evasive ring members caught | Honest flagged (of {HONEST}) | Precision | Recall | F1 |\n|---|---|---|---|---|---|---|\n{}\n{}",
        rules.row("Single-claim rules", evasive_total),
        graph_t.row("Diktiax graph agent", evasive_total)
    );
    let title = match prereg {
        Some(seed) => format!(
            "Scale benchmark (pre-registered, seed {seed})\n\nThe answer key in this folder was \
             committed before this file was generated; `git log` shows the order."
        ),
        None => "Scale benchmark".into(),
    };
    let report = format!(
        "# {title}\n\n{} claims: {HONEST} honest, {RINGS} tight rings x {RING_SIZE} ({EVASIVE_RINGS} evasive), \
         {LOOSE_RINGS} loose rings x {RING_SIZE}. Answer key SHA-256 (fixed before scoring): `{key_hash}`.\n\n{table}\n\n{by_group}\n\
         Graph agent per investigation: median {:.1} ms, p95 {:.1} ms, max {:.1} ms (rule-based planner, \
         including verdict write-back). Loading {} claims took {load_s:.1} s.\n",
        data.len(), pct(0.5), pct(0.95), pct(1.0), data.len()
    );
    // Keep the hand-written sections (LLM-only run, honest reading) that follow the generated part.
    let path = format!("{out}/RESULTS.md");
    let notes = std::fs::read_to_string(&path)
        .ok()
        .and_then(|old| old.find("\n## ").map(|i| old[i..].to_string()))
        .unwrap_or_default();
    std::fs::write(&path, format!("{report}{notes}")).expect("write results");
    println!("\n{report}");

    // Headline counts (groups: ring, evasive ring, loose ring, honest); checked after the files are written.
    // They belong to the default seed; a pre-registered run is scored, not asserted.
    assert_eq!(data.len(), 5_040, "claims");
    if prereg.is_some() {
        return;
    }
    assert_eq!(rule_hits, vec![120, 0, 0, 2_789], "per-claim rules");
    assert_eq!(agent_hits, vec![120, 80, 0, 0], "graph agent frozen");
    assert_eq!(audit_hits, vec![0, 0, 40, 169], "graph agent audit");
    assert_eq!(
        (rules.tp, rules.fp, rules.evasive_caught),
        (120, 2_789, 0),
        "rules totals"
    );
    assert_eq!(
        (graph_t.tp, graph_t.fp, graph_t.evasive_caught),
        (200, 0, 80),
        "graph totals"
    );
}
