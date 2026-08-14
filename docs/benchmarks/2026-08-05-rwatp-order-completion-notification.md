---
title: RWATP order-completion notification (Core → Notifier)
date: 2026-08-05
project: rwatp
repositories: rwatp-core, rwatp-notifier
issue: phase-2-benchmark-01
status: Complete
# Reusing the existing gap class established by
# 2026-07-14-vestascan-notification-pubsub.md.  This is the SECOND instance
# of the same class — N moves from 1 to 2 upon sign-off.  Threshold is 3.
gap_id: cross-repo-contracts
gap_classification: cross-repository
gap_description: Google Cloud PubSub topic + command type strings connect Core publisher to Notifier subscriber across repository boundaries; Atlas cannot connect them because both are string literals surfaced at unrelated file paths in unrelated repos
gap_implementation: Extract topic and command-type string literals from publishCommand call sites in publisher and .on() handler chains in subscriber; cross-reference across repos in the same project; surface as PublishesTo / SubscribesTo edges attached to a synthetic Contract node keyed by (topic, command-type)
gap_success: Cross-repo investigation of an order-completion anchor set produces a single connected trace from payment-settlement.service.ts through the notify topic to notification.service.ts without manual source reads
gap_threshold: 3
---

# Benchmark: RWATP Order-Completion Notification (Core → Notifier)

## Product context

Project: rwatp
Architecture summary:
- **rwatp-core** — Node.js/TypeScript/Express+Apollo/Mongoose; publishes cross-service commands via `@google-cloud/pubsub`.
- **rwatp-notifier** — Node.js/TypeScript/Express/Mongoose; subscribes to the same PubSub topic; renders Handlebars templates; delivers via SendGrid or Nodemailer.
- **rwatp-blockchain, rwatp-user-fe** — registered as `UserConfirmed`/`NotAccessible`; not participating in this specific trace but preserved in the project identity.

Atlas ingested (per repo): git + typescript, one shared DB at `/home/sanoy/Vesta/atlas-phase2.db`.
Ingestion driven by `atlas project ingest rwatp --typescript`. Core: 82 commits, 2502 structural edges. Notifier: 9 commits, 145 structural edges.

## Question

*"When an admin approves a manual payment for an order, how does that order-completion event become an email delivered to the user? Which files, functions, and contracts are on the path from Core to Notifier?"*

## Ground Truth

Reconstructed by manual source reading (no `atlas` commands used), 2026-08-05.

### Participating repositories and their roles

| Repo | Role in this transaction |
|------|--------------------------|
| rwatp-core     | Receives admin approval mutation; transitions order to `PAYMENT_CONFIRMED`; publishes `notify.manual-payment-approved` to PubSub topic `notify`. |
| rwatp-notifier | Subscribes to topic `notify`; dispatches on command type; renders `MANUAL_PAYMENT_APPROVED` template; sends email. |

### Hop-by-hop trace

1. **[external]** Admin (or automated process) invokes GraphQL mutation `approveManualPayment`.
2. **rwatp-core** `src/modules/payment/graphql/payment.resolvers.ts:40` `Mutation.approveManualPayment` — resolver entry point.
3. → **rwatp-core** `src/modules/payment/services/manual-payment.service.ts:115` `ManualPaymentService.approveManualPayment` — validates input; resolves provider; delegates to `provider.approve`.
4. → **rwatp-core** `src/modules/payment/providers/manual.provider.ts:137` `ManualProvider.approve` — calls `PaymentSettlementService.confirm(transactionId, adminId, { note })`.
5. → **rwatp-core** `src/modules/payment/services/payment-settlement.service.ts:37` `PaymentSettlementService.confirm` — transactional block:
   - Line 161: sets `order.status = OrderStatus.PAYMENT_CONFIRMED`.
   - Lines 168-185: `OrderHistoryService.record(...)` with `eventType: OrderEventType.PAYMENT_ATTEMPT_SUCCEEDED`.
   - **Line 197**: `publishCommand(Topics.Notify, CommandTypes.ManualPaymentApproved, { userId, email, name, transactionId, amount, currency, reference, approvedAt })` — the outbound publish.
