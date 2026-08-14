---
title: Logarithmic Traversal and Scalable Knowledge Representation for Atlas
date: 2026-08-06
status: Research proposal
---

# Logarithmic Traversal and Scalable Knowledge Representation for Atlas

## Thesis

Atlas should be a **versioned evidence store with incrementally maintained
derived views**, not a graph database whose primary objects are asserted
architectural concepts.  Its navigation surface should be a hybrid:

1. deterministic containment and program facts provide the hard, auditable
   substrate;
2. materialized relational indexes make common narrowing steps cheap;
3. a small number of typed graph traversals cross relationships that SQL joins
   do not express readably; and
4. inferred semantic groupings are explicitly provisional, ranked, and
   reproducible from their evidence.

This retains the central Atlas proposition—understanding that has been earned
should survive—without claiming that an LLM's present interpretation is an
enduring fact.  It also answers the scaling requirement more precisely than
“logarithmic traversal”: cost should be proportional to the **affected slice**
for updates and to the **candidate frontier** for queries, not to all files.
No general semantic system can guarantee `O(log N)` for arbitrary natural
language questions.  A high-degree concept such as “authentication” can
legitimately touch much of a large estate.  The engineering goal is predictable
bounded fan-out, early pruning, and visible degradation when the evidence does
not support a narrow answer.

The proposal is deliberately conservative about Atlas's present maturity.  The
existing IR already separates entities, relationships, and evidence, and the
Project work correctly established cross-repository *observation* without
pretending that repo membership implies system understanding.  The next
primitive should be a narrow, benchmarked derived-slice mechanism—not a global
`Capability` ontology.

## 1. The problem, stated operationally

An engineering question has two different jobs:

* **retrieval:** find the small set of objects likely to matter;
* **explanation:** show why those objects are related and why Atlas chose them.

Search engines are good at the first when lexical anchors exist.  Program graphs
are good at the second when the relation is syntactic, typed, or data-flow based.
Neither alone preserves an architectural model across history, repositories,
and changing names.  Atlas's differentiator can be to retain the chain:

```
raw observation → normalized fact → derived view → query plan → answer claim
        ↑                                                    │
        └──────────────────── provenance ────────────────────┘
```

The hard problem is therefore not “store a hierarchy.”  It is maintaining
multiple, partly incompatible views over changing evidence while never making
the view more authoritative than its sources.

### A necessary correction to the map metaphor

Google Maps works because the world supplies stable geography and a mostly
tree-shaped containment hierarchy.  Software has several overlapping maps:

```
physical:      project / repository / directory / file / symbol
program:       import, call, type, inheritance, read/write, generated-from
operational:   package, service, topic, endpoint, database, deployment
historical:    commit, rename lineage, PR, decision, author
semantic:      recurrent slice labels and summaries (uncertain)
```

“JWT middleware” may be a directory, a library import, a call chain, an API
contract and an historical change cluster simultaneously.  Forcing these into
one parent-child tree loses information.  Atlas should offer a *navigation
tree* for orientation, backed by an attributed multigraph for truth.

## 2. Prior work and what transfers

Atlas is not unprecedented; it combines mature ideas that normally live in
separate products.  Its potentially distinctive contribution is the union of
durable engineering evidence, temporal identity, and explainable,
cross-repository investigation—not the use of a graph or of LLM labels.

