---
title: Question mode lost C5.1-R seeds to an alphabetical tiebreak
date: 2026-08-10
status: Implemented
---

## Problem

Observed during a real investigation of RWATP core (rwatp-core) on 2026-08-10.

```
$ atlas investigate "how does the unread count for support tickets get computed" --no-ai

#1  0.70  scripts/rbac/refresh-service-account-permissions.js
#2  0.61  src/common/enum/bank-account.enums.ts
#3  0.57  src/infrastructure/storage/storage.factory.ts
```

No `src/modules/support/**` file appeared anywhere in the 16 returned
candidates, for a question naming the subsystem twice. The same subject in
anchor mode (`atlas investigate support ticket`) was correct at rank 1.

## Root cause

Retrieval was never the problem. Instrumenting the bag immediately before
`truncate(MAX_CORE_FILES)` showed the correct files present and `seed_files`
already holding all four:

```
[bag] seed_files=[…, support-ticket-message.model.ts, support-ticket.model.ts,
                  support-ticket.service.ts, support-ticket.validation.ts, …]
[bag] pre-truncate n=41
[bag]   0 scripts/dev/probe-commerce-queries.ts
[bag]   1 scripts/rbac/refresh-service-account-permissions.js
…
[bag]  35 src/modules/support/services/support-ticket.service.ts
```

The bag is in **alphabetical path order**. That is the tell.

`build_evidence_packet` forces C5.1-R seed files to the front of the bag
(`reasoning.rs:149`, with a comment recording a previous truncation bug). Three
rerank stages then run — C5.1-L lexical, C5.1-E role-aware, path-class — and
each re-sorts the *whole* bag. When they cannot separate candidates they return
equal scores, and every one of those sorts falls through to its shared
`file.cmp()` tiebreak. Ordering collapses to alphabetical path order, the forced
seed prefix is destroyed, and `src/modules/support/...` sorts to index 30-37 of
41. `truncate(16)` then deletes it.

The uniform `candidate score≈0.90` printed against every ranked item was the
visible symptom of that score collapse, not a display quirk.

Two smaller retrieval-noise defects were found in the same investigation:

- `extract_window` slices a fixed character count around the anchor with no word
  boundary, so cutting "…support…" yields the fragment `pport`, which then
  verifies as a real expansion term because it is a substring of a genuine file
  path.
- `resolve_concepts` bridges *any* anchor with few file-path matches through the
  first PR body that mentions it. The generic word "computed" (16 documentary
  bodies, 0 file paths) bridged into an unrelated KYC PR and injected seven
  anchors — `order`, `access`, `control`, `permission`, `access-control` among
  them — which is where the payment/access-control noise entered.

## Decision

Three changes, all in the deterministic path:

1. **Restore seed order after the rerank chain** (`reasoning.rs`). Partition the
   reranked bag into seeds and non-seeds, re-emit seeds in `seed_files` order
   (retrieval strength), then the rest in reranked order. Applied immediately
   before the cap so nothing downstream can re-sort it away.
2. **Trim partial edge words** from documentary windows (`lib.rs`), so slicing
   can no longer manufacture vocabulary like `pport`.
3. **Guard concept bridging by corpus breadth** (`lib.rs`): bridge only anchors
   appearing in ≤ 8 distinct PR/issue bodies — terms written across the whole
   corpus are shared project prose, not distinguishing concepts — and cap
   expansions per anchor at 4 rather than 8.

Deliberately *not* done: retuning the rerank weights. The collapse is a
tiebreak-ordering bug, not a weighting problem, and the weights are validated by
the existing corpora.

## Validated outcome

Question mode, same command that failed:

```
#1  0.87  src/modules/support/services/support-ticket.service.ts
#2  0.79  src/modules/support/services/support-attachment.service.ts
#5  0.70  src/modules/support/models/support-ticket.model.ts
#6  0.70  src/modules/support/models/support-ticket-message.model.ts
#11 0.56  src/modules/support/validations/support-ticket.validation.ts
```

Two further question-mode spot checks on the same repository:

| Question | #1 result |
|---|---|
| where are KYC flow steps validated | `compliance/services/kyc-flow-engine.service.ts` |
| how are listing assets uploaded to GCS | `handlers/sweep-listing-assets.handler.ts` |

Regression corpus — 16 anchor-mode cases across four repositories (ds4,
gigatoken, jj, rwatp), unchanged before and after:

```
cases: 16   passed: 15 (94%)   avg center rate: 94%   avg noise rate: 0%
```

The single failure, `jj-operation-log`, predates this work and is unrelated.

## Harness gap

`atlas eval` calls `investigate(&anchors, …)` directly — the raw anchor path. It
never routes through `options_from_question` / `build_evidence_packet`, so
**question mode is not covered by the benchmark corpus at all**. A question
supplied as a single anchor is matched as a literal string and hits nothing,
which produces a FAIL that says nothing about question mode.

This is why the defect survived a 100%-passing suite. The corpus in
`eval/investigations/rwatp.toml` therefore carries anchor-mode cases only, and
question-mode behaviour must be checked by running the CLI until the harness
grows a case kind that exercises the reasoning path. That harness extension is
the highest-value next change to the eval tooling.

## Retained knowledge

- Question mode and anchor mode share retrieval and diverge in the rerank chain.
  When question mode looks wrong, re-run the subject in anchor mode first: if
  anchor mode is right, the bug is downstream of retrieval.
- Uniform scores across ranked output means the ranking is not discriminating
  and order is coming from a tiebreak. Treat identical scores as a bug signal.
- Any ranking stage added before the cap in `build_evidence_packet` inherits the
  same hazard: it will re-sort the bag and silently undo the seed prefix.
