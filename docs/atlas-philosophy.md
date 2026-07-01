# Atlas Philosophy

This document is the constitution of Atlas.
Every PR, every feature, every crate decision can be evaluated against these principles.

---

## Atlas is deterministic by default.

Given the same repository state, Atlas produces the same output.
AI never invents facts. AI reasons over facts Atlas has already collected.

---

## Atlas stores evidence, not opinions.

Every entity Atlas knows about is traceable to a source artifact:
a commit, a PR, a file, a message, a log line.
If Atlas cannot point to evidence, it does not assert the claim.

---

## AI is a consumer, not the source of truth.

The knowledge graph is built without AI.
AI sits at the top of the stack, consuming structured context.
It does not write to storage. It does not define relationships.

---

## Every recommendation is explainable.

If Atlas says "this file is high-risk," it can show which commits, which authors,
which PRs, and which issues lead to that conclusion.
Black-box scores are not Atlas.

---

## Everything becomes entities and relationships.

A commit is an entity. A file is an entity. A PR is an entity.
The fact that a PR modified a file is a relationship with evidence.
The fact that an issue caused a commit is a relationship with evidence.
Atlas's job is to collect these and make them queryable.

---

## The user owns the data.

Atlas runs locally. Data lives in a local SQLite file the user controls.
Nothing is sent to a cloud service without explicit user action.
Atlas has no telemetry.

---

## Capabilities outlast implementations.

"Repository History" is a capability. Git is one implementation of it.
When a better implementation exists, you swap the implementation.
The capability, the IR, and the queries remain unchanged.

---

## Connectors collect. Parsers transform. Storage persists. Core orchestrates. CLI presents.

No layer does another layer's job.
A connector does not parse. A parser does not store.
Storage does not know about git. Core does not print to the terminal.
