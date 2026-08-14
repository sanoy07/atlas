---
title: Document ingestion coverage — README and recursive docs/
date: 2026-08-08
repository: (to fill: RWATP core, VestaScan, Atlas self)
issue: Repository Intelligence Ingestion — Step 1 validation
status: Draft
---

# Benchmark: Document ingestion coverage after extending `ingest_documents`

## Repository

Name: TBD — this stub covers **three** target validations:
  1. RWATP core (TypeScript / Express / GraphQL / Mongoose)
  2. VestaScan (TypeScript / dist-committed)
  3. Atlas self (Rust workspace)

Atlas ingested: git + (github where token available) + language-specific

## Question

Does extending `ingest_documents` to also cover `README.md` and
recursive `docs/**/*.md` produce useful documentary evidence in
`atlas search` and `atlas investigate` without generating noise?

The narrower operational question is:

> When Atlas is run against a repository that follows any of the common
> layouts — `docs/architecture/`, `docs/guides/`, `docs/runbooks/`, or
> project-level `README.md` — do those documents appear in
> `SearchDocument.matches` and `InvestigationDocument.documentary` in a way
> that improves at least one previously-blocked investigation?

## Ground Truth

**RWATP core (to run):**
- What documentation exists under `docs/`?
- Which of those documents mention terms that anchor an investigation
  (e.g., `token deployment`, `compliance`, `identity`)?
- Which of those documents were previously invisible to Atlas?

**VestaScan (to run):**
- Same as above.

**Atlas self (to run):**
- `README.md` — does it exist yet?  If yes, does it appear in `atlas search readme`?
- `docs/decisions/*.md` — unchanged behaviour expected.
- `docs/benchmarks/*.md` — should now appear as `doc_type = "doc"`.
- `docs/research/*.md` — should now appear as `doc_type = "doc"`.

---

## Atlas Evaluation

### Commands used (in order)

To run for each target repository:

```bash
# Sanity check: what does ingest report?
atlas ingest .

# Count and spot-check the new document rows.
sqlite3 "$ATLAS_DB" "SELECT doc_type, COUNT(*) FROM documents WHERE repo_path='<path>' GROUP BY doc_type;"

# Verify a term that only appears in a doc is now searchable.
atlas search <term-known-to-only-appear-in-a-doc>

# Verify an investigation whose blocking evidence was previously invisible.
atlas investigate <anchor-1> <anchor-2>
```

### Manual source reads required

To fill after running.

### Wrong branches followed

To fill after running — particular attention to whether the recursive walk
pulls in documents that muddy investigations rather than clarify them.

### False positives

| Query | Unexpected match | Reason | Severity |
|-------|------------------|--------|----------|

To fill after running.

### Useful observations

To fill after running.

---

## Classification

| Dimension | Result |
|-----------|--------|
| Overall | (TBD) |
| Commands needed | (TBD) |
| Source reads needed | (TBD) |
| Confidence at completion | (TBD) |
| Noise removed (vs. prior) | (TBD) |
| Hidden understanding revealed | (TBD) |

---

## Outcomes

**Decision produced?** Y — `docs/decisions/2026-08-08-extend-document-ingestion.md`.

**New primitive required?** No.  This benchmark measures whether an existing
primitive (documentary ingestion) with a widened scope is a net improvement.
If the answer is "no" for any of the three target repositories, the decision
record is revisable.

**New abstraction earned?** No.

**Regression?** To measure — the acceptance criterion is that no previously
useful `SearchDocument` result disappears.  The `preserves_decision_and_adr_types`
unit test provides the tightest local guard for the change; this benchmark
provides the cross-repository check.

---

## Notes

This benchmark is deliberately committed in **Draft** state alongside the
implementation.  Marking it Complete requires running Atlas against at least
two of the three target repositories and pasting the concrete numbers into
the sections above.
