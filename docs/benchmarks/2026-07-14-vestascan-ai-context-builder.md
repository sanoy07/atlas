---
title: VestaScan AI Context Builder
date: 2026-07-14
repository: vestascan-api
issue: dogfood-02
status: Complete
gap_0_id: repository-awareness
gap_0_classification: context
gap_0_description: dist/ build artifacts masked the entire src/infrastructure/ai/ factory layer — 70% of candidates were compiled output with zero source signal
gap_0_threshold: 3
gap_1_id: short-anchor-false-positives
gap_1_classification: retrieval
gap_1_description: Anchor "AI" (2 chars) matches "blockchain" and "chains" as a substring — 70% of candidates were unrelated blockchain files with no AI relevance
gap_1_implementation: Enforce word-boundary matching for anchors <= 3 characters; longer anchors keep current substring behaviour
gap_1_success: atlas investigate ai context builder returns only AI-related files; no blockchain or chains files in the candidate set
gap_1_threshold: 3
---

# Benchmark: AI Context Builder (VestaScan)

## Repository

Name: vestascan-api  
Architecture: TypeScript/Express + Apollo GraphQL + MongoDB/Mongoose + Anthropic SDK  
Size (approx commits): 89 commits  
Atlas ingested: git+typescript (development branch)

## Question

What context does the AI assistant receive? What feeds the system prompt?

## Ground Truth

Determined after: `chat.service.ts` → `ContextBuilderService.buildSystemPrompt` → `TokenService.getTokens` + `CacheManager.getInstance`. The context builder pulls token data and caches the system prompt. `chat.service.ts` is the Anthropic SDK entry point.

---

## Atlas Evaluation

### Commands used (in order)

```
atlas investigate AI context builder
```

### Manual source reads required

None — the chain was fully surfaced by Atlas. Manual read of `notification-message.model.ts` done separately to confirm isolation was correct.

### Wrong branches followed

- "AI" matched as substring of "blockchain" ("blockch**ai**n") and "chains" ("ch**ai**ns") — 8 dist/ files and 8 chain icon assets entered the candidate set as false positives.
- Total noise: 16 of 23 candidates (70%) were irrelevant — the worst signal-to-noise ratio across all VestaScan investigations.

### Useful observations

- Despite the high noise, the 3 relevant files (`context-builder.service.ts`, `chat.service.ts`, `token.service.ts`) were all correctly surfaced and their structural connections were accurately shown.
- CALLS_STATIC edges revealed: `chat.service.ts` → `ContextBuilderService.buildSystemPrompt`, and `context-builder.service.ts` → `TokenService.getTokens` + `CacheManager.getInstance`. This is the complete chain.
- Historical co-changes confirmed: `chat.service.ts` and `context-builder.service.ts` were changed together 2× — Atlas correctly identified them as coupled.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall | Improved |
| Commands needed | 1 |
| Source reads needed | 0 (chain) |
| Confidence at completion | Medium |

**Improved** — Atlas found the correct chain in 1 command. Confidence is Medium because 70% of candidates were noise, making the signal hard to read without domain knowledge to filter it.

---

## Outcomes

**Decision produced?** N

**New primitive required?** Y  
Gap 1 (confirmed): dist/ exclusion — same as investigation 01. The 8 dist/ files appeared again.  
Gap 2 (new): Short anchor false positives. "AI" (2 characters) matched as a substring within "blockchain" and "chains" — semantic concepts that have nothing to do with artificial intelligence. The current substring LIKE match is appropriate for longer anchors (5+ chars) but produces high false-positive rates for 2-3 character anchors. Proposed primitive: word-boundary enforcement for anchors shorter than 4 characters (e.g., require the anchor to be surrounded by `/`, `-`, `.`, or string boundaries in file paths).

**New abstraction earned?** N (dist/ exclusion is now confirmed across 2 investigations — approaching threshold)

**Regression?** N

---

## Notes

The short-anchor false positive is architecturally different from the dist/ noise problem. It would occur even in a clean repository without dist/. "AI" → "blockchain" is a concept resolution failure: the system found the substring but not the concept. This is a different failure mode from missing a relationship.

The dist/ exclusion primitive is now confirmed across investigations 01 and 02 (N=2). One more occurrence earns the abstraction. Given VestaScan commits dist/ systematically, any subsequent investigation will confirm it again.

The actual AI chain (chat.service → context-builder → token.service) is a clean 3-node chain that Atlas surfaced perfectly. The investigation is not Blocked — it's Improved because the useful signal is present, just buried.
