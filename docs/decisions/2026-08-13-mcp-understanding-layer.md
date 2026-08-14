---
title: atlas mcp — Atlas as the understanding layer beneath coding agents
date: 2026-08-13
status: Proposed
---

> **Format note.** Every other decision record in this repository is written
> after implementation, and `status:` admits only Implemented / Deferred /
> Superseded. This one is deliberately written *before* any code exists, and
> introduces `status: Proposed`. The reason is Principle 1 in its strongest
> form: this capability is not earned by production evidence, so the benchmark
> that would earn it — including its kill condition — must be fixed before the
> implementation can bias it. `## Validated outcome` is therefore empty by
> design, and must be filled before this record moves to Implemented.

## Problem

Atlas answers structural questions deterministically in 10–50 ms. Coding
agents answer the same questions by grepping and reading source, from scratch,
every session — spending context to rediscover structure that Atlas already
holds. Nothing connects the two.

The prior phase's local agent (`agent/atlas_agent.py`, qwen3:4b) proved the
evidence is *sufficient* — a 4B model reaches correct multi-hop answers when
Atlas supplies the structure. But it takes 1–3 minutes per hard question, and
the reasoning quality is the binding constraint, not the evidence quality.
That is the wrong division of labor to keep optimizing: the substrate is
Atlas's durable product, and the model is explicitly interchangeable.

The unproven hypothesis, stated so it can fail:

> A coding agent that can query Atlas reaches a correct architectural
> understanding with materially less context consumed and fewer wrong
> assumptions than the same agent working from grep and file reads alone.

If that is false, Atlas is a good CLI and should stay one. If it is true, it
is the product, and the frontier model becomes an execution engine downstream
of Atlas rather than a competitor to it.

### The change-intent framing this serves

The capability being enabled is not code generation. It is the layer before
execution — answering, with evidence, the five questions a developer (or an
agent) needs settled before writing anything:

1. **WHERE** does this change belong? (files, symbols, modules, interfaces)
2. **WHAT** exactly changes? (add / modify / extend an existing abstraction)
3. **WHY HERE** — what makes this the correct architectural location?
4. **WHAT BREAKS** — callers, implementations, tests, configuration.
5. **HOW DO I VERIFY** — invariants, relevant tests, likely regressions.

Atlas supplies 1, 2, 4 deterministically and the evidence for 3 and 5. The
model supplies the synthesis. Neither does the other's job — the same layering
rule the crates already follow, extended across the model boundary.

## Methodology validation

- **Principle 1 (features earned by production evidence):** *not satisfied in
  the normal way, and this record does not pretend otherwise.* N=0 for
  "coding agent consuming Atlas" — no such consumer has ever existed, so no
  investigation failure can have been observed. What is satisfied is the
  weaker, honest form: the friction is observed on the *human* side (every
  agent session rediscovers structure), and the benchmark below is defined
  before implementation so the result can contradict the hypothesis.
- **Principle 2 (abstractions earned by repetition):** satisfied. The six
  tools are existing, individually validated operations. No new abstraction is
  introduced — the MCP server is an adapter, not a layer.
- **Principle 3 (knowledge accumulated):** this record; benchmark below;
  `## Validated outcome` blocks promotion until filled.
- **Principle 4 (validation precedes generalization):** enforced by the kill
  condition. The tool surface does not grow past six until the A/B benchmark
  shows Atlas helps.

Carrying forward the Phase 1 discipline: this is a **research bet**, not an
earned primitive, and is labelled as such. The methodology becomes theater the
moment every improvement is retroactively declared evidence-driven.

## Decision

### 1. Six tools. The MCP surface is smaller than the CLI, permanently.

| Tool | Backing core function | Answers |
|---|---|---|
| `atlas_map` | `build_map` | What is this repository? |
| `atlas_capabilities` | `compute_capabilities` | What infrastructure/product surfaces exist? |
| `atlas_search` | `definition_ranked_search` | Where is X defined? (DEFINITION/WIRING/CALL_SITE/REFERENCE/TEST) |
| `atlas_callers` | `find_callers` | Who calls this? |
| `atlas_implementations` | `find_implementations` | What implements this? |
| `atlas_investigate` | `investigate` | Structured evidence packet for a question |

The 33 remaining CLI commands are deliberately absent. A model does not need
to know Atlas has 39 commands; it needs primitives that cover UNDERSTAND /
LOCATE / RELATE / INVESTIGATE. Growth is gated on the benchmark, not on
"the command already exists."

