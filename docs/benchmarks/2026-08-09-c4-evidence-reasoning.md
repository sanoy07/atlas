---
title: C4-ER evidence reasoning — sacred Redis/timeout regression
date: 2026-08-09
repository: rwatp-core (pattern) + atlas fixtures
status: Complete
---

## Repository

Synthetic fixture mirroring RWATP failure class, plus unit packet for
`orders timeout` + Redis issue #19 + redis-rate-limiter path.

## Question

For vague **“orders timeout”**, when AI (or a synthetic claim) cites:

- `src/infrastructure/rate-limiting/implementations/redis-rate-limiter.ts`
- Issue #19 “Configure Redis Command Timeout to Prevent Grey Failures”

…does Atlas mark a **causal** claim about order processing as **SUPPORTED**?

## Ground Truth

No. Existence of Redis timeout configuration and an issue describing it does
**not** entail that Redis causes order timeouts. At best PLAUSIBLE/UNRESOLVED.
Order-domain files remain the subject-relevant candidates.

## Atlas Evaluation

### Commands used (in order)

1. `cargo test -p atlas-core --lib sacred_`
2. `cargo test -p atlas-core --test reasoning_fixture sacred_orders_timeout_redis_not_supported`
3. `cargo test -p atlas-core --test show_fixture commit_shows_linked_pr_via_message_ref`
4. `cargo test --workspace`

### Manual source reads required

None for the sacred path (deterministic verify).

### Wrong branches followed

Pre-C4: soft verify promoted existence → SUPPORTED (adversarial eval finding).

### False positives

| Query | Unexpected match | Reason | Severity |
|-------|------------------|--------|----------|
| orders timeout | Redis issue #19 as causal support | existence ≠ entailment | Critical (pre-C4) |

### Useful observations

- Ranking promotes order.service over unrelated intent when tokens match
- Supersession notes separate intent vs implementation without “latest wins”
- Message `(#134)` PR link recovers incomplete merge-SHA linkage

## Classification

| Overall | Improved |
| Commands needed | 0 (unit/fixture) |
| Source reads needed | 0 |
| Confidence | High on sacred regression; Medium pending live Ollama re-eval on RWATP |
| Noise removed | Causal existence→support path closed for cross-domain order↔redis |
| Hidden understanding revealed | Verification policy is first-class packet content |

## Outcomes

- Decision produced? Yes — `docs/decisions/2026-08-09-c4-evidence-reasoning.md`
- New primitive earned? **Evidence Reasoning (C4-ER)**: ranking + supersession + hard entailment
- New abstraction earned? No generic framework; concrete rules only
- Regression? Sacred tests guard against Redis false Supported
- Unexpected discoveries? C4-B message linking is high leverage for incomplete GitHub corpus

## Retest plan (same adversarial suite)

After operational GitHub re-ingest (C4-B complete corpus):

1. `ATLAS_DB=~/.atlas/rwatp-core.db atlas investigate "orders timeout" --no-ai --json`
2. Same with local AI enabled; assert no claim with Redis↔order causal language is `supported`
3. Chronology cases: intent after impl does not override; later fix commit demotes older impl description
