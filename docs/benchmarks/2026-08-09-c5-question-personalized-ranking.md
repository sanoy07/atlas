---
title: C5.0/C5.1 question-personalized evidence ranking
date: 2026-08-09
repository: rwatp-core (+ fixtures)
status: Draft
---

## Repository

RWATP core (Atlas DB) + unit fixtures.

## Question

Does question-personalized structural PageRank improve top-k file localization
for `"orders timeout"` vs pre-C5 bag ranking, without reintroducing Redis
SUPPORTED causal claims?

## Ground Truth

See `eval/localization/rwatp-orders-timeout.json`.

## Atlas Evaluation

### Commands

```bash
cargo test -p atlas-core personalized_
ATLAS_DB=~/.atlas/rwatp-core.db atlas investigate "orders timeout" --no-ai
```

### Metrics (C5.0)

| Metric | Target |
|--------|--------|
| top-5 hit rate vs gold_files | improve vs C4-only bag |
| hard_negatives in top-5 | prefer 0–1 |
| sacred Redis SUPPORTED | still 0 |

## First RWATP smoke (post C5.1, 2026-08-09)

Question: `orders timeout` · `ATLAS_DB=~/.atlas/rwatp-core.db` · `--no-ai`

| Metric | Result |
|--------|--------|
| FILE top-5 | order.model, order.service, order-history.model, order-history.service, global-settings.service |
| top-5 gold hits | **4** (of gold set) |
| top-5 hard negatives | **0** (no redis-rate-limiter / image-processor) |
| top-10 gold hits | **6** |
| top-10 hard negatives | 1 |
| C4 sacred | still enforced (policy + hard verify) |

vs pre-C5 bag: Redis intent and image-processor often competed in top ranks; order.service was not reliably first-class.

## Classification

| Overall | Improved (localization) |
| Commands needed | investigate --no-ai |
| Confidence | Medium–High on this case; expand gold set next |

## Outcomes

- Decision: `docs/decisions/2026-08-09-c5-question-personalized-evidence-ranking.md`
- Primitive: Question-personalized evidence ranking (structural PageRank + subject affinity)
- Not yet: SCIP, agent tools, CodeQL paths
