---
title: C5.0/C5.1 — Question-personalized evidence ranking (not “Aider Repo Map”)
date: 2026-08-09
status: Implemented
---

## Problem

Post-C4 adversarial retest showed:

- Causal overclaim (Redis → SUPPORTED) is largely fixed by C4 hard verify.
- **Retrieval noise remains**: vaguely related files/issues (Redis, image-processor)
  still enter the evidence bag for questions like `"orders timeout"`.
- Local AI (Qwen 4B/7B) cannot salvage a bad bag; it needs a better top set.

Measured gap is **where to look**, not only **what claim is allowed**.

## Methodology validation

| Principle | Application |
|-----------|-------------|
| Features earned by production evidence | C4 retest: ranking/refusal improved; localization still weak |
| Abstractions earned by repetition | Steal Aider’s *algorithmic idea*, not product/constants |
| Validation before generalization | C5.0 golden localization set before broader agent tools |
| Smallest deterministic change | Rank over **existing** `structural_edges`; no SCIP/graph DB |

## Decision

### Name

**Question-Personalized Evidence Ranking** — not “Aider Repo Map.”

Aider supplies the **structural ranking component**. Atlas also has chronology,
provenance, and C4 verification. Ranking selects investigation priority;
**C4 decides support for claims**.

### C5.0 — Golden localization benchmark

Hand-labeled relevant files for RWATP-style questions (starting with
`"orders timeout"` and related cases). Metric: top-k hit rate of ranked files
vs gold; plus sacred Redis non-SUPPORTED.

### C5.1 — Personalized structural ranking

On the neighborhood already present in an evidence packet:

1. Build a directed weighted graph from structural edges (and observed structure).
2. Edge weights: kind boost × √multiplicity × mention/specificity factors
   (Aider-inspired, **Atlas-tuned**, not Aider defaults).
3. Personalization vector: question tokens + seed/core files (chat analog).
4. Power-iteration PageRank (pure Rust).
5. Re-weight `ranked_evidence` file items by PageRank score; demote
   cross-domain path noise when question is order-centric.

**Explicitly deferred:** SCIP, CodeQL paths, graph DB, embeddings, full agent
tool loop, cloning Aider `--map-tokens` / `--map-multiplier-no-files`.

## Alternatives considered

| Alternative | Why rejected now |
|-------------|------------------|
| Full Aider port (tags.scm + NetworkX + token budget) | Larger than earned; TS call edges already stronger in Atlas |
| SCIP first | Better long-term symbols; doesn’t fix bag noise without rank first |
| Agent tool loop first | Tools over a noisy bag still waste turns |
| Copy Aider constants (×8 / 1k tokens) | Docs say no-files multiplier **2**; source may differ — don’t clone knobs |

## Validated outcome

- Unit tests for PageRank personalization and order-vs-redis demotion.
- Wired into `enrich_packet` / `rank_evidence` when structural links available.
- C5.0 gold file under `eval/localization/`.

## Future

- C5.2 symbol tags if rank saturates without defs/refs.
- C5.3 multi-hop path extraction (CodeQL *concept*).
- C5.4–C5.5 structural query + tool loop **after** C5.1 moves localization metrics.
