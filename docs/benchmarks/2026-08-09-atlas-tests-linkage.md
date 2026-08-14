---
title: atlas tests — test↔module path linkage validation
date: 2026-08-09
repository: rwatp-core (primary); vestascan-api, rwatp-notifier (empty-link cases)
status: Complete
---

# Benchmark: Does `atlas tests` link only via explicit path rules?

## Question

Which tests associate with which modules under documented rules, without
ownership claims?

## Ground Truth

RWATP has `tests/<module>/` and some in-module `__tests__`. `tests/rbac/*`
must not auto-link without a `rbac` module.

## Atlas Evaluation

### Commands

```
atlas tests --modules src/modules
atlas tests --json --modules src/modules
```

### Fixture tests

`tests_linkage_fixture.rs` — **6** tests.

### Workspace

356 passed (freeze).

### Representative (RWATP)

- test files considered: **67**
- links: **51**
- unlinked: **16** (includes `tests/rbac/*`)

VestaScan / notifier: **0** links when path heuristic finds no tests —
correct empty report.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |
| Confidence | High for path rules; N/A for semantic ownership |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-tests-linkage.md`
- Limitation: empty B6 is evidence of layout mismatch, not a bug