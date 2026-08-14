# C5.0 Localization Gold Schema

Each case is a JSON file under `eval/localization/`.

## Fields

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `id` | string | yes | Stable id (`rwatp-…`) |
| `repository` | string | yes | Logical repo name (`rwatp-core`) |
| `workflow` | string | yes | `bug_localization` \| `system_flow` \| `issue_implementation` |
| `question` | string | yes* | Free-text question for `atlas investigate` |
| `issue` | number | yes* | Alternative to question: `atlas investigate --issue N` |
| `notes` | string | yes | Why these labels; what “relevant” means |
| `gold_files` | string[] | yes | Paths an engineer **should** open first (must-hit) |
| `gold_optional` | string[] | no | Useful but not required for a pass |
| `hard_negatives` | string[] | yes | Paths that should **not** appear in top-k (wrong domain) |
| `gold_documentary` | string[] | no | e.g. `issue#12`, `pr#16` that should rank when relevant |
| `c4_expectations` | object | no | Sacred / status constraints |
| `metrics` | object | no | Overrides (`top_k`: [1,5,10]) |

\* Exactly one of `question` or `issue` required (both allowed: question used for free text).

## Metrics (computed by `score_localization.mjs`)

| Metric | Definition |
|--------|------------|
| `top1_hit` | top-1 **file** id is in `gold_files` |
| `top5_gold_hits` | count of top-5 files intersecting `gold_files` |
| `top5_gold_recall` | top5_gold_hits / \|gold_files\| (capped by 5) |
| `top10_gold_hits` | same at k=10 |
| `top5_hard_neg` | count of top-5 files intersecting `hard_negatives` |
| `top10_hard_neg` | same at k=10 |
| `domain_intrusion` | hard_negatives in top-5 > 0 |
| `documentary_hit` | any ranked `issue#`/`pr#` in gold_documentary within top-10 |
| `c4_sacred` | if `c4_expectations.forbid_supported_causal_redis`: no verified claim with Supported + redis causal language |

## Pass gate (suite-level)

A case **passes** when:

1. `top5_hard_neg == 0` (or documented exception)
2. `top5_gold_hits >= min(3, |gold_files|)` OR `top1_hit`
3. Any `c4_expectations` satisfied

Suite **earns C5.2** when ≥80% of cases pass on the same Atlas binary/DB without prompt tuning.

## Labeling rules

- Gold = “I would open this next in a real investigation,” not “somehow related.”
- Hard negatives = plausible false friends (same keyword, wrong subsystem).
- Prefer production paths over tests; include 1–2 tests when they encode the contract.
- Do not put Redis infrastructure in gold for **order** timeout bugs.
- Do put Redis infrastructure in gold when the question/issue **is about Redis**.
