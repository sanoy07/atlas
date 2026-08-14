---
title: Blind adversarial evaluation — deterministic Atlas vs Qwen3:4b agent
date: 2026-08-10
repos: jujutsu (jj), gigatoken
gold: eval/blind/2026-08-10-gold-set.md (frozen before execution)
raw: eval/blind/raw_results.json, eval/blind/transcripts.md
---

## Headline

The agent does not improve Atlas. Overall it scores **50.1 vs 49.5** for the
deterministic pipeline — inside noise — while costing **120× the latency**
(116.7s vs 0.97s mean). That average hides the real result, which is a clean
split: the agent is **+15 on jj** and **−14 on gigatoken**. It helps where
Atlas's retrieval is weak and multi-hop, and actively harms where Atlas's
retrieval is already good.

Against that, it introduced **11 unsupported claims, 5 C4 violations, and 5
invented references** across 14 questions. The deterministic pipeline produced
zero of any of those, because it does not make claims.

The most important finding is not about the agent at all: **23.5% of jj's and
26.0% of gigatoken's indexed file paths no longer exist at HEAD**, and Atlas
ranks these dead paths at the very top of results (scores 0.84–0.90). This
corrupts both arms on at least 5 of 14 questions and is the highest-value fix
available.

## 1–4 · Scores

| | deterministic | Qwen agent |
|---|---|---|
| **Overall (14 q)** | **49.5** | **50.1** |
| jj (7 q) | 42.7 | **58.1** |
| gigatoken (7 q) | **56.3** | 42.1 |

## 5 · Per-question results

Score is the mean of applicable axes. `A`/`B` = deterministic / agent.

