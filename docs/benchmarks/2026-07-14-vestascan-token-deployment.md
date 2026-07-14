---
title: VestaScan Token Deployment Flow
date: 2026-07-14
repository: vestascan-api
issue: dogfood-01
status: Complete
gap_id: repository-awareness
gap_classification: context
gap_description: Build artifacts in dist/ occupy 23% of investigation candidate slots and appear in historical co-change evidence despite being generated output a developer would never investigate
gap_implementation: Filter ingested files against hardcoded build artifact directories and .gitignore patterns during atlas ingest
gap_success: Token deployment investigation returns only source files with no dist/ candidates
gap_threshold: 3
---

# Benchmark: Token Deployment Flow (VestaScan)

## Repository

Name: vestascan-api  
Architecture: TypeScript/Express + Apollo GraphQL + MongoDB/Mongoose + Google Cloud PubSub + Stripe  
Size (approx commits): 89 commits  
Atlas ingested: git+typescript (development branch)

## Question

Where does the token deployment flow live — from GraphQL resolver through to blockchain recording and notification?

## Ground Truth

Determined after: resolver (`resolvers.ts`) → `DeploymentService.recordERC1404Deployment` → `Token.create()` chain. AI module (`context-builder.service.ts`, `chat.service.ts`) consumes `TokenService`. PubSub notification trigger lives in `indexer-trigger.service.ts` → `PubSubFactory.getProvider`.

---

## Atlas Evaluation

### Commands used (in order)

```
atlas investigate token deployment
```

### Manual source reads required

None for the core chain. The `dist/` noise required visual filtering.

### Wrong branches followed

- 7 of 30 core candidates are `dist/*.js` files — compiled output committed to the repository. These appeared as "STRUCTURALLY ISOLATED" with no edges, correctly flagged, but they consumed candidate slots.
- `dist/modules/quota/` contains ghost files from a module rename (quota→plan); appeared in HISTORICAL EVIDENCE pointing to old paths.

### Useful observations

- Atlas correctly surfaced the resolver → DeploymentService → Token.create() chain via CALLS_STATIC edges.
- `indexer-trigger.service.ts` → `PubSubFactory.getProvider` surfaced — the outbound notification trigger.
- AI module appeared as a 1-hop neighbor of `token.service.ts` — correct; `ContextBuilderService.buildSystemPrompt` calls `TokenService.getTokens`.
- Historical evidence correctly showed `token.service.ts` as the most-changed file (16×), consistent with it being the core domain object.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall | Improved |
| Commands needed | 1 |
| Source reads needed | 0 (chain) / 1 visual (dist/ filtering) |
| Confidence at completion | High |

**Improved** — Atlas surfaced the full deployment chain with zero source reads. Manual visual filtering was needed to exclude dist/ candidates from the result set.

---

## Outcomes

**Decision produced?** N (observation only — dist/ exclusion deferred to primitive evaluation)

**New primitive required?** Y  
Gap: Atlas ingests committed `dist/` build output alongside source. No mechanism to exclude build artifact directories during ingest. This created 23% candidate noise (7/30 slots occupied by compiled JS). The dist/ files also appear in HISTORICAL EVIDENCE with misleading co-change attribution (dist/ co-changes other dist/ files because they're all rebuilt together).  
Proposed primitive: `.gitignore`-aware ingest filter, or explicit `--exclude-path` flag for ingest.

**New abstraction earned?** N (single occurrence)

**Regression?** N

---

## Notes

VestaScan is the first repository tested that commits compiled output (`dist/`) to version control. RWATP-core did not have this pattern. The `dist/` noise is architecturally distinct from irrelevant source files — the compiled files are correct duplicates of source files, not unrelated code. An exclusion mechanism should strip them entirely, not rank them lower.

The `dist/modules/quota/` ghost confirms a module rename happened (quota→plan). Atlas has no rename tracking, so old paths remain in the database and appear in historical evidence. This is the known identity gap — not a new finding.

Cross-repo gap confirmed: the PubSub notification bridge (vestascan-api publishes → vestascan-notifier subscribes) is not visible within a single-repo investigation. Atlas correctly surfaces the publish side (`indexer-trigger.service.ts`) but cannot show the subscribe side without a multi-repo ingestion strategy.
