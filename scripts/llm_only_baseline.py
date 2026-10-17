"""LLM-only baseline: can a model spot the fraud reading one claim file at a time, with no graph?

Each claim is shown to the model on its own, exactly as a single-claim reviewer would see it:
its amount, damage type, accident description, police-report flag, location and repair shop.
Nothing about other claims. The 13 claims and their labels are the same as in
backend/tests/baseline.rs (seeded claims from data/seed.cypher plus the constructed groups);
the labels were fixed by construction before anything ran.

    GROQ_API_KEY=... python3 scripts/llm_only_baseline.py [runs]

Uses Groq's OpenAI-compatible API with openai/gpt-oss-120b at temperature 0.
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request

MODEL = os.environ.get("GROQ_MODEL", "openai/gpt-oss-120b")
URL = "https://api.groq.com/openai/v1/chat/completions"
RUNS = int(sys.argv[1]) if len(sys.argv) > 1 else 3

# (group, is_fraud, claim record as the reviewer sees it)
CLAIMS = [
    ("Seeded fraud (ring + cloned VIN)", True, {"claim_id": "CLM-48291", "amount_usd": 42500, "damage_type": "COLLISION_AND_INJURY", "accident": "Nighttime side-impact at blind intersection. Opposing driver fled.", "police_report_filed": False, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Seeded fraud (ring + cloned VIN)", True, {"claim_id": "CLM-39102", "amount_usd": 38000, "damage_type": "COLLISION_AND_INJURY", "accident": "Late evening T-bone during lane merge. Phantom vehicle left scene.", "police_report_filed": False, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Seeded fraud (ring + cloned VIN)", True, {"claim_id": "CLM-44730", "amount_usd": 35500, "damage_type": "COLLISION_AND_INJURY", "accident": "Sudden-stop rear impact. Lead vehicle drove away.", "police_report_filed": False, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Seeded fraud (ring + cloned VIN)", True, {"claim_id": "CLM-55104", "amount_usd": 58000, "damage_type": "TOTAL_LOSS_FIRE", "accident": "Single-vehicle rollover on rural curve. Vehicle caught fire.", "police_report_filed": False}),
    ("Seeded fraud (ring + cloned VIN)", True, {"claim_id": "CLM-31205", "amount_usd": 56000, "damage_type": "THEFT_TOTAL_LOSS", "accident": "Reported theft and burnout in desert basin.", "police_report_filed": False}),
    ("Seeded clean controls", False, {"claim_id": "CLM-10022", "amount_usd": 4200, "damage_type": "REAR_BUMPER_REPAIR", "accident": "Low-speed rear-end bump at signal. Police report filed.", "police_report_filed": True}),
    ("Seeded clean controls", False, {"claim_id": "CLM-20417", "amount_usd": 6800, "damage_type": "SIDE_PANEL_REPAIR", "accident": "Parking-lot side swipe, witness statement and police report.", "police_report_filed": True}),
    ("Seeded clean controls", False, {"claim_id": "CLM-20988", "amount_usd": 3100, "damage_type": "FRONT_BUMPER_REPAIR", "accident": "Minor fender bender in rush hour, camera footage available.", "police_report_filed": True}),
    ("Evasive ring members", True, {"claim_id": "NEW-1", "amount_usd": 6400, "police_report_filed": True, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Evasive ring members", True, {"claim_id": "NEW-2", "amount_usd": 7900, "police_report_filed": True, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Evasive ring members", True, {"claim_id": "NEW-3", "amount_usd": 5200, "police_report_filed": True, "location": "LOC-441", "repair_shop": "SHOP-801"}),
    ("Honest claims with red flags", False, {"claim_id": "NEW-4", "amount_usd": 31000, "police_report_filed": False, "location": "LOC-900", "repair_shop": "SHOP-900"}),
    ("Honest claims with red flags", False, {"claim_id": "NEW-5", "amount_usd": 27500, "police_report_filed": True, "location": "LOC-901", "repair_shop": "SHOP-901"}),
]

SYSTEM = (
    "You are an insurance fraud reviewer. You see ONE claim file at a time and nothing else. "
    "Decide whether this claim should be flagged for a fraud investigation. "
    'Reply with JSON only: {"flag": true or false, "reason": "<one short sentence>"}.'
)


def ask(claim: dict) -> bool:
    body = json.dumps({
        "model": MODEL,
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": json.dumps(claim)}],
    }).encode()
    for _ in range(6):
        req = urllib.request.Request(URL, data=body, headers={
            "Authorization": f"Bearer {os.environ['GROQ_API_KEY']}", "Content-Type": "application/json",
            "User-Agent": "diktiax-baseline"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                reply = json.load(r)["choices"][0]["message"]["content"]
                return bool(json.loads(reply)["flag"])
        except urllib.error.HTTPError as e:
            if e.code != 429:
                raise
            time.sleep(float(e.headers.get("retry-after") or 5) + 1)
    raise SystemExit("rate limited too many times")


groups = list(dict.fromkeys(g for g, _, _ in CLAIMS))
flags = {c["claim_id"]: [] for _, _, c in CLAIMS}
for run in range(RUNS):
    for _, _, claim in CLAIMS:
        flags[claim["claim_id"]].append(ask(claim))
        time.sleep(2)

def flagged(cid: str) -> bool:  # majority vote across runs
    v = flags[cid]
    return sum(v) * 2 > len(v)

print(f"| Group | Claims | LLM reading one claim ({MODEL}, {RUNS} runs, majority) |\n|---|---|---|")
for g in groups:
    ids = [c["claim_id"] for gg, _, c in CLAIMS if gg == g]
    print(f"| {g} | {len(ids)} | {sum(flagged(i) for i in ids)}/{len(ids)} |")
unstable = [cid for cid, v in flags.items() if 0 < sum(v) < len(v)]
fraud = [c["claim_id"] for _, f, c in CLAIMS if f]
honest = [c["claim_id"] for _, f, c in CLAIMS if not f]
print(f"\nFraud caught: {sum(map(flagged, fraud))}/{len(fraud)}. Honest claims flagged: {sum(map(flagged, honest))}/{len(honest)}.")
print(f"Claims whose answer changed between runs: {unstable or 'none'}")
