---
title: rwatp-core investigate synthesis — share class listing
date: 2026-07-17
repository: rwatp-core
status: Complete
---

## Repository
rwatp-core — NestJS/GraphQL backend, TypeScript, MongoDB/Mongoose. Ingested with `--typescript --github`.

## Question
What does the share class / listing system do in rwatp-core?

## Ground Truth
rwatp-core has a share class module and a listing module. Share classes represent equity tranches for investment offerings. Listings are the public-facing investment products. The modules share a supply ledger for tracking token supply. Structural connections run through resolvers → services → models, and permissions are managed separately via permission files.

## Atlas Evaluation

### Commands used (in order)
1. `atlas investigate share class listing`

### Manual source reads required
0 — synthesis covered the domain without source reads.

### Wrong branches followed
0

### False positives
None on this query.

### Useful observations
- AI synthesis named the domain correctly (listings + share classes) from structural evidence alone
- Correctly identified MongoDB operations (findById, findOne, find) from REFERENCES_MODEL edges
- Correctly identified PR #63 (smart contract deployment) as most recent change
- Correctly flagged enum/type/typeDefs files as structurally unresolved (no edges observed)
- GAPS section accurately pointed at files in UNRESOLVED that have documentary backing but no structural connections
- Total wall-clock time: ~15s (includes Ollama inference)

## Classification
| Overall | Optimal |
| Commands needed | 1 |
| Source reads needed | 0 |
| Confidence | High |
| Noise removed | 17 candidates → 4-bullet synthesis |
| Hidden understanding revealed | Supply ledger connection, permissions isolation, PR #63 deployment context |

## Outcomes
- Decision produced? Yes — `docs/decisions/2026-07-17-local-ai-synthesis.md`
- New primitive earned? Yes — local AI synthesis layer for `atlas investigate`
- New abstraction earned? No — one concrete implementation, no abstraction extracted
- Regression? None — `--raw` and `--json` paths unchanged
- Unexpected discoveries? The GAPS section proved valuable: the model correctly identified that enums/types/typeDefs lack structural edges, which is a real gap (those files define vocabulary but have no runtime callers in the extracted graph)
