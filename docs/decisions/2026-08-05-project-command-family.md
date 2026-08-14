---
title: Project command family — establishing project-level awareness
date: 2026-08-05
status: Implemented
---

## Scope claim

This decision establishes **project-level observation only**. It intentionally
does not claim cross-repository understanding. Cross-repository understanding
requires observed *relationships* between repositories, which remain future work
to be earned through benchmark-driven primitives. In the maturity ladder
([[project-atlas-maturity-ladder]]), this change moves Atlas from Stage 0 toward
Stage 2 by making Stage 1 possible — it does not itself enter Stage 2.

## Problem

Atlas can investigate a single repository, but cannot investigate a *product* consisting
of multiple repositories. RWATP is five repositories (Core, Notifier, Blockchain,
Frontend, Admin Console) that communicate through GraphQL, REST, and Pub/Sub. Any
production bug that spans two of them (buy-tokens transaction, notification delivery,
blockchain settlement) is invisible to Atlas today because every table in `atlas-storage`
keys on a single `repo_path` string.

Concrete manifestations:

- No way to record that Core, Notifier, and Blockchain are one product.
- No way to run `atlas ingest` across a set of repositories with one command.
- No way to observe cross-repository facts, even the trivial ones (which repos in this
  project ingest MongoDB? which declare `@google-cloud/pubsub`?).
- Every prospective cross-repo primitive (contract extraction, semantic-slice traversal)
  needs a project-scoped identity to fan out from — without it, they cannot be built.

## Methodology validation

**Principle 1 (features earned by evidence).** The failure count is *every* cross-repo
investigation, forever. N ≥ 3 by RWATP alone (Core / Notifier / Blockchain, all
present in the working corpus). This is not speculative — it is a permanent structural
limitation of the current storage assumption `one repo_path = one system`.

**Principle 2 (abstractions earned by repetition).** **This change adds zero new
abstractions.** Every IR type it uses (`ProjectRecord`, `RepositoryRecord`,
`ProfileClaim`, `ProjectCensus`) was already scaffolded in `crates/ir/src/lib.rs`
during v0.7a. Every storage method it depends on (`create_project`,
`register_repository`, `replace_profile_claims`, …) was already implemented in
`crates/storage/src/lib.rs`. What was missing was the composition layer and the
observation surface.

**Principle 3 (knowledge accumulated).** This document; test suite grew from 39 to
44 blackbox tests and from ~22 to 27 core unit tests. `MEMORY.md` updated with three
new principles that emerged from tonight's conversation: [[project-atlas-one-principle]],
[[feedback-capabilities-emerge-not-declared]], [[feedback-observation-over-concept]].

**Principle 4 (validation before generalization).** Deliberately limited scope. This
change alone does **not** enable cross-repo investigation. It enables Phase 2 (a
real RWATP cross-repo bug benchmark) which will then earn the Contract primitive.

## Decision

Wire the dormant Project layer into a working end-to-end pipeline by adding:

1. **`crates/core/src/repo_inspector.rs`** — an observation surface. Deterministic
   scan of a repository root producing `Vec<ProfileClaim>` using only the existing
   `ProfileClaimKind` variants (Runtime, Language, EntryPoint, Framework, Persistence,
   Messaging, Auth, EmailProvider, TemplateEngine, BlockchainClient, Module,
   PackageManager) and existing `ClaimEvidence` variants. Reads: `package.json`
   (dependencies, `main`, scripts), lockfile presence, `src/` subdirectories,
   source-file extensions, `Cargo.toml`. Every claim carries specific evidence.
   No interpretation.

2. **`crates/core/src/project.rs`** — composition over the existing per-repo pipeline.
   Public functions: `create_project`, `register_repository_at_path` (idempotent;
   canonicalises path; detects git existence to set `ExistenceSource`/`AccessState`),
   `ingest_project` (fans out to `ingest_git` + rename evidence + identity rebuild +
   auto-detected structural extractors + documents + lexicon per repo; updates
   `IngestionState`), `build_project_census` (runs the inspector per accessible repo,
   persists observed claims, returns `ProjectCensus`).

