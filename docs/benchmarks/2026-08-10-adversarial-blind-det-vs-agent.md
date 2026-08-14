---
title: Blind adversarial evaluation — Deterministic C5.1+C4 vs Qwen 3:4b tool-calling agent
date: 2026-08-10
repositories: jj (Jujutsu), GigaToken
status: Complete
protocol: Gold first (GOLD.md), then Atlas; no gold edits after runs; no Atlas code changes during eval
artifacts: /tmp/atlas-adversarial-blind/
---

## Executive summary (skeptical)

| Metric | Deterministic `investigate --no-ai` | Qwen tool-calling agent |
|--------|--------------------------------------|-------------------------|
| **Overall mean (0–100, suite scorer)** | **54.2** | **67.3** |
| JJ mean | 57.3 | 68.1 |
| GigaToken mean | 51.1 | 66.4 |
| Mean latency | **~1.0 s** | **~116 s** |
| Mean tool calls / steps | n/a | **2.4 calls · 3.4 steps** |
| Cases agent found gold det missed (top10) | — | **7 / 14** |
| C4-style causal discipline failures | **0** (det hyp max PLAUSIBLE) | **1** (`gt-adversarial`) |
| Invented path references (scorer) | 0 | **0** |

**Headline:** The agent **raises open-ended orientation/architecture scores** (where det bag is empty) and **occasionally surfaces files det missed**, but it **does not dominate** when the deterministic pipeline already localizes well. On name-anchored impact/flow/issue cases, det often **wins on precision** and is **~100× faster**. The agent still **mis-localizes concurrent-ops (JJ)** and **pretoken-cache thrashing (GT)**, and once made an **over-confident causal negation**.

This is **not** proof that Atlas “understands unfamiliar repos end-to-end.” It is evidence that **tool selection over map/search/focus** helps **orientation**, while **C5.1 subject resolution** remains the right path for **named-subject localization**, and **C4 is still required** because free-form agent prose will assert causality.

Gold frozen before runs: `eval/localization/adversarial-blind/GOLD.md` (+ `suite.json` copied to `/tmp/atlas-adversarial-blind/suite.gold.frozen.json`).

---

## Protocol

1. Manual gold for 14 questions (7 JJ + 7 GigaToken) written **before** any Atlas/agent execution.
2. Run **A:** `atlas investigate "<q>" --no-ai --json --rounds 1`
3. Run **B:** `python3 agent/atlas_agent.py --repo <path> --max-steps 8 "<q>"` (qwen3:4b, think on, 11 tools)
4. No gold edits after outputs. No Atlas feature changes during the eval.
5. Scores are **heuristic 0–100** over gold hits / hard-negatives / causal flags / agent novelty — **not** human preference. Qualitative review overrides where scorer and truth diverge.

Repos: `/home/sanoy/projects/research/jj`, `/home/sanoy/projects/research/gigatoken` (existing `atlas.db`).

---

## Per-question results

### Jujutsu

| Id | Workflow | Det | Agent | Δ | Det bag∩gold | Det top5∩gold | Agent tools | New gold vs det top10 | Notes |
|----|----------|----:|------:|--:|-------------:|--------------:|-------------|----------------------|-------|
| jj-orient | orientation | 22 | **64** | +42 | 0 | 0 | map | — | Det bag empty on free-text; agent map orients but **overweights hot files** (`default_index`, `cli_util`) vs `lib.rs`/`repo.rs` |
| jj-architecture | architecture | 22 | **68** | +46 | 0 | 0 | map | `repo.rs` | Agent better layers narrative; still incomplete vs backend/op_store/WC |
| jj-bug | bug | 21 | **48** | +27 | 0 | 0 | map→focus→explain | — | **Both weak.** Agent **wrong locus**: `lib/src/lock` (file locks) not **op_heads / op_store / transaction** |
| jj-flow | flow | **88** | 76 | −12 | 4 | 3 | map→search→focus→explain | `commit_builder.rs` | **Det wins.** Agent discovers commit_builder but weaker stage completeness vs scored det bag |
| jj-issue | issue/impl | **83** | 74 | −9 | 4 | 3 | map→search→focus→read_file | `docs/conflicts.md` | Det strong on conflicts.rs/merge; agent adds docs intent — good intent/impl split signal |
| jj-impact | impact | **83** | 71 | −12 | 4 | 2 | search→search→impact | — | Det C5.1-S on `op_store` is strong; agent impact OK but slower |
| jj-adversarial | adversarial | **82** | 76 | −6 | 3 | 2 | map→focus→impact | `merge.rs` | Det bags conflicts+git_backend; agent **hedges** (“evidence does not settle”) — good C4 posture |

