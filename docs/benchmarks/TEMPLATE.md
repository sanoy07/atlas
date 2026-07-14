---
title: Atlas Evaluation Benchmark
date: YYYY-MM-DD
repository: <repo-name>
issue: <issue number or description>
status: Draft | Complete
# Gap declarations — parsed by `atlas campaign next` to derive evidence counts.
# Single gap: use gap_id, gap_classification, etc.
# Multiple gaps: use gap_0_id, gap_0_classification, ... gap_1_id, ...
# Omit entirely if no new primitives were found.
#
# gap_id: <kebab-case-identifier>
# gap_classification: context | retrieval | structural | cross-repository | runtime | unknown
# gap_description: <one sentence — what Atlas could not see and why>
# gap_implementation: <one sentence — what to build>
# gap_success: <one sentence — what Optimal looks like after the fix>
# gap_threshold: 3
---

# Benchmark: <short title>

## Repository

Name:
Architecture: (e.g., TypeScript/Express/Mongoose, Go/gRPC, Python/Django)
Size (approx commits):
Atlas ingested: (git-only | git+github | git+github+typescript)

## Question

What engineering question was being answered?

> e.g., "Where does the currency service handle errors and why is createError absent?"

## Ground Truth

What is the correct answer, verified by source inspection?

> State this before running Atlas, or mark as determined-after if the answer wasn't known upfront.

---

## Atlas Evaluation

### Commands used (in order)

```
atlas investigate ...
atlas structural ...
atlas search ...
```

### Manual source reads required

List any files that had to be read manually to answer the question that Atlas did not surface:

- `path/to/file.ts` — reason: Atlas did not surface this because ...

### Wrong branches followed

Candidates or leads that turned out to be incorrect:

- Atlas surfaced X, but X was not relevant because ...

### False positives

Anchor matches that were semantically incorrect (wrong concept, substring collision, etc.):

| Query | Unexpected match | Reason | Severity (Low/Med/High) |
|-------|-----------------|--------|------------------------|
| "AI" | blockchain/chains/* | "ai" substring of "blockchain" | Low |

_This field is for measurement, not implementation. The corpus of false positives across benchmarks earns retrieval primitives._

### Useful observations

Things Atlas produced that were genuinely useful, even if the final answer required more:

- ...

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall | Optimal / Improved / Blocked |
| Commands needed | N |
| Source reads needed | N |
| Confidence at completion | High / Medium / Low |
| Noise removed (vs. prior) | N candidates eliminated |
| Hidden understanding revealed | What became visible that was previously masked? |

**Optimal** — Atlas answered the question; no manual source reading required.  
**Improved** — Atlas reduced the investigation but did not complete it alone.  
**Blocked** — Atlas provided no useful signal; investigation was fully manual.

_The last two rows distinguish optimization from understanding improvement. Filtering bad evidence is an optimization. Revealing previously hidden evidence is a step change in what Atlas knows._

---

## Outcomes

**Decision produced?** (Y/N)
If yes, link: `docs/decisions/YYYY-MM-DD-<slug>.md`

**New primitive required?** (Y/N)
If yes, describe the specific gap: what edge type, evidence class, or query was missing?

**New abstraction earned?** (Y/N)
An abstraction is earned only if the same gap appeared 2+ times in this evaluation corpus.

**Regression?** (Y/N)
Did this run reveal a behavior that was previously better?

---

## Notes

Anything that doesn't fit the above — architectural observations, surprising Atlas behaviors, edge cases.
