---
title: Qwen3-4B thinking mode as an Atlas reasoning layer — measured study
date: 2026-08-10
hardware: RTX 3050 6GB Laptop GPU · 15GB RAM · 12 cores · NixOS
stack: Ollama 0.32.6 (ollama-cuda) · qwen3:4b Q4_K_M
---

## Summary

A 4B model cannot know your repository. It can, however, *operate* an evidence
engine that does. Every measurement below points the same direction: spend the
model's small budget on tool selection and short synthesis, and let Atlas carry
the facts.

Three findings drive every configuration choice in this document:

1. **12288 is the context ceiling on 6GB.** At 12k all 37/37 layers stay on the
   GPU at ~52 tok/s. At 24k+ fourteen layers spill to CPU and throughput halves.
2. **Thinking is extremely verbose.** 6,241–10,832 characters of reasoning for a
   question whose answer is three sentences. Thinking, not evidence, is what
   actually consumes the window.
3. **Tool calling works correctly** on this stack, including alongside thinking —
   the upstream Qwen3 tool-call bugs are fixed in 0.32.6.

## What this model actually is

`ollama show qwen3:4b` on this machine:

```
architecture qwen3      parameters 4.0B     context length 262144
quantization Q4_K_M     capabilities: completion, tools, thinking
default params: temperature 0.6, top_k 20, top_p 0.95, repeat_penalty 1
```

