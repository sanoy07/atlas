# Code-intel implementation phase report

**Date:** 2026-08-13  
**Binary:** atlas 0.2.0 (release, `~/.local/bin/atlas`)  
**Corpus:** `/home/sanoy/Vesta/rwatp-core` + `rwatp-eval.db`

## Problem

Agent answers for storage/flow were held together by **host domain drills** (regex + hardcoded ListingAsset/tryEnqueue prose). That does not scale.

## What shipped

### Deterministic CLI (structural_edges)

| Command | Role |
|---------|------|
| `atlas callers <symbol\|file>` | OBSERVED reverse call edges; production before tests |
| `atlas callers --callees` | Outgoing callees emphasis |
| `atlas implementations <Interface\|path>` | DERIVED adapters via factory + `*.interface.ts` |
| `atlas capabilities` | DERIVED infra fan-in → product surfaces (storage notes: data-room vs KYC vs support) |
| `atlas code-search <q>` | Definition-ranked structural search (DEFINITION/WIRING/CALL_SITE/TEST) |

### Storage / core

- `structural_edges_by_target_symbol`, `_by_source_symbol`, `_symbol_search`, `structural_importers_of`
- `crates/core/src/code_intel.rs` — pure query layer over Store

### Agent

- Tools: `atlas_callers`, `atlas_implementations`, `atlas_capabilities`, `atlas_code_search`
- Host **flow drill** → `atlas_callers` (rg only if empty)
- Host **storage drill** → capabilities + implementations + code-search (not hardcoded GCS essay)
- System prompt updated to prefer structural tools

### little-coder

- Research: `docs/research/2026-08-13-little-coder-for-atlas.md`
- Integration: `integrations/little-coder/` (skill card, README, extension sketch)
- Conclusion: use as **harness for small models**, Atlas remains evidence substrate

### Bench harness

- `eval/code-intel-bench.sh` (deterministic + optional agent)

## Benchmark results

### Fix-all re-bench (`code-intel-fixall-20260813-205501`)

**14/14 pass, 0 fail** after implements edges + multi-line imports + agent path tracking.

### Deterministic

| Case | Result | Latency |
|------|--------|---------|
| callers tryEnqueue | OK (payment-settlement + signing) | ~15ms |
| callers Class.method | OK | ~13ms |
| implementations IStorageProvider | OK **OBSERVED implements** | ~23ms |
| structural adapter shows IMPLEMENTS | OK | ~9ms |
| capabilities storage | OK (listing-asset surfaces) | ~22ms |
| code-search ListingAsset / getSignedUrl | OK | ~10–15ms |
| callers/capabilities JSON | OK | ~10–20ms |
| map / investigate storage | OK | ~47–168ms |

### Agent (qwen3:4b, `--no-web`)

| Case | Result | Latency |
|------|--------|---------|
| "storing files in this backend" | OK | ~59s |
| "data room document storage … GCS" | OK | ~183s |
| "payment settlement → fulfillment" | OK | ~144s |

**Structural edges after re-ingest:** 4028 total, **26 implements** (was 0).
## Pre-change baseline (qualitative)

Before this phase:

- Vague storage → fs.writeFile trap (fixed earlier with host storage drill)
- Data-room → sometimes KYC
- Payment flow → needed tryEnqueue host drill

After:

- Same agent questions **pass** with structural tools; host drills call general ops instead of product-specific essays
- Deterministic `atlas callers tryEnqueue` alone is the gold answer for payment multi-hop

## How to re-run

```bash
export ATLAS_DB=~/projects/atlas/eval/cross-repo-suite/rwatp-eval.db
export PATH="$HOME/.local/bin:$PATH"
# deterministic only
SKIP_AGENT=1 ./eval/code-intel-bench.sh after
# full including Ollama agent
./eval/code-intel-bench.sh full
```

## Not done (next)

- True `implements` edge from TS parser / SCIP ingest
- Capability index persisted at ingest (currently query-time)
- little-coder live pi extension with registered tools
- Cross-repo suite re-run of full 60-case hard eval
