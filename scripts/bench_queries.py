"""Server-side latency of every graph tool query, as published in the README.

Seed a separate graph first (never benchmark the dev graph):
    FALKORDB_GRAPH=diktiax_bench backend/target/release/diktiax-backend --seed
Then:
    python3 scripts/bench_queries.py [container] [graph] [runs]
Defaults: diktiax-falkordb, diktiax_bench, 200. Uses FalkorDB's own
"internal execution time" via redis-cli inside the container, after 10 warm-up runs.
Delete the graph afterwards: docker exec <container> redis-cli GRAPH.DELETE diktiax_bench
"""
import pathlib
import re
import statistics
import subprocess
import sys

CONTAINER, GRAPH, RUNS = (sys.argv[1:] + ["diktiax-falkordb", "diktiax_bench", "200"][len(sys.argv[1:]):])[:3]
RUNS, WARMUP, CLAIM = int(RUNS), 10, "CLM-48291"
SRC = (pathlib.Path(__file__).parent.parent / "backend/src/db/queries.rs").read_text()


def const(name: str) -> str:
    body = re.search(rf'pub const {name}: &str =\s*"(.*?)";', SRC, re.S).group(1)
    return re.sub(r"\\\n\s*", "", body)


SUBGRAPH = re.search(r'"(MATCH p = .*?)"\s*\)', SRC, re.S).group(1)
SUBGRAPH = re.sub(r"\\\n\s*", "", SUBGRAPH).replace("{{", "{").replace("}}", "}").replace("{depth}", "3")
QUERIES = {
    "explore_claim_subgraph (3 hops)": (f"CYPHER claim_id='{CLAIM}' ", SUBGRAPH),
    "detect_ghost_collision": (f"CYPHER claim_id='{CLAIM}' window_days=90 ", const("GHOST_COLLISION")),
    "detect_shared_contacts": (f"CYPHER claim_id='{CLAIM}' ", const("SHARED_CONTACTS")),
    "detect_vin_cloning": (f"CYPHER claim_id='{CLAIM}' ", const("VIN_CLONING")),
    "detect_kickback_triangle": (f"CYPHER claim_id='{CLAIM}' ", const("KICKBACK_TRIANGLE")),
    "shop_centrality": ("", const("SHOP_DEGREES")),
}

print(f"| Query (on {CLAIM}) | Median ms | p95 ms | Max ms |\n|---|---|---|---|")
for name, (params, query) in QUERIES.items():
    cmd = 'GRAPH.RO_QUERY {} "{}"\n'.format(GRAPH, (params + query).replace('"', '\\"'))
    out = subprocess.run(["docker", "exec", "-i", CONTAINER, "redis-cli"], input=cmd * (RUNS + WARMUP),
                         capture_output=True, text=True, check=True).stdout
    times = sorted(float(t) for t in re.findall(r"internal execution time: ([\d.]+) milliseconds", out)[WARMUP:])
    if len(times) != RUNS:
        sys.exit(f"{name}: expected {RUNS} timings, got {len(times)}: {out[:200]}")
    print(f"| `{name}` | {statistics.median(times):.3f} | {times[int(0.95 * RUNS)]:.3f} | {times[-1]:.3f} |")
