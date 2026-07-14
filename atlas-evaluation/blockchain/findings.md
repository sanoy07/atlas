# VestaScan Blockchain — Engineering Findings

## Architecture (OBSERVED)

Standalone Node.js blockchain indexer service. NOT a smart contract deployment service.
Purpose: watches ERC-1404 token contracts on-chain, indexes events into MongoDB, resolves user identities.

Hot files all changed exactly 2x (2 commits) — brand-new repo, entire codebase introduced in one shot.

## Indexer Architecture (OBSERVED)

chain-indexer.service.ts — orchestrator
  → event-fetcher.service.ts (reads blockchain events)
  → event-processor.service.ts (writes to DB)
  → user-resolver.service.ts (maps wallet addresses to user IDs)

Three event models:
- IndexedTokenEvent — generic token events
- IndexedTransfer — ERC-1404 transfer events
- IndexedWhitelistEvent — whitelist add/remove events

IndexerCursor — tracks current indexing position (block height)
IndexerChainConfig — per-chain configuration (which contracts to watch)
IndexerRun — audit log of each indexer run

Routes: /index (trigger), /reconcile (manual resync), /snapshot

## Reconciliation (OBSERVED)

reconcile.route.ts → reconciliation.service.ts → IndexerCursor.findOneAndUpdate + Token.find

The reconciliation route re-syncs from a given block height. Reads from shared Token model (shared MongoDB with vestascan-api).

## UNEXPECTED: shared Token model

src/models/shared/token.model.ts — OBSERVED but significant.
The blockchain service reads Token records from the same MongoDB as vestascan-api.
This is the cross-repo data channel: vestascan-api writes Token records, vestascan-blockchain reads them to know which contracts to index.

INFERRED: vestascan-blockchain is triggered by vestascan-api (via PubSub or HTTP) and uses shared MongoDB to find which tokens to watch. It writes indexed events back to the same MongoDB so vestascan-api can query them.

## Atlas Failures

1. The "shared MongoDB" cross-repo relationship is only visible by reading model names — Atlas surfaces Token.find in vestascan-blockchain but cannot confirm it references the same collection as vestascan-api's token.model.ts. Cross-service data ownership is UNKNOWN.
2. user-resolver.service.ts resolves wallet addresses — but HOW it gets user data (API call? shared MongoDB users collection?) is not visible. UNKNOWN.
3. What triggers the indexer (PubSub message from vestascan-api? HTTP call? cron?) is invisible. UNKNOWN.
