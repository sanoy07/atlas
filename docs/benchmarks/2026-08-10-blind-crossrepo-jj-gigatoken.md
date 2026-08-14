---
title: Blind cross-repository evaluation — JJ + GigaToken
date: 2026-08-10
repository: jj + gigatoken (independent; not development corpora)
status: Complete
---

## Repository

| Corpus | Path | Role | Ingest |
|--------|------|------|--------|
| **JJ** (Jujutsu) | `projects/research/jj` | Large mature VCS; unusual architecture; Rust monorepo `lib/` + `cli/` | HEAD-only + Rust structural (~1.2k edges) |
| **GigaToken** | `projects/research/gigatoken` | High-perf tokenizer; different domain vocabulary; Rust core + Python bindings | HEAD-only + Rust structural (~130 edges) |
| RWATP / VestaScan | (prior suites) | Development / multi-product corpus — **not re-run here**; used only for progression context | prior |

**Evaluator protocol:** Gold established by independent source inspection of each repository **before** Atlas outputs were used for ground truth. Suite runner: `eval/localization/crossrepo/`. Artifacts: `/tmp/atlas-crossrepo-eval/`. AI synthesis disabled (`--no-ai`) so Layer 2 measures C5 retrieval/ranking + C4 packet structure only.

**Progression target:**

```text
RWATP → VestaScan → GigaToken → JJ
```

This benchmark is the **unfamiliar-repo** half of that progression.

---

## Question

How useful is Atlas for understanding repositories it did **not** evolve around?

Three layers:

1. **Old Atlas** — status, map, tree, hot-files, modules, impact, search, context, co-changes, timeline, structural, show, inspect  
2. **C4/C5 pipeline** — investigate → C5.1-R/L/E ranking → C4 verification policy packet (no Qwen this run)  
3. **Blind expert comparison** — independent gold vs Atlas bag/top5/hard-negatives  

---

## Ground Truth

Fifteen cases under `eval/localization/crossrepo/` (8 JJ, 7 GigaToken). Gold files / hard negatives / workflows established from repository layout and source reading, not from Atlas.

### JJ gold (summary)

| Id | Question type | Gold core |
|----|---------------|-----------|
| jj-orient | orientation | `lib/src/lib.rs`, `lib/src/repo.rs`, `cli/src/main.rs`, `Cargo.toml`, `README.md` |
| jj-op-log | architecture | `operation.rs`, `op_store.rs`, `op_heads_store.rs`, `simple_op_store.rs`, `op_walk.rs` |
| jj-conflicts | architecture | `conflicts.rs`, `merge.rs`, `conflict_labels.rs` (+ tree merge) |
| jj-backend | architecture | `backend.rs`, `git_backend.rs`, `git.rs` |
| jj-workspace | architecture | `workspace.rs`, `working_copy.rs`, `local_working_copy.rs`, `workspace_store.rs` |
| jj-flow-commit | system flow | WC → `commit_builder`/`commit` → `repo`/`transaction` → `backend` |
| jj-impact-op-store | change impact | op_store neighborhood: `simple_op_store`, `operation`, `op_heads`, `transaction` |
| jj-adversarial-git | adversarial causal | conflicts/merge primary; git_backend is storage — must not sole-cause SUPPORTED |

### GigaToken gold (summary)

| Id | Question type | Gold core |
|----|---------------|-----------|
| gt-orient | orientation | `src/lib.rs`, `src/token.rs`, `Cargo.toml`, `README.md` |
| gt-tokenize | architecture | `token.rs`, pretokenize, BPE |
| gt-cache | architecture | `src/bpe/pretoken_cache.rs` (+ pretoken traits) |
| gt-batch | architecture | `src/batch.rs` |
| gt-bpe | architecture | `src/bpe/mod.rs`, tiktoken, sentencepiece |
| gt-flow-encode | system flow | load_tokenizer → token → pretokenize → bpe → batch |
| gt-adversarial-cache-cause | adversarial | cache relevant; causal claim needs multi-evidence |

---

## Atlas Evaluation

### Commands used (in order)

