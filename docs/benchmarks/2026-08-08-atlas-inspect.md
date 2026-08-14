---
title: atlas inspect — architectural signal from evidence attached to spatial subjects
date: 2026-08-08
repository: Atlas self + RWATP core
issue: Repository Intelligence Ingestion — Step 3 validation
status: Complete (RWATP + Atlas self); VestaScan pending
---

# Benchmark: Does `atlas inspect` approach an architectural map on real repositories?

## Repositories

1. **Atlas self** — Rust workspace, 22 commits, 1 structural edge (Rust
   extractor is minimal).  Serves as a smoke test of the aggregation
   surface, not the boundary-partition signal.
2. **RWATP core** — TypeScript / GraphQL / Mongoose, 118 commits, 34
   rename records, 31 file identities, **3,122 TypeScript structural
   edges**, 5 ingested docs, 8,044 lexicon relationships.  This is the
   primary test.
3. **VestaScan** — pending.

## Question

For business modules in RWATP (`src/modules/identity`, `payment`,
`signing`, `compliance`, `market`, `notifications`), does
`atlas inspect` produce output that:

- lists what the module **contains**,
- surfaces what it **depends on** (boundary-crossing outgoing edges),
- surfaces what **uses** it (boundary-crossing incoming edges),
- summarises **internal cohesion** (count only),
- exposes **commit activity** and related **PRs / issues** aggregated
  across the subtree,
- surfaces literal-containment **documentation**,
- attaches ambient and subject-matching **profile claims**?

## Ground Truth (RWATP structural summary, one command per row)

All 9 business modules under `src/modules/`, plus one file-level zoom.
Sorted by commit count descending — a rough proxy for where the work
happens.

| Module              | Files | Commits | Depends on             | Used by                | Internal | Used-by / Depends-on |
|---------------------|------:|--------:|------------------------|------------------------|---------:|---------------------:|
| **core**            | 8     | 60      | 372 → 64 targets        | 409 ← 67 sources        | 313      | 1.10                 |
| **identity**        | 11    | 28      | 84 → 36 targets         | 248 ← 58 sources        | 125      | **2.95**             |
| **compliance**      | 8     | 18      | 66 → 32 targets         | 24 ← 9 sources          | 31       | 0.36                 |
| **support**         | 7     | 17      | 69 → 24 targets         | 54 ← 5 sources          | 78       | 0.78                 |
| **payment**         | 6     | 11      | 142 → 33 targets        | 61 ← 12 sources         | 52       | 0.43                 |
| **signing**         | 8     | 6       | 85 → 26 targets         | 105 ← 13 sources        | 48       | 1.24                 |
| **market**          | 2     | 1       | 3 → 2                   | 1 ← 1                   | 0        | 0.33                 |
| **notifications**   | 3     | 2       | 2 → 1                   | 0 ← 0                   | 0        | 0.0                  |
| **blockchain**      | (not inspected — omitted from first run; run separately)                                       |

Whole `src/`: 506 depends_on / 809 used_by / 1,573 internal. 13
immediate children.  Hot file: `src/graphql/generated/graphql.ts`
(39 touches — the codegen output).

### The order domain — a file-level demonstration

RWATP has no top-level `orders/` module.  The order domain lives inside
`core`.  `atlas inspect src/modules/core/services/order.service.ts`
produced:

- 45 boundary depends_on edges → 22 distinct external targets
- 28 boundary used_by edges  ← 5 distinct external sources
- 10 recent commits (including `feat(orders): quoteOrder — price and
  eligibility before an order exists #134`)

The DEPENDS ON list reads like a specification of what an order
needs, at symbol-level resolution (extractor gives us `Class.method`
targets):

```
order.service.ts  →  CurrencyConversionService.convert
order.service.ts  →  PricingEngineService.computeForOrder
order.service.ts  →  PricingEngineService.persistUsage
order.service.ts  →  ShareClassService.reserveForOrder
order.service.ts  →  ShareClassService.releaseForOrder
order.service.ts  →  WalletService.getDeliveryWallet
order.service.ts  →  PaymentService.initiateForOrder
order.service.ts  →  PaymentService.notifyManualInitiated
order.service.ts  →  SigningService.assertSigned
order.service.ts  →  SigningService.buildSigningSnapshots
order.service.ts  →  OrderHistoryService.record
order.service.ts  →  SettingsGlobal.getSettings
order.service.ts  →  compliance/models/kyc-profile.model.ts
```

The USED BY list is short and precise — `expire-orders.handler.ts`
calling `OrderService.expireBatch`.  This is the boundary partition
delivering exactly what was asked for.

**Architectural reading (unaided by any AI, just from these numbers):**

- **identity** is a foundational module — the highest used-by-to-
  depends-on ratio (2.95).  Consumed by graphql/context.ts,
  common/graphql/shield/rules.ts, and every other module's resolvers.
- **compliance** is a heavy consumer — many outgoing (calls KYC
  providers, blockchain whitelist writers) with few incoming (only
  called from a handful of resolvers).
