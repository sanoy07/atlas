---
title: Freeze C5.1+C4; agent is orientation layer, not truth layer
date: 2026-08-10
status: Implemented
---

## Problem

After C5.1-S/path-class/C4 and the Qwen tool-calling agent both shipped, it was unclear whether the agent improved Atlas on unfamiliar repos (JJ, GigaToken) beyond deterministic investigate.

## Methodology validation

Blind gold-first adversarial suite (14 questions). No gold edits after runs. No Atlas changes during eval. Full report: `docs/benchmarks/2026-08-10-adversarial-blind-det-vs-agent.md`.

## Decision

1. **Keep C5.1-S/R/L/E + C4 frozen** as the localization + verification core.
2. **Treat the Python agent as a UX/orchestration experiment**, not as authority over claims.
3. **Next earned change:** agent must call investigate (or subject resolution) and **hard_verify** final causal claims — not C5.2 embeddings, not more free tools.
4. Do **not** declare multi-hop “solved” by the agent (jj-bug and gt-bug still wrong).

## Validated outcome

| | Det | Agent |
|--|-----|-------|
| Mean score | 54.2 | 67.3 |
| Latency | ~1s | ~116s |
| C4 flags | 0 | 1 |
| New gold vs det top10 | — | 7/14 |

## Future

Agent+C4 integration; optional `investigate` tool; re-run this suite as promotion gate before C5.2.
