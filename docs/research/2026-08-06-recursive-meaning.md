---
title: The Unit of Understanding — Recursive Meaning as an Atlas Research Hypothesis
date: 2026-08-06
status: Research proposal
supersedes_in_part: 2026-08-06-logarithmic-traversal.md
---

# The Unit of Understanding — Recursive Meaning as an Atlas Research Hypothesis

## The correction

The foundational question is not “how should Atlas store understanding?” but
**“what is the smallest unit that counts as understanding?”**  Storage, graphs,
and retrieval follow from that answer.  A senior engineer does not normally
retrieve a list of imports to answer what `jwt.service.ts` does.  They use a
compact proposition: “this service issues and verifies JWTs.”  At a higher
resolution they compose propositions: “authentication accepts SIWE/Privy
identity and turns it into application authorization.”

This suggests a better research hypothesis:

> An Atlas *meaning* is a versioned, evidence-grounded, testable compressed
> model of a bounded part of a software system.  It predicts salient facts,
> behaviors, and interfaces of that part; it can be composed into a parent
> model; and it changes only when its predictive/explanatory adequacy changes.

The last clauses are essential.  A one-line description is an excellent
**rendering** of meaning, but it is not enough to be the stored unit.  “Handles
authentication” may be short and memorable but makes no falsifiable predictions
and has no stated scope.  Atlas must preserve a machine-checkable semantic
object underneath the one line, or it will only cache attractive prose.

## Why this is a serious direction

The intuition has real antecedents, though none directly solves codebase
understanding.

