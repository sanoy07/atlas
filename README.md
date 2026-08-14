# Atlas

**Local developer knowledge engine.**  
Ingest a repository → query a deterministic evidence graph → optional local AI reasons over sealed packets.  
You implement. Atlas never pretends the model is the source of truth.

```
Repo  →  parsers / git / structure  →  SQLite evidence
                                         ↓
                              map · focus · impact · investigate
                                         ↓
                         local Ollama (optional) over ranked evidence
                                         ↓
                              plans · checklists · synthesis
                                         ↓
                                    you ship the code
```

## Install (this machine)

```bash
cd ~/projects/atlas
make install          # builds release → ~/.local/bin/atlas

cd /any/repo
atlas init            # creates <git root>/atlas.db, ignores it, first ingest
atlas status          # doctor: DB, git, evidence freshness, Ollama models
```

Ensure `~/.local/bin` is on your `PATH` (Fish on NixOS: already wired via `modules/shell/fish`).

### Models (6GB laptop GPU defaults)

```bash
ollama pull qwen3:4b                      # reasoning / investigate
ollama pull qwen2.5-coder:7b-instruct     # prose synthesis + plan
# optional:
ollama pull nomic-embed-text
```

| Variable | Default | Role |
|----------|---------|------|
| `ATLAS_DB` | `<git root>/atlas.db` | SQLite path — override wins verbatim (multi-repo / eval DBs) |
| `ATLAS_OLLAMA_URL` | `http://localhost:11434` | Ollama |
| `ATLAS_OLLAMA_MODEL` | `qwen3:4b` | Reasoning investigation |
| `ATLAS_OLLAMA_SYNTHESIS_MODEL` | `qwen2.5-coder:7b-instruct` | Prose / plan |
| `ATLAS_OLLAMA_NUM_CTX` | `12288` | Full-GPU safe on 6GB |
| `ATLAS_OLLAMA_THINK` | auto for qwen3* | `0` to disable thinking |

## Daily workflow

Commands work from **anywhere inside the repository** — the database is anchored
at the git root, not the current directory.

```bash
# 1. Refresh evidence (structural extractors are auto-detected)
atlas ingest .
# atlas ingest . --github     # + issues/PRs (needs gh or token)
# `atlas status` reports when the graph has fallen behind HEAD.

# 2. Orient
atlas map
atlas modules
atlas capabilities            # infrastructure capabilities + product surfaces
atlas tree --depth 2

# 2b. Locate (deterministic structural queries — no model, ~10–50ms)
atlas code-search ListingAsset          # ranked DEFINITION/WIRING/CALL_SITE/REFERENCE/TEST
atlas callers tryEnqueue                # who calls this symbol (prod before tests)
atlas implementations IStorageProvider  # OBSERVED `implements` edges preferred

# 3. Investigate (deterministic + optional synthesis)
atlas investigate "orders timeout under concurrency"
atlas investigate auth order --no-ai          # facts only
atlas investigate --file path/to/service.ts
atlas investigate --issue 42

# 3b. Agentic explore (local Ollama — read-only tool loop)
#     Tools: atlas_* · ripgrep · read_file · web_search/fetch
#     Needs: python3, Ollama (qwen3:4b), atlas.db or ATLAS_DB
atlas agent "where is order fulfillment handled?"
atlas agent --no-web "how does SIWE auth attach the investor?"
atlas agent --fast "order"                    # investigate --no-ai only

# 4. Implement with evidence
atlas focus src/modules/orders
atlas impact src/modules/orders/order.service.ts
atlas conventions src/modules                 # peer patterns for new modules
atlas plan 42                                 # issue → human checklist / snippets

# 5. Drill down
atlas show path/to/file.ts
atlas structural path/to/file.ts --reverse
atlas search order timeout
```

**Rule of thumb:** known subject → `--no-ai` first. Vague question → full investigate. Causal claims in model prose are capped by C4 verification.

## Philosophy (short)

1. **Deterministic by default** — same repo state → same evidence.  
2. **Evidence, not opinions** — every entity traces to an artifact.  
3. **AI is a consumer** — never writes the knowledge graph.  
4. **Explainable** — ranked files and claim status, not black-box scores.  
5. **Local** — SQLite on disk; no telemetry; cloud is opt-in and not default.

Full constitution: [`docs/atlas-philosophy.md`](docs/atlas-philosophy.md)  
Methodology: [`docs/atlas-methodology.md`](docs/atlas-methodology.md)

## Build from source

```bash
nix develop          # or any Rust + sqlite + git + gh toolchain
cargo build --release -p atlas
cargo test --workspace
```

## Agent (optional)

For multi-step tool use over Atlas commands with a C4 final gate:

```bash
python3 agent/atlas_agent.py "Users see inconsistent state — where to look?"
python3 agent/atlas_agent.py --fast "If I change lib/src/op_store.rs, what else?"
```

See [`agent/README.md`](agent/README.md).

## Fish helpers

```fish
source ~/projects/atlas/shell/atlas.fish
# or rely on system fish config after nixos-rebuild
ai          # atlas investigate -- …
amap        # atlas map
astatus     # atlas status
```
