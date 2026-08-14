# Blind gold set — JJ + GigaToken

**Established:** 2026-08-10  
**Before any Atlas / agent execution for this suite.**  
**Method:** Independent reading of repository trees, README, design docs, and key source paths. Not derived from Atlas output.

---

## Repositories

| Key | Path | DB |
|-----|------|-----|
| jj | `/home/sanoy/projects/research/jj` | `atlas.db` in repo |
| gigatoken | `/home/sanoy/projects/research/gigatoken` | `atlas.db` in repo |

---

## JJ cases

### jj-orient
**Q:** What are the major production subsystems of this repository, and where should I start reading?

| Field | Gold |
|-------|------|
| production files | `lib/src/lib.rs`, `lib/src/repo.rs`, `cli/src/main.rs`, `Cargo.toml`, `README.md` |
| modules | `lib/` (core VCS), `cli/` (user interface), storage backends under lib |
| hard negatives | `demos/`, `.github/workflows/`, `web/` marketing only, PNG assets |
| start reading | `lib/src/lib.rs` then `repo.rs`, `working_copy.rs`, `backend.rs` |
| unsupported claim | Naming CHANGELOG.md or scorecards as “core architecture” |

### jj-architecture
**Q:** What is the main architecture of this system? Identify the important layers/components and how they relate.

| Field | Gold |
|-------|------|
| production files | `lib/src/backend.rs`, `lib/src/git_backend.rs`, `lib/src/op_store.rs`, `lib/src/working_copy.rs`, `lib/src/repo.rs`, `lib/src/conflicts.rs`, `cli/src/cli_util.rs` |
| layers | (1) pluggable storage backend (2) operation log / op_store (3) working copy (4) first-class conflicts (5) CLI over lib |
| hard negatives | demos scripts, PNG, examples/custom-* as primary architecture |
| flow | CLI → lib repo/transaction → WC/backend/op_store |

### jj-bug
**Q:** Users are seeing inconsistent repository state when two processes run Jujutsu operations on the same workspace at the same time. Where does this behavior originate and what should we inspect first?

| Field | Gold |
|-------|------|
| production files | `lib/src/op_heads_store.rs`, `lib/src/simple_op_heads_store.rs`, `lib/src/op_store.rs`, `lib/src/transaction.rs`, `docs/technical/concurrency.md` (if present) / operation log docs |
| optional | `lib/src/operation.rs`, lock-related paths under `lib/src/lock/` |
| hard negatives | `lib/src/conflicts.rs` alone, `lib/src/annotate.rs`, demos |
| causal discipline | Must NOT SUPPORTED-claim a single file “causes” races without multi-evidence; concurrent op-head divergence is the documented model |
| unsupported | Blaming Git backend alone for concurrent WC races |

### jj-flow
**Q:** Walk me through the end-to-end flow for creating and storing a new commit. Start from the working copy and follow it through the important stages.

| Field | Gold |
|-------|------|
| production files | `lib/src/working_copy.rs`, `lib/src/local_working_copy.rs`, `lib/src/commit_builder.rs`, `lib/src/commit.rs`, `lib/src/repo.rs`, `lib/src/transaction.rs`, `lib/src/backend.rs` |
| flow stages | WC snapshot → commit build → mut repo/transaction → backend store → (often) op log update |
| hard negatives | `lib/src/conflicts.rs`, `lib/src/annotate.rs`, pure CLI debug WC commands as sole path |
| flow completeness | Must name ≥3 stages, not only WC neighborhood |

### jj-issue
**Q:** Jujutsu treats conflicts as first-class objects rather than only textual merge failure. What would I need to change in this repository to extend or harden first-class conflict representation? Distinguish current implementation from documented intent.

| Field | Gold |
|-------|------|
| production files | `lib/src/conflicts.rs`, `lib/src/merge.rs`, `lib/src/conflict_labels.rs`, `lib/src/merged_tree.rs`, `docs/conflicts.md` |
| intent docs | `docs/conflicts.md`, design notes under `docs/technical/conflicts.md` if present |
| hard negatives | `lib/src/git_backend.rs` as sole answer, demos conflict scripts as implementation |
| historical | Commits touching conflicts/merge; docs may predate code details |
| discipline | Docs = intent; `conflicts.rs`/`merge.rs` = current behavior |

### jj-impact
**Q:** If I modify the operation store interface (`lib/src/op_store.rs`), what other production components should I investigate and why?

| Field | Gold |
|-------|------|
| production files | `lib/src/op_store.rs`, `lib/src/simple_op_store.rs`, `lib/src/operation.rs`, `lib/src/op_heads_store.rs`, `lib/src/transaction.rs`, `lib/src/op_walk.rs` |
| hard negatives | `lib/src/git.rs` as top neighbor, `lib/src/conflicts.rs`, CHANGELOG.md |
| why | Co-change + imports: simple_op_store implements trait; transaction/op_heads consume ops |

