# Atlas IR — The Conceptual Model

This document describes the Atlas Intermediate Representation in concepts, not code.
Read this before reading `crates/ir/src/lib.rs`.

---

## What is the IR?

The IR is the universal language Atlas speaks internally.
Every connector — git, GitHub, Google Chat, Cloud Run — eventually produces IR.
Every query runs against IR stored in SQLite.

The IR has two layers:

1. **Domain types** — concrete, familiar objects (Commit, File, PullRequest)
2. **Graph primitives** — the abstract structure (Entity, Relationship, Evidence)

---

## Domain Types

These are the things Atlas knows about today.

### Repository
A local git repository. The root of most ingestion pipelines.

```
Repository
  path         — filesystem path
  name         — derived from the directory name
  remote_url   — optional GitHub/GitLab URL
```

### Commit
A single unit of work in version control.

```
Commit
  hash          — full SHA
  short_hash    — 7-char display form
  message       — the commit subject line
  author_name
  author_email
  timestamp
  files_changed — list of file paths touched by this commit
```

### File
A path that has appeared in at least one commit.

```
File
  path       — relative to the repository root
  extension  — derived from the path
```

### Author
A person who has made at least one commit.
Currently implicit in Commit — will become a first-class entity.

```
Author
  name
  email
```

### PullRequest
A GitHub pull request that proposed and (optionally) merged a set of commits.

```
PullRequest
  number
  title
  state            — open, closed, merged
  body
  author
  merge_commit_sha — the commit hash that landed this PR on the main branch
```

### Issue
A GitHub issue linked to engineering work.

```
Issue
  number
  title
  state   — open, closed
  body
  author
```

---

## Graph Primitives

These power the future knowledge graph and are already in the codebase.

### Entity
Any named thing in the Atlas world.

```
Entity
  id    — globally unique string (e.g. "commit:abc123")
  kind  — Commit | File | PullRequest | Issue | Author
  label — human-readable display name
```

### Relationship
A directed edge between two entities.

```
Relationship
  from_id
  to_id
  kind    — Modifies | Merges | Closes | AuthoredBy
```

Examples:

```
commit:abc123  --[Modifies]-->   file:src/main.rs
pr:42          --[Merges]-->     commit:abc123
pr:42          --[Closes]-->     issue:17
commit:abc123  --[AuthoredBy]--> author:alice@example.com
```

### Evidence
Proof that an entity or relationship exists.

```
Evidence
  entity_id  — what this evidence supports
  source     — Git | GitHub | Manual
  raw        — the original string that produced this fact
```

Evidence is how Atlas stays honest.
Every entity can be traced back to a raw source artifact.

---

## Relationship Map (current)

```
Repository
  └── contains ──► File
  └── contains ──► Commit

Commit
  └── Modifies ──► File
  └── AuthoredBy ──► Author

PullRequest
  └── Merges ──► Commit
  └── Closes ──► Issue (future)

Issue
  (currently standalone — will link to PRs and Commits)
```

---

## What is not in the IR yet

These will be added as connectors are built:

- `Message` — a Google Chat or Slack message
- `LogEntry` — a Cloud Run or Kubernetes log line
- `Deployment` — a production deploy event
- `Review` — a PR code review comment
- `Function` — a named function in source code (requires language parsing)
- `Test` — a test case linked to the code it covers

Each of these follows the same pattern: domain type + relationships + evidence.
