---
title: Aider Repo Map — reference for Atlas C5.1 personalized structural ranking
date: 2026-08-09
status: Research
sources:
  - https://aider.chat/docs/repomap.html
  - https://aider.chat/2023/10/22/repomap.html
  - https://github.com/Aider-AI/aider/blob/main/aider/repomap.py
  - https://anishgandhi.com/aider-pagerank-codebase-ranking/
---

# Aider Repo Map — what to steal for Atlas

This is an implementation reference, not a product pitch. Goal: adapt
**personalized symbol-graph PageRank** into Atlas C4/C5 without importing
Aider, NetworkX, or a graph database.

## Problem Aider solves (maps to our C4 gap)

LLMs cannot take the whole repo. Aider must choose a **token-budgeted**
summary of the most relevant code for the current chat turn.

Atlas C4 still feeds a **bag of related evidence** (files, issues, commits).
Noise (Redis, image-processor) competes with true order subjects.

Aider’s answer: **parse → symbol graph → personalized PageRank → fit budget**.

---

## Pipeline (end-to-end)

```text
Source files (working tree)
        │
        ▼
Tree-sitter parse + language tags.scm queries
        │
        ▼
Tags: (rel_fname, fname, line, name, kind∈{def,ref})
        │
        ▼
Aggregates:
  defines[ident]  → set of files that define ident
  references[ident] → list of files that reference ident
  definitions[(file,ident)] → Tag objects (for line numbers)
        │
        ▼
Personalization vector over FILES (chat + mentions)
        │
        ▼
MultiDiGraph: edge referencer → definer, weighted by ident
        │
        ▼
networkx.pagerank(G, weight="weight", personalization=…)
        │
        ▼
Distribute file rank onto (definer_file, ident) pairs
        │
        ▼
Sort tags by rank; binary-search prefix length to fit max_map_tokens
        │
        ▼
Render: per-file skeleton with TreeContext lines-of-interest
```

Key source: `aider/repomap.py` class `RepoMap`.

---

## Data structures

### Tag

```text
Tag = (rel_fname, fname, line, name, kind)
  kind: "def" | "ref"
```

### Graph nodes

**Files** (relative paths), not symbols.

### Graph edges

For each identifier `ident` that has **both** at least one def and one ref:

```text
for each (referencer_file, count) in Counter(references[ident]):
  for each definer_file in defines[ident]:
    add_edge(referencer → definer,
             weight = mul(ident) * sqrt(count),
             payload: ident)
```

Also: self-edges weight `0.1` for defs that never appear as refs
(tree-sitter version quirks).

If **no refs at all** in the repo after parse: treat every def as a self-ref
list so the graph is non-empty.

---

## Edge weight multipliers (`mul`)

Applied **per identifier**, then × reference count (sqrt-scaled):

| Condition | Multiplier |
|-----------|------------|
| Identifier mentioned in user chat text | **×10** |
| “Specific” name: snake/kebab/camel **and** `len(ident) ≥ 8` | **×10** |
| Name starts with `_` (private-ish) | **×0.1** |
| Defined in **>5 files** (ubiquitous / collision) | **×0.1** |
| Referencer file is already in the **chat** | **×50** extra on that edge |
| Reference frequency | `sqrt(num_refs)` not raw count |

Personalization on **nodes** (separate from edge mul):

```text
base = 100 / num_files   # personalize unit

if file in chat_files:           personalization[file] += base
if file in mentioned_fnames:     personalization[file] = max(..., base)
if any path component / basename intersects mentioned_idents:
                                 personalization[file] += base
```

PageRank call:

```python
nx.pagerank(G, weight="weight",
            personalization=personalization,
            dangling=personalization)  # when personalization non-empty
```

Fallback without personalization if vector empty; catch `ZeroDivisionError`.

---

## Rank distribution onto symbols

After PageRank scores each **file** `src`:

```text
for each out-edge src → dst with weight w:
  edge_rank = page_rank[src] * w / sum(out weights of src)
  ranked_definitions[(dst, ident)] += edge_rank
```

Then sort `(file, ident)` by score descending. Emit Tag defs for those
idents (skip files already fully in chat). Pad with high PageRank files
that had no tags, then remaining files.

---

## Extraction: tree-sitter tags queries

Aider does **not** invent a semantic type system. It runs SCM query files
shipping under `aider/queries/`:

- `tree-sitter-languages/{lang}-tags.scm`
- or `tree-sitter-language-pack/{lang}-tags.scm`

Capture naming convention:

- `name.definition.*` → kind `def`
- `name.reference.*` → kind `ref`

### TypeScript (high-value for RWATP) — defs include

- function / method / abstract method signatures  
- class / abstract class / interface / type alias / enum / module  

