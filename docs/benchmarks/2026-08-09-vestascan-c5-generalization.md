---
title: VestaScan-API C5 generalization suite (vs RWATP)
date: 2026-08-09
repository: vestascan-api
status: Complete
---

## Question

Does C5.1-R + C5.1-L + C5.1 PageRank + C4 **generalize** beyond RWATP
orders/timeout vocabulary to VestaScan token deploy, data rooms, and secrets?

## Method

- Gold suite: `eval/localization/vestascan/` (15 cases, 4 clusters)
- Runner: `eval/localization/vestascan/run_suite.mjs`
- Mode: `atlas investigate "<q>" --no-ai` (deterministic retrieval/ranking first)
- Full JSON + human text: `/tmp/atlas-vestascan-suite/`
- Gate for cross-repo: **≥70%** case pass (looser than RWATP 80% on mature gold)

### Per-case scoring

| Dimension | Metric |
|-----------|--------|
| Retrieval | gold files in bag (core ∪ ranked files) |
| Ranking | top-5 gold hits; top-5 hard negatives; top-1 |
| Reasoning | deferred to Qwen pass (not in this deterministic run) |
| Verification | C4 policy present; claim statuses when AI run |

Pass = retrieval_ok (≥2 gold in bag, or min(2,|gold|)) AND ranking_ok (hard5=0 and (top5_gold≥min(3,|gold|) OR top1)).

## Results (deterministic)

| pass_rate | **66.7% (10/15)** |
| gate ≥70% | **FAIL** (barely under) |
| RWATP C5 suite | **100% (9/9)** after C5.1-R+L |

### By cluster

| Cluster | passed | avg top5 gold | avg hard5 |
|---------|--------|---------------|-----------|
| **data_rooms** | **4/4** | 3.0 | 0 |
| **secret_management** | **3/4** | 3.0 | 0 |
| **token_deployment** | **2/4** | 2.25 | 0.25 |
| **adversarial** | **1/3** | 1.67 | 0.33 |

### Full table

| id | pass | bag gold | top5 gold | hard5 | top1 | Notes |
|----|------|----------|-----------|-------|------|-------|
| token-deploy-how | N | 1 | 1 | 1 | Y | marketing deploy loaders noise; **deployment.service missing from bag** |
| token-deploy-trace | N | 2 | 1 | 0 | N | token.service present; deployment.service weak |
| token-deploy-fail | Y | 3 | 3 | 0 | Y | |
| token-deploy-modules | Y | 5 | 4 | 0 | Y | Best token case |
| dataroom-create | Y | 5 | 3 | 0 | N | Strong domain localization |
| dataroom-lifecycle | Y | 4 | 3 | 0 | Y | Excellent |
| dataroom-access | Y | 3 | 3 | 0 | Y | Access service on top |
| dataroom-investigate | Y | 4 | 3 | 0 | N | |
| secret-how | Y | 4 | 4 | 0 | Y | secret-manager stack clean |
| secret-load-consume | N | 1 | 1 | 0 | Y | Only secrets.ts; factory/adapter missing bag |
| secret-startup-missing | Y | 3 | 3 | 0 | N | |
| secret-centralized | Y | 4 | 4 | 0 | Y | |
| adv-token-intermittent | N | 3 | 2 | 0 | N | Constants drown primary; need ≥3 top5 |
| adv-dataroom-create-no-access | Y | 3 | 3 | 0 | Y | Access-first — good adversarial |
| adv-secret-change-failure | N | **0** | 0 | 1 | N | **"deployment" → deployment.service** hijack; secrets absent |

## Ground-truth correction: token deployment is dual-repo

Verified in source (2026-08-09):

| Layer | Where | Role |
|-------|--------|------|
| **On-chain deploy** | **`vestascan-user-fe`** | `DeploymentWizard` → wallet/`writeContract` → contract address + tx hash |
| **Record after deploy** | **`vestascan-api`** | GraphQL `recordDeployment` → `DeploymentService.recordERC1404Deployment` (“Day 0” persistence, admin association, caches) |
| **Token queries / admin** | **api** | `token.service`, models, permissions |

So questions like *“How are tokens deployed?”* against **api alone** are **partially mis-scoped**: Atlas cannot retrieve FE wizard code from the api DB. Weak `deployment.service` ranking is not pure failure—API gold is the **record path**, not the full deploy flow.

**Implication for grading**

- Treat token-deploy cluster on api as **post-deploy backend** evaluation.
- Full deploy flow needs a **second gold suite on vestascan-user-fe** (and eventually multi-repo project investigation).
- Adversarial “deployed successfully but later fails” correctly emphasizes **api token/verification** more than FE deploy wizard.

