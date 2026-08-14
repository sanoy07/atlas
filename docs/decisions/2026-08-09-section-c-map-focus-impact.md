---
title: Section C1–C3 — Map, Focus, Impact as claim-oriented orientation
date: 2026-08-09
status: Implemented
---

## Problem

After B1–B10 and the reasoning loop, Atlas still required engineers to
know which aggregation command to run. The product need is:

1. **Map** — what is this repository?
2. **Focus** — what surrounds this subject?
3. **Impact** — if I touch this, what else should I investigate?

with explicit **observed / derived / inferred / unknown** layers and
deterministic ranking dimensions for later AI reasoning.

## Decision

Add three composition surfaces over existing evidence (no new extractors,
no schema, no LLM):

| Command | Core | Output |
|---------|------|--------|
| `atlas map` | `build_map` | `MapReport` + `OrientationClaim`s |
| `atlas focus <subject>` | `build_focus` | `FocusReport` |
| `atlas impact <subject>` | `build_impact` | `ImpactReport` + `ImpactNeighbor` + `EvidenceDimensions` |

### Epistemic layers

- **Observed** — direct stored facts (edges, hot-file counts, config rows)
- **Derived** — deterministic aggregation (max commits, coupling rank, test links)
- **Inferred** — reserved for weak heuristic ranks (sparingly in v1)
- **Unknown** — explicit coverage gaps

### Impact ranking dimensions (deterministic inputs for future AI)

```
subject_relevance, structural_connectivity, historical_cochange,
corroboration, temporal_recency (0.0 reserved in v1)
rank_score = 0.35*struct + 0.30*cochange + 0.20*rel + 0.15*corr
```

Not ownership, not safety, not root cause.

### Modules subject resolution

Prefer non-empty `src/modules`; else fall back to `src` (layered repos).

## Non-goals

- No architecture meaning models (Section D)
- No deeper semantic proof of claims (still separate from AI verification)
- No rewrite of B commands

## Validated

- `section_c_fixture` 7 tests
- RWATP smoke: map, focus core/order.service, impact order.service

## Future

- temporal_recency on neighbors
- feed Map/Focus/Impact claims into reasoning EvidencePacket automatically
- stronger evidence priority model for intent vs implementation (reasoning + C together)
