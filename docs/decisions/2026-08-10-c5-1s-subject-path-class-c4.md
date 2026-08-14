---
title: C5.1-S subject resolution + path class ranking + C4 hyp hard_verify
date: 2026-08-10
status: Implemented
---

## Problem

Blind JJ + GigaToken evaluation scored overall usefulness **45/100** with **40% (6/15)** localization pass. Failures were not “need more AI”:

1. Free-text did not resolve to structural subjects (`operation store` ↛ `lib/src/op_store.rs`) even though path-seeded impact/focus worked.
2. Lexical matches on demos, assets, CI, notebooks, rename debris outranked production implementation.
3. Deterministic hypotheses were emitted as **STATUS: SUPPORTED** from ranking alone — C4 policy text without C4 enforcement.

## Methodology validation

- Principle 1: Earned by measured blind-eval failure (N=2 unfamiliar repos).
- Principle 2: Three concrete fixes; no new framework abstraction beyond path-class enum.
- Principle 4: Same suite re-run as promotion gate before Qwen/C5.2.

## Decision

### C5.1-S — Subject resolution (`subject_resolve.rs`)

Pipeline position:

```text
question → concepts/compounds → candidate subjects → structural 1-hop
        → C5.1-R → C5.1-L → C5.1-E → path_class → PageRank blend
```

- Multi-word entities → path forms: `operation_store`, `op_store` (prefix abbreviations), hyphens.
- Score paths via `search_anchor` + full path scan for compound stems.
- Orientation questions seed crate entrypoints (`lib/src/lib.rs`, `src/lib.rs`, README, Cargo.toml).
- Layout-aware module roots: `discover_code_roots` finds `lib/src`, `cli/src`, `src`, `crates/*/src` — map no longer assumes Nest `src/modules`.

### Path class ranking (`path_class.rs`)

Soft (not exclusive) classes: production, library, cli, test, example, demo, benchmark, notebook, documentation, config, generated, vendor, asset, migration.

Multipliers demote demos/PNG/CI/notebooks unless the question asks for them. Applied in lexical score, bag re-rank, and C5.1 PageRank blend.

### C4 hard_verify on all hypotheses

Deterministic “associates with top file” hypotheses now:

1. Emit with `Unresolved` pre-status.
2. Always pass through `hard_verify_hypotheses` before result return.
3. Sacred regression: ranking association **never SUPPORTED**.

**C5 localizes. C4 verifies. Ranking never upgrades support.**

## Alternatives considered

| Alternative | Why rejected |
|-------------|--------------|
| Embeddings / C5.2 multi-hop first | Eval showed seed resolution + demotion closed most of the gap without vectors |
| Absolute exclude demos | Soft signal only — demos can be the answer |
| Qwen before C4 fix | Would amplify false SUPPORTED prose |
| JJ-specific op_store rules | Compounds + path class are generic; only phrase-aware boost for “operation log” uses question text + `lib/` prefix |

## Validated outcome

Same suite `eval/localization/crossrepo/`:

| Metric | Before | After |
|--------|--------|-------|
| Pass rate | **40% (6/15)** | **80% (12/15)** |
| JJ | 2/7 (+crash) | **8/8** |
| GigaToken | 4/7 | 4/7 |
| C4 association SUPPORTED | universal det hyp | **0 violations** |
| Gate (≥60% + zero C4 bad SUPPORT) | fail | **pass** |
| Map subject (JJ) | wrong `src/` | **`lib/src`** |

Artifacts: `/tmp/atlas-crossrepo-eval-v2/summary.json`

## Future

- GigaToken remaining misses: multi-stage encode flow, package rename debris (`gigatok`/`jeton`), orientation top5 still Python-heavy despite bag gold — candidate for rename-aware retrieval + flow expansion (not embeddings-first).
- Qwen 3 Thinking as hypothesis proposer **after** this packet, still C4-gated.
- Re-run RWATP + VestaScan localization suites on the next green CI cycle to confirm no regression.
