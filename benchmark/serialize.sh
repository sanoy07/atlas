#!/usr/bin/env bash
# Serialize Atlas InvestigationDocument JSON → benchmark evidence.json
# Usage: ./serialize.sh <case-id> <task> <anchors...>
# Example: ./serialize.sh kyc-notification-001 "Why doesn't the user receive an email after KYC approval?" kyc notification webhook
#
# Reads ATLAS_DB and ATLAS_REPO from environment.
# Output: benchmark/cases/<case-id>/evidence.json

set -euo pipefail

CASE_ID="$1"; shift
TASK="$1";    shift
ANCHORS=("$@")

ATLAS_DB="${ATLAS_DB:-./atlas.db}"
ATLAS_REPO="${ATLAS_REPO:-$(git rev-parse --show-toplevel 2>/dev/null || echo .)}"
ATLAS_BIN="${ATLAS_BIN:-atlas}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT="$SCRIPT_DIR/cases/$CASE_ID/evidence.json"

ATLAS_COMMIT=$(git -C "$ATLAS_REPO" rev-parse HEAD 2>/dev/null || echo "unknown")
ATLAS_VERSION=$("$ATLAS_BIN" --version 2>/dev/null | head -1 || echo "unknown")

RAW=$(ATLAS_DB="$ATLAS_DB" RUST_LOG=warn "$ATLAS_BIN" investigate "${ANCHORS[@]}" \
        --repo "$ATLAS_REPO" --json 2>/dev/null)

node - <<EOF
const raw = JSON.parse(\`$(echo "$RAW" | sed "s/\`/\\\\\`/g")\`);
const task = $(echo "$TASK" | node -e "process.stdout.write(JSON.stringify(require('fs').readFileSync('/dev/stdin','utf8').trim()))");
const caseId = "$CASE_ID";
const repoCommit = "$ATLAS_COMMIT";
const generatedBy = "$ATLAS_VERSION";

const items = [];
let counter = 1;
const id = () => \`E\${counter++}\`;

// Map type from role/kind
function truncate(s, n) {
  if (!s) return "";
  const words = s.split(/\s+/);
  return words.slice(0, n).join(" ");
}

// 1. Core candidates → source_signal
for (const c of (raw.core_candidates || [])) {
  const reasons = (c.reasons || []).map(r => {
    if (r.reason === "anchor_match") return \`anchor "\${r.anchor}" via \${r.via}\`;
    if (r.reason === "structural_neighbor") return \`structural neighbor \${r.kind} from \${r.from_file}\`;
    return JSON.stringify(r);
  }).join("; ");
  items.push({
    id: id(),
    type: "source_signal",
    summary: truncate(\`\${c.file}\`, 30),
    detail: truncate(reasons, 40),
    source: { kind: "file_path", file: c.file }
  });
}

// 2. Supporting artifacts → source_signal with role tag
for (const c of (raw.supporting_artifacts || [])) {
  const reasons = (c.reasons || []).map(r => {
    if (r.reason === "anchor_match") return \`anchor "\${r.anchor}" via \${r.via}\`;
    return JSON.stringify(r);
  }).join("; ");
  items.push({
    id: id(),
    type: "source_signal",
    summary: truncate(\`[\${c.role}] \${c.file}\`, 30),
    detail: truncate(reasons, 40),
    source: { kind: "file_path", file: c.file }
  });
}

// 3. Structural edges → observed_call / observed_import / model_reference
for (const obs of (raw.observed_structure || [])) {
  for (const e of (obs.outgoing || [])) {
    const kind = e.kind === "imports" ? "observed_import"
               : e.kind === "references_model" ? "model_reference"
               : "observed_call";
    const sym = e.symbol ? \` [\${e.symbol}]\` : "";
    items.push({
      id: id(),
      type: kind,
      summary: truncate(\`\${obs.file} → \${e.file}\${sym}\`, 30),
      detail: truncate(\`kind=\${e.kind}\`, 40),
      source: { kind: "AST", file: obs.file }
    });
  }
}

// 4. Documentary evidence → documentary
for (const ev of (raw.documentary || [])) {
  const label = ev.kind === "pr" ? "PR" : "Issue";
  items.push({
    id: id(),
    type: "documentary",
    summary: truncate(\`\${label} #\${ev.number}: \${ev.title}\`, 30),
    detail: truncate((ev.snippets || [])[0] || ev.matched_anchors.join(", "), 40),
    source: { kind: ev.kind === "pr" ? "PR" : "Issue", number: ev.number }
  });
}

// 5. Historical evidence → historical
for (const h of (raw.historical || [])) {
  if (h.touch_count === 0) continue;
  items.push({
    id: id(),
    type: "historical",
    summary: truncate(\`touched \${h.touch_count}x: \${h.file}\`, 30),
    detail: truncate((h.co_changed_candidates || []).join(", "), 40),
    source: { kind: "git", file: h.file }
  });
}

// 6. Unresolved → boundary
for (const u of (raw.unresolved || [])) {
  items.push({
    id: id(),
    type: "boundary",
    summary: truncate(\`unresolved: \${u.subject}\`, 30),
    detail: truncate(u.observation, 40),
    source: { kind: "inference" }
  });
}

const out = {
  case_id: caseId,
  task,
  repo: "RWATP",
  repo_commit: repoCommit,
  generated_by: \`atlas \${generatedBy}\`,
  evidence: items
};

console.log(JSON.stringify(out, null, 2));
EOF