### 2. No write tools. Ever, at this layer.

No `atlas_edit_file`, `atlas_apply_patch`, `atlas_run_command`,
`atlas_generate_code`. The consuming agent already has those and is better at
them. Atlas's contribution is the understanding that precedes them; an Atlas
that also edits is a worse Claude Code competing on someone else's ground.

### 3. Every response carries provenance. This is the differentiator.

Grep returns matches. Atlas returns matches *plus how much to trust them*.
Every tool result is wrapped:

```jsonc
{
  "repo": "/abs/path",
  "freshness": {                     // Phase 1 FreshnessReport
    "state": "stale",
    "commits_behind": 3,
    "warning": "evidence graph is 3 commit(s) behind — re-run `atlas ingest .`"
  },
  "basis": "OBSERVED",               // OBSERVED (edge in DB) | DERIVED (heuristic) | MIXED
  "extractor_version": "typescript-v1.0",
  "limitations": [
    "Structural graph is a snapshot of the working tree at last ingest.",
    "Runtime DI and dynamic imports are not observed."
  ],
  "result": { /* the Serialize report from atlas-core, unchanged */ }
}
```

`FreshnessReport::warning()` returning `Option<String>` rather than printing
(Phase 1) is what makes this possible without a second freshness
implementation. The `limitations` array is not decoration: it is the
mechanism by which Atlas refuses to state a relationship with more certainty
than its evidence permits, at the exact boundary where a model would otherwise
over-read the result.

### 4. The MCP server is a presentation layer, like the CLI.

Verified before writing this: all six backing functions exist in `atlas-core`
and every report type (`CallersReport`, `ImplementationsReport`,
`CapabilitiesReport`, `CodeSearchReport`, `MapReport`, `InvestigationDocument`)
already derives `Serialize`. The server calls core and serializes. It contains
no business logic, exactly as `apps/cli` contains none. If the MCP server ever
needs logic the CLI lacks, that logic belongs in core.

### 5. Transport: stdio JSON-RPC 2.0, hand-rolled, no new dependency.

`serde_json` and `tokio` are already workspace dependencies. The stdio MCP
surface needed here is three methods (`initialize`, `tools/list`,
`tools/call`). Hand-rolling is ~300 lines and adds no external SDK to a
project whose entire thesis is a durable substrate.

Rejected: adding the official Rust MCP SDK. Revisit if the protocol surface
grows past those three methods, or if spec churn makes hand-rolling a
maintenance cost rather than a saving.

### 6. Repository resolution: Phase 1's rules, unchanged.

The server resolves its repository exactly as the CLI does — `ATLAS_DB` →
git-root-anchored `atlas.db` → cwd — plus an explicit `--repo` flag for
clients that launch with an unrelated working directory. This is the whole
reason Phase 1 came first: a position-dependent server would answer from a
database the calling agent cannot see or verify, and the agent has no working
directory of its own to reason about the discrepancy.

### 7. `atlas_impact` is pre-registered, not shipped.

"What breaks?" is question 4 of the five, and it is the question a coding agent
most needs answered before editing — so its absence from the six needs a
reason better than "the list was already full."

The reason: `callers` already answers the deterministic core of "what breaks"
— direct reverse edges, OBSERVED, from `structural_edges`. `impact` extends
that with transitive reach and co-change coupling, which is **ranked guidance
and explicitly DERIVED**. Shipping it in the first six would put the most
weakly-supported tool into the smallest, most trust-sensitive surface, at
precisely the moment we are trying to measure whether provenance labelling
restrains a model from over-reading results.

So it is **pre-registered**: if the benchmark shows condition-B agents asking
"what else does this affect" and failing to get it from `callers` alone,
`atlas_impact` is added — and because the trigger is written down now, that
addition is an earned result rather than post-hoc surface growth. Any *other*
tool addition requires the same treatment: name the failure first.

### 8. The provenance envelope is uniform, on every response.

The alternative — envelope only on DERIVED results — was rejected. Two
reasons, one of which is decisive:

- **Freshness is orthogonal to basis.** A stale OBSERVED edge is still stale.
  Restricting the envelope to DERIVED results would strip freshness from
  exactly the results a model trusts most.
- **Absence would become an unearned signal.** If the envelope appears only
  on uncertain results, its absence silently reads as "this is certain" — a
  claim Atlas never actually made. Making certainty implicit is how an
  epistemic system starts lying by omission.

Token cost is managed by keeping the envelope compact and omitting empty
fields (`limitations` appears only when non-empty), not by making it
conditional on basis.

