# Vector-search baseline over the claim text

Question: if you embed each claim's own file and look up its nearest neighbours, do the ring-mates come back, and do honest claims stay out?

## Method

- **Text.** Each claim is embedded from the record a single-claim reviewer sees: the dicts in `scripts/llm_only_baseline.py`, the same 13 claims and labels as `backend/tests/baseline.rs`. The claim id is removed, since the scale sample's ids encode the ring (`R10-0`). Two variants:
  - *Full record*: the JSON of amount, damage type, accident description, police-report flag, location id and repair-shop id, whichever fields that claim has.
  - *Narrative only*: damage type plus the accident description. Only the 8 seeded claims have these fields; the 5 constructed claims have none.
- **Model.** `sentence-transformers/all-MiniLM-L6-v2` (384 dimensions, normalised).
- **Store and search.** On a throwaway FalkorDB v4.22.1 graph: `CREATE VECTOR INDEX ... OPTIONS {dimension: 384, similarityFunction: 'cosine'}`, vectors set with `vecf32`, neighbours from `db.idx.vector.queryNodes` (k+1 results, the claim itself dropped). The graph is deleted at the end.
- **k.** 3 and 5.
- **Graph comparison.** The same throwaway graph also gets `data/seed.cypher` and the five constructed claims (with the phone, address and shop that `baseline.rs` gives them). `algo.WCC` then runs with the projection that `ring_components` uses.
- **Ground truth (fixed by construction).** Ghost ring: CLM-48291, CLM-39102, CLM-44730 plus the three evasive members (NEW-1..3). Cloned-VIN ring: CLM-55104, CLM-31205. Honest: the three clean controls plus the two red-flag claims (NEW-4, NEW-5). In the scale sample the ring is the claim-id prefix (`L00-5` is in ring L00).
- **Chance level.** On average, k random neighbours out of n-1 recover k/(n-1) of the ring-mates.

## Reproduce

```
uv run --with falkordb --with sentence-transformers python scripts/vector_baseline.py
```

Environment variables: `FALKORDB_URL` (default `redis://localhost:6379`) and `EMBED_MODEL`. Two runs gave identical output (sentence-transformers 6.1.0, torch 2.14.1, CPU).

## Summary

| Set | k | Ring-mates found (vector) | Chance | Ring-mates found (graph WCC) | Honest claims in fraud claims' neighbour slots (vector) | Fraud claims with ≥1 honest neighbour (vector) | Honest claims in fraud claims' components (graph) |
|---|---|---|---|---|---|---|---|
| 13 claims, full record | 3 | 14/32 (44%) | 25% | 32/32 | 10/24 | 8/8 | 0 |
| 13 claims, full record | 5 | 17/32 (53%) | 42% | 32/32 | 18/40 | 8/8 | 0 |
| 8 seeded, narrative only | 3 | 8/8 (100%) | 43% | 8/8 | 5/15 | 4/5 | 0 |
| 8 seeded, narrative only | 5 | 8/8 (100%) | 71% | 8/8 | 11/25 | 5/5 | 0 |
| 50-claim sample, full record (6 claims with a mate in the sample) | 3 | 2/6 (33%) | 6% | n/a | 2/18 | 2/6 | n/a |
| 50-claim sample, full record (6 claims with a mate in the sample) | 5 | 2/6 (33%) | 10% | n/a | 7/30 | 4/6 | n/a |
| 50-claim sample, all 24 fraud claims | 3 | n/a | n/a | n/a | 5/72 | 4/24 | n/a |
| 50-claim sample, all 24 fraud claims | 5 | n/a | n/a | n/a | 10/120 | 6/24 | n/a |

