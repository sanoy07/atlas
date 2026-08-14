# Cross-repo Atlas + Agent evaluation report

**Date:** 2026-08-13  
**Binary:** atlas 0.2.0 (`~/.local/bin/atlas`)  
**Agent:** `agent/atlas_agent.py` + `atlas agent` CLI  
**Model:** qwen3:4b (`num_ctx=12288`, web disabled for repo purity)

## Scope

| Product | Repositories |
|---------|----------------|
| **rwatp** | rwatp-core, rwatp-notifier, rwatp-console, rwatp-user-frontend |
| **vestascan** | vestascan-api, vestascan-notifier, vestascan-blockchain, vestascan-user-fe, vestascan-console |
| **research** | jj, gigatoken (`projects/research/`) |

## Iteration loop

| Round | Action | Result |
|-------|--------|--------|
| **1** | Ingest 3 projects + smoke suite (map/modules/investigate/agent_fast/rg/agent_full) | **60/60 hard OK** (no crashes) |
| **1 analysis** | Soft-fail: 8/11 `atlas modules` returned **0** under default `src/modules` while `map` found modules | **Quality bug** |
| **1 analysis** | `atlas status` showed “no ingest” after `project ingest` | **ingest_runs never written** |
| **2** | Auto-resolve modules subject (modules/coupling/conventions/anomalies/tests/cohorts) | **11/11 modules non-empty** |
| **2** | Project ingest writes `ingest_runs`; status path candidates | Status OK after CLI ingest; project path instrumented |
| **3** | Proof re-checks + this report | Closed |

## Hard suite proof (round 1)

- Results: `eval/cross-repo-suite/results-20260813-012847/`
- `proof.jsonl`: **ok=60 fail=0 skip=0**
- Ingest times (bottleneck #1):
  - research (jj+gigatoken, git): **~237s**
  - vestascan TS: **~85s**
  - rwatp TS: **~78s**
- Full agent (tool loop): **~45–47s** per question (Ollama bound)
- Deterministic investigate: **~50–300ms** typical; **~1.3s** on large jj

## Soft suite proof (round 2 — modules auto)

- Results: `eval/cross-repo-suite/results-post-fix-20260813-014029/`
- All 11 repos: **modules total > 0** after fix

Examples:

| Repo | Before | After |
|------|--------|-------|
| rwatp-notifier | total 0 | **16** under `src` |
| rwatp-console | total 0 | **14** under `src` |
| rwatp-user-frontend | total 0 | **9** under `src` |
| jj | total 0 | **5** under `lib/src` |
| gigatoken | total 0 | **8** under `src` |
| rwatp-core | 11 | 11 (unchanged `src/modules`) |

## Localization quality samples (manual)

| Repo | Question | Top hit | Verified |
|------|----------|---------|----------|
| rwatp-core | order fulfillment | `order-fulfillment.service.ts` | Yes — payment/signing call `tryEnqueue` |
| rwatp-notifier | ORDER_CREATED notify | `notify.handler.ts` + contracts | Yes — neighborhood correct |
| jj | workspace commit | `workspace_store.rs`, `commit.rs` | Plausible / good anchors |

## Fixes shipped this loop

1. **`resolve_modules_path_for_cli`** — default `src/modules` falls back to map’s layout heuristic with stderr note. Wired into **modules, coupling, conventions, anomalies, tests, cohorts**.
2. **`project ingest` → `ingest_runs`** — each repo starts/finishes a run with per-stage JSON so status can display history.
3. **`atlas status`** — multi-path lookup for repo realpath; multi-repo ingest hint.
4. **Eval harness** — `eval/cross-repo-suite/run_suite.sh` for repeatable proof.

## Remaining drawbacks / failures (capacity — not all “bugs”)

| Issue | Severity | Notes |
|-------|----------|-------|
| **Ingest latency** on large histories (jj) | High bottleneck | ~4 min research ingest; HEAD-only still large |
| **Full agent latency** | High | ~45s+/question on 4B + think; not interactive for many queries |
| **No Python on system PATH** | Medium UX | `atlas agent` falls back to `nix-shell -p python3` |
| **No `gh` → no GitHub documentary** | Medium quality | PRs/issues = 0; chronology thin on intent |
| **jj modules under-count** | Medium | Flat `lib/src/*.rs` files are not “child modules”; map same limitation |
| **Historical modules** (access-control, base) | Low | Still appear when present in `files` table |
| **Anomalies ≠ quality** | By design | Type safety / stub auth still need tsc + human |
| **No cross-repo edges** | By design | Project census only; Pub/Sub/GraphQL seams manual |
| **Web search flaky without `ddgs`** | Low | Instant Answer works; install `ddgs` for better hits |
| **Agent truncates tool output** | Correctness tradeoff | Budgeted; can hide gold if re-query not done |
| **status after project ingest** | Fixed in code | Needs re-run of project ingest to populate old DBs |

## Bottlenecks ranked

1. **Git history ingest** (especially monorepos / long history)  
2. **Ollama full agent loop** (thinking + multi-tool)  
3. **TypeScript structural parse** at scale  
4. **Python availability** for agent entry  
5. **Default module path mismatch** (fixed)

## How to re-run proof

```bash
export ATLAS_BIN=~/.local/bin/atlas
export ATLAS_AGENT_PYTHON=$(nix-shell -p python3 --run 'which python3')
export PATH="$(dirname $ATLAS_AGENT_PYTHON):$HOME/.local/bin:$PATH"
bash ~/projects/atlas/eval/cross-repo-suite/run_suite.sh
```

## Round 3 — “fix everything” (2026-08-13)

| Fix | Proof |
|-----|--------|
| Ghost modules dropped when not on disk | rwatp-core modules **9** (no access-control/base); discovery rule notes on-disk filter |
| Peer structure peers on-disk only | conventions PEERS: 9 live modules |
| Anomaly noise: @types/toolchain excluded | declared_dependency list no longer floods with typescript/@types/*; total 34 (was 53) |
| Agent: think off by default; symbol → ripgrep required | OrderFulfillmentService → **order-fulfillment.service.ts** (PROOF correct path); ~32–46s |
| Agent C4 remembers rg paths | ripgrep hits feed evidence_paths_cited |
| Suite quality gates | modules total>0; investigate must show evidence |
| NixOS: python3, gh, sqlite | packages.nix + fish.nix |
| Project ingest → ingest_runs | earlier; CLI ingest status shows LAST INGEST |

### Still not “everything in the universe”
- Typecheck / security audit still **out of capacity**
- Cross-repo graph still **not implemented**
- jj flat `lib/src/*.rs` still under-counted as modules
- Full `snowfall` required for system python3/gh on PATH

## Conclusion

- **Hard reliability** across 11 repos: pass (suite).  
- **Layout UX** for non-`src/modules` apps: **fixed**.  
- **Ghost modules + anomaly noise**: **fixed with proof**.  
- **Agent symbol localization**: **fixed** (ripgrep required).  
- **Agent + Atlas together** work for localization on TS and Rust.  
- Capacity limits (type safety, cross-repo edges, huge-history ingest) remain explicit — not silently claimed fixed.
