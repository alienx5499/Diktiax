# Scale benchmark

5040 claims: 4800 honest, 25 tight rings x 8 (10 evasive), 5 loose rings x 8. Answer key SHA-256 (fixed before scoring): `e03eb6b91c75b8abde29cccce320ad95ddb39030dbc10a75c07280eeb0e1399a`.

| Method | Fraud caught | Evasive ring members caught | Honest flagged (of 4800) | Precision | Recall | F1 |
|---|---|---|---|---|---|---|
| Single-claim rules | 120 / 240 | 0/80 | 2789 | 0.04 | 0.50 | 0.08 |
| Diktiax graph agent | 200 / 240 | 80/80 | 0 | 1.00 | 0.83 | 0.91 |

| Group | Claims | Single-claim rules flagged | Graph agent flagged (payout frozen) | Graph agent sent to audit |
|---|---|---|---|---|
| ring | 120 | 120 | 120 | 0 |
| evasive ring | 80 | 0 | 80 | 0 |
| loose ring | 40 | 0 | 0 | 40 |
| honest | 4800 | 2789 | 0 | 169 |

Graph agent per investigation: median 17.2 ms, p95 23.4 ms, max 34.8 ms (rule-based planner, including verdict write-back). Loading 5040 claims took 4.3 s.

## LLM-only, on a stratified 50-claim sample

`python3 scripts/llm_only_baseline.py 1 scripts/scale/llm_sample.json` (Groq `openai/gpt-oss-120b`, temperature 0, one run to stay inside the provider's daily token budget). The LLM sees each claim's own fields only.

| Group | Claims | LLM flagged | Graph agent flagged (from the full run above) |
|---|---|---|---|
| ring | 8 | 3/8 | 8/8 |
| evasive ring | 8 | 0/8 | 8/8 |
| loose ring | 8 | 0/8 | 0/8 (all 8 sent to audit) |
| honest | 26 | 2/26 | 0/26 |

## Reading this honestly

- The data is generated, so the result reflects how the rings were built: tight rings reuse a phone, location, shop, clinic and adjuster, which is exactly what the detectors look for.
- Loose rings (shared location and shop only) are a real blind spot for freezing: the graph agent sends them to audit (MEDIUM) but does not freeze them.
- The audit tier also catches 169 of 4,800 honest claims (3.5%) through coincidental shop/location overlap or shared household addresses.
- Reproduce: `cd backend && cargo test --release --test scale_benchmark -- --ignored --nocapture`. The answer key and its SHA-256 are in this folder.
- Verify the key: `shasum -a 256 scripts/scale/answer_key.json` must print the hash at the top of this file.
- The key is produced by the same seeded generator (SplitMix64, fixed seed) that builds the graph, and it is hashed before any claim is investigated. The hash shows the key wasn't edited after scoring. It can't prove the generator wasn't tuned between runs.