### GigaToken

| Id | Workflow | Det | Agent | Δ | bag | top5 | hard | Agent tools | New gold | Notes |
|----|----------|----:|------:|--:|----:|-----:|-----:|-------------|---------|-------|
| gt-orient | orientation | 22 | **51** | +29 | 0 | 0 | 0 | map | — | Agent still thin; map modules not full `src/lib.rs` narrative |
| gt-architecture | architecture | 22 | **74** | +52 | 0 | 0 | 0 | map | `pretokenize/mod.rs` | Largest agent win; map module inventory helps |
| gt-bug | bug | 46 | **65** | +19 | 2 | 0 | 0 | map→focus | — | Det has cache in bag but not top5; agent **misses `pretoken_cache.rs`**, blames `pretokenize/mod.rs` generically |
| gt-flow | flow | 66 | **79** | +13 | 3 | 0 | **2** | map→search→focus | `pretokenize/mod.rs` | Det hard-neg notebooks/renames in top5; agent cleaner flow narrative |
| gt-issue | intent vs impl | 52 | **68** | +16 | 1 | 1 | 0 | map→search→read_file→focus | `design_doc.md` | Agent correctly opens design_doc — **intent surface** |
| gt-impact | impact | **83** | 71 | −12 | 4 | 2 | 0 | impact | — | Det impact neighborhood excellent once path known |
| gt-adversarial | adversarial | **67** | 57 | −10 | 2 | 1 | 0 | search→focus | — | **C4 failure (agent):** confident “**not** caused by hub redownload” from cache-related commits — overclaim |

---

## Aggregates requested

| # | Metric | Value |
|---|--------|------:|
| 1 | Overall deterministic score | **54.2** |
| 2 | Overall Qwen-agent score | **67.3** |
| 3 | JJ (det / agent) | **57.3 / 68.1** |
| 4 | GigaToken (det / agent) | **51.1 / 66.4** |
| 7 | Average tool calls | **2.36** |
| 8 | Average latency | det **1.0s** · agent **116s** |
| 9 | Cases Qwen discovered new useful gold vs det top10 | **7** |
| 10 | Cases where Qwen mainly repeated det neighborhood | **~7** (impact/flow/adversarial overlap) |
| 11 | Unsupported agent claims (qualitative) | Several overconfident on bugs; **1** hard C4 flag |
| 12 | C4 violations (suite flag) | **1** (`gt-adversarial`) |
| 13 | Hallucinated path refs (scorer) | **0** |
| 14 | Top failure modes | see below |

---

## Failure class taxonomy (primary class per weak case)

| Case | Primary class | Rationale |
|------|---------------|-----------|
| jj-orient (det) | **A. Retrieval** | Free-text orientation bag empty; map tool not used by investigate |
| jj-architecture (det) | **A. Retrieval** | Same |
| jj-bug (both) | **E. Agent** (agent) / **A. Retrieval** (det) | Agent had tools to search “op heads concurrent” but chose lock module via hot-file map bias |
| gt-orient (both) | **A/E** | Map insufficient for “start reading”; agent stopped after map |
| gt-bug (agent) | **E. Agent** | Cache file exists; search terms wrong; confident wrong module |
| gt-adversarial (agent) | **F. Reasoning/verification** | Evidence for “uses HF cache” ≠ proof encode slowness is not hub-related in all scenarios; asserted causal negative |
| gt-flow (det) | **B. Ranking** | Gold in bag; notebooks/rename debris in top5 |
| When det ≥80 | — | Not failures; agent slightly dilutes precision with map preamble |

**No gold/scope failures** (G). **No pure structural graph missing edges** proven as sole class (C) — agent could have used search/impact more aggressively. Historical/intent (D): agent **helped** on `docs/conflicts.md` and `design_doc.md`.

---

## Deterministic vs Qwen comparison

```text
When question is OPEN (orient/architecture):
  det investigate  ≈ weak bag (0 gold)
  agent atlas_map  ≈ strong orientation lift (+29 to +52)

When question is ANCHORED (path/name/impact/flow after C5.1-S):
  det investigate  ≈ strong (80–88)
  agent            ≈ good but slower and slightly noisier (−6 to −12)

When question is ADVERSARIAL CAUSAL:
  det              ≈ C4 keeps hyp PLAUSIBLE; bags correct mixed evidence
  agent            ≈ often hedges well (jj-adversarial) OR overclaims (gt-adversarial)
```

**Multi-hop (previous 80% ceiling cases):**  
- **gt-flow:** agent improved (+13) and reduced hard-neg dominance narrative.  
- **gt-orient:** agent improved but still mediocre (51).  
- **jj-flow:** det already excellent; agent did not beat it.  
- **jj-bug concurrent:** **neither solved** — remains a real gap.

