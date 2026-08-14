---
title: atlas config — configuration provenance validation
date: 2026-08-09
repository: rwatp-core, rwatp-notifier, vestascan-api
status: Complete
---

# Benchmark: Does `atlas config` expose artifact SHA + commit history honestly?

## Question

After config ingest, does Atlas inventory configuration artifacts and
report path touch history without inventing historical content snapshots?

## Ground Truth

`ingest_configuration_artifacts` stores root configs with SHA-256.
Historical bodies per commit are **not** stored.

## Atlas Evaluation

### Commands

```
atlas config
atlas config package.json
atlas config --json
atlas config --json package.json
```

### Fixture tests

`config_fixture.rs` — **6** tests (inventory, SHA, isolation, historical
redirect, missing artifact still reports history).

### Workspace

356 passed (freeze).

### Representative (RWATP post re-ingest)

Inventory **5** artifacts:

| path | kind | touches (approx) |
|------|------|-----------------:|
| .gitignore | gitignore | 4 |
| Dockerfile | dockerfile | 3 |
| package-lock.json | package_lock | 12 |
| package.json | package_json | **59** |
| tsconfig.json | tsconfig | 2 |

`package.json`: artifact_present=true, full SHA-256, content_bytes=7517,
limitations include "CURRENT ingested content only".

Notifier and vestascan-api: same 5-kind inventory pattern after ingest;
B7 declaration_provenance uses configuration_artifacts on all three.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-config-provenance.md`
- Prerequisite: re-ingest required for empty legacy DBs