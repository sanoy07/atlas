---
title: Repository Tree — usefulness of `atlas tree` as a spatial coordinate system
date: 2026-08-08
repository: (to fill: RWATP core, VestaScan, Atlas self)
issue: Repository Intelligence Ingestion — Step 2 validation
status: Draft
---

# Benchmark: Is `atlas tree` a reliable spatial coordinate system?

## Repository

Name: TBD — this stub covers **three** target validations:
  1. Atlas self (Rust workspace, `target/` present, no `node_modules/`)
  2. RWATP core (TypeScript, `node_modules/`, `dist/` — not present locally at time of Step 2)
  3. VestaScan (TypeScript, commits `dist/` — earlier N=3 for RepoAwareness)

Atlas ingested: not required — `atlas tree` is a working-tree view, independent of ingestion.

## Question

Does `atlas tree` produce output that a downstream command
(`atlas inspect <path>`) can rely on as a stable, deterministic address
book for repository locations?

Concretely:

1. Are `node_modules/`, `dist/`, `target/`, and `.git/` reliably absent
   from the tree, and reported in the `excluded` footer?
2. Is the alphabetical ordering the same on repeated runs?
3. Does the `relative_path` field always match the address a caller would
   need to pass to `atlas context` or `atlas structural`?
4. On a large repo, is `--depth 2` a readable summary?
5. Are there `.gitignore` patterns in real repositories that leak through
   the current prefix-only exclusion (e.g. glob patterns, file patterns)?

## Ground Truth

**Atlas self (initial run, 2026-08-08):**

Depth 1 output (verified):

```
atlas/
├── .claude/
├── .gitignore
├── CLAUDE.md
├── Cargo.lock
├── Cargo.toml
├── apps/
├── atlas-evaluation/
├── atlas.db
├── benchmark/
├── crates/
├── docs/
├── eval/
├── flake.lock
├── flake.nix
├── vestascan.db
└── vestascan2.db

excluded: .git, target
```

Observations:

- `.git` and `target` excluded correctly.
- `atlas.db`, `vestascan.db`, `vestascan2.db` **leak through** — they are
  in `.gitignore` as `atlas.db` (bare filename), but `RepoAwareness`
  transforms every bare-name entry into a directory prefix (`atlas.db/`),
  so file patterns from `.gitignore` are silently ignored.  This is a
  pre-existing `RepoAwareness` limitation, not a `atlas tree` bug.
- Hidden entries (`.claude/`, `.gitignore`) are correctly included —
  they are legitimate parts of the repo shape.

**RWATP core (to run):**
Not present at `/home/sanoy/projects/` at time of Step 2 implementation.
When available:
- Verify `node_modules/` and `dist/` are excluded.
- Note whether any nested `packages/*/node_modules/` leaks through
  (expected: yes, because current `RepoAwareness` is top-level only).
- Note total node count and rendered line count at `--depth 2` and
  unlimited.

**VestaScan (to run):**
- Verify `dist/` is excluded even though it is committed to the repo
  (this was the N=3 case for `RepoAwareness`).
- Note whether any other build artifacts leak.

---

## Atlas Evaluation

### Commands used (in order)

```bash
atlas tree                 # unlimited depth
atlas tree --depth 1
atlas tree --depth 2
atlas tree --depth 2 --json
```

### Manual source reads required

To fill after running on RWATP and VestaScan.

### Wrong branches followed

To fill after running.  Specifically: which paths appear in the tree that
should not (leakage), and which paths are absent that should be present
(over-exclusion).

### False positives

| Query | Unexpected leak | Reason | Severity |
|-------|------------------|--------|----------|
| `atlas tree` on Atlas | `atlas.db`, `vestascan.db`, `vestascan2.db` | `.gitignore` file patterns not honoured — `RepoAwareness` treats bare names as directory prefixes | Low (cosmetic; downstream commands can ignore .db files by extension) |

### Useful observations

- The `excluded:` footer provides an immediate view of what the tree does
  NOT cover.  This is the coverage-boundary discipline the rest of Atlas
  applies to search and investigation results.
- Depth 0 correctly produces a single-node tree (root only), useful for
  scripting.
- JSON output at depth 2 was well-formed and consumable — sanity check
  for future machine consumers.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall (Atlas self) | Improved (exclusions correct; one gap noted) |
| Overall (RWATP) | TBD |
| Overall (VestaScan) | TBD |
| Commands needed | 1 |
| Source reads needed | 0 |
| Confidence at completion (Atlas self) | High |
| Noise removed (vs. prior) | N/A — no prior tree command |
| Hidden understanding revealed | Which artifacts are still leaking despite `.gitignore` (see False positives) |

---

## Outcomes

**Decision produced?** Y — `docs/decisions/2026-08-08-repository-tree-view.md`.

**New primitive required?** No.  `atlas tree` is a projection over the
working tree that reuses existing `RepoAwareness` unchanged.

**New abstraction earned?** No.  The IR types `RepositoryTree` and
`TreeNode` are the minimum shape needed to expose the four required fields
(`name`, `relative_path`, `kind`, `children`) plus the coverage boundary.

**Regression?** No.  No ingest stage was changed; no existing test was
modified.

---

## Notes

The one clear follow-up observed during Step 2 is the `.gitignore`
file-pattern gap in `RepoAwareness`.  It belongs under its own decision
record because fixing it changes behaviour for every ingest stage that
uses `RepoAwareness`, not just `atlas tree`.

Marking this benchmark Complete requires runs against RWATP and VestaScan
plus a rendered line count at `--depth 2` and unlimited depth for each.
