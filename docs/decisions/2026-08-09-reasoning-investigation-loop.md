---
title: Local-first reasoning investigation loop over evidence packets
date: 2026-08-09
status: Implemented
---

## Problem

Atlas could retrieve candidates (`investigate`) and optionally print
unstructured Ollama prose, but could not:

1. Accept a vague natural-language problem or issue/file seed
2. Bound evidence into a reusable packet
3. Separate deterministic verification from non-deterministic hypothesis generation
4. Mark AI claims as supported / contradicted / plausible / unresolved
5. Preserve chronology roles (intent vs implementation)

## Decision

Add a **reasoning investigation loop** without replacing B1–B10 or the
legacy anchor `investigate` path.

### Components

| Piece | Location |
|-------|----------|
| IR: `EvidencePacket`, `ProposedClaim`, `Hypothesis`, `ReasoningInvestigationResult` | `crates/ir` |
| Provider trait + Ollama + Fake | `crates/core/src/ai_provider.rs` |
| Packet build, verify, multi-round loop | `crates/core/src/reasoning.rs` |
| CLI | `atlas investigate` modes |

### Loop

```
question/issue/file → EvidencePacket (deterministic investigate + chronology)
  → optional local AI (structured JSON)
  → verify claims against packet
  → optional expand requested_subjects (≤3 rounds)
  → ReasoningInvestigationResult
```

### Privacy

- **Local AI default** (Ollama HTTP, `ATLAS_OLLAMA_URL` / `ATLAS_OLLAMA_MODEL`)
- Cloud escalation **not implemented**
- `--no-ai` / `--raw` never call the model
- AI output is never written as repository evidence

### CLI

| Invocation | Mode |
|------------|------|
| `atlas investigate auth order` | Legacy anchors (+ optional prose AI) |
| `atlas investigate "orders timeout"` | Reasoning loop |
| `atlas investigate --issue 12` | Issue anchors → loop |
| `atlas investigate --file path` | Seed file → loop |
| `… --no-ai --json` | Deterministic packet only |

### Non-goals

- No claim of root cause, ownership, quality, or safety
- No schema change
- No graph DB
- No infinite agent
- No automatic full-repo upload

## Validated outcome

- `reasoning_fixture` 9 tests; blackbox +3
- RWATP scenarios A/B/C under `--no-ai` produce likely area `core`, order
  neighborhood, chronology with intent/implementation roles

## Future

- Cloud escalation via same provider trait + explicit evidence packet display
- Stronger contradiction detection using commit order vs claim temporal_scope
- Feed verified claims into eventual map/focus/impact (Section C product)
