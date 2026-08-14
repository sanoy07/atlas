---
title: atlas cohorts — co-change directory pairs and thresholded components
date: 2026-08-09
status: Implemented
---

## Problem

Teams asked which directories "move together." Co-change is available in
commit file lists, but was not aggregated into an explicit, thresholded
cohort report. Semantic "same domain" claims are out of scope.

## Methodology validation

- **Principle 1.** Coupling answers structural edges; cohorts answer
  historical co-touch — complementary, evidence already stored.
- **Principle 2.** Uses `all_commits_with_files` + B5-style immediate
  child directories under a subject — no new table.
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-cohorts.md`.

## Decision

Command: `atlas cohorts [path] [--threshold N]` (default subject
`src/modules`, default threshold **2**).

Core: `compute_directory_cohorts` → `CohortsReport`.

### Algorithm

1. Candidate directories = immediate children of `subject` (files table).
2. For each commit, set of candidate dirs touched (path prefix).
3. Every unordered pair in that set increments `cochange_commit_count`
   by 1 → **DETERMINISTIC** pair counts.
4. Undirected graph of pairs with count ≥ threshold; connected components
   of size ≥ 2 → **DERIVED** cohorts.
5. Directories with no threshold edge → **singletons** (listed, never
   silently discarded).

### Repository isolation

`all_commits_with_files(repo_path)`, `all_file_paths(repo_path)`.

### Non-goals / refusals

- Not "business domain discovery."
- No time decay, no line blame, no ownership.
- No schema / extractor / LLM.

## Alternatives considered

- **File-level co-change only.** Already exists as `atlas co-changes`;
  B8 is directory-level for architectural scale.
- **Implicit threshold.** Rejected — threshold is always explicit on the
  report.

## Validated outcome

RWATP: pairs e.g. blockchain×core 12, core×identity 12; threshold=2
cohorts and singletons listed. Fixture: `cohorts_fixture` (6 tests).

## Future

No timing benchmark performed.