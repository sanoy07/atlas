#!/usr/bin/env node
/**
 * C5.0 localization scorer.
 *
 * Runs `atlas investigate` (or --issue) for each gold case, scores top-k
 * file localization + hard negatives + optional C4 sacred check.
 *
 * Usage:
 *   ATLAS_BIN=./target/debug/atlas \
 *   ATLAS_DB=~/.atlas/rwatp-core.db \
 *   REPO=/home/sanoy/Vesta/rwatp-core \
 *   node eval/localization/score_localization.mjs
 *
 * Does not modify the repository or database.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const suitePath = path.join(__dirname, "suite.json");
const suite = JSON.parse(fs.readFileSync(suitePath, "utf8"));

const ATLAS = process.env.ATLAS_BIN || process.env.ATLAS || "atlas";
const ATLAS_DB = process.env.ATLAS_DB || `${process.env.HOME}/.atlas/rwatp-core.db`;
const REPO = process.env.REPO || process.env.RWATP_CORE || "/home/sanoy/Vesta/rwatp-core";
const OUT_DIR = process.env.SCORE_OUT || "/tmp/atlas-c5-localization-score";

fs.mkdirSync(OUT_DIR, { recursive: true });

function runInvestigate(caseSpec) {
  const args = ["investigate", "--no-ai", "--json", "--rounds", "1"];
  // Prefer --issue when set: exercises C5.1-R issue-anchored retrieval.
  // Use force_question: true to score free-text only.
  if (caseSpec.issue != null && !caseSpec.force_question) {
    args.push("--issue", String(caseSpec.issue));
  } else if (caseSpec.question) {
    args.push(caseSpec.question);
  } else {
    throw new Error(`case ${caseSpec.id}: need question or issue`);
  }

  const env = { ...process.env, ATLAS_DB };
  const r = spawnSync(ATLAS, args, {
    cwd: REPO,
    env,
    encoding: "utf8",
    maxBuffer: 40 * 1024 * 1024,
    timeout: 120_000,
  });
  if (r.error) {
    return { ok: false, error: String(r.error) };
  }
  if (r.status !== 0) {
    return {
      ok: false,
      error: `exit ${r.status}: ${(r.stderr || r.stdout || "").slice(0, 500)}`,
    };
  }
  // JSON may be preceded by status lines — find first {
  const out = r.stdout || "";
  const i = out.indexOf("{");
  if (i < 0) return { ok: false, error: "no JSON object in output" };
  try {
    return { ok: true, result: JSON.parse(out.slice(i)) };
  } catch (e) {
    return { ok: false, error: `JSON parse: ${e.message}` };
  }
}

function rankedFiles(result) {
  const p = result.packet || result;
  const ranked = p.ranked_evidence || [];
  return ranked
    .filter((r) => r.ref_?.kind === "file" || r.ref_?.kind === "path")
    .map((r) => r.ref_.id);
}

function rankedAllIds(result) {
  const p = result.packet || result;
  return (p.ranked_evidence || []).map((r) => r.ref_?.id).filter(Boolean);
}

function intersects(paths, needles) {
  return paths.filter((f) => needles.some((n) => f === n || f.endsWith(n) || f.includes(n)));
}

function scoreCase(caseSpec, result) {
  const files = rankedFiles(result);
  const allIds = rankedAllIds(result);
  const gold = caseSpec.gold_files || [];
  const hard = caseSpec.hard_negatives || [];
  const top1 = files.slice(0, 1);
  const top5 = files.slice(0, 5);
  const top10 = files.slice(0, 10);

  const top1_hit = intersects(top1, gold).length > 0;
  const top5_gold = intersects(top5, gold);
  const top10_gold = intersects(top10, gold);
  const top5_hard = intersects(top5, hard);
  const top10_hard = intersects(top10, hard);

  const claims = result.claims || [];
  let sacred_ok = true;
  let sacred_detail = null;
  if (caseSpec.c4_expectations?.forbid_supported_causal_redis) {
    const bad = claims.filter((c) => {
      if (c.status !== "supported") return false;
      const blob = `${c.statement || ""} ${JSON.stringify(c.evidence_refs || [])}`.toLowerCase();
      return (
        /redis/.test(blob) &&
        /(cause|because|related to|timeout|order)/.test(blob)
      );
    });
    sacred_ok = bad.length === 0;
    sacred_detail = bad.map((c) => c.statement?.slice(0, 80));
  }

  let documentary_hit = null;
  if (caseSpec.gold_documentary?.length) {
    const topDoc = allIds.slice(0, 10);
    documentary_hit =
      intersects(
        topDoc,
        caseSpec.gold_documentary.map((d) => d.replace(/^#/, ""))
      ).length > 0 ||
      caseSpec.gold_documentary.some((d) =>
        topDoc.some((id) => id.includes(d.replace("issue#", "").replace("pr#", "")) || id === d)
      );
  }

  const minHits = Math.min(3, gold.length);
  const pass =
    top5_hard.length === 0 &&
    (top5_gold.length >= minHits || top1_hit) &&
    sacred_ok;

  return {
    id: caseSpec.id,
    workflow: caseSpec.workflow,
    pass,
    top1_hit,
    top5_gold_hits: top5_gold.length,
    top5_gold_recall: gold.length ? top5_gold.length / Math.min(5, gold.length) : 0,
    top10_gold_hits: top10_gold.length,
    top5_hard_neg: top5_hard.length,
    top10_hard_neg: top10_hard.length,
    domain_intrusion: top5_hard.length > 0,
    documentary_hit,
    sacred_ok,
    sacred_detail,
    top5_files: top5,
    top5_gold_matched: top5_gold,
    top5_hard_matched: top5_hard,
  };
}

function main() {
  const rows = [];
  console.log(`Suite: ${suite.name}`);
  console.log(`ATLAS=${ATLAS}`);
  console.log(`ATLAS_DB=${ATLAS_DB}`);
  console.log(`REPO=${REPO}`);
  console.log("");

  for (const file of suite.cases) {
    const casePath = path.join(__dirname, file);
    const caseSpec = JSON.parse(fs.readFileSync(casePath, "utf8"));
    process.stdout.write(`… ${caseSpec.id} `);
    const run = runInvestigate(caseSpec);
    const rawPath = path.join(OUT_DIR, `${caseSpec.id}.json`);
    if (!run.ok) {
      console.log(`FAIL run: ${run.error}`);
      rows.push({
        id: caseSpec.id,
        workflow: caseSpec.workflow,
        pass: false,
        error: run.error,
      });
      fs.writeFileSync(rawPath, JSON.stringify({ error: run.error }, null, 2));
      continue;
    }
    fs.writeFileSync(rawPath, JSON.stringify(run.result, null, 2));
    const sc = scoreCase(caseSpec, run.result);
    rows.push(sc);
    console.log(
      `${sc.pass ? "PASS" : "FAIL"} top5_gold=${sc.top5_gold_hits} hard=${sc.top5_hard_neg} top1=${sc.top1_hit}`
    );
  }

  const scored = rows.filter((r) => r.top5_gold_hits != null);
  const passed = rows.filter((r) => r.pass).length;
  const passRate = rows.length ? passed / rows.length : 0;
  const gate = passRate >= (suite.gate?.min_case_pass_rate ?? 0.8);

  const summary = {
    suite: suite.name,
    cases: rows.length,
    passed,
    pass_rate: passRate,
    gate_min: suite.gate?.min_case_pass_rate ?? 0.8,
    gate_pass: gate,
    next: gate ? suite.gate?.next_milestone_if_pass : suite.gate?.next_if_fail,
    by_workflow: {},
    rows,
  };
  for (const w of ["bug_localization", "system_flow", "issue_implementation"]) {
    const sub = rows.filter((r) => r.workflow === w);
    summary.by_workflow[w] = {
      n: sub.length,
      passed: sub.filter((r) => r.pass).length,
      avg_top5_gold:
        sub.filter((r) => r.top5_gold_hits != null).reduce((a, r) => a + r.top5_gold_hits, 0) /
          (sub.filter((r) => r.top5_gold_hits != null).length || 1),
      avg_top5_hard:
        sub.filter((r) => r.top5_hard_neg != null).reduce((a, r) => a + r.top5_hard_neg, 0) /
          (sub.filter((r) => r.top5_hard_neg != null).length || 1),
    };
  }

  const summaryPath = path.join(OUT_DIR, "summary.json");
  fs.writeFileSync(summaryPath, JSON.stringify(summary, null, 2));

  console.log("\n=== SUMMARY ===");
  console.log(`pass_rate: ${(passRate * 100).toFixed(1)}% (${passed}/${rows.length})`);
  console.log(`gate (≥${(summary.gate_min * 100).toFixed(0)}%): ${gate ? "PASS → " + summary.next : "FAIL → " + summary.next}`);
  console.log("by workflow:", JSON.stringify(summary.by_workflow, null, 2));
  console.log(`wrote ${summaryPath}`);
  process.exit(gate ? 0 : 1);
}

main();