1. Layer 1 per repo: `status`, `map`, `tree --depth 2`, `hot-files`, `modules`, `impact <known gold file>`  
2. Layer 2: `investigate <question> --no-ai --json --rounds 1` (and human form) for each case  
3. Spot-checks: `search`, `context`, `co-changes`, `timeline`, `structural`, `show`, `focus`, `inspect`  

### Automated localization scores (Layer 2)

| Metric | Value |
|--------|-------|
| **Pass rate** | **40.0% (6/15)** |
| Gate (`min_pass_rate` 0.6) | **FAIL** |
| JJ | **2/7 scored** (+ 1 run crash) |
| GigaToken | **4/7** |

| Id | Pass | bag∩gold | top5∩gold | top5∩hard | top1 | Notes |
|----|------|----------|-----------|-----------|------|-------|
| jj-orient | ✗ | 0 | 0 | 0 | ✗ | Top: `.github/workflows/scorecards.yml` |
| jj-op-log | ✗ | — | — | — | — | **Panic** UTF-8 char boundary on em-dash in question |
| jj-conflicts | ✗ | 1 | 0 | 0 | ✗ | Gold in bag (`conflicts.rs`); top5 demos/PNG |
| jj-backend | ✓ | 3 | 2 | 0 | ✓ | Strong name-aligned hit |
| jj-workspace | ✓ | 2 | 2 | 0 | ✗ | WC files ranked; misses `workspace.rs` as #1 |
| jj-flow-commit | ✗ | 0 | 0 | 0 | ✗ | Stuck on working_copy; no multi-hop to commit/tx/backend |
| jj-impact-op-store | ✗ | 0 | 0 | 0 | ✗ | investigate ranks CHANGELOG/CLI ops; **not** `op_store.rs` |
| jj-adversarial-git | ✗ | 3 | 0 | 0 | ✗ | Gold in bag; top5 examples/tests/`secret_backend` |
| gt-orient | ✗ | 0 | 0 | 0 | ✗ | Python `_load/hub` not Rust `src/lib.rs` |
| gt-tokenize | ✗ | 1 | 0 | 0 | ✗ | Near-miss pretokenize leaf; misses `token.rs` pipeline |
| gt-cache | ✓ | 1 | 1 | 0 | ✓ | Clean lexical+path hit |
| gt-batch | ✓ | 1 | 1 | 0 | ✗ | `batch.rs` in top5; CI.yml ranked above it |
| gt-bpe | ✓ | 2 | 2 | 0 | ✗ | Good BPE module hit |
| gt-flow-encode | ✗ | 2 | 1 | **1** | ✗ | Hard-neg `notebooks/`; rename debris `gigatok`/`jeton` #1–2 |
| gt-adversarial-cache-cause | ✓ | 1 | 1 | 0 | ✓ | Localizes cache; hypothesis still auto-SUPPORTED |

### Layer 1 — old Atlas capabilities

#### What worked without knowing gold paths

| Capability | JJ | GigaToken | Verdict |
|------------|----|-----------|---------|
| **ingest + status** | ok HEAD-only | ok | Operational |
| **tree** | Excellent monorepo shape (`lib/`, `cli/`, `docs/`) | Excellent (`src/`, `gigatoken/`, benches) | **High value orientation** |
| **hot-files** | Real hotspots (`cli_util`, `revset`, `git`, `repo`) | Real (`lib.rs`, pretokenize, tiktoken) | Useful after tree |
| **map** | **Wrong subject**: treats `src/` as root → only `commands`/`config`; misses `lib/` | Correct 8 modules under `src/` + coupling cells | **Layout-sensitive failure** |
| **modules** | 0 under default `src/modules` | 0 under default `src/modules` | Default subject wrong for both |
| **impact (seed known)** | `op_store.rs` → `simple_op_store` 0.60, `repo`, strong co-change | `pretoken_cache` → tiktoken, batch | **Best change-impact tool when seed known** |
| **search** | `conflict merge` → gold `merge.rs`, `conflicts.rs`, docs | (spot) lexical works | Often **better** than investigate for named concepts |
| **context / show / timeline / co-changes** | Rich once path known | Same | Senior engineer tools post-localization |
| **structural / focus** | Imports graph works on Rust | Focus on `batch.rs` shows real import neighborhood | Strong **neighborhood**, not flow |
| **inspect** | Directory listing | — | Thin; not architecture map |

