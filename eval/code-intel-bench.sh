#!/usr/bin/env bash
# Deterministic + agent benchmarks for code-intel primitives.
# Usage:
#   ./eval/code-intel-bench.sh baseline|after|full
# Env:
#   ATLAS_BIN, ATLAS_DB, REPO (default rwatp-core), OUT_DIR
set -euo pipefail

PHASE="${1:-full}"
ATLAS="${ATLAS_BIN:-$HOME/.local/bin/atlas}"
REPO="${REPO:-$HOME/Vesta/rwatp-core}"
DB="${ATLAS_DB:-$HOME/projects/atlas/eval/cross-repo-suite/rwatp-eval.db}"
OUT_DIR="${OUT_DIR:-$HOME/projects/atlas/eval/code-intel-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT_DIR"
PROOF="$OUT_DIR/proof.jsonl"
SUMMARY="$OUT_DIR/SUMMARY.md"
export ATLAS_DB="$DB"
export PATH="$HOME/.local/bin:$PATH"
export ATLAS_AGENT_SCRIPT="${ATLAS_AGENT_SCRIPT:-$HOME/projects/atlas/agent/atlas_agent.py}"
export ATLAS_AGENT_WEB=0
export ATLAS_AGENT_THINK=0

log() { printf '%s\n' "$*" | tee -a "$OUT_DIR/suite.log"; }
record() {
  local name="$1" status="$2" ms="$3" detail="${4:-}"
  printf '{"ts":"%s","phase":"%s","case":"%s","status":"%s","ms":%s,"detail":%s}\n' \
    "$(date -Iseconds)" "$PHASE" "$name" "$status" "$ms" \
    "$(printf '%s' "$detail" | head -c 1500 | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null || echo '""')" \
    >> "$PROOF"
}

pass=0; fail=0

run_check() {
  local name="$1"; shift
  local start end ms out rc=0
  start=$(date +%s%3N 2>/dev/null || date +%s000)
  set +e
  out=$("$@" 2>&1)
  rc=$?
  set -e
  end=$(date +%s%3N 2>/dev/null || date +%s000)
  ms=$((end - start))
  printf '%s\n' "$out" > "$OUT_DIR/${name//\//_}.out"
  if [[ $rc -ne 0 ]]; then
    record "$name" "fail" "$ms" "rc=$rc $out"
    log "  FAIL $name (${ms}ms) rc=$rc"
    fail=$((fail+1))
    return 1
  fi
  record "$name" "ok" "$ms" "$(echo "$out" | head -c 400)"
  log "  OK   $name (${ms}ms)"
  pass=$((pass+1))
  return 0
}

# Assert helper: command must produce stdout matching regex
assert_cmd() {
  local name="$1" pattern="$2"; shift 2
  local start end ms out rc=0
  start=$(date +%s%3N 2>/dev/null || date +%s000)
  set +e
  out=$("$@" 2>&1)
  rc=$?
  set -e
  end=$(date +%s%3N 2>/dev/null || date +%s000)
  ms=$((end - start))
  printf '%s\n' "$out" > "$OUT_DIR/${name//\//_}.out"
  if [[ $rc -ne 0 ]] || ! echo "$out" | grep -qiE "$pattern"; then
    record "$name" "fail" "$ms" "pattern=$pattern out=$(echo "$out" | head -c 600)"
    log "  FAIL $name (${ms}ms) want~/$pattern/"
    fail=$((fail+1))
    return 1
  fi
  record "$name" "ok" "$ms" "matched $pattern"
  log "  OK   $name (${ms}ms)"
  pass=$((pass+1))
}

log "=== code-intel bench phase=$PHASE $(date -Iseconds) ==="
log "ATLAS=$ATLAS REPO=$REPO DB=$DB"
log "atlas version: $($ATLAS --version 2>&1 || true)"

cd "$REPO"

# ── Deterministic primitives ────────────────────────────────────────────────
log "--- deterministic ---"

