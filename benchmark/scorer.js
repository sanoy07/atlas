#!/usr/bin/env node
// Benchmark 0.2 — Deterministic Evidence Ranker
// Usage: node scorer.js <case-id> [--version v5]
//
// v1: cross_type (degenerate) + centrality + historical + task_overlap
// v2: task_anchor (0.85) + centrality + historical  — fixes hub dominance
// v3: v2 + BFS proximity (discarded — re-introduces hub dominance in dense modules)
// v4: v2 + per-file anchor promotion — source_signal items inherit richer edge anchors
// v5: v4 + structural edge promotion (fma>1/N) + token-count tie-breaker

const fs = require("fs");
const path = require("path");

const [,, caseId, ...flags] = process.argv;
if (!caseId) {
  console.error("Usage: node scorer.js <case-id>");
  process.exit(1);
}

const versionFlag = flags.indexOf("--version");
const scorerVersion = versionFlag >= 0 ? flags[versionFlag + 1] : "v5";

const CASES_DIR = path.join(__dirname, "cases");
const evidencePath = path.join(CASES_DIR, caseId, "evidence.json");
const groundTruthPath = path.join(CASES_DIR, caseId, "ground-truth.json");
const regressionsPath = path.join(CASES_DIR, caseId, "regressions.json");

const evidence = JSON.parse(fs.readFileSync(evidencePath, "utf8"));
const gt = JSON.parse(fs.readFileSync(groundTruthPath, "utf8"));
const regressions = fs.existsSync(regressionsPath)
  ? JSON.parse(fs.readFileSync(regressionsPath, "utf8"))
  : [];

const items = evidence.evidence;
const task = evidence.task;
const essential = new Set(gt.essential);
const distractors = new Set(gt.distractors);

// ── Weights (Scorer v3) ────────────────────────────────────────────────────────
// task_anchor stays dominant (0.85) — BFS proximity only fills in zero-anchor items.
const WEIGHTS = {
  task_anchor: 0.85,  // fraction of (stemmed) task tokens found in item text
  proximity:   0.00,  // BFS: only applied when task_anchor=0 (see scoring below)
  historical:  0.075, // normalized touch count (tiebreaker)
  centrality:  0.075, // normalized structural edge degree (tiebreaker)
};
const PROX_BOOST = 0.00; // disabled — BFS re-introduces hub dominance for zero-anchor dense modules

// ── Tokenizer + Stemmer ───────────────────────────────────────────────────────

const STOPWORDS = new Set([
  "the","a","an","is","in","of","to","for","and","or","not","that","this",
  "why","what","how","does","after","before","when","which","with","from",
  "into","by","be","was","were","are","on","at","its","src","modules","services",
  "models","graphql","common","enum","enums","types","utils","dto","interface",
  "interfaces","schema","schemas","module","handler","handlers","middleware",
  "providers","provider","via","file","path","anchor","touched","kind","asc",
  "per","two","one","all","get","set","new","any","use","has","had","have","may",
  "would","could","should","will","shall","can","let","var","const","type",
  "true","false","null","undefined"
]);

function tokenize(text) {
  if (!text) return [];
  return text
    .replace(/([a-z])([A-Z])/g, "$1 $2")       // camelCase split
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2") // acronym split
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, " ")
    .split(/\s+/)
    .filter(t => t.length > 2 && !STOPWORDS.has(t));
}

// No stemming — 's'-only stripping was tried but caused net regressions
// ("selects"→"select", "users"→"user" flooded wrong files).
function tokenVariants(token) {
  return new Set([token]);
}

function buildTaskTokenSet(taskStr) {
  return new Set(tokenize(taskStr));
}

function itemText(item) {
  return [item.summary || "", item.detail || "", item.source?.file || ""].join(" ");
}

function itemSourceFile(item) {
  return item.source?.file || null;
}

// ── Feature: task anchor strength ─────────────────────────────────────────────
// Fraction of distinct task tokens found in item text.

function computeTaskAnchor(item, taskBase) {
  if (taskBase.size === 0) return 0;
  const textTokens = new Set(tokenize(itemText(item)));
  let hits = 0;
  for (const t of taskBase) {
    for (const v of tokenVariants(t)) {
      if (textTokens.has(v)) { hits++; break; }
    }
  }
  return hits / taskBase.size;
}

// ── Feature: per-file anchor promotion ───────────────────────────────────────────
// source_signal items have sparse text; structural
// edge items for the same file are richer. Promote each file to the best anchor
// seen across all items where that file is the SOURCE (source_signal or structural edge).
// Target propagation was tried but inflated hub-file non-essentials.

function buildFileMaxAnchor(items, rawAnchors) {
  const map = new Map();
  for (let i = 0; i < items.length; i++) {
    const f = itemSourceFile(items[i]);
    if (f && rawAnchors[i] > (map.get(f) || 0)) map.set(f, rawAnchors[i]);
  }
  return map;
}

// ── Feature: BFS proximity ────────────────────────────────────────────────────
// Items adjacent in the structural graph to task-matching seed files get boosted.

