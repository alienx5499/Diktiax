# LLM-only baseline: recorded run

Output of `python3 scripts/llm_only_baseline.py 3` on 2026-10-17 (Groq, `openai/gpt-oss-120b`, temperature 0):

| Group | Claims | LLM reading one claim (openai/gpt-oss-120b, 3 runs, majority) |
|---|---|---|
| Seeded fraud (ring + cloned VIN) | 5 | 5/5 |
| Seeded clean controls | 3 | 0/3 |
| Evasive ring members | 3 | 0/3 |
| Honest claims with red flags | 2 | 1/2 |

Fraud caught: 5/8. Honest claims flagged: 1/5.
Claims whose answer changed between runs: none

LLM output can drift as the hosted model changes; rerun the script to reproduce.
