# ADR 0002 — SQLite as the storage layer

**Date:** 2026-07-02  **Status:** Accepted

## Decision

All Atlas state is persisted in a local SQLite file (default: `./atlas.db`).

## Why

- Single file, zero network, zero daemon. Matches Atlas's local-first philosophy.
- `rusqlite` with `features = ["bundled"]` compiles SQLite in — no system dependency.
- WAL mode allows concurrent readers without blocking writes.
- Queries over commit/file/PR relationships are standard SQL — no ORM needed.

## Alternatives rejected

- **PostgreSQL / MySQL**: Requires a running daemon. Overkill for a local tool.
- **DuckDB**: Excellent analytics. Rust bindings less mature than rusqlite.
- **Flat files / JSON**: Simple to write, painful to query across relationships.
