---
title: atlas anomalies — deviation from observed patterns (not quality lint)
date: 2026-08-09
status: Implemented
---

## Problem

Operators need a single place to see "what deviates from patterns Atlas
already observed," without inventing a second convention engine or
quality-scoring system.

## Methodology validation

- **Principle 1.** B1 already produces deviations; B5/B6/B7 produce
  test association and declared∩¬observed signals — B9 composes them.
- **Principle 2.** No new detector; reuses `detect_peer_structure`,
  `compute_modules`, `compute_dependency_linkage`.
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-anomalies.md`.

## Decision

Command: `atlas anomalies [path]` (default `src/modules`).

Core: `compute_anomalies` → `AnomaliesReport` / `AnomalyEntry`.

### Anomaly kinds (all marked DERIVED as classifications)

| Kind | Source |
|------|--------|
| `PeerStructureDeviation` | B1 deviations (strict majority threshold) |
| `MissingAssociatedTests` | B5 `has_associated_tests == false` |
| `DeclaredDependencyUnobserved` | B7 declared and not observed |

Each entry carries: observation, expected, evidence strings,
`threshold_note`, `evidence_class`.

### Repository isolation

Inherited from reused compute functions (repo-scoped storage).

### Non-goals / refusals

- Language must not claim "bad architecture", "poor design", "bug", or
  "wrong implementation."
- No suppression of high-volume declared-unobserved packages for taste —
  volume is evidence (codegen/types packages often unobserved).
- No schema / extractor / LLM.

## Alternatives considered

- **Standalone lint with hard-coded architectural rules.** Rejected —
  not earned; would invent "correct" architecture.
- **Filter anomalies by severity UX.** Deferred as product decision.

## Validated outcome

RWATP ~54 anomalies under current rules (dominated by declared-unobserved
+ missing tests + peer gaps). Fixture: `anomalies_fixture` (6 tests).

## Future

UX filtering / severity is a separate product decision after freeze.