---
title: C5.0 broader localization suite — first full score
date: 2026-08-09
repository: rwatp-core
status: Complete
---

## Question

Does C5.1 question-personalized ranking **reliably** localize the right
evidence across bug / flow / issue workflows — without confusing relevance
with truth (C4)?

## Method

- Gold cases: `eval/localization/rwatp-*.json` (9 cases, 3 workflows)
- Schema: `eval/localization/SCHEMA.md`
- Scorer: `eval/localization/score_localization.mjs`
- Binary: post-C5.1 atlas · DB: `~/.atlas/rwatp-core.db` · cwd: rwatp-core
- Mode: `investigate --no-ai --json`

### Gate

≥80% cases pass, where pass =

1. top-5 hard negatives = 0  
2. top-5 gold hits ≥ min(3, |gold|) **OR** top-1 hit  
3. C4 sacred expectations (when present)

If gate fails → **diagnose ranking**; do not add AI tools / SCIP / agent loop.

## Results (first full run)

| pass_rate | 5/9 = **55.6%** |
| gate | **FAIL** |
| next | Diagnose ranking failure modes |

| Workflow | n | passed | avg top5 gold | avg top5 hard |
|----------|---|--------|---------------|---------------|
| bug_localization | 4 | 2 | 1.25 | 0 |
| system_flow | 3 | 2 | 1.33 | 0.33 |
| issue_implementation | 2 | 1 | 1.0 | 0 |

### Per-case (summary)

| Case | Result | Notes |
|------|--------|-------|
| orders timeout | PASS | top1 order.model; hard 0 |
| concurrent race | PASS (thin) | top1 hit but top5_gold=1 only |
| order error clue | **FAIL** | 0 gold in top5 — localization miss |
| auth login failure | **FAIL** | weak auth file recall |
| explain order flow | **FAIL** | hard-neg intrusion + thin gold |
| order created | PASS | top1 hit |
| how auth works | PASS (thin) | top1 hit, top5_gold=1 |
| issue 12 share class | PASS | share-class/listing localized |
| issue 19 redis timeout | **FAIL** | redis gold not in top5 (domain flip miss) |

## Failure modes (earned diagnosis)

1. **Vague error-style input** (“error around order processing”) still fails to
   converge on order files — ranking cannot invent anchors the retriever missed.
2. **Auth questions** undershoot identity auth files (AuthService etc.) unless
   path tokens already in candidate bag.
3. **Flow questions** still admit high-centrality or off-domain noise.
4. **Inverse domain** (issue #19 Redis *as subject*) does not yet promote Redis
   infra over unrelated hubs — personalization is asymmetric / bag-limited.
5. Gate **OR top1** can pass thin recall (concurrent race / how-auth) — keep
   metric, but report top5_gold_hits separately for honesty.

## Classification

| Overall | Blocked on gate (55% < 80%) |
| C5.1 hypothesis | Partially confirmed (orders timeout strong); **not** suite-reliable |
| Sacred C4 | Held on orders-timeout class cases |
| New architecture earned? | **No** — diagnose retrieval + rank before C5.2 |

## Outcomes

- Expanded gold set + scorer delivered
- Gate decision: **stay on C5.1 diagnosis**, not SCIP/agent
- Artifacts under `/tmp/atlas-c5-localization-score/`

## Next (measured only)

1. Inspect failed case packets: empty gold ⇒ retrieval (investigate) vs rank.
2. If candidates never include gold files → **retrieval** fix.
3. If gold in bag but not top-5 → **rank** fix.
4. Re-run suite; only when pass_rate ≥ 80% open C5.2.
