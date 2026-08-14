---
title: Structural code-intel — deterministic operations vs. host domain drills
date: 2026-08-13
repository: rwatp-core
status: Complete
---

## Repository

`/home/sanoy/Vesta/rwatp-core` with `rwatp-eval.db`. TypeScript, NestJS-adjacent
but not NestJS: static-method service classes, ES imports, direct Mongoose
access. 4028 structural edges after re-ingest.

## Question

Can general structural operations replace product-specific host drills without
losing answer quality?

The prior state: the agent answered storage and payment-flow questions
correctly, but via regexes and prose templates in `agent/atlas_agent.py` that
named `ListingAsset`, GCS, and `tryEnqueue` directly. The question is whether
domain-neutral operations over `structural_edges` reach the same answers — and
what they cost.

## Ground Truth

Established by manual source reading before the operations were built:

1. `tryEnqueue` is called from `payment-settlement.service.ts` and
   `signing.service.ts`, both via `OrderFulfillmentService.tryEnqueue`.
2. `GoogleCloudStorageAdapter` implements `IStorageProvider`, declared in
   `storage.interface.ts`.
3. Data-room storage flows through `ListingAsset`; KYC uses GCS through a
   different surface. Conflating them is the known trap.
4. A test's `fs.writeFile` is not evidence that the backend stores files.

## Atlas Evaluation

### Commands used (in order)

```
atlas callers tryEnqueue
atlas callers OrderFulfillmentService.tryEnqueue
atlas implementations IStorageProvider
atlas structural src/infrastructure/storage/google-cloud-storage.adapter.ts
atlas capabilities
atlas code-search ListingAsset
atlas code-search getSignedUrl
atlas map
atlas investigate "storing files"
```

Plus three agent cases: storing files, data room / GCS, payment→fulfillment.

### Manual source reads required

Zero for the deterministic cases. The `implements` relationship, previously
only reachable by opening the adapter file, is now an OBSERVED edge.

### Wrong branches followed

None in the final run. Historically — and this is the finding that motivated
the phase — the wrong branches were: naming a hub service instead of the
callers of `tryEnqueue`; treating a `.js` import path as a definition while
the `.ts` existed; and answering the storage question from a test's
`fs.writeFile`.

### False positives

| Query | Unexpected match | Reason | Severity |
|---|---|---|---|
| `implementations IStorageProvider` (first cut) | factory-imported classes | DERIVED naming/factory heuristic, no `implements` evidence existed | High — plausible and unverifiable |
| `implements` targets (before import fix) | `UNRESOLVED:type` | multi-line `import { … }` never registered symbols | High — silently produced 26 edges pointing nowhere |
| storage questions (agent) | test `fs.writeFile` | test paths ranked alongside production | Medium — fixed by production-before-tests ordering |
| data-room prose (agent) | `uploadPublic` for private documents | synthesis wording, not structure | Low — paths correct, wording imprecise |

### Useful observations

- **The multi-line import bug was the real blocker.** Adding the `implements`
  pass alone would have produced 26 edges resolving to `UNRESOLVED:type` — a
  feature that appears to work, reports edges, and answers nothing. Detection
  and extraction disagreeing about what a symbol is remains the most dangerous
  class of parser bug, because the output looks structured.
- **Latency ratio is the headline.** Deterministic operations answer in
  9–47 ms; the agent answers the same questions in 59–183 s. Three to four
  orders of magnitude, on identical evidence.
- Production-before-tests ordering did more for answer quality than any
  ranking change — most false positives were tests outranking sources.

## Classification

| | |
|---|---|
| Overall | Optimal (deterministic cases) / Improved (agent cases) |
| Commands needed | 1 per structural question (was 3+ plus a source read) |
| Source reads needed | 0 |
| Confidence | High for rwatp-core; **Low for generalization** — single repository |
| Noise removed | Test-file false positives; factory-heuristic implementations replaced by OBSERVED edges |
| Hidden understanding revealed | That the agent's correctness lived in the host, not in Atlas — the drills were right for the wrong reason |

## Outcomes

- **Decision produced?** Yes — `docs/decisions/2026-08-13-structural-code-intel.md`
  (written retroactively; the phase shipped without it).
- **New primitive earned?** Yes — `StructuralEdgeKind::Implements`. Earned by
  a direct evidence limitation: the relationship is *stated in the source*
  (`class X implements I`) and Atlas was inferring it heuristically instead of
  observing it. This is an Evidence change, not a Context change.
- **New abstraction earned?** Yes — general structural operations, at N=3
  (flow drill, storage drill, definition-ranking drill). The third would have
  been the forbidden copy-paste.
- **Regression?** None detected. 14/14 bench pass.
- **Unexpected discoveries?** That `implementations` had been shipping DERIVED
  answers with the confident shape of OBSERVED ones. Nothing in the output
  distinguished "I found an `implements` edge" from "a file imports the
  factory and its name ends in Adapter" until the labelling was made explicit.

## Not covered

- **Cross-repository validation — the outstanding Principle 4 obligation,
  still open.** Everything here is rwatp-core. The operations are
  domain-neutral by construction, but that is an argument, not evidence.

  The full 60-case cross-repo suite was run afterwards against the Phase 1
  binary (60/60 pass — see `docs/benchmarks/2026-08-13-cli-foundations.md`),
  and it **does not discharge this debt**. Verified rather than assumed:
  `run_suite.sh` contains zero references to `callers`, `implementations`,
  `capabilities`, or `code-search` — it exercises `map`, `modules`,
  `investigate`, `agent`, and `ripgrep`. A green suite says the code-intel
  work caused no regression elsewhere; it says nothing about whether the
  code-intel operations generalize.

  Neither harness can close this as written. `code-intel-bench.sh` asserts on
  `tryEnqueue`, `IStorageProvider`, `ListingAsset`, and `getSignedUrl` — all
  rwatp symbols. Closing the debt requires **new ground truth per repository**
  (a known interface + its implementations, a known multi-hop call chain) for
  VestaScan and the research corpus. That is the specific, unglamorous work
  this benchmark is deferring, named so it cannot be quietly forgotten.
- Capability index remains query-time; no latency measurement justifies moving
  it yet.