#### Layer 1 qualitative score (unfamiliar engineer, 30 minutes)

- **With tree + hot-files + search:** can orient and name major subsystems  
- **With map/modules alone on JJ:** actively misleading  
- **Without a seed path:** impact/focus unused; investigate must carry the load  

### Layer 2 — C5 retrieval / ranking failures (detail)

#### Correct findings (do not undervalue)

1. **Filename-aligned architecture questions work** — backend, workspace WC, cache, batch, BPE.  
2. **Hard negatives often avoided** when pass criteria met (except `gt-flow-encode`).  
3. **Bag of gold often in bag even when top5 fails** (conflicts, adversarial-git) — ranking problem more than total miss.  
4. **Supersession + verification policy text always present** in packet (C4 scaffolding lives).  
5. **GigaToken (4/7) > JJ (2/7)** — smaller, flatter `src/` layout matches Atlas assumptions better than JJ monorepo.

#### Missed findings

| Class | Examples |
|-------|----------|
| Orientation without anchors | `lib/src/lib.rs`, `src/lib.rs`, README never surface as top evidence |
| Multi-hop flow | commit_builder/transaction never appear for commit flow; token.rs misses encode flow |
| Production over demo | conflicts demos/PNG outrank `conflicts.rs` |
| Rename debris | historical package names `gigatok`/`jeton` outrank current `gigatoken` + `src/` |
| Investigate ≠ impact | asking impact in natural language does not invoke impact graph of `op_store` |

#### False positives / noise

- CI workflows, scorecards, CHANGELOG as “architecture”  
- Demo scripts and **binary PNG/SVG** as ranked implementation evidence  
- Examples (`custom-backend`, `custom-working-copy`) above production traits  
- Notebooks on encode flow  

#### Unsupported claims (C4 gap under --no-ai)

Every successful investigation emitted:

```text
HYPOTHESIS 1
  Deterministic retrieval associates this question with `<top file>` …
  STATUS: SUPPORTED
  Supporting evidence:
    - [file] <top file> — Top-ranked core candidate from anchor investigation
```

This **collapses existence/ranking into SUPPORTED**. C4 policy text says existence is necessary but not sufficient; the hypothesis emitter does not apply that policy. For adversarial questions (“Could X be caused by Y?”) this is exactly the Redis-class failure mode: **localization without causal entailment still prints SUPPORTED**.

#### Important unknowns Atlas failed to expose

- That JJ core is under **`lib/` not `src/`** (map never said so)  
- That encode flow spans **Python bindings + Rust core** with rename history  
- That **no GitHub PRs/issues** were ingested (stated in “does not know” — good) but orientation still pretended confidence via SUPPORTED  
- Crash on non-ASCII punctuation means some real engineer questions never run  

### Manual source reads required (evaluator)

Independent gold required reading:

- JJ: `lib/src/lib.rs`, op_store/operation cluster, conflicts/merge, backend traits, workspace/WC  
- GigaToken: `src/lib.rs`, `token.rs`, `batch.rs`, `bpe/*`, `pretokenize/*`, `load_tokenizer/*`  

After Atlas: additional reads still needed for every FAIL case and for verifying PASS cases were not demo-noise.

### Wrong branches followed (Atlas-led)

1. GitHub scorecards / docs as “repository structure”  
2. Demos and conflict PNGs as conflict implementation  
3. Working-copy examples as commit-creation flow  
4. CLI operation commands + CHANGELOG as op_store impact  
5. Python hub loaders as GigaToken architecture overview  
6. Obsolete package names on encode flow  

### False positives table

| Query | Unexpected match | Reason | Severity |
|-------|------------------|--------|----------|
| jj-orient | scorecards.yml | lexical “repository”/github noise + hot docs | High |
| jj-conflicts | demo_*.sh, *.png | path token “conflict” in demos | High |
| jj-flow-commit | custom-working-copy example | WC anchors without flow expansion | High |
| jj-impact-op-store | CHANGELOG.md | hot-file bleed into investigate | High |
| gt-orient | gigatoken/_load/hub.py | “load/hub” vs library core | High |
| gt-flow-encode | gigatok/, jeton/, notebooks/ | renames + notebooks hard-neg | High |
| gt-batch | .github/workflows/CI.yml | “batch”/CI lexical | Medium |
| adversarial (both) | examples/tests first | role-aware incomplete | Medium |

