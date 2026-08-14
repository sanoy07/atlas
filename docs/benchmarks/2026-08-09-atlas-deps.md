---
title: atlas deps — declared vs observed package linkage
date: 2026-08-09
repository: rwatp-core, rwatp-notifier, vestascan-api
status: Complete
---

# Benchmark: Does `atlas deps` separate package.json declaration from structural observation?

## Question

Which packages are declared in package.json, which appear as
`UNRESOLVED:external` structural targets, and which are both / only-one?

## Ground Truth

After RWATP re-ingest, `configuration_artifacts` holds package.json;
structural edges hold external imports. Runtime usage is not knowable.

## Atlas Evaluation

### Commands

```
atlas deps --limit 8
atlas deps --json
```

### Fixture tests

`deps_fixture.rs` — **7** tests.

### Workspace

356 passed (freeze).

### Representative (RWATP post re-ingest)

| Metric | Value |
|--------|------:|
| declaration_provenance | configuration_artifacts (package.json) |
| declared | 70 |
| observed | 58 |
| both | 47 |
| declared-only | 23 |
| observed-undeclared | 11 |

Top observed: zod, mongoose (DECLARED+OBSERVED).

Cross-repo: notifier declared=24/observed=16; vestascan declared=64/observed=55;
all use configuration_artifacts provenance after ingest.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |
| Confidence | High for static evidence |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-deps.md`
- Limitation: OBSERVED ≠ runtime