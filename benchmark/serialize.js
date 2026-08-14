#!/usr/bin/env node
// Converts Atlas InvestigationDocument JSON (stdin) → benchmark evidence.json (stdout)
// Usage: atlas investigate ... --json | node serialize.js <case-id> <task>

const [,, caseId, task, repoCommit, generatedBy] = process.argv;
if (!caseId || !task) {
  console.error("Usage: atlas investigate ... --json | node serialize.js <case-id> <task> [commit] [version]");
  process.exit(1);
}

let raw = "";
process.stdin.on("data", d => raw += d);
process.stdin.on("end", () => {
  const doc = JSON.parse(raw);
  const items = [];
  let n = 1;
  const id = () => `E${n++}`;

  const trunc = (s, words) => {
    if (!s) return "";
    return s.split(/\s+/).slice(0, words).join(" ");
  };

  // Core candidates → source_signal
  for (const c of (doc.core_candidates || [])) {
    const reason = (c.reasons || []).map(r => {
      if (r.reason === "anchor_match") return `anchor "${r.anchor}" via ${r.via}`;
      if (r.reason === "structural_neighbor") return `${r.kind} neighbor from ${r.from_file}`;
      return JSON.stringify(r);
    }).join("; ");
    items.push({
      id: id(), type: "source_signal",
      summary: trunc(c.file, 30),
      detail: trunc(reason, 40),
      source: { kind: "file_path", file: c.file }
    });
  }

  // Supporting artifacts → source_signal with role
  for (const c of (doc.supporting_artifacts || [])) {
    const reason = (c.reasons || []).map(r => {
      if (r.reason === "anchor_match") return `anchor "${r.anchor}" via ${r.via}`;
      return JSON.stringify(r);
    }).join("; ");
    items.push({
      id: id(), type: "source_signal",
      summary: trunc(`[${c.role}] ${c.file}`, 30),
      detail: trunc(reason, 40),
      source: { kind: "file_path", file: c.file }
    });
  }

  // Structural edges → observed_import / model_reference / observed_call
  for (const obs of (doc.observed_structure || [])) {
    for (const e of (obs.outgoing || [])) {
      const type = e.kind === "imports" ? "observed_import"
                 : e.kind === "references_model" ? "model_reference"
                 : "observed_call";
      const sym = e.symbol ? ` [${e.symbol}]` : "";
      items.push({
        id: id(), type,
        summary: trunc(`${obs.file} → ${e.file}${sym}`, 30),
        detail: trunc(`kind=${e.kind}`, 40),
        source: { kind: "AST", file: obs.file }
      });
    }
  }

  // Documentary → documentary
  for (const ev of (doc.documentary || [])) {
    const label = ev.kind === "pr" ? "PR" : "Issue";
    items.push({
      id: id(), type: "documentary",
      summary: trunc(`${label} #${ev.number}: ${ev.title}`, 30),
      detail: trunc((ev.snippets || [])[0] || ev.matched_anchors.join(", "), 40),
      source: { kind: ev.kind === "pr" ? "PR" : "Issue", number: ev.number }
    });
  }

  // Historical → historical
  for (const h of (doc.historical || [])) {
    if (!h.touch_count) continue;
    items.push({
      id: id(), type: "historical",
      summary: trunc(`touched ${h.touch_count}x: ${h.file}`, 30),
      detail: trunc((h.co_changed_candidates || []).join(", "), 40),
      source: { kind: "git", file: h.file }
    });
  }

  // Unresolved → boundary
  for (const u of (doc.unresolved || [])) {
    items.push({
      id: id(), type: "boundary",
      summary: trunc(`unresolved: ${u.subject}`, 30),
      detail: trunc(u.observation, 40),
      source: { kind: "inference" }
    });
  }

  console.log(JSON.stringify({
    case_id: caseId,
    task,
    repo: "RWATP",
    repo_commit: repoCommit || "unknown",
    generated_by: `atlas ${generatedBy || "unknown"}`,
    evidence: items
  }, null, 2));
});
