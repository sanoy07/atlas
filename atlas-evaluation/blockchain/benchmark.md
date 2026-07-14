# VestaScan Blockchain — Benchmarks

## Benchmark 6: Blockchain Indexer Architecture

Question: What does vestascan-blockchain do and how is it structured?
Commands: (1) `atlas hot-files --limit 10`, (2) `atlas investigate deploy contract erc1404`, (3) `atlas investigate indexer event sync`, (4) `atlas investigate reconcile blockchain transaction`
Manual source reads: 0
Wrong branches: (1) returned `docs/deployment.md` + `ERC1404.abi.v1.ts` (useful but incomplete); (3) gave complete architecture
False positives: none
Noise removed: All files changed exactly 2× — reveals brand-new repo (no hot-file signal)
Hidden understanding revealed: shared/token.model.ts — blockchain service reads Token records from shared MongoDB; user-resolver.service.ts maps wallet addresses to users; reconciliation as first-class operation
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: IndexerCursor (block position tracking), IndexerRun (audit log), three route types (index/reconcile/snapshot) — full operational toolkit for a production indexer

---

## Benchmark 7: Cross-Repo Data Flow (vestascan-api → vestascan-blockchain)

Question: How does vestascan-api trigger vestascan-blockchain and how does blockchain state flow back?
Commands: `atlas investigate indexer event sync` (blockchain dir), `atlas investigate token deployment` (api dir)
Manual source reads: 0 (but answer is incomplete)
Wrong branches: none
False positives: none
Hidden understanding revealed: vestascan-api has indexer-trigger.service.ts; vestascan-blockchain reads shared Token model from MongoDB
Classification: Blocked (cross-repo channel not determinable)
Source reads needed: 2+ (indexer-trigger.service.ts to see PubSub topic; blockchain index.route.ts to see how it receives trigger)
New primitive earned: Y (N=2 — same gap seen in notification/PubSub cross-repo investigation)
  Gap: Cross-service event contracts. Atlas surfaces "this service publishes" and "this service subscribes/is triggered" but cannot connect them. The connection lives in topic name string literals or HTTP endpoint URLs, not in import/call structure. This is now confirmed across two different cross-repo communication patterns (PubSub api→notifier, and trigger api→blockchain).
Unexpected discoveries: The shared MongoDB model pattern (shared/token.model.ts in blockchain, token.model.ts in api) suggests the two services share a database rather than communicating via API. This is an architectural observation not visible from either repo alone.
