#!/usr/bin/env node
// Benchmark 0.1 runner — one (model × case × condition × repetition)
// Usage: node run.js <model> <case-id> <condition> <repetition>
// model: qwen2.5-coder:7b-instruct | qwen3:4b
// condition: B-combined | B-split
// repetition: 1 | 2 | 3

const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

const [,, model, caseId, condition, repStr] = process.argv;
if (!model || !caseId || !condition || !repStr) {
  console.error("Usage: node run.js <model> <case-id> <condition> <repetition>");
  process.exit(1);
}
const repetition = parseInt(repStr);
const CASES_DIR = path.join(__dirname, "cases");
const RESULTS_DIR = path.join(__dirname, "results");
const RAW_DIR = path.join(RESULTS_DIR, "raw");

const evidencePath = path.join(CASES_DIR, caseId, "evidence.json");
const groundTruthPath = path.join(CASES_DIR, caseId, "ground-truth.json");
const evidence = JSON.parse(fs.readFileSync(evidencePath, "utf8"));
const gt = JSON.parse(fs.readFileSync(groundTruthPath, "utf8"));

const SYSTEM_PROMPT = `You are an evidence triage classifier for a code investigation system.
You will receive a TASK and a numbered list of EVIDENCE items.

Classify each item into exactly one of:
RELEVANT — directly needed to address the task
POSSIBLY_RELEVANT — may be needed; keep compactly
UNKNOWN — cannot judge from the given information
NOT_NEEDED_NOW — not needed for this task

Rules:
- Do not answer the task.
- Do not infer relationships not present in the evidence.
- Give a one-sentence reason for every classification.
- Output JSON only, matching the schema below. No prose outside JSON.

Output schema:
{"decisions":[{"id":"E1","class":"RELEVANT","reason":"<one sentence>"}]}`;

function serializeItem(e) {
  return `[${e.id}] ${e.type}: ${e.summary} (${
    e.source.file || e.source.kind + (e.source.number ? " #" + e.source.number : "")
  })`;
}

function buildUserMessage(items) {
  const evidenceLines = items.map(serializeItem).join("\n");
  return `TASK: ${evidence.task}\n\nEVIDENCE:\n${evidenceLines}`;
}

function callOllama(userMessage) {
  const payload = {
    model,
    stream: false,
    options: { temperature: 0, seed: 42 },
    messages: [
      { role: "system", content: SYSTEM_PROMPT },
      { role: "user", content: userMessage }
    ]
  };
  const result = execSync(
    `curl -s -m 300 -X POST http://localhost:11434/api/chat -H 'Content-Type: application/json' -d '${JSON.stringify(payload).replace(/'/g, "'\\''")}'`,
    { maxBuffer: 10 * 1024 * 1024, timeout: 310000 }
  );
  const resp = JSON.parse(result.toString());
  return resp.message?.content || "";
}

function parseDecisions(raw) {
  // Try direct parse
  try {
    const parsed = JSON.parse(raw);
    if (parsed.decisions) return { decisions: parsed.decisions, valid: true };
  } catch {}
  // Try extracting JSON block
  const match = raw.match(/\{[\s\S]*"decisions"[\s\S]*\}/);
  if (match) {
    try {
      const parsed = JSON.parse(match[0]);
      if (parsed.decisions) return { decisions: parsed.decisions, valid: false };
    } catch {}
  }
  return { decisions: [], valid: false };
}

function countTokens(text) {
  return Math.ceil(text.split(/\s+/).length * 1.3); // rough estimate
}

// ── Main ──────────────────────────────────────────────────────────────────────

const runId = `${model.replace(/[:/]/g,"-")}_${caseId}_${condition}_rep${repetition}`;
console.error(`[run] ${runId}`);

let rawOutput = "";
let decisions = [];
let jsonValid = false;

const startMs = Date.now();

