---
title: Repository Tree — read-only spatial view over the working tree
date: 2026-08-08
status: Implemented
---

## Problem

Downstream commands that want to attach Atlas evidence to a location in the
repository (e.g. the planned `atlas inspect src/modules/identity`) had no
first-class way to name a location, list its children, or say which
subtrees the working copy contains.  The `files` table records paths that
have been touched by commits, but that is history, not the current shape
of the tree — a file present on disk but never committed does not appear.

The narrower operational question: **when the user types
`atlas tree` in a repository, what is the reliable, deterministic view of
"where things are"?**

## Methodology validation

- **Principle 1 (features earned by evidence).**  The gap was earned by the
  Repository Intelligence Ingestion Step 2 checkpoint.  The prior state
  offered no way to see the disk shape of a repository at all; `atlas` had
  ingest, search, investigate, and inspect commands but no navigation
  command over the working tree.
- **Principle 2 (abstractions earned by repetition).**  No new abstraction.
  `RepoAwareness` is reused as-is (not extended, not made public).  The
  new IR types `RepositoryTree` and `TreeNode` are the minimum shape needed
  to expose "name / relative_path / kind / children" and coverage boundary;
  there is no attempt to unify with `FileIdentity` or any other Atlas
  entity.
- **Principle 3 (knowledge accumulated).**  This record + the benchmark
  stub in `docs/benchmarks/2026-08-08-repository-tree.md` are the retained
  knowledge.
- **Principle 4 (validation before generalization).**  The benchmark is
  Draft.  Marking it Complete requires running Atlas against at least two
  real repositories and confirming that the produced tree is a useful
  coordinate system for the Step 3 `atlas inspect` command.

## Decision

Added a read-only, transient `atlas tree [--depth N] [--json]` command:

- **Source of truth**: the working tree on disk at the discovered repo root.
- **Exclusion rules**: the existing `RepoAwareness` (build-artifact prefixes
  + simple `.gitignore` directory patterns) is reused unchanged.  `.git/`
  is additionally pruned at any depth by the walker itself so that git
  internals never appear in a spatial view — this filter is local to the
  tree walker because adding `.git/` to `RepoAwareness` would change
  behaviour of other ingest stages that share the type.
- **Depth semantics**:
    - `None` (no `--depth`) — walk to every leaf.
    - `Some(0)` — root only, no children.
    - `Some(N)` — root plus N descendant levels; directories reached at
      exactly depth N appear as leaves with `children: []`.
- **Ordering**: children are sorted alphabetically by basename
  (case-sensitive) at every level.  Deterministic.
- **Coverage boundary**: every excluded directory is recorded in
  `RepositoryTree.excluded` and rendered in a trailing `excluded:` line,
  so the consumer sees what the tree does NOT cover — the same
  epistemic invariant Atlas applies to search coverage and investigation
  coverage.
- **No persistence**: `RepositoryTree` is `Serialize`-only.  Nothing is
  written to the database.  Nothing is cached between calls.

New IR types are transient — `RepositoryTree`, `TreeNode`, `TreeNodeKind`
— and live in `crates/ir/src/lib.rs` alongside `ContextDocument` and
`InvestigationDocument` for uniformity.  `schema_version` field follows
the same discipline the other document types use.

## Alternatives considered

- **Add a `DirectoryNode` / `FileNode` first-class entity to the IR, with
  stable identifiers spanning renames.**  Rejected.  `FileIdentity`
  already provides that for files; directory identity has no earned use
  case (no investigation has failed because directories weren't stable).
  Building it now is the "abstractions before N=3" trap.
- **Persist the tree in SQLite so it can be queried like other evidence.**
  Rejected.  The tree is a projection of the current working state; it
  invalidates the moment a file is added or deleted.  Query-time
  computation is fast (a bounded directory walk) and avoids a stale-cache
  problem.
- **Source the tree from the `files` table instead of disk.**  Rejected —
  that would show ingest history, not the current repository shape.  The
  entire point of Step 2 is to see the current disk state.
- **Recurse into `packages/*/node_modules/` (deep exclusion) rather than
  only top-level `node_modules/`.**  Deferred.  Current `RepoAwareness` is
  prefix-based (top-level only).  Extending it would change behaviour of
  every ingest stage that uses the type; a real repository must
  demonstrate the shortcoming first.
- **Introduce a default depth cap for the CLI (e.g. 3).**  Deferred.
  Unlimited is the simplest starting behaviour.  If a real repository
  produces unreadable output, we set a default depth and add a
  `--depth all` opt-out.
- **Skip hidden files (`.foo`) by default.**  Rejected — `.gitignore`,
  `.github/`, `.claude/` are legitimate parts of a repository's shape.
  Only `.git/` is git internals worth suppressing.

## Validated outcome

Manual runs against Atlas itself:

- `atlas tree --depth 0` — root only, no children.
- `atlas tree --depth 1` — top-level entries alphabetical, `excluded: .git, target` footer present.
- `atlas tree --depth 2` — one level deeper, same footer.
- `atlas tree --depth 2 --json` — well-formed JSON, `schema_version: 1`,
  `depth_limit: 2`, `excluded: [".git", "target"]`.

Tests: 8 new unit tests in `crates/core/tests/tree_fixture.rs`, all
passing.  Full workspace: 204 tests, 0 failures.

One pre-existing quirk of `RepoAwareness` was observed but not addressed:
bare-name `.gitignore` entries (e.g. `atlas.db`) are treated as directory
prefixes (`atlas.db/`), so file patterns from `.gitignore` are silently
ignored.  This causes `atlas.db`, `vestascan.db`, and `vestascan2.db` to
appear in Atlas's own tree output.  Recording as a follow-up; Step 2's
scope was explicitly not to change `RepoAwareness`.

## Future

- Enables Step 3: `atlas inspect <path>` uses this coordinate system to
  attach existing Atlas evidence (commits, PRs, structural edges, docs,
  peer observations) to each node.  Step 2 attaches none of that.
- If real-repository runs produce unreadable output for large repos,
  introduce a default depth cap and `--depth all`.
- If `.gitignore` file-pattern exclusion becomes a real gap, extend
  `RepoAwareness` under its own decision record.