### Useful observations

- **`atlas search` recovered conflict gold that `investigate` buried under demos** — dual-path retrieval not unified.  
- **`atlas impact <file>` is high quality** when the engineer already localized the seed; investigate does not call it.  
- **C5.1 name-match is real** on backend/cache/bpe — not zero generalization.  
- **GigaToken map under `src/`** is the first time Section C map looked production-useful on a non-RWATP tree.  
- **No embeddings used** — failures are structural/layout/role/flow, not “need vectors first.”

---

## Classification

| Overall | **Blocked** for blind senior-engineer usefulness; **Improved** vs pure lexical for named subsystems |
| Commands needed (orientation) | tree + hot-files + search ≈ 3–5; investigate alone insufficient |
| Source reads needed | High on FAIL; moderate on PASS |
| Confidence | **High** in scores (deterministic suite + independent gold) |
| Noise removed | Role-aware / L helped some; still insufficient on demos/CI/renames |
| Hidden understanding revealed | Impact graph and co-change excellent **after** seed; investigate does not bridge to them |

### Metrics (engineering meaning)

| Metric | Observation |
|--------|-------------|
| Recall@5 (files) | Weak on orientation/flow; strong on name-aligned architecture |
| Precision@5 | Often polluted by demos/CI/examples |
| Flow coverage | **Poor** — multi-hop not reconstructed |
| Cross-module coverage | Partial via structural when seed known; weak from free text |
| Historical accuracy | Path-scoped history good; chronology follows wrong seeds |
| False causal claims | Auto-hypothesis **SUPPORTED** without entailment |
| Unknown handling | Packet lists unknowns; contradicted by SUPPORTED hypothesis |
| Qwen usefulness | **Not measured** this run (`--no-ai`) |

---

## Scores 0–100 (independent evaluator)

| Axis | Score | Rationale |
|------|------:|-----------|
| Retrieval | **42** | Name-aligned hits; orientation/flow bag often empty of gold |
| Ranking | **38** | Frequent demos/CI/examples/PNG above production |
| Structural understanding | **62** | focus/impact/structural strong with known path; free-text weak |
| Historical understanding | **55** | context/timeline/co-change solid; polluted when seed wrong |
| Evidence verification (C4) | **32** | Policy present; hypothesis emitter violates existence≠support |
| Reasoning (AI) | **n/a → 0** | Not run; deferred until retrieval earns it |
| Cross-repository understanding | **40** | Gate fail; GT > JJ; layout assumptions leak |
| **Overall usefulness to a senior engineer** | **45** | Good **after** localization; weak **to** localization on unfamiliar trees |

**Progression read:** Not yet an “actual repository-understanding system” across four corpora. Closer to a **strong neighborhood toolkit + name-aligned investigate**, overfit-risk still material on monorepo layouts unlike RWATP/VestaScan TS trees.

---

## Three highest-value deficiencies

Ordered by evaluation evidence only (not technical interest):

### 1. Free-text localization fails without filename anchors (orientation + multi-hop flow)

**Evidence:** jj-orient, jj-flow-commit, jj-impact-op-store, gt-orient, gt-flow-encode, gt-tokenize near-miss.  
**Cannot answer:** “What is this repo?” / “Trace X through the system” / “If I change X (in English)” when X is a concept not a path.  
**Architectural implication:** Need **layout priors** (Cargo workspace roots, README/lib entrypoints) and **flow expansion** that walks structural/co-change from multiple seeds — not more prose.  
**Counter-evidence that impact already works:** Layer1 `impact lib/src/op_store.rs` ranked `simple_op_store` correctly while investigate on the same concept did not find `op_store` at all. **Bridge investigate → impact/focus** is earned.

### 2. Non-production and rename debris dominate ranking

**Evidence:** demos/PNG on conflicts; scorecards on orient; CI on batch; `gigatok`/`jeton` on encode; examples over traits.  
**Cannot answer:** Reliable top-k for implementation work without human filtering.  
**Architectural implication:** Harden **role-aware / path-class demotion** (demos, assets, `.github`, notebooks, historical rename paths) and prefer **ProductionSource** + current package names. C5.1-E incomplete outside RWATP roles.

