#!/usr/bin/env node
/**
 * VestaScan-API cross-repo generalization suite.
 * Captures full investigate JSON (+ optional human text) and scores retrieval/ranking.
 *
 * ATLAS_BIN=./target/debug/atlas \
 * ATLAS_DB=~/.atlas/vestascan-api.db \
 * REPO=/home/sanoy/Vesta/vestascan-api \
 * node eval/localization/vestascan/run_suite.mjs
 *
 * WITH_AI=1 also runs local AI (slow) for reasoning samples.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const suite = JSON.parse(fs.readFileSync(path.join(__dirname, "suite.json"), "utf8"));
const ATLAS = process.env.ATLAS_BIN || "atlas";
const ATLAS_DB = process.env.ATLAS_DB || `${process.env.HOME}/.atlas/vestascan-api.db`;
const REPO = process.env.REPO || "/home/sanoy/Vesta/vestascan-api";
const OUT = process.env.SCORE_OUT || "/tmp/atlas-vestascan-suite";
const WITH_AI = process.env.WITH_AI === "1";
const AI_CASES = new Set(
  (process.env.AI_CASES || "vs-adv-token-intermittent,vs-adv-dataroom-create-no-access,vs-adv-secret-change-failure")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
);

fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(path.join(OUT, "json"), { recursive: true });
fs.mkdirSync(path.join(OUT, "human"), { recursive: true });

function runAtlas(args, timeoutMs = 180_000) {
  return spawnSync(ATLAS, args, {
    cwd: REPO,
    env: { ...process.env, ATLAS_DB },
    encoding: "utf8",
    maxBuffer: 50 * 1024 * 1024,
    timeout: timeoutMs,
  });
}

function parseJsonOut(stdout) {
  const i = (stdout || "").indexOf("{");
  if (i < 0) throw new Error("no JSON");
  return JSON.parse(stdout.slice(i));
}

function rankedFiles(result) {
  const p = result.packet || result;
  return (p.ranked_evidence || [])
    .filter((r) => r.ref_?.kind === "file" || r.ref_?.kind === "path")
    .map((r) => r.ref_.id);
}

function coreFiles(result) {
  const p = result.packet || result;
  return (p.investigation?.core_candidates || []).map((c) => c.file);
}

function hit(paths, needles) {
  return paths.filter((f) =>
    needles.some((n) => f === n || f.endsWith(n) || f.includes(n) || n.includes(f))
  );
}

function scoreCase(spec, result) {
  const files = rankedFiles(result);
  const bag = [...new Set([...coreFiles(result), ...files])];
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const top1 = files.slice(0, 1);
  const top5 = files.slice(0, 5);
  const top10 = files.slice(0, 10);

  const bag_gold = hit(bag, gold);
  const top5_gold = hit(top5, gold);
  const top10_gold = hit(top10, gold);
  const top5_hard = hit(top5, hard);
  const top10_hard = hit(top10, hard);
  const top1_hit = hit(top1, gold).length > 0;

  const minHits = Math.min(3, gold.length);
  const retrieval_ok = bag_gold.length >= Math.min(2, gold.length);
  const ranking_ok =
    top5_hard.length === 0 && (top5_gold.length >= minHits || top1_hit);
  const pass = retrieval_ok && ranking_ok;

  const claims = result.claims || [];
  const supported = claims.filter((c) => c.status === "supported");
  const plausible = claims.filter((c) => c.status === "plausible");

  return {
    id: spec.id,
    cluster: spec.cluster,
    workflow: spec.workflow,
    adversarial: !!spec.adversarial,
    pass,
    retrieval_ok,
    ranking_ok,
    bag_gold_hits: bag_gold.length,
    bag_size: bag.length,
    top1_hit,
    top5_gold_hits: top5_gold.length,
    top10_gold_hits: top10_gold.length,
    top5_hard_neg: top5_hard.length,
    top10_hard_neg: top10_hard.length,
    top5_files: top5,
    bag_gold_matched: bag_gold,
    top5_gold_matched: top5_gold,
    top5_hard_matched: top5_hard,
    likely_area: result.likely_area || [],
    claims_supported: supported.length,
    claims_plausible: plausible.length,
    mode: result.mode,
    c5_notes: (result.packet?.limitations || []).filter((l) =>
      String(l).includes("c5")
    ),
  };
}

function main() {
  const rows = [];
  console.log(`Suite: ${suite.name}`);
  console.log(`ATLAS=${ATLAS} DB=${ATLAS_DB} REPO=${REPO} WITH_AI=${WITH_AI}`);

  for (const file of suite.cases) {
    const spec = JSON.parse(fs.readFileSync(path.join(__dirname, file), "utf8"));
    process.stdout.write(`… ${spec.id} `);

    // Deterministic capture
    const det = runAtlas([
      "investigate",
      spec.question,
      "--no-ai",
      "--json",
      "--rounds",
      "1",
    ]);
    if (det.status !== 0) {
      console.log(`FAIL run: ${(det.stderr || det.stdout || "").slice(0, 200)}`);
      rows.push({ id: spec.id, pass: false, error: det.stderr || "run failed" });
      continue;
    }
    let result;
    try {
      result = parseJsonOut(det.stdout);
    } catch (e) {
      console.log(`FAIL parse: ${e.message}`);
      rows.push({ id: spec.id, pass: false, error: e.message });
      continue;
    }
    fs.writeFileSync(
      path.join(OUT, "json", `${spec.id}.json`),
      JSON.stringify(result, null, 2)
    );

    // Human text capture
    const hum = runAtlas(["investigate", spec.question, "--no-ai", "--rounds", "1"]);
    fs.writeFileSync(
      path.join(OUT, "human", `${spec.id}.txt`),
      hum.stdout || ""
    );

    // Optional AI for adversarial (or all if WITH_AI and listed)
    let ai_summary = null;
    if (WITH_AI && AI_CASES.has(spec.id)) {
      process.stdout.write("[ai] ");
      const ai = runAtlas(
        ["investigate", spec.question, "--rounds", "1", "--json"],
        400_000
      );
      if (ai.status === 0) {
        try {
          const ar = parseJsonOut(ai.stdout);
          fs.writeFileSync(
            path.join(OUT, "json", `${spec.id}.ai.json`),
            JSON.stringify(ar, null, 2)
          );
          ai_summary = {
            mode: ar.mode,
            model: ar.model,
            hyps: (ar.hypotheses || []).map((h) => ({
              status: h.status,
              s: (h.statement || "").slice(0, 160),
            })),
            claims: (ar.claims || []).map((c) => ({
              status: c.status,
              kind: c.kind,
              s: (c.statement || "").slice(0, 120),
            })),
            explanation: (ar.explanation || "").slice(0, 500),
          };
        } catch (_) {
          ai_summary = { error: "ai parse failed" };
        }
      } else {
        ai_summary = { error: "ai run failed", detail: (ai.stderr || "").slice(0, 200) };
      }
    }

    const sc = scoreCase(spec, result);
    sc.ai = ai_summary;
    rows.push(sc);
    console.log(
      `${sc.pass ? "PASS" : "FAIL"} ret=${sc.bag_gold_hits}/${goldN(spec)} rank_top5=${sc.top5_gold_hits} hard=${sc.top5_hard_neg} top1=${sc.top1_hit}`
    );
  }

  const passed = rows.filter((r) => r.pass).length;
  const passRate = rows.length ? passed / rows.length : 0;
  const gateMin = suite.gate?.min_case_pass_rate ?? 0.7;
  const gate = passRate >= gateMin;

  const byCluster = {};
  for (const r of rows) {
    const c = r.cluster || "unknown";
    byCluster[c] = byCluster[c] || { n: 0, passed: 0, avg_top5: 0, avg_hard: 0 };
    byCluster[c].n++;
    if (r.pass) byCluster[c].passed++;
    byCluster[c].avg_top5 += r.top5_gold_hits || 0;
    byCluster[c].avg_hard += r.top5_hard_neg || 0;
  }
  for (const c of Object.keys(byCluster)) {
    byCluster[c].avg_top5 /= byCluster[c].n || 1;
    byCluster[c].avg_hard /= byCluster[c].n || 1;
  }

  const summary = {
    suite: suite.name,
    repository: suite.repository,
    cases: rows.length,
    passed,
    pass_rate: passRate,
    gate_min: gateMin,
    gate_pass: gate,
    by_cluster: byCluster,
    rows,
    comparison_note:
      "Compare to RWATP C5 suite: 9/9 after C5.1-R+L. This suite tests generalization to VestaScan domains.",
  };
  fs.writeFileSync(path.join(OUT, "summary.json"), JSON.stringify(summary, null, 2));

  // Markdown report skeleton for human grading of reasoning
  let md = `# VestaScan-API C5 generalization suite\n\n`;
  md += `Date: ${new Date().toISOString()}\n\n`;
  md += `pass_rate: **${(passRate * 100).toFixed(1)}%** (${passed}/${rows.length}) · gate≥${gateMin}: **${gate ? "PASS" : "FAIL"}**\n\n`;
  md += `Artifacts: \`${OUT}\`\n\n`;
  md += `## Per-case (deterministic retrieval + ranking)\n\n`;
  md += `| id | cluster | pass | bag gold | top5 gold | hard5 | top1 |\n|----|---------|------|----------|-----------|-------|------|\n`;
  for (const r of rows) {
    md += `| ${r.id} | ${r.cluster} | ${r.pass ? "Y" : "N"} | ${r.bag_gold_hits ?? "-"} | ${r.top5_gold_hits ?? "-"} | ${r.top5_hard_neg ?? "-"} | ${r.top1_hit} |\n`;
  }
  md += `\n## Scoring dimensions for human/AI review\n\n`;
  md += `1. **Retrieval** — bag_gold_hits (did correct files enter packet?)\n`;
  md += `2. **Ranking** — top5_gold / hard_neg\n`;
  md += `3. **Reasoning** — see \`json/*.ai.json\` if WITH_AI=1; else run Qwen separately\n`;
  md += `4. **Verification** — claim statuses under C4; ask "what would disprove this?"\n\n`;
  for (const r of rows) {
    md += `### ${r.id}\n`;
    md += `- top5: ${(r.top5_files || []).map((f) => "`" + f.split("/").slice(-2).join("/") + "`").join(", ")}\n`;
    md += `- bag gold matched: ${(r.bag_gold_matched || []).map((f) => "`" + f.split("/").pop() + "`").join(", ") || "—"}\n`;
    if (r.ai) {
      md += `- AI: ${JSON.stringify(r.ai).slice(0, 300)}\n`;
    }
    md += `\n`;
  }
  fs.writeFileSync(path.join(OUT, "REPORT.md"), md);

  console.log("\n=== SUMMARY ===");
  console.log(`pass_rate: ${(passRate * 100).toFixed(1)}% (${passed}/${rows.length})`);
  console.log(`gate (≥${gateMin}): ${gate ? "PASS" : "FAIL"}`);
  console.log("by cluster:", JSON.stringify(byCluster, null, 2));
  console.log(`wrote ${path.join(OUT, "summary.json")}`);
  console.log(`wrote ${path.join(OUT, "REPORT.md")}`);
  process.exit(gate ? 0 : 1);
}

function goldN(spec) {
  return (spec.gold_files || []).length;
}

main();