Refs include (in TS file):

- type annotations (`type_identifier`)  
- `new Foo` constructors  

**Important gap vs Atlas:** TS tags file is **lighter on call refs** than
Rust’s (which has `call_expression`). JS tags emphasize defs; many refs
may come from **pygments backfill** when defs exist but no refs were seen.

### Pygments ref backfill

If a file saw defs but **zero** tree-sitter refs:

```text
lexer.get_tokens(code) → all Token.Name → emit as kind=ref, line=-1
```

Noisy but keeps PageRank connected for languages with def-only queries.

### Caching

- Disk: `diskcache` under `.aider.tags.cache.v{3|4}/` keyed by absolute path  
- Invalidate on `mtime`  
- CACHE_VERSION bumps with tree-sitter language pack  

Atlas parallel: store tags in SQLite at ingest time (we already re-ingest
structural edges).

---

## Personalization inputs from chat

From `base_coder.py`:

| Function | Behavior |
|----------|----------|
| `get_ident_mentions(text)` | `re.split(r"\W+", text)` → set of tokens |
| `get_file_mentions(content)` | words matching addable relative paths / basenames |
| `get_ident_filename_matches(idents)` | ident length ≥5 matching `Path.stem` of any repo file |

Then:

```text
mentioned_fnames += get_ident_filename_matches(mentioned_idents)
repo_map.get_repo_map(chat_files, other_files,
                      mentioned_fnames, mentioned_idents)
```

**Atlas analog for `"orders timeout"`:**

```text
mentioned_idents = {orders, timeout, …}  # existing anchors_from_question
mentioned_fnames = paths whose components hit anchors
chat_files = seed files / core candidates already selected
```

---

## Fitting the token budget

Default `--map-tokens` ≈ **1024**.

If **no files in chat** and context window known:

```text
max_map_tokens = min(map_tokens * map_mul_no_files, context - 4096)
```

**Constants caveat (do not copy blindly into Atlas):**

| Source | `--map-tokens` default | no-files multiplier |
|--------|------------------------|---------------------|
| Aider **docs** (`options.html`, 2026) | 1k | **2** (`--map-multiplier-no-files`) |
| Aider **source** `RepoMap.__init__` (as of research fetch) | `map_tokens=1024` | constructor arg default was **8** in one snapshot |

Treat Aider’s numbers as **inspiration only**. Atlas should cap ranked evidence by **count / relevance**, not by cloning Aider’s token knobs.

Binary search on number of ranked tags:

```text
middle tags → to_tree() → token_count
stop when within ~15% of budget or best under budget
```

### Rendering (`to_tree` + `TreeContext`)

Not full files. For each selected file:

```text
path/to/file.ts:
⋮...
│export class OrderService {
│  async createOrder(...) {
⋮...
```

`grep_ast.TreeContext` expands lines-of-interest with parent scopes,
collapses the rest with `⋮...`. Lines truncated to 100 chars (minified JS).

Special files (`package.json`, `README.md`, `Cargo.toml`, …) from
`filter_important_files` are **prepended** to the ranked list.

---

## Dependencies / stack

| Piece | Aider uses | Atlas should use |
|-------|------------|------------------|
| Parse | tree-sitter + grep-ast | Existing Atlas parsers / tree-sitter |
| Graph | networkx MultiDiGraph | Pure Rust PageRank on sparse adjacency (no NetworkX) |
| Cache | diskcache SQLite | Atlas SQLite tables |
| Render | TreeContext skeletons | Optional later; C5.1 can emit ranked file/symbol list into EvidencePacket |
| License | Apache-2.0 (aider) | Steal algorithm; do not copy large source blobs without review |

---

## What we do **not** need from Aider

- Chat session / edit formats / whole Coder agent  
- GPT token estimators for ranking (we can rank by score then cap N files)  
- diskcache directory in the target repo  
- Binary-search token fit on first iteration (cap top-K files + top-K symbols)  
- Embedding / vector search (Aider explicitly is topology-only)

---

## Atlas mapping (recommended C5.1 design)

### Option A — Fast path (use edges we already have)

Do **not** wait for full tags.scm port.

```text
Nodes: files (already in structural_edges + files table)
Edges: IMPORTS / CALLS_* / REFERENCES_MODEL already in SQLite
  direction: source → target (caller/importer → callee/imported)

Personalization:
  anchors_from_question → boost files whose path/name contains anchors
  seed_files / core_candidates → chat-like ×50 neighborhood bias
  mentioned idents (order, timeout, …) → boost edge mult if edge/symbol text matches

Weight:
  edge_kind weights (calls > imports > model refs) × sqrt(multiplicity)
  × mention multipliers (Aider-style)

PageRank → ranked_files
Distribute to structural neighbors already in investigation packet
Replace bag dump with top-K ranked_evidence from this score
```