6. → **rwatp-core** `src/lib/commands/publisher.ts:publishCommand` — type-safe wrapper; calls `PubSubFactory.getProvider().publish(topic, message)`.
7. → **[contract]** Google Cloud PubSub topic `"notify"`, command type `"notify.manual-payment-approved"`.
8. → **rwatp-notifier** subscriber infrastructure (`src/lib/commands/handler.ts` and callers) receives the message.
9. → **rwatp-notifier** `src/handlers/notify.handler.ts:118` `.on(CommandTypes.ManualPaymentApproved, ...)` — dispatch to `notificationService.notifyManualPaymentApproved(payload, context)`.
10. → **rwatp-notifier** `src/services/notification.service.ts:786` `NotificationService.notifyManualPaymentApproved` — logs; calls `sendNotification(messageId, email, userId, "notify.manual-payment-approved", "MANUAL_PAYMENT_APPROVED", payload, context)`.
11. → **rwatp-notifier** `src/services/notification.service.ts:295` `sendNotification` (private) — looks up template config; calls `EmailProviderFactory.getProvider()` (line 325); renders and delivers.
12. → **rwatp-notifier** `src/email/template-configs.ts:650` template entry keyed by `CommandTypes.ManualPaymentApproved` — subject *"Your payment has been confirmed"*; content includes badge, title, greeting, rows (amount/reference/confirmedAt), CTA *"View Order"*.
13. → **[terminal]** Email delivered via SendGrid or Nodemailer (whichever `EmailProviderFactory.getActiveProviderName()` selects at runtime).

### Contracts crossed

| # | Kind | Identifier | Producer | Consumer |
|---|------|-----------|----------|----------|
| 1 | pubsub-topic + command-type | `notify` + `notify.manual-payment-approved` | rwatp-core, `src/modules/payment/services/payment-settlement.service.ts:197`, inside `PaymentSettlementService.confirm` | rwatp-notifier, `src/handlers/notify.handler.ts:118`, `.on(CommandTypes.ManualPaymentApproved, …)` |
| 2 | typed payload | `ManualPaymentApprovedPayload` (userId, email, name, transactionId, amount, currency, reference, approvedAt) | rwatp-core, `src/contracts/commands/notify/contracts.ts:159` | rwatp-notifier, `src/contracts/commands/notify/contracts.ts` (parallel copy) |

**Important note on Contract #2:** the payload type is declared in *both* repos as parallel `interface` copies, not shared. This is a source of drift — see the *Additional finding* section below.

### Files a human had to open to reconstruct this trace

Nine files across two repos. This list is the human-attack-surface metric Atlas must shrink.

**rwatp-core (6 files):**
1. `src/modules/payment/graphql/payment.resolvers.ts` — locate the entry mutation.
2. `src/modules/payment/services/manual-payment.service.ts` — trace to provider layer.
3. `src/modules/payment/providers/manual.provider.ts` — trace to settlement service.
4. `src/modules/payment/services/payment-settlement.service.ts` — locate the publishCommand call.
5. `src/lib/commands/publisher.ts` — understand the publish mechanism.
6. `src/contracts/commands/notify/constants.ts` and `contracts.ts` — identify the topic and payload shape.

**rwatp-notifier (3 files):**
7. `src/handlers/notify.handler.ts` — locate the dispatch entry.
8. `src/services/notification.service.ts` — locate the service implementation.
9. `src/email/template-configs.ts` — locate the template.

### Terminal outcome

A transactional email is delivered to the user's email address with subject *"Your payment has been confirmed"* and body including the amount, reference, and confirmation timestamp.

---

## Atlas Evaluation

Executed 2026-08-05 against `/home/sanoy/Vesta/atlas-phase2.db`. Two runs of the
same six commands: **pre-GitHub** (git + typescript only) and **post-GitHub**
(same, plus `--github` on both repos: 78 PRs from core, 9 PRs from notifier).

Raw output artifacts preserved at `/tmp/atlas-phase2/{pre,post}-*.txt`.

### Per-command ground-truth coverage

Legend: ✓ = surfaced by Atlas at any evidence tier; ✗ = not surfaced.

