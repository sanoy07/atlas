# Atlas Development Methodology

This document describes how Atlas evolves — not what Atlas is (see `atlas-philosophy.md`), but how decisions get made and how capabilities earn their place.

These principles emerged from real implementation work. They were not designed in advance. They are recorded here because the discipline is harder to maintain than any individual feature.

---

## The research loop

Atlas development follows this cycle, not a feature roadmap:

```
Real engineering problem
        ↓
Atlas investigates
        ↓
Benchmark
        ↓
Identify friction
        ↓
Implement smallest deterministic improvement
        ↓
Write engineering decision
        ↓
Benchmark again
        ↓
Retain knowledge
        ↓
Repeat
```

This is evidence-driven product evolution. Every capability flows through evidence, not through roadmap or marketing.

---

## Principle 1: Features are earned by production evidence

Not "this seems useful" but "this friction occurred repeatedly during real investigations."

**Why:** Every feature built from theoretical completeness has been deferred or rejected. Every feature built from observed failure has been useful: peer observations (Issue #55), decision records (self-understanding benchmark), decision-aware investigations (blocked queries).

**How to apply:** Before building anything, name the specific investigation failure that motivated it and how many times it occurred. If you can't name it, defer.

---

## Principle 2: Abstractions are earned by repetition

N=1 is a coincidence. N=3 is a pattern. N=5 is a law.

**Why:** Consistently deferred: RepositoryExpectation, configurable thresholds, decision graph, author graph, benchmark graph. All deferred because there were insufficient concrete examples. The `sibling_edges_by_pattern(kind)` method was extracted only after IMPORTS, CALLS_STATIC, and CALLS_INSTANCE all proved to use identical aggregation logic — that's N=3.

**How to apply:** When two things look similar, implement them separately first. Extract the abstraction only when the third case would require copy-pasting the pattern.

---

## Principle 3: Knowledge is accumulated, not generated

Most tools follow: Question → Reason → Answer.

Atlas follows: Question → Evidence → Reason → Answer → **Knowledge retained**.

**Why:** The last step changes everything. Investigations leave artifacts (benchmarks, decision records) that future investigations can find. The system improves because of what it has already seen, not because a model was updated. This loop is structural, not accidental.

**How to apply:** Every friction point that produces implementation should also produce a decision record. Every evaluation should produce a benchmark. The record becomes Atlas evidence. That evidence becomes available to future investigations.

---

## Principle 4: Validation precedes generalization

Not "this works" but "this works on two architectures."

**Why:** Demonstrated consistently across every major capability:
- CALLS_INSTANCE → VestaScan validation → general feature
- Peer observations → RWATP validation → decision record → VestaScan next
- Decision records → self-understanding benchmark → investigation context → structured metadata deferred

Generalizing before cross-repo validation bakes in assumptions that may be architecture-specific.

**How to apply:** Before generalizing any capability — making it configurable, extracting an abstraction, expanding its scope — name the second repository it was validated on. If there's only one, defer generalization.

**Note:** This is distinct from Principle 2. Principle 2 is about code abstractions (N occurrences in the same codebase). Principle 4 is about behavioral validation (N architectures where the feature survives intact).

---

## The maturity ladder

Every new artifact type — structural edges, benchmarks, conventions, decision records — progresses through stages deliberately:

```
Stage 1: plain markdown / raw data
Stage 2: full-text searchable
Stage 3: investigation context
Stage 4: structured metadata / criteria queries
Stage 5: graph entity / traversal queries
```

Move to the next stage only when the current stage proves insufficient for real queries.

Triggers:
- Stage 2: earned when you need `atlas search` to find the artifact
- Stage 3: earned when search returns results but investigation doesn't surface them contextually
- Stage 4: earned when repeated queries need criteria filtering that text search can't answer
- Stage 5: earned when SQL criteria queries are insufficient and relationships need traversal

---

## What the benchmark template preserves

Every formal evaluation uses `docs/benchmarks/TEMPLATE.md`. The template has a field that is easy to skip and should never be skipped:

**Wrong branches followed.**

Software engineering is not just about reaching the correct answer. It is about accumulating knowledge of which paths lead to dead ends. After enough benchmarks, Atlas will know: "Investigating payment issues? Most engineers start at PaymentProvider. The root cause is historically in SettlementService." No model learns that automatically. The benchmark corpus does.

---

## The methodology is subject to the methodology

The four principles are not immutable. They are engineering artifacts, subject to the same discipline they describe.

If a principle consistently produces poor outcomes — features that pass the checklist but fail in practice, or validations that are required but produce no learning — benchmark the failure. Write a decision record. Revise the principle.

The methodology earns its authority the same way every other Atlas capability earns its place: through repeated evidence. A principle that survives ten real implementation decisions is stronger than one written in advance. A principle that repeatedly produces wrong outcomes should be challenged, documented, and corrected.

This prevents the methodology from becoming dogma.

---

## The flywheel

> Every engineering improvement must leave behind evidence that makes the next investigation better.

The LLM consumer is interchangeable. The evidence base is the product.

```
Repository
    ↓
Evidence extraction
    ↓
Knowledge accumulation
    ↓
Investigation
    ↓
Decision
    ↓
Knowledge retained
    ↓
(repeat — each cycle the evidence base improves)
```
