---
title: CLI foundations — position independence, freshness, cold start
date: 2026-08-13
repository: atlas (self), synthetic TypeScript fixture
status: Complete
---

## Repository

Atlas itself (`/home/sanoy/projects/atlas`, HEAD `29c47af`, 148 files
classified), plus a synthetic two-file TypeScript repository created cold to
exercise the first-run path with no prior Atlas state.

## Question

Can a developer who does not already know how Atlas works get correct answers
from it — from wherever they happen to be standing in the repository, and
without mistaking stale evidence for current evidence?

This is a usability benchmark, not a retrieval-quality one. The structural
operations being reached (`callers`, `implementations`, `capabilities`,
`code-search`) were already validated in
`docs/benchmarks/` for the code-intel phase; what is under test is whether they
are reachable.

## Ground Truth

1. From any directory inside a repository, a read command must consult the
   evidence graph belonging to that repository.
2. A graph built before the current HEAD must be reported as stale, with the
   commit gap named.
3. A cold TypeScript repository must reach an OBSERVED `implements` edge with
   no flags and no manual configuration.

## Atlas Evaluation

### Commands used (in order)

```
cd crates/core/src && atlas hot-files --limit 3     # position independence
atlas status                                        # freshness, fresh graph
atlas init                                          # cold start, synthetic repo
atlas implementations IStorageProvider              # end-to-end structural reach
```

### Manual source reads required

Zero, for all three ground-truth questions.

### Wrong branches followed

None during evaluation. The pre-fix failure mode is itself the finding: a
subdirectory read produced empty results indistinguishable from a genuine
"no evidence" answer, which is a wrong branch Atlas *caused* rather than one
the evaluator followed.

### False positives

| Query | Unexpected match | Reason | Severity |
|---|---|---|---|
| `atlas hot-files` from `crates/core/src` (pre-fix) | empty result set | cwd-relative `./atlas.db` opened a fresh empty DB | High — silently wrong, and indistinguishable from a correct empty answer |
| `atlas implementations IStorageProvider` | `(line Some(2))` | `Option` rendered with `{:?}` in `code_intel.rs` | Low — cosmetic, but leaks internals into an answer |
| `atlas init` (first cut) | "no Atlas database … run `atlas ingest`" | write path shared the read path's missing-DB warning | Low — contradictory advice during the command that creates it |

### Useful observations

- A single resolution choke point paid for itself: 34 read commands were
  corrected by one function. The pre-existing discipline of routing every
  command through `resolve_db_path()` + `discover_repo_root()` is what made a
  one-line-of-reasoning fix possible.
- `discover_repo_root()` already walked up correctly. Only the *database* path
  was position-dependent, so repo identity and repo evidence disagreed — the
  hardest class of bug to see from the output, because each half looks right.
- Freshness needed no schema change. `ingest_runs.git_head` had been recorded
  since the ingest-runs work; nothing had ever read it back for this purpose.
- TypeScript-detection asymmetry was invisible from the code: the ingest stage
  list reads uniformly, and only `enabled:` differed between `has_python` and
  a raw `typescript` flag.

## Classification

| | |
|---|---|
| Overall | Improved |
| Commands needed | 4 |
| Source reads needed | 0 |
| Confidence | High — every claim has a blackbox reproducer |
| Noise removed | Eliminated an entire class of false-empty answers across 34 commands; removed stray-DB creation |
| Hidden understanding revealed | That Atlas's evidence could silently describe a tree that no longer exists, with no signal to the user or to any downstream consumer |

## Outcomes

- **Decision produced?** Yes — `docs/decisions/2026-08-13-cli-foundations.md`.
- **New primitive earned?** No. Freshness is close to one — it is a claim about
  evidence rather than about the repository — but it observes nothing new and
  adds no evidence class. It is provenance made visible. Recorded here so that
  if a second and third "how much should this evidence be trusted" signal
  appears (dirty tree, extractor version drift, partial ingest), the
  abstraction is earned by N=3 rather than invented now.
- **New abstraction earned?** No. `resolve_db_path_for_write` is a second
  concrete case, not a framework.
- **Regression?** None. 407 tests passing, 0 failing (402 before). The eval
  harness's `ATLAS_DB` override is explicitly pinned by a new test, since every
  cross-repo benchmark depends on it.
- **Unexpected discoveries?** Two. TypeScript being the only flag-gated
  extractor meant `atlas ingest .` on a TypeScript repo produced zero
  structural edges — the exact repositories the code-intel phase was built for.
  And the freshness work found that the Atlas root database predated
  `ingest_runs` entirely, so `atlas status` reported "no ingest yet" while
  `hot-files` returned real evidence from the same database: two commands
  disagreeing about whether the repository had been ingested.

## Not covered

- ~~No cross-repository validation.~~ **Done.** The full cross-repo suite was
  run against the Phase 1 binary: `eval/cross-repo-suite/results-phase1-20260813-232040`,
  **60 cases, 0 fail, 0 skip**, across 11 repositories (rwatp ×4, vestascan ×5,
  research ×2). This is the first true full-suite execution — the prior run
  (`results-post-fix-20260813-014029`) was a narrow 11-case `modules_auto`
  check, so there is no case-name overlap and therefore no per-case regression
  comparison available; the comparison is "11 narrow cases green" → "60 broad
  cases green."

  Ingests re-ran clean under the new auto-detecting TypeScript path
  (rwatp 119 s, vestascan 86 s, research 169 s), and the `ATLAS_DB` override
  drove all of it, which is the multi-repo path this benchmark wanted
  exercised. Eval DBs were backed up to `backup-pre-phase1/` first, since the
  suite overwrites them.

- **What the green suite does *not* prove.** It contains no `callers`,
  `implementations`, `capabilities`, or `code-search` cases. It is a
  no-regression result for Phase 1, not cross-repo validation of code-intel;
  that debt stays open and is described in
  `docs/benchmarks/2026-08-13-code-intel-rwatp.md`.
