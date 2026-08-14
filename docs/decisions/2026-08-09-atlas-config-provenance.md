---
title: atlas config — configuration artifact inventory and path provenance
date: 2026-08-09
status: Implemented
---

## Problem

Configuration files (`package.json`, `tsconfig`, locks, Dockerfile, …)
are ingested into `configuration_artifacts` with SHA-256, but operators
lacked a command that answers:

> What configs does Atlas hold, and what commit history is known for this
> path — without fabricating historical content snapshots?

## Methodology validation

- **Principle 1.** B7 depends on config artifact presence; dogfood
  showed empty inventory until re-ingest — B10 makes that state visible.
- **Principle 2.** Reuses `list_configuration_artifacts`,
  `configuration_artifact`, `commits_for_file` /
  `commits_for_identity`, historical redirect helpers.
- **Principle 3.** This decision +
  `docs/benchmarks/2026-08-09-atlas-config-provenance.md`.

## Decision

Command:

- `atlas config` → inventory (`ConfigInventoryReport`)
- `atlas config <path>` → provenance (`ConfigArtifactReport`)

Core: `compute_config_inventory`, `compute_config_provenance`.

### Evidence source

| Field | Source |
|-------|--------|
| artifact_kind, sha256, raw content length, ingested_at | `configuration_artifacts` (repo-scoped) |
| touching commits | `commits` ⨝ `commit_files` or identity commits when FileIdentity exists |
| redirect_note | historical path → current via identity |

### Deterministic vs derived

Content identity and commit lists are **DETERMINISTIC** projections.
Absence of historical **content** versions is stated in `limitations`
rather than implied.

### Repository isolation

All queries filter `repo_path`.

### Non-goals / refusals

- **No historical content snapshots** per commit — only current ingested
  body + SHA and the list of touching commits.
- No second config extractor; recognised root files only (existing
  `ingest_configuration_artifacts` list).
- No schema change for B10 itself.

## Alternatives considered

- **Extend `atlas show config:…` only.** Insufficient for inventory and
  explicit limitations language.
- **Store full content history.** Deferred — large, not earned by B10
  question.

## Validated outcome

RWATP after re-ingest: 5 artifacts; `package.json` artifact_present,
full SHA, 59 touching commits. Cross-repo: notifier + vestascan-api also
5 artifacts with config-artifacts provenance for deps.

## Future

No timing benchmark performed. Nested package.json (monorepo packages/*)
remains out of scope per ingest decision.