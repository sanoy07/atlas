---
title: Retrieval techniques for Atlas (GrepRAG, Sourcegraph, ast-grep)
date: 2026-08-09
status: Research
---

# What to borrow after C5.1-R (88.9% gate)

## Priority (now)

### 1. GrepRAG-style identifier-weighted lexical retrieval

Source: arXiv GrepRAG (2026) — lexical retrieval + identifier rerank + structure dedup
can compete with heavier graph/semantic stacks for repo tasks.

**Atlas mapping (C5.1-L):**

| Signal | Weight class |
|--------|----------------|
| Exact path / basename / stem | Strong |
| Identifier-like token (`OrderService`, `redis-rate-limiter`) | Strong |
| Path component domain match | Medium–strong |
| Generic English (`service`, `process`) | Weak |
| Generated / vendor / contracts noise | Negative |

Implemented in `crates/core/src/lexical_relevance.rs`.

### 2. Sourcegraph-style layered scoring

Not “import Sourcegraph.” Steal **layered signals**:

```text
lexical + identifier + path + role − generated − ubiquitous
```

then C5.1 PageRank on the enriched bag.

### 3. ast-grep / structural retrieval → **C5.2**

Syntax-shaped queries (calls to `OrderService.create`) after lexical bag is good.

### 4. CodeGrep / learned multi-hop agent → **later**

Only after deterministic retrieval plateaus — else failure attribution is muddy.

### 5. Embeddings → **not yet**

Atlas advantage is structure + history + chronology + C4. Don’t dilute with pure semantic similarity first.

## Pipeline (target)

```text
question → entity/path extraction → lexical+identifier retrieval
        → structural expansion (existing)
        → candidate union → C5.1-L score + dedup
        → C5.1 PageRank → packet → C4 → Qwen
```

## Explicit non-goals (now)

- Replace SQLite with vector DB  
- Import GrepRAG/Sourcegraph code wholesale  
- Qwen retrieval agent before deterministic ceiling  
