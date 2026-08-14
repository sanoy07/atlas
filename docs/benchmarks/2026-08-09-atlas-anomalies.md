---
title: atlas anomalies — deviation composition validation
date: 2026-08-09
repository: rwatp-core (primary)
status: Complete
---

# Benchmark: Does `atlas anomalies` restate observed deviations without quality judgments?

## Question

Can Atlas list deviations from peer structure, missing test association,
and declared-but-unobserved packages with explicit thresholds and
provenance?

## Ground Truth

Anomalies must reuse B1/B5/B6/B7 outputs. High volume of declared-unobserved
(codegen/types) is expected, not a failure.

## Atlas Evaluation

### Commands

```
atlas anomalies
atlas anomalies --json
```

### Fixture tests

`anomalies_fixture.rs` — **6** tests (including language scan against
"bad architecture").

### Workspace

356 passed (freeze).

### Representative (RWATP)

`total_anomalies` ≈ **54** under freeze smoke (peer gaps + missing tests +
declared-unobserved). JSON retains `methodology` and per-entry
`threshold_note`.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-anomalies.md`
- Limitation: volume is evidence; filtering is a product decision