### 9. Output format is a benchmark variable, not a settled decision.

Two candidates: compact rendered text (the format already validated with
qwen3:4b) versus structured JSON. JSON is more precise; rendered text is
fewer tokens, and tokens-before-implementation is the primary metric. The
first implementation returns rendered text with the provenance envelope as a
compact header, and exposes `format: "json"` for the same content. Which wins
is measured, not asserted.

## Alternatives considered

**Expose all 39 CLI commands.** Rejected. The temptation is that they already
exist. But a large surface makes tool selection the model's hardest problem,
and the code-intel phase already demonstrated that the win comes from *general
structural operations*, not from more of them. Smaller surface, better
selection.

**Make `atlas_investigate` a generic "ask Atlas anything" endpoint.**
Rejected — it would return prose, and prose is the one thing the consuming
model produces better than Atlas. It returns a structured evidence packet:
understanding, evidence paths, relationships, status, limitations. The
frontier model reasons over that. Atlas must not compete on synthesis.

**Return raw source alongside evidence.** Rejected. The agent can already read
files, and doing so would spend exactly the context this exists to save.
Atlas returns paths, symbols, relationships, and provenance; the agent reads
the two files that matter instead of the forty it would have grepped.

**Build `atlas plan` / `atlas review` first.** Deferred, not rejected — these
are the more valuable products (a plan the developer implements; a review of a
diff against observed architecture). They are deferred because both are
*synthesis* over the same six primitives. Building the substrate and proving a
model exploits it must come first; otherwise `atlas plan` bets on synthesis
quality before knowing the evidence layer transfers to a consumer at all.

**Retire `agent/atlas_agent.py`.** Rejected. It becomes the local reference
client and control condition — the cleanest available experimental setup:

- Qwen succeeds on the six primitives → the interface is genuinely model-agnostic.
- Qwen fails, frontier succeeds → the constraint is reasoning, not evidence.
- Both fail → Atlas has an evidence or interface problem, and the MCP surface
  is wrong rather than the model being weak.

That third branch is the one that would otherwise be misdiagnosed for months.

## Validated outcome

**Empty by design — this record precedes implementation.** It must be filled
from the benchmark below before `status:` moves to Implemented.

The benchmark is specified now, before the code exists, so the implementation
cannot be tuned to it after the fact.

### Experiment

Same tasks, same frontier model, two conditions:

- **A (control):** agent with grep / read / search only.
- **B (treatment):** same agent, plus the six Atlas MCP tools.

Tasks drawn from rwatp-core, where ground truth is already established by the
code-intel phase — e.g. "add support for an S3-compatible storage provider",
"add a new payment provider", "where does payment settlement trigger order
fulfillment". Each has a known-correct architectural answer
(`IStorageProvider` + adapter + factory; `tryEnqueue` reached from settlement
and signing), so grading is not subjective.

### Measured

| Metric | Why it matters |
|---|---|
| Tokens consumed before a correct plan | The headline claim; context is the scarce resource |
| Source files opened | Rediscovery cost |
| Tool calls to first correct plan | Search efficiency |
| Wrong architectural assumptions | The failure Atlas should prevent (e.g. bypassing the factory) |
| Unsupported claims | Does provenance actually restrain over-reading? |
| Reaches the same architecture as ground truth | Correctness, not just speed |

### Kill condition

If condition B does not reduce tokens-before-correct-plan **and** does not
reduce wrong architectural assumptions, the MCP server does not graduate past
experimental, and the tool surface does not grow. "Neither better nor worse,
but it feels more principled" is a failure result and will be recorded as one.

## Future

Enabled, explicitly deferred until the benchmark returns:

- **`atlas plan "<change>"`** — WHERE / WHAT / WHY HERE / WHAT BREAKS / HOW TO
  VERIFY, synthesized over the six primitives. The most valuable product in
  this direction; deferred because it depends on the substrate transferring.
- **`atlas review`** — a written diff compared against observed architecture:
  uses existing abstraction ✓, bypasses factory ⚠, missing contract test ⚠.
  Requires the diff-vs-graph comparison Atlas does not yet have.
- **`atlas verify`** — did an implementation preserve the architectural
  invariants the plan named?
- **Handing an Atlas plan to a frontier agent as its brief** (understanding,
  change surface, constraints, verification criteria) — the full division of
  labor. Needs `atlas plan` first.
- **Capability index persisted at ingest** — still query-time from import
  fan-in. Fine until the MCP path makes it a latency problem; measure before
  moving it.