## Failure modes (generalization)

### 1. Vocabulary not ported from RWATP (retrieval)

RWATP had hand-tuned fragments (`order.service`, `AuthService`, `redis-rate-limiter`).
VestaScan uses **`deployment.service`**, **`data-room-file`**, **`secret-manager`**.

- “tokens **deployed**” often retrieves marketing `deploy-*` loaders + token.model, **not** `deployment.service.ts`.
- Adversarial “production **deployment** … secret was changed” ranks **token deployment.service**, not secret-manager.

This is **domain fragment coverage**, not PageRank math.

### 2. Multi-hop “load → consume” under-retrieval

`secret-load-consume`: `secrets.ts` tops, but factory/adapter/server not always bagged.
Retrieval is single-hop lexical/seed, not “who imports SecretManager”.

### 3. Ranking satellites vs entrypoints (partial)

Token intermittent: token.service + verification in bag, but constants files occupy top slots
(same class as order-history vs order.service before C5.1-L primary boost).

### 4. What generalizes well

| Pattern | VestaScan evidence |
|---------|-------------------|
| Path-aligned domains (`data-room`, `secret-manager`) | Near-perfect cluster scores |
| Access-control adversarial (create vs later access) | **PASS** — access service ranked first |
| C4-style “associate with top candidate” det hyp | Always SUPPORTED (not causal) |
| Supersession / verification policy | Present on all packets |

## Comparison to RWATP

| | RWATP C5 suite | VestaScan suite |
|--|----------------|-----------------|
| Cases | 9 (orders/auth/redis) | 15 (token/dataroom/secret/adv) |
| pass_rate | **100%** | **66.7%** |
| Hard-neg top5 | 0 | mostly 0; 2 cases with intrusion |
| Tuned domain fragments | order/auth/redis | **not** deploy/data-room/secret-manager |
| Conclusion | Optimized + gold-aligned | **Partial generalization**; path-shaped domains work; English “deploy/deployment” ambiguous |

**Verdict:** C5 is **not** only RWATP-overfit for all structure-aligned names (`data-room-*`, `secret-manager/*`). It **is** under-generalized for:

1. Ambiguous English (`deployment` = release vs token deploy)  
2. Repo-specific entrypoint names not in fragment tables  
3. Multi-stage “load vs consume” without import-graph hop  

## Reasoning / verification (this run)

- Deterministic only (`--no-ai`).  
- Det hyp is always “associates with top file” SUPPORTED — localization signal, **not** causal claim.  
- Full human + JSON outputs under `/tmp/atlas-vestascan-suite/` for Qwen grading.  
- Recommended AI pass: re-run `WITH_AI=1` on the three adversarial ids only.

### Disproof probe (for graders)

For each answer, require:

> “What evidence would prove this wrong?”

Examples:

- Token deploy: evidence that issuance is only in blockchain package, not core deployment.service  
- Data room access: evidence access is not gated by data-room-file-access  
- Secret change: evidence failure is only deploy path with secrets unchanged  

## Artifacts

```text
eval/localization/vestascan/          # gold + suite + runner
/tmp/atlas-vestascan-suite/json/      # full JSON packets
/tmp/atlas-vestascan-suite/human/     # full CLI text
/tmp/atlas-vestascan-suite/summary.json
/tmp/atlas-vestascan-suite/REPORT.md
```

Re-run:

```bash
ATLAS_BIN=./target/debug/atlas \
ATLAS_DB=~/.atlas/vestascan-api.db \
REPO=/home/sanoy/Vesta/vestascan-api \
node eval/localization/vestascan/run_suite.mjs

# Optional Qwen on adversarial:
WITH_AI=1 AI_CASES=vs-adv-token-intermittent,vs-adv-dataroom-create-no-access,vs-adv-secret-change-failure \
  node eval/localization/vestascan/run_suite.mjs
```

## Classification

| Overall | Improved on path-aligned domains; **blocked** as full cross-repo claim |
| C5 generalizes? | **Partially** — not a pure RWATP overfit, not yet general |
| Next earned | Repo-agnostic domain/entrypoint extraction OR structural hop for consumers; **not** more Qwen |
| Gold | Keep fixed; do not loosen for deploy/secret fails |

## Outcomes

- VestaScan suite established as second validation corpus  
- Data rooms + secrets (simple) ≈ RWATP quality  
- Token “deploy” ambiguity + adversarial secret-vs-deployment = next retrieval earned failures  