* In chess, Chase and Simon's classic study found that stronger players
  organized meaningful board configurations differently from novices; their
  task was specifically to isolate the perceptual structures players used
  ([1973](https://www.sciencedirect.com/science/article/abs/pii/0010028573900042)).
  The important qualification is that chunks are domain-trained structures, not
  arbitrary grouping.
* The semantic/episodic distinction is an unusually apt analogy.  Semantic
  memory concerns organized knowledge of symbols, meanings, relations and rules,
  while episodic memory concerns temporally situated events
  ([review of Tulving's distinction](https://pmc.ncbi.nlm.nih.gov/articles/PMC6993580/)).
  In Atlas, commits, blobs, tickets, and execution traces are episodes;
  maintained meanings are semantic knowledge.  Neither replaces the other.
* Program-comprehension research itself distinguishes a textual program model
  from a situation model; expertise, task, and time affect both
  ([Burkhardt, Détienne & Wiedenbeck](https://researchportal.ip-paris.fr/en/publications/object-oriented-program-comprehension-effect-of-expertise-task-an/)).
  This is closer to Atlas than the claim that engineers simply memorize file
  trees.
* Predictive processing offers a productive metaphor: hierarchical models send
  top-down predictions and receive bottom-up errors.  It is influential but
  remains contested as a comprehensive cognitive mechanism; recent critiques
  explicitly warn against treating it as a broad re-description rather than a
  mechanism ([critique](https://www.frontiersin.org/journals/human-neuroscience/articles/10.3389/fnhum.2025.1743028/full)).
  Atlas should borrow its engineering pattern—predictions plus error and
  precision—not claim to implement a brain.
* Compression has a rigorous counterpart.  Minimum Description Length selects
  an explanation using the joint cost of model and encoded data
  ([Grünwald](https://homepages.cwi.nl/~pdg/book/book.html)); the Information
  Bottleneck formalizes a trade-off between a compact representation and
  prediction of a relevant variable ([Tishby, Pereira & Bialek](https://mlanthology.org/misc/1999/tishby1999misc-information/)).
  These provide criteria, not an automatic ontology learner.

The novel claim for Atlas would be operational: preserve and incrementally
maintain such compressed models of software **with their evidence and error
history**.  Graphs, ASTs and documents then become the observational substrate,
not the product's primary cognitive artifact.

## Define the object before naming it

Call the object a **Meaning Unit** (MU) for research purposes, not yet a product
primitive.  An MU must contain more than text:

```
MeaningUnit
  scope:             a bounded, versioned set of observations / child MUs
  proposition:       one-line human rendering
  contract:          typed claims and expected observable consequences
  abstraction:       what details it intentionally omits
  evidence:          supporting observations and derivations
  counterevidence:   observed exceptions, ambiguity and known gaps
  dependencies:      child MUs and source observation fingerprints
  confidence:        calibration based on tests, not stylistic fluency
  revision:          validity range, producer/model/rule version
```

For example, the proposition might be “Issues and verifies JWTs for authenticated
users.”  Its contract could predict that the scoped code: (a) imports/uses a JWT
library or an equivalent signing primitive; (b) exposes issuance and validation
operations; (c) consumes a configured signing key; and (d) is invoked on an
identity-to-session path.  It should also state what it does *not* assert:
authorization policy and user identity proof may live elsewhere.

This makes the one line useful in both directions:

```
bottom-up: observations contradict/support contract → MU revision decision
top-down:  parent expectations select which child evidence to inspect
```

An MU is therefore neither a file, a graph node, an embedding, nor an LLM
summary.  It is a claim-bearing model whose members may be files, symbols,
contracts, documents, or other MUs.  Membership is many-to-many: one function
can support authentication and an audit trail.  The resulting *meaning graph*
is recursive and overlapping even if the user interface presents a convenient
expandable tree.

## The missing distinction: compression versus decoration

“Authentication” is not a valid abstraction merely because the name feels
right.  It earns existence when it improves a task.  A candidate MU must beat
its children along three dimensions:

| Test | Question | Failure signal |
|---|---|---|
| Compression | Does the parent reduce description complexity versus listing children? | It only repeats child names or needs a long caveat list. |
| Prediction | Does it correctly predict held-out salient properties of children/new changes? | It predicts JWT where a sibling has only API-key auth, or misses the actual common role. |
| Intervention | Does it help an investigator choose the next place to inspect/change? | It does not reduce search, explain an incident, or anticipate impact. |

This is a much stronger standard than clustering files with similar embeddings.
Similarity is a proposal mechanism; it cannot establish a semantic chunk.  It
also prevents the system from rewarding vacuous parents: “RWATP contains code”
is perfectly compressive only because it predicts nothing useful.

There is a second correction to the hypothesis.  A parent cannot reliably
predict *all* children in a living codebase.  Software contains historical
accident, optional plugins, migrations, and violations of its own architecture.
The target is a calibrated partial model: it must say what it predicts, with
what confidence, and preserve residuals rather than force exceptions into the
schema.

## Recursive maintenance: meaning delta, not source delta

The insight about propagation is sound, with a critical implementation detail:
do not ask an LLM “did the meaning change?” over raw diffs.  Compare a prior MU
contract against updated evidence, then selectively seek a revision.

```
changed source/commit
  → update observations and deterministic facts
  → identify directly dependent Meaning Units
  → evaluate each unit's predictions against the new evidence
  → unchanged adequacy: retain unit, attach new validity evidence
  → mismatch/new explanatory opportunity: propose a revised unit
  → test revision against sibling/parent contracts and held-out evidence
  → propagate only if the child's public contract or residual profile changed
  → stop when parent adequacy is unchanged
```

The unit of invalidation is not necessarily a function: it is the smallest MU
whose contract is affected.  The initial, safe granularity may be a file-level
MU.  Fine-grained symbols and cross-repo MUs should only be introduced when
benchmarks show that file-level revision causes unacceptable false propagation.

The propagation criterion matters.  A private refactor can alter every child
observation while preserving a parent's public model.  Conversely, a new
authentication mode can leave individual JWT files unchanged but invalidate the
parent's claim “all authentication uses JWT.”  Content hashes are necessary for
detecting source change; meaning-contract deltas decide whether higher levels
need reconsideration.

## How MUs are discovered without a fixed ontology

Discovery should produce candidates, never unreviewed truth:

1. Begin with deterministic observation anchors: containment, exports/imports,
   endpoints, topics, schemas, package manifests, co-change, and curated
   engineering documents.
2. Generate competing candidate partitions/overlaps from repeated structural
   patterns, lexical terms, contracts, historical co-change, and optionally
   embedding similarity.
3. Induce a proposed proposition and contract from the candidate's evidence.
4. Evaluate compression, prediction and intervention on benchmark questions and
   on changes not used to create it.
5. Retain only candidates with provenance, calibrated confidence, and a
   measurable advantage over their children; otherwise retain the observations
   and no abstraction.

LLMs have a legitimate role at steps 2–3: proposing vocabulary, alternatives,
and a compact rendering.  A deterministic verifier, tests, and human review
must remain the judges.  The LLM should also be able to return “no stable unit
found”; forced abstraction is a primary failure mode.

## A research program, not a product commitment

The first experiment should be deliberately small and falsifiable.

**Corpus.** Use the existing RWATP and VestaScan benchmarks plus a set of
historical changes with known review/incident descriptions.  Define a small
number of ground-truth investigation questions, not a universal capability
taxonomy.

**Conditions.** Compare (A) Atlas's current evidence retrieval, (B) the same
retrieval plus flat LLM file summaries, and (C) recursively maintained MUs with
contracts and revision checks.

**Measures.** Score answer correctness, evidence coverage, source reads,
planner branching factor, time to orient after a six-month simulated gap,
one-file update work, false parent updates, and stale-summary rate.  Also score
calibration: when the model says its meaning did not change, how often does a
blinded reviewer find a materially changed responsibility?

**Disconfirmation.** Abandon or narrow the theory if MUs do not outperform
ordinary evidence retrieval and summaries on held-out tasks; if their contracts
require so much manual authoring that compression disappears; if local changes
regularly cause global re-clustering; or if experts disagree too strongly about
the supposed unit.  “Meaning” is valuable only if it increases predictive and
interventional power, not because it resembles human intuition.

## Implications for the earlier architecture report

The fact ledger and provenance recommendations remain necessary.  They become
the episodic substrate from which semantic MUs are earned.  The earlier report
was incomplete in treating semantic slices chiefly as navigational views.  The
revised position is:

* an indexed graph/relational store answers *what was observed and connected*;
* a Meaning Unit answers *what this bounded region is for, what it predicts,
  and why that compression is justified*;
* navigation descends through MUs when they have been validated, and falls back
  to evidence-first traversal when they have not.

Thus, “one-line explanation” is not a UI convenience.  It is the human-facing
projection of the candidate unit of accumulated understanding.  Atlas's core
research challenge is to make that projection earned, composable, revisable,
and honest about its uncertainty.

## Selected references

* Chase, W. G. & Simon, H. A. (1973). [Perception in chess](https://www.sciencedirect.com/science/article/abs/pii/0010028573900042). *Cognitive Psychology*, 4(1), 55–81.
* Greenberg, D. L. & Verfaellie, M. (2010). [Semantic memory and the hippocampus](https://pmc.ncbi.nlm.nih.gov/articles/PMC6993580/).
* Pennington, N. (1987). [Stimulus structures and mental representations in expert comprehension of computer programs](https://www.cs.kent.edu/~jmaletic/cs69995-PC/papers/pennington87.pdf). *Cognitive Psychology*.
* Grünwald, P. (2007). [The Minimum Description Length Principle](https://homepages.cwi.nl/~pdg/book/book.html).
* Tishby, N., Pereira, F. & Bialek, W. (1999). [The Information Bottleneck Method](https://mlanthology.org/misc/1999/tishby1999misc-information/).
* Clarke, A. et al. (2025). [Predictive coding: mechanistic model or metaphorical re-description?](https://www.frontiersin.org/journals/human-neuroscience/articles/10.3389/fnhum.2025.1743028/full).