| Ground-truth file | Core naive (pre) | Core naive (post) | Core targeted (pre=post) | Notifier naive (pre=post) | Notifier targeted (pre=post) |
|---|---|---|---|---|---|
| `rwatp-core src/modules/payment/services/payment-settlement.service.ts` | ✗ | ✓ (co-change of `order.resolvers.ts`; also `CALLS_STATIC` neighbour) | ✓ (top-tier candidate) | n/a | n/a |
| `rwatp-core src/modules/payment/services/manual-payment.service.ts`     | ✗ | ✓ (co-change) | ✓ | n/a | n/a |
| `rwatp-core src/modules/payment/providers/manual.provider.ts`           | ✗ | ✗ | ✓ | n/a | n/a |
| `rwatp-core src/modules/payment/graphql/payment.resolvers.ts`           | ✗ | ✓ (co-change) | ✓ | n/a | n/a |
| `rwatp-core src/lib/commands/publisher.ts`                              | ✗ | ✗ | ✗ | n/a | n/a |
| `rwatp-core src/contracts/commands/notify/constants.ts`                 | ✗ | ✗ | ✗ | n/a | n/a |
| `rwatp-core src/contracts/commands/notify/contracts.ts`                 | ✗ | ✗ | ✗ | n/a | n/a |
| `rwatp-notifier src/handlers/notify.handler.ts`                         | n/a | n/a | n/a | ✓ (top-tier) | ✗ (0 candidates) |
| `rwatp-notifier src/services/notification.service.ts`                   | n/a | n/a | n/a | ✓ (top-tier) | ✗ (0 candidates) |
| `rwatp-notifier src/email/template-configs.ts`                          | n/a | n/a | n/a | ✗ (surfaces `template.service.ts` — related file, not the config) | ✗ |

Search results:
- `atlas search "notify.manual-payment-approved"` — **0 matches** pre AND post-GitHub. String does not appear in file paths, commit messages, or PR/issue bodies.
- `atlas search "publishCommand"` — 0 matches pre-GitHub; **8 documentary matches post-GitHub** across Issues #43/#56/#69/#81/#121 and PRs #51/#61/#122. PR #122 body explicitly references `src/lib/commands/publisher.ts`. Issue #43 shows a literal `publishCommand(Topics.Notify, CommandType…)` snippet.

### The cross-repo gap (unchanged pre and post-GitHub)

Atlas has **zero** evidence types today that connect a file in `rwatp-core` to a file in `rwatp-notifier`. Every investigation is repo-scoped. Even when both sides are individually well-observed (post-GitHub Core naive surfaces `payment-settlement.service.ts`; Notifier naive surfaces `notify.handler.ts`), no output line ever states "these two files are on the same transaction."

Specifically Atlas cannot answer:
- "Who consumes `notify.manual-payment-approved`?"
- "Which repositories are on the path from `approveManualPayment` mutation to the confirmation email?"
- "If I change the `ManualPaymentApprovedPayload` interface in Core, which Notifier file breaks?"

### Manual source reads still required

Even after GitHub ingest, a human answering the original question would still have to open all 9 files listed in the ground-truth section, plus one or two `atlas search` calls to jump straight to specific string literals. Atlas's contribution is real (structural chains from `order.resolvers.ts` to `PaymentSettlementService.confirm` are visible post-GitHub) but the reader still assembles the cross-repo trace by hand.

### False positives and useful observations

- No false positives observed in the top 20 of any query.
- Notifier naive query's `notify.handler.ts` `CALLS_INSTANCE` list is unusually complete: it enumerates all 20+ dispatch targets including `notifyOrderCreated/Cancelled/Expired`. **The cross-repo schema drift is visible within one repo** — Notifier is prepared for events Core's main branch doesn't publish. Atlas surfaces this by accident, not by design.
- Post-GitHub Core naive query surfaces `PaymentSettlementService.confirm` and `PaymentSettlementService.fail` as structural neighbours of `order.resolvers.ts` — good structural evidence of the intra-Core hop from resolver to settlement.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall (per-repo)   | **Improved** (Core targeted found 4/6 files; Notifier naive found the top 2 of 3) |
| Overall (cross-repo) | **Blocked** (no evidence type connects Core files to Notifier files) |
| Commands needed      | 6 (2 investigates per repo + 2 searches); still insufficient |
| Source reads needed  | 9 (unchanged from ground-truth baseline; Atlas did not shrink this) |
| Repositories traversed by Atlas | 1 per command (0 cross-repo traversal) |
| Contracts observed as anchors   | 0 (Atlas has no Contract primitive) |

