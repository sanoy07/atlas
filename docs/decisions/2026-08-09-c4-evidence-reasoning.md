---
title: C4-ER — Evidence ranking, temporal supersession, hard claim entailment
date: 2026-08-09
status: Implemented
---

## Problem

Independent adversarial evaluation of Atlas on RWATP scored **~47/100 overall**
(evidence engine ~58, reasoning layer ~28). The critical failure class:

> **Existence of a reference is being confused with support for a claim.**

Concrete failure: for the vague question **"orders timeout"**, local AI found a
real Redis timeout issue (#19) and file, connected them to order problems, and
Atlas marked the causal claim **SUPPORTED** solely because the cited file and
issue resolved inside the evidence packet.

That is a verification bug, not a missing command. Building flow reconstruction
or Section D next would amplify confident wrong answers.

Secondary gap (C4-B): GitHub corpus incomplete (~29 issues stopping around #62;
order-related #134 visible in commits but missing as issue). Merge-SHA PR
linking often fails when `merge_commit_sha` is absent.

## Methodology validation

| Principle | Application |
|-----------|-------------|
| Features earned by production evidence | Failure reproduced on RWATP adversarial suite (N≥1 high-severity; same class expected on other repos) |
| Abstractions earned by repetition | Concrete hard-verify rules first (cross-domain ban, causal default PLAUSIBLE); no generic “truth engine” framework |
| Knowledge accumulated | This decision + benchmark + sacred regression tests |
| Validation before generalization | Sacred Redis regression locked; retest same adversarial suite after C4 |

## Decision

**C4 is an Evidence Reasoning Engine**, not “temporal recency” alone.

### C4-A — Evidence reasoning

1. **Evidence ranking** (`rank_evidence`) over dimensions:
   - subject relevance, structural proximity, directness/semantics,
     chronology, corroboration (implementation vs intent weighting)
2. **Temporal supersession** (`compute_supersession`):
   - later implementation may supersede earlier implementation/intent on topic
   - later **intent does not** automatically override earlier implementation
   - model: **provenance + chronology + event semantics + structural corroboration → weight**,
     not `timestamp → truth`
3. **Hard claim entailment** (`hard_verify_claim`):
   - file/issue existence ≠ causal support
   - causal language (cause/because/related-to/timeout↔redis/…) max **PLAUSIBLE**
     unless multi-source same-subject structural+historical support — and even
     then not automatic runtime proof (still PLAUSIBLE for causal)
   - **cross-domain ban**: order/payment ↔ redis/rate-limit/cache/otel causal links
     never **SUPPORTED** without structural co-evidence path (currently: never SUPPORTED)
4. **Reasoning packet upgrade** (`enrich_packet`):
   - `ranked_evidence`, `supersession`, `verification_policy` (schema_version ≥ 2)
   - prompt summary prefers ranked evidence over bag dump
5. **AI proposes; Atlas evaluates** — `verify_claims` / `verify_hypotheses` route
   through hard verify (not soft “all refs exist → Supported”)

### C4-B — Completeness hooks (partial)

- Parse GitHub numbers from commit messages: `(#N)` / `#N`
- `atlas show <commit>` links PRs/issues via message refs when merge-SHA link fails
- Full re-ingest of complete GitHub corpus remains operational (token + scope),
  not solved by ranking alone

### Sacred regression

**Permanent test case:** for question `"orders timeout"`, a claim that Redis
timeout/file/issue **causes** or **explains** order timeouts must never be
`ClaimStatus::Supported`. If a future change reintroduces that confidence, the
build fails.

Locations:
- `crates/core/src/evidence_reasoning.rs` `sacred_tests`
- `crates/core/tests/reasoning_fixture.rs` `sacred_orders_timeout_redis_not_supported`

## Alternatives considered

| Alternative | Why rejected |
|-------------|--------------|
| “Latest wins” recency only | Intent issues can be newer than implementation; architectural decisions remain valid when old |
| Soften AI prompt only | Prompt non-compliance still left verification soft; existence→support remained |
| Jump to flow reconstruction / Section D | Would amplify false Supported claims |
| Graph/runtime tracing for true causal proof | Not earned; Atlas is evidence engine, not production tracer |

## Validated outcome

- Sacred unit + fixture tests green
- Workspace tests green
- Packet schema 2 carries ranked_evidence + supersession + verification_policy
- Show: `commit_shows_linked_pr_via_message_ref`, `commit_shows_linked_issue_via_message_ref`

## Future

- Deeper corroboration: structural path between claim subject domains before any causal upgrade
- Issue/PR ↔ commit body linking at ingest time (not only show-time)
- Complete GitHub re-ingest for RWATP so #134-class issues participate as documentary
- Re-run full adversarial suite; target lift reasoning score without inventing commands
- Section D only after C4 regression holds under real AI rounds on RWATP