| Adjacent work | Established idea | Transfer to Atlas | Do not copy |
|---|---|---|---|
| [Glean](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/) | Language indexers emit facts; derived facts are queried; database stacks can add or hide information non-destructively. | Treat parser output as facts and each analysis as a derived layer with a schema and producer version. | A global, build-integrated developer platform before Atlas has validated its first cross-repo edge. |
| [Sourcegraph SCIP](https://sourcegraph.com/docs/code-navigation/precise-code-navigation) | Compiler-accurate definition/reference indexes; precise navigation with search fallback. | Import or emit symbol/document identities where feasible.  Use lexical search as explicitly lower-confidence fallback. | Equating symbol navigation with architectural understanding. |
| [Code Property Graphs](https://docs.joern.io/code-property-graph/) | AST, control flow and data flow coexist as typed graph overlays and are queried across views. | Keep overlays separate: structural facts first, expensive flow overlays only on demand or selected languages. | Statement/expression-level universal CPG ingestion: it is storage-expensive and unnecessary for most investigations. |
| [Tree-sitter](https://tree-sitter.github.io/tree-sitter/) and [LSP incremental sync](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/specification.md) | Reuse unchanged syntax structure; transmit edits as deltas. | Use content hashes and a changed-region boundary; reparsing a file is already a worthwhile first granularity. | Treating a parse tree as semantic resolution, particularly in dynamic languages. |
| [rustc's query system](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html) and [Salsa](https://salsa-rs.github.io/salsa/) | Demand-driven queries record dependencies and validate/recompute only affected results. | Every derived Atlas fact needs declared inputs, analysis version, and invalidation keys. | Building a general incremental-computation runtime before actual invalidation patterns are measured. |
| [DBSP](https://arxiv.org/abs/2203.16684) and [F-IVM](https://arxiv.org/abs/1703.07484) | Materialized queries can be maintained from deltas; factorization lowers maintenance cost. | Represent queryable slices and counters as materialized views with local delta maintenance. | Assuming arbitrary clustering/LLM summaries admit exact delta maintenance. They do not. |
| Git | Content-addressed immutable objects, commit DAG, rename detection as heuristic rather than identity. | Use immutable observations keyed by revision/content; keep lineage as evidence with confidence, never overwrite history. | Path as entity identity or a single “canonical rename” when history is ambiguous. |
| Datalog/static analysis | Relations plus rules derive reachability, points-to and dependencies. | A restricted rule layer is a strong later fit for deterministic contracts and slices. | Unbounded recursive traversal without cost limits or provenance. |
| GraphRAG | Retrieve graph neighborhoods, then let a model synthesize. | The planner may expand typed neighborhoods after deterministic anchoring. | Pre-generating a semantic graph from embeddings and calling it evidence. |

Neo4j and other graph databases are useful products, but not an architectural
answer.  They make some traversals pleasant; they do not establish node identity,
incremental derivation, ontology correctness, or a query planner.  SQLite can
remain Atlas's first engine because its B-trees, FTS5, transactions, window
functions, recursive CTEs, and ordinary joins cover the proposed MVP.  The
decision to change the serving engine should follow a measured workload: e.g.,
multi-hop concurrent reachability that cannot meet latency targets with a
materialized frontier table.

## 3. Proposed representation: fact ledger + overlays + serving indexes

### 3.1 Three classes of assertion

Every stored statement must declare its epistemic status.

| Class | Example | May be used as a hard filter? | Retention |
|---|---|---:|---|
| Observed | file blob has path at commit; AST contains import; manifest names a package | yes | immutable/versioned |
| Derived deterministic | symbol belongs to parsed file; import resolves to package; a topic producer binds to a literal topic | yes, with producer version | refreshable |
| Inferred | “this slice is authentication”; LLM summary; embedding cluster | only as a ranked candidate generator | replaceable, expiry-aware |

Evidence should therefore not merely point from an entity to raw text.  A claim
needs a provenance record: input observation IDs, extractor and version,
configuration, timestamp, confidence/calibration method, and invalidation
policy.  This makes a later correction a new derivation rather than a mutation
of truth.

### 3.2 Identity: stable where possible, plural where necessary

Paths are locations, not identities.  Nor is a function name a reliable
identity.  Atlas needs distinct identities for an observed artifact, a logical
lineage, and a semantic symbol:

```
RepositoryIdentity (remote origin + durable local registration)
  └─ Revision (commit SHA / working-tree snapshot)
       └─ BlobObservation (git blob SHA, revision, path)
            └─ FileLineage (stable Atlas UUID; many observations)
                 └─ SymbolObservation (parser/language symbol at revision)
                      └─ SymbolLineage (only when resolution proves continuity)
```

* A Git blob SHA is excellent for identical content and survives a move, but not
  an edit.
* A `FileLineage` is created from an observed rename, exact blob continuity, or
  a separately recorded similarity rule.  It has **multiple candidate links**
  when extraction or split/merge is ambiguous.
* `SymbolObservation` is keyed by language, enclosing file observation, syntax
  kind, selection range, normalized signature, and content hash.  Semantic
  indexer IDs (SCIP or compiler IDs) are preferred when present.
* A refactor produces `supersedes`, `split_from`, or `merged_from` relations;
  it must not silently reuse an old identifier.  Logical continuity is a
  relationship with provenance, not a property guessed from a path.

This model intentionally permits identity uncertainty.  A system that demands
one immutable identity for every refactored function will manufacture false
continuity precisely where an investigator needs doubt displayed.

### 3.3 Logical schema

The following is relational in storage and graph-shaped in use.  It is a sketch,
not a request to add all tables now.

```
observations(id, repo_id, revision_id, kind, locator, content_hash, payload_ref)
entities(id, kind, created_from_observation, lifecycle)
entity_versions(entity_id, valid_from_revision, valid_to_revision, attrs_json)
relations(id, src_entity, dst_entity, kind, valid_from, valid_to, status)
provenance(subject_kind, subject_id, observation_id, producer, producer_ver,
           config_hash, confidence, created_at)
dependencies(derived_subject, input_subject, dependency_kind)
invalidation_queue(subject, reason, revision)

-- serving/index layer, always rebuildable from the above
symbol_lookup(repo_id, language, normalized_name, symbol_id, revision_range)
edge_by_src(src_id, kind, dst_id, revision_range)
edge_by_dst(dst_id, kind, src_id, revision_range)
containment_closure(ancestor_id, descendant_id, depth, revision_range)
fts_documents(entity_id, text, source_kind)
slice_membership(slice_id, entity_id, score, method, derivation_id)
```

`attrs_json` is useful for evolving extractor payloads but must not become the
primary query model.  Promote fields to typed columns and indexes only after a
benchmark repeatedly needs them.  This follows Atlas's maturity ladder and
avoids an invented universal ontology.

### 3.4 Do layers become the traversal engine?

Yes, but only as **derived navigation indexes**, not as the source of truth.
The physical containment layer is deterministic and can offer logarithmic
lookup through B-tree indexes.  A semantic layer should be a view containing
membership scores and explanations, not an exclusive hierarchy.  A node may
appear in several slices; selecting a slice means choosing a start frontier,
not asserting a parent.

```
                    Query / planner
                         │
      ┌──────────────────┼──────────────────┐
      ▼                  ▼                  ▼
  FTS + lexicon    structured filters    semantic candidates
      └──────────────────┼──────────────────┘
                         ▼
                typed frontier (bounded)
                         ▼
       containment / dependency / contract traversal
                         ▼
           evidence bundle + ranked explanation
```

The planner should prefer cheap, high-precision anchors (exact symbol, endpoint,
topic, package, changed file) before broad text, embeddings, or LLM labels.
Every edge expansion has a relationship allow-list, depth budget, fan-out cap,
time/revision scope, and a reason recorded in the answer.

## 4. Building abstractions without inventing an ontology

There is no single correct source for a capability.  Use a ladder of evidence:

1. **Deterministic containment:** repositories, packages, directories, files,
   declared symbols.  Always build this.
2. **Deterministic technical roles:** routes, RPC/topic names, database models,
   package dependencies, framework registrations.  Add one extractor only after
   a corpus-backed question requires it.
3. **Explicit domain vocabulary:** names from code, ADRs, tickets, API schemas,
   and existing docs.  Store as lexicon observations, including source and
   spelling variants.
4. **Candidate slices:** join the above facts using a versioned rule or a
   clustering model.  A candidate says “these members cohere under these
   signals,” not “this is the Orders capability.”
5. **Human/LLM names and summaries:** attach to the candidate with citations,
   confidence and expiry.  A label cannot be its own evidence.

An LLM is valuable at two narrow tasks: proposing terminology and explaining a
selected, evidence-bounded slice.  It is weak as an unrestricted ontology
author: labels drift, merges are hard to reverse, and output changes with model
version.  Clustering is useful for discovery and evaluation, but brittle as a
primary navigation address because clusters change globally after local data
changes.  Deterministic rules are auditable and incrementally maintainable but
miss conventions.  The correct answer is a hybrid with the asymmetric trust
model above.

## 5. Update propagation

### 5.1 Change protocol

For a commit or working-tree edit, Atlas should process a delta, not re-ingest
the universe:

```
Git diff / editor change
  → revision + blob observations
  → changed-file parse and symbol/edge diff
  → invalidate direct dependents using dependencies
  → recompute dirty deterministic views in topological order
  → refresh affected serving indexes
  → mark inferred slices stale; selectively rescore bounded candidates
  → retain old derivations as historical facts
```

The essential missing abstraction is not “capability”; it is a **derivation
dependency record**.  A derived `topic_binding` depends on a symbol observation
and a literal observation.  A file summary depends on its blob.  A project
contract index depends on bindings.  The reverse dependency index answers
exactly what can become stale.

### 5.2 What changes when one function changes?

| Input change | Recompute synchronously | Mark dirty / recompute on demand | Do not recompute |
|---|---|---|---|
| function body only | file parse; that function's local facts; outgoing calls/uses | callers or data-flow analyses if their dependency rule says so; summaries containing it | unrelated files, project census |
| exported signature | file parse; symbol resolution; direct references | transitive type/check or API-contract consumers | unrelated components |
| moved/renamed file | path observation; containment; lineage candidates; imports affected by resolution | directory/slice memberships tied to path | unchanged blob-derived facts |
| manifest/contract change | dependency/endpoint/topic bindings in that repo | matched bindings in registered project repos | all projects globally |
| model/prompt change | none of the deterministic facts | inference outputs produced by that model | parser and historical observations |

This is query-based compilation applied to knowledge derivation: validate a
dependency fingerprint before reuse; recompute only after an input changed.
Database IVM supplies the complementary rule: maintain selected aggregate and
join views from insertion/deletion deltas.  Do **not** blindly maintain every
possible view: storage and write amplification would dominate the benefit.

### 5.3 Complexity and the honest limit

Let `Δ` be changed observations, `d` the dependency closure actually affected,
`EΔ` changed edges, and `k` the returned result size.  With proper indexes:

* direct fact replacement is approximately `O(|Δ| log N + |EΔ| log N)`;
* deterministic maintenance is `O(|d|)` plus the cost of the selected joins;
* lookup by exact ID/name is `O(log N + k)`;
* bounded traversal is `O(Σ frontier_i + k)`, not `O(log N)` in the general
  case; and
* an unconstrained transitive closure remains potentially `O(V + E)`.

The desired invariant is therefore `update_cost ≈ affected_closure`, with an
explicit budget.  Dense hubs, wildcard imports, generated code, and shared
libraries make `d` large.  Atlas must surface that fact instead of hiding a
repository-wide recomputation behind an “incremental” label.

## 6. Query architecture

Natural language is an interface, not the query language of record.  Convert it
to a typed, inspectable plan:

```
question
  → anchor extraction (symbols, terms, paths, contracts, time)
  → candidate retrieval (FTS / exact / vector optional)
  → planner chooses typed expansions and budgets
  → execute SQL joins + bounded graph frontier queries
  → rank by evidence strength and relevance
  → synthesizer cites observations, edges, uncertainty and omissions
```

An example plan is more valuable than an opaque “authentication” traversal:

```
Question: Where does a successful order completion cause notification delivery?
Anchors: event names {order-completed, completed}; project RWATP
1. FTS/lexicon → topic literals and producer/consumer symbols
2. contract_binding(topic) across project repositories
3. expand only {PUBLISHES, SUBSCRIBES, CALLS_STATIC, CONTAINS}
4. collect defining files, commits/PRs that introduced the bindings
5. explain the producer → topic → consumer path; report unmatched variants
```

SQLite is adequate for the first versions: a `frontier(node_id, depth)` temporary
table, indexed edge tables, recursive CTEs for shallow paths, and FTS5 for text.
Recursive CTEs should be depth-capped; for richer search, run a bidirectional
BFS in the application with batched `IN` queries, duplicate elimination, and a
strict frontier budget.  Materialize only recurring traversals demonstrated by
benchmarks (for example, direct contract bindings), not arbitrary reachability.

## 7. Critique and failure modes

The proposed ambition fails if it confuses faster navigation with comprehension.

* **The ontology trap.** “Capabilities” look intuitive but are unstable,
  overlapping, organization-specific, and sometimes contested.  Prematurely
  naming them creates false precision and expensive migration.  Countermeasure:
  navigable candidate slices with provenance, never required parents.
* **The graph-everything trap.** Statement-level AST/CFG/PDG graphs can exceed
  source size by orders of magnitude; broad traversal becomes a denial-of-service
  primitive.  Countermeasure: store symbols and selected edges first; make
  expensive overlays language- and question-specific.
* **The semantic-cache trap.** An LLM summary is a cache with a hidden dependency
  set.  After a change it can be plausible and wrong.  Countermeasure: content
  fingerprints, cited member sets, stale state, TTL/model version, and no hard
  filtering based solely on summaries.
* **The incremental-is-free trap.** Fine granularity increases dependency and
  provenance metadata, invalidation fan-out, write amplification, and debugging
  complexity.  Countermeasure: start at file and declared-symbol granularity;
  measure invalidation closure before subdividing.
* **The cross-repo matching trap.** Same strings are not contracts; different
  strings can be the same integration.  Countermeasure: scope bindings to a
  Project, require extractor-specific evidence, preserve unmatched candidates,
  and benchmark false joins.
* **The history trap.** Git rename detection is heuristic and rebases/rewrite
  histories occur.  Countermeasure: append facts by revision, support lineage
  alternatives, and avoid destructive identity merges.
* **The central SQLite trap.** A single embedded database has a single-writer
  operational shape and is not a fleet-wide ingestion service.  Countermeasure:
  use per-repo/project shards and immutable extraction artifacts first; separate
  ingest workers from query replicas only when benchmarks force it.
* **The evaluation trap.** Faster retrieval can produce a convincing wrong slice.
  Countermeasure: benchmarks score recall, precision, evidence completeness,
  stale-answer rate, update latency, and investigator time—not only command time.

## 8. A staged, evidence-led roadmap

This sequence respects the repository's existing rule that primitives must be
earned by repeated investigation failures.

### POC: prove one concrete cross-repository path

Choose the existing RWATP order-completion/notification benchmark.  Add only
the evidence needed to answer it: a `ContractBinding` for one protocol (Pub/Sub
is a good candidate), with literal/location evidence on both producer and
consumer.  Query it with normal SQL joins.  Record precision, missed aliases,
and manual reads.  Repeat on VestaScan or a second architecture before naming a
general contract abstraction.  Deliver a before/after benchmark, not a
hierarchy.

### MVP: introduce derivation and slice infrastructure

1. Add immutable revision/blob observations and file/symbol observation IDs.
2. Add `derivation` and reverse `dependency` records to every new extractor.
3. Re-ingest by Git diff: preserve historical validity ranges and only parse
   changed files; start with whole-file replacement if incremental AST edits are
   not yet justified.
4. Add exact symbol and contract indexes, FTS, indexed edge tables, and a
   bounded traversal API that returns its evidence plan.
5. Add a `Slice` as a **derived result**: membership, method, score, evidence,
   derivation version.  Implement one deterministic slice rule observed in at
   least two benchmarks; do not add a generic capability command.
6. Measure cold ingest, one-file update, rename, query p50/p95, storage per
   source byte, and answer-evidence coverage on both validation repositories.

### Production: make correctness and operations first-class

* Run extraction from commits/CI so code intelligence shares the real build
  context, as precise indexers commonly do.
* Make per-repo databases/shards independently updatable, with a project catalog
  of immutable artifact versions and a snapshot-consistent query epoch.
* Add backpressure, resumable jobs, integrity checks, schema/extractor
  migrations, observability for invalidation fan-out, and deterministic replay.
* Introduce authorization boundaries before project-wide evidence is served; a
  cross-repo traversal must never leak a private repository through a joined
  edge.

### Research: only after the foundations produce data

Study learned slice discovery against a labeled corpus of real investigations:
compare lexical/structural rules, embeddings, graph clustering, and LLM planning
on recall, calibration, churn under commits, and explanation fidelity.  Explore
incremental Datalog for contract propagation and program analyses.  Consider a
separate graph-serving engine only after traces demonstrate that SQLite's
indexed, bounded traversal is the measured bottleneck.  The research outcome
may be that hierarchical semantic navigation is useful only for certain classes
of question; that is a valuable negative result.

## 9. Decision criteria and open questions

Before adopting any proposed layer, Atlas should be able to answer:

1. Which two real investigations fail without it, and what is their ground
   truth?
2. What raw observations support each output row?
3. What is the entity's identity through a rename, split, and merge?
4. What exact input changes invalidate it, and how large was the observed
   invalidation closure?
5. Is it a hard fact, deterministic derivation, or hypothesis?
6. Can a query show why this candidate was included and what was pruned?
7. What happens when the extractor, model, or repository history changes?

Open questions worth experiments, not commitments, include: whether contract
matching should be rule-based or type-aware per protocol; how to calibrate and
expire LLM-generated summaries; whether semantic candidate labels become stable
enough to be user-facing addresses; the useful granularity for symbol lineage in
dynamic languages; and the threshold at which project-level serving should move
beyond SQLite.

## Conclusion

Atlas should not attempt to “understand a codebase” by erecting a universal
tree.  It should accumulate versioned evidence, derive explicit overlays,
maintain the ones that recurring questions prove valuable, and navigate through
bounded typed frontiers.  That is less glamorous than a capability graph, but
it is the route to a system that can explain, correct, and incrementally update
its understanding.  The novel opportunity begins where Atlas joins this
discipline to retained investigation artifacts and cross-repository evidence;
the graph, the compiler-style invalidation, and the semantic search individually
are established prior art.

## Selected references

* Budiu, McSherry, Ryzhyk, and Tannen, [*DBSP: Automatic Incremental View
  Maintenance for Rich Query Languages*](https://arxiv.org/abs/2203.16684),
  VLDB 2023.
* Nikolic and Olteanu, [*Incremental View Maintenance with Triple Lock
  Factorization Benefits*](https://arxiv.org/abs/1703.07484), SIGMOD 2018.
* Yamaguchi et al., [Code Property Graph
  specification](https://cpg.joern.io/); see also [Joern's CPG
  overview](https://docs.joern.io/code-property-graph/).
* Meta, [*Indexing code at scale with
  Glean*](https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/),
  2024.
* Sourcegraph, [Precise Code Navigation and
  SCIP](https://sourcegraph.com/docs/code-navigation/precise-code-navigation).
* Rust Compiler Development Guide, [Incremental compilation in
  detail](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation-in-detail.html);
  [Salsa](https://salsa-rs.github.io/salsa/).
* Microsoft et al., [Language Server Protocol
  specification](https://microsoft.github.io/language-server-protocol/).
