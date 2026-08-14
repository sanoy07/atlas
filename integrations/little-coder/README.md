# Atlas ↔ little-coder integration

[little-coder](https://github.com/itayinbarr/little-coder) is a **small-model coding harness** (pi + extensions).
Atlas is the **evidence substrate**. This folder wires them without merging products.

## Quick setup

```bash
# 1. Atlas on PATH + DB for the repo you care about
export PATH="$HOME/.local/bin:$PATH"
export ATLAS_DB=/path/to/atlas.db   # after: atlas ingest . --typescript

# 2. Allow atlas in little-coder's bash gate
export LITTLE_CODER_BASH_ALLOW="atlas "

# 3. Install skill for the agent
mkdir -p ~/.config/little-coder/extensions
# skill injection: copy skill into little-coder skills or project AGENTS.md
cp skills/atlas-evidence.md /path/to/your-project/ATLAS.md
# Or append a pointer in AGENTS.md:
#   See ATLAS.md for repository evidence tools.
```

Optional: install little-coder itself:

```bash
npm install -g little-coder   # needs Node ≥ 22.19
export OLLAMA_API_KEY=noop
little-coder --model ollama/qwen3:4b
```

## What ships here

| Path | Purpose |
|------|---------|
| `skills/atlas-evidence.md` | Skill card: when/how to call Atlas CLI |
| `extensions/atlas-bash-hint.ts` | Hint-only extension sketch |
| `extensions/atlas-tools.ts` | Tool wrappers that shell to `atlas` CLI |
| `../../docs/research/2026-08-13-little-coder-for-atlas.md` | Research write-up |

### Wire tools (optional)

```bash
export LITTLE_CODER_EXTRA_EXTENSIONS="$HOME/projects/atlas/integrations/little-coder/extensions/atlas-tools.ts"
export LITTLE_CODER_BASH_ALLOW="atlas "
export ATLAS_DB=/path/to/atlas.db
little-coder --model ollama/qwen3:4b
```

If pi rejects the extension schema, use the skill + bash allowlist only — same CLI underneath.

## Benchmark

Atlas understanding benchmarks stay in-repo:

```bash
./eval/code-intel-bench.sh full
```

Do not use little-coder polyglot scores as Atlas localization scores.
