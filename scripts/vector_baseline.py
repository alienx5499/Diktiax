"""Vector-search baseline: does "find claims with similar text" surface fraud rings?

Each claim is embedded from the exact record a single-claim reviewer sees (the dicts in
scripts/llm_only_baseline.py, i.e. the claim file used by backend/tests/baseline.rs), with the
claim id removed. Vectors are stored on a throwaway FalkorDB graph under a vector index
(CREATE VECTOR INDEX) and each claim's nearest neighbours come from db.idx.vector.queryNodes.
The same graph also holds the seed data plus the five constructed claims, so the ring component
the backend uses (algo.WCC, same projection as RING_COMPONENT in backend/src/db/queries.rs) is
measured side by side. The graph is deleted at the end.

    uv run --with falkordb --with sentence-transformers python scripts/vector_baseline.py

Env: FALKORDB_URL (default redis://localhost:6379), EMBED_MODEL (default all-MiniLM-L6-v2).
"""
import ast
import json
import os
import re
import uuid
from pathlib import Path

from falkordb import FalkorDB
from sentence_transformers import SentenceTransformer

ROOT = Path(__file__).resolve().parent.parent
MODEL = os.environ.get("EMBED_MODEL", "sentence-transformers/all-MiniLM-L6-v2")
KS = (3, 5)

# The reviewer records, copied from llm_only_baseline.py (importing it would call the LLM API).
SRC = (ROOT / "scripts/llm_only_baseline.py").read_text()
CLAIMS = ast.literal_eval(re.search(r"^CLAIMS = (\[.*?^\])", SRC, re.S | re.M).group(1))

# Ground truth, fixed by construction (seed.cypher + baseline.rs): which ring each fraud claim is in.
RING = {"CLM-48291": "ghost", "CLM-39102": "ghost", "CLM-44730": "ghost",
        "NEW-1": "ghost", "NEW-2": "ghost", "NEW-3": "ghost",  # evasive: ring phone, LOC-441, SHOP-801
        "CLM-55104": "vin", "CLM-31205": "vin"}
# Constructed claims as baseline.rs ingests them (phone, address, shop), for the WCC comparison.
CONSTRUCTED = {
    "NEW-1": ("+1-555-019-8821", "00 Evasive St", "SHOP-801"),
    "NEW-2": ("+1-555-019-8821", "10 Evasive St", "SHOP-801"),
    "NEW-3": ("+1-555-019-8821", "20 Evasive St", "SHOP-801"),
    "NEW-4": ("+1-555-777-0000", "0 Honest Ave", "SHOP-900"),
    "NEW-5": ("+1-555-777-0001", "1 Honest Ave", "SHOP-901"),
}
WCC = ("CALL algo.WCC({nodeLabels: ['Customer','Claim','Phone','Address','RepairShop','Clinic'], "
       "relationshipTypes: ['FILED_CLAIM','HAS_PHONE','LIVES_AT','REPAIRED_AT','TREATED_AT']}) "
       "YIELD node, componentId WITH node, componentId WHERE node:Claim "
       "RETURN node.id, componentId")


def file_text(rec: dict) -> str:
    return json.dumps({k: v for k, v in rec.items() if k != "claim_id"})


def narrative_text(rec: dict) -> str:
    return f"{rec.get('damage_type', '')}. {rec.get('accident', '')}".strip(". ")


def store_and_query(g, label, items, embed, kmax):
    """items: [(id, text)]. Returns {id: [neighbour ids, nearest first, self excluded]}."""
    vecs = embed([t for _, t in items])
    g.query(f"CREATE VECTOR INDEX FOR (n:{label}) ON (n.embedding) "
            f"OPTIONS {{dimension: {len(vecs[0])}, similarityFunction: 'cosine'}}")
    for (cid, text), v in zip(items, vecs):
        g.query(f"MERGE (n:{label} {{id: $id}}) SET n.text = $t, n.embedding = vecf32($v)",
                {"id": cid, "t": text, "v": v})
    out = {}
    for (cid, _), v in zip(items, vecs):
        rows = g.ro_query(f"CALL db.idx.vector.queryNodes('{label}', 'embedding', $k, vecf32($v)) "
                          "YIELD node, score RETURN node.id ORDER BY score ASC",
                          {"k": kmax + 1, "v": v}).result_set
        out[cid] = [r[0] for r in rows if r[0] != cid][:kmax]
    return out


def score(nn, ring, fraud, honest, k):
    """Per fraud claim: ring-mates found in top-k / ring-mates that exist, and honest neighbours."""
    rows, found, possible, honest_hits = [], 0, 0, 0
    for cid in fraud:
        mates = {o for o in ring if o != cid and ring[o] == ring[cid]}
        top = nn[cid][:k]
        f, h = len(mates & set(top)), sum(o in honest for o in top)
        rows.append((cid, f, len(mates), h, top))
        found, possible, honest_hits = found + f, possible + len(mates), honest_hits + h
    return rows, found, possible, honest_hits


