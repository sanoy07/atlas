#!/usr/bin/env node
/**
 * Blind cross-repo evaluation runner (JJ + GigaToken).
 * Gold is pre-established from independent repo inspection (not from Atlas).
 *
 * ATLAS_BIN=./target/debug/atlas node eval/localization/crossrepo/run_suite.mjs
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const suite = JSON.parse(fs.readFileSync(path.join(__dirname, "suite.json"), "utf8"));
const ATLAS = process.env.ATLAS_BIN || "atlas";
const OUT = process.env.SCORE_OUT || "/tmp/atlas-crossrepo-eval";

fs.mkdirSync(path.join(OUT, "json"), { recursive: true });
fs.mkdirSync(path.join(OUT, "human"), { recursive: true });
fs.mkdirSync(path.join(OUT, "layer1"), { recursive: true });

function run(repo, db, args, timeout = 180000) {
  return spawnSync(ATLAS, args, {
    cwd: repo,
    env: { ...process.env, ATLAS_DB: db },
    encoding: "utf8",
    maxBuffer: 40 * 1024 * 1024,
    timeout,
  });
}

function parseJson(stdout) {
  const i = (stdout || "").indexOf("{");
  if (i < 0) throw new Error("no json");
  return JSON.parse(stdout.slice(i));
}

function hit(paths, needles) {
  return paths.filter((f) =>
    needles.some((n) => f.includes(n) || n.includes(f))
  );
}

function score(spec, result) {
  const p = result.packet || result;
  const ranked = (p.ranked_evidence || [])
    .filter((r) => r.ref_?.kind === "file")
    .map((r) => r.ref_.id);
  const bag = [
    ...new Set([
      ...(p.investigation?.core_candidates || []).map((c) => c.file),
      ...ranked,
    ]),
  ];
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const top5 = ranked.slice(0, 5);
  const top1 = ranked.slice(0, 1);
  const bag_g = hit(bag, gold);
  const t5g = hit(top5, gold);
  const t5h = hit(top5, hard);
  const top1_hit = hit(top1, gold).length > 0;
  const minHits = Math.min(2, gold.length);
  const loc_pass =
    bag_g.length >= Math.min(1, gold.length) &&
    t5h.length === 0 &&
    (t5g.length >= minHits || top1_hit);

  // C4 gate: no hypothesis may be SUPPORTED solely for localization/causal existence.
  // Adversarial + any "associates" det hyp must be plausible/unresolved max.
  const hyps = result.hypotheses || [];
  const supported = hyps.filter(
    (h) => String(h.status || "").toLowerCase() === "supported"
  );
  const causalOrAssoc = (h) => {
    const s = String(h.statement || "").toLowerCase();
    return (
      s.includes("associat") ||
      s.includes("caused") ||
      s.includes("because") ||
      s.includes("related to") ||
      s.includes("root cause") ||
      /could .+ be caused/.test(s)
    );
  };
  const bad_support = supported.filter(causalOrAssoc);
  // Also flag any SUPPORTED on adversarial workflow
  const adv_support =
    spec.workflow === "adversarial"
      ? supported.length
      : bad_support.length;
  const c4_ok = adv_support === 0 && bad_support.length === 0;
  const pass = loc_pass && c4_ok;

  return {
    id: spec.id,
    repo_key: spec.repo_key,
    workflow: spec.workflow,
    pass,
    loc_pass,
    c4_ok,
    bag_gold_hits: bag_g.length,
    top5_gold_hits: t5g.length,
    top5_hard_neg: t5h.length,
    top1_hit,
    top5_files: top5,
    bag_gold_matched: bag_g,
    hyp_statuses: hyps.map((h) => h.status),
    bad_supported: bad_support.map((h) => h.statement?.slice?.(0, 80)),
    likely_area: result.likely_area || [],
    mode: result.mode,
  };
}

function layer1(repoKey, repo, db) {
  const cmds = [
    ["status"],
    ["map"],
    ["tree", "--depth", "2"],
    ["hot-files", "--limit", "10"],
    ["modules"],
  ];
  const out = {};
  for (const args of cmds) {
    const name = args[0];
    const r = run(repo, db, args, 60000);
    out[name] = {
      ok: r.status === 0,
      head: (r.stdout || r.stderr || "").slice(0, 2500),
    };
  }
  // impact on a known gold file if exists
  const impactTarget =
    repoKey === "jj"
      ? "lib/src/op_store.rs"
      : "src/bpe/pretoken_cache.rs";
  const imp = run(repo, db, ["impact", impactTarget], 60000);
  out.impact = { ok: imp.status === 0, head: (imp.stdout || "").slice(0, 2500), target: impactTarget };
  fs.writeFileSync(
    path.join(OUT, "layer1", `${repoKey}.json`),
    JSON.stringify(out, null, 2)
  );
  return out;
}

function main() {
  const rows = [];
  console.log("Suite:", suite.name);

  // Layer 1 orientation once per repo
  for (const [key, conf] of Object.entries(suite.repos)) {
    console.log(`\n--- Layer1 orientation: ${key} ---`);
    const L = layer1(key, conf.path, conf.db);
    console.log(
      Object.entries(L)
        .map(([k, v]) => `${k}:${v.ok ? "ok" : "fail"}`)
        .join(" ")
    );
  }

  console.log("\n--- Layer2 C5 investigate cases ---");
  for (const file of suite.cases) {
    const spec = JSON.parse(fs.readFileSync(path.join(__dirname, file), "utf8"));
    const conf = suite.repos[spec.repo_key];
    process.stdout.write(`… ${spec.id} `);
    const r = run(conf.path, conf.db, [
      "investigate",
      spec.question,
      "--no-ai",
      "--json",
      "--rounds",
      "1",
    ]);
    if (r.status !== 0) {
      console.log("FAIL run", (r.stderr || "").slice(0, 120));
      rows.push({ id: spec.id, pass: false, error: "run failed" });
      continue;
    }
    let result;
    try {
      result = parseJson(r.stdout);
    } catch (e) {
      console.log("FAIL parse");
      rows.push({ id: spec.id, pass: false, error: e.message });
      continue;
    }
    fs.writeFileSync(
      path.join(OUT, "json", `${spec.id}.json`),
      JSON.stringify(result, null, 2)
    );
    const hum = run(conf.path, conf.db, [
      "investigate",
      spec.question,
      "--no-ai",
      "--rounds",
      "1",
    ]);
    fs.writeFileSync(path.join(OUT, "human", `${spec.id}.txt`), hum.stdout || "");

    const sc = score(spec, result);
    rows.push(sc);
    console.log(
      `${sc.pass ? "PASS" : "FAIL"} bag=${sc.bag_gold_hits} top5=${sc.top5_gold_hits} hard=${sc.top5_hard_neg} top1=${sc.top1_hit} c4=${sc.c4_ok}`
    );
  }

  const byRepo = {};
  for (const r of rows) {
    const k = r.repo_key || "x";
    byRepo[k] = byRepo[k] || { n: 0, passed: 0 };
    byRepo[k].n++;
    if (r.pass) byRepo[k].passed++;
  }
  const passed = rows.filter((r) => r.pass).length;
  const rate = rows.length ? passed / rows.length : 0;
  const c4_violations = rows.filter((r) => r.c4_ok === false).length;
  const loc_rate =
    rows.filter((r) => r.loc_pass).length / (rows.length || 1);
  // Gate: ≥60% pass AND zero C4 association/causal SUPPORTED violations
  const gate =
    rate >= (suite.gate?.min_pass_rate ?? 0.6) && c4_violations === 0;

  const summary = {
    suite: suite.name,
    pass_rate: rate,
    loc_pass_rate: loc_rate,
    passed,
    total: rows.length,
    c4_violations,
    gate_pass: gate,
    by_repo: byRepo,
    rows,
    scores_0_100: {
      note: "Post C5.1-S + path_class + C4 hard_verify on det hyp",
      retrieval: null,
      ranking: null,
      structural: null,
      historical: null,
      verification: null,
      reasoning: null,
      cross_repo: null,
      overall: null,
    },
  };
  fs.writeFileSync(path.join(OUT, "summary.json"), JSON.stringify(summary, null, 2));
  console.log("\n=== SUMMARY ===");
  console.log(
    `pass_rate ${(rate * 100).toFixed(1)}% (${passed}/${rows.length}) loc=${(loc_rate * 100).toFixed(1)}% c4_violations=${c4_violations} gate=${gate}`
  );
  console.log(JSON.stringify(byRepo, null, 2));
  process.exit(gate ? 0 : 1);
}

main();
