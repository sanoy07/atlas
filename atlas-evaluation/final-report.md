# VestaScan Atlas Evaluation — Final Report

Date: 2026-07-14  
Atlas version: Methodology v1 (with Repository Awareness)  
Repos evaluated: vestascan-api (89 commits), vestascan-notifier (14 commits), vestascan-blockchain (2 commits)  
Total investigations: 9  
Benchmarks: 9 formal entries  

---

## 1. Repository Summaries

### vestascan-api
ERC-1404 security token issuance platform. GraphQL-first Express API. Seven modules: core (token lifecycle), compliance (KYC/org verification), identity (user management), plan (subscriptions/quota), payment (Stripe), ai (Anthropic SDK assistant), support.

Provider factory pattern is the dominant architectural pattern: PaymentFactory, AIProviderFactory, PubSubFactory, EmailProviderFactory, CacheManager (singleton), StorageFactory. Every external dependency has a factory + adapter abstraction.

Most changed files: resolvers.ts (20×), token.service.ts (16×), server.ts (15×) — core token operations are the primary area of active development.

### vestascan-notifier
Dedicated notification microservice. Receives events via PubSub HTTP push, dispatches templated emails. Dual email providers (Nodemailer/SMTP + SendGrid). 29 named event types spanning user, KYC, token, data room, support, and billing events. Local HTML template rendering (not using SendGrid's template engine).

### vestascan-blockchain
Blockchain event indexer. Watches ERC-1404 contracts on multiple chains, indexes token events (transfers, whitelist changes) into MongoDB, resolves wallet addresses to user identities. Has reconciliation and snapshot capabilities. Brand-new service (2 commits, entire codebase in one shot).

---

## 2. Cross-Repo Architecture

OBSERVED:
- vestascan-api publishes events via PubSub (indexer-trigger.service.ts, lib/commands/publisher.ts)
- vestascan-notifier subscribes to PubSub and dispatches notifications
- vestascan-blockchain indexes blockchain events and writes to MongoDB

INFERRED (not Atlas-observable):
- vestascan-api triggers vestascan-blockchain via PubSub or HTTP
- vestascan-blockchain and vestascan-api likely share a MongoDB instance (shared/token.model.ts in blockchain references the same Token collection as vestascan-api)
- vestascan-blockchain writes IndexedTokenEvent/IndexedTransfer/IndexedWhitelistEvent back to MongoDB; vestascan-api reads them for cap table display

UNKNOWN:
- Exact PubSub topic names connecting services
- Whether blockchain→api communication is via shared MongoDB or via API calls
- How user-resolver.service.ts obtains user records (shared DB? API call?)

---

## 3. Atlas Capability Validation Table

| Capability | vestascan-api | vestascan-notifier | vestascan-blockchain | Status |
|-----------|--------------|-------------------|---------------------|--------|
| Repository Awareness (dist/ exclusion) | ✓ Validated | ✓ Validated | ✓ Validated | Validated |
| CALLS_STATIC edges | ✓ Validated | ✓ Validated | ✓ Validated | Validated |
| CALLS_INSTANCE edges | ~ Partial | ✓ Validated (29 methods) | ✓ Validated | Validated |
| REFERENCES_MODEL edges | ✓ Validated | ✓ Validated | ✓ Validated | Validated |
| IMPORTS edges | ✓ Validated | ✓ Validated | ✓ Validated | Validated |
| Hot-files accuracy | ✓ Validated | ✓ Validated | ~ (all 2× — new repo signal loss) | Validated |
| Peer observations | ✓ Validated (deployment.service) | ~ not tested | ? too few commits | Partially Validated |
| Engineering memory | ✓ (Atlas has 0 VestaScan decisions — absence correctly reported) | ✓ | ✓ | Validated |
| Investigation anchor matching | ✓ for compound paths | ✗ for conceptual terms (auth) | ✓ | Partially Validated |
| Cross-repo communication | ✗ Invisible | ✗ Invisible | ✗ Invisible | Invalidated |

---

## 4. Atlas Strengths

1. **Payment/billing architecture** — Optimal in one command. Full Stripe factory chain, webhook idempotency, subscription lifecycle surfaced with zero source reads.

2. **Event taxonomies** — Optimal for notification flow. 29 named CALLS_INSTANCE methods surfaced automatically. This is the strongest Atlas capability demonstrated in this evaluation.

3. **Provider factory patterns** — Atlas reliably surfaces factory → adapter chains (payment, AI, email, PubSub). These are structurally the most visible patterns.

4. **Unexpected discoveries** — AI tool-executor reading data room files; interest-request gating data room access; command bus pattern in expire-subscriptions. These were not queried for and were found via structural neighborhood expansion.

5. **REFERENCES_MODEL specificity** — Method names (e.g., `WebhookEvent.findOne`, `WebhookEvent.create`, `WebhookEvent.findByIdAndUpdate`) reveal operational patterns that file-level analysis would miss.

6. **Historical evidence quality** — Commit messages explained design decisions (auth context, error handling) that weren't in code comments.

---

## 5. Atlas Failures

### FAILURE 1: Configuration-time wiring (N=1, new)

**What failed:** All 8 `permissions.ts` files in vestascan-api have zero structural edges. Atlas cannot reconstruct the GraphQL authorization model.

**Why:** These files export policy functions consumed by Apollo Server's plugin/context mechanism at server startup — not via CALLS_STATIC at runtime. The consumer is `server.ts` at configuration time, which is not analyzable as a static call.

**Evidence needed:** A "configuration-time import" edge type — detecting that a module's exports are passed as constructor/configuration arguments rather than called statically.

**Severity:** HIGH — authorization is a critical system and Atlas cannot reconstruct it.

**Primitive earned:** N=1, not yet earned.

### FAILURE 2: Cross-repo event contracts (N=2, confirmed)

**What failed:** Atlas cannot connect vestascan-api's PubSub publishers to vestascan-notifier's handlers, or to vestascan-blockchain's trigger mechanism. Two separate cross-repo communication patterns are both invisible.

**Why:** The connections are PubSub topic name string literals in config files, not import/call structure. Atlas has no visibility into infrastructure-level contracts.

**Primitive earned:** N=2 (PubSub api→notifier + trigger api→blockchain). One more independent occurrence earns it.

### FAILURE 3: Conceptual anchor mismatch (N=1, confirmed)

**What failed:** `atlas investigate authentication authorization middleware` returned only 2 isolated middleware files (complete miss). Second attempt with `auth guard permission role` partially recovered.

**Why:** The VestaScan auth system uses Firebase/Privy for identity and per-module permissions.ts for authorization — none of which use the word "authentication" in their file paths. Atlas's anchor matching is path-substring-based; it cannot bridge conceptual terms to their architectural implementations.

**Severity:** MEDIUM — second anchor attempt recovered significant signal, but a developer unfamiliar with the codebase would likely give up after the first failed attempt.

**Primitive earned:** N=1, not yet earned.

### FAILURE 4: Short anchor false positives (N=2, confirmed)

**What failed:** "AI" matches "blockchain"/"chains" as substring. "file" matches "Dockerfile", "kyc-profile.model.ts". "data" is too broad.

**Why:** 2-3 character anchors match as substrings in file paths. Already identified in investigation 02 (AI context builder); confirmed again here.

**Primitive earned:** N=2 (ai→blockchain + file→Dockerfile). One more independent occurrence earns word-boundary enforcement for short anchors.

---

## 6. False Positive Analysis

| Anchor | Match | Reason | Severity | Investigation |
|--------|-------|--------|----------|---------------|
| "AI" | blockchain/chains/* | substring of "blockchain"/"chains" | Low | AI context builder (prior), auth (this) |
| "file" | Dockerfile | substring | Low | Data room |
| "file" | kyc-profile.model.ts | substring of "profile" | Low | Data room |
| "authentication" | middleware/* only | conceptual mismatch | HIGH | Auth investigation |
| "data" | compliance/types/normalized-data.types.ts | substring | Low | Data room |

---

## 7. New Primitives

### Cross-repo event contracts — CANDIDATE (N=2, needs N=3)

Same gap confirmed across two different cross-repo communication patterns:
1. vestascan-api PubSub publishers → vestascan-notifier handlers (PubSub topic names)
2. vestascan-api indexer-trigger.service.ts → vestascan-blockchain (trigger mechanism unknown)

One more independent occurrence across a different repo pair earns the primitive.

### Configuration-time wiring — CANDIDATE (N=1)

GraphQL permissions.ts pattern is invisible. Need to confirm in another GraphQL codebase (NestJS guards, similar Apollo Server pattern) before earning.

### Short anchor word-boundary — CANDIDATE (N=2)

"AI"→blockchain confirmed in two separate investigations. Next occurrence earns it.

---

## 8. Evidence vs. Context Observations

The three-dimensional model held cleanly across all three repos:

**Evidence** (CALLS_STATIC, REFERENCES_MODEL, IMPORTS, CALLS_INSTANCE) — all four types produced genuine signal. CALLS_INSTANCE proved most valuable for notification event discovery.

**Context** (Repository Awareness) — correctly excluded dist/ across all repos. No false exclusions observed.

**Methodology** — the benchmark template's "wrong branches followed" field captured the auth investigation failure correctly (first attempt was a valid wrong branch, not a bug).

---

## 9. Comparison with RWATP

| Dimension | RWATP | VestaScan |
|-----------|-------|-----------|
| Architecture | Express REST + Mongoose | GraphQL Apollo + Mongoose |
| Auth pattern | JWT middleware | Firebase + per-module permissions (invisible to Atlas) |
| Module structure | Route-based | Module-based with GraphQL schema per module |
| Cross-repo | None (single repo) | 3 repos via PubSub + shared MongoDB |
| AI integration | None | Full Anthropic SDK with context builder |
| Atlas coverage | High | High for intra-repo, zero for cross-repo |
| PEER OBSERVATIONS | 1 case validated | 1 case validated (10 peers, same pattern) |

The 50% peer observation threshold validated on both architectures (different module sizes, different service counts). No threshold adjustment earned.

---

## 10. Unexpected Discoveries

1. **AI assistant reads investor data room files** — tool-executor.service.ts in the AI module calls data-room-file.service.ts. This means the Anthropic assistant can answer investor questions about specific documents. Not obvious from module names.

2. **Dual identity system** — Firebase (email-based) + Privy (wallet-based) running simultaneously. VestaScan serves both traditional investors (email login) and crypto-native users (wallet login). Not queryable but emerged from lib/privyIdentity.ts appearing as an anchor match.

3. **Command bus pattern** — src/contracts/commands/ directory with expire-subscriptions command structure suggests a deliberate command bus pattern beyond just the one observed command. UNKNOWN: how many other commands exist.

4. **IndexerCursor for exactly-once semantics** — blockchain service tracks block height position. This is a sophisticated operational concern (preventing duplicate indexing) that Atlas surfaced without being asked.

5. **WebhookEvent idempotency** — the Stripe webhook handler reads WebhookEvent before creating it. This dedup pattern is only visible because Atlas surfaces the three separate REFERENCES_MODEL operations with their method names.

---

## Final Answer: What does reality know about VestaScan that Atlas cannot currently surface?

1. **The authorization model** — How permissions.ts files gate GraphQL resolver access is completely invisible. A developer cannot use Atlas to answer "who can call this mutation?" for any operation.

2. **Cross-repo event contracts** — How vestascan-api triggers vestascan-blockchain (topic name, HTTP endpoint) and how state flows back (shared MongoDB? API call?) is UNKNOWN. The three services form a system; Atlas sees three independent repos.

3. **The GraphQL context construction** — What gets injected into every resolver (auth state, user, organization) is invisible because context construction is a configuration-time concern.

4. **Configuration-time dependencies** — Any pattern where a module's exports are passed as constructor arguments or plugin configuration rather than called at runtime is invisible. This includes Apollo Server plugins, Mongoose middleware, Express router mounting.

5. **Infrastructure topology** — Which services share a MongoDB instance, what PubSub subscriptions exist, what environment-specific configuration governs provider selection (Nodemailer vs. SendGrid, which cache backend) — none of this is visible from static analysis of code structure.

These five gaps are structurally distinct from each other. Three of them (authorization wiring, cross-repo contracts, configuration-time deps) would require new evidence classes to address. The other two (infrastructure topology, provider selection) may be addressable through runtime trace ingestion or config file parsing — both deferred until earned.