| Q | A | B | verdict |
|---|---|---|---|
| J1 orientation | 38 | 32 | both fail. A returns `local_working_copy.rs`, `diff_util.rs`; B reports `config, default_index, diff_presentation, lock, protos` — the only *directories* under `lib/src` — as "the major subsystems", missing backend, repo, op log, revsets entirely |
| J2 architecture | 40 | 38 | both fail. A ranks `lib/gen-protos/src/main.rs` #1. B names real hot files but invents roles ("`diff_presentation` renders UI elements"), misses the lib/cli layering and the `Backend` trait |
| J3 concurrency bug | 8 | 62 | **A total retrieval failure** — top-5 are `src/commands.rs`, `src/commands/git.rs`, `src/commands/bench.rs`: paths deleted from jj years ago. No `op_heads_store.rs`, no `lock/`. B found `lock/mod.rs`, `lock/windows.rs`, the real commit `bcdf3e942` ("lock: retry Windows file creation on ERROR_SHARING_VIOLATION") and the `lock_concurrent` test — but asserted a **Windows** root cause with nothing establishing the platform, and missed `op_heads_store.rs` (the actual divergent-op-heads mechanism) |
| J4 commit flow | 68 | 55 | A retrieves a good neighborhood (`commit.rs`, `commit_builder.rs`, `cli_util.rs`) but is a neighborhood, not a flow. B gives 4 of 8 gold stages, below the ≥5 bar; both miss `transaction.rs` → `op_store.rs` |
| J5 intent vs impl | 12 | 78 | **A fails completely** (stale `src/commands*.rs` again; never surfaces `docs/design/run.md` or `run.rs`). **B's clearest win**: found the design doc, read it, enumerated real requirements, and correctly separated intent from `cli/src/commands/run.rs` |
| J6 change impact | 88 | 80 | both good. A is the best deterministic result in the suite — `backend.rs` #2 with all four implementors. B adds *why* (co-change counts, "implements the trait") but retrieves less |
| J7 adversarial (conflicts) | 45 | 62 | **A falls for the trap**: `conflicts.rs` #2, `conflict_labels.rs` #3, correct `resolve.rs` buried at #4 under `config_resolver.rs` #1. B answers `resolve.rs` correctly — but misses `merge_tools/` and mischaracterises commits `9b9656d06`/`5be4d4acf` (async refactors) as "explicitly modify this file for conflict resolution workflows" |
| G1 orientation | 15 | 42 | A returns essentially nothing (2 items, `examples/quickstart.py`). B names pretokenize/bindings/input but ranks by commit count and misses `bpe`, `load_tokenizer`, `batch.rs` |
| G2 architecture | 30 | 45 | A returns only `src/main.rs` + commits. B gets the Rust-core/Python-bindings shape and the input→pretokenize→bpe pipeline, then **invents an "Output Layer (`output` module)"** that does not exist |
| G3 CPU-dependent bug | 82 | 35 | **A is strong** — `reference/state_machine.rs`, `fast/cl100k.rs`, `fast/mask.rs`. **B regresses**: cites `src/pretokenize/simd.rs` and `pretoken_avx512.rs` (both deleted) and old commits "Comment out pretokenize SIMD code" / "Unused simd module" as the current mechanism — contradicted by the live `is_x86_feature_detected!` gates in `fast/mask.rs` |
| G4 encode flow | 80 | 45 | A retrieves the pipeline well (`batch.rs` #1, `input/mod.rs`, `bpe/*`). B cites `src/encode/mod.rs` (does not exist) and calls `examples/encode_files.py` "the Python API entry point" |
| G5 from_tiktoken | 30 | 28 | **both fail.** Gold core is `src/load_tokenizer/tiktoken.rs`, `src/bpe/tiktoken.rs`, `gigatoken/_load/tiktoken.py`. A ranks `pretokenize/*` top and finds only `tests/test_from_tiktoken.py` (#6). B guesses "`gigatoken/_tokenizer.py` or `src/token.rs`" and **fabricates a GitHub URL** (`github.com/sanoy/gigatoken`; the real remote is `marcelroed/gigatoken`) |
| G6 change impact | 85 | 82 | both good and closely matched; B adds relationships and co-change counts |
| G7 adversarial (startup) | 72 | 18 | **A resists the trap** — `load_tokenizer/hub.rs` #3, `mod.rs` #4, `hf.rs` #6 (though dead paths `gigatok/_tokenizer.py`, `jeton/_tokenizer.py` take #1/#2). **B walks straight into it**: answers `src/pretokenize` as the *cause* of slow start-up, justified by "75 commits, highest among modules". No timing evidence exists anywhere in the corpus |

## 6 · Deterministic vs agent

The agent wins exactly where the question needs a **second hop through
documentary evidence**: J5 (read the design doc), J3 (open `lock/mod.rs` and
find the Windows retry commit). Both are cases where the deterministic packet
was empty or wrong and the agent's `read_file`/`atlas_search` recovered.

The agent loses where deterministic retrieval was already correct (G3, G4, G7).
In each case it took a *subset* of the available evidence, picked the most
lexically attractive item, and asserted a conclusion. On G7 it inverted a
correct deterministic result into a wrong causal claim.

## 7–8 · Cost

| | deterministic | agent |
|---|---|---|
| mean latency | 0.97s | 116.7s |
| mean tool calls | n/a | 2.2 |
| single-call answers | n/a | 4 of 14 |
| max | 2.2s | 227.8s (G4) |

## 9–10 · Did the agent find anything new?

- **Genuinely new evidence: 2 of 14** — J3 (`lock/windows.rs`, commit
  `bcdf3e942`, `lock_concurrent` test, none of which appear in the deterministic
  packet) and J5 (contents of `docs/design/run.md`).
- **Marginal: 2** — J6 and G6 add co-change counts and stated relationships, but
  over the same file set Atlas already ranked.
- **Pure paraphrase or a strict subset: 7** — J1, J2, G1, G2, G3, G4, G7.
- **Refinement behaviour:** only G4 re-queried the same tool with new arguments.
  In 4 questions the agent made a single call and answered.

## 11–13 · Discipline failures

**11 unsupported claims:** commit-count⇒importance (J1, G1, G7); "`diff_presentation`
renders UI elements" (J2); Windows root cause (J3); commits mischaracterised as
conflict-resolution work (J7); invented `output` module (G2); "SIMD is commented
out" as current state (G3); `examples/` as the API entry point (G4); fabricated
commit URL (G5); "`pretokenize` causes slow start-up" (G7).

**5 C4 violations** (association presented as causation): J1, J3, J7, G1, G7.
G7 is the worst: a confident cause with zero timing evidence in the corpus, where
the pre-registered correct verdict was UNRESOLVED. G3 additionally violates the
provenance rule — older evidence about a *removed* SIMD module overrode the
current implementation.

**5 invented references**, of two distinct kinds:
- *Pure fabrication (2):* the `github.com/sanoy/gigatoken` URL; the `output` module.
- *Dead paths presented as current (3):* `src/pretokenize/simd.rs`,
  `pretokenize/pretoken_avx512.rs`, `src/encode/mod.rs`. These are real
  historical paths **served by Atlas**, which the agent did not verify.

## 14 · Top failure modes

1. **Atlas serves paths that no longer exist, and ranks them first.** 215/915
   jj files (23.5%) and 60/231 gigatoken files (26.0%) are absent at HEAD.
   `src/commands.rs` scores 0.84 on J3/J5; `gigatok/_tokenizer.py` and
   `jeton/_tokenizer.py` (a *former project name*) take #1/#2 on G7. This single
   defect damages J3, J5, G3, G4, G7 across both arms.
2. **Orientation is unanswerable** because `atlas map` defines modules as
   immediate child directories. jj's `lib/src` is 88 flat `.rs` files, so the
   "modules" are `config, lock, protos, default_index, diff_presentation` — an
   artifact, not an architecture. Both J1 and G1 fail on this.
3. **The agent under-uses its tools** — 2.2 calls mean, 4 single-call answers,
   one instance of refinement in 14.
4. **The agent converts association into causation** whenever the question is
   phrased causally (J3, G7 especially).
5. **No call graph** — flow questions (J4, G4) can retrieve the right
   neighborhood but cannot order it into stages.

## Failure classification (one primary class each)

| class | count | questions |
|---|---|---|
| **A · retrieval** | 5 | J1(A), J3(A), J5(A), G1(A), G5(both) |
| **B · ranking** | 3 | J7(A), G7(A, dead paths on top), G5(A) |
| **C · structural** | 2 | J4(both), G4(A) |
| **D · historical/provenance** | 2 | G3(B), G4(B) |
| **E · agent** | 3 | J1(B), J2(B), G1(B) |
| **F · reasoning/verification** | 5 | J3(B), J7(B), G2(B), G5(B), G7(B) |
| **G · gold/scope** | 0 | — |

Note the asymmetry: **deterministic failures are A/B (retrieval and ranking);
agent failures are E/F (tool use and unsupported reasoning).** They are not the
same problem, and adding the agent did not address the A/B failures — it
inherited them.

## Decision gates

**GATE 1 — does Qwen improve overall performance?** **No.** 50.1 vs 49.5 is
noise. The honest statement is that it trades gigatoken accuracy for jj
accuracy at 120× the latency.

**GATE 2 — does it fix the multi-hop failures behind the previous ceiling?**
**Partially, and only one kind.** It solves *documentary* multi-hop (J5, J3):
find a doc or a file, open it, reason over its contents. It does not solve
*structural* multi-hop (J4, G4 flows). And it introduced regressions (G3, G7)
that the deterministic pipeline did not have.

**GATE 3 — new evidence or paraphrase?** **Mostly paraphrase.** 2 of 14 clearly
new, 7 of 14 a strict subset of what deterministic already returned.

**GATE 4 — does it introduce unsupported claims or C4 violations?** **Yes,
severely.** 11 unsupported claims, 5 C4 violations, 5 invented references, from
zero in the deterministic arm. This is the strongest evidence in the study and
it points one way: prose generation adds a failure mode Atlas did not have.

**GATE 5 — does this justify C5.2 structural/AST retrieval?** **No — not yet.**
The evidence does not support it as the next move. Only 2 of 14 failures (J4,
G4 flows) are class C. Five are caused by ranking dead files, which is a
`WHERE path exists at HEAD` problem, not an AST problem. Building symbol-level
retrieval on top of an index where a quarter of the paths are dead would make
the wrong answers more precise, not more correct.

## Final verdict

**1. Is the deterministic pipeline generalising beyond RWATP?**
Unevenly. It is genuinely good at *bounded, well-named* questions in both repos
— J6 (88), G6 (85), G3 (82), G4 (80). It collapses on orientation (J1 38, G1 15)
and on anything where history has moved files (J3 8, J5 12). RWATP flattered it:
RWATP is young, with stable paths and a strict `src/modules/<name>` convention.
jj's 11,503 commits of refactoring and gigatoken's two renames break assumptions
that RWATP never tested.

**2. Does the Qwen3:4b agent materially improve Atlas?**
No. It materially changes *where* Atlas fails, and adds an unsupported-claims
failure mode. On these 14 questions it is not a net improvement.

**3. Which problems does the agent solve?**
Reading a design document and separating documented intent from current
implementation (J5); opening a specific file to recover evidence that retrieval
ranked away (J3); attaching *reasons* to change-impact lists (J6, G6); and
resisting one lexical trap that deterministic ranking fell for (J7).

**4. Which remain unsolved?**
Orientation on flat-layout repos; end-to-end flow ordering; dead-path
contamination; and `from_tiktoken`-style questions (G5) where both arms failed.

**5. Is Qwen useful as the investigation/orchestration layer today?**
Not as an answerer. It is defensible as an *evidence-gatherer* whose output a
human reads — its tool selection was reasonable in most cases, and its
`read_file` follow-ups are what produced both wins. It should not be trusted to
state conclusions, and nothing it says should be persisted as repository truth.
Atlas's existing rule — never promote model text to evidence — held up and
should be tightened, not relaxed.

**6. Should we build C5.2 structural/AST retrieval next?**
No. Revisit after the dead-path defect is fixed and orientation works, then
re-measure. Flow questions are the only genuine case for it and they are 2 of 14.

**7. Should we expand the agent loop?**
Not by making it longer. The failures are not "ran out of steps" — mean 2.2
calls against a 10-step budget. The binding constraints are verification and
grounding, not depth. If anything changes in the loop, it should be a
verification pass that checks every path the model names against the current
tree before the answer is emitted, and forces an explicit SUPPORTED / PLAUSIBLE
/ UNRESOLVED label on any causal claim.

**8. Single highest-value next engineering change.**
**Stop ranking files that no longer exist at HEAD.** Atlas already stores enough
to know (`files`, rename evidence, commit history); it simply does not use it at
ranking time. This is deterministic, cheap, testable, and it damages five of
fourteen questions across two unrelated repositories and both arms
simultaneously. Every other candidate change — agent verification, orientation
redesign, AST retrieval — is worth less until the index stops recommending
deleted code.
