# ADR 0004 — Connector trait as the common interface for all data sources

**Date:** 2026-07-01  **Status:** Accepted

## Decision

Every Atlas data source implements the `Connector` trait defined in `crates/connectors`.
The `connectors` crate contains only the trait — zero implementations.
Implementations live in their own crates (`crates/git`, and eventually `crates/filesystem`,
`crates/cloudrun`, etc.).

## Why

Atlas already had two connectors (git log and GitHub CLI) living in the same crate with
no shared interface. Adding a third would have meant inventing a third ingestion shape.
Defining the trait when there are exactly two implementations — not one (too early to see
the pattern) and not ten (too late to refactor cleanly) — is the right moment.

The trait answers three questions every connector must be able to answer:
- Who are you?              (`name`)
- What do you produce?      (`capability`)
- Give me your raw data.    (`fetch_raw`)

Parsing, storage, and orchestration are deliberately excluded from the trait.

## What the trait does NOT do

- Parse raw output into IR types. That is `atlas-parser`'s job.
- Persist anything. That is `atlas-storage`'s job.
- Know about other connectors. That is `atlas-core`'s job.
- Handle incremental sync or event streaming. Those are future concerns.

## Connector splitting

The old `GitHub` struct (which had two methods: `pull_requests_raw` and `issues_raw`)
was split into `GitHubPrConnector` and `GitHubIssueConnector`. Each implements `Connector`
independently. Each has a distinct capability:
- `GitHubPrConnector`    → "Collaboration Metadata"
- `GitHubIssueConnector` → "Issue Tracking"

This aligns with the principle that each connector answers exactly one question.

## Alternatives rejected

- **Keep both connectors in one struct with multiple fetch methods**: Doesn't satisfy the
  single-responsibility principle. A struct with two fetch methods is two connectors pretending
  to be one.
- **Use a trait object (`Box<dyn Connector>`) in core immediately**: Premature. Core currently
  knows about concrete connector types. Dynamic dispatch is available when needed (e.g. a
  plugin system), but is not needed today.
- **Put the trait in `atlas-ir`**: IR is the universal *data* language. The connector is a
  *behavior* contract. Mixing them would violate IR's invariant of containing only types.

## Future connectors

Any new data source follows the same pattern:
1. Create a struct.
2. Implement `Connector`.
3. Register it in `atlas-core`'s ingestion pipeline.

No changes to the trait, the parser interface, or storage are required.
