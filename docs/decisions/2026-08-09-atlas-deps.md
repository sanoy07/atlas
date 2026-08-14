---
title: atlas deps — package.json declarations vs structural external observations
date: 2026-08-09
status: Implemented
---

## Problem

`package.json` lists dependencies; structural edges record
`UNRESOLVED:external:*` imports. Operators needed the join:

> What is **declared** vs what is **observed** statically in source?

Without that join, "we depend on X" and "we import X" are conflated.

## Methodology validation

- **Principle 1.** Coupling already surfaced externals; B7 makes the
  declaration side explicit and comparable.
- **Principle 2.** No new extractor — reuses `structural_edges` and
  `configuration_artifacts` (with disk fallback only if artifact missing).
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-deps.md`.

## Decision

Command: `atlas deps [--limit N] [--json]`.

Core: `compute_dependency_linkage` → `DependencyLinkageReport`.

### Evidence source

1. **DECLARED:** `package.json` from `configuration_artifacts` preferred;
   else working-tree file if present (provenance string states which).
   Sections: dependencies, devDependencies, optionalDependencies,
   peerDependencies.
2. **OBSERVED:** edges whose `target_file` matches
   `UNRESOLVED:external:…`, package root extraction:
   - unscoped: first path segment
   - scoped: `@scope/name`

### Deterministic vs derived

- Declaration presence and observation rows: **DETERMINISTIC** projections.
- Aggregation counts (`total_*`, per-package observation tallies):
  **DERIVED** aggregation over those rows (methodology array on report).

### Repository isolation

`configuration_artifact(..., repo_path)` and
`structural_edges_from_prefix("", repo_path)`.

### Non-goals / refusals

- No claim of **runtime** usage or bundle inclusion.
- No inference that declared-only packages are "dead code" as a quality
  judgment — only "unobserved under structural edges".
- No schema / extractor / LLM.

## Alternatives considered

- **Declared-only report.** Insufficient — loses the structural half.
- **New npm lockfile parser as primary source.** Rejected for v1;
  package.json + edges suffice for the question asked.

## Validated outcome

Post re-ingest RWATP: provenance=`configuration_artifacts (package.json)`;
declared=70, observed=58, both=47. Fixture: `deps_fixture` (7 tests).

## Future

Timing benchmarks not performed. Lockfile-based resolution deferred.