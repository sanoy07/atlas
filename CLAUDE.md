# Atlas — Principal Engineer Brief

You are a long-term co-developer of Atlas. Read this document before doing anything else. It governs every decision made in this repository.

---

## What Atlas Is

Atlas is a **deterministic software engineering evidence engine**.

It is not an AI coding assistant. It is not a smarter grep. It is not a code generator.

Atlas ingests repositories and accumulates engineering knowledge. LLMs are interchangeable consumers of that knowledge. The evidence pipeline is the durable product.

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
Future investigations become better
```

The AI layer must remain replaceable. Atlas is the permanent system.

---

## Architecture

Rust workspace. All crates are in `crates/`, the CLI is in `apps/cli/`.

| Crate | Purpose |
|-------|---------|
| `atlas-ir` | Intermediate representation — all shared types (`Commit`, `StructuralEdge`, `InvestigationDocument`, etc.) |
| `atlas-git` | Git connectors — reads raw git log, GitHub PRs, GitHub issues |
| `atlas-parser` | Parsers — git log, GitHub JSON, TypeScript structural extraction, rename evidence |
| `atlas-storage` | SQLite persistence — all DB reads and writes. Never contains business logic |
| `atlas-core` | Orchestration — `ingest_git`, `build_context`, `search`, `build_investigation`. Only place allowed to call multiple crates together |
| `atlas` (CLI) | Terminal presentation only. No business logic. Calls `atlas-core` and prints |

**Layering rule:** No layer does another layer's job. Connectors collect. Parsers transform. Storage persists. Core orchestrates. CLI presents.

## Current Evidence Types

```
EvidenceType::Observed     — file paths, structural edges
EvidenceType::Documentary  — PRs, issues
EvidenceType::Engineering  — decision records, ADRs
EvidenceType::Historical   — commit messages
```

## Current Structural Edge Types

```
IMPORTS          — ES import statements
CALLS_STATIC     — static method calls (ClassName.method)
CALLS_INSTANCE   — instance method calls (obj.method)
REFERENCES_MODEL — Mongoose model operations (Model.findOne, Model.create, etc.)
```

## Evidence vs. Context

These are orthogonal axes. **Never confuse them.**

**Evidence** answers: *What does Atlas know?*
**Context** answers: *What should participate in this investigation?*

New capability must be classified as Evidence or Context before implementation begins. Repository Awareness is Context (it shapes which evidence participates), not Evidence (it doesn't observe anything new).

---

## Atlas Development Methodology v1

These principles are non-negotiable. They emerged from real implementation work and are validated across multiple repositories.

### Principle 1: Features are earned by production evidence

Not "this seems useful" — "this friction occurred repeatedly during real investigations."

Before building anything: name the specific investigation failure that motivated it and how many times it occurred. If you cannot name it, do not build it. Recommend a benchmark instead.

### Principle 2: Abstractions are earned by repetition

N=1 is a coincidence. N=3 is a pattern. N=5 is a law.

Implement concrete cases first. Extract abstractions only when the third case would require copy-pasting the pattern. Never create generic frameworks speculatively.

### Principle 3: Knowledge is accumulated, not generated

Every implementation that changes Atlas behavior must produce:
- A decision record (`docs/decisions/YYYY-MM-DD-slug.md`)
- A benchmark entry (`docs/benchmarks/YYYY-MM-DD-slug.md`)
- Updated documentation

Implementation without retained knowledge is incomplete.

### Principle 4: Validation precedes generalization

A capability working on one repository is insufficient. Cross-repository validation is required before promoting an observation into a general Atlas primitive.

RWATP and VestaScan are the current validation corpus. Both must confirm a gap before it earns implementation.

### The methodology is subject to the methodology

If a principle consistently produces poor outcomes, benchmark the failure, write a decision record, and revise the principle. The methodology is an engineering artifact, not dogma.

---

## Before Touching Code

This sequence is mandatory. Do not skip steps.

1. **Identify the exact engineering problem.** What investigation failed? Which query returned wrong results?
2. **Show evidence.** What Atlas output demonstrated the failure?
3. **Classify the failure:**
   - Evidence limitation (missing edge type, missing source)
   - Context limitation (wrong files participating)
   - Structural limitation (parser gap)
   - Repository Awareness failure (build artifacts, generated code)
   - Retrieval failure (anchor mismatch, substring noise)
   - Threshold tuning (50% peer threshold, MAX_SEEDS, etc.)
   - Missing primitive (new evidence class needed)
   - Bug (existing behavior incorrect)
   - Regression
   - Unknown
4. **Determine if implementation is earned.** Count N across repositories. If N < 2, write a benchmark instead of code.
5. **Only then implement** — the smallest deterministic change that addresses the classified failure.

---

## Common Atlas CLI Commands

Run from the relevant repository directory with `ATLAS_DB` set:

```bash
# Investigation (most powerful — multi-anchor structural neighborhood)
atlas investigate token deployment

# Full-text search across git, decisions, PRs, issues
atlas search error handling

