# VestaScan API — Benchmarks

## Benchmark 1: Payment/Subscription Architecture

Question: How does Stripe billing integrate with subscription management?
Commands: `atlas investigate stripe subscription payment`
Manual source reads: none
Wrong branches: none (docs/commands/expire-subscriptions.md correctly surfaced as documentation)
False positives: none
Noise removed: N/A (dist/ already excluded)
Hidden understanding revealed: WebhookEvent idempotency model — Atlas surfaced all three operations (findOne→create→findByIdAndUpdate) showing the dedup pattern
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: expire-subscriptions is a command-pattern handler (contracts/commands/ directory), suggesting a broader command bus architecture

---

## Benchmark 2: KYC/Compliance Flow

Question: How does KYC identity verification work?
Commands: `atlas investigate kyc identity verification`
Manual source reads: none (surfaced enough for architecture understanding)
Wrong branches: file anchor pulled in kyc-profile.model.ts as "profile" match — useful
False positives: none significant
Hidden understanding revealed: lib/privyIdentity.ts — wallet-based identity alongside Firebase; Sumsub referenced in constants
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: Privy (wallet identity) alongside Firebase (email identity) — dual identity system

---

## Benchmark 3: Authentication Architecture

Question: How does authentication and authorization work?
Commands: (1) `atlas investigate authentication authorization middleware`, (2) `atlas investigate auth guard permission role`
Manual source reads: 0 (but answer is incomplete)
Wrong branches: First investigation returned only 2 isolated middleware files — entirely wrong path
False positives: "authentication" → middleware files only | conceptual mismatch | HIGH
Hidden understanding revealed: Second attempt surfaced per-module permissions.ts pattern and Firebase auth
Classification: Blocked (first attempt) → Improved (second attempt)
Source reads needed: 1 minimum (to understand how permissions.ts files are wired)
New primitive earned: Y (N=1)
  Gap: GraphQL authorization via exported policy objects is invisible to Atlas. permissions.ts files have zero structural edges because they're consumed at server configuration time (Apollo Server plugins), not via CALLS_STATIC. This is the "configuration-time wiring" gap — a pattern where module exports are consumed by a framework's registration mechanism, not direct imports.
Unexpected discoveries: The first investigation revealing only middleware confirms this is NOT an Express middleware-based auth system — that's a correct negative finding.

---

## Benchmark 4: Data Room Investor Access

Question: How do investors access data room files?
Commands: `atlas investigate data room investor access file`
Manual source reads: 0
Wrong branches: "file" pulled in Dockerfile and kyc-profile.model.ts as noise
False positives: "file" → Dockerfile | path substring | Low
Hidden understanding revealed: interest-request.service.ts gates data room access (CALLS_STATIC to data-room-file-access.service.ts) — investors must file an interest request first
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: AI tool-executor.service.ts reads data room files — the Anthropic assistant can answer investor questions about data room contents

---

## Benchmark 5: Error Handling Infrastructure

Question: How does the API handle and surface errors?
Commands: `atlas search error handling`
Manual source reads: 0
Wrong branches: none
False positives: none
Hidden understanding revealed: Commit d74c6ce message confirmed auth errors return unauthenticated context (not thrown) — historical evidence clarified the design intent
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: graphql/errorFormatter.ts exists as a dedicated file — errors are formatted at the GraphQL boundary, not in individual resolvers