---

## Outcomes

**Contributes to gap:** `cross-repo-contracts` (N=1 → **N=2**). Threshold remains 3; one more instance across a different product still required to earn Phase 3.

### Falsification check against the plan doc's five criteria

| Criterion | Result | Evidence |
|---|---|---|
| F1 lexical | **Refuted** | Topic string `"notify"` and command-type string `"notify.manual-payment-approved"` are byte-identical on Core and Notifier sides. No naming drift. |
| F2 coverage | **Refuted** | All 9 ground-truth files live in `src/`. None in excluded directories. |
| F3 documentary | **Partially supported (surprise finding)** | PR/Issue bodies contain rich contract patterns — `publishCommand(Topics.X, CommandTypes.Y, …)` appears in 8 documents including full code snippets. `atlas search` surfaces them. But `atlas investigate` never brings documentary matches into candidate assembly, so this documentary bridge is invisible during investigation. See "Additional findings" below. |
| F4 shared type | **Partially supported** | `ManualPaymentApprovedPayload` is declared as **parallel `interface` copies** in both repos (not imported from a shared package). Atlas has no primitive to observe the structural sameness of two interfaces in two repos. |
| F5 runtime-only | **Refuted** | Topic and command-type are compile-time string constants in typed registries. |

### Primary hypothesis (Contract as Evidence) — survives with refinements

The Phase 3 primitive shape is now sharper than the plan doc anticipated:

1. **A Contract must be identified by `(topic, command-type)` pair**, not just topic. RWATP funnels 20+ command types through a single topic (`notify`); per-topic granularity would collapse `ManualPaymentApproved` and `ShareTransferred` into one contract.
2. **Two publisher-side extractors, not one:**
   - Typed API pattern: `publishCommand(Topics.X, CommandTypes.Y, …)` — RWATP style.
   - Plain string literal pattern: `pubsub.publish("some.topic", …)` — VestaScan style.
3. **Two subscriber-side extractors, not one:**
   - Typed dispatch pattern: `.on(CommandTypes.X, …)` in a fluent handler chain.
   - Generic pattern: `.subscribe("some.topic", handler)`.
4. **Documentary contract-bridge is a complementary primitive.** PR/Issue bodies contain the same string literals; when the investigation anchor set doesn't lexically reach the publish site (as in the Notifier targeted query), a documentary bridge would still connect the anchor to the file via PR text.
5. **Parallel-type observation is a distinct candidate primitive.** When two repos declare structurally identical interfaces under the same directory prefix (`contracts/commands/notify/…`), that is evidence of a shared contract even without a shared package. Worth separate benchmark evidence before earning.

**Regression?** N.

---

## Additional findings (not scoped to Phase 2 — recorded so they aren't lost)

### AF-1: `atlas investigate` does not consume documentary matches for cross-repo bridging

Even with 87 PRs ingested and clear documentary matches for anchors like `publishCommand`, the investigation output has no `DOCUMENTARY` section and no `CONCEPT EXPANSIONS`. The `documentary` field on `InvestigationDocument` is empty for every RWATP query run today. The path from documentary evidence → candidate ranking exists in IR but is not populated by `investigate()` in these anchor conditions. This is a Phase 1 investigation-pipeline finding, not a Phase 3 primitive. Worth writing a small benchmark to characterise before touching.

### AF-2: `atlas investigate` COVERAGE panel misreports GitHub state

Post-GitHub investigate output still displays:
```
Documentary      [--]  GitHub not ingested
```
even though the same DB has 78 PRs ingested and `atlas search publishCommand` returns 8 documentary matches from those PRs. The `InvestigationCoverage.github_prs` boolean is being computed incorrectly (or from a stale source). Deterministic reproducer.