# Structural edges for a file (--reverse shows peer observations / convention gaps)
atlas structural src/modules/core/services/token.service.ts --reverse

# Complete context document for a file
atlas context src/modules/core/services/token.service.ts

# Co-change coupling partners
atlas co-changes src/modules/core/services/token.service.ts

# Change timeline
atlas timeline src/modules/core/services/token.service.ts

# Most frequently modified files
atlas hot-files --limit 20

# First commit introducing a file
atlas when-introduced src/modules/core/services/token.service.ts

# Ingest a repository
atlas ingest . --typescript   # git + TypeScript structural edges
atlas ingest . --github       # also fetch GitHub PRs and issues
```

**Running against a non-Atlas repo:**
```bash
cd /path/to/repo && ATLAS_DB=/path/to/custom.db atlas investigate ...
```

---

## Repository Awareness

Atlas excludes build artifacts during ingest. Hardcoded defaults: `dist/`, `node_modules/`, `target/`, `build/`, `.next/`, `coverage/`, `__pycache__/`, `.cache/`, `out/`, `.nuxt/`. Also reads `.gitignore` for project-specific patterns.

This is an earned primitive (N=3 across RWATP, Atlas self-ingest, VestaScan). Do not weaken or remove it without strong counter-evidence.

---

## Decision Record Format

File: `docs/decisions/YYYY-MM-DD-slug.md`

Required fields:
```markdown
---
title: Short descriptive title
date: YYYY-MM-DD
status: Implemented | Deferred | Superseded
---

## Problem
[What investigation failure or friction motivated this]

## Methodology validation
[Which principles were satisfied before implementation]

## Decision
[What was built and why this approach over alternatives]

## Alternatives considered
[What was rejected and why]

## Validated outcome
[Before/after comparison with specific Atlas command output]

## Future
[What this enables but defers]
```

---

## Benchmark Format

File: `docs/benchmarks/YYYY-MM-DD-slug.md`

Required fields:
```markdown
---
title:
date:
repository:
status: Complete | Draft
---

## Repository
## Question
## Ground Truth

## Atlas Evaluation
### Commands used (in order)
### Manual source reads required
### Wrong branches followed
### False positives
| Query | Unexpected match | Reason | Severity |
### Useful observations

## Classification
| Overall | Optimal / Improved / Blocked |
| Commands needed | N |
| Source reads needed | N |
| Confidence | High / Medium / Low |
| Noise removed | N candidates eliminated |
| Hidden understanding revealed | What became visible |

## Outcomes
- Decision produced?
- New primitive earned? (describe gap if Y)
- New abstraction earned?
- Regression?
- Unexpected discoveries?
```

---

## Campaign Format (for large investigations)

Never attempt an open-ended investigation. Decompose into campaigns:

```
Campaign: [what system are you understanding]
    ↓
Phase 1: [one bounded objective] → [one deliverable] → [stop condition]
    ↓
Phase 2: [next bounded objective, uses Phase 1 output]
    ↓
...
```

Each phase has: one objective, explicit deliverables, explicit stopping conditions (N commands OR source read required OR all questions answered).

---

## Required Deliverables Per Implementation

Every PR or implementation session must produce:
1. Code (smallest change that addresses the classified failure)
2. Tests (verify the new behavior; do not break existing 166 tests)
3. Decision record
4. Benchmark update (or new benchmark)
5. Documentation updates if commands or output format changed

Implementation without all five is incomplete.

---

## What Not To Do

- Do not implement because something "seems useful"
- Do not extract abstractions before N=3 concrete cases
- Do not generalize before cross-repository validation
- Do not move deterministic work into the AI layer
- Do not add configuration knobs before the default fails in production
- Do not skip decision records ("we'll document it later" means it won't be documented)
- Do not start with code — start with Atlas investigations

---

## Success Criteria

The goal is not more commands, more AI, or more features.

The goal is:
- Lower investigation time for real engineering questions
- Higher confidence in investigation results
- More deterministic, reproducible evidence
- Better engineering memory across sessions
- Higher cross-repository generalization
- Lower reasoning burden on the AI consumer

---

## Current Known Gaps (do not fix without earned evidence)

| Gap | N | Status |
|-----|---|--------|
| Cross-repo event contracts (PubSub topics, HTTP triggers invisible) | 2 | Candidate — needs N=3 |
| Short anchor false positives ("AI"⊂"blockchain") | 2 | Candidate — needs N=3 |
| Configuration-time wiring (GraphQL permissions.ts invisible) | 1 | Watch — needs N=2 |
| No rename/move tracking | — | Known, deferred |
| GitHub PRs/issues require token | — | Operational, not architectural |

---

## End-of-Session Deliverable

Every session ends with:

```markdown
## Investigation Summary
## Atlas Evaluation (successes and failures)
## Implementation (what changed, why)
## Validation (how tested)
## Remaining Unknowns
## Future Work (earned by evidence only)
## Decision Record (created/updated)
## Benchmark (created/updated)
## Methodology (did this strengthen or weaken Atlas Methodology v1?)
```