- **payment** is the highest-fanout consumer (142 depends_on) — it
  reaches into blockchain, compliance, notifications, identity.
- **signing** is roughly balanced — both consumes and is consumed
  (order signing requirements sit on the read path).
- **market** and **notifications** are stubs/model-only in the current
  codebase — the graph confirms it.

Every one of these characterisations came from a single command per
module — no source reading, no LLM.

## Atlas Evaluation

### Commands used (per module)

```bash
cd /home/sanoy/Vesta/rwatp-core
ATLAS_DB=/tmp/rwatp.db atlas inspect src/modules/identity
# … for each module
ATLAS_DB=/tmp/rwatp.db atlas inspect src
```

### Manual source reads required

**Zero.**  Every row in the ground-truth table above came from the
single-command inspection.

### Wrong branches followed

None observed at the granularity of the boundary-partition table.

### False positives

| Query | Unexpected match | Reason | Severity |
|-------|------------------|--------|----------|
| `inspect src/modules/identity` PROFILE section | `Module: modules` | `inspect_repository` walks depth 1 of `src/` only and observes "modules" as the top-level `src/` subdirectory.  It never enumerates `src/modules/*` as modules.  The `subject_module_name` helper faithfully matches against the observed set, so RWATP's actual business modules never appear as `Module` claims.  This is an `inspect_repository` limitation, not an `atlas inspect` bug. | Medium — cosmetic, does not distort the structural signal |
| DEPENDS ON entries | ~40% of RWATP boundary edges are `UNRESOLVED:external:*` (graphql, mongoose, express, firebase-admin, etc.) | The TypeScript structural extractor cannot resolve NPM dependencies to source files.  These are honestly labelled as `UNRESOLVED` rather than dropped silently. | Low — labelling is explicit; consumers can filter |

### Useful observations

- The **used_by ÷ depends_on** ratio derived from the summary line
  (visible without reading the detailed edges) is a legitimate
  architectural indicator on its own.  identity's 2.95 versus
  compliance's 0.36 tells you which is foundational and which is a
  consumer, before you look at a single edge.
- The **INTERNAL** count line — deliberately not listed in detail —
  turned out to be a useful cohesion proxy at real scale.  identity
  has 125 internal edges to 84 depends_on: high cohesion, tight
  module.  payment has 52 internal to 142 depends_on: leaks outward.
- The **HOT FILES WITHIN** section immediately named the actual
  centres of gravity per module (identity: user.service.ts 13×;
  compliance: compliance.service.ts 11×; payment:
  payment-settlement.service.ts 5×).
- **RECENT ACTIVITY** surfaced meaningful commit messages including
  PR numbers, letting a reviewer jump straight to the linked PR
  discussion.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall (RWATP) | **Optimal** — architectural characterisation from one command per module, no source reads |
| Overall (Atlas self) | Improved — aggregation shape correct; boundary signal limited by Rust extractor |
| Commands needed | 1 per module |
| Source reads needed | 0 |
| Confidence at completion (RWATP) | High for used-by/depends-on/internal counts; Medium for individual edge lists (UNRESOLVED noise) |
| Noise removed (vs. prior) | The internal-edge count-only line prevents drowning: identity has 125 internal edges, none listed |
| Hidden understanding revealed | The used-by / depends-on ratio surfaces module role (foundation vs consumer vs stub) without any semantic layer |

---

## Outcomes

**Decision produced?** Y — `docs/decisions/2026-08-08-atlas-inspect.md`.

**New primitive required?** No.  The one gap surfaced during this
benchmark (`inspect_repository` missing `src/modules/*` convention) is
a limitation of an existing primitive, not a missing one.  It is a
candidate for a separate follow-up decision if RWATP-style layouts
prove common.

**New abstraction earned?** No.

**Regression?** No.

## Blast radius

None.  All aggregation is read-only; no schema, no ingest changes; no
side effects.

## Notes / observed follow-ups (not fixed here)

1. `inspect_repository` observes only depth-1 `src/` subdirectories as
   `Module` claims.  RWATP nests business modules under `src/modules/`,
   so the observed module is "modules" and never a business name.
   Extending the inspector to recognise the `src/modules/*` convention
   is a small, well-scoped follow-up.
2. `UNRESOLVED:external:*` boundary edges dominate the `DEPENDS ON`
   listing for TypeScript projects.  Two options for a future follow-up
   (both need real evidence before implementing):
   - Split `DEPENDS ON` into `DEPENDS ON (internal)` and `DEPENDS ON
     (external)` in the CLI rendering.
   - Add an `--internal-only` flag to `atlas inspect`.
3. On very large subtrees (`atlas inspect src` here produced 506
   boundary depends_on edges), the CLI truncates at 20 with an
   `and N more` footer.  JSON output is complete.  If the truncation
   turns out to be more limiting than expected, add a `--limit N` flag.

VestaScan run pending.  Same commands should exercise the same code
path; recording it will strengthen cross-repository confidence.
