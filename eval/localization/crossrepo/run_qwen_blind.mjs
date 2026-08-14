#!/usr/bin/env node
/**
 * Frozen C5.1 + C4 baseline vs Atlas + Qwen 3 Thinking investigator.
 *
 * Protocol:
 *   - Same gold cases as blind JJ + GigaToken suite (independent gold).
 *   - A: deterministic packet only (--no-ai)
 *   - B: Qwen 3 over the evidence packet (multi-round, can request subjects)
 *   - Qwen never gets the repository tree beyond the packet.
 *
 * Env:
 *   ATLAS_BIN          path to atlas binary
 *   ATLAS_OLLAMA_MODEL default qwen3:4b
 *   SCORE_OUT          default /tmp/atlas-qwen-blind
 *   QWEN_ROUNDS        default 2
 *   SKIP_DET=1         skip re-running det if det/ exists
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const suite = JSON.parse(fs.readFileSync(path.join(__dirname, "suite.json"), "utf8"));
const ATLAS = process.env.ATLAS_BIN || "atlas";
const OUT = process.env.SCORE_OUT || "/tmp/atlas-qwen-blind";
const MODEL = process.env.ATLAS_OLLAMA_MODEL || "qwen3:4b";
const ROUNDS = Number(process.env.QWEN_ROUNDS || "1");
const SKIP_DET = process.env.SKIP_DET === "1";
const SKIP_QWEN = process.env.SKIP_QWEN === "1"; // resume: keep existing qwen/*.json
// Per-round Ollama budget (seconds). Suite kills atlas if wall time exceeded.
const OLLAMA_TIMEOUT = process.env.ATLAS_OLLAMA_TIMEOUT || "90";
const WALL_DET = Number(process.env.WALL_DET_MS || "90000");
const WALL_QWEN = Number(process.env.WALL_QWEN_MS || String(Number(OLLAMA_TIMEOUT) * 1000 * ROUNDS + 60000));

for (const d of ["det", "qwen", "human"]) {
  fs.mkdirSync(path.join(OUT, d), { recursive: true });
}

function run(repo, db, args, timeout = 300000) {
  // detached process group so timeout can kill ollama-waiting atlas + children
  return spawnSync(ATLAS, args, {
    cwd: repo,
    env: {
      ...process.env,
      ATLAS_DB: db,
      ATLAS_OLLAMA_MODEL: MODEL,
      // Thinking experiment: enable thinking channel for qwen3
      ATLAS_OLLAMA_THINK: process.env.ATLAS_OLLAMA_THINK || "1",
      ATLAS_OLLAMA_TIMEOUT: OLLAMA_TIMEOUT,
      ATLAS_OLLAMA_NUM_PREDICT: process.env.ATLAS_OLLAMA_NUM_PREDICT || "4096",
    },
    encoding: "utf8",
    maxBuffer: 50 * 1024 * 1024,
    timeout,
    killSignal: "SIGKILL",
  });
}

function parseJson(stdout) {
  const i = (stdout || "").indexOf("{");
  if (i < 0) throw new Error("no json");
  return JSON.parse(stdout.slice(i));
}

function hit(paths, needles) {
  return paths.filter((f) => needles.some((n) => f.includes(n) || n.includes(f)));
}

function collectPaths(result) {
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
  return { ranked, bag, top5: ranked.slice(0, 5), top1: ranked.slice(0, 1) };
}

function packetPathSet(result) {
  const p = result.packet || result;
  const set = new Set();
  for (const c of p.investigation?.core_candidates || []) set.add(c.file);
  for (const c of p.investigation?.supporting_artifacts || []) set.add(c.file);
  for (const r of p.ranked_evidence || []) {
    if (r.ref_?.kind === "file") set.add(r.ref_.id);
    if (r.ref_?.id) set.add(r.ref_.id);
  }
  for (const a of p.anchors || []) set.add(a);
  for (const e of p.chronology || []) if (e.id) set.add(e.id);
  return set;
}

function locScore(spec, result) {
  const { bag, top5, top1 } = collectPaths(result);
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const bag_g = hit(bag, gold);
  const t5g = hit(top5, gold);
  const t5h = hit(top5, hard);
  const top1_hit = hit(top1, gold).length > 0;
  const minHits = Math.min(2, gold.length);
  const loc_pass =
    bag_g.length >= Math.min(1, gold.length) &&
    t5h.length === 0 &&
    (t5g.length >= minHits || top1_hit);
  return {
    loc_pass,
    bag_gold_hits: bag_g.length,
    top5_gold_hits: t5g.length,
    top5_hard_neg: t5h.length,
    top1_hit,
    top5_files: top5,
    bag_gold_matched: bag_g,
  };
}

function aiScore(spec, result) {
  const hyps = result.hypotheses || [];
  const claims = result.claims || [];
  const rounds = result.rounds || [];
  const gold = spec.gold_files || [];
  const hard = spec.hard_negatives || [];
  const packetPaths = packetPathSet(result);
  const explanation = String(result.explanation || "");
  const expl_l = explanation.toLowerCase();

  // Hypotheses / claims statuses after C4
  const hyp_status = hyps.map((h) => String(h.status || "").toLowerCase());
  const claim_status = claims.map((c) => String(c.status || "").toLowerCase());
  const supported_hyps = hyps.filter(
    (h) => String(h.status || "").toLowerCase() === "supported"
  );
  const supported_claims = claims.filter(
    (c) => String(c.status || "").toLowerCase() === "supported"
  );

  const isCausalOrAssoc = (s) => {
    const t = String(s || "").toLowerCase();
    return (
      t.includes("associat") ||
      t.includes("caused") ||
      t.includes("because") ||
      t.includes("related to") ||
      t.includes("root cause") ||
      /could .+ be caused/.test(t)
    );
  };
  const bad_supported = [
    ...supported_hyps.filter((h) => isCausalOrAssoc(h.statement)),
    ...supported_claims.filter(
      (c) =>
        isCausalOrAssoc(c.statement) ||
        String(c.kind || "").toLowerCase() === "causal"
    ),
  ];
  if (spec.workflow === "adversarial") {
    for (const h of supported_hyps) {
      if (!bad_supported.includes(h)) bad_supported.push(h);
    }
    for (const c of supported_claims) {
      if (!bad_supported.includes(c)) bad_supported.push(c);
    }
  }

  // Requested subjects from AI rounds
  const requested = [];
  for (const r of rounds) {
    const raw = r.raw_ai_response || {};
    for (const s of raw.requested_subjects || []) {
      if (s && !requested.includes(s)) requested.push(s);
    }
  }
  const req_gold = hit(requested, gold);
  const req_hard = hit(requested, hard);

  // Gold file mentions in explanation
  const expl_gold = gold.filter(
    (g) =>
      expl_l.includes(g.toLowerCase()) ||
      expl_l.includes(g.split("/").pop().toLowerCase().replace(/\.[^.]+$/, ""))
  );

  // Invented refs: claim/hyp evidence not in packet (approx)
  const invented = [];
  const checkRef = (r) => {
    if (!r || !r.id) return;
    const id = r.id;
    if (packetPaths.has(id)) return;
    // allow #N issue form if documentary not present
    if (/^#?\d+$/.test(id)) return;
    // partial path match
    for (const p of packetPaths) {
      if (typeof p === "string" && (p.includes(id) || id.includes(p))) return;
    }
    invented.push(id);
  };
  for (const h of hyps) {
    for (const r of h.supporting || []) checkRef(r);
  }
  for (const c of claims) {
    for (const r of c.evidence_refs || []) checkRef(r);
  }

  // Unknown handling: explanation or questions acknowledge gaps
  const questions = [];
  for (const r of rounds) {
    for (const q of r.raw_ai_response?.questions || []) questions.push(q);
  }
  const acknowledges_unknown =
    /\b(unknown|not (in|enough|sufficient)|cannot establish|missing|unclear|insufficient|do not know|doesn't show|does not show|no evidence)\b/i.test(
      explanation
    ) || questions.length > 0;

  // Structured JSON success: non-empty hyps or claims from AI (not only det)
  const ai_invoked = rounds.some((r) => r.ai_invoked);
  const structured =
    claims.length > 0 ||
    hyps.some((h) => h.id && h.id !== "det-1") ||
    requested.length > 0;

  // Multi-hop signal: requested subjects that expand beyond initial top5
  const { top5 } = collectPaths(result);
  const multi_hop_request = requested.some(
    (s) => !top5.some((t) => t.includes(s) || s.includes(t))
  );

  return {
    mode: result.mode,
    model: result.model,
    ai_invoked,
    rounds: rounds.length,
    structured_json: structured,
    hyp_count: hyps.length,
    claim_count: claims.length,
    hyp_statuses: hyp_status,
    claim_statuses: claim_status,
    supported_hyp_count: supported_hyps.length,
    supported_claim_count: supported_claims.length,
    bad_supported_count: bad_supported.length,
    c4_ok: bad_supported.length === 0,
    requested_subjects: requested,
    requested_gold_hits: req_gold.length,
    requested_hard_hits: req_hard.length,
    requested_gold: req_gold,
    expl_gold_mentions: expl_gold.length,
    expl_gold,
    invented_refs: [...new Set(invented)].slice(0, 20),
    invented_count: new Set(invented).size,
    acknowledges_unknown,
    multi_hop_request,
    explanation_head: explanation.slice(0, 400),
    questions: questions.slice(0, 8),
  };
}

function investigate(repo, db, question, { noAi, rounds }) {
  const args = ["investigate", question, "--json", "--rounds", String(rounds)];
  if (noAi) args.push("--no-ai");
  const r = run(repo, db, args, noAi ? WALL_DET : WALL_QWEN);
  return r;
}

function main() {
  console.log("Suite:", suite.name);
  console.log("Model:", MODEL, "rounds:", ROUNDS);
  console.log("OUT:", OUT);

  const rows = [];

  for (const file of suite.cases) {
    const spec = JSON.parse(fs.readFileSync(path.join(__dirname, file), "utf8"));
    const conf = suite.repos[spec.repo_key];
    process.stdout.write(`\n… ${spec.id} `);

    // ── A: deterministic ──────────────────────────────────────────────
    let detResult;
    const detPath = path.join(OUT, "det", `${spec.id}.json`);
    if (SKIP_DET && fs.existsSync(detPath)) {
      detResult = JSON.parse(fs.readFileSync(detPath, "utf8"));
      process.stdout.write("det=cache ");
    } else {
      const r = investigate(conf.path, conf.db, spec.question, {
        noAi: true,
        rounds: 1,
      });
      if (r.error === "ETIMEDOUT" || r.signal) {
        console.log("DET TIMEOUT/KILL", r.signal || r.error);
        rows.push({ id: spec.id, error: "det timeout" });
        continue;
      }
      if (r.status !== 0) {
        console.log("DET FAIL", (r.stderr || "").slice(0, 100));
        rows.push({ id: spec.id, error: "det failed" });
        continue;
      }
      try {
        detResult = parseJson(r.stdout);
        fs.writeFileSync(detPath, JSON.stringify(detResult, null, 2));
      } catch (e) {
        console.log("DET PARSE FAIL", e.message);
        rows.push({ id: spec.id, error: "det parse" });
        continue;
      }
      process.stdout.write("det=ok ");
    }

    // ── B: Qwen investigator ──────────────────────────────────────────
    let qwenResult;
    const qwenPath = path.join(OUT, "qwen", `${spec.id}.json`);
    if (SKIP_QWEN && fs.existsSync(qwenPath)) {
      qwenResult = JSON.parse(fs.readFileSync(qwenPath, "utf8"));
      process.stdout.write("qwen=cache ");
    } else {
      const rAi = investigate(conf.path, conf.db, spec.question, {
        noAi: false,
        rounds: ROUNDS,
      });
      if (rAi.error === "ETIMEDOUT" || rAi.signal) {
        console.log("QWEN TIMEOUT/KILL", rAi.signal || rAi.error);
        const detLoc = locScore(spec, detResult);
        rows.push({
          id: spec.id,
          repo_key: spec.repo_key,
          workflow: spec.workflow,
          det: detLoc,
          qwen: { error: "timeout", signal: rAi.signal },
          delta: null,
        });
        continue;
      }
      if (rAi.status !== 0) {
        console.log("QWEN FAIL", (rAi.stderr || rAi.stdout || "").slice(0, 120));
        const detLoc = locScore(spec, detResult);
        rows.push({
          id: spec.id,
          repo_key: spec.repo_key,
          workflow: spec.workflow,
          det: detLoc,
          qwen: { error: "run failed", stderr: (rAi.stderr || "").slice(0, 200) },
          delta: null,
        });
        continue;
      }
      try {
        qwenResult = parseJson(rAi.stdout);
        fs.writeFileSync(qwenPath, JSON.stringify(qwenResult, null, 2));
      } catch (e) {
        console.log("QWEN PARSE FAIL");
        rows.push({ id: spec.id, error: "qwen parse" });
        continue;
      }
    }

    const detLoc = locScore(spec, detResult);
    const qwenLoc = locScore(spec, qwenResult);
    const qwenAi = aiScore(spec, qwenResult);

    const delta = {
      loc_pass: Number(qwenLoc.loc_pass) - Number(detLoc.loc_pass),
      bag_gold: qwenLoc.bag_gold_hits - detLoc.bag_gold_hits,
      top5_gold: qwenLoc.top5_gold_hits - detLoc.top5_gold_hits,
      top5_hard: qwenLoc.top5_hard_neg - detLoc.top5_hard_neg,
      // positive when Qwen requests gold subjects
      requested_gold: qwenAi.requested_gold_hits,
      invented: qwenAi.invented_count,
      bad_supported: qwenAi.bad_supported_count,
    };

    rows.push({
      id: spec.id,
      repo_key: spec.repo_key,
      workflow: spec.workflow,
      question: spec.question,
      det: detLoc,
      qwen: { ...qwenLoc, ...qwenAi },
      delta,
    });

    console.log(
      `det_loc=${detLoc.loc_pass ? "P" : "F"} qwen_loc=${qwenLoc.loc_pass ? "P" : "F"} ` +
        `struct=${qwenAi.structured_json} req_gold=${qwenAi.requested_gold_hits} ` +
        `invented=${qwenAi.invented_count} c4=${qwenAi.c4_ok} ` +
        `Δbag=${delta.bag_gold}`
    );
  }

  // ── Aggregate ─────────────────────────────────────────────────────────
  const ok = rows.filter((r) => r.det && r.qwen && !r.qwen.error);
  const det_pass = ok.filter((r) => r.det.loc_pass).length;
  const qwen_pass = ok.filter((r) => r.qwen.loc_pass).length;
  const c4_viol = ok.filter((r) => r.qwen.c4_ok === false).length;
  const structured = ok.filter((r) => r.qwen.structured_json).length;
  const improved_bag = ok.filter((r) => r.delta && r.delta.bag_gold > 0).length;
  const worse_bag = ok.filter((r) => r.delta && r.delta.bag_gold < 0).length;
  const req_gold_any = ok.filter((r) => r.qwen.requested_gold_hits > 0).length;
  const invented_any = ok.filter((r) => r.qwen.invented_count > 0).length;
  const unknown_ok = ok.filter((r) => r.qwen.acknowledges_unknown).length;
  const multi_hop = ok.filter((r) => r.qwen.multi_hop_request).length;

  const byRepo = {};
  for (const r of ok) {
    const k = r.repo_key;
    byRepo[k] = byRepo[k] || {
      n: 0,
      det_pass: 0,
      qwen_pass: 0,
      bag_improved: 0,
    };
    byRepo[k].n++;
    if (r.det.loc_pass) byRepo[k].det_pass++;
    if (r.qwen.loc_pass) byRepo[k].qwen_pass++;
    if (r.delta?.bag_gold > 0) byRepo[k].bag_improved++;
  }

  // Remaining FAIL cases under det — did Qwen help?
  const detFails = ok.filter((r) => !r.det.loc_pass);
  const rescued = detFails.filter((r) => r.qwen.loc_pass);

  const summary = {
    suite: suite.name,
    experiment: "atlas-deterministic vs atlas+qwen3-thinking",
    model: MODEL,
    rounds: ROUNDS,
    n: ok.length,
    det_loc_pass: det_pass,
    qwen_loc_pass: qwen_pass,
    det_loc_rate: ok.length ? det_pass / ok.length : 0,
    qwen_loc_rate: ok.length ? qwen_pass / ok.length : 0,
    c4_violations: c4_viol,
    structured_json_rate: ok.length ? structured / ok.length : 0,
    bag_improved: improved_bag,
    bag_worse: worse_bag,
    requested_gold_cases: req_gold_any,
    invented_ref_cases: invented_any,
    acknowledges_unknown_cases: unknown_ok,
    multi_hop_request_cases: multi_hop,
    det_fails: detFails.map((r) => r.id),
    rescued_by_qwen: rescued.map((r) => r.id),
    by_repo: byRepo,
    rows,
    measures: {
      note: "Independent gold unchanged. Qwen receives evidence packet only.",
      questions: [
        "Does Qwen correctly synthesize the evidence?",
        "Does it discover multi-hop relationships?",
        "Does it recognize missing evidence?",
        "Does it distinguish implementation from historical intent?",
        "Does it generate useful follow-up retrieval requests?",
        "Does it introduce unsupported claims?",
        "Does C4 successfully reject those claims?",
        "Does additional retrieval actually improve the answer?",
      ],
    },
  };

  fs.writeFileSync(path.join(OUT, "summary.json"), JSON.stringify(summary, null, 2));
  console.log("\n=== SUMMARY ===");
  console.log(
    `det_loc ${det_pass}/${ok.length} (${(summary.det_loc_rate * 100).toFixed(1)}%)  ` +
      `qwen_loc ${qwen_pass}/${ok.length} (${(summary.qwen_loc_rate * 100).toFixed(1)}%)`
  );
  console.log(
    `structured=${structured}/${ok.length} c4_viol=${c4_viol} ` +
      `bag+${improved_bag}/-${worse_bag} req_gold=${req_gold_any} invented=${invented_any}`
  );
  console.log(
    `unknown_ack=${unknown_ok} multi_hop_req=${multi_hop} rescued=${rescued.map((r) => r.id).join(",") || "none"}`
  );
  console.log(JSON.stringify(byRepo, null, 2));
  // Experiment does not fail the process on localization — report only
  process.exit(0);
}

main();
