---
title: Repository Lexicon — deterministic vocabulary bridge for investigation Phase 0a
date: 2026-07-26
status: Implemented
---

## Problem

Atlas investigations fail when user vocabulary does not match implementation file naming.
Confirmed across two repositories during benchmark session:

- jj: `atlas investigate conflict` → 0 candidates. Ground truth: `lib/src/merge.rs`
- jj: `atlas investigate operation` → 0 candidates. Ground truth: `lib/src/op_store.rs`
- Gigatoken: `atlas investigate batch` → 0 candidates. Ground truth: `src/bpe/tiktoken.rs`

The existing Phase 0 (concept resolution via PR/issue bodies) only bridges when the
documentary corpus explicitly connects user vocabulary to file names. For repositories
without GitHub PRs ingested, or where this bridging does not appear in PR/issue text,
concept resolution produces nothing.

N=4: confirmed vocabulary mismatch failures across jj and Gigatoken.

## Methodology validation

**Principle 1 (features earned by evidence)**: Four specific investigation failures named above.

**Principle 2 (abstractions earned by repetition)**: Two passes, each addressing a specific
observation. No speculative generalisation beyond what the evidence required.

**Principle 3 (knowledge accumulated)**: This decision record + new benchmark entries.

**Principle 4 (cross-repository validation)**: Gap confirmed in both jj (Rust VCS) and
Gigatoken (Rust tokenization library) — architecturally distinct repositories.

## Decision

Implemented a Repository Lexicon subsystem with two ingest-time passes:

**Pass 1 — Structural analysis** (`pass1_structural`):
Decomposes all file paths into tokens. Detects:
- `CompoundComponent`: tokens that co-appear in the same compound filename (`op` + `store` in `op_store.rs`)
- `Abbreviation`: token pairs where one is a known prefix or consonant-drop of the other (`op` ↔ `operation`, `repo` ↔ `repository`)
- `ModuleSibling`: tokens from files in the same directory that share naming patterns

**Pass 2 — Commit correlation** (`pass2_commit_bridge`):
For every commit: cross-correlates subject line tokens with file path tokens.
When subject token S and path token P co-occur in ≥ 3 commits, records `CommitBridge(S, P)`.
This captures vocabulary the team actually used without any LLM, embedding, or ontology.

**Investigation integration** (new Phase 0a):
Before the existing documentary concept resolution (Phase 0), query the lexicon for each
anchor. High-confidence expansions (≥ 0.65) are added to the effective anchor set.
Expansions are surfaced as `lexicon_expansions` in InvestigationDocument (schema_version 5).

**Confidence model**:
- CaseVariant: 1.0 (deterministic)
- Abbreviation: 0.88 (structural rule, high reliability)
- CommitBridge: 0.50–0.85 (asymptotic, grows with co-occurrence count)
- CompoundComponent: 0.70
- ModuleSibling: 0.60
- Threshold for investigation expansion: 0.65

## Alternatives considered

**ML-based / embedding similarity**: Rejected. Non-deterministic, not benchmarkable,
adds infrastructure complexity. Atlas's constraint is deterministic evidence.

**Hard-coded synonym tables**: Rejected. Repository-specific vocabulary cannot be
known in advance. The lexicon is discovered from the repository itself.

**LLM-mediated expansion at query time**: Rejected. Moves deterministic work into
the probabilistic layer. The vocabulary relationship is a fact about the repository,
not an inference that needs regeneration on every query.

**Widen the existing concept resolution pass**: Considered. The existing Phase 0 only
uses PR/issue bodies; expanding it to commit messages would partially overlap with
CommitBridge. Decision: keep them separate — lexicon runs at ingest time (cheap, cached),
concept resolution runs at query time (expensive, per-investigation).

## Validated outcome

Before: `atlas investigate operation` on jj → 0 candidates, "No candidates found."

After ingest with lexicon rebuild:
- Lexicon records `operation → op` (CommitBridge, commits where jj devs wrote "operation"
  in subject lines while touching `op_store.rs`)
- Phase 0a expands anchors: ["operation"] → ["operation", "op"]
- `op` matches `lib/src/op_store.rs` via file_path anchor search
- Investigation now finds the correct file

## Future

- Pass 3: documentation analysis (README, ADR bodies) — `DocumentAlias` relationships
- Confidence decay for stale relationships (when file is renamed/deleted)
- Incremental update: only reprocess commits added since last ingest
- `atlas lexicon explain <term>` command for visibility
