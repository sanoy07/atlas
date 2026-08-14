# atlas_agent v2 — Qwen orchestration over Atlas

Gives a small local model (`qwen3:4b` by default) **tool access to Atlas**, not a
second memory of the repository.

## Architecture (locked)

```text
User's vague question
        ↓
      Qwen          "What should I inspect?"
        ↓
 Atlas tools / atlas_investigate
   (C5.1-S → R → L → E → rank → C4 packet)
        ↓
      Qwen          "What does this evidence mean?"
        ↓
     C4 FINAL GATE
        ↓
 Final answer
```

**Not:** `Qwen → grep → read random files → believe Qwen`.

| Layer | Role |
|-------|------|
| **Deterministic Atlas** | Evidence engine. Anchored questions: use `atlas investigate --no-ai` (~1s). |
| **Qwen agent** | Exploration / orchestration for cold-start and multi-step questions. |
| **C4** | Causal claims cannot become factual prose without support. |

## Requirements

- Ollama with a tool-capable model (`qwen3:4b` lists `tools` + `thinking`)
- `atlas.db` in the target repo
- Atlas binary (`cargo build --release`)

## Use

### Preferred: via Atlas CLI

```bash
cd /path/to/repo   # or set ATLAS_DB for multi-repo projects
atlas agent "Users see inconsistent state under concurrent operations — where to look?"
atlas agent --no-web "where is OrderService?"
atlas agent --fast "order"              # deterministic investigate only
atlas agent --show-thinking "how auth works"
```

### Direct Python

```bash
cd /path/to/repo-with-atlas.db
python3 /home/sanoy/projects/atlas/agent/atlas_agent.py \
  "Users see inconsistent state under concurrent operations — where to look?"

# Fast deterministic only (~1s) — preferred when the subject is already known
python3 …/atlas_agent.py --fast "If I change lib/src/op_store.rs, what else?"

# Or call Atlas directly
atlas investigate "…" --no-ai
```

| variable | default | meaning |
|---|---|---|
| `ATLAS_BIN` | `…/target/release/atlas` | atlas binary (`atlas` on PATH also works) |
| `ATLAS_DB` | `./atlas.db` | evidence DB (multi-repo projects set this) |
| `OLLAMA_URL` | `http://localhost:11434` | Ollama |
| `AGENT_MODEL` | `qwen3:4b` | tool-capable model |
| `AGENT_NUM_CTX` | `12288` | GPU-safe context on 6GB |
| `ATLAS_AGENT_WEB` | `0` (fish default) / `1` in script | set `0` to disable web_search / web_fetch |
| `ATLAS_AGENT_THINK` | `0` | set `1` for Qwen thinking (slower, better hard reasoning) |
| `ATLAS_AGENT_FETCH_MAX` | `80000` | max HTTP body bytes for web_fetch |

## Tools

All tools are **read-only**. There is no shell, no write, no git mutate.

**Primary:** `atlas_investigate` — full C5.1 + C4 evidence packet  
**Orientation:** `atlas_map`, `atlas_modules`, `list_dir`  
**Narrow:** `atlas_search`, `atlas_focus`, `atlas_impact`, `atlas_explain`, `atlas_show`, `atlas_cohorts`  
**Working tree:** `read_file`, `ripgrep` (advanced), `grep` (alias), `git_log`  
**Internet (optional):** `web_search` (free DuckDuckGo), `web_fetch` (HTTP GET, capped)

### Safety

- Path jail under `--repo`
- Denies `.env*`, credentials, keys, etc.
- `rg` skips `node_modules`, `dist`, `.git`, `.env*`
- Tool output char budgets (truncation is a correctness feature)
- Web tools: `ATLAS_AGENT_WEB=0` to disable

### Free web search dependency

```bash
pip install ddgs
# or: pip install duckduckgo-search
```

Without it, `web_search` falls back to a best-effort HTML scrape (may break).

## C4 final gate

When the model stops calling tools, the agent appends a **C4 FINAL GATE** block:

- Causal language → max **PLAUSIBLE** unless strongly multi-path grounded
- Certainty + thin evidence → demoted with an explicit warning
- Causal claim with no cited tool paths → **UNRESOLVED**
- Packet hypothesis statuses from `atlas_investigate` are treated as authoritative

A ranked file in a neighborhood is **never** silent license for “X causes Y.”

## Two traps

1. **Tool output budgets are correctness.** A flat 2600-char cap deleted gold mid-search. Budgets live in `TOOL_CHAR_BUDGET`.
2. **Replay assistant `tool_calls` before tool results** (already handled in the loop).

## Benchmark

```bash
python3 agent/bench_agent.py --repo /home/sanoy/Vesta/rwatp-core
```

Adversarial blind suite (JJ + GigaToken, gold frozen):

```bash
node eval/localization/adversarial-blind/run_eval.mjs
```

After agent v2, re-run that suite **without changing gold**.

## Docs

- Research: `docs/research/2026-08-10-qwen3-4b-thinking.md`
- Blind det vs agent: `docs/benchmarks/2026-08-10-adversarial-blind-det-vs-agent.md`
- Decision: `docs/decisions/2026-08-10-agent-v2-investigate-c4.md`
