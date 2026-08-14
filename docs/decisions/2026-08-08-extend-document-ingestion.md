---
title: Extend document ingestion to README and recursive docs/
date: 2026-08-08
status: Implemented
---

## Problem

`atlas ingest_documents` scanned only `docs/decisions/` and `docs/adr/`.  Any
project-level README, architecture note, guide, or ad-hoc explanation that lived
elsewhere under `docs/` — or at the repository root — was invisible to
`atlas search` and to every downstream investigation that consumes documentary
evidence.

Two concrete consequences of this gap:

- The Atlas repository's own `README.md`, if we had one, would not be
  ingested by `atlas ingest .` — so anchor searches against Atlas from within
  Atlas would miss the most obvious documentary source.
- Repositories that follow a `docs/architecture/`, `docs/guides/`, or
  `docs/runbooks/` layout produced no `Documentary` evidence for
  `InvestigationDocument` beyond decision records and ADRs.

## Methodology validation

- **Principle 1 (features earned by evidence).**  The gap was surfaced during
  the Repository Intelligence Ingestion planning session: the working
  hypothesis is that the shape of a repository — including its written
  documentation — is a first-class input to investigations.  `README.md` and
  `docs/**/*.md` are the smallest concrete piece of that hypothesis testable
  without new schema or new IR concepts.
- **Principle 2 (abstractions earned by repetition).**  No abstraction is
  introduced.  The change reuses the existing `documents` table, the existing
  `Store::insert_document` API, and the existing frontmatter/filename title
  fallback.  A single private helper `ingest_one_doc` is extracted because
  the same four-line sequence would otherwise be repeated three times in one
  function — N=3, the earning threshold.
- **Principle 3 (knowledge accumulated).**  This record + the benchmark stub
  in `docs/benchmarks/2026-08-08-document-ingestion-coverage.md` are the
  retained knowledge.
- **Principle 4 (validation before generalization).**  Deferred deliberately.
  See `docs/benchmarks/2026-08-08-document-ingestion-coverage.md` — the
  benchmark is a Draft to be filled in once we run this pass against RWATP
  and VestaScan.  If either repository produces false-positive or
  false-negative documentary evidence, this decision will be revised.

## Decision

`ingest_documents` now scans four sources in explicit precedence order:

| # | Source                              | `doc_type` |
|---|-------------------------------------|------------|
| 1 | `docs/decisions/*.md` (top-level)   | `decision` |
| 2 | `docs/adr/*.md` (top-level)         | `adr`      |
| 3 | root `README.md`                    | `readme`   |
| 4 | any other `*.md` under `docs/`, recursively | `doc`  |

The recursive pass in (4) explicitly skips anything under `docs/decisions/`
or `docs/adr/`, so a file's `doc_type` is deterministic regardless of the
order in which the OS returns directory entries.  There is no reliance on
`INSERT OR REPLACE` to resolve conflicts.

No schema change was required.  The `documents` table's
`UNIQUE(file_path, repo_path)` constraint already prevents duplicate rows,
and the existing `INSERT OR REPLACE` semantics of `insert_document` remain
untouched.

A single small storage accessor was added: `Store::list_documents(repo_path)`.
It returns `(file_path, doc_type, title)` triples for tests to assert against,
and will be reused by `atlas inspect` in Step 3.

## Alternatives considered

- **Ingest every `**/*.md` in the repository.**  Rejected: `RepoAwareness`
  would need to be applied inside the walk to avoid pulling in
  `node_modules/**/README.md`, and even then the risk of ingesting
  unrelated third-party documentation is high.  `docs/` plus root `README.md`
  is a conservative, well-earned starting scope.
- **Also ingest nested `README.md` files** (e.g. `crates/core/README.md`).
  Rejected for this step: no benchmark demonstrates an investigation missed
  because of a nested README.  Trivial to add in a follow-up once earned.
- **Introduce H1 fallback for titles** so a README with no frontmatter shows
  a useful title.  Rejected for this step: broadening the title-extraction
  fallback would also change titles for existing decisions and ADRs, which
  is a behaviour change that deserves its own decision and its own tests.
- **Persist `ProfileClaim`s during single-repo `atlas ingest`.**  Rejected
  for now — see the Repository Intelligence plan.  The census pathway
  (`atlas project census`) still persists them; the single-repo path
  recomputes at query time when needed by future `atlas inspect`.
- **Introduce a `Documentation` first-class IR concept, or a new document
  ontology.**  Rejected on `feedback_observation_over_concept.md` grounds:
  documents are already an observation surface, not a concept.  Adding a
  new ontology is disallowed at N=1.

## Validated outcome

Before:

```text
Ingesting decision records and ADRs … 2 documents
```

After (same repository with a `README.md` and a `docs/guides/setup.md`):

```text
Ingesting decision records and ADRs … 4 documents
```

Five new tests in `crates/core/tests/documents_fixture.rs` cover:

- `ingests_root_readme`
- `walks_docs_directory_recursively`
- `preserves_decision_and_adr_types` (regression guard for existing behaviour)
- `empty_repo_returns_zero`
- `readme_without_docs_dir_returns_one`

Full workspace: 196 tests, 0 failures.

## Future

Enables — but does not build — Step 2 (`atlas tree`) and Step 3
(`atlas inspect`) of the Repository Intelligence Ingestion plan.  Both are
gated on a review of Step 1's behaviour against a real repository before
proceeding.
