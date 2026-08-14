---
title: atlas inspect — attach existing evidence to a spatial subject
date: 2026-08-08
status: Implemented
---

## Problem

Steps 1 (extended document ingestion) and 2 (`atlas tree`) established
the substrate: Atlas ingested more of what a repository contains, and it
could show the working tree as a stable coordinate system.  What was
still missing: a way to point at a subtree and see **what Atlas already
knew about it**.

Every fact Atlas has about a location was reachable through a different
command — `atlas context` for one file, `atlas structural` for one
file's edges, `atlas hot-files` for the whole repo, `atlas search` for
anchors, `atlas project census` for repository-level claims.  None of
them answered the plain question "what's here?" for a directory.

## Methodology validation

- **Principle 1 (features earned by evidence).**  The gap was earned by
  the Step 2 review: the tree existed but nothing attached to it.  The
  user's Step 3 charge was explicit — "attach the existing Git,
  structural, documentary, and profile evidence to the spatial
  coordinates we just created."
- **Principle 2 (abstractions earned by repetition).**  Reused every
  existing surface: `build_context` (file case), `commits_under_prefix`
  / `documents_under_prefix` / `structural_edges_{from,to}_prefix` /
  `prs_under_prefix` / `issues_under_prefix` (dir case, additive),
  `inspect_repository` (profile claims), `classify_artifact_role`
  (roles).  New IR types are transient projections
  (`InspectionDocument`, `InspectionEdge`, `InspectionChild`,
  `InspectionDocumentRef`, `InspectionCoverage`,
  `InspectionSubjectKind`) — no new persisted entity.
- **Principle 3 (knowledge accumulated).**  This record + the benchmark
  stub in `docs/benchmarks/2026-08-08-atlas-inspect.md` are the retained
  knowledge.  10 unit tests in `crates/core/tests/inspect_fixture.rs`
  pin behaviour.
- **Principle 4 (validation before generalization).**  The benchmark is
  Draft.  Marking it Complete requires running `atlas inspect` against
  real business modules in RWATP and VestaScan and confirming the
  output approaches an architectural map.

## Decision

New command `atlas inspect <path>` producing an `InspectionDocument`.
Path is repo-relative; kind auto-detected from disk (Directory / File);
a nonexistent path defaults to Directory with `exists_on_disk = false`.

### Structural-edge partitioning (the crucial clarification)

For directory subjects, structural edges are partitioned into three
buckets on the boundary of the subject subtree:

- **`structural_internal`**: both endpoints inside the subtree —
  cohesion signal, rendered as a count only.
- **`structural_depends_on`**: source inside, target outside — "what
  this subtree relies on".
- **`structural_used_by`**: source outside, target inside — "what
  relies on this subtree".

For file subjects, `structural_internal` is always empty; `depends_on`
= outgoing edges; `used_by` = incoming edges.  The distinction
collapses cleanly on a single-file subject.

### Rendering discipline

- `depends_on` and `used_by` are the primary architectural signal, so
  they are listed in detail (up to 20 each, with an `and N more`
  footer).
- `structural_internal` is only a count line, to avoid drowning the
  reader in the intra-module edges of a large subtree.
- Every unpopulated section is suppressed in the text renderer;
  every field is always present in JSON.

### Storage additions (additive, read-only)

Six new `Store` accessors, all `SELECT`s, all `LIKE 'prefix%'`:

- `commits_under_prefix`
- `documents_under_prefix`
- `structural_edges_from_prefix`
- `structural_edges_to_prefix`
- `prs_under_prefix`
- `issues_under_prefix`

No schema change.  No new indexes (the workload is small; add later if
proven necessary).

### Documents section

**Literal containment only.**  Docs appear iff their `file_path` is
inside the subject.  Docs *mentioning* the subject go through
`atlas search` / `atlas investigate` — that keeps `inspect` and
`investigate` semantically distinct:

- `inspect` = "what does Atlas know that is **inside** this path?"
- `investigate` = "what does Atlas know that is **related** to this
  concept?"

### Profile claims

`inspect_repository` runs at query time (unchanged from Step 1
decision: no persistence for single-repo ingest).  Ambient claims
(`Runtime`, `Language`, `PackageManager`) are always included.  A
`Module` claim is included iff the subject is `src/<name>/…` and
`<name>` appears in the observed set of top-level `src/` subdirectories.

## Alternatives considered