### jj-adversarial
**Q:** Could broken merges be caused by the Git backend mis-storing conflict data?

| Field | Gold |
|-------|------|
| investigation files | `lib/src/conflicts.rs`, `lib/src/merge.rs`, `lib/src/git_backend.rs`, `lib/src/backend.rs` |
| correct stance | Conflicts are first-class in conflicts/merge; git_backend is storage — causal claim needs multi-source same-subject support |
| hard negatives | Treating CLI only as root; `lib/src/annotate.rs` |
| C4 | Existence of git_backend in bag ≠ SUPPORTED cause of broken merges |
| answer quality | PLAUSIBLE at best for git_backend sole-cause; should surface conflicts/merge as primary |

---

## GigaToken cases

### gt-orient
**Q:** What are the major production subsystems of this repository, and where should I start reading?

| Field | Gold |
|-------|------|
| production files | `src/lib.rs`, `src/token.rs`, `src/batch.rs`, `Cargo.toml`, `README.md` |
| modules | `src/pretokenize`, `src/bpe`, `src/batch`, `src/load_tokenizer`, `src/bindings`, Python `gigatoken/` |
| hard negatives | `notebooks/`, `profiling/`, historical package names alone |
| start | `src/lib.rs` + README “How does Gigatoken work?” |

### gt-architecture
**Q:** What is the main architecture of this system? Identify the important layers/components and how they relate.

| Field | Gold |
|-------|------|
| production files | `src/lib.rs`, `src/token.rs`, `src/pretokenize/mod.rs`, `src/bpe/mod.rs`, `src/batch.rs`, `src/load_tokenizer/mod.rs` |
| layers | load tokenizer → pretokenize → BPE encode → batch/parallel → Python bindings |
| hard negatives | `src/input/decompress.rs` alone, notebooks |

### gt-bug
**Q:** Encoding throughput drops sharply when the same short strings are tokenized over and over in a long batch. Where does this behavior originate?

| Field | Gold |
|-------|------|
| production files | `src/bpe/pretoken_cache.rs`, `src/pretokenize/pretoken.rs`, `src/bpe/mod.rs` |
| optional | `src/batch.rs` |
| hard negatives | `src/load_tokenizer/hub.rs` (load ≠ encode loop), `src/input/jsonl.rs` |
| C4 | Cache miss is a candidate cause (PLAUSIBLE) not SUPPORTED without hit-rate evidence |

### gt-flow
**Q:** Walk me through the end-to-end flow from loading a tokenizer to encoding a batch of text. Start from the entry point and follow the important stages.

| Field | Gold |
|-------|------|
| production files | `src/load_tokenizer/mod.rs`, `src/token.rs`, `src/pretokenize/mod.rs`, `src/bpe/mod.rs`, `src/batch.rs` |
| stages | load (HF/tiktoken) → token object → pretokenize → BPE → batch encode |
| hard negatives | `notebooks/`, historical `gigatok/`/`jeton/` paths as current entry |
| flow completeness | ≥3 stages across load + pretokenize + bpe/batch |

### gt-issue
**Q:** The design document describes persisting tokenizer state so repeated Python use keeps cache memory. What would I need to change to implement or harden that requirement? Distinguish documented intent from current implementation.

| Field | Gold |
|-------|------|
| production files | `src/bpe/pretoken_cache.rs`, `src/bindings/` or Python `gigatoken/` lifecycle, `design_doc.md` (“Persistence of tokenizer”) |
| intent | `design_doc.md` section Persistence |
| hard negatives | claiming hub download alone is the persistence layer |
| discipline | design_doc = intent; cache/bindings = implementation |

### gt-impact
**Q:** If I modify `src/bpe/pretoken_cache.rs`, what other production components should I investigate and why?

| Field | Gold |
|-------|------|
| production files | `src/bpe/pretoken_cache.rs`, `src/bpe/tiktoken.rs`, `src/bpe/mod.rs`, `src/batch.rs`, pretokenize callers |
| hard negatives | `src/input/parquet.rs`, notebooks |

### gt-adversarial
**Q:** Could slow tokenization be caused by the HuggingFace hub loader always re-downloading model files?

| Field | Gold |
|-------|------|
| investigation | `src/load_tokenizer/hub.rs` (relevant to download), `src/bpe/pretoken_cache.rs`, `src/batch.rs`, `src/pretokenize/` |
| correct stance | Hub loader affects **load**, not per-encode loop; slow **tokenization** more often cache/BPE/batch |
| C4 | Must not SUPPORTED-claim hub redownload causes encode slowness without structural co-evidence |
| hard negatives | Making hub the sole answer |

---

## Scoring notes

- Gold paths are production-oriented; demos/assets/CI are hard negatives unless question asks for them.
- Causal questions max PLAUSIBLE without multi-source same-subject support (C4 sacred rule).
- Agent “discovered new evidence” only if a gold production path appears in agent tool results but not in det top-10 ranked files / core candidates.
