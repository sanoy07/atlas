---
title: atlas tests — path-rule test ↔ module linkage
date: 2026-08-09
status: Implemented
---

## Problem

Investigations asked which tests "belong" to a module. Without explicit
rules, any answer risks inventing ownership. Atlas needed a linkage view
that only asserts path relationships it can prove.

## Methodology validation

- **Principle 1.** RWATP has both `tests/<module>/` trees and in-module
  `__tests__` files; unlinked trees (e.g. `tests/rbac/`) must stay
  unlinked until a rule earns them.
- **Principle 2.** Two concrete path rules only — no fuzzy matching,
  no content analysis.
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-tests-linkage.md`.

## Decision

Command: `atlas tests [--modules SUBJECT] [path-filter]`.

Core: `compute_test_module_links` → `TestModuleReport` / `TestModuleLink`.

### Evidence source

- `files` table for path enumeration
- Module set from same discovery rule as B5 under `modules_subject`
- Test classification: same path heuristic as `ArtifactRole::Test`
  (directory segments `test`/`tests`/`spec`/`__tests__`, or
  `*.test.ts` / `*.spec.ts` suffixes)

### Linkage rules

| Kind | Class | Rule |
|------|-------|------|
| `DirectPathPrefix` | **DETERMINISTIC** | test path starts with `{modules_subject}/{module}/` |
| `ConventionalTestsDir` | **DERIVED** | test path starts with `tests/{module}/` AND that module exists under `modules_subject` |

Unmatched tests are listed in `unlinked_tests` — never forced into a module.

### Repository isolation

`all_file_paths(repo_path)` only.

### Non-goals / refusals

- No ownership ("this test owns that module").
- No inventing links for `tests/rbac` without a module named `rbac`.
- No confidence scores without a mathematical definition from evidence.
- No schema / extractor / LLM.

## Alternatives considered

- **Structural import edges test→source.** Deferred: not earned as
  N≥2 cross-repo requirement; path rules already answer RWATP's layout.
- **Filename substring matching.** Rejected as semantic guesswork.

## Validated outcome

RWATP: 67 test files, 51 links, 16 unlinked. VestaScan/notifier: 0 links
when heuristics match nothing — correct empty result.

## Future

Additional rules (e.g. rbac→identity) only if earned by repeated
investigation failure with N≥2 repositories.