### AF-3: Schema drift between Core and Notifier `CommandTypes`

Confirmed via source read in the ground-truth phase; also visible in Atlas output. Notifier's `notify.handler.ts` `CALLS_INSTANCE` list shows handlers for `OrderCreated/Cancelled/Expired` and `SignatureCompleted/Declined/Voided` that Core's `contracts/commands/notify/constants.ts` on main does not declare. User confirmed these are in unmerged PRs. This suggests a future primitive: **branch-aware contract observation** or **contract-drift detection between parallel copies**. Not fixing here — logging so it isn't lost.

### AF-4: Phase 1 field-use finding — inspector overloads EntryPoint kind

The `atlas project census` output surfaces every `npm script` name as an `EntryPoint` claim, producing a 40+ item dump for Core. `main` and `scripts:*` should be separate `ProfileClaim` kinds. Non-blocking Phase 2, but a real refinement to log against the maturity ladder's Stage 1 "missing 10%."

### AF-5: Notifier targeted query returns "No candidates found — run `atlas ingest .` first" even after ingest

The message text is misleading. The zero-result was because Notifier's file paths contain no substring of "manual", "payment", or "approved" — the vocabulary lives in method names (`notifyManualPaymentApproved`) and typed constants. The error message points the user to re-ingest, which is wrong advice. A more honest failure message would be: "0 candidates matched anchors [manual, payment, approved] in this repository's file paths; the code may reference these terms only in identifiers not yet observable by Atlas." Small UX/honesty improvement.


---

## Classification

*(to be filled after Atlas runs)*

| Dimension | Result |
|-----------|--------|
| Overall (per-repo)   | TBD |
| Overall (cross-repo) | TBD |
| Commands needed      | TBD |
| Source reads needed  | TBD (baseline: 9) |
| Repositories traversed by Atlas | TBD |
| Contracts observed as anchors   | TBD |

---

## Outcomes

*(to be filled after Atlas runs)*

**Contributes to which gap?** `cross-repo-contracts` (N=1 → N=2 upon sign-off).

**Falsification check.** The trace above satisfies the working hypothesis (Contract as Evidence, string-literal extraction on publish + subscribe sides). Predicted outcomes:
- Falsification 1 (lexical): not expected — topic string `"notify"` and command-type string `"notify.manual-payment-approved"` are byte-identical on both sides.
- Falsification 2 (coverage): not expected — all files listed are in `src/`, no excluded directories.
- Falsification 3 (documentary): not applicable — flow is code-only.
- Falsification 4 (structural — shared type def): **partial** — the payload type is declared in both repos, not imported from a shared package. If Atlas surfaces the parallel `interface ManualPaymentApprovedPayload` definitions via some shared-model observation, the primitive shape narrows.
- Falsification 5 (runtime-only): not applicable — topic name is a string constant.

---

## Notes

**Additional finding: cross-repo schema drift (worth its own future benchmark).**

While tracing this flow I noticed that rwatp-notifier's `src/contracts/commands/notify/constants.ts` declares `CommandTypes` that rwatp-core's registry does **not** currently declare on its main branch:

- `SignatureCompleted`, `SignatureDeclined`, `SignatureVoided`
- `OrderCreated`, `OrderCancelled`, `OrderExpired`

The `notify.handler.ts` in Notifier is already wired to handle these, but Core does not publish them. User confirmed these are in PRs raised against each repo but not yet merged to base branches. This means at any moment the shape of `CommandTypes` between Core and Notifier can drift — and Atlas, which ingests only merged branch state, cannot see the drift-in-progress.

This is a distinct observation from the current benchmark and could motivate a future primitive: *branch-aware contract observation* or *contract-drift detection between parallel copies*. Not fixing it here — logging it so it isn't lost.

**Additional finding: Phase 1 field-use issue.**

The `atlas project census` output surfaced every npm script name as an `EntryPoint` claim, producing a 40+ item dump. The inspector should distinguish `EntryPoint` (from `main`) from a new `Script` kind. Recorded as a Phase 1 refinement — not blocking Phase 2.
