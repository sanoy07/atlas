# Token deployment ground truth (VestaScan)

## Split of responsibility

```text
User (browser)
    │
    ▼
vestascan-user-fe
  DeploymentWizard / DeployWalletSelector / …
  → wallet transaction (on-chain contract create)
  → receipt.contractAddress + deploymentTxHash
    │
    │  GraphQL recordDeployment
    ▼
vestascan-api
  resolvers → DeploymentService.recordERC1404Deployment
  → validate metadata (deployment.schema)
  → persist smart contract / token association
  → deployer admin association, cache invalidation, TokenDeployed command
```

## API files (post-deploy record + token domain)

| Path | Role |
|------|------|
| `src/modules/core/services/deployment.service.ts` | Record ERC-1404 deployment after chain success |
| `src/schemas/deployment.schema.ts` | Metadata validation |
| `src/modules/core/graphql/resolvers.ts` | `recordDeployment` mutation wiring |
| `src/modules/core/services/token.service.ts` | Token queries, deployer checks, caches |
| `src/modules/core/models/token.model.ts` | Persistence |

## FE files (actual deploy UX + chain write)

| Path | Role |
|------|------|
| `src/components/deploy/DeploymentWizard.tsx` | Multi-step deploy + `RecordDeployment` mutation after receipt |
| `src/components/deploy/DeployWalletSelector.tsx` | Deploying wallet / admin |
| `src/components/deploy/DeploymentSteps.tsx` | Step UI |
| `src/app/(main)/dashboard/deploy/page.tsx` | Route entry |
| `src/hooks/useDeployerWallets.ts` | Wallet selection |
| `src/schemas/deployment.schema.ts` | Shared schema shape |

## Atlas evaluation consequences

1. **Single-repo api suite** should score “token deploy” as **backend record path**, not full product deploy.
2. Missing FE paths is **correct** for api-only DB — not a retrieval bug.
3. Marketing `deploy-count` / `tokens-by-deployer` loaders are **satellites** of “deploy” English, not primary deploy implementation.
4. **Cross-repo** `atlas project` / multi-repo investigation is the right long-term product surface for “how are tokens deployed end-to-end?”
