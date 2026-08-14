#!/usr/bin/env node
/**
 * Blind adversarial eval: deterministic investigate vs Qwen tool-calling agent.
 * Gold is fixed in suite.json / GOLD.md — never rewritten after runs.
 *
 * Usage:
 *   ATLAS_BIN=./target/debug/atlas \
 *   AGENT_PY="nix-shell -p python3 --run python3" \
 *   node eval/localization/adversarial-blind/run_eval.mjs
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const suite = JSON.parse(fs.readFileSync(path.join(__dirname, "suite.json"), "utf8"));
const ATLAS = process.env.ATLAS_BIN || "atlas";
const AGENT = process.env.AGENT_SCRIPT || path.join(__dirname, "../../../agent/atlas_agent.py");
const OUT = process.env.SCORE_OUT || "/tmp/atlas-adversarial-blind";
const ONLY = process.env.ONLY || ""; // optional case id filter

fs.mkdirSync(path.join(OUT, "det"), { recursive: true });
fs.mkdirSync(path.join(OUT, "agent"), { recursive: true });
fs.mkdirSync(path.join(OUT, "logs"), { recursive: true });

function hit(paths, needles) {
  if (!paths?.length || !needles?.length) return [];
  return paths.filter((f) =>
    needles.some((n) => {
      const a = String(f).toLowerCase();
      const b = String(n).toLowerCase();
      return a.includes(b) || b.includes(a);
    })
  );
}

function parseJson(stdout) {
  const i = (stdout || "").indexOf("{");
  if (i < 0) throw new Error("no json");
  return JSON.parse(stdout.slice(i));
}

function detPaths(result) {
  const p = result.packet || result;
  const ranked = (p.ranked_evidence || [])
    .filter((r) => r.ref_?.kind === "file")
    .map((r) => r.ref_.id);
  const core = (p.investigation?.core_candidates || []).map((c) => c.file);
  const bag = [...new Set([...core, ...ranked])];
  return { ranked, core, bag, top5: ranked.slice(0, 5), top10: ranked.slice(0, 10) };
}

function runDet(repo, db, question) {
  const t0 = Date.now();
  const r = spawnSync(
    ATLAS,
    ["investigate", question, "--no-ai", "--json", "--rounds", "1"],
    {
      cwd: repo,
      env: { ...process.env, ATLAS_DB: db },
      encoding: "utf8",
      maxBuffer: 50 * 1024 * 1024,
      timeout: 120000,
      killSignal: "SIGKILL",
    }
  );
  return { r, ms: Date.now() - t0 };
}

function runAgent(repo, question) {
  const t0 = Date.now();
  // Prefer nix-shell python3 when bare python3 missing
  const py = process.env.PYTHON || "python3";
  let r;
  if (process.env.USE_NIX_PYTHON === "1" || py === "nix") {
    r = spawnSync(
      "nix-shell",
      ["-p", "python3", "--run", `python3 ${JSON.stringify(AGENT)} --repo ${JSON.stringify(repo)} --max-steps 8 ${JSON.stringify(question)}`],
      {
        encoding: "utf8",
        maxBuffer: 20 * 1024 * 1024,
        timeout: 300000,
        killSignal: "SIGKILL",
        env: {
          ...process.env,
          ATLAS_BIN: ATLAS,
          AGENT_MODEL: process.env.AGENT_MODEL || "qwen3:4b",
          AGENT_NUM_CTX: process.env.AGENT_NUM_CTX || "12288",
        },
      }
    );
  } else {
    r = spawnSync(
      py,
      [AGENT, "--repo", repo, "--max-steps", "8", question],
      {
        encoding: "utf8",
        maxBuffer: 20 * 1024 * 1024,
        timeout: 300000,
        killSignal: "SIGKILL",
        env: {
          ...process.env,
          ATLAS_BIN: ATLAS,
          AGENT_MODEL: process.env.AGENT_MODEL || "qwen3:4b",
          AGENT_NUM_CTX: process.env.AGENT_NUM_CTX || "12288",
        },
      }
    );
  }
  const out = (r.stdout || "") + (r.stderr || "");
  const calls = [...out.matchAll(/→ (\w+)\(/g)].map((m) => m[1]);
  const stepM = out.match(/\[(\d+) step\(s\), ([\d.]+)s/);
  // Final answer: after last tool result marker or whole tail
  let answer = out;
  if (out.includes("← ")) {
    const parts = out.split(/\n(?=\[?\d+ step)/);
    answer = parts[parts.length - 1] || out;
  }
  // Prefer content after last tool call block
  const lines = out.split("\n");
  let lastTool = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].includes("→ ") || lines[i].includes("← ")) lastTool = i;
  }
  if (lastTool >= 0) {
    answer = lines.slice(lastTool + 1).join("\n").trim();
  }
  return {
    r,
    ms: Date.now() - t0,
    out,
    calls,
    steps: stepM ? Number(stepM[1]) : calls.length + 1,
    reported_s: stepM ? Number(stepM[2]) : (Date.now() - t0) / 1000,
    answer,
  };
}

function scoreDet(spec, result) {
  const { bag, top5, top10 } = detPaths(result);
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const bag_g = hit(bag, gold);
  const t5g = hit(top5, gold);
  const t10g = hit(top10, gold);
  const t5h = hit(top5, hard);
  const hyps = result.hypotheses || [];
  const supported = hyps.filter((h) => String(h.status).toLowerCase() === "supported");
  const causalBad =
    spec.causal &&
    supported.some((h) => /caus|because|associat|root cause/i.test(h.statement || ""));
  const statuses = hyps.map((h) => h.status);

  // axis scores 0-100 (skeptical)
  const retrieval = Math.min(100, (bag_g.length / Math.max(1, Math.min(3, gold.length))) * 100);
  const ranking =
    t5h.length > 0
      ? Math.max(0, 40 - t5h.length * 15)
      : Math.min(100, (t5g.length / Math.max(1, Math.min(2, gold.length))) * 70 + (t5g.length ? 30 : 0));
  const structural = t10g.length >= 2 ? 70 : t10g.length === 1 ? 45 : 15;
  const historical = 40; // investigate det has chronology but weak on this suite without GH
  const causal = causalBad ? 0 : spec.causal ? 75 : 80;
  const flow =
    spec.workflow === "system_flow"
      ? Math.min(100, (bag_g.length / Math.max(2, Math.min(4, gold.length))) * 100)
      : structural;

  const overall = Math.round(
    retrieval * 0.25 +
      ranking * 0.2 +
      structural * 0.15 +
      flow * 0.15 +
      causal * 0.15 +
      (causalBad ? 0 : 50) * 0.1
  );

  return {
    bag_gold: bag_g.length,
    top5_gold: t5g.length,
    top10_gold: t10g.length,
    top5_hard: t5h.length,
    top5: top5,
    bag_matched: bag_g,
    hyp_statuses: statuses,
    causal_supported_violation: !!causalBad,
    axes: {
      retrieval: Math.round(retrieval),
      ranking: Math.round(ranking),
      structural: Math.round(structural),
      historical,
      reasoning: 0, // no AI
      grounding: 70, // det packet is grounded by construction
      causal_discipline: causal,
      flow_completeness: Math.round(flow),
      agent_usefulness: 0,
    },
    overall,
  };
}

function scoreAgent(spec, agent, detScore, detTop10) {
  const text = (agent.answer || "") + "\n" + (agent.out || "");
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const goldInAnswer = gold.filter(
    (g) =>
      text.toLowerCase().includes(g.toLowerCase()) ||
      text.toLowerCase().includes(path.basename(g).toLowerCase().replace(/\.[^.]+$/, ""))
  );
  const hardInAnswer = hard.filter((h) => text.toLowerCase().includes(h.toLowerCase().replace(/\/$/, "")));
  // Gold in tool trace (agent discovered via tools)
  const goldInTools = gold.filter((g) => (agent.out || "").toLowerCase().includes(g.toLowerCase()));
  const newVsDet = goldInTools.filter(
    (g) => !detTop10.some((d) => d.includes(g) || g.includes(d))
  );

  // Hallucinated paths: .rs/.ts/.py paths mentioned not in gold and not obviously in tool output
  const pathMentions = [...text.matchAll(/[\w./-]+\.(rs|ts|py|md)/g)].map((m) => m[0]);
  const invented = pathMentions.filter((p) => {
    if (gold.some((g) => g.includes(p) || p.includes(g))) return false;
    if ((agent.out || "").includes(p)) return false; // appeared in tool output
    // ignore very generic
    if (p.split("/").length < 2) return false;
    return true;
  });

  const causalClaim =
    /caused by|is caused|root cause|because of the|definitely causes/i.test(text);
  const hedges = /plausible|possible|might|may|could|not (enough|sufficient)|unclear|unknown|does not (prove|establish)/i.test(
    text
  );
  const causal_violation =
    !!spec.must_not_supported_cause && causalClaim && !hedges;

  const retrieval = Math.min(
    100,
    (Math.max(goldInTools.length, goldInAnswer.length) / Math.max(1, Math.min(3, gold.length))) *
      100
  );
  const ranking = hardInAnswer.length
    ? Math.max(0, 50 - hardInAnswer.length * 20)
    : goldInAnswer.length
      ? 70
      : 30;
  const structural = goldInAnswer.length >= 2 ? 65 : goldInAnswer.length === 1 ? 40 : 20;
  const reasoning = goldInAnswer.length >= 1 && agent.calls.length > 0 ? 55 : 25;
  const grounding =
    invented.length > 3 ? 25 : agent.calls.length === 0 ? 10 : invented.length ? 50 : 75;
  const causal = causal_violation ? 10 : spec.causal ? (hedges ? 80 : 45) : 70;
  const flow =
    spec.workflow === "system_flow"
      ? Math.min(100, goldInAnswer.length * 25)
      : structural;
  const agent_usefulness = newVsDet.length
    ? 80
    : goldInTools.length > (detScore.top10_gold || 0)
      ? 55
      : agent.calls.length
        ? 35
        : 0;

  const overall = Math.round(
    retrieval * 0.2 +
      ranking * 0.12 +
      structural * 0.12 +
      flow * 0.12 +
      reasoning * 0.12 +
      grounding * 0.12 +
      causal * 0.12 +
      agent_usefulness * 0.08
  );

  // Primary failure class (if overall < 55)
  let failure = null;
  if (overall < 55) {
    if (goldInTools.length === 0 && goldInAnswer.length === 0) failure = "A.Retrieval";
    else if (goldInTools.length > 0 && goldInAnswer.length === 0) failure = "F.Reasoning";
    else if (causal_violation) failure = "F.Reasoning/verification";
    else if (invented.length > 2) failure = "F.Reasoning/verification";
    else if (agent.calls.length === 0) failure = "E.Agent";
    else if (spec.workflow === "system_flow" && goldInAnswer.length < 2)
      failure = "C.Structural/flow";
    else failure = "E.Agent";
  }

  return {
    gold_in_answer: goldInAnswer,
    gold_in_tools: goldInTools,
    hard_in_answer: hardInAnswer,
    new_vs_det: newVsDet,
    invented: [...new Set(invented)].slice(0, 15),
    causal_claim: causalClaim,
    hedges,
    causal_violation,
    calls: agent.calls,
    steps: agent.steps,
    axes: {
      retrieval: Math.round(retrieval),
      ranking: Math.round(ranking),
      structural: Math.round(structural),
      historical: 35,
      reasoning: Math.round(reasoning),
      grounding: Math.round(grounding),
      causal_discipline: Math.round(causal),
      flow_completeness: Math.round(flow),
      agent_usefulness: Math.round(agent_usefulness),
    },
    overall,
    failure,
  };
}

function mean(xs) {
  return xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0;
}

function main() {
  console.log("=== BLIND adversarial eval: det vs Qwen agent ===");
  console.log("OUT", OUT);
  console.log("ATLAS", ATLAS);
  console.log("AGENT", AGENT);

  // Freeze gold copy
  fs.copyFileSync(
    path.join(__dirname, "suite.json"),
    path.join(OUT, "suite.gold.frozen.json")
  );
  fs.copyFileSync(path.join(__dirname, "GOLD.md"), path.join(OUT, "GOLD.md"));

  const rows = [];
  const cases = suite.cases.filter((c) => !ONLY || c.id === ONLY);

  for (const spec of cases) {
    const conf = suite.repos[spec.repo_key];
    console.log(`\n━━ ${spec.id} ━━`);
    console.log("Q:", spec.question.slice(0, 90) + "…");

    // A deterministic
    process.stdout.write("  det… ");
    const d = runDet(conf.path, conf.db, spec.question);
    let detResult = null;
    let detSc = null;
    if (d.r.error || d.r.status !== 0) {
      console.log("FAIL", d.r.error || d.r.signal || (d.r.stderr || "").slice(0, 80));
      detSc = { overall: 0, axes: {}, error: true };
    } else {
      try {
        detResult = parseJson(d.r.stdout);
        fs.writeFileSync(
          path.join(OUT, "det", `${spec.id}.json`),
          JSON.stringify(detResult, null, 2)
        );
        detSc = scoreDet(spec, detResult);
        detSc.ms = d.ms;
        console.log(
          `ok overall=${detSc.overall} bag=${detSc.bag_gold} top5=${detSc.top5_gold} hard=${detSc.top5_hard}`
        );
      } catch (e) {
        console.log("parse fail", e.message);
        detSc = { overall: 0, axes: {}, error: true };
      }
    }

    // B agent
    process.stdout.write("  agent… ");
    const a = runAgent(conf.path, spec.question);
    fs.writeFileSync(path.join(OUT, "logs", `${spec.id}.agent.txt`), a.out || "");
    let agSc = null;
    if (a.r.error === "ETIMEDOUT" || a.r.signal === "SIGKILL") {
      console.log("TIMEOUT");
      agSc = {
        overall: 0,
        axes: {},
        error: "timeout",
        calls: a.calls,
        steps: a.steps,
      };
    } else {
      const detTop10 = detResult ? detPaths(detResult).top10 : [];
      agSc = scoreAgent(spec, a, detSc || {}, detTop10);
      agSc.ms = a.ms;
      agSc.answer_head = (a.answer || "").slice(0, 500);
      fs.writeFileSync(
        path.join(OUT, "agent", `${spec.id}.json`),
        JSON.stringify(
          {
            calls: a.calls,
            steps: a.steps,
            ms: a.ms,
            answer: a.answer,
            score: agSc,
          },
          null,
          2
        )
      );
      console.log(
        `ok overall=${agSc.overall} calls=${a.calls.join(">") || "—"} new=${agSc.new_vs_det.length} invent=${agSc.invented.length}`
      );
    }

    rows.push({
      id: spec.id,
      repo_key: spec.repo_key,
      workflow: spec.workflow,
      question: spec.question,
      det: detSc,
      agent: agSc,
      delta_overall: (agSc?.overall || 0) - (detSc?.overall || 0),
    });
  }

  // Aggregates
  const ok = rows.filter((r) => r.det && !r.det.error && r.agent && !r.agent.error);
  const detMean = mean(ok.map((r) => r.det.overall));
  const agMean = mean(ok.map((r) => r.agent.overall));
  const byRepo = {};
  for (const r of ok) {
    byRepo[r.repo_key] = byRepo[r.repo_key] || { n: 0, det: 0, agent: 0 };
    byRepo[r.repo_key].n++;
    byRepo[r.repo_key].det += r.det.overall;
    byRepo[r.repo_key].agent += r.agent.overall;
  }
  for (const k of Object.keys(byRepo)) {
    byRepo[k].det_mean = byRepo[k].det / byRepo[k].n;
    byRepo[k].agent_mean = byRepo[k].agent / byRepo[k].n;
  }

  const newEvidence = ok.filter((r) => (r.agent.new_vs_det || []).length > 0);
  const repeated = ok.filter(
    (r) =>
      (r.agent.gold_in_tools || []).length > 0 &&
      (r.agent.new_vs_det || []).length === 0
  );
  const c4v = ok.filter((r) => r.agent.causal_violation || r.det.causal_supported_violation);
  const inventedCases = ok.filter((r) => (r.agent.invented || []).length > 0);
  const improved = ok.filter((r) => r.delta_overall > 5);
  const worsened = ok.filter((r) => r.delta_overall < -5);

  const summary = {
    suite: suite.name,
    n: ok.length,
    det_overall_mean: detMean,
    agent_overall_mean: agMean,
    by_repo: byRepo,
    avg_agent_steps: mean(ok.map((r) => r.agent.steps || 0)),
    avg_agent_calls: mean(ok.map((r) => (r.agent.calls || []).length)),
    avg_det_ms: mean(ok.map((r) => r.det.ms || 0)),
    avg_agent_ms: mean(ok.map((r) => r.agent.ms || 0)),
    qwen_new_evidence_cases: newEvidence.map((r) => r.id),
    qwen_repeat_only_cases: repeated.map((r) => r.id),
    c4_violations: c4v.map((r) => r.id),
    invented_ref_cases: inventedCases.map((r) => r.id),
    improved_cases: improved.map((r) => ({ id: r.id, d: r.delta_overall })),
    worsened_cases: worsened.map((r) => ({ id: r.id, d: r.delta_overall })),
    gates: {
      G1_qwen_improves_overall: agMean > detMean + 3,
      G2_multihop_rescue: improved.some((r) =>
        ["jj-flow", "gt-flow", "gt-orient", "jj-bug"].includes(r.id)
      ),
      G3_new_evidence: newEvidence.length > 0,
      G4_c4_violations: c4v.length,
      G5_need_c52_vs_agent:
        "deferred to report narrative from failure classes",
    },
    rows,
  };

  fs.writeFileSync(path.join(OUT, "summary.json"), JSON.stringify(summary, null, 2));
  console.log("\n=== SUMMARY ===");
  console.log(
    `det_mean=${detMean.toFixed(1)} agent_mean=${agMean.toFixed(1)} n=${ok.length}`
  );
  console.log(
    `new_ev=${newEvidence.length} c4_viol=${c4v.length} improved=${improved.length} worsened=${worsened.length}`
  );
  console.log(JSON.stringify(byRepo, null, 2));
  console.log("wrote", path.join(OUT, "summary.json"));
}

main();
