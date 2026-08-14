---
title: atlas modules — deterministic inventory of subject child directories
date: 2026-08-09
status: Implemented
---

## Problem

After B1–B4, Atlas could report peer patterns, coupling, drill-down, and
authors, but had no single inventory answer for:

> "What module directories exist under `src/modules` (or another parent),
> and what deterministic evidence counts attach to each?"

Operators had to recombine `atlas conventions`, `atlas inspect`, and ad-hoc
path inspection. That friction appeared repeatedly on RWATP and VestaScan
module maps.

## Methodology validation

- **Principle 1.** Earned by multi-repo dogfood needing a stable module list
  before coupling/conventions interpretation.
- **Principle 2.** Reuses `files`, `commits`+`commit_files` (prefix),
  `structural_edges`, and the same path-test heuristic used for roles —
  no second convention detector.
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-modules.md`.
- **Principle 4.** Validated on RWATP, rwatp-notifier (with subject `src`),
  and vestascan-api.

## Decision

Command: `atlas modules [path]` (default `src/modules`).

Core: `compute_modules` → transient `ModulesReport` / `ModuleEntry`.

### Scope / algorithm

1. **Discovery (DETERMINISTIC):** immediate child directory names of
   `subject` that appear in the `files` table (path has a segment after
   `subject/` with a further `/`).
2. Per module, attach counts only from existing evidence:
   - `file_count`, `subdirectories` — `files`
   - `observed_commit_count` — `commits_under_prefix` (DISTINCT commits)
   - `outgoing_edge_count` / `incoming_edge_count` — `structural_edges`
   - `in_module_test_file_count` — path-test heuristic under module prefix
3. **`has_associated_tests` (DERIVED):** true if in-module tests exist OR
   any file path starts with `tests/<module>/`. Rule text stored on each
   entry as `test_association_rule`.

Alphabetical module order. `--json` emits the full report.

### Repository isolation

All storage access is `repo_path`-scoped (files, commits, edges).

### Non-goals / refusals

- No semantic domain labels ("identity is authentication").
- No ownership or expertise claims.
- No filter of "stub" or historical directories — inventory is evidence of
  what the `files` table knows, which may include historical paths.
- No schema change, no new extractor, no LLM.

## Alternatives considered

- **Live working-tree only.** Rejected for B5: would diverge from ingested
  evidence and break offline/DB-only investigation.
- **Reuse B1 report as inventory.** Rejected: B1 is peer-pattern
  prevalence, not per-module evidence counts.

## Validated outcome

RWATP (`src/modules`): 11 modules; core 102 files / 60 commits;
identity 46 / 28. Fixture: `modules_fixture` (6 tests).

## Future

UX for default subject when `src/modules` is empty (e.g. notifier) is a
separate product decision — not changed in freeze.