if (condition === "B-combined") {
  const userMsg = buildUserMessage(evidence.evidence);
  rawOutput = callOllama(userMsg);
  const parsed = parseDecisions(rawOutput);
  decisions = parsed.decisions;
  jsonValid = parsed.valid;
} else if (condition === "B-split") {
  // Split by type, run 4 passes, merge
  const typeGroups = {
    structural: ["observed_call", "observed_import", "model_reference"],
    documentary: ["documentary"],
    historical: ["historical"],
    source: ["source_signal", "boundary"]
  };
  const allDecisions = [];
  for (const [groupName, types] of Object.entries(typeGroups)) {
    const items = evidence.evidence.filter(e => types.includes(e.type));
    if (!items.length) continue;
    const userMsg = buildUserMessage(items);
    const raw = callOllama(userMsg);
    rawOutput += `\n\n// --- ${groupName} batch ---\n` + raw;
    const parsed = parseDecisions(raw);
    allDecisions.push(...parsed.decisions);
    if (!parsed.valid) jsonValid = false; else if (jsonValid !== false) jsonValid = true;
  }
  decisions = allDecisions;
} else {
  console.error("Unknown condition:", condition); process.exit(1);
}

const latencyMs = Date.now() - startMs;

// ── Score ─────────────────────────────────────────────────────────────────────

const decisionMap = {};
for (const d of decisions) decisionMap[d.id] = d.class;

const essential = new Set(gt.essential);
const distractors = new Set(gt.distractors);

const nRelevant = decisions.filter(d => d.class === "RELEVANT").length;
const nPossible = decisions.filter(d => d.class === "POSSIBLY_RELEVANT").length;
const nUnknown  = decisions.filter(d => d.class === "UNKNOWN").length;
const nDeferred = decisions.filter(d => d.class === "NOT_NEEDED_NOW").length;

const essentialRetained = [...essential].filter(id =>
  decisionMap[id] === "RELEVANT" || decisionMap[id] === "POSSIBLY_RELEVANT"
).length;
const falseNegatives = [...essential].filter(id =>
  decisionMap[id] === "NOT_NEEDED_NOW"
).length;
const distractorsDeferred = [...distractors].filter(id =>
  decisionMap[id] === "NOT_NEEDED_NOW"
).length;

// Retained = RELEVANT + POSSIBLY_RELEVANT items
const retainedItems = decisions.filter(d =>
  d.class === "RELEVANT" || d.class === "POSSIBLY_RELEVANT"
);
const inputText = evidence.evidence.map(serializeItem).join(" ");
const retainedText = retainedItems.map(e => {
  const orig = evidence.evidence.find(ev => ev.id === e.id);
  return orig ? serializeItem(orig) : "";
}).join(" ");

const inputTokens  = countTokens(inputText);
const outputTokens = countTokens(rawOutput);
const retainedTokens = countTokens(retainedText);
const compressionPct = Math.round((1 - retainedTokens / inputTokens) * 100);

// ── Save raw output ───────────────────────────────────────────────────────────

fs.writeFileSync(
  path.join(RAW_DIR, `${runId}.json`),
  JSON.stringify({ runId, model, caseId, condition, repetition, decisions, rawOutput }, null, 2)
);

// ── Print CSV row ─────────────────────────────────────────────────────────────

const csvRow = [
  runId, caseId, model, condition, repetition,
  evidence.evidence.length, inputTokens, outputTokens, latencyMs,
  nRelevant, nPossible, nUnknown, nDeferred,
  retainedTokens, compressionPct,
  essential.size, essentialRetained, falseNegatives,
  distractors.size, distractorsDeferred,
  jsonValid ? "true" : "false",
  "", // decision_stability_pct — computed across reps separately
  ""  // notes
].join(",");

const csvPath = path.join(RESULTS_DIR, "runs.csv");
const header = "run_id,case_id,model,condition,repetition,input_items,input_tokens,output_tokens,latency_ms,n_relevant,n_possible,n_unknown,n_deferred,retained_tokens,compression_pct,essential_total,essential_retained,false_negatives,distractors_total,distractors_deferred,json_valid,decision_stability_pct,notes";
if (!fs.existsSync(csvPath)) fs.writeFileSync(csvPath, header + "\n");
fs.appendFileSync(csvPath, csvRow + "\n");

// ── Console summary ───────────────────────────────────────────────────────────

const pass = falseNegatives === 0 && compressionPct >= 70;
console.log(`[${pass ? "PASS" : "FAIL"}] ${runId}`);
console.log(`  compression: ${compressionPct}%  false-negatives: ${falseNegatives}/${essential.size}  distractors-deferred: ${distractorsDeferred}/${distractors.size}  json-valid: ${jsonValid}  latency: ${latencyMs}ms`);
if (falseNegatives > 0) {
  const missed = [...essential].filter(id => decisionMap[id] === "NOT_NEEDED_NOW");
  console.log(`  MISSED ESSENTIAL: ${missed.join(", ")}`);
}
