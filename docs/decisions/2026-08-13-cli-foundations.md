---
title: CLI foundations — repo-anchored DB, evidence freshness, one-command init
date: 2026-08-13
status: Implemented
---

## Problem

Atlas's deterministic operations are fast and correct, but reaching them
required knowing things no user should have to know.

Three concrete failures, all reproducible before this change:

1. **Position-dependent database.** `resolve_db_path()` returned cwd-relative
   `./atlas.db`. Running any read command from a subdirectory opened a
   *different*, empty database. Atlas then reported "no history found" — the
   same output it produces for a genuinely unknown file. The user cannot
   distinguish "Atlas has no evidence" from "you are standing in the wrong
   directory," and nothing in the output hints at the difference. SQLite
   silently created the empty file, so the failure left a stray `atlas.db`
   behind in whatever directory the user happened to be in.

2. **Silently stale evidence.** The structural graph is a snapshot taken at
   the last ingest. Nothing invalidated it when the repository moved on, and
   nothing reported the drift. A graph built ten commits ago answered
   structural questions with the same confident formatting as a fresh one.
   This is the one place Atlas violated its own epistemic invariant: it stated
   relationships with more certainty than the evidence supported, because the
   evidence was no longer about the current tree.

3. **TypeScript required a flag.** C, Rust, and Python extractors auto-detected
   via `repo_has_*_files`. TypeScript — the most developed extractor, and the
   one carrying the `implements` work from earlier today — was gated behind
   `--typescript`. Plain `atlas ingest .` on a TypeScript repository produced
   zero structural edges, which is precisely the case Atlas serves best.

## Methodology validation

Stated plainly: this is not a Principle 1 / Principle 4 feature, and it was not
justified by cross-repository N-counting. It is bug-fix and affordance work on
capabilities that already exist and are already validated.

- **Principle 1 (features earned by production evidence):** satisfied in its
  bug-fix form. Each item above is a reproducible defect with a failing test,
  not a speculative improvement. No new evidence class was added.
- **Principle 2 (abstractions earned by repetition):** no new abstraction. DB
  resolution was already duplicated across four sites (`mod.rs`, `ingest.rs`,
  `plan.rs`, `project.rs`); this consolidates the existing repetition rather
  than anticipating future cases.
- **Principle 3 (knowledge accumulated):** this record plus
  `docs/benchmarks/2026-08-13-cli-foundations.md`.
- **Principle 4 (validation precedes generalization):** not applicable — no
  observation was promoted to a general primitive.

Explicit ordering claim: these were fixed *before* the MCP server rather than
after, because an MCP server inherits repository resolution and freshness
wholesale. Exposing a position-dependent, staleness-blind Atlas to another
agent does not contain the problem — it spreads it to a consumer that cannot
see the working directory it is being answered from.

## Decision

**Repo-anchored DB resolution.** `resolve_db_path()` now resolves in order:
`ATLAS_DB` (explicit override, verbatim) → `<git root>/atlas.db` → `./atlas.db`
(not in a repo). `find_repo_root` walks up for `.git`, testing existence rather
than directory-ness so worktrees and submodules work. Because the resolved path
at the repo root is identical to the old one, existing databases keep working
untouched. `resolve_db_path_for_write` is the same resolution without the
"no database yet" warning, for `ingest` and `init` whose job is to create it.

**Evidence freshness.** New `crates/core/src/freshness.rs` compares the git HEAD
recorded in `ingest_runs` against the current HEAD, reporting
`Current | Stale { commits_behind } | NeverIngested | Unknown { reason }`.
`atlas status` renders it as a `freshness` line. `commits_behind` is `Option`
because an unreachable ingested commit (rebase, force-push, different clone) is
an unknown gap, not zero. Required no schema change — `ingest_runs.git_head`
was already recorded.

**One-command init.** `atlas init` discovers the root, reports where the DB
will live, appends `atlas.db` to `.gitignore` (idempotent), runs the first
ingest with auto-detected extractors, and prints task-oriented next steps.

**TypeScript auto-detection.** `repo_has_ts_files` reuses the extractor's own
`collect_ts_files`, so detection and extraction cannot disagree about what
counts as a TypeScript file. `--typescript` is now force-on, not enable.