def report(title, nn, ring, fraud, honest, comp=None):
    print(f"\n### {title}\n")
    for k in KS:
        rows, f, p, h = score(nn, ring, fraud, honest, k)
        print(f"k={k}: ring-mates found {f}/{p} ({100 * f / p:.0f}%; random neighbours would "
              f"average {100 * min(k, len(nn) - 1) / (len(nn) - 1):.0f}%); honest claims among fraud "
              f"claims' neighbours {h}/{k * len(fraud)} slots; fraud claims with >=1 honest "
              f"neighbour {sum(r[3] > 0 for r in rows)}/{len(fraud)}\n")
        print("| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |\n|---|---|---|---|")
        for cid, fi, pi, hi, top in rows:
            print(f"| {cid} | {fi}/{pi} | {hi} | {', '.join(top)} |")
        print()
    if comp:
        cf = cp = ch = 0
        for cid in fraud:
            members = {o for o in comp if comp[o] == comp[cid] and o != cid}
            mates = {o for o in ring if o != cid and ring[o] == ring[cid]}
            cf, cp, ch = cf + len(mates & members), cp + len(mates), ch + len(members & honest)
        print(f"Graph ring component (algo.WCC, same projection as ring_components): ring-mates "
              f"found {cf}/{cp} ({100 * cf / cp:.0f}%); honest claims in fraud claims' components {ch}.\n")


def main():
    model = SentenceTransformer(MODEL)
    embed = lambda texts: [v.tolist() for v in model.encode(texts, normalize_embeddings=True)]
    db = FalkorDB.from_url(os.environ.get("FALKORDB_URL", "redis://localhost:6379"))
    name = f"vector_baseline_{uuid.uuid4().hex[:8]}"
    assert name != "claimgraph"
    g = db.select_graph(name)
    try:
        # Seed graph + constructed claims, so WCC sees the same data the backend would.
        seed = "\n".join(l for l in (ROOT / "data/seed.cypher").read_text().splitlines()
                         if not l.lstrip().startswith("//"))
        for stmt in filter(str.strip, seed.split(";")):
            g.query(stmt)
        for cid, (phone, addr, shop) in CONSTRUCTED.items():
            g.query("MERGE (p:Phone {number: $p}) MERGE (a:Address {line: $a}) "
                    "MERGE (s:RepairShop {id: $s}) CREATE (u:Customer {id: 'CUST-' + $c}), "
                    "(c:Claim {id: $c}), (u)-[:HAS_PHONE]->(p), (u)-[:LIVES_AT]->(a), "
                    "(u)-[:FILED_CLAIM]->(c), (c)-[:REPAIRED_AT]->(s)",
                    {"p": phone, "a": addr, "s": shop, "c": cid})
        comp = dict(g.ro_query(WCC).result_set)

        recs = {c["claim_id"]: c for _, _, c in CLAIMS}
        fraud = [c["claim_id"] for _, f, c in CLAIMS if f]
        honest = {c["claim_id"] for _, f, c in CLAIMS if not f}
        print(f"Model: {MODEL}; FalkorDB vector index (cosine); graph {name}")

        nn = store_and_query(g, "ClaimFile", [(i, file_text(r)) for i, r in recs.items()], embed, max(KS))
        report("13 claims, full reviewer record (claim id removed)", nn, RING, fraud, honest, comp)

        seeded = [i for i, r in recs.items() if "accident" in r]  # only these have narrative text
        nn = store_and_query(g, "ClaimNarrative", [(i, narrative_text(recs[i])) for i in seeded], embed, max(KS))
        report("8 seeded claims, narrative only (damage type + accident description)", nn,
               {i: RING[i] for i in seeded if i in RING}, [i for i in fraud if i in seeded],
               honest & set(seeded), {i: comp[i] for i in seeded})

        sample = json.load(open(ROOT / "scripts/scale/llm_sample.json"))
        srecs = {c["claim"]["claim_id"]: c for c in sample}
        sring = {i: i.split("-")[0] for i, c in srecs.items() if c["fraud"]}  # R10-0 -> ring R10
        sfraud = [i for i in sring if sum(r == sring[i] for r in sring.values()) > 1]
        shonest = {i for i, c in srecs.items() if not c["fraud"]}
        nn = store_and_query(g, "SampleClaim", [(i, file_text(c["claim"])) for i, c in srecs.items()], embed, max(KS))
        report(f"50-claim scale sample, full record (only fraud claims with a ring-mate in the "
               f"sample: {len(sfraud)})", nn, sring, sfraud, shonest)
        # Honest neighbours over every fraud claim in the sample, not just those with a mate.
        allf = list(sring)
        for k in KS:
            h = sum(o in shonest for i in allf for o in nn[i][:k])
            print(f"Sample, all {len(allf)} fraud claims, k={k}: honest claims in neighbour slots "
                  f"{h}/{k * len(allf)}; fraud claims with >=1 honest neighbour "
                  f"{sum(any(o in shonest for o in nn[i][:k]) for i in allf)}/{len(allf)}")
    finally:
        g.delete()


if __name__ == "__main__":
    main()