3. **`apps/cli/src/commands/project.rs` + main.rs wiring** — CLI surface:
   - `atlas project init <name> [--description]`
   - `atlas project register <project> <path> [--role] [--name]`
   - `atlas project list [<project>]`
   - `atlas project ingest <project> [--typescript] [--github]`
   - `atlas project census <project> [--json]`

The change touches no IR types, no schema, no existing tests. Only additive files
plus enum + match-arm additions in `main.rs`.

## Alternatives considered

**Skip straight to Contract primitives.** Rejected. The Contract primitive needs to
be scoped to a project (a Pub/Sub topic named "investment.created" in RWATP is a
different observation from the same string in some unrelated codebase). Without a
project identity, cross-repo edges have nowhere to attach. The benchmark that would
earn contract extraction cannot be *written* until `atlas project census rwatp`
exists as ground-truth infrastructure.

**Rewrite storage to key on `repository_id` instead of `repo_path`.** Rejected as
premature. It would touch ~40 SQL queries and every call site in core, with no
observable improvement until the semantic-slice traversal is implemented. Additive
Project tables + `repositories.local_path` as the join column keep the invariant
"any repo_path from a registered repository joins to exactly one RepositoryRecord"
without touching existing code paths.

**Introduce a `Capability` primitive alongside Project.** Rejected per
[[feedback-capabilities-emerge-not-declared]]. Capabilities are human labels for
recurring cross-repo traversals; the traversal is the evidence, the name is not.
Adding `Capability` now would encode identity that later evidence cannot revise.

## Validated outcome

Before:
```
$ atlas ingest rwatp-core
Ingested 4127 commits.
$ atlas ingest rwatp-notifier
Ingested 812 commits.
$ # nothing associates them; no cross-repo query surface
```

After:
```
$ atlas project init rwatp
Project 'rwatp' ready (id 1).
$ atlas project register rwatp ./rwatp-core     --name core
$ atlas project register rwatp ./rwatp-notifier --name notifier
$ atlas project ingest rwatp
  [done] core       commits=4127 renames=88 identities=52 prs=0 edges=0 docs=3
  [done] notifier   commits=812  renames=12 identities=8  prs=0 edges=0 docs=1
Ingested 2 repositories (0 skipped).

$ atlas project census rwatp
── core (accessible)
   path: /home/sanoy/rwatp-core
   framework          express, apollo-server, @nestjs/common, …
   language           TypeScript
   messaging          @google-cloud/pubsub
   module             modules, services, models
   package_manager    npm
   persistence        mongoose
   runtime            Node.js

── notifier (accessible)
   ...
```

Test suite: 191 tests pass, 0 fail (was 39 blackbox + 22 core; now 44 blackbox +
27 core; 5 new project blackbox tests, 5 new core unit tests). No existing tests
modified.

## Future

This change enables — but deliberately does not deliver — Phase 2 and Phase 3 of the
cross-repository roadmap:

- **Phase 2** — pick one real RWATP cross-repo bug (candidate: a buy-tokens transaction
  where the blockchain returns `SUCCESS` but Core expects `COMPLETED`). Document the
  ground truth. Run current Atlas against it. Record the specific failure. That
  failure earns Phase 3.
- **Phase 3** — implement `Contract` + `ContractBinding` for one contract type
  (Pub/Sub only, per the plan). Extend `InvestigationDocument` traversal to cross
  the repo frontier via matched contract bindings. **No** new document type; **no**
  `Capability` primitive; **no** declaration commands.

Emergence of recurring cross-repository slice shapes is deferred until enough
`InvestigationDocument`s have been produced across a project to make pattern-mining
meaningful — per the working principle that observations precede concepts.
