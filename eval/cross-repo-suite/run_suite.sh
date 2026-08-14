#!/usr/bin/env bash
# Cross-repo Atlas + agent evaluation suite.
# Produces JSONL proof under $OUT_DIR and a summary markdown report.
set -euo pipefail

ATLAS="${ATLAS_BIN:-$HOME/.local/bin/atlas}"
OUT_DIR="${OUT_DIR:-/home/sanoy/projects/atlas/eval/cross-repo-suite/results-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT_DIR"
PROOF="$OUT_DIR/proof.jsonl"
SUMMARY="$OUT_DIR/SUMMARY.md"
export PATH="$HOME/.local/bin:$PATH"

# Resolve python once
if [[ -z "${ATLAS_AGENT_PYTHON:-}" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    export ATLAS_AGENT_PYTHON=$(command -v python3)
  elif command -v nix-shell >/dev/null 2>&1; then
    export ATLAS_AGENT_PYTHON=$(nix-shell -p python3 --run 'which python3' 2>/dev/null || true)
  fi
fi
export ATLAS_AGENT_SCRIPT="${ATLAS_AGENT_SCRIPT:-/home/sanoy/projects/atlas/agent/atlas_agent.py}"
export AGENT_MODEL="${AGENT_MODEL:-qwen3:4b}"
export AGENT_NUM_CTX="${AGENT_NUM_CTX:-12288}"
export ATLAS_AGENT_WEB="${ATLAS_AGENT_WEB:-0}"
export ATLAS_BIN="$ATLAS"

log() { printf '%s\n' "$*" | tee -a "$OUT_DIR/suite.log"; }
json_escape() { python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null || printf '"%s"' "${1//\"/\\\"}"; }

record() {
  # record name repo status ms detail
  local name="$1" repo="$2" status="$3" ms="$4" detail="${5:-}"
  printf '{"ts":"%s","case":"%s","repo":"%s","status":"%s","ms":%s,"detail":%s}\n' \
    "$(date -Iseconds)" "$name" "$repo" "$status" "$ms" \
    "$(printf '%s' "$detail" | head -c 2000 | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null || echo '""')" \
    >> "$PROOF"
}

run_case() {
  local name="$1" repo="$2"
  shift 2
  local start end ms out rc=0
  start=$(date +%s%3N)
  set +e
  out=$("$@" 2>&1)
  rc=$?
  set -e
  end=$(date +%s%3N)
  ms=$((end - start))
  if [[ $rc -eq 0 ]]; then
    # empty or error-like content still fails
    if echo "$out" | grep -qiE '^(!!|error:|ERROR:)' || [[ -z "${out// }" ]]; then
      record "$name" "$repo" "fail" "$ms" "$out"
      log "  FAIL $name (${ms}ms)"
      printf '%s\n' "$out" > "$OUT_DIR/${name//\//_}.out"
      return 1
    fi
    record "$name" "$repo" "ok" "$ms" "$(echo "$out" | head -c 500)"
    log "  OK   $name (${ms}ms)"
    printf '%s\n' "$out" > "$OUT_DIR/${name//\//_}.out"
    return 0
  else
    record "$name" "$repo" "fail" "$ms" "rc=$rc $out"
    log "  FAIL $name rc=$rc (${ms}ms)"
    printf '%s\n' "$out" > "$OUT_DIR/${name//\//_}.out"
    return 1
  fi
}

# Repo table: path|product|question|extra_ingest_flags
REPOS=(
  "/home/sanoy/Vesta/rwatp-core|rwatp|order fulfillment|--typescript"
  "/home/sanoy/Vesta/rwatp-notifier|rwatp|ORDER_CREATED notify|--typescript"
  "/home/sanoy/Vesta/rwatp-console|rwatp|permission check|--typescript"
  "/home/sanoy/Vesta/rwatp-user-frontend|rwatp|SIWE login|--typescript"
  "/home/sanoy/Vesta/vestascan-api|vestascan|token deploy|--typescript"
  "/home/sanoy/Vesta/vestascan-notifier|vestascan|notification publish|--typescript"
  "/home/sanoy/Vesta/vestascan-blockchain|vestascan|smart contract|--typescript"
  "/home/sanoy/Vesta/vestascan-user-fe|vestascan|listing page|--typescript"
  "/home/sanoy/Vesta/vestascan-console|vestascan|admin dataroom|--typescript"
  "/home/sanoy/projects/research/jj|research|workspace commit| "
  "/home/sanoy/projects/research/gigatoken|research|pretokenizer cache| "
)

# Shared DBs per product
DB_RWATP="/home/sanoy/projects/atlas/eval/cross-repo-suite/rwatp-eval.db"
DB_VESTA="/home/sanoy/projects/atlas/eval/cross-repo-suite/vestascan-eval.db"
DB_RES="/home/sanoy/projects/atlas/eval/cross-repo-suite/research-eval.db"
mkdir -p "$(dirname "$DB_RWATP")"

log "=== Cross-repo suite start $(date -Iseconds) ==="
log "ATLAS=$ATLAS"
log "OUT=$OUT_DIR"
log "PYTHON=${ATLAS_AGENT_PYTHON:-missing}"

ok=0; fail=0; skip=0

# --- Phase 1: project register + ingest ---
setup_product() {
  local product="$1" db="$2"
  shift 2
  export ATLAS_DB="$db"
  $ATLAS project init "$product" --description "eval suite $product" >/dev/null 2>&1 || true
  for entry in "$@"; do
    local path="${entry%%|*}"
    local role="${entry##*|}"
    local name
    name=$(basename "$path")
    if [[ ! -d "$path/.git" ]]; then
      log "SKIP register missing $path"
      continue
    fi
    $ATLAS project register "$product" "$path" --role "$role" --name "$name" >>"$OUT_DIR/ingest.log" 2>&1 || true
  done
}

setup_product rwatp "$DB_RWATP" \
  "/home/sanoy/Vesta/rwatp-core|api" \
  "/home/sanoy/Vesta/rwatp-notifier|notifier" \
  "/home/sanoy/Vesta/rwatp-console|console" \
  "/home/sanoy/Vesta/rwatp-user-frontend|user-fe"

setup_product vestascan "$DB_VESTA" \
  "/home/sanoy/Vesta/vestascan-api|api" \
  "/home/sanoy/Vesta/vestascan-notifier|notifier" \
  "/home/sanoy/Vesta/vestascan-blockchain|chain" \
  "/home/sanoy/Vesta/vestascan-user-fe|user-fe" \
  "/home/sanoy/Vesta/vestascan-console|console"

setup_product research "$DB_RES" \
  "/home/sanoy/projects/research/jj|vcs" \
  "/home/sanoy/projects/research/gigatoken|tokenizer"

log "=== Ingest rwatp (typescript) ==="
export ATLAS_DB="$DB_RWATP"
start=$(date +%s%3N)
set +e
$ATLAS project ingest rwatp --typescript >>"$OUT_DIR/ingest.log" 2>&1
rc=$?
set -e
end=$(date +%s%3N)
record "ingest_rwatp" "rwatp" "$([[ $rc -eq 0 ]] && echo ok || echo fail)" "$((end-start))" "rc=$rc"
log "ingest rwatp rc=$rc ms=$((end-start))"

log "=== Ingest vestascan (typescript) ==="
export ATLAS_DB="$DB_VESTA"
start=$(date +%s%3N)
set +e
$ATLAS project ingest vestascan --typescript >>"$OUT_DIR/ingest.log" 2>&1
rc=$?
set -e
end=$(date +%s%3N)
record "ingest_vestascan" "vestascan" "$([[ $rc -eq 0 ]] && echo ok || echo fail)" "$((end-start))" "rc=$rc"
log "ingest vestascan rc=$rc ms=$((end-start))"

log "=== Ingest research (git only) ==="
export ATLAS_DB="$DB_RES"
start=$(date +%s%3N)
set +e
$ATLAS project ingest research >>"$OUT_DIR/ingest.log" 2>&1
rc=$?
set -e
end=$(date +%s%3N)
record "ingest_research" "research" "$([[ $rc -eq 0 ]] && echo ok || echo fail)" "$((end-start))" "rc=$rc"
log "ingest research rc=$rc ms=$((end-start))"

# --- Phase 2: per-repo command matrix ---
for row in "${REPOS[@]}"; do
  IFS='|' read -r path product question flags <<<"$row"
  name=$(basename "$path")
  log "=== REPO $name ($product) ==="
  if [[ ! -d "$path/.git" ]]; then
    record "presence" "$name" "skip" 0 "missing"
    skip=$((skip+1))
    continue
  fi

  case "$product" in
    rwatp) export ATLAS_DB="$DB_RWATP" ;;
    vestascan) export ATLAS_DB="$DB_VESTA" ;;
    research) export ATLAS_DB="$DB_RES" ;;
  esac

  # map
  if run_case "${name}/map" "$name" bash -c "cd '$path' && $ATLAS map"; then ok=$((ok+1)); else fail=$((fail+1)); fi
  # modules — quality gate: must list total > 0 when map found modules
  if run_case "${name}/modules" "$name" bash -c "
    cd '$path' || exit 1
    out=\$($ATLAS modules 2>&1) || exit 1
    echo \"\$out\"
    n=\$(echo \"\$out\" | sed -n 's/.*total: \\([0-9][0-9]*\\).*/\\1/p' | head -1)
    if [[ -z \"\$n\" || \"\$n\" -eq 0 ]]; then
      echo 'ERROR: modules total is 0 (quality gate)'
      exit 1
    fi
  "; then ok=$((ok+1)); else fail=$((fail+1)); fi
  # investigate no-ai — quality gate: must mention a path-like hit
  if run_case "${name}/investigate_noai" "$name" bash -c "
    cd '$path' || exit 1
    out=\$($ATLAS investigate --no-ai $(printf %q "$question") 2>&1) || exit 1
    echo \"\$out\"
    if ! echo \"\$out\" | grep -qE 'src/|lib/|cli/|\\[file\\]|CORE|RANKED|LIKELY'; then
      echo 'ERROR: investigate produced no recognizable evidence (quality gate)'
      exit 1
    fi
  "; then ok=$((ok+1)); else fail=$((fail+1)); fi
  # agent fast
  if [[ -n "${ATLAS_AGENT_PYTHON:-}" && -f "$ATLAS_AGENT_SCRIPT" ]]; then
    if run_case "${name}/agent_fast" "$name" bash -c "cd '$path' && $ATLAS agent --fast --no-web $(printf %q "$question")"; then ok=$((ok+1)); else fail=$((fail+1)); fi
  else
    record "${name}/agent_fast" "$name" "skip" 0 "no python"
    skip=$((skip+1))
    log "  SKIP agent_fast (no python)"
  fi
  # ripgrep tool unit (via python import)
  if [[ -n "${ATLAS_AGENT_PYTHON:-}" && -f "$ATLAS_AGENT_SCRIPT" ]]; then
    if run_case "${name}/ripgrep_tool" "$name" bash -c "
      cd '$path' && '$ATLAS_AGENT_PYTHON' - <<'PY'
import sys
sys.path.insert(0, '/home/sanoy/projects/atlas/agent')
import atlas_agent as a
out = a.t_ripgrep(repo='$path', pattern='TODO|FIXME|export ', path='.', max_matches=5, glob='*.{ts,tsx,rs,js}')
print(out[:1500] if out else 'EMPTY')
if out.startswith('ERROR:'):
    raise SystemExit(1)
PY
    "; then ok=$((ok+1)); else fail=$((fail+1)); fi
  fi
done

# --- Phase 3: targeted full agent (subset, expensive) ---
if [[ -n "${ATLAS_AGENT_PYTHON:-}" && "${RUN_FULL_AGENT:-1}" == "1" ]]; then
  log "=== Full agent samples ==="
  export ATLAS_DB="$DB_RWATP"
  if run_case "rwatp-core/agent_full" "rwatp-core" bash -c \
    "cd /home/sanoy/Vesta/rwatp-core && $ATLAS agent --no-web --max-steps 6 'where is order fulfillment and what calls it?'"; then
    ok=$((ok+1)); else fail=$((fail+1)); fi
  export ATLAS_DB="$DB_RES"
  if run_case "jj/agent_full" "jj" bash -c \
    "cd /home/sanoy/projects/research/jj && $ATLAS agent --no-web --max-steps 6 'where is the operation log stored?'"; then
    ok=$((ok+1)); else fail=$((fail+1)); fi
fi

# --- Summary ---
{
  echo "# Cross-repo suite summary"
  echo
  echo "- Finished: $(date -Iseconds)"
  echo "- Results dir: \`$OUT_DIR\`"
  echo "- Proof log: \`$PROOF\`"
  echo "- OK: $ok  FAIL: $fail  SKIP: $skip"
  echo
  echo "## Cases by status"
  echo
  if [[ -f "$PROOF" ]]; then
    echo '```'
    awk -F'"' '/"status":"ok"/{ok++} /"status":"fail"/{fail++} /"status":"skip"/{skip++} END{print "ok="ok+0,"fail="fail+0,"skip="skip+0}' "$PROOF"
    echo '```'
    echo
    echo "### Failures"
    echo '```'
    grep '"status":"fail"' "$PROOF" | head -80 || true
    echo '```'
    echo
    echo "### Slowest cases (ms)"
    echo '```'
    # crude extract
    grep '"ms":' "$PROOF" | sed 's/.*"case":"\([^"]*\)".*"ms":\([0-9]*\).*/\2 \1/' | sort -rn | head -20
    echo '```'
  fi
} > "$SUMMARY"

log "=== DONE ok=$ok fail=$fail skip=$skip ==="
log "Summary: $SUMMARY"
cat "$SUMMARY"