**Task-oriented help.** `after_help` groups the 39 commands under START HERE /
UNDERSTAND / LOCATE / INVESTIGATE / CONVENTIONS. clap 4 cannot group
subcommands natively, and the alphabetical list does not tell a user which of
`callers`, `code-search`, and `investigate` answers their question.

## Alternatives considered

**A config file (`.atlas/config.toml`) instead of walking to the git root.**
Rejected: adds a setup step to fix a problem that has a zero-configuration
answer. Git already defines where the repository is; Atlas should read that,
not ask the user to restate it. CLAUDE.md: do not add configuration knobs
before the default fails.

**Warning on a dirty working tree as part of freshness.** Rejected for now.
Structural extraction does read the working tree, so uncommitted edits genuinely
age the graph — but a dirty tree is the normal state during development, and a
warning that fires constantly trains users to ignore the warning, including
when it means something. HEAD drift is the crisp deterministic signal.
Documented as a known limitation in `freshness.rs` rather than silently omitted.

**Auto-reingest when stale.** Rejected: ingest is not free, and silently
rebuilding evidence underneath a query makes results irreproducible. Atlas
reports the staleness and lets the user decide.

**Blocking commands on a stale graph.** Rejected: stale evidence is still
evidence, and often still correct. Report, do not refuse.

## Validated outcome

Before — from a subdirectory of a repo with a fully ingested graph:

```
$ cd crates/core/src && atlas hot-files
(creates crates/core/src/atlas.db; reports no history)
```

After:

```
$ cd crates/core/src && atlas hot-files --limit 3
Most frequently modified files:
     7×  crates/core/src/lib.rs
     6×  crates/ir/src/lib.rs
     5×  apps/cli/tests/blackbox.rs
```

No stray database created. One function change fixed all 34 read commands,
which is the whole argument for having had a single resolution choke point.

Freshness, on the Atlas repository itself:

```
$ atlas status
LAST INGEST
  HEAD      29c47af  (master)
  status    ✓ ok
  freshness ✓ current with HEAD
```

And after one commit of drift (blackbox fixture):

```
  freshness ! 1 commit(s) behind HEAD — re-run `atlas ingest . --typescript`
```

Cold `atlas init` on a fresh TypeScript repository, with no flags, reaching an
OBSERVED `implements` edge end-to-end:

```
$ atlas init
  ✓ added `atlas.db` to .gitignore
[ 4/9 ] typescript structural    ✓ 2 edges

$ atlas implementations IStorageProvider
IMPLEMENTATION CANDIDATES  (OBSERVED implements preferred)
  [prod] src/gcs.adapter.ts  — OBSERVED implements IStorageProvider (line 2)
```

Also fixed en route: `Option` leaking through `{:?}` into user-facing output
(`(line Some(2))` → `(line 2)`) in `code_intel.rs`.

Tests: 407 passing, 0 failing (402 before; 5 added). The new tests are
reproducers — `db_resolves_from_repo_root_not_cwd` asserts both that
subdirectory reads see root evidence and that no stray database appears;
`atlas_db_env_overrides_repo_root` pins the override the eval harness depends
on; `status_reports_freshness_against_head` commits into the fixture and
asserts the exact drift count.

## Future

Enables, and deliberately defers:

- **`atlas mcp`** (Phase 2). The reasoning surface is intentionally smaller
  than the CLI: UNDERSTAND (`map`, `modules`, `capabilities`), LOCATE
  (`code_search`, `callers`, `implementations`), INVESTIGATE (`investigate`,
  `impact`, `history`). The `after_help` grouping introduced here is the first
  draft of that surface. Exposing all 39 commands is explicitly rejected.
- **Freshness in the MCP contract.** A machine consumer should receive
  freshness as a field, not as prose. `FreshnessReport` returns
  `warning() -> Option<String>` rather than printing, so the CLI and any future
  consumer render it in their own idiom. No consumer exists yet, so no
  serialization format is fixed.
- **Dirty-working-tree freshness** — needs evidence that HEAD drift alone
  misses real cases before it earns the noise.
- **One-command install** — still `cargo install` + PATH. Not yet earned.