The 262,144-token native context and the baked-in `temperature 0.6 / top_p 0.95
/ top_k 20` identify this as the 2507-lineage thinking build, not the original
hybrid Qwen3-4B. The defaults already match [Qwen's published thinking-mode
sampling](https://huggingface.co/Qwen/Qwen3-4B) — which matters, because
anything that overrides them is fighting the model.

Published benchmarks for [Qwen3-4B-Thinking-2507](https://huggingface.co/Qwen/Qwen3-4B-Thinking-2507):

| Benchmark | Score |
|---|---|
| AIME25 | 81.3 |
| HMMT25 | 55.5 |
| GPQA | 65.8 |
| LiveCodeBench v6 | 55.2 |
| BFCL-v3 (tool use) | 71.2 |
| Arena-Hard v2 | 34.9 |

BFCL-v3 at 71.2 is the number that matters here: tool *selection* is this
model's strongest relevant skill, and open-ended judgement (Arena-Hard 34.9) is
its weakest. That asymmetry is the whole design brief — route it toward
choosing tools, away from free-form conclusions.

The 262K context is real but largely unusable locally: it is a property of the
weights, not of your VRAM. See the ceiling below.

## Measurement 1 — context size vs. throughput

Fixed prompt, thinking enabled, model unloaded between runs.

| num_ctx | GPU layers | throughput | note |
|---|---|---|---|
| 4096 | 37/37 | 52.2 tok/s | |
| 8192 | 37/37 | 51.9 tok/s | |
| **12288** | **37/37** | **51.9 tok/s** | **sweet spot** |
| 16384 | 35/37 | 46.0 tok/s | first spill |
| 24576 | — | 24.0 tok/s | |
| 32768 | 23/37 | 25.7 tok/s | 1792 MiB KV on CPU |

KV cache growth is what pushes layers off the GPU: 576 MiB at default, 2176 MiB
at 16k, 2816 MiB at 32k — against ~5675 MiB of usable VRAM shared with a 2376
MiB model.

**Consequence:** set `num_ctx` explicitly to 12288. Ollama's default window is
far smaller than an Atlas evidence packet, and an overflowing packet is
*silently truncated* rather than rejected — the failure looks like the model
being stupid, not like an error.

`OLLAMA_FLASH_ATTENTION=1` plus `OLLAMA_KV_CACHE_TYPE=q8_0` halves KV memory at
negligible quality cost and should move the full-offload ceiling to roughly 24k.
Flash attention is a hard prerequisite: without it the cache-type setting is
silently ignored. Verify with `journalctl -u ollama | grep offloaded` — if it
does not say 37/37, the window is too big.

## Measurement 2 — thinking verbosity

Same trivial prompt ("explain in three sentences why append-only logs simplify
crash recovery"), thinking trace length:

```
6,241 · 7,953 · 7,045 · 1,077 · 10,832 · 986 characters
```

Generation ran to 1,268–2,119 tokens for a three-sentence answer. Thinking is
unbounded and does not scale down with question difficulty — it scales with how
much room you give it.

**Consequence:** the budget is not "12k for evidence". It is roughly *4-6k for
evidence and 6-8k for thinking*. This is why tool output must be capped; the
agent truncates any tool result past 2,600 characters and tells the model to
narrow its query rather than silently dropping evidence.

Ollama exposes `"think": "low" | "medium" | "high" | "max"` as well as
`true`/`false` for Qwen3, which is the correct lever when reasoning crowds out
evidence — prefer it to disabling thinking outright.

## Measurement 3 — tool calling

Verified directly against `/api/chat` on 0.32.6 with a `tools` array and
`"think": true`:

```json
"thinking": "…There's a function called get_weather that takes a city name…",
"tool_calls": [{"function": {"name": "get_weather", "arguments": {"city": "Kochi"}}}]
```

Well-formed. The known upstream failures — [issue #14601](https://github.com/ollama/ollama/issues/14601):
tool definitions serialised as Go structs, prior tool calls stripped from
history, redundant `/think` injection — were reported against 0.17.5 and do not
reproduce here. **Do not apply the "embed tools in the system prompt" workaround
that circulates for this**; it is obsolete on 0.32.6 and strictly worse.

One requirement is real and easy to get wrong: the assistant turn carrying
`tool_calls` must be replayed into the message list *before* the tool results,
or the model loses track of what it asked for.

## Measurement 4 — the agent against Atlas

`agent/bench_agent.py`, six questions over rwatp-core, scored on three axes that
fail independently: *grounded* (called any tool at all), *tool_fit* (first tool
was a sensible choice), *correct* (final answer named the expected path).

| case | grounded | tool_fit | correct | steps | latency | first tool |
|---|---|---|---|---|---|---|
| support-unread | ✓ | ✓ | ✓ | 4 | 58.8s | atlas_search |
| rbac-permissions | ✓ | ✓ | ✓ | 2 | 25.7s | atlas_search |
| kyc-flow | ✓ | ✓ | ✓ | 2 | 19.6s | atlas_search |
| hottest-file | ✓ | ✓ | ✓ | 2 | 32.1s | atlas_map |
| coupling | ✓ | ✓ | ✓ | 2 | 21.9s | atlas_map |
| blast-radius | ✓ | ✓ | ✓ | 2 | 24.1s | atlas_impact |
| **aggregate** | **6/6** | **6/6** | **6/6** | **2.3** | **30.4s** | |

Tool selection was correct in every case, and correct for the right reasons:
`atlas_search` for "where is X", `atlas_map` for repository-wide properties,
`atlas_impact` for a blast-radius question. This is the BFCL-v3 71.2 number
showing up in practice.

### The truncation finding

The first run of this benchmark scored 5/6, failing `kyc-flow`. The model was
not at fault. A flat 2,600-character cap on tool output cut `atlas search kyc`
(15,363 chars) at char 2,600, while the answer-bearing line —
`src/modules/compliance/services/kyc-flow-engine.service.ts` — sits at char
3,410. The model reasoned correctly over evidence from which the answer had
been deleted, and confidently returned the adjacent `kyc-flow.service.ts`.

Replacing the flat cap with per-tool budgets (`atlas_search` 6000,
`atlas_map` 5000, narrowed views 3500-4000) fixed the case *and lowered mean
latency from 35.8s to 30.4s* — the model spent less time reasoning around
missing evidence than it had spent reading the extra evidence.

**The general lesson: for a small model, truncating tool output is a
correctness decision, not a cost decision.** Truncation converts "I could not
find it" into a confident wrong answer, which is the worst available failure
mode. The truncation notice now explicitly instructs the model to re-query
rather than conclude absence.

## How to use it intelligently with Atlas

**Give it Atlas's narrow commands, not its broad ones.** `atlas_search`,
`atlas_focus`, `atlas_impact`, `atlas_explain` and `atlas_show` return bounded,
already-ranked evidence. A 4B model handles "here are 15 ranked files" well and
"here is the whole repository" badly.

**Force singular anchor terms.** Atlas matches anchors against paths, not
sentences. `support ticket` localises correctly at rank 1; the full question
does not. The agent's system prompt states this explicitly because the model
will otherwise pass prose straight through.

**Never let it answer from weights.** The system prompt's first rule is that an
un-tooled answer is by definition a guess. This is the single highest-leverage
instruction for a small model.

**Use `atlas investigate --no-ai` for retrieval and the model for synthesis.**
Atlas's deterministic retrieval is stronger than the model's judgement. Let
Atlas decide *what is relevant* and the model decide *how to say it*.

### Atlas's own Ollama provider

`crates/core/src/ai_provider.rs` was sending `temperature: 0.1` for every call,
including thinking calls. Qwen documents that near-greedy decoding degrades
thinking models and induces endless repetition — this was actively harmful. It
also never set `num_ctx`, so packets were being truncated into whatever
Ollama's default window happened to be.

Both fixed: thinking calls now use Qwen's 0.6/0.95/20/min_p 0 preset,
non-thinking calls keep the low temperature that makes structured JSON stable,
and `num_ctx` is explicit and configurable.

| variable | default | meaning |
|---|---|---|
| `ATLAS_OLLAMA_MODEL` | qwen2.5-coder:7b-instruct | set to `qwen3:4b` to use the thinking model |
| `ATLAS_OLLAMA_NUM_CTX` | 12288 | measured full-offload ceiling |
| `ATLAS_OLLAMA_NUM_PREDICT` | 4096 for qwen3 | must exceed the thinking trace |
| `ATLAS_OLLAMA_THINK` | on for qwen3 | `0` to disable |
| `ATLAS_OLLAMA_TIMEOUT` | 180 | raise for multi-round investigations |

## NixOS

The working config is already correct in one important respect — `package =
pkgs.ollama-cuda` gives genuine CUDA offload, confirmed by
`CUDA : ARCHS = 750,800,860,890,900,…` and `offloaded 37/37 layers` in the
service log. What it lacks is memory and residency tuning.

`agent/nixos/nvidia-ollama.nix` is a drop-in replacement adding flash attention,
q8_0 KV cache, single-model/single-slot limits (6GB holds exactly one 4B model),
a 30-minute keep-alive so agent loops stop paying a ~2s reload per call, and
`loadModels = ["qwen3:4b"]` so a rebuild never leaves you without the model.

Apply with `sudo nixos-rebuild switch`, then confirm:

```bash
journalctl -u ollama | grep -E "offloaded|KV buffer"   # want 37/37
ollama ps                                              # want 100% GPU
```

## Honest limits

- **34.9 on Arena-Hard v2.** Open-ended judgement is weak. Do not ask this model
  whether an architecture is *good*; ask it what the evidence *says*.
- **Latency is real.** ~46s for a 3-step investigation; a 10-step one runs into
  minutes. This is a background tool, not an autocomplete.
- **The 262K context is theoretical here.** 12k is the practical ceiling on 6GB,
  ~24k with a quantised cache.
- **Thinking cannot be trusted as output.** It is a scratchpad. Atlas's existing
  discipline — verify model claims against evidence, never persist model text as
  repository truth — is the correct posture and should not be relaxed.

## Sources

- [Qwen3-4B-Thinking-2507 model card](https://huggingface.co/Qwen/Qwen3-4B-Thinking-2507)
- [Qwen3-4B model card](https://huggingface.co/Qwen/Qwen3-4B)
- [Ollama thinking capability docs](https://docs.ollama.com/capabilities/thinking)
- [Ollama issue #14601 — Qwen3 tool calling](https://github.com/ollama/ollama/issues/14601)
- [NixOS Wiki — Ollama](https://wiki.nixos.org/wiki/Ollama)
- [Bringing K/V context quantisation to Ollama](https://smcleod.net/2024/12/bringing-k/v-context-quantisation-to-ollama/)
