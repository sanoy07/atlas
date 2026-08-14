---
title: C5.1-R — Retrieval recall (issue / domain / flow seeds)
date: 2026-08-09
status: Implemented
---

## Problem

C5.0 suite scored **55.6%** after C5.1 ranking alone. Diagnosis:

| Layer | Status |
|-------|--------|
| C4 verification | OK on sacred orders case |
| C5.1 ranking | OK **when gold is in the bag** |
| Retrieval | **Bottleneck** — gold never entered candidates |

Failed classes:

1. Error-clue → no order files in bag  
2. Issue #19 Redis → redis infra missing or truncated  
3. Auth → partial auth neighborhood  
4. Order flow → gold present but hubs/noise + bag shape  

## Methodology validation

- Earned by measured suite failures (not speculation)  
- Smallest deterministic change: expand seeds/anchors before investigate  
- No Qwen expansion, no SCIP, no graph DB  
- Same gold set (honesty gate)  

## Decision

Add **`retrieval_expand`** applied in `build_evidence_packet` / `options_from_issue`:

1. **Issue-anchored:** detect `#N` / `--issue` → issue anchors, closing PRs, merge/commit files, title compounds (`share class` → `share-class`)  
2. **Domain fragments:** order / auth / redis path fragments via `search_anchor`  
3. **Flow multi-stage:** extra fragments when question is flow-shaped  
4. **Seed priority:** force seeds to **front** of core candidates (fix append-then-truncate drop)  
5. **Title-overlap seed sort:** feature issues prefer paths matching title tokens  

Ranking (C5.1) and verification (C4) unchanged in role.

## Alternatives considered

| Alternative | Why not now |
|-------------|-------------|
| Bigger PageRank only | Cannot rank missing files |
| Qwen agent exploration | Not earned; retrieval is deterministic gap |
| SCIP | Deferred until suite gate holds without it |

## Validated outcome

| Suite | Before C5.1-R | After |
|-------|---------------|-------|
| pass_rate | 55.6% (5/9) | **88.9% (8/9)** |
| gate ≥80% | FAIL | **PASS** |
| bug_localization | 2/4 | **4/4** |
| system_flow | 2/3 | **3/3** |
| issue_implementation | 1/2 | 1/2 (#19 still thin top-5) |

Remaining: `rwatp-issue-19-redis-timeout` — redis gold in bag, top-5 gold=2 without top-1 (entry noise). Track for rank/seed polish; gate already earned.

## Future

- C5.2 symbol/path fidelity **now justified by gate**  
- Optional: tighten issue-19 top-5 without changing gold  
- Do not loosen gold to chase 9/9  
