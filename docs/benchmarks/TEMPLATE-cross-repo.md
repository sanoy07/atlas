---
title: Atlas Cross-Repository Evaluation Benchmark
date: YYYY-MM-DD
project: <project-name>                # e.g. "rwatp", "vestascan"
repositories: <comma-separated names>  # repos participating in this transaction
issue: <issue number or description>
status: Draft | Complete
# ── Gap declarations ─────────────────────────────────────────────────────────
# For cross-repo benchmarks the gap class is typically the same one first
# recorded by 2026-07-14-vestascan-notification-pubsub.md.  Reuse gap_id when
# extending an existing class so `atlas campaign next` increments N correctly.
#
# gap_id: cross-repo-contracts
# gap_classification: cross-repository
# gap_description: <one sentence — what Atlas could not see and why>
# gap_implementation: <one sentence — what would close it>
# gap_success: <one sentence — what Optimal-cross-repo would look like>
# gap_threshold: 3
---

# Benchmark: <short title>

## Product context

Project: <project-name>
Architecture summary (one line per repo):
- <repo-a> — <language/framework/role>
- <repo-b> — <language/framework/role>
- <repo-c> — <language/framework/role>

Atlas ingested (per repo): (git-only | git+github | git+github+typescript)

**Note:** with Phase 1's project layer, ingestion happens through
`atlas project ingest <project>` and all repos share one DB.  Cross-repo
benchmarks should always use one shared DB — running per-repo DBs defeats
the purpose.

## Question

What engineering question spans multiple repositories?

> e.g., "How does the buy-tokens transaction move from the frontend through
> Core to the blockchain and back to the notifier?"

## Ground Truth

Written **before** running Atlas — this is the earned-failure requirement per
[[feedback-understand-failure-first]].  Determined-after entries invalidate
this benchmark for the purposes of earning Phase 3 primitives.

### Participating repositories and their roles

| Repo | Role in this transaction |
|------|--------------------------|
| <repo-a> | <e.g. "GraphQL mutation entrypoint"> |
| <repo-b> | <e.g. "Publishes investment.created"> |
| <repo-c> | <e.g. "Consumes investment.created, sends email"> |

### The hop-by-hop trace

Number every hop.  A hop is a transition of control or data from one
observable location to another — a function call, a network round trip, a
pub/sub topic, a database write.  Each hop names the *specific* file and
symbol on each side.

1. **<repo-a>** `path/to/entrypoint.ts` `handlerName` — <what happens here>
2. → **<repo-a>** `path/to/service.ts` `ServiceClass.method` — <what happens>
3. → **[contract]** `<contract identifier>` — e.g. `pubsub:investment.created`
4. → **<repo-b>** `path/to/subscriber.ts` `onInvestmentCreated` — <what happens>
5. → **<repo-b>** … (continue)
6. → **[terminal]** <e.g. "email sent via SendGrid API">

### Contracts crossed

List every observable boundary between repos.

| # | Contract kind | Identifier | Producer (repo, file, symbol) | Consumer (repo, file, symbol) |
|---|---------------|------------|-------------------------------|-------------------------------|
| 1 | pubsub-topic  | investment.created | rwatp-core, `src/services/order.service.ts`, `publishCreated` | rwatp-blockchain, `src/subscribers/investment.subscriber.ts`, `onCreated` |
| 2 | graphql-op    | mutation buyTokens | rwatp-fe, `apps/web/src/graphql/mutations.ts` | rwatp-core, `src/modules/tokens/tokens.resolver.ts`, `buyTokens` |

### What a human engineer had to read to solve this

List files that a human read to reconstruct the trace, along with *why*.
This is the ground-truth attack surface Atlas must eventually shrink.

- `<repo>/<path>` — <reason a human needed to open this>
- ...

### Terminal outcome

What is the visible end-state of the transaction?  (email sent, DB row
written, blockchain event, frontend refresh, …)

---

## Atlas Evaluation

### Commands used (in order)

```bash
# Setup (once)
atlas project init <project-name>
atlas project register <project-name> <path/to/repo-a> --name <a>
atlas project register <project-name> <path/to/repo-b> --name <b>
atlas project ingest  <project-name> --typescript
atlas project census  <project-name>            # <-- Phase 1 sanity check

# Investigation attempts
atlas investigate <anchors>                      # from repo-a
atlas investigate <anchors>                      # from repo-b
atlas search <anchors>                           # cross-corpus text search
```

### What Atlas produced, per repository

**<repo-a>:**
- Candidates surfaced: ...
- Correctly identified as central: ...
- Missing: ...

**<repo-b>:**
- Candidates surfaced: ...
- Correctly identified as central: ...
- Missing: ...

### The cross-repo gap

Atlas surfaces both sides independently.  It does NOT connect them.
Specifically, Atlas cannot answer:

- <question that requires the contract edge, e.g. "who consumes investment.created?">
- <question that requires the traversal, e.g. "which repos are on the path from buyTokens to the email being sent?">

### Manual source reads required

List every file a human still had to open, even after running Atlas:

- `<repo>/<path>` — reason: Atlas did not connect ...

### False positives

| Query | Unexpected match | Reason | Severity |
|-------|-----------------|--------|----------|

### Useful observations

Per-repo signal that was correct and helpful (even if the cross-repo
connection remained invisible).

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall (per-repo)   | Optimal / Improved / Blocked |
| Overall (cross-repo) | Optimal / Improved / Blocked |
| Commands needed      | N |
| Source reads needed  | N (of which N are cross-repo joins Atlas cannot make) |
| Repositories traversed by Atlas | 0 / X / all |
| Contracts observed as anchors   | 0 / X / all |

**Cross-repo Optimal**   — Atlas reconstructed the hop-by-hop trace itself.
**Cross-repo Improved**  — Atlas surfaced correct per-repo signal but required
manual joins to connect them.
**Cross-repo Blocked**   — Atlas provided no signal on either side.

---

## Outcomes

**Decision produced?** (Y/N — link if yes)

**Contributes to which gap?** (must reference an existing gap_id or declare a new one — reuse `cross-repo-contracts` when applicable so N counters aggregate correctly)

**New primitive earned by this case?** (Y/N — only Y if this case moves an
existing gap's N above its declared threshold)

**Falsification check.**  Did the failure characterize as expected under the
current hypothesis (contract-observation gap), or does the shape of the gap
suggest a different primitive?  (e.g. lexical mismatch → lexicon problem;
topic name buried in generated code → parser problem; producer file was in
`.gitignore` → coverage problem.)  If falsification: describe.

**Regression?** (Y/N)

---

## Notes

Anything that doesn't fit the above — architectural surprises, unexpected
Atlas behaviours, edge cases in the cross-repo trace, dead ends.
