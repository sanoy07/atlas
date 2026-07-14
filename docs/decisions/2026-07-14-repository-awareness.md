---
title: Repository Awareness — build artifact exclusion during ingest
date: 2026-07-14
status: Implemented
implements_gap: repository-awareness
tags: [ingest, investigation-hygiene, environment-primitive]
---

# Repository Awareness

## Problem

During VestaScan dogfood (investigations 01–03), build artifacts committed to version control appeared throughout investigation output:

- `dist/*.js` files occupied CORE IMPLEMENTATION NEIGHBORHOOD candidate slots (23% noise in investigation 01)
- `dist/` co-change clusters dominated HISTORICAL EVIDENCE with misleading co-change attribution
- `dist/modules/quota/` ghost files from a module rename (quota→plan) appeared as active historical evidence
- In the AI context builder investigation (02), 8 dist/ blockchain files masked the entire `src/infrastructure/ai/` directory, which was completely absent from the candidate set

This is categorically different from a relevance problem. The files are correct—they contain accurate compiled output—but they represent generated artifacts that a developer would never intentionally investigate.

## Methodology validation

This primitive satisfied all four criteria before implementation:

| Criterion | Evidence |
|-----------|----------|
| Production evidence | 3 independent investigations with dist/ noise |
| Repetition (N≥3) | vestascan-api investigation 01, 02, 03 all hit it |
| Knowledge retained | Benchmark files 2026-07-14-vestascan-* documented the gap |
| Validated (N≥2 repos) | Confirmed in Atlas self-ingest (target/) and VestaScan (dist/) |

## Decision

Add `RepoAwareness` as a private struct in `crates/core/src/lib.rs`, applied during `ingest_git()` and `ingest_typescript()`.

This is the first **environment primitive** in Atlas — distinct from all previous evidence types (source code, history, documentation, engineering decisions) because it encodes knowledge about *repository layout* rather than repository *content*.

## Implementation

**Two exclusion sources, in priority order:**

1. **Hardcoded common build artifact patterns:** `dist/`, `node_modules/`, `target/`, `build/`, `.next/`, `coverage/`, `__pycache__/`, `.cache/`, `out/`, `.nuxt/`

   Hardcoded patterns are necessary because VestaScan commits `dist/` to version control — it is NOT in `.gitignore`. Parsing `.gitignore` alone would not solve the confirmed N=3 case.

2. **`.gitignore` parsing (additive):** Reads simple name patterns from the repository's `.gitignore`, excluding globs (`*`, `?`, `[`) and negations (`!`). Normalizes to slash-terminated prefixes for path matching.

**Applied in:**
- `ingest_git()`: filters `commit.files_changed` before writing to `commit_files` table
- `ingest_typescript()`: filters `StructuralEdge` pairs before writing; return value is kept-edge count

**Not applied in:**
- `ingest_rename_evidence()`: rename records pass through; orphaned identity chains (for excluded files) are harmless
- `search_anchor()`: the search LIKE query runs on the stored data; excluded files are absent from storage and will not appear

## Validated outcome

Before repository awareness — `atlas investigate AI context builder`:
- 23 candidates, 8 dist/ blockchain files present, `src/infrastructure/ai/` entirely absent
- Classification: Improved (70% noise)

After repository awareness — same query:
- 25 candidates, 0 dist/ files, `src/infrastructure/ai/` fully surfaced (factory, adapter, interface, types, index)
- New structural chain revealed: `chat.service.ts → AIProviderFactory.getProvider`
- Classification: Optimal (zero source reads required)

## Alternatives considered

**`.gitignore`-only exclusion:** Rejected. VestaScan's dist/ is committed to the repo, so `.gitignore` is not authoritative for committed build output.

**Query-time filtering:** Rejected. Filtering at query time would still store the artifact data and slow down all queries. Ingest-time exclusion keeps the evidence base clean from the start.

**Configurable patterns via Atlas config:** Deferred. The hardcoded defaults handle N=3 confirmed cases. A user-configurable override (`.atlasignore` or `atlas.toml [ingest] exclude`) earns its implementation when a case requires a pattern outside the defaults.

## Future

The framework is in place for three future exclusion sources:
- `.git/info/exclude` (repo-level gitignore equivalent)
- Language-convention inference (presence of `package.json` → add `node_modules/`)
- User-defined `.atlasignore` or `[ingest] exclude` in Atlas config

These are deferred until cases arise that the current defaults cannot handle.
