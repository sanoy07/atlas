---
title: Engineering Decision Ledger
date: 2026-07-14
status: Accepted (discipline); Pending (ingestion support)
authors:
  - Sanoy Simon
contributors:
  - Claude Sonnet 4.6
validated_by:
  - Atlas Self-Understanding Benchmark 2026-07-14
pending_validation:
  - Re-run Task 2 after 5+ decision records exist
---

# Decision: Engineering Decision Ledger

## Trigger

Atlas Self-Understanding Benchmark, 2026-07-14.

Atlas was run against its own repository with 13 questions across three
categories: architecture, historical evolution, and engineering philosophy.

## Observation

Task 1 (Architecture): 4/5 Answered. ADRs work.
Task 2 (Historical Evolution): 0/4 Answered. All Blocked.
Task 3 (Engineering Philosophy): 3/4 Answered. Philosophy doc works.

All 4 Task 2 failures had the same root cause: the features in question
(CALLS_INSTANCE, peer observations, structural extraction, review-context,
supporting artifacts, investigate command) were built in conversation and
committed without rationale. Their origin — the observations that motivated them,
the alternatives rejected, the limitations discovered — exists only in chat
transcripts.

Key finding: "The engineering knowledge from this session exists only in
this conversation."

## Problem

Atlas preserves code, commits, architecture (via ADRs), and philosophy
(via `docs/atlas-philosophy.md`). It does not preserve the *reasoning* behind
individual features: what was observed, what alternatives were rejected, what
was learned during validation.

This is the gap between what Atlas records and what a senior engineer actually
wants to know: not just *what* was built, but *why this approach, why now,
and what we learned*.

## Alternatives Considered

### AI memory as primary store

Rejected. Conversation transcripts are not queryable by Atlas. They do not live
in the git repository. They are lost when context windows clear.

### Richer commit messages only

Insufficient. A commit message subject line can state motivation but cannot
capture alternatives considered, limitations discovered, or future validation
targets without becoming unwieldy. The body rarely gets read.

### No change — let history live in chat

Rejected. The benchmark produced concrete evidence of the cost: 4 of 13
questions about Atlas's own evolution were unanswerable.

### `atlas record-decision` command built immediately

Deferred. ADRs proved the discipline works without a specialized command —
someone wrote markdown files and they became queryable. The command would lower
friction but is not required to validate the concept. Build after 5+ decision
records demonstrate the format is stable.

## Decision

Adopt a three-layer knowledge preservation discipline:

1. **ADRs** (`docs/adr/`) — major architecture. Rare. Already working.
2. **Decision records** (`docs/decisions/`) — feature-level rationale. Frequent.
   One record per significant decision, committed atomically with the code it
   documents.
3. **Git commit messages** — implementation motivation in the subject line.

Decision records follow the investigation log format: Trigger, Observation,
Problem, Alternatives Considered, Decision, Validation, Limitations, Lessons
Learned, Future Validation. Plus YAML frontmatter capturing authors,
contributors, and validators — the provenance of the idea, not just the code.

**Authorship model:**
Decision records capture four distinct kinds of provenance:
- `authors` — who drove the decision
- `contributors` — who participated in developing it (including AI collaborators)
- `validated_by` — what evidence confirmed it worked
- `pending_validation` — what still needs testing

These are different from git blame. Git records who committed the code. Decision
records record who introduced the *idea* and what convinced the team it was right.

## Validation

Immediate test: after committing the two initial decision records and re-ingesting,
`atlas search "peer observations"` returned the commit message naming the decision.
`atlas search "decisions"` returned both record file paths.

Limitation: decision record *bodies* are not yet searchable. `atlas search
"first-use blindness"` returns nothing because source code is not ingested
(v0.5b). When body ingestion for `docs/decisions/` is implemented, the full
rationale becomes queryable.

## Lessons Learned

The self-understanding benchmark exposed that Atlas's knowledge has a hard cutoff
at commit time. Work that exists only in working tree or chat is invisible.
The fix is not a new Atlas feature — it is a workflow change: commit rationale
alongside code, atomically, every time.

The benchmark also confirmed that explicit documents outperform implicit knowledge.
`docs/atlas-philosophy.md` scored 3/4 on philosophy questions because someone
made the philosophy explicit. ADRs scored 4/5 on architecture because someone
wrote ADRs. The lesson generalizes.

## Future Work

**Decision record ingestion (next implementation):**
Ingest `docs/decisions/*.md` and `docs/adr/*.md` bodies into a searchable
documentary layer, surfaced as "ENGINEERING DECISIONS" in `atlas search` output.
This makes `atlas search "first-use blindness"` return the peer observations
decision record body.

**`atlas record-decision` command (after 5+ records):**
Scaffold a new decision record with today's date, open `$EDITOR`, optionally
stage the file. Build only after the format has stabilized across multiple real
decisions.
