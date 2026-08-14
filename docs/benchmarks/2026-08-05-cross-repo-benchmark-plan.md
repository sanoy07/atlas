---
title: Cross-repository benchmark plan — Phase 2 falsification target
date: 2026-08-05
status: Draft
---

## Purpose

This document declares the Phase 2 benchmark plan and — critically — the
outcome that would **falsify** the working hypothesis behind Phase 3.

Per [[feedback-understand-failure-first]] a primitive can only be built
after the failure it addresses has been characterized in a real case, on
paper, before any code is written.  Per Atlas Methodology Principle 4,
a characterization on one repository is a coincidence; three across
distinct products is what earns a primitive.

## Current state of the cross-repo gap class

- Gap id: `cross-repo-contracts`
- Threshold: 3 (declared in `2026-07-14-vestascan-notification-pubsub.md`)
- Current N: **1** (VestaScan PubSub, VestaScan-API ↔ VestaScan-Notifier)
- Status: candidate — needs two more instances across distinct products
  before Phase 3 code is earned.

The VestaScan case characterized the gap as follows:

> Atlas can surface "this file publishes to PubSub" and "this file
> subscribes from PubSub" but cannot connect them.  The topic names that
> connect publisher to subscriber are string literals in config, not
> structural edges.

## Working hypothesis (subject to falsification)

**Phase 3 primitive candidate:** Contract as Evidence.  A new observed
edge kind (`PublishesTo` / `SubscribesTo`) extracted from string literals
at pub/sub call sites, cross-referenced across the repositories inside
one project, surfacing as directed edges attached to a `Contract`
node keyed by topic identifier.

**Predicted outcome if hypothesis is correct:** the RWATP benchmark
case will characterize under exactly the same terms as VestaScan —
producer visible in repo A, consumer visible in repo B, both invisible
as a connected transaction because the connecting evidence is a matched
string literal that no current parser surfaces as a structural fact.

## Falsification criteria

If the RWATP characterization *does not* match the VestaScan
characterization, Phase 3 must be redirected.  Specifically:

1. **Lexical falsification.**  If the producer and consumer refer to the
   topic under different names (naming drift, versioning suffix, environment
   prefix) and Atlas's failure is that it cannot recognize them as the same
   contract, then the primitive is a *lexicon* extension, not a new edge
   type.
2. **Coverage falsification.**  If either side's file lives in a directory
   Atlas excludes (build output, generated code, config), the primitive is
   a *Repository Awareness* extension, not a new edge type.
3. **Documentary falsification.**  If the connection is discoverable
   through PR/issue text that mentions both sides but Atlas fails to
   bridge them via concept resolution, the primitive is a
   *documentary bridge* improvement, not a contract extractor.
4. **Structural falsification.**  If the contract is expressed in a shared
   type definition (a message schema in a shared package, a GraphQL SDL
   file imported by both sides), then the primitive is a *shared-artifact
   observation*, not per-repo string-literal extraction.
5. **Runtime-only falsification.**  If the connection is not observable
   in source at all — only visible in runtime configuration or
   infrastructure-as-code Atlas does not ingest — then the primitive
   requires a new *connector*, not a new parser.

Any of these outcomes is a legitimate Phase 2 result and would change
what Phase 3 looks like.  None of them is a failure of Phase 2 — they
are the exact evidence the process is designed to produce.

## Success criterion for Phase 2 as a whole

Phase 2 is complete when the corpus contains at least **three** cross-repo
benchmark cases across at least **two** products (RWATP counts as one,
VestaScan as another; Atlas dogfood cannot count because Atlas is a single
git repository).  Each case must:

- Have ground truth written before Atlas is run.
- Follow the schema in [`TEMPLATE-cross-repo.md`](./TEMPLATE-cross-repo.md).
- Declare which of the falsification criteria above (if any) apply.
- Either reuse `gap_id: cross-repo-contracts` (incrementing N) or declare
  a new gap and document why the existing class did not fit.

Only when N=3 across products is the Contract primitive earned.  Phase 3
begins at that moment, not earlier.

## What Phase 2 does NOT do

- Does not extend the `atlas eval` TOML runner.  The current runner runs
  one investigation per case; cross-repo cases require multiple
  invocations composed by a human runner.  Automating this is speculative
  until at least one cross-repo case is authored and shows what
  automation would need to score.
- Does not build any parser, edge kind, or IR type.
- Does not add a `Capability` primitive, a `Contract` primitive, or any
  new document type.
- Does not modify existing tests.

## Immediate next action

Author the second cross-repo benchmark case using one real RWATP
transaction, following `TEMPLATE-cross-repo.md`.  The transaction must
be one the user has actually debugged or traced, with real filenames
and real symbol names on both sides.  Without user input this cannot
proceed — Atlas cannot invent ground truth for a codebase it cannot see.

Once the second case exists, run today's Atlas (with the new project
layer) against it, characterize the exact gap, and check against the
falsification criteria above.  Only then do we know what Phase 3 should
build.
