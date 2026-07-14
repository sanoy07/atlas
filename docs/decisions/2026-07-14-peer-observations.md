# Peer Observations

**Date:** 2026-07-14  **Status:** Implemented

## Problem

During a structured evaluation of Atlas against 8 open RWATP issues, Issue #55
required the following sequence to reach a conclusion:

1. `atlas search "createError"` — 14 documentary references confirmed this is a
   system-wide error contract
2. `atlas search "errorCodes"` — confirmed a parallel pattern
3. Read `src/modules/core/services/currency.service.ts` source to confirm
   neither was imported

Confidence was Medium because the absence had to be verified manually. The
information existed in the repository — 5 of 6 peer services imported createError
— but Atlas had no way to surface it.

## Decision

Add a PEER OBSERVATIONS section to `atlas structural`. When a file has a compound
suffix (e.g. `.service.ts`, `.resolver.ts`) and ≥2 sibling files with the same
suffix exist in the same directory, compute which imports, static calls, instance
calls, and model references appear in ≥50% of peers but are absent in the target
file. Surface them as observed gaps.

Keying:
- Imports: by `target_file`
- Static/instance calls: by `(target_file, target_symbol)`
- Model references: by `target_file` (distinct sibling count, not row count)

Threshold: count ≥ 2 AND count × 2 ≥ peer_count. External packages
(`UNRESOLVED:external:*`) excluded.

## Alternatives considered

**`RepositoryExpectation` abstraction up front** — rejected. Only one concrete
instance existed (import gaps). The abstraction was earned after implementing
all four edge kinds and observing that two helper functions covered all cases.
Generalized to `SiblingEdgeRow` and `sibling_edges_by_pattern(kind)` in storage,
but no `RepositoryExpectation` trait.

**Confidence labels (Very High / High / Medium)** — rejected. The ratio
`(5 of 6 peers)` is the evidence. Labeling it adds a mapping rule that doesn't
exist in the data.

**Configurable threshold** — deferred. The 50% threshold was calibrated against
rwatp-core data. A second repository (VestaScan) should produce evidence that
a different value is needed before adding configurability.

## Evidence

- RWATP benchmark 2026-07-14: Issue #55 reduced from 3 commands + 1 source read
  to 1 command + 0 source reads. Confidence upgraded Medium → High.
- Bug found during production validation: counting rows instead of distinct
  siblings produced `8 of 6 peers` for model references. Fixed: aggregate by
  `HashSet<sibling_file>` per target, not raw row count. Synthetic tests could
  not have caught this — fixture data had 1:1 edge density.
- False positive correctly suppressed: `memory-cache.service.ts` vs
  `redis-cache.service.ts` (1 peer) triggered the peer_count ≥ 2 guard.
  Without it, redis/connection.ts would have appeared as a gap — a meaningless
  recommendation since memory cache intentionally avoids Redis.

## Known limitations confirmed

**First-use blindness.** If no peer has established a convention yet (e.g.
`user-holding.model.ts` is imported by zero other services), peer observations
produce no signal. This is a property of the approach, not a bug. The engine is
retrospective.

**Minimum-peer guard blocks low-population directories.** Modules with only 2
implementation files (one peer each) get no observations. Correct for precision;
the guard is load-bearing.

## Future validation

Run against VestaScan before expanding the 50% threshold or adding new convention
types. Specifically: does the threshold produce noise in a different architectural
style? Does the minimum-peer guard suppress too many signals in a smaller repo?

Second convention types to earn from VestaScan dogfood:
transaction patterns (CALLS_INSTANCE → `withTransaction`), logger usage,
permission checks.
