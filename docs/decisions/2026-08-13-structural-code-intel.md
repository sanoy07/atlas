---
title: Structural code-intel — general operations replace host domain drills
date: 2026-08-13
status: Implemented
---

> **Retroactive record.** This phase shipped before its decision record was
> written, leaving only `eval/code-intel-PHASE-REPORT.md` — a run report, not
> retained knowledge in the required format. That violated Principle 3 and
> required deliverables 3 and 4. Written after the fact and labelled as such,
> because a record that hides its own lateness is worse than a late record.

## Problem

The local agent produced correct answers on rwatp-core for storage and
multi-hop flow questions, but the correctness lived in the **host**, not in
Atlas. `agent/atlas_agent.py` carried domain drills: regexes for
`tryEnqueue`, hardcoded rejection of "the backend doesn't store files", and
prose templates naming `ListingAsset` and GCS.

That is product knowledge compiled into a Python harness. It does not survive
a new repository, a renamed symbol, or a second product. Every additional
domain would have added another drill, and the drills would have kept being
right for the wrong reason — the agent looked like it understood storage
because someone had written down the answer.

Concrete failures behind it:

| Question | Failure |
|---|---|
| "How does payment trigger fulfillment?" | Model named a hub service rather than the callers of `tryEnqueue` |
| "Where is X defined?" | An import path (`.js`) was treated as a definition while the real `.ts` existed |
| "Storing files / data room" | Latched onto a test's `fs.writeFile`, or blended KYC GCS usage with ListingAsset data-room storage |

Every one of these is a *structural* question that `structural_edges` already
contained the answer to. The gap was not evidence. It was that nothing exposed
reverse edges, interface implementations, or definition-ranked search as
operations an agent could call.

## Methodology validation

- **Principle 1 (features earned by production evidence):** satisfied. Each
  operation traces to a named repeated failure above, observed across multiple
  agent runs on rwatp-core, not to "this seems useful."
- **Principle 2 (abstractions earned by repetition):** satisfied, and this is
  the core of the decision. Three host drills had accumulated (flow, storage,
  definition-ranking) — N=3, the threshold at which the pattern is real. The
  third drill would have been the copy-paste that Principle 2 forbids. The
  abstraction extracted is *general structural operations*, not a "drill
  framework."
- **Principle 3 (knowledge accumulated):** satisfied only now, by this record
  and `docs/benchmarks/2026-08-13-code-intel-rwatp.md`.
- **Principle 4 (validation precedes generalization):** **partially satisfied,
  and this is the phase's real weakness.** Validation was rwatp-core only. The
  operations are domain-neutral by construction, but "domain-neutral by
  construction" is an argument, not evidence. Cross-repo confirmation was
  carried forward, not performed.

## Decision

**`crates/core/src/code_intel.rs`** — a pure query layer over
`structural_edges`, with explicit epistemic labelling: **OBSERVED** (an edge
present in the database from the last structural ingest) versus **DERIVED**
(path, import, or factory heuristic). No claim of complete runtime DI or
dynamic-import coverage.

**Storage queries** (`crates/storage`): edges by target symbol, edges by
source symbol, symbol search with definition-ish ranking, importers of a path
prefix.

**Four CLI commands**, each answering a question the drills had been faking:

| Command | Answers |
|---|---|
| `atlas callers <symbol\|file>` | Who calls this? Production callers before tests |
| `atlas implementations <Interface>` | What implements this? |
| `atlas capabilities` | Infrastructure capabilities and product surfaces via import fan-in |
| `atlas code-search <q>` | Ranked DEFINITION / WIRING / CALL_SITE / REFERENCE / TEST |

**True `implements` edges.** The first cut inferred implementations from
factory imports and adapter naming — DERIVED. Real TypeScript states it:
`export class GoogleCloudStorageAdapter implements IStorageProvider`.
`StructuralEdgeKind::Implements` was added to `atlas-ir`, with a third pass in
the TypeScript extractor.

**Multi-line import resolution.** The importer only handled single-line
`import … from …`, so symbols imported as