- **Two separate document types for File and Directory subjects.**
  Rejected as needless duplication.  A unified `InspectionDocument`
  with a `kind` enum and Options/Vecs for kind-specific fields is
  cleaner and easier to consume.
- **Recursive tree in `InspectionDocument.children`** (mirror of
  `atlas tree`).  Rejected as scope creep.  Children are shallow;
  callers wanting the deep tree run `atlas tree` and pipe.
- **Search-based "docs mentioning subject".**  Rejected for v1 — it's
  what `atlas investigate` is for and blurs the `inspect` boundary.
- **Peer observations attached per file.**  Rejected for v1.  Already
  accessible via `atlas structural --reverse`; adding to `inspect`
  would triple the scope.  Add when a benchmark demands it.
- **A `RepoAwareness`-anchored default depth cap for `hot_files_within`**
  above 10.  Rejected — 10 matches the existing pattern in `atlas
  hot-files`.  Users wanting more can query directly.
- **Persist `InspectionDocument`s in a new table for diffing across
  time.**  Rejected as premature.  Nothing in the current workflow
  needs it; add when investigation demands.

## Validated outcome

Fresh ingest of Atlas → `atlas inspect crates/core`:

```
ATLAS INSPECT
crates/core  [directory]

CONTAINS (3 immediate children)
  Cargo.toml
  src/
  tests/

RECENT ACTIVITY (10 commits, showing 5 most recent)
  29c47af  2026-07-18  july 18 2-26
  fbd0453  2026-07-14  july 15, 2026
  d01d4d3  2026-07-14  feat: structural extraction, investigation, peer observations, and decision records
  ...

HOT FILES WITHIN
    10×  crates/core/src/lib.rs
     4×  crates/core/Cargo.toml
     1×  crates/core/tests/rename_fixture.rs

STRUCTURAL EDGES
  Depends on:  0 boundary edges  →  0 distinct external targets
  Used by:     0 boundary edges  ←  0 distinct external sources
  Internal:    1 edge within the subtree (cohesion signal only, not listed)

PROFILE
  PackageManager: cargo
  Language: JavaScript
  Language: Rust
  Runtime: Rust

COVERAGE
  ✓ Git history        ✓ Structural edges   ✓ Documentation
  ✗ GitHub PRs         ✗ GitHub issues      ✓ Profile claims
  ✓ Working tree
```

`atlas inspect crates/core/src/lib.rs` (file subject) — populates
`IDENTITY`, `RECENT ACTIVITY`, and `HISTORICAL COUPLING` from
`build_context`; suppresses `CONTAINS`, `HOT FILES WITHIN`, and
`INTERNAL EDGES`.

`atlas inspect docs` — literal containment surfaces every ingested doc
(15 decisions + ADRs + docs) with `doc_type` prefix labels.

`atlas inspect nonexistent/path` — non-error; empty result with
`(not present on disk)` label and `Working tree ✗` in coverage.

The zero structural-edge count on `crates/core` reflects a real
limitation of the current Rust structural extractor (only 1 edge
inserted for the whole workspace), not of `atlas inspect`.  A benchmark
run on a TypeScript repository (RWATP) will exercise the boundary
partitioning at real scale.

Tests: 10 new unit tests in `crates/core/tests/inspect_fixture.rs`,
covering file/dir kind detection, subtree isolation, structural-edge
boundary partitioning (both directions, both kinds), docs containment,
Module claim relevance, nonexistent-path graceful handling, and
repo_path isolation across two repos in one DB.

Full workspace: **223 tests, 0 failures** (was 213; +10).

## Blast radius

Purely additive.  New IR types are `Serialize`-only.  New Store methods
are read-only.  No existing code path was modified.  No ingest stage
was touched.  Rerunning `atlas ingest` produces identical output.

## Future

- Run `atlas inspect` on RWATP business modules (`src/modules/identity`,
  `orders`, `payment`, `signing`, `compliance`) and record whether the
  boundary partition reveals real architectural signal.  This is the
  next step per the Step 3 approval message.
- If real repos expose `hot_files_within` bottlenecks, add a
  `hot_files_under_prefix` SQL accessor rather than filtering all hot
  files in Rust.
- If real repos surface subjects whose most relevant docs live outside
  the subject subtree (README next to a service, decision record in
  `docs/`), evaluate adding a "docs referencing subject" section — but
  only after demonstrating the current literal-containment surface is
  genuinely insufficient.
- Persistent `InspectionDocument` snapshots would enable diffing "what
  Atlas knew about identity a month ago vs today."  Defer until a real
  investigation demands it.