This can ship without new parsers and still tests the hypothesis:
**personalized structural rank beats lexical concept-expansion noise.**

### Option B — Full Aider parity (later)

1. Ingest phase: tags.scm (TS/JS/Rust/Python) → `symbol_tags` table  
   `(repo_path, file, line, name, kind def|ref)`  
2. Build defines/references maps as Aider  
3. Same PageRank math in Rust  
4. Optional skeleton render for investigate packet  

### Hybrid (likely best)

- C5.1a: Option A on current edges + question personalization  
- C5.1b: add symbol def/ref tags for TypeScript only (RWATP)  
- Re-run sacred + SWE-Explore-style top-5 localization  

---

## Worked example: `"orders timeout"` under Aider rules

Personalization / mention effects we should expect:

| Signal | Effect |
|--------|--------|
| idents `orders`, `timeout` | ×10 on edges whose `ident` matches; path components `order*` get node boost |
| basename `order.service` | `get_ident_filename_matches` if stem ≥5 and word matches |
| core candidate already selected | treat as chat file → ×50 on outgoing edges |
| issue#19 “Redis…Timeout” | string “timeout” also boosts Redis-related idents/paths — **same pitfall we have**; need Atlas-side demotion of cross-domain *or* require multi-token co-occurrence |

Aider alone does **not** solve cross-domain Redis. It ranks topology + name mentions. **C4 verification still required** for causal claims; Repo Map only improves *which* files enter the packet.

---

## Limitations (honest)

1. **Name-based, not type-resolved.** `create` collisions; Aider damps idents defined in >5 files.  
2. **Topology ≠ semantics.** No vector similarity; related-but-unlinked files miss.  
3. **TS ref coverage weaker than call graphs.** Atlas `CALLS_STATIC` may be *better* than Aider’s TS tags for call edges.  
4. **Working-tree only** (like our structural snapshot).  
5. **No history / issues / PRs.** Atlas must keep C4 chronology layer *beside* the map.  

---

## Concrete constants to port (first implementation)

```text
PERSONALIZE_UNIT     = 100 / n_files
CHAT_NODE_BOOST      = +PERSONALIZE_UNIT  (or mark as chat)
MENTION_IDENT_EDGE   = 10.0
SPECIFIC_NAME_EDGE   = 10.0   # snake/kebab/camel and len>=8
PRIVATE_NAME_EDGE    = 0.1    # starts with _
UBIQUITOUS_DEF_EDGE  = 0.1    # defined in >5 files
CHAT_REFERRER_EDGE   = 50.0
REF_COUNT_SCALE      = sqrt
SELF_EDGE_WEIGHT     = 0.1
MAX_RANKED_FILES     = 20     # Atlas packet cap (not Aider map-tokens)
# Do NOT port Aider map_mul_no_files (docs=2, source may differ)
```

PageRank: damping default NetworkX `alpha=0.85` is fine unless we have reason to change.

---

## Evaluation hooks (pair with C5.0)

For each golden question:

1. Run personalized rank → top-10 files  
2. Score hit rate vs hand-labeled relevant set  
3. Count Redis / rate-limit files in top-10 for `"orders timeout"` (should drop vs current bag)  
4. Sacred: still zero SUPPORTED Redis causal claims under C4  

---

## Source index

| Resource | URL |
|----------|-----|
| Docs | https://aider.chat/docs/repomap.html |
| Blog (2023) | https://aider.chat/2023/10/22/repomap.html |
| Implementation | https://github.com/Aider-AI/aider/blob/main/aider/repomap.py |
| Tags queries | https://github.com/Aider-AI/aider/tree/main/aider/queries |
| Mention extraction | `aider/coders/base_coder.py` (`get_ident_mentions`, …) |
| Important files list | `aider/special.py` `ROOT_IMPORTANT_FILES` |
| TreeContext | https://github.com/Aider-AI/grep-ast |
| Explainer | https://anishgandhi.com/aider-pagerank-codebase-ranking/ |
| NetworkX PageRank | https://networkx.org/documentation/stable/reference/algorithms/link_analysis.html |

---

## Recommended Atlas next step (from this research only)

1. Implement **personalized PageRank over existing `structural_edges`** (Option A).  
2. Personalize with `anchors_from_question` + seed/core candidates.  
3. Write results into `EvidencePacket.ranked_evidence` (replace or re-weight C4 rank).  
4. Do **not** block on tags.scm or NetworkX.  
5. Measure with golden top-k before adopting full symbol tags or agent tools.