---

## Decision gates

| Gate | Result | Evidence |
|------|--------|----------|
| **G1** Does Qwen improve overall vs det? | **YES (weak–moderate)** | 67.3 > 54.2; lift concentrated in orientation/architecture |
| **G2** Does Qwen fix multi-hop ceiling cases? | **PARTIAL** | Helps gt-flow/orient somewhat; **fails jj-bug**; does not beat det on jj-flow |
| **G3** New useful evidence vs paraphrase? | **YES, half the suite** | 7/14 new gold hits vs det top10; other half paraphrases |
| **G4** Unsupported claims / C4 violations? | **YES (present)** | 1 suite flag + qualitative wrong bug loci |
| **G5** Justify C5.2 structural/AST next? | **NOT YET as primary** | Failures are mostly **agent tool choice / free-text det bag empty / causal prose**, not “missing AST edges” in this suite. C5.2 not earned by these 14 questions alone |

---

## Final verdict (direct answers)

### 1. Is deterministic Atlas generalizing beyond RWATP?

**Partially yes, with a sharp condition.**  
On JJ/GigaToken, when the question **names or strongly compounds to a subject** (`op_store`, conflicts, pretoken_cache impact), C5.1-S + path class + C4 **generalizes** (scores 80–88). On **pure orientation free-text**, investigate still **collapses** (bag 0). So: **not RWATP-only for localization-with-anchors; still weak for cold-start orientation.**

### 2. Does the Qwen 3:4b agent materially improve Atlas?

**Yes for orientation/architecture product UX; no as a universal replacement for investigate.**  
Net score up ~13 points, but latency ~100× worse, and **regresses** several already-solved anchored cases.

### 3. Which JJ/GigaToken problems does the agent solve?

- Cold-start **“where do I start?”** via `atlas_map`
- Pulling **docs/design_doc** for intent vs implementation
- Sometimes **cleaner flow narrative** when det ranking is polluted (gt-flow hard-negs)
- **Hedging** on some causal traps (jj-adversarial)

### 4. Which problems remain unsolved?

- Concurrent ops → **op_heads/transaction** (agent went to **locks**)
- Encode thrashing → **`pretoken_cache.rs`** (agent stayed in pretokenize generically)
- Det free-text orientation still broken without calling map
- Causal overclaim risk without C4 on agent text
- Agent does not call **`investigate`** (misses C5.1-S + C4 packet)

### 5. Is Qwen useful as investigation/orchestration layer?

**Conditionally yes — as a tool router over Atlas CLI**, not as a truth oracle.  
Useful orchestration pattern: map/search/focus/impact. **Unsafe** if answers are accepted without C4 / human check on causal claims.

### 6. Should we build C5.2 structural/AST retrieval next?

**Not as the top priority from this suite.**  
Failures did not primarily show “edge type missing in the graph.” They show **empty free-text bag**, **hot-file bias**, and **agent not selecting search anchors that hit gold files**.

### 7. Should we expand the agent loop?

**Yes, but surgically:**  
- Prefer/force `atlas investigate` (or C5.1-S seeds) before free prose  
- Feed agent **verification_policy + hard_verify** on final claims  
- Discourage stopping after a single `atlas_map` for localization bugs  
- Do **not** add more tools until those control loops exist

### 8. Single highest-value next engineering change

**Wire the agent’s final answer (and preferably each causal claim) through C4 hard_verify, and make `investigate` / subject-resolution a first-class tool (or default first step for localization/bug questions) instead of only map/search/focus.**

That unifies the two architectures:

```text
Qwen selects tools
   → Atlas retrieve/rank (including investigate packet)
   → C4 verify claims
   → only then emit answer
```

Not embeddings. Not a bigger model. **Orchestration + verification.**

---

## Gates summary line

| G1 improve overall | G2 multi-hop | G3 new evidence | G4 C4 risk | G5 C5.2 now? |
|--------------------|--------------|-----------------|------------|--------------|
| **Yes (orient-heavy)** | **Partial** | **Yes (7/14)** | **Yes (1+)** | **No** |

---

## Reproduction

```bash
# Gold is already frozen in eval/localization/adversarial-blind/
ATLAS_BIN=./target/release/atlas USE_NIX_PYTHON=1 \
SCORE_OUT=/tmp/atlas-adversarial-blind \
node eval/localization/adversarial-blind/run_eval.mjs
```

Artifacts: `/tmp/atlas-adversarial-blind/{det,agent,logs,summary.json,GOLD.md,suite.gold.frozen.json}`
