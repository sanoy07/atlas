---
title: C5.1-L — Identifier-weighted lexical relevance (GrepRAG/Sourcegraph-inspired)
date: 2026-08-09
status: Implemented
---

## Problem

After C5.1-R (88.9% suite), remaining gaps and research pointed to:

- Generic token matches (`service`, `order` in `order-history`) drowning **primary** artifacts
- Issue #19 bag correct but weak top-k vs noise
- Need smarter **lexical** retrieval before PageRank — not embeddings

## Decision

Add **C5.1-L** (`lexical_relevance.rs`) between candidate union and C5.1 PageRank:

1. **Identifier-weighted path scores** (exact path/stem/identifier ≫ generic words)  
2. **Structure-aware dedup** (exact path, stem, per-dir caps, weak `index.ts`)  
3. **Primary-artifact boosts** (`order.service` > `order-history`; redis-rate-limiter > barrels)  
4. Align PageRank **affinity** with the same primary-artifact preferences  

Do **not** add embeddings, CodeGrep agent, or ast-grep yet.

## Validated outcome

| Suite | C5.1-R only | + C5.1-L |
|-------|-------------|----------|
| pass_rate | 88.9% (8/9) | **100% (9/9)** |
| issue #19 | FAIL (thin top-5) | **PASS** |
| hard negatives top-5 | ~0 | **0 all cases** |

## Pipeline

```text
expand seeds (C5.1-R) → investigate → force seeds
  → C5.1-L lexical score + dedup → cap bag
  → C5.1 PageRank packet rank → C4 verify
```

## Future

- C5.2: AST/structural retrieval (ast-grep *concept*)  
- C5.3: multi-hop graph exploration  
- Learned retrieval agent / embeddings only if deterministic plateaus  
