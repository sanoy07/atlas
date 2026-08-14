---
title: C5.1-E role-aware retrieval — dual suite
date: 2026-08-09
repository: rwatp-core + vestascan-api
status: Complete
---

## Question

Does role-aware retrieval (no repo-specific service hardcodes) lift VestaScan
past the 70% cross-repo gate without regressing RWATP?

## Results

| Suite | pass_rate | Gate |
|-------|-----------|------|
| RWATP (9 cases) | **100% (9/9)** | PASS (≥80%) |
| VestaScan (15 cases) | **73.3% (11/15)** | **PASS (≥70%)** |

### VestaScan by cluster (post C5.1-E)

| Cluster | Result |
|---------|--------|
| data_rooms | 4/4 |
| secret_management | 4/4 |
| token_deployment | 3/4 |
| adversarial | 0/3 |

## Classification

| Overall | Cross-repo gate earned; adversarial still weak |
| C5.2 justified? | **Yes** for multi-hop / flow questions (secret consume, deploy fail halfway) |
| Qwen justified? | **Not yet** — retrieval still the limit on adversarial |

## Outcomes

- Decision: `docs/decisions/2026-08-09-c5-1e-role-aware-retrieval.md`  
- RWATP regression: none  
- Architecture: roles of extracted artifacts matter more than new LLM authority  
