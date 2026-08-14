#!/usr/bin/env bash
# Runs the full benchmark matrix, skipping any run already in runs.csv.
# Models frozen at run-1: qwen2.5-coder:7b-instruct  qwen3:4b

set -euo pipefail

MODELS=("qwen2.5-coder:7b-instruct" "qwen3:4b")
CASES=("kyc-notification-001" "listing-status-001" "caching-infra-001" "content-block-001" "auth-context-001" "permission-cache-001" "supply-ledger-001")
CONDITIONS=("B-combined" "B-split")
REPS=(1 2 3)

BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
TOTAL=$(( ${#MODELS[@]} * ${#CASES[@]} * ${#CONDITIONS[@]} * ${#REPS[@]} ))
DONE=0
SKIPPED=0

# Load completed run_ids from CSV
CSV="$BENCH_DIR/results/runs.csv"
declare -A COMPLETED
if [ -f "$CSV" ]; then
  while IFS=, read -r run_id rest; do
    COMPLETED["$run_id"]=1
  done < <(tail -n +2 "$CSV")
fi

echo "=== Benchmark 0.1 — Full Matrix Run ==="
echo "Models: ${MODELS[*]}"
echo "Cases:  ${#CASES[@]}  Conds: ${CONDITIONS[*]}  Reps: ${REPS[*]}"
echo "Total:  $TOTAL runs  (${#COMPLETED[@]} already done)"
echo ""

for model in "${MODELS[@]}"; do
  safe_model="${model//:/-}"
  safe_model="${safe_model//\//-}"
  for case_id in "${CASES[@]}"; do
    for condition in "${CONDITIONS[@]}"; do
      for rep in "${REPS[@]}"; do
        DONE=$(( DONE + 1 ))
        run_id="${safe_model}_${case_id}_${condition}_rep${rep}"
        if [ "${COMPLETED[$run_id]+_}" ]; then
          SKIPPED=$(( SKIPPED + 1 ))
          echo "  [skip $DONE/$TOTAL] $run_id"
          continue
        fi
        echo "  [$DONE/$TOTAL] $run_id"
        node "$BENCH_DIR/run.js" "$model" "$case_id" "$condition" "$rep" || \
          echo "    ERROR"
      done
    done
  done
done

echo ""
echo "=== Matrix complete ($TOTAL total, $SKIPPED skipped). ==="
echo ""

# Summary table
node - "$CSV" <<'EOF'
const fs = require("fs");
const csvPath = process.argv[2];
const csv = fs.readFileSync(csvPath, "utf8").trim().split("\n");
const headers = csv[0].split(",");
const rows = csv.slice(1).map(r => {
  const vals = r.split(",");
  return Object.fromEntries(headers.map((h, i) => [h, vals[i]]));
});

const groups = {};
for (const r of rows) {
  const key = `${r.model.substring(0,28).padEnd(28)} / ${r.condition}`;
  if (!groups[key]) groups[key] = { fn: 0, compressions: [], essRetained: [], essTotal: 0, disDef: [], disTotal: 0, valid: 0, total: 0 };
  const g = groups[key];
  g.total++;
  g.fn += parseInt(r.false_negatives);
  g.compressions.push(parseFloat(r.compression_pct));
  g.essRetained.push(parseInt(r.essential_retained));
  g.essTotal = parseInt(r.essential_total);
  g.disDef.push(parseInt(r.distractors_deferred));
  g.disTotal = parseInt(r.distractors_total);
  if (r.json_valid === "true") g.valid++;
}

const avg = arr => arr.length ? (arr.reduce((a,b)=>a+b,0)/arr.length).toFixed(1) : "-";

console.log("\nModel × Condition Results");
console.log("─".repeat(100));
console.log("Model / Condition                              | N  | FN | Cmp%  | EssRet | DisDef | JSONok");
console.log("─".repeat(100));
for (const [key, g] of Object.entries(groups)) {
  const fn0 = g.fn === 0 ? "0  ✓" : `${g.fn} ✗`;
  const cmp = `${avg(g.compressions)}%`;
  const ess = `${avg(g.essRetained)}/${g.essTotal}`;
  const dis = g.disTotal > 0 ? `${avg(g.disDef)}/${g.disTotal}` : "n/a";
  const jv  = `${Math.round(g.valid/g.total*100)}%`;
  console.log(
    key.padEnd(46) + "| " + String(g.total).padEnd(3) + "| " +
    fn0.padEnd(3) + "| " + cmp.padEnd(6) + "| " + ess.padEnd(7) + "| " + dis.padEnd(7) + "| " + jv
  );
}
console.log("─".repeat(100));
console.log("\nPass criteria: FN=0 AND compression≥70% AND distractor-deferral≥60% AND JSON≥95%");
EOF
