---
title: C5.1-L identifier-weighted lexical bag — suite 9/9
date: 2026-08-09
repository: rwatp-core
status: Complete
---

## Question

Does GrepRAG-style identifier weighting + structure dedup lift the full
localization suite (including issue #19) without embeddings or agents?

## Results

```text
pass_rate: 100.0% (9/9)
gate (≥80%): PASS → C5.2 symbol/path fidelity
hard_negatives in top-5: 0 on all cases
```

| Workflow | passed | avg top5 gold |
|----------|--------|---------------|
| bug_localization | 4/4 | 3.0 |
| system_flow | 3/3 | 3.3 |
| issue_implementation | 2/2 | 3.0 |

## Classification

| Overall | Optimal for current gold suite |
| Next justified | C5.2 structural/AST retrieval |
| Not next | Qwen retrieval agent, embeddings |

## Outcomes

- Decision: `docs/decisions/2026-08-09-c5-1l-lexical-identifier-ranking.md`  
- Research: `docs/research/2026-08-09-retrieval-techniques-for-atlas.md`  