if $ATLAS callers --help >/dev/null 2>&1; then
  assert_cmd "det.callers.tryEnqueue" \
    "payment-settlement|signing\.service" \
    $ATLAS callers tryEnqueue --limit 40

  assert_cmd "det.callers.OrderFulfillmentService.tryEnqueue" \
    "payment-settlement|signing" \
    $ATLAS callers "OrderFulfillmentService.tryEnqueue" --limit 40

  assert_cmd "det.implementations.IStorageProvider" \
    "OBSERVED implements|google-cloud-storage|adapter|storage\.interface" \
    $ATLAS implementations IStorageProvider --limit 40

  assert_cmd "det.implements.edge.IStorage" \
    "IStorageProvider|storage\.interface" \
    $ATLAS structural src/infrastructure/storage/google-cloud-storage.adapter.ts

  assert_cmd "det.capabilities.storage" \
    "listing-asset|storage\.factory|google-cloud" \
    $ATLAS capabilities

  assert_cmd "det.code-search.ListingAsset" \
    "listing-asset|ListingAsset" \
    $ATLAS code-search ListingAsset --limit 30

  assert_cmd "det.code-search.getSignedUrl" \
    "signed|getSignedUrl|storage" \
    $ATLAS code-search getSignedUrl --limit 30

  # JSON schema smoke
  assert_cmd "det.callers.json" \
    "production_callers" \
    $ATLAS callers tryEnqueue --json --limit 20

  assert_cmd "det.capabilities.json" \
    "capabilities" \
    $ATLAS capabilities --json
else
  log "  SKIP deterministic (atlas callers not in this binary — baseline without code-intel)"
  record "det.skip" "skip" "0" "binary lacks callers"
fi

# Always-available baseline commands
assert_cmd "det.map" "modules|CLAIMS|ATLAS MAP" $ATLAS map
assert_cmd "det.investigate.storage" \
  "google-cloud-storage|storage\.factory|ListingAsset|infrastructure/storage" \
  $ATLAS investigate "data room GCS ListingAsset" --no-ai

# ── Agent cases (optional if ollama up) ─────────────────────────────────────
if [[ "${SKIP_AGENT:-0}" != "1" ]] && command -v python3 >/dev/null 2>&1; then
  if curl -sf http://127.0.0.1:11434/api/tags >/dev/null 2>&1; then
    log "--- agent (qwen) ---"
    export ATLAS_BIN="$ATLAS"
    assert_cmd "agent.storing_files" \
      "ListingAsset|google-cloud|GCS|listing-asset|storage\.factory|getSignedUrl" \
      $ATLAS agent --no-web --max-steps 8 "tell me about storing files in this backend"

    assert_cmd "agent.data_room_gcs" \
      "ListingAsset|listing-asset|google-cloud|documents|getSignedUrl|GCS" \
      $ATLAS agent --no-web --max-steps 8 "how does data room document storage work with GCS?"

    assert_cmd "agent.payment_fulfill" \
      "tryEnqueue|payment-settlement|OrderFulfillment" \
      $ATLAS agent --no-web --max-steps 8 "how does payment settlement trigger order fulfillment?"
  else
    log "  SKIP agent (ollama not reachable)"
    record "agent.skip" "skip" "0" "no ollama"
  fi
else
  log "  SKIP agent (SKIP_AGENT=1 or no python3)"
fi

{
  echo "# Code-intel bench — $PHASE"
  echo
  echo "- when: $(date -Iseconds)"
  echo "- atlas: \`$ATLAS\`"
  echo "- repo: \`$REPO\`"
  echo "- db: \`$DB\`"
  echo "- pass: $pass  fail: $fail"
  echo
  echo "## Cases"
  echo
  if [[ -f "$PROOF" ]]; then
    python3 - <<'PY' "$PROOF" 2>/dev/null || true
import json,sys
from pathlib import Path
p=Path(sys.argv[1])
for line in p.read_text().splitlines():
    try:
        o=json.loads(line)
    except Exception:
        continue
    print(f"- **{o.get('case')}**: {o.get('status')} ({o.get('ms')}ms)")
PY
  fi
} > "$SUMMARY"

log "=== done pass=$pass fail=$fail out=$OUT_DIR ==="
echo "$OUT_DIR"
[[ $fail -eq 0 ]]
