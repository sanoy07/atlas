# VestaScan API — Atlas Commands Log

All commands run from `/home/sanoy/Vesta/vestascan-api` with `ATLAS_DB=.../vestascan-eval.db`.

| # | Command | Reason | New evidence produced |
|---|---------|--------|----------------------|
| 1 | `atlas hot-files --limit 15` | Identify most active files to understand development focus | resolvers.ts (20×), token.service.ts (16×), server.ts (15×), typeDefs.ts (12×) |
| 2 | `atlas investigate stripe subscription payment` | Map billing/payment architecture | Full payment factory, webhook idempotency (WebhookEvent), expire-subscriptions command, PaymentSession lifecycle |
| 3 | `atlas investigate kyc identity verification` | Map compliance/KYC flow | compliance module, KYC profile model, org-verification, Privy identity, sumsub constants |
| 4 | `atlas investigate authentication authorization middleware` | Find auth layer | Only 2 middleware files, both isolated — auth is not middleware-based |
| 5 | `atlas investigate auth guard permission role` | Retry auth with different anchors | Firebase auth, per-module permissions.ts (8 files), auth.ts (JWT), auth.router.ts → UserService.getOrCreateUser |
| 6 | `atlas investigate data room investor access file` | Map investor data room flow | data-room-file.service.ts + data-room-file-access.service.ts, thumbnail.service.ts, AI tool-executor calls data-room-file |
| 7 | `atlas structural src/lib/auth.ts` | Understand auth.ts dependencies | Imports jsonwebtoken + secrets.ts + walletType.ts — JWT signing util only |
| 8 | `atlas structural src/modules/core/graphql/resolvers.ts --reverse` | Map resolver API surface | 9 CALLS_STATIC: DeploymentService.recordERC1404Deployment, BlockchainActionService, TokenVerificationService (7 methods) |
| 9 | `atlas search error handling` | Find error infrastructure | src/common/errors/ (createError, errorCodes), graphql/errorFormatter.ts, errorParser.ts |
