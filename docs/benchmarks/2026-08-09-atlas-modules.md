---
title: atlas modules — module inventory validation
date: 2026-08-09
repository: rwatp-core, rwatp-notifier, vestascan-api
status: Complete
---

# Benchmark: Does `atlas modules` inventory child directories with honest evidence counts?

## Repository

| Repo | Ingest | Subject used |
|------|--------|--------------|
| rwatp-core | 118 commits, 3122 TS edges, `~/.atlas/rwatp-core.db` | `src/modules` (default) |
| rwatp-notifier | 9 commits, 145 edges, `~/.atlas/rwatp-notifier.db` | `src` (no `src/modules`) |
| vestascan-api | 98 commits, 1104 edges, `~/.atlas/vestascan-api.db` | `src/modules` |

## Question

Can Atlas list immediate child directories of a subject and attach only
deterministic file/commit/edge counts (plus an explicit derived test flag)?

## Ground Truth

- RWATP and VestaScan use business modules under `src/modules/*`.
- Notifier uses a layered `src/*` layout without `src/modules`.
- Counts must match prefix aggregation over ingested evidence, not a
  live-only tree.

## Atlas Evaluation

### Commands used

```
atlas modules
atlas modules src          # notifier
atlas modules --json
```

### Fixture tests

`crates/core/tests/modules_fixture.rs` — **6** tests (discovery, ordering,
counts, isolation, empty, derived tests flag).

### Workspace

`cargo test --workspace` — **356** passed (freeze run).

### Representative outputs (RWATP)

11 modules including historical `access-control` / `base` from `files`
evidence. Live-heavy: core 102 files / 60 commits; identity 46 / 28.

Notifier default `src/modules` → 0; `atlas modules src` → 16 layer dirs.

### Timing

**No timing benchmark was performed.**

## Classification

| Overall | Complete |
| Commands needed | 1–2 |
| Source reads needed | 0 for inventory |
| Confidence | High (path evidence) |
| Hidden understanding | Historical dirs remain visible |

## Outcomes

- Decision: `docs/decisions/2026-08-09-atlas-modules.md`
- New primitive: no (aggregation only)
- Regression: none observed
- Limitation: default subject UX for non-modular repos (product decision)