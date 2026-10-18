# Claims and how to check them

Every number in the README, the blog and the demo video, with the file or test that backs it and the command that reproduces it. Integration tests need a local FalkorDB (`make db-up`) and run from `backend/`; they share one database, so run them with `--test-threads=1`.

## Results

| Claim | Evidence | Reproduce |
|---|---|---|
| On 5,040 generated claims the graph agent froze 200 of 240 fraud claims and 0 of 4,800 honest ones (precision 1.00, recall 0.83, F1 0.91) | [`scripts/scale/RESULTS.md`](scripts/scale/RESULTS.md), first added in `2a0eff0` | `cargo test --release --test scale_benchmark scale_benchmark -- --ignored --exact --nocapture` |
| Per-claim rules flagged 2,789 honest drivers and caught 120 of 240 fraud claims, 0 of 80 evasive ones | same file | same command |
| 40 loose-ring claims and 169 honest claims went to audit, not frozen | same file, group table | same command |
| A pre-registered re-run on a fresh seed gave the same picture: rules 120 of 240 and 2,753 honest flagged; graph 200 of 240 and 0 honest; 178 honest to audit | [`scripts/scale/prereg/RESULTS.md`](scripts/scale/prereg/RESULTS.md); key committed in `9826efe` (18 Oct 02:53 IST) before the results in `2cf0baa` (03:13 IST) | `SCALE_SEED=20_261_019 cargo test --release --test scale_benchmark scale_benchmark -- --ignored --exact` (fails if the key's hash differs from [`answer_key.sha256`](scripts/scale/prereg/answer_key.sha256)) |
| An LLM reading a stratified 50-claim sample one claim at a time caught 3 of 24 fraud claims | `scripts/scale/RESULTS.md`, "LLM-only" section | `python3 scripts/llm_only_baseline.py 1 scripts/scale/llm_sample.json` (needs `GROQ_API_KEY`) |
| A vector-search baseline (all-MiniLM-L6-v2, FalkorDB vector index) finds every seeded ring-mate in its top 3 from the accident stories, but every fraud claim also gets honest neighbours (8 of 8 at k=3 on the full records), and the evasive members don't reach their ring at k=3; the graph's WCC components contain no honest claim | [`scripts/vector_baseline/RESULTS.md`](scripts/vector_baseline/RESULTS.md) | `uv run --with falkordb --with sentence-transformers python scripts/vector_baseline.py` |
| On 13 hand-built claims: rules 5 of 8, an LLM reading each claim 5 of 8, the graph agent 8 of 8 with 0 honest flagged | [`backend/tests/baseline.rs`](backend/tests/baseline.rs) | `cargo test --test baseline -- --ignored --nocapture` |
| 17 ms median per investigation at 5,040 claims, including the verdict write-back | `scripts/scale/RESULTS.md` (median 17.2 ms, p95 23.4 ms); machine-dependent | scale benchmark command above |

## Agent behaviour

| Claim | Evidence | Reproduce |
|---|---|---|
| The ring claim CLM-48291 takes 9 tool calls on a first investigation and 7 on a re-run, with the same score | [`first_run_has_no_memory_and_unchanged_steps`](backend/tests/agent_memory.rs), [`second_run_remembers_and_skips_corroborated_work`](backend/tests/agent_memory.rs) | `cargo test --test agent_memory -- --ignored --test-threads=1` |
| FalkorDB's `algo.WCC` component chooses which linked claims get corroborated (CLM-44730 and CLM-39102 for the ring claim) | [`backend/src/agent/rule_planner.rs`](backend/src/agent/rule_planner.rs), [`backend/tests/agent_investigation.rs`](backend/tests/agent_investigation.rs) | `cargo test --test agent_investigation -- --ignored` |
| `connection_path` (`algo.SPpaths`) links CLM-48291 to CLM-39102 in 2 hops, through SHOP-801 and through MED-201, and finds no path to the clean claim CLM-10022 within 6 hops | [`connection_path_links_ring_claims_only`](backend/tests/graph_algorithms.rs) | `cargo test --test graph_algorithms -- --ignored --test-threads=1` |
| `shop_influence` (`algo.pageRank`) ranks the ring's clinic MED-201 first and SHOP-801 first among repair shops on the seed graph | [`shop_influence_ranks_the_ring_providers_first`](backend/tests/graph_algorithms.rs) | same |
| `ring_communities` (`algo.labelPropagation`) puts CLM-39102, CLM-44730 and CLM-48291 in one community with SHOP-801 and MED-201, apart from the clean claims; the clean pairs CLM-20417/CLM-20988 and CLM-31205/CLM-55104 are communities too (same split as WCC) | [`ring_communities_separate_the_ring`](backend/tests/graph_algorithms.rs) | same |
| Every graph-algorithm query runs through the read-only user | [`graph_algorithms_run_through_the_reader`](backend/tests/read_only_lane.rs) | `cargo test --test read_only_lane -- --ignored --test-threads=1` |
| With a 1 ms timeout the WCC, pageRank and labelPropagation queries fail with the server's timeout error (SPpaths, capped at 6 hops, finishes in ~2 ms and is not timed); a single `algo.*` call is not interrupted mid-run | [`algorithm_queries_obey_the_read_timeout`](backend/tests/algorithm_timeout.rs) | `cargo test --test algorithm_timeout -- --ignored --nocapture` |
| A frozen payout is a hold until its assigned investigator reviews it: anyone else gets 403, a claim that isn't frozen or was already reviewed gets 409, and the review is one statement (`RECORD_REVIEW`) writing the `Review`, its `REVIEWS` and `REVIEWED` edges, the status and the investigator's `open_cases` | [`review_confirms_or_releases_a_frozen_payout`](backend/tests/human_review.rs) | `cargo test --test human_review -- --ignored --test-threads=1` |
| A re-run that reaches a different tier than the latest verdict sets `NEEDS_REVIEW_CONFLICTING_VERDICTS`, `memory.escalated` and an `ESCALATE_TO_HUMAN_REVIEW` action (CLM-55104: HIGH_SUSPICION 75, then MEDIUM_RISK 25 once the cloned-VIN edge is removed); an agreeing re-run omits `escalated` | [`disagreeing_reinvestigation_escalates_to_human_review`](backend/tests/human_review.rs) | same |
| `investigation_history`, `evidence_reuse`, `connection_path`, `shop_influence`, `ring_communities` are external-only: in the MCP and HTTP tool lists, never in the agent's planner catalog | `external_only_tools_stay_out_of_the_planner` in [`backend/src/mcp/protocol.rs`](backend/src/mcp/protocol.rs), `agent_loop_cannot_parse_external_only_tools` in [`backend/src/tools/execute.rs`](backend/src/tools/execute.rs) | `cargo test --lib` |
| `review_claim` is on the stdio MCP server only: not in the HTTP tool list, and `POST /api/mcp/call` refuses it with 400; `POST /api/claims/:id/review` needs the admin token (same fail-closed policy as `/api/seed`) | `writing_tools_are_not_callable_over_http` and `seed_fails_closed` in [`backend/src/api/handlers.rs`](backend/src/api/handlers.rs), `external_only_tools_stay_out_of_the_planner` in [`backend/src/mcp/protocol.rs`](backend/src/mcp/protocol.rs) | `cargo test --lib` |
| A review is final: re-investigating a reviewed claim records the verdict but keeps `CONFIRMED_FRAUD` / `RELEASED_AFTER_REVIEW` and the payout state; a conflict on a never-escalated claim (MEDIUM_RISK, then LOW_RISK) still assigns an investigator | [`reinvestigation_keeps_the_reviewed_status`](backend/tests/review_outcomes.rs), [`escalated_conflict_always_has_an_investigator`](backend/tests/review_outcomes.rs) | `cargo test --test review_outcomes -- --ignored --test-threads=1` |
| `ring_components` runs at most once per investigation, whatever the planner asks | `ring_components_runs_once_per_investigation` in [`backend/src/agent/planner.rs`](backend/src/agent/planner.rs) | `cargo test --lib` |
| A skipped detector still runs before scoring; a failed query never clears a claim | [`backend/src/agent/investigator.rs`](backend/src/agent/investigator.rs) (`coverage_guard`), [`backend/src/agent/verdict.rs`](backend/src/agent/verdict.rs) (`apply_graph_failures`) and their unit tests | `cargo test --lib` |
| The LLM never sets the score | [`FraudRiskEvaluator::evaluate`](backend/src/domain/evaluator.rs) is plain code over the detector results on the target claim, called from [`backend/src/agent/investigator.rs`](backend/src/agent/investigator.rs) after planning ends | `cargo test --lib` |
| Evidence queries run as a FalkorDB user that can only run `GRAPH.RO_QUERY`: its writes and `GRAPH.DELETE` get `NOPERM`, and a failed ACL setup falls back to the shared client | [`reader_user_is_denied_writes_and_delete`](backend/tests/read_only_lane.rs), [`failed_acl_setup_falls_back_to_shared_client`](backend/tests/read_only_lane.rs) | `cargo test --test read_only_lane -- --ignored --test-threads=1` |

## Memory and coordination

| Claim | Evidence | Reproduce |
|---|---|---|
| Two agents share one claim without messaging: the agent freezes and assigns, an MCP reviewer reads the chain, `evidence_reuse` and `connection_path`, confirms fraud with `review_claim`, and the agent's re-run skips 2 calls and keeps the human decision | README "Two agents, one claim" (real run, trimmed) | [`scripts/two_agents_demo.py`](scripts/two_agents_demo.py) (usage in its header) |
| 32 racing assignments of one claim leave exactly one investigator edge | [`concurrent_direct_assignment_creates_one_edge`](backend/tests/agent_memory.rs) | `cargo test --test agent_memory -- --ignored --test-threads=1` |
| 16 concurrent investigations of one claim leave one linear `SUPERSEDES` chain | [`concurrent_investigations_form_one_supersedes_chain`](backend/tests/agent_memory.rs), [`chain_constraints_reject_a_fork`](backend/tests/agent_memory.rs) | same |
| 8 concurrent MCP `investigate_claim` calls on a new claim run the agent once; 7 get the stored verdict | [`concurrent_mcp_investigate_claim_runs_the_agent_once`](backend/tests/agent_memory.rs) | same |
| 8 concurrent re-runs each report memory consistent with the chain they read | [`concurrent_reinvestigations_report_consistent_memory`](backend/tests/agent_memory.rs) | same |
| 16 concurrent reviews of one frozen payout leave one `Review` and free one case; the other 15 get 409 (10 consecutive runs) | [`concurrent_reviews_leave_one_review`](backend/tests/human_review.rs) | `for i in $(seq 10); do cargo test --test human_review concurrent_reviews_leave_one_review -- --ignored --exact; done` |
| `Review.claim_id` is unique: a second review attempt on a still-frozen claim is rejected and rolled back (status and `open_cases` unchanged) | [`review_constraint_rejects_a_second_review`](backend/tests/human_review.rs) | `cargo test --test human_review -- --ignored --test-threads=1` |
| With the review constraint dropped, 80 rounds of 16 racing reviews (1,280 calls) never left a second `Review` on v4.22.1; the constraint stays as the guarantee | [`without_the_constraint_concurrent_review_race_is_observed`](backend/tests/human_review.rs) (reports what it sees; it does not fail on zero duplicates) | same |
| Verdicts survive a new session | [`verdicts_survive_across_sessions`](backend/tests/agent_memory.rs) | same |
| `investigation_history` walks the `SUPERSEDES` chain head first: after 3 investigations of CLM-48291 it returns 3 entries, each `supersedes` the next older one, and the newest `relied_on` contains CLM-39102 | [`history_walks_the_chain_and_evidence_reuse_reads_relied_on`](backend/tests/agent_memory.rs) | same |
| `evidence_reuse` on CLM-39102 returns the 3 CLM-48291 verdicts that `RELIED_ON` it, newest first, with only the newest `is_head`; a claim nothing relied on returns `[]`, an unknown one is "not found" | same test | same |
| Chain-ordered history and `evidence_reuse` run through the read-only user | [`verdict_chain_reads_run_through_the_reader`](backend/tests/read_only_lane.rs) | `cargo test --test read_only_lane -- --ignored --test-threads=1` |
| With the assignment constraint dropped, 1,920 racing calls of the single-statement assignment never left a duplicate on v4.22.1; the constraint stays as the guarantee because FalkorDB doesn't document the statement as atomic | [`without_the_constraint_concurrent_assignment_race_is_observed`](backend/tests/agent_memory.rs) (reports what it sees; it does not fail on zero duplicates) | same |

## Test suite

| Claim | Reproduce |
|---|---|
| 143 Rust tests: 100 unit tests and 43 integration tests against a live FalkorDB | `cargo test -- --list` (all) and `cargo test -- --ignored --list` (live) |

## What these numbers don't show

- The data is generated. The answer key comes from the generator's labels, and the detectors never see those labels, but the rings are built the way the detectors look for them, so the results measure the mechanism and act as a ceiling, not real-world accuracy.
- The hash proves the key wasn't edited after scoring. It can't prove the generator wasn't tuned between runs.
- Latency was measured on a laptop with a local FalkorDB.
