---
title: Local AI synthesis as default output for atlas investigate
date: 2026-07-17
status: Implemented
---

## Problem

`atlas investigate` produced a dense, multi-section raw evidence dump that was difficult to interpret at a glance. Users needed to mentally cross-reference CORE CANDIDATES, SUPPORTING ARTIFACTS, OBSERVED STRUCTURE, DOCUMENTARY EVIDENCE, and HISTORICAL sections to form a picture of what the code domain does. The commands individually were even less useful — `atlas co-changes` or `atlas structural` alone read like spreadsheets, not engineering insight.

The specific friction: a developer asking "what does the share class / listing system do?" would get 17 files listed with structural reasons, but no narrative about what those files collectively accomplish.

## Methodology validation

- **Principle 1 (features by evidence):** The failure was demonstrated live: `atlas investigate share class listing` produced unreadable output. The synthesis direction was validated by comparing the before/after on the same query.
- **Principle 2 (abstractions by repetition):** No new abstraction extracted — one concrete synthesis path added to one command.
- **Principle 3 (knowledge accumulated):** Decision record and benchmark produced this session.
- **Principle 4 (validation before generalization):** Synthesis tested on rwatp-core. Generalization (adding synthesis to other commands) deferred until N≥3 friction cases.

## Decision

Add a local AI synthesis layer as the **default** output of `atlas investigate`. The raw evidence render becomes opt-in via `--raw`.

Architecture:
- New `apps/cli/src/ai.rs` module with `synthesize(doc: &InvestigationDocument) -> Option<String>`
- `build_prompt()` constructs a structured, facts-only prompt from the `InvestigationDocument`
- Calls Ollama `/api/chat` via `curl --data-binary @-` subprocess — consistent with existing gh/git shell-out pattern; no new Rust dependencies
- Model: `qwen2.5-coder:7b-instruct` (code-specialized, instruct-tuned, already available locally)
- Temperature: 0.1, num_predict: 600, timeout: 90s
- `synthesize()` returns `Option<String>` — `None` on any failure; graceful fallback to raw render

The `InvestigationDocument` remains the AI boundary. `synthesize()` reads the finished evidence bundle; it cannot observe anything new.

Output format enforced in prompt:
```
WHAT THIS DOES      — 2-3 sentences from evidence only
KEY BEHAVIORS       — bullets using ·
RECENT CHANGES      — 1 sentence from documentary evidence
GAPS                — omitted if no unresolved connections
```

Followed by the COVERAGE table and a hint: "Run --raw for full evidence · --json for machine-readable output."

## Alternatives considered

- **reqwest HTTP client**: Adds a compile-time dependency and async complexity. Rejected — curl subprocess is consistent with how `gh` and `git` are called elsewhere in the CLI.
- **System prompt via /api/generate**: Wrong endpoint for instruct-tuned models; `/api/chat` handles the chat template correctly.
- **qwen3:4b**: Smaller and faster but not code-specialized. Rejected in favor of qwen2.5-coder:7b-instruct for higher precision on code domain questions.
- **Making synthesis mandatory (no fallback)**: Rejected — Ollama is a local dev dependency, not a guaranteed runtime. Atlas must be usable without it.

## Validated outcome

Before (raw):
```
INVESTIGATION
anchors: share · class · listing

CORE IMPLEMENTATION NEIGHBORHOOD  (17 files)
  src/common/enum/listing.enums.ts
    ← anchor match "listing" (file_path)
  ...17 files, structural reasons, no narrative
```

After (default with Ollama):
```
Synthesizing with qwen2.5-coder:7b-instruct …
INVESTIGATION
anchors: share · class · listing

WHAT THIS DOES
This code domain appears to be a backend service for managing listings and share classes...

KEY BEHAVIORS
· Data Retrieval: resolves data using findById, findOne, find
· Data Manipulation: creates, updates, deletes via service calls
· Permissions Management: manages permissions using ShareClassService
· Supply Operations: handles supply ledger entries

RECENT CHANGES
PR #63 introduced smart contract deployment recording...

GAPS
share-class.enums.ts, share-class.types.ts not directly connected...

COVERAGE
  Git history      ✓
  ...
Run --raw for full evidence · --json for machine-readable output.
```

## Future

- Synthesis for other commands (`atlas context`, `atlas review-context`) — deferred until N≥3 readability friction cases on those commands specifically
- Model selection flag (e.g. `--model`) — deferred, default works well
- Prompt tuning based on synthesis quality feedback — defer until N≥3 bad synthesis cases observed