function buildAdjacency(items) {
  const adj = new Map(); // file -> Set<adjacent file>
  for (const item of items) {
    if (!["observed_call", "observed_import", "model_reference"].includes(item.type)) continue;
    const src = itemSourceFile(item);
    if (!src) continue;
    const m = item.summary.match(/→\s+([^\s\[]+)/);
    if (!m) continue;
    const tgt = m[1];
    if (!adj.has(src)) adj.set(src, new Set());
    if (!adj.has(tgt)) adj.set(tgt, new Set());
    adj.get(src).add(tgt);
    adj.get(tgt).add(src);
  }
  return adj;
}

function computeProximity(items, taskAnchorScores, adj) {
  // Seed: any file-based item with non-zero task anchor
  const seedFiles = new Set();
  for (let i = 0; i < items.length; i++) {
    if (taskAnchorScores[i] > 0) {
      const f = itemSourceFile(items[i]);
      if (f) seedFiles.add(f);
    }
  }

  const prox = new Map();
  for (const f of seedFiles) prox.set(f, 1.0);

  // hop 1
  for (const f of seedFiles) {
    for (const n of (adj.get(f) || [])) {
      if (!prox.has(n)) prox.set(n, 0.5);
    }
  }

  // hop 2
  const hop1 = [...prox.entries()].filter(([, v]) => v === 0.5).map(([f]) => f);
  for (const f of hop1) {
    for (const n of (adj.get(f) || [])) {
      if (!prox.has(n)) prox.set(n, 0.2);
    }
  }

  return prox;
}

// ── Feature: historical + centrality ─────────────────────────────────────────

function buildHistoricalMap(items) {
  const map = new Map();
  for (const item of items) {
    if (item.type !== "historical") continue;
    const m = item.summary.match(/touched\s+(\d+)x/i);
    const count = m ? parseInt(m[1]) : 1;
    const f = itemSourceFile(item);
    if (f) map.set(f, (map.get(f) || 0) + count);
  }
  return map;
}

function buildCentralityMap(items) {
  const map = new Map();
  const inc = f => { if (f) map.set(f, (map.get(f) || 0) + 1); };
  for (const item of items) {
    if (!["observed_call", "observed_import", "model_reference"].includes(item.type)) continue;
    inc(itemSourceFile(item));
    const m = item.summary.match(/→\s+([^\s\[]+)/);
    if (m) inc(m[1]);
  }
  return map;
}

function normalizeByMax(values) {
  const max = Math.max(...values, 1e-9);
  return values.map(v => v / max);
}

// ── Score ─────────────────────────────────────────────────────────────────────

const taskBase = buildTaskTokenSet(task);
const historicalMap = buildHistoricalMap(items);
const centralityMap = buildCentralityMap(items);
const adj = buildAdjacency(items);

const rawTaskAnchor = items.map(item => computeTaskAnchor(item, taskBase));
const fileMaxAnchor = buildFileMaxAnchor(items, rawTaskAnchor);

// Per-file anchor promotion:
// - FILE_ITEM_TYPES (source_signal, boundary, historical) always inherit the richest
//   edge anchor seen for their source file.
// - Structural edge items inherit their source file's max anchor ONLY when that max
//   is strictly above 1 task token — requiring 2+ task tokens to avoid hub flooding.
const FILE_ITEM_TYPES = new Set(["source_signal", "boundary", "historical"]);
const STRUCT_EDGE_TYPES = new Set(["observed_call", "observed_import", "model_reference"]);
const singleToken = taskBase.size > 0 ? 1 / taskBase.size : 0;

const effectiveTaskAnchor = items.map((item, i) => {
  const f = itemSourceFile(item);
  if (!f) return rawTaskAnchor[i];
  const fma = fileMaxAnchor.get(f) || 0;
  if (FILE_ITEM_TYPES.has(item.type)) {
    return Math.max(rawTaskAnchor[i], fma);
  }
  if (STRUCT_EDGE_TYPES.has(item.type) && fma > singleToken) {
    return Math.max(rawTaskAnchor[i], fma);
  }
  return rawTaskAnchor[i];
});
const proxMap = computeProximity(items, effectiveTaskAnchor, adj);

const rawProximity  = items.map((item, i) => {
  const f = itemSourceFile(item);
  if (f) return proxMap.get(f) || 0;
  // documentary: use task anchor as proxy
  return effectiveTaskAnchor[i] > 0 ? 0.8 : 0;
});
const rawHistorical = items.map(item => {
  const f = itemSourceFile(item);
  return f ? (historicalMap.get(f) || 0) : 0;
});
const rawCentrality = items.map(item => {
  const f = itemSourceFile(item);
  return f ? (centralityMap.get(f) || 0) : 0;
});

const normHistorical = normalizeByMax(rawHistorical);
const normCentrality = normalizeByMax(rawCentrality);

// Token-count tie-breaker: total task-token occurrences (not just unique fraction).
// Weight is tiny (0.001) — only matters when task_anchor + hist + cent are equal.
// Breaks documentary ties where all items have the same anchor fraction.
const rawTokenCount = items.map(item =>
  tokenize(itemText(item)).filter(t => taskBase.has(t)).length
);

const scored = items.map((item, i) => {
  // Proximity only helps zero-anchor items — avoids inflating dense modules
  const proximityContrib = effectiveTaskAnchor[i] === 0 ? PROX_BOOST * rawProximity[i] : 0;
  return {
  ...item,
  _score:
    WEIGHTS.task_anchor * effectiveTaskAnchor[i] +
    proximityContrib +
    WEIGHTS.historical  * normHistorical[i] +
    WEIGHTS.centrality  * normCentrality[i] +
    0.001 * rawTokenCount[i],
  _features: {
    task_anchor: effectiveTaskAnchor[i].toFixed(3),
    raw_anchor:  rawTaskAnchor[i].toFixed(3),
    proximity:   rawProximity[i].toFixed(3),
    historical:  normHistorical[i].toFixed(3),
    centrality:  normCentrality[i].toFixed(3),
  },
  };
});

scored.sort((a, b) => b._score - a._score);

// ── Recall metrics ────────────────────────────────────────────────────────────

function recallAtN(ranked, essential, n) {
  const topIds = new Set(ranked.slice(0, n).map(e => e.id));
  const hit = [...essential].filter(id => topIds.has(id)).length;
  return { hit, total: essential.size, pct: Math.round(hit / essential.size * 100) };
}

const r20 = recallAtN(scored, essential, 20);
const r30 = recallAtN(scored, essential, 30);
const r40 = recallAtN(scored, essential, 40);

const rankMap = {};
scored.forEach((item, i) => { rankMap[item.id] = i + 1; });

const regressionResults = regressions.map(reg => {
  const rank = rankMap[reg.id];
  const pass = rank !== undefined && rank <= reg.max_rank;
  return { ...reg, rank: rank ?? "missing", pass };
});

// ── Output ────────────────────────────────────────────────────────────────────

console.log(`\nBenchmark 0.2 — ${caseId}  [scorer ${scorerVersion}]`);
console.log(`Task: ${task}`);
console.log(`Task tokens (${taskBase.size}): ${[...taskBase].join(", ")}`);
console.log(`Items: ${items.length}  Essential: ${essential.size}  Distractors: ${distractors.size}`);
console.log(`\nWeights: task_anchor=${WEIGHTS.task_anchor} proximity=${WEIGHTS.proximity} historical=${WEIGHTS.historical} centrality=${WEIGHTS.centrality}`);

console.log("\n── Recall ───────────────────────────────────────");
console.log(`  @20: ${r20.hit}/${r20.total}  (${r20.pct}%)  ${r20.pct === 100 ? "✓" : "✗"}`);
console.log(`  @30: ${r30.hit}/${r30.total}  (${r30.pct}%)  ${r30.pct === 100 ? "✓" : "✗"}`);
console.log(`  @40: ${r40.hit}/${r40.total}  (${r40.pct}%)  ${r40.pct === 100 ? "✓" : "✗"}`);

console.log("\n── Essential item ranks ─────────────────────────");
for (const id of [...essential].sort((a, b) => (rankMap[a] ?? 999) - (rankMap[b] ?? 999))) {
  const rank = rankMap[id] ?? "missing";
  const inTop30 = typeof rank === "number" && rank <= 30;
  const item = scored.find(x => x.id === id);
  const f = item?._features ?? {};
  console.log(`  ${id.padEnd(6)} rank ${String(rank).padEnd(4)}  tsk=${f.task_anchor} raw=${f.raw_anchor}  ${inTop30 ? "✓" : "✗"}`);
}

if (regressionResults.length > 0) {
  console.log("\n── Regressions ──────────────────────────────────");
  for (const r of regressionResults) {
    console.log(`  ${r.id}  rank ${r.rank}  max_rank ${r.max_rank}  ${r.pass ? "PASS ✓" : "FAIL ✗"}  (${r.reason})`);
  }
}

console.log("\n── Top 40 ranked items ──────────────────────────");
console.log("  Rank  ID      Tag   Type                   Score   tsk    raw    hist   cent");
for (let i = 0; i < Math.min(40, scored.length); i++) {
  const item = scored[i];
  const tag = essential.has(item.id) ? "ESS" : distractors.has(item.id) ? "DIS" : "   ";
  const f = item._features;
  console.log(
    `  ${String(i+1).padStart(3)}   ${item.id.padEnd(6)}  ${tag}  ${item.type.padEnd(22)} ${item._score.toFixed(3)}  ${f.task_anchor}  ${f.raw_anchor}  ${f.historical}  ${f.centrality}`
  );
}

const pass = r30.pct === 100 && regressionResults.every(r => r.pass);
console.log(`\n${pass ? "PASS" : "FAIL"} — Recall@30=${r30.pct}%  regressions=${regressionResults.filter(r=>r.pass).length}/${regressionResults.length} passed`);
console.log("");
