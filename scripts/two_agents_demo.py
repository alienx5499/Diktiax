"""Two agents, one graph: the Diktiax agent over HTTP and an MCP client acting as the SIU reviewer
over stdio. Start the API on a throwaway graph first, e.g.
  FALKORDB_GRAPH=demo_two PORT=3012 DIKTIAX_DEV=1 backend/target/release/diktiax-backend
then: python3 scripts/two_agents_demo.py backend/target/release/diktiax-backend demo_two http://127.0.0.1:3012
Delete the graph afterwards (GRAPH.DELETE demo_two)."""
import json, subprocess, sys, time, urllib.request

BIN, GRAPH, HTTP = sys.argv[1], sys.argv[2], sys.argv[3]
env = {"FALKORDB_GRAPH": GRAPH, "FALKORDB_URL": "falkor://127.0.0.1:6379", "PATH": "/usr/bin:/bin"}
mcp = subprocess.Popen([BIN, "--mcp-server"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL, text=True, env=env)
rid = 0


def rpc(method, params=None):
    global rid
    rid += 1
    mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}) + "\n")
    mcp.stdin.flush()
    while True:
        line = mcp.stdout.readline()
        msg = json.loads(line)
        if msg.get("id") == rid:
            return msg


def tool(name, args):
    res = rpc("tools/call", {"name": name, "arguments": args})["result"]
    text = res["content"][0]["text"]
    print(f"[reviewer over MCP] → {name} {json.dumps(args)}")
    print(f"[reviewer over MCP] ← {text[:600]}")
    return json.loads(text) if not res.get("isError") else text


def http(method, path):
    req = urllib.request.Request(HTTP + path, method=method)
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())


rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "reviewer", "version": "1"}})
print("[Diktiax agent over HTTP] POST /api/investigate/CLM-48291")
d = http("POST", "/api/investigate/CLM-48291")
assign = next((a["details"] for a in d["remediation"]["actions_log"] if "assigned" in a["details"].lower()), "")
print(f"[Diktiax agent over HTTP] ← {d['risk_tier']} {d['risk_score']}, {len(d['agent_trace'])} steps; {assign}")
hist = tool("investigation_history", {"claim_id": "CLM-48291"})
tool("evidence_reuse", {"claim_id": "CLM-39102"})
tool("connection_path", {"claim_a": "CLM-48291", "claim_b": "CLM-39102"})
reviewer = assign.split("investigator ")[-1].split()[0].strip(".,") if "investigator " in assign else "INV-202"
tool("review_claim", {"claim_id": "CLM-48291", "decision": "confirm_fraud", "reviewer": reviewer,
                      "note": "Shared clinic and shop with CLM-39102; ring confirmed."})
time.sleep(16)
print("[Diktiax agent over HTTP] POST /api/investigate/CLM-48291 (re-run)")
d2 = http("POST", "/api/investigate/CLM-48291")
kept = [a["details"] for a in d2["remediation"]["actions_log"] if "kept" in a["details"].lower() or "status" in a["details"].lower()]
m = d2.get("memory") or {}
print(f"[Diktiax agent over HTTP] ← {d2['risk_tier']} {d2['risk_score']}, {len(d2['agent_trace'])} steps; "
      f"memory: prior={m.get('prior_investigations')}, agrees={m.get('agrees_with_latest')}, "
      f"skipped={m.get('skipped_corroboration')}; actions: {kept}")
mcp.terminate()