### 3. C4 verification not applied to deterministic hypotheses

**Evidence:** All cases auto-`STATUS: SUPPORTED` from “top-ranked candidate”; adversarial gt-cache/jj-git still SUPPORTED association.  
**Cannot answer:** “What evidence would prove Atlas wrong?” / causal adversarial questions.  
**Architectural implication:** Wire **hard_verify** to hypothesis emission — association ≤ PLAUSIBLE; causal needs multi-source same-subject support (sacred Redis rule). Policy text alone is theater.

### Bonus defect (not a feature request)

**UTF-8 panic** in `detect_issue_numbers` (`retrieval_expand.rs`) on multi-byte punctuation (em-dash in jj-op-log question). Classified **Bug**. Blocks evaluation and production questions with typographic dashes.

---

## Outcomes

- **Decision produced?** Evaluation-only decision: do **not** start C5.2 embeddings / Qwen-first / architecture invent from this suite. Fix ordering: (0) UTF-8 bug, (1) investigate→impact bridge + layout priors, (2) path-class ranking, (3) C4 on hypotheses.  
- **New primitive earned?**  
  - **Layout-aware module root discovery** (Cargo workspace / multi-crate) — N≥2 (JJ fails, GigaToken succeeds under `src/`).  
  - **Investigate→impact/focus bridge** when concept resolves to a trait/file — earned by op_store divergence.  
  - **Hypothesis status must call hard_verify** — earned by universal false SUPPORTED.  
- **New abstraction earned?** No — implement concrete fixes first.  
- **Regression?** Not vs RWATP 9/9 or VestaScan ≥70% (not re-run this session). Cross-repo gate fail is new measured bar.  
- **Unexpected discoveries?**  
  - `search` sometimes beats `investigate` for the same anchors.  
  - Binary assets can enter ranked_evidence as implementation.  
  - Package rename history is a first-class retrieval poison on GigaToken.

---

## What this does *not* recommend yet

- Embeddings-first C5.2  
- Qwen as primary reasoner before retrieval/C4 fixes  
- Full Aider Repo Map import  
- New command families without closing the three deficiencies  

---

## Reproduction

```bash
# DBs assumed ingested under each research repo
ATLAS_BIN=./target/debug/atlas \
SCORE_OUT=/tmp/atlas-crossrepo-eval \
node eval/localization/crossrepo/run_suite.mjs
```

Gold cases: `eval/localization/crossrepo/*.json`  
Suite gate: `min_pass_rate` 0.6 (failed at 0.40)

---

## Investigation Summary (session end)

Blind dual-repo eval completed. Layer1 orientation tools mixed; Layer2 C5 investigate 40% pass. Top deficiencies are free-text localization, non-production ranking, and C4 hypothesis wiring — not “need more AI.”

---

## Re-run after C5.1-S + path_class + C4 hyp hard_verify (same day)

**Decision:** `docs/decisions/2026-08-10-c5-1s-subject-path-class-c4.md`  
**Artifacts:** `/tmp/atlas-crossrepo-eval-v2/`

| Metric | Baseline | After three fixes |
|--------|----------|-------------------|
| Pass rate | 40% (6/15) | **80% (12/15)** |
| JJ | 2/7 | **8/8** |
| GigaToken | 4/7 | 4/7 |
| C4 bad SUPPORTED | universal det hyp | **0** |
| Gate (≥60% + C4 clean) | fail | **pass** |

### Remaining GigaToken misses (not fixed this pass)

| Id | Notes |
|----|-------|
| gt-orient | `src/lib.rs` in bag; top5 still Python package loaders |
| gt-tokenize | pretok only; misses `token.rs` + BPE multi-module end-to-end |
| gt-flow-encode | batch + bpe in bag; rename debris `gigatok`/`jeton` still top |

### Architectural conclusion (updated)

> Atlas’s deterministic substrate is useful when the engineer gives it a subject. **C5.1-S + path class closed the free-text→subject gap on JJ completely (8/8).** GigaToken flow/rename cases remain the next earned failure class — not embeddings-first, not Qwen-first.