What this shows:
- **Narrative text works on the seeded rings.** On the 8 seeded claims the narrative alone puts every ring-mate in the top 3. The seeded ghost-ring descriptions share a "the other driver left" story, and the two cloned-VIN total-loss stories land next to each other. Vectors do well on this part.
- **Vectors do not separate the rings from the honest claims.** Every fraud claim has at least one honest neighbour at k=5, in every set. The graph component holds no honest claim.
- **Text misses the link between the evasive members and the ring they joined.** NEW-1..3 share the ring's phone, location and shop. In the full-record variant their nearest neighbours are each other and the two honest red-flag claims (NEW-4, NEW-5), not the seeded ghost claims, because their records have the same short shape. Across the two halves the link is weak: at k=3 no seeded ghost claim and no evasive member has the other half among its neighbours, and at k=5 only CLM-44730 and NEW-2 meet, and NEW-1 picks up CLM-44730, each in fifth place. The ring comes back in two halves, sorted by how the file looks, while WCC joins all six through the shared phone and shop.
- **The 50-claim sample is weak evidence.** See the caveats below.

## Caveats

- **Small sets.** With 13 claims (and 8 in the narrative variant) a top-5 list covers 42% (71%) of the pool, so chance is high and one claim moves a percentage a lot.
- **Same author.** The seeded narratives were written by the same author to read like typical staged-crash stories. That makes text similarity easier than in real claims, where ring members write their stories separately.
- **The sample has no narrative text.** The scale sample (`scripts/scale/llm_sample.json`) has only amount, police flag and ids for location, shop, clinic and adjuster. Its generated ids also name the ring (`SHOP-R10`, `LOC-L3`), so any similarity between ring members is a token match on those ids, not on narrative. That is also why fraud claims mostly sit next to other fraud claims (only 5/72 honest neighbour slots at k=3).
- **Few ring-mates in the sample.** The stratified sample keeps at most two members of any ring, so ring-mate recall can be measured only for the three loose-ring pairs (6 claims). Its graph WCC column is n/a because the sample has no phones or addresses. The graph's scale numbers are in `scripts/scale/RESULTS.md`.
- **One small model, raw cosine.** No tuning, no reranking, no hybrid or field-weighted search. A model trained on claim text, or exact matching on the shop and phone fields, would do differently. Exact matching on shared ids is the graph approach under another name.

## Full output


### 13 claims, full reviewer record (claim id removed)

k=3: ring-mates found 14/32 (44%; random neighbours would average 25%); honest claims among fraud claims' neighbours 10/24 slots; fraud claims with >=1 honest neighbour 8/8

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| CLM-48291 | 2/5 | 1 | CLM-44730, CLM-39102, CLM-20417 |
| CLM-39102 | 2/5 | 1 | CLM-48291, CLM-44730, CLM-10022 |
| CLM-44730 | 2/5 | 1 | CLM-48291, CLM-39102, CLM-10022 |
| CLM-55104 | 1/1 | 2 | CLM-31205, CLM-10022, CLM-20988 |
| CLM-31205 | 1/1 | 2 | CLM-55104, CLM-20988, NEW-4 |
| NEW-1 | 2/5 | 1 | NEW-2, NEW-3, NEW-5 |
| NEW-2 | 2/5 | 1 | NEW-3, NEW-1, NEW-5 |
| NEW-3 | 2/5 | 1 | NEW-2, NEW-1, NEW-5 |

k=5: ring-mates found 17/32 (53%; random neighbours would average 42%); honest claims among fraud claims' neighbours 18/40 slots; fraud claims with >=1 honest neighbour 8/8

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| CLM-48291 | 2/5 | 3 | CLM-44730, CLM-39102, CLM-20417, CLM-20988, CLM-10022 |
| CLM-39102 | 2/5 | 3 | CLM-48291, CLM-44730, CLM-10022, CLM-20988, CLM-20417 |
| CLM-44730 | 3/5 | 2 | CLM-48291, CLM-39102, CLM-10022, CLM-20988, NEW-2 |
| CLM-55104 | 1/1 | 2 | CLM-31205, CLM-10022, CLM-20988, CLM-44730, CLM-48291 |
| CLM-31205 | 1/1 | 2 | CLM-55104, CLM-20988, NEW-4, NEW-3, NEW-2 |
| NEW-1 | 3/5 | 2 | NEW-2, NEW-3, NEW-5, NEW-4, CLM-44730 |
| NEW-2 | 3/5 | 2 | NEW-3, NEW-1, NEW-5, NEW-4, CLM-44730 |
| NEW-3 | 2/5 | 2 | NEW-2, NEW-1, NEW-5, NEW-4, CLM-31205 |

