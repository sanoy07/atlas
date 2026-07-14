# Engineering Decision Ledger

**Date:** 2026-07-14  **Status:** Adopted (discipline), Not yet implemented (command)

## Problem

The self-understanding benchmark (2026-07-14) ran Atlas against its own repository
and asked 13 questions across three categories: architecture, historical evolution,
and engineering philosophy.

Results:
- Architecture (5 questions): 4 Answered, 1 Partial — ADRs work well
- Historical Evolution (4 questions): 0 Answered, 4 Blocked
- Engineering Philosophy (4 questions): 3 Answered, 1 Partial

All 4 Task 2 failures had the same root cause: the features in question
(CALLS_INSTANCE, peer observations, structural extraction, review-context,
supporting artifacts, investigate command) were built but never committed with
rationale. Their origin exists only in conversation transcripts.

Key sentence from the benchmark report:

> "The engineering knowledge from this session exists only in this conversation."

## Decision

Adopt a three-layer knowledge preservation discipline:

1. **ADRs** (`docs/adr/`) — major architecture, rare, already working
2. **Decision records** (`docs/decisions/`) — feature evolution rationale,
   frequent, each covering one decision with: Problem, Decision, Alternatives
   considered, Evidence, Known limitations, Future validation
3. **Git commit messages** — implementation, expressing the motivation in the
   subject line (e.g. "feat: add peer observations to surface missing imports")

Every significant feature addition should be committed with both the code change
and a decision record in `docs/decisions/`, atomically in the same commit.

This is a discipline change, not primarily a tooling change. The ADRs already
proved that well-named markdown files in a `docs/` subdirectory are sufficient
for Atlas to find and surface them.

## What a decision record covers

Decision records complement ADRs. ADRs answer "how does the system work?"
Decision records answer "why did the team make this specific choice?"

Things appropriate for decision records, not ADRs:
- Why was a threshold chosen (e.g. ≥50% for peer observations)?
- Why was an abstraction deferred (e.g. RepositoryExpectation)?
- What repository evidence motivated a feature (e.g. Issue #55)?
- What limitations were confirmed during production validation?
- What the next validation target should be?

## Alternatives considered

**AI memory / chat history** — rejected as primary store. Conversation transcripts
are not queryable by Atlas. They are not in the git repository. They decay when
context windows are cleared.

**Richer commit messages only** — insufficient. A commit message can state the
motivation but cannot capture alternatives considered, known limitations, or
future validation targets without becoming unwieldy.

**`atlas record-decision` command** — not built yet. The discipline works
without it (as ADRs demonstrated). The command would lower friction by opening
an editor with a template. Build it after the discipline is proven to be valuable
across 5+ decision records.

## Evidence

Self-understanding benchmark 2026-07-14: Atlas scored 7/13 Answered.
The 4 Blocked questions all pointed to features built without committed rationale.
The 3 Answered Task 3 questions all pointed to `docs/atlas-philosophy.md` —
a document that exists because someone made it explicit.

## Future validation

After 5+ decision records are committed, re-run Task 2 of the self-understanding
benchmark. Expected: questions about CALLS_INSTANCE, peer observations, and
supporting artifacts become Answered. If they do, the discipline is validated.
If they don't, the decision record format needs revision.
