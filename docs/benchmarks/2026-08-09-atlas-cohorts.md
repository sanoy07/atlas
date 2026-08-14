---
title: atlas cohorts — directory co-change validation
date: 2026-08-09
repository: rwatp-core, vestascan-api, rwatp-notifier
status: Complete
---

# Benchmark: Does `atlas cohorts` report co-change pairs without domain claims?

## Question

Which immediate child directories of a subject repeatedly co-occur in the
same commits, above an explicit threshold?

## Ground Truth

Co-change is defined solely by shared commits touching two dirs — not
business domain identity.

## Atlas Evaluation

### Commands

```
atlas cohorts
atlas cohorts src          # notifier layers
atlas cohorts --json
```

### Fixture tests

`cohorts_fixture.rs` — **6** tests (pairs, threshold, isolation, singletons).

### Workspace

356 passed (freeze).

### Representative (RWATP `src/modules`, threshold=2)

Pairs include blockchain×core **12**, core×identity **12**,
compliance×identity **10**. Singletons listed. Methodology text forbids
domain interpretation.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-cohorts.md`
- Limitation: shallow git history compresses counts (ingest scope)