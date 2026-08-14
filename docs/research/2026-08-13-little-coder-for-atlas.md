# little-coder × Atlas

**Date:** 2026-08-13  
**Subject:** [itayinbarr/little-coder](https://github.com/itayinbarr/little-coder) as a harness complement to Atlas  
**Status:** Research note + integration path (not a product merge)

## What little-coder is

little-coder is a **coding agent harness optimized for small local models**, built on [pi](https://pi.dev):

- pi supplies the agent loop, multi-provider API, TUI, session tree, compaction, extension model
- little-coder adds ~30 pi extensions + skill markdown + a Python benchmark harness
- Designed for **scaffold–model fit** (their claim: small models jump a lot when the harness is tight)
- Ships with Ollama / llama.cpp / LM Studio providers, permission gates, skill injection, evidence tools, compaction watchdogs

It is **not** a repository intelligence engine. It is a **coding agent runtime**.

## Why it is promising for Atlas

| Atlas need | little-coder property |
|------------|------------------------|
| Small local models (Qwen 4B–35B) | Explicit design target |
| Tool surface matters more than model size | Validated by their polyglot/TB work and by our Atlas agent drills |
| Avoid letting the model invent repo structure | Permission + skill injection can **force** Atlas CLI first |
| Eval discipline | Python harness + per-benchmark profiles |
| Read-before-edit / write-guard | Safer if we ever allow write tools around Atlas evidence |

Atlas stays the **evidence substrate**. little-coder (or pi) can be the **action harness** that *uses* Atlas.

## What not to do

- Do **not** replace `atlas agent` with little-coder as the source of truth
- Do **not** dump the whole repo into little-coder context and skip C4
- Do **not** treat their Aider Polyglot numbers as Atlas understanding scores

## Integration paths (ordered)

### A. Skill card (lowest friction) — shipped under `integrations/little-coder/`

A markdown skill that teaches the model:

```text
For repository structure / history / callers / storage surfaces:
  atlas investigate "…"
  atlas callers <symbol>
  atlas capabilities
  atlas implementations <Interface>
  atlas code-search <symbol>
Never answer product architecture from grep alone when atlas is installed.
```

### B. Bash allowlist

```bash
export LITTLE_CODER_BASH_ALLOW="atlas ,atlas"
```

So little-coder’s permission gate allows Atlas CLI without freeform shell.

### C. Custom pi extension (later)

A thin extension that registers MCP-like tools:

- `atlas_investigate`, `atlas_callers`, `atlas_capabilities`, …

wrapping the same CLI Atlas agent already uses. Prefer CLI over reimplementing SQLite access.

### D. Dual-bench

Keep Atlas `eval/code-intel-bench.sh` for **understanding**.  
Use little-coder benchmarks only if measuring **edit success** with Atlas tools available.

## Recommendation

1. Ship Atlas code-intel CLI first (callers / capabilities / …) — done in this phase.
2. Add little-coder skill + allowlist so Qwen-in-little-coder can call Atlas.
3. Only after that, consider pi extension parity with `atlas_agent.py` tools.

Atlas differentiator remains: **evidence, provenance, epistemic status** — not the agent loop.
