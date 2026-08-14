# Atlas evidence tools (read-only repository intelligence)

You are working in a repo that may have **Atlas** installed (`atlas` on PATH, `ATLAS_DB` set).

Atlas is a **local evidence engine**. It is the source of structural truth for this repository.
You are **not** allowed to invent architecture from grepping tests alone when Atlas can answer.

## When to use Atlas

Use Atlas for:

- where does X live / who owns Y
- how does payment / fulfillment / upload flow work
- storing files / GCS / data room / uploads
- who calls a method
- what implements an interface
- what product surfaces use storage/cache/messaging
- history of a file (commits, PRs, issues)

## Commands (prefer these over raw grep)

```bash
# Orientation
atlas map
atlas modules

# Primary localization packet (deterministic)
atlas investigate "your question" --no-ai

# Structural code-intel (new)
atlas callers tryEnqueue
atlas callers OrderFulfillmentService.tryEnqueue
atlas implementations IStorageProvider
atlas capabilities
atlas code-search ListingAsset

# Neighborhood
atlas impact path/to/file.ts
atlas structural path/to/file.ts --reverse
atlas focus src/modules/core
```

## Rules

1. For **flow** questions: call `atlas callers <method>` and name **production callers** + method.
2. For **storage / data-room / files**: call `atlas capabilities` and read the `storage` product_surfaces notes. Do **not** conclude from `fs.writeFile` in tests.
3. For **interfaces**: `atlas implementations IStorageProvider` (or path).
4. Cite paths Atlas returned. Mark unknowns. Do not invent PR numbers.
5. If `atlas` is missing, say so and fall back to read-only search — do not fake Atlas output.

## Epistemic status (Atlas language)

- OBSERVED = in structural_edges / git / files table
- DERIVED = computed (capabilities, implementation heuristics)
- PLAUSIBLE / UNRESOLVED = investigation hypotheses — not facts until grounded
