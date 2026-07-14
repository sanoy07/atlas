---
title: VestaScan Notification/PubSub Flow (cross-repo)
date: 2026-07-14
repository: vestascan-api + vestascan-notifier
issue: dogfood-03
status: Complete
gap_0_id: repository-awareness
gap_0_classification: context
gap_0_description: dist/ files appear in historical co-change evidence alongside source files — same artifact pollution as investigations 01 and 02; this occurrence confirms N=3
gap_0_threshold: 3
gap_1_id: cross-repo-contracts
gap_1_classification: cross-repository
gap_1_description: PubSub topic names connecting vestascan-api publisher to vestascan-notifier subscriber are invisible to static analysis — Atlas surfaces both sides but cannot connect them across repositories
gap_1_implementation: Extract PubSub topic string literals from publisher and subscriber files; cross-reference topic names across all repos in the same ATLAS_DB; surface as PUBLISHES_TO / SUBSCRIBES_FROM structural edges
gap_1_success: Cross-repo notification flow investigation reaches Optimal without source reads; topic ownership visible in atlas investigate output
gap_1_threshold: 3
---

# Benchmark: Notification/PubSub Flow (VestaScan)

## Repository

Name: vestascan-api (publish side) + vestascan-notifier (subscribe side)  
Architecture: API — Express/Apollo/Mongoose; Notifier — standalone Node.js/Mongoose/Nodemailer  
Size: vestascan-api 89 commits, vestascan-notifier 14 commits  
Atlas ingested: both repos git+typescript (development branch, same DB)

## Question

How do notifications flow from vestascan-api to vestascan-notifier? What events exist and who handles them?

## Ground Truth

Determined after:  
- Publish side: `indexer-trigger.service.ts` + `lib/commands/publisher.ts` → `PubSubFactory.getProvider` → Google PubSub  
- Subscribe side: `notify.handler.ts` dispatches 29 typed notification methods to `notification.service.ts` → `EmailProviderFactory.getProvider` → email delivery  
- 29 event types confirmed in `notify.handler.ts` spanning user, KYC, token, data room, support, and billing events  

---

## Atlas Evaluation

### Commands used (in order)

```
# Run from vestascan-api directory:
atlas investigate notification pubsub

# Run from vestascan-notifier directory:
atlas investigate notification pubsub subscription
```

### Manual source reads required

- `notification-message.model.ts` — read to confirm structural isolation was correct (it is: the model logs delivery attempts but is not imported by notification.service.ts — only by send-pubsub-http.ts test script and co-changes). Not a gap — this is correct.

### Wrong branches followed

None. Both investigations were clean — no false positives.

### Useful observations

**vestascan-api investigation:**
- PubSub infrastructure cleanly surfaced: `pubsub/connection.ts`, `google-pubsub.adapter.ts`, `pubsub.interface.ts`, `pubsub.factory.ts`, `pubsub/index.ts`
- Two publishers confirmed: `indexer-trigger.service.ts` (core domain events) and `lib/commands/publisher.ts` (command-pattern entrypoint)
- `notification-gate.ts` correctly surfaced as structural neighbor via `CacheManager.getInstance` — the notification gate checks cache before allowing notifications
- `pubsub-auth.middleware.ts` correctly flagged as STRUCTURALLY ISOLATED — it's the inbound PubSub push authenticator, not connected to the publisher graph

**vestascan-notifier investigation:**
- Complete event dispatch map surfaced: 29 `CALLS_INSTANCE` edges from `notify.handler.ts` to `notification.service.ts` with full method names
- `EmailProviderFactory.getProvider` + `EmailProviderFactory.getActiveProviderName` surfaced — dual-provider email infrastructure
- `docs/notification-events.md` surfaced as documentation artifact
- `test/pubsub-publish-commands.sh` surfaced in historical evidence — test tooling for event simulation
- `notification-message.model.ts` correctly isolated — it's the delivery log, not in the notification send path
- `template.service.ts` correctly surfaces as neighbor via `NotificationTemplate` model reference

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall | Optimal (per-repo) / Improved (cross-repo) |
| Commands needed | 2 (one per repo) |
| Source reads needed | 0 (signal) / 1 (isolation confirmation) |
| Confidence at completion | High |

**Optimal** within each repository — Atlas answered "what handles notifications?" and "what events exist?" completely without manual source reads.  
**Improved** cross-repo — Atlas cannot show that `indexer-trigger.service.ts` (publisher) sends events that `notify.handler.ts` (subscriber) receives. That relationship is only visible at the infrastructure level (Google PubSub topic names), which Atlas has no evidence for.

---

## Outcomes

**Decision produced?** N (observation only)

**New primitive required?** Y  
Gap (new): Cross-repo relationship inference. The PubSub bridge between vestascan-api and vestascan-notifier is invisible to Atlas because it exists at runtime infrastructure, not in code. The topic names that connect publisher to subscriber are string literals in config, not structural edges. This is a new primitive class: **event contract awareness**. The gap is: Atlas can surface "this file publishes to PubSub" and "this file subscribes from PubSub" but cannot connect them.  
Note: This is an architectural gap, not an implementation gap. It requires either (a) topic-name extraction and matching across repos, or (b) explicit contract files (e.g., a shared event schema). Not earned yet — one occurrence.

**New abstraction earned?** Y (dist/ exclusion: N=3 confirmed across vestascan investigations 01, 02, and 03 — dist/ files appear in historical co-changes here too)

**Regression?** N

---

## Notes

The vestascan-notifier investigation produced the most complete picture of any single investigation in the VestaScan dogfood. 29 named event types, full handler dispatch map, email provider factory, template lookup — all in one command. This is the strongest Optimal classification earned so far.

The `notification-message.model.ts` isolation finding is worth keeping: a model that IS a Mongoose schema definition but is only used externally (not referenced by the service that writes notifications) appears correctly as STRUCTURALLY ISOLATED. This is accurate behavior — the model's consumers are admin tooling, not the core service.

The 29-method dispatch list in `notify.handler.ts` is Atlas's first complete "event taxonomy" surfacing — a useful artifact for onboarding or compliance review. It required zero manual work.

**dist/ abstraction earned (N=3):** All three VestaScan investigations encountered dist/ noise. The primitive is now earned: Atlas should support excluding build artifact directories during ingest. Implementation options: parse `.gitignore`, explicit `--exclude-path` flag, or hardcoded common patterns (`dist/`, `build/`, `out/`). Recommend `.gitignore`-aware as the most composable approach.
