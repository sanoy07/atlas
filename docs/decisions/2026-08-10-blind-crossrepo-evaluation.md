---
title: Blind cross-repo evaluation gates next work (JJ + GigaToken)
date: 2026-08-10
status: Implemented
---

## Problem

Atlas localization (C5) and verification (C4) were validated primarily on RWATP and secondarily on VestaScan — repositories Atlas evolved around. That left open whether we were building a **repository-understanding system** or an **RWATP-specific retrieval engine**.

## Methodology validation

- Principle 1: Features earned by production evidence — this decision records measured failure modes on **unfamiliar** corpora before new architecture.
- Principle 4: Validation precedes generalization — dual-repo blind gold before C5.2 / Qwen-first / embeddings.
- Principle 3: Knowledge accumulated — full benchmark `docs/benchmarks/2026-08-10-blind-crossrepo-jj-gigatoken.md`.

## Decision

1. **Treat JJ + GigaToken as standing blind evaluation corpora** under `eval/localization/crossrepo/` with independent gold established without Atlas.
2. **Do not invent architecture from aspiration.** Measured deficiencies order the backlog:
   1. Free-text localization (orientation + multi-hop flow); bridge investigate → impact/focus when a seed path exists or can be resolved.
   2. Path-class / role ranking (demos, assets, CI, notebooks, rename debris vs ProductionSource).
   3. Apply hard_verify to deterministic hypothesis STATUS (existence ≠ SUPPORTED; sacred causal rule).
3. **Qwen / embeddings / C5.2 remain deferred** until (1)–(3) move the blind pass rate.
4. **UTF-8 issue-number scanner panic** is a bug fix, not a feature — allowed immediately.

## Alternatives considered

| Alternative | Why rejected |
|-------------|--------------|
| Proceed to C5.2 multi-hop graph as next milestone | Flow failures may be ranking/seed selection; multi-hop without demotion amplifies demos |
| Turn on Qwen for the same suite first | Reasoning on wrong bags measured poorly before; --no-ai already shows packet-level SUPPORTED abuse |
| Declare cross-repo success from GigaToken 4/7 alone | Gate is joint; JJ 2/7 and orientation collapse are the harder truth |
| Import Aider Repo Map wholesale | Prior decision locked C5 as personalized ranking; this eval reinforces path-class and layout, not PageRank constants |

## Validated outcome

| Suite | Pass rate | Gate |
|-------|-----------|------|
| blind-crossrepo-jj-gigatoken | **40% (6/15)** | fail (need 60%) |
| Overall usefulness (evaluator) | **45/100** | — |

Evidence: `/tmp/atlas-crossrepo-eval/summary.json`, benchmark doc.

## Future

- After (1)–(3), re-run this suite as the **promotion gate** before claiming cross-repo generalization.
- Optionally add RWATP + VestaScan rows to the same report for the full progression table.
- Adversarial WITH_AI pass only after hypothesis STATUS uses hard_verify.
