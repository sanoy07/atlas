---
title: Reasoning investigation loop — evidence packet + local AI verification
date: 2026-08-09
repository: rwatp-core
status: Complete
---

# Benchmark: vague problem → evidence → (optional local AI) → verified investigation

## Capability under test

```
question / issue / file
    → bounded EvidencePacket (deterministic)
    → local AI hypotheses (optional)
    → Atlas claim verification
    → ReasoningInvestigationResult
```

## Fixture tests

`crates/core/tests/reasoning_fixture.rs` — 9 tests  
(packet bounds, verification, fake provider, chronology, JSON, no-AI).

CLI blackbox: question `--no-ai --json`, `--file` seed, legacy anchors.

## Workspace

See freeze run after implementation.

## RWATP evaluation scenarios

Database: `ATLAS_DB=~/.atlas/rwatp-core.db`  
Repo cwd: `/home/sanoy/Vesta/rwatp-core`  
Commands use `--no-ai` for CI-reproducible eval; local Ollama may be used interactively.

### A. Bug investigation

**Question:** `"order timeout concurrent"`

**Expected useful evidence (grounded):**

- Candidates under `src/modules/core` related to orders / processing
- Chronology of commits touching order-related files when present
- Explicit limitations (no runtime scheduling)

**Command:**

```bash
atlas investigate "order timeout concurrent" --no-ai --json
```

**Pass criteria:** JSON `schema_version=1`, non-empty `packet.limitations`,
`mode=deterministic_only`, `likely_area` or core candidates non-empty OR honest empty
with next_investigation guidance.

### B. Order-flow understanding

**Question:** `"explain the order flow"`

**Expected:**

- Anchors include `order` / `flow`
- Module/core neighborhood if structural+path evidence exists
- Chronology separates intent vs implementation roles when docs present

```bash
atlas investigate "explain the order flow" --no-ai
```

### C. File-seeded investigation (proxy for issue planning without GitHub ingest)

Issue ingest may be empty without `--github`. File seed exercises the same loop:

```bash
atlas investigate --file src/modules/core/services/order.service.ts --no-ai
```

**Expected:** `order.service.ts` in affected components / core candidates; chronology of
commits on that path when history exists.

## Timing

No formal timing benchmark. Local AI rounds are wall-clock dependent on Ollama.

## Outcomes

- Decision: local AI is optional; verification is mandatory for proposed claims
- Cloud escalation: not implemented (provider trait ready for later)
- Hallucinated file refs → CONTRADICTED status in verify_claims
