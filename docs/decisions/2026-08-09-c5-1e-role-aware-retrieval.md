---
title: C5.1-E — Role-aware / entrypoint-aware retrieval
date: 2026-08-09
status: Implemented
---

## Problem

VestaScan suite scored **66.7%** after C5.1-R+L while RWATP was **100%**.

Failures were not “need more keywords” but **role confusion**:

- marketing `deploy-*` loaders vs `deployment.service`
- constants/satellites vs primary services
- “production **deployment** + **secret** changed” hijacked to token deploy

Hardcoding `order.service` / `deployment.service` / `secret-manager` would not generalize.

## Decision

**C5.1-E** — infer artifact **role** and **primacy** from structure + bag statistics:

1. **InferredRole** from path shape only: Entrypoint / Implementation / Model / Config / Satellite / Test  
2. **Bag-relative IDF** over query tokens (discriminative concepts win)  
3. **Multi-concept coverage** + demotion when path only matches common tokens while missing distinctive ones  
4. **Co-occurrence rule**: if question has `secret` and path is deploy* without secret → demote (disambiguate English “deployment”)  
5. **Concept search fragments** from question tokens/bigrams (not hardcoded service basenames)  
6. **Structural fan-in** (non-import edges) as soft primacy signal  

PageRank affinity uses the same roles (no RWATP service name list).

## Explicit non-goals

- C5.2 multi-hop structural traversal (next, if secret load/consume still weak)  
- Qwen expansion  
- Embeddings  
- VestaScan-specific path tables  

## Validated outcome

| Suite | Before C5.1-E | After |
|-------|---------------|-------|
| RWATP | 9/9 | **9/9** (no regression) |
| VestaScan | 66.7% (10/15) | **73.3% (11/15)** |
| VestaScan gate ≥70% | FAIL | **PASS** |
| data_rooms | 4/4 | 4/4 |
| secret_management | 3/4 | **4/4** |
| token_deployment | 2/4 | **3/4** |
| adversarial | 1/3 | 0/3 (harder; role still incomplete for multi-hop) |

## Remaining (earned C5.2 candidates)

- Adversarial secret-after-deploy: bag can still miss secret-manager under long wording  
- Token “how deployed” overview still under-ranks `deployment.service` sometimes  
- Multi-hop load→consume chains  

## Future

C5.2 structural multi-hop when question implies flow/dependency, after this role layer is stable.
