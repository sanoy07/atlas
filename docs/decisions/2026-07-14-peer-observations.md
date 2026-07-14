---
title: Peer Observations
date: 2026-07-14
status: Accepted
authors:
  - Sanoy Simon
contributors:
  - Claude Sonnet 4.6
validated_by:
  - RWATP Benchmark 2026-07-14
pending_validation:
  - VestaScan
---

# Decision: Peer Observations

## Trigger

RWATP benchmark 2026-07-14, Issue #55.

Atlas correctly identified `currency.service.ts` as the file needing change.
Investigation still required a manual source read to confirm `createError` was
absent. Atlas knew createError was a system-wide contract (14 documentary
references) but could not confirm the import was missing without reading the file.

## Observation

Comparable services in `src/modules/core/services/` consistently imported
`createError.ts`. The target service did not. This was a repository convention —
not something documented anywhere, but observable by comparing sibling files.

Developers were doing this comparison mentally. Atlas was not.

## Problem

Atlas understood structure (what a file imports).
Atlas did not understand repository conventions (what a file *should* import,
given what comparable files do).

The gap required manual source reading on every issue where the convention was
the signal.

## Alternatives Considered

### RepositoryExpectation abstraction

Rejected. Only one concrete example (import gaps) existed at the time. The
abstraction had not been earned from multiple instances. Building it first would
have been framework-building for its own sake.

### Import-only comparison

Accepted as first iteration. Used existing deterministic data with no parser
changes required. Allowed immediate production validation.

### Configurable threshold

Deferred. The 50% threshold was calibrated against rwatp-core. A second
repository (VestaScan) should provide evidence that a different value is needed
before adding configurability.

## Decision

Implement peer observations using deterministic statistics over sibling files
sharing the same directory and compound suffix (e.g., `*.service.ts`).

Four edge kinds implemented:
- Import observations (keyed by target_file)
- CALLS_STATIC observations (keyed by target_file + target_symbol)
- CALLS_INSTANCE observations (keyed by target_file + target_symbol)
- REFERENCES_MODEL observations (keyed by target_file, distinct-sibling count)

Thresholds: count ≥ 2, count × 2 ≥ peer_count (≥50%). External packages
excluded. Minimum 2 peers required.

Storage generalized from `sibling_imports_by_pattern` to
`sibling_edges_by_pattern(kind)` — one method serves all four kinds.

## Validation

Synthetic tests: 39/39 passing.

Production validation revealed a bug: counting rows instead of distinct sibling
files produced `8 of 6 peers` for model references. A single service calling
`User.findById` and `User.findOne` contributed 2 to the count for the same
target file. Fixed by aggregating into `HashSet<sibling_file>` per target.

Synthetic tests could not have caught this — fixture data had 1:1 edge density.
Real Mongoose services have N:1.

After fix: Issue #55 reduced from 3 commands + 1 source read (confidence: Medium)
to 1 command + 0 source reads (confidence: High).

False positive correctly suppressed: `memory-cache.service.ts` vs
`redis-cache.service.ts` had only 1 peer, triggering the peer_count ≥ 2 guard.
Without the guard, `redis/connection.ts` would have appeared as a gap — a wrong
recommendation since memory cache intentionally avoids Redis.

## Limitations Confirmed

**First-use blindness.** If no peer has established a convention yet,
observations produce no signal. This is a property of the approach, not a bug.
The engine is retrospective. Confirmed on Issue #54: `user-holding.model.ts`
was used by zero services — no gap could be surfaced.

**Minimum-peer guard blocks small modules.** Directories with only 2 files of
the same suffix (one peer each) produce no observations. Correct for precision.

## Lessons Learned

Repository conventions can be inferred without AI.

The abstraction should emerge from multiple deterministic examples rather than
being designed first. We had one instance (imports), implemented it, then earned
the generalization when CALLS_STATIC and REFERENCES_MODEL proved to follow the
same pattern with identical aggregation logic.

The "8 of 6 peers" bug demonstrates that production validation is a different
verification class from synthetic tests. Both are required.

## Future Validation

Run against VestaScan before:
- Expanding convention types (transaction patterns, permission checks, logger)
- Making the threshold configurable
- Building a `RepositoryExpectation` abstraction

Questions to answer: does 50% produce noise in a different architectural style?
Does the minimum-peer guard suppress too many signals in a smaller repo?
