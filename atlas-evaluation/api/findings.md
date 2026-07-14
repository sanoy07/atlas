# VestaScan API — Engineering Findings

## Architecture (OBSERVED)

GraphQL-first Express API. Modules: core (token lifecycle), compliance (KYC), identity (user/auth), plan (subscriptions/quota), payment (Stripe), ai (Anthropic SDK), support.

Provider factory pattern used consistently: PaymentFactory→stripe, AIProviderFactory→anthropic, PubSubFactory→google-pubsub, CacheManager (singleton).

Each module: graphql/resolvers.ts + graphql/typeDefs.ts + graphql/permissions.ts + services/*.service.ts.

## Payment/Subscription (OBSERVED)

plan/resolvers.ts → PaymentService.createCheckoutSession → infrastructure/payment/index.ts → stripe.adapter.ts

stripe.webhook.ts → WebhookEvent.findOne/create/findByIdAndUpdate (idempotency) → payment.service.ts

expire-subscriptions.handler.ts → subscription.service.ts → CacheManager.getInstance

subscription.model.ts (entity) + user-plan.model.ts (current entitlements) — two distinct models.

INFERRED: Stripe webhooks are authoritative for subscription state, not real-time mutations.

## Authentication (FAILURE)

`atlas investigate authentication authorization middleware` → only 2 isolated middleware files. Complete miss.

`atlas investigate auth guard permission role` → partially recovered: Firebase auth, per-module permissions.ts (8 files isolated), auth.ts (JWT util), auth.router.ts → UserService.getOrCreateUser.

FAILURE REASON: permissions.ts files have ZERO structural edges — they export policy functions consumed at GraphQL server configuration time (dynamic), not via CALLS_STATIC. Atlas cannot reconstruct the authorization model.

UNKNOWN: How permissions.ts files are wired into GraphQL server context.

## Data Room (OBSERVED)

data-room-file.service.ts (file management) + data-room-file-access.service.ts (access control).
interest-request.service.ts gates data room access (CALLS_STATIC neighbor).

UNEXPECTED: tool-executor.service.ts (AI module) calls data-room-file.service.ts — AI assistant reads data room files for investor queries.

## Error Handling (OBSERVED)

GraphQL-native: createError.ts + errorCodes.ts + graphql/errorFormatter.ts + errorParser.ts.
Historical: auth errors return unauthenticated context, not thrown (commit d74c6ce).

## Atlas Failures

1. permissions.ts isolation — 8 files, zero edges. Authorization model invisible.
2. "authentication" anchor mismatch — auth files not named that way.
3. Cross-repo trigger — indexer-trigger.service.ts→PubSub→vestascan-blockchain invisible.
4. Dynamic auth context injection invisible to static analysis.
