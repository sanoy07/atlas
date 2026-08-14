---
title: C5.1-R retrieval recall — suite re-score
date: 2026-08-09
repository: rwatp-core
status: Complete
---

## Question

Do deterministic retrieval expansions (issue/domain/flow seeds + seed-front cap)
lift the C5.0 localization suite past the 80% gate without new AI?

## Method

Same 9 gold cases · same scorer · C5.1 ranking + C5.1-R retrieval.

```bash
ATLAS_BIN=./target/debug/atlas \
ATLAS_DB=~/.atlas/rwatp-core.db \
REPO=/path/to/rwatp-core \
node eval/localization/score_localization.mjs
```

## Results

| Metric | Pre C5.1-R | Post C5.1-R |
|--------|------------|-------------|
| pass_rate | 55.6% (5/9) | **88.9% (8/9)** |
| gate (≥80%) | FAIL | **PASS** |
| bug_localization | 2/4 | **4/4** |
| system_flow | 2/3 | **3/3** |
| issue_implementation | 1/2 | 1/2 |

### Fixes that moved the needle

1. Domain path fragments for order/auth/redis  
2. Issue `#N` / `--issue` → PR/commit files + compound title fragments  
3. Seeds forced to **front** of core (survive MAX_CORE=16)  
4. Title-overlap seed ranking (share-class before settlement noise)  

### Remaining fail

`rwatp-issue-19-redis-timeout`: top5_gold=2, top1=false — bag includes rate-limiter/connection; ranking still admits non-gold redis/index & command noise ahead of some gold. **Not** a missing-bag failure.

## Classification

| Overall | Improved — gate earned |
| Architecture | Retrieval → rank → C4 unchanged |
| Next justified | C5.2 symbol/path fidelity |
| Do not | Add Qwen tools solely for #19 thin miss |

## Outcomes

- Decision: `docs/decisions/2026-08-09-c5-1r-retrieval-recall.md`  
- Empirical pipeline confirmed:  
  **Retrieval determines what Atlas considers; C5.1 what to look at first; C4 what to believe.**  
