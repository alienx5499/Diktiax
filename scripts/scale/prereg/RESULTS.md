# Scale benchmark (pre-registered, seed 20261019)

The answer key in this folder was committed before this file was generated; `git log` shows the order.

5040 claims: 4800 honest, 25 tight rings x 8 (10 evasive), 5 loose rings x 8. Answer key SHA-256 (fixed before scoring): `1638c7a56c4f5135b4f3d2365c7e442405c8f0e1454a04cbd58d22ac87991725`.

| Method | Fraud caught | Evasive ring members caught | Honest flagged (of 4800) | Precision | Recall | F1 |
|---|---|---|---|---|---|---|
| Single-claim rules | 120 / 240 | 0/80 | 2753 | 0.04 | 0.50 | 0.08 |
| Diktiax graph agent | 200 / 240 | 80/80 | 0 | 1.00 | 0.83 | 0.91 |

| Group | Claims | Single-claim rules flagged | Graph agent flagged (payout frozen) | Graph agent sent to audit |
|---|---|---|---|---|
| ring | 120 | 120 | 120 | 0 |
| evasive ring | 80 | 0 | 80 | 0 |
| loose ring | 40 | 0 | 0 | 40 |
| honest | 4800 | 2753 | 0 | 178 |

Graph agent per investigation: median 17.0 ms, p95 22.4 ms, max 74.8 ms (rule-based planner, including verdict write-back). Loading 5040 claims took 4.3 s.
