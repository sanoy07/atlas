---
title: RepoAwareness — bare `.gitignore` names must exclude root files
date: 2026-08-08
status: Implemented
---

## Problem

`RepoAwareness` in `crates/core/src/lib.rs` transformed every simple
`.gitignore` entry into a **directory prefix** by appending `/`.  A line
like `atlas.db` in `.gitignore` was stored as `atlas.db/`, and the
`is_excluded` check used `path.starts_with("atlas.db/")` — which is
never true for the string `"atlas.db"` (missing the trailing slash).

Consequence, observed on the Atlas repository during Step 2 (`atlas tree`):
`atlas.db` appeared in the working-tree view even though `.gitignore`
listed it.  The same silent misclassification affected five ingestion
paths (`ingest_git`, `ingest_typescript`, `ingest_c`, `ingest_python`,
`ingest_rust`) because they share the same `RepoAwareness` instance —
any file matching a bare `.gitignore` entry could leak into
`commit_files` or into structural-edge rows.

## Methodology validation

- **Principle 1 (features earned by evidence).**  The gap is concrete
  and reproducible on Atlas itself with two commands (`atlas tree` +
  `cat .gitignore`).  No hypothetical scenario.
- **Principle 2 (abstractions earned by repetition).**  No new
  abstraction.  `RepoAwareness` remains a private struct with the same
  two-method API (`load`, `is_excluded`).  Internally it now carries
  two small `Vec<String>` fields instead of one.
- **Principle 3 (knowledge accumulated).**  This record + the 9 unit
  tests in `crates/core/src/lib.rs` under `mod repo_awareness_tests`.
- **Principle 4 (validation before generalization).**  The fix is
  deliberately minimal; it does not attempt to implement broader
  `.gitignore` semantics.

## Decision

The `RepoAwareness` internal representation is split into two collections:

- `dir_prefixes: Vec<String>` — every entry ends with `/`, matched with
  `path.starts_with(prefix)`.  Contains the 10 hardcoded defaults
  (`dist/`, `node_modules/`, etc.) and every `.gitignore` entry that
  already ended with `/`.  **Behaviour identical to prior code.**
- `names: Vec<String>` — bare `.gitignore` entries that did not end
  with `/`.  Matched with:
  ```
  p == name  ||  ( p.starts_with(name) && p[name.len()] == '/' )
  ```
  Both branches are **root-anchored** — a bare name still does not match
  nested paths like `packages/foo/atlas.db`.  This preserves the
  existing `starts_with`-from-start-of-path semantics that every other
  prefix in `RepoAwareness` uses.

Additional line-parsing change: entries starting with `/` (anchored
gitignore patterns like `/only-root`) are now **skipped** rather than
silently stored as `/only-root/`.  Supporting anchored patterns requires
its own decision (they impose "match here and nowhere else"; the current
match model has no equivalent) and is out of scope.

Explicitly **not** changed:

- Nested `.gitignore` files (still ignored — only the root file is read).
- Glob patterns `*`, `?`, `[` — still silently unsupported.
- Negation lines beginning with `!` — still silently unsupported.
- Hardcoded default prefix set — unchanged.
- No new dependency on any real gitignore matcher.
- No change to any caller of `RepoAwareness`.  All five ingestion sites
  and the `walk_tree` in `build_repository_tree` see the correction
  transparently.

## Alternatives considered

- **Store bare names as a single prefix without trailing slash** (e.g.
  `atlas.db`) and rely on `starts_with` alone.  Rejected: `"foobar".starts_with("foo")`
  is true, which would spuriously exclude `foobar` when `.gitignore`
  contains `foo`.  Explicit word-boundary check at the path separator
  is required.
- **Match bare names at any depth (unrooted, matching real gitignore
  semantics).**  Rejected — it would silently expand `RepoAwareness`
  behaviour for every ingestion caller, changing which files land in
  `commit_files` and `structural_edges` on real repositories.  The user
  ask was explicit: make the smallest change consistent with existing
  semantics.  Any move to unrooted matching deserves its own decision.
- **Replace `RepoAwareness` with a full gitignore matcher (e.g. the
  `ignore` crate).**  Rejected as scope.  A full matcher would bring
  every gitignore feature at once, including nested files and unrooted
  matching — a separate project with a much larger blast radius.
- **Anchored `/foo` patterns silently reinterpreted as root-anchored
  bare names.**  Rejected in favour of explicit skip, so future readers
  are not surprised by a subtle semantic conflation.

## Validated outcome

Before:

```
$ atlas tree --depth 1
atlas/
├── .claude/
├── .gitignore
├── CLAUDE.md
├── ...
├── atlas.db          <- listed in .gitignore, still exposed
├── ...
└── vestascan2.db

excluded: .git, target
```

After:

```
$ atlas tree --depth 1
atlas/
├── .claude/
├── .gitignore
├── CLAUDE.md
├── ...
├── vestascan.db      <- not in .gitignore, correctly still shown
└── vestascan2.db     <- not in .gitignore, correctly still shown

excluded: .git, atlas.db, target
```

`atlas.db` is now excluded and reported in the coverage footer.
`vestascan.db` and `vestascan2.db` remain because they are neither in
`.gitignore` nor in the hardcoded defaults — an accurate reflection of
the current repository state, not a limitation of the fix.

Tests:

- 9 new unit tests in `crates/core/src/lib.rs::repo_awareness_tests`,
  covering: hardcoded defaults, bare-name file exclusion, bare-name
  directory-content exclusion, no partial-prefix match, root-anchoring
  discipline, trailing-slash pattern unchanged, glob/negation still
  ignored, anchored `/foo` skipped, empty/comment lines skipped.
- Full workspace: **213 tests, 0 failures** (was 204; +9).

## Blast radius

For any repo with a `.gitignore` containing bare-name entries whose
matching file(s) were previously walked/ingested:

- `ingest_git`: those files disappear from `commit_files` rows on the
  next ingest.
- `ingest_typescript` / `ingest_c` / `ingest_python` / `ingest_rust`:
  structural edges whose source or target matches a bare name are
  dropped.
- `build_repository_tree`: those files no longer appear in the tree.

For the Atlas repo, only `atlas.db` is affected — and `atlas.db` had no
commits, no structural edges, and no docs.  Only the tree output changes.

For other repos (RWATP, VestaScan) the effect depends on their
`.gitignore` contents.  A re-ingest is safe: existing `commit_files`
rows are re-created without the newly-excluded entries because
`insert_commit` is idempotent on `hash`, but the *set* of
file-membership rows may shrink.

## Future

- If a bare `.gitignore` name should also match at any depth
  (unrooted, real gitignore semantics), that is a separate decision.
  It changes the semantics of every existing hardcoded prefix too,
  because uniformity across `dir_prefixes` and `names` is the point of
  the current design.
- If nested `.gitignore` files matter for any real investigation, that
  is a separate decision.
- If glob/negation support is ever needed, adopting a real gitignore
  crate deserves its own investigation.