```ts
import {
  IStorageProvider,
} from "./storage.interface.js";
```

never entered the symbol map, and every `implements` target resolved to
`UNRESOLVED:type`. Joining multi-line imports fixed the resolution. This was
the actual blocker — the `implements` pass alone would have produced 26 edges
pointing at nothing.

**Host drills rewritten as thin wrappers.** The flow drill now forces
`atlas_callers` (ripgrep only if structural returns empty); the storage drill
forces `capabilities` + `implementations` + `code-search` and answers from
tool output. The drills remain, but they no longer contain domain knowledge —
they route to general operations.

**Agent C4 path harvesting.** The final gate reported
`evidence_paths_cited: 0` even when tools had returned many `src/…` paths,
because paths were only remembered from investigate packets.
`_remember_paths_from_text` now harvests from every tool result.

## Alternatives considered

**Keep extending host drills.** Rejected — this is the decision. Each drill
is correct for one product and invisible to Atlas, so nothing accumulates and
the evidence engine gets no better.

**Adopt SCIP as the symbol index.** Rejected for now. SCIP is the principled
long-term answer to symbol resolution, but `implements` edges plus multi-line
import joining closed the specific TypeScript hole at a fraction of the cost.
Revisit when a language arrives whose symbol resolution cannot be handled by
the existing extractor pattern.

**Embeddings / semantic search over source.** Rejected. The failures were
structural, not semantic — the answer was already an edge in SQLite. Adding a
non-deterministic retrieval layer to fix a deterministic lookup would trade
away the property the product is built on.

**A capability index persisted at ingest.** Deferred. Capabilities are
computed at query time from import fan-in, at ~22 ms. A table would be faster
and staler. Revisit when measurement shows query-time cost matters.

## Validated outcome

Benchmark `eval/code-intel-fixall-20260813-205501` — **14/14 pass, 0 fail**.

Deterministic operations on rwatp-core, with no model in the loop:

| Case | Result | Latency |
|---|---|---|
| `callers tryEnqueue` | payment-settlement + signing | 15 ms |
| `callers OrderFulfillmentService.tryEnqueue` | OK | 13 ms |
| `implementations IStorageProvider` | **OBSERVED implements** | 23 ms |
| structural adapter shows IMPLEMENTS | OK | 9 ms |
| `capabilities` storage | listing-asset surfaces | 22 ms |
| `code-search ListingAsset` / `getSignedUrl` | OK | 10–15 ms |
| `map` | OK | 47 ms |
| `investigate` storage | OK | 168 ms |

Agent cases (qwen3:4b, no web): storing files 59 s, data-room 183 s,
payment→fulfillment 144 s — all passing, and all **three to four orders of
magnitude** slower than the deterministic operations answering the same
structural questions. That ratio is the phase's most important finding and
directly motivated the MCP direction: the evidence layer is fast and correct;
the local model is the bottleneck.

Graph effect of the `implements` + multi-line import work, after re-ingesting
rwatp-core:

| Metric | Before | After |
|---|---|---|
| Structural edges | ~3455 | 4028 |
| `implements` edges | 0 | 26 |
| `IStorageProvider` target | `UNRESOLVED:type` | `storage.interface.ts` |

Agent C4 on "storing files": PLAUSIBLE with `evidence_paths_cited: 0` →
SUPPORTED with 2+ paths cited.

## Future

- **Cross-repository validation** (VestaScan, research corpus) — the
  outstanding Principle 4 obligation. Partially addressed by the Phase 1
  suite re-run; see the benchmark's "Not covered".
- **`atlas mcp`** — see `2026-08-13-mcp-understanding-layer.md`. The latency
  ratio above is that decision's premise.
- **Capability index at ingest** — deferred pending measurement.
- **SCIP** — deferred pending a language that needs it.
- **Data-room prose precision** — the model still occasionally blurs
  `uploadPublic` with private signed PUT. The structural path is correct; the
  wording is not. A synthesis-layer issue, and explicitly the kind of thing a
  frontier consumer should fix rather than a deterministic patch.
