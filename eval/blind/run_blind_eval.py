#!/usr/bin/env python3
"""
Blind adversarial evaluation runner.

Arm A: deterministic  — atlas investigate "<q>" --no-ai
Arm B: agent          — agent/atlas_agent.py --repo <repo> "<q>"

Records the full output of both arms, plus the agent's tool calls, step count
and latency. Scoring is done by hand against eval/blind/2026-08-10-gold-set.md,
which was frozen before this script was first run.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time

ATLAS = "/home/sanoy/projects/atlas/target/release/atlas"
AGENT = "/home/sanoy/projects/atlas/agent/atlas_agent.py"
JJ = "/home/sanoy/projects/research/jj"
GT = "/home/sanoy/projects/research/gigatoken"

QUESTIONS = [
    ("J1", JJ, "What are the major production subsystems of this repository, and where should I start reading?"),
    ("J2", JJ, "What is the main architecture of this system? Identify the important layers and components and how they relate."),
    ("J3", JJ, "Two jj commands running at the same time occasionally leave the repository reporting a different history than expected afterwards. Where should I look?"),
    ("J4", JJ, "Walk me through the end-to-end flow of creating a new commit in jj, from the CLI entry point through to storage."),
    ("J5", JJ, "This repository contains a design document for the jj run command. What would I need to change to bring the implementation in line with that document?"),
    ("J6", JJ, "If I modify lib/src/backend.rs, what other production components should I investigate and why?"),
    ("J7", JJ, "Which component is responsible for resolving conflicts when a user runs jj resolve?"),
    ("G1", GT, "What are the major production subsystems of this repository, and where should I start reading?"),
    ("G2", GT, "What is the main architecture of this system? Identify the important layers and components and how they relate."),
    ("G3", GT, "Some users report that tokenizing the same text gives different results on their machine than on ours, and it seems to depend on their CPU. Where should I look?"),
    ("G4", GT, "Walk me through the end-to-end flow of encoding a batch of documents into tokens, starting from the Python API."),
    ("G5", GT, "A recent change made from_tiktoken take the pretokenizer and special tokens from the caller instead of inferring them. What would I need to change to implement that?"),
    ("G6", GT, "If I modify src/bpe/pretoken_cache.rs, what other production components should I investigate and why?"),
    ("G7", GT, "Which component causes the slow start-up time when loading a tokenizer from HuggingFace?"),
]

CALL_RE = re.compile(r"→ (\w+)\((.*?)\)")
STEPS_RE = re.compile(r"\[(\d+) step\(s\), ([\d.]+)s")


def run_deterministic(repo: str, q: str) -> dict:
    t0 = time.time()
    try:
        p = subprocess.run(
            [ATLAS, "investigate", q, "--no-ai"],
            cwd=repo, capture_output=True, text=True, timeout=300,
            env={**os.environ, "RUST_LOG": "warn"},
        )
        out = p.stdout
    except subprocess.TimeoutExpired:
        return {"status": "TIMEOUT", "elapsed": time.time() - t0, "output": ""}
    return {"status": "OK", "elapsed": time.time() - t0, "output": out}


def run_agent(repo: str, q: str) -> dict:
    t0 = time.time()
    try:
        p = subprocess.run(
            [sys.executable, AGENT, "--repo", repo, q],
            capture_output=True, text=True, timeout=900,
        )
        out = p.stdout + (("\nSTDERR:\n" + p.stderr) if p.stderr.strip() else "")
    except subprocess.TimeoutExpired:
        return {"status": "TIMEOUT", "elapsed": time.time() - t0, "output": ""}

    calls = [{"tool": m.group(1), "args": m.group(2)} for m in CALL_RE.finditer(out)]
    m = STEPS_RE.search(out)
    return {
        "status": "OK",
        "elapsed": float(m.group(2)) if m else time.time() - t0,
        "steps": int(m.group(1)) if m else None,
        "tool_calls": calls,
        "n_calls": len(calls),
        "distinct_tools": sorted({c["tool"] for c in calls}),
        # did it re-query the same tool with different args (refinement)?
        "refined": len(calls) > len({(c["tool"], c["args"]) for c in calls})
                   or len(calls) > len({c["tool"] for c in calls}),
        "output": out,
    }


def main() -> int:
    only = sys.argv[1:] or None
    outdir = os.path.dirname(os.path.abspath(__file__))
    results = []

    for qid, repo, q in QUESTIONS:
        if only and qid not in only:
            continue
        print(f"\n{'=' * 70}\n{qid}  [{os.path.basename(repo)}]\n{q}\n{'=' * 70}", flush=True)

        det = run_deterministic(repo, q)
        print(f"  A/deterministic: {det['status']} {det['elapsed']:.1f}s "
              f"({len(det['output'])} chars)", flush=True)

        ag = run_agent(repo, q)
        tools = ",".join(c["tool"] for c in ag.get("tool_calls", []))
        print(f"  B/agent:         {ag['status']} {ag['elapsed']:.1f}s "
              f"steps={ag.get('steps')} calls={ag.get('n_calls')} [{tools}]", flush=True)

        results.append({"id": qid, "repo": repo, "question": q,
                        "deterministic": det, "agent": ag})

        with open(os.path.join(outdir, "raw_results.json"), "w") as fh:
            json.dump(results, fh, indent=2)

    # Human-readable transcript for scoring
    with open(os.path.join(outdir, "transcripts.md"), "w") as fh:
        for r in results:
            fh.write(f"\n\n{'#' * 3} {r['id']} — {r['question']}\n")
            fh.write(f"\n**Repo:** `{r['repo']}`\n")
            fh.write("\n<details><summary>A · deterministic</summary>\n\n```\n")
            fh.write(r["deterministic"]["output"][:6000])
            fh.write("\n```\n</details>\n")
            fh.write("\n<details><summary>B · agent</summary>\n\n```\n")
            fh.write(r["agent"]["output"][:6000])
            fh.write("\n```\n</details>\n")

    print(f"\nwrote raw_results.json and transcripts.md to {outdir}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