Graph ring component (algo.WCC, same projection as ring_components): ring-mates found 32/32 (100%); honest claims in fraud claims' components 0.


### 8 seeded claims, narrative only (damage type + accident description)

k=3: ring-mates found 8/8 (100%; random neighbours would average 43%); honest claims among fraud claims' neighbours 5/15 slots; fraud claims with >=1 honest neighbour 4/5

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| CLM-48291 | 2/2 | 1 | CLM-39102, CLM-44730, CLM-20988 |
| CLM-39102 | 2/2 | 1 | CLM-48291, CLM-44730, CLM-20988 |
| CLM-44730 | 2/2 | 1 | CLM-48291, CLM-39102, CLM-20988 |
| CLM-55104 | 1/1 | 0 | CLM-31205, CLM-44730, CLM-48291 |
| CLM-31205 | 1/1 | 2 | CLM-55104, CLM-20417, CLM-20988 |

k=5: ring-mates found 8/8 (100%; random neighbours would average 71%); honest claims among fraud claims' neighbours 11/25 slots; fraud claims with >=1 honest neighbour 5/5

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| CLM-48291 | 2/2 | 2 | CLM-39102, CLM-44730, CLM-20988, CLM-20417, CLM-55104 |
| CLM-39102 | 2/2 | 3 | CLM-48291, CLM-44730, CLM-20988, CLM-10022, CLM-20417 |
| CLM-44730 | 2/2 | 2 | CLM-48291, CLM-39102, CLM-20988, CLM-10022, CLM-55104 |
| CLM-55104 | 1/1 | 2 | CLM-31205, CLM-44730, CLM-48291, CLM-20988, CLM-10022 |
| CLM-31205 | 1/1 | 2 | CLM-55104, CLM-20417, CLM-20988, CLM-44730, CLM-48291 |

Graph ring component (algo.WCC, same projection as ring_components): ring-mates found 8/8 (100%); honest claims in fraud claims' components 0.


### 50-claim scale sample, full record (only fraud claims with a ring-mate in the sample: 6)

k=3: ring-mates found 2/6 (33%; random neighbours would average 6%); honest claims among fraud claims' neighbours 2/18 slots; fraud claims with >=1 honest neighbour 2/6

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| L00-0 | 0/1 | 0 | L01-7, L01-2, L02-4 |
| L00-5 | 0/1 | 1 | R01-2, R08-6, H-02208 |
| L01-2 | 1/1 | 0 | L01-7, L00-0, L03-1 |
| L01-7 | 1/1 | 0 | L01-2, L00-0, L02-4 |
| L03-1 | 0/1 | 0 | L01-2, L00-0, L01-7 |
| L03-6 | 0/1 | 1 | L00-5, H-00000, R03-6 |

k=5: ring-mates found 2/6 (33%; random neighbours would average 10%); honest claims among fraud claims' neighbours 7/30 slots; fraud claims with >=1 honest neighbour 4/6

| Fraud claim | Ring-mates in top-k | Honest in top-k | Top-k (nearest first) |
|---|---|---|---|
| L00-0 | 0/1 | 1 | L01-7, L01-2, L02-4, L04-3, H-04600 |
| L00-5 | 0/1 | 2 | R01-2, R08-6, H-02208, H-01104, L03-6 |
| L01-2 | 1/1 | 1 | L01-7, L00-0, L03-1, L02-4, H-01656 |
| L01-7 | 1/1 | 0 | L01-2, L00-0, L02-4, L03-1, L04-3 |
| L03-1 | 0/1 | 0 | L01-2, L00-0, L01-7, L02-4, L04-3 |
| L03-6 | 0/1 | 3 | L00-5, H-00000, R03-6, H-02392, H-00184 |

Sample, all 24 fraud claims, k=3: honest claims in neighbour slots 5/72; fraud claims with >=1 honest neighbour 4/24
Sample, all 24 fraud claims, k=5: honest claims in neighbour slots 10/120; fraud claims with >=1 honest neighbour 6/24
