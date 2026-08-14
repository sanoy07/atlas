#!/usr/bin/env python3
"""
Measure whether qwen3:4b actually uses Atlas correctly as a tool-calling agent.

Each case states a question, the file path the final answer must land on, and
the tools a competent investigator would reach for. We score three things
separately, because they fail independently:

  grounded   — did it call any tool at all, rather than answering from weights?
  tool_fit   — was its first tool choice one of the sensible ones?
  correct    — did the final answer name the expected path?

Run:  python3 bench_agent.py [--repo /path] [--runs 1]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time

AGENT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "atlas_agent.py")

CASES = [
    {
        "id": "support-unread",
        "q": "how is the unread support ticket count computed?",
        "expect": "support-ticket.service.ts",
        "good_first": {"atlas_search", "atlas_map", "atlas_focus", "grep"},
    },
    {
        "id": "rbac-permissions",
        "q": "where are RBAC permissions declared?",
        "expect": "permissions.registry.ts",
        "good_first": {"atlas_search", "atlas_map", "grep"},
    },
    {
        "id": "kyc-flow",
        "q": "which service runs the KYC flow engine?",
        "expect": "kyc-flow-engine.service.ts",
        "good_first": {"atlas_search", "atlas_map", "atlas_focus", "grep"},
    },
    {
        "id": "hottest-file",
        "q": "which file changes most often in this repository?",
        "expect": "package.json",
        "good_first": {"atlas_map", "atlas_modules"},
    },
    {
        "id": "coupling",
        "q": "which two modules are most coupled to each other?",
        "expect": "core",
        "good_first": {"atlas_map", "atlas_cohorts", "atlas_modules"},
    },
    {
        "id": "blast-radius",
        "q": "if I change src/common/rbac/permissions.registry.ts, what else is likely affected?",
        "expect": "roles.registry.ts",
        "good_first": {"atlas_impact", "atlas_focus", "atlas_explain"},
    },
]

CALL_RE = re.compile(r"→ (\w+)\(")
STEPS_RE = re.compile(r"\[(\d+) step\(s\), ([\d.]+)s")


def run_case(case: dict, repo: str, timeout: int) -> dict:
    t0 = time.time()
    try:
        p = subprocess.run(
            [sys.executable, AGENT, "--repo", repo, case["q"]],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        out = p.stdout
    except subprocess.TimeoutExpired:
        return {**case, "status": "TIMEOUT", "elapsed": time.time() - t0}

    calls = CALL_RE.findall(out)
    m = STEPS_RE.search(out)
    steps = int(m.group(1)) if m else len(calls) + 1
    elapsed = float(m.group(2)) if m else time.time() - t0

    # The final answer is everything after the last tool-result line.
    answer = out.split("← ")[-1]
    answer = answer.split("\n", 1)[1] if "\n" in answer else answer

    grounded = len(calls) > 0
    tool_fit = bool(calls) and calls[0] in case["good_first"]
    correct = case["expect"].lower() in answer.lower()

    return {
        **case,
        "status": "OK",
        "calls": calls,
        "steps": steps,
        "elapsed": elapsed,
        "grounded": grounded,
        "tool_fit": tool_fit,
        "correct": correct,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="/home/sanoy/Vesta/rwatp-core")
    ap.add_argument("--runs", type=int, default=1, help="repeats per case")
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--json-out", default="")
    args = ap.parse_args()

    results = []
    print(f"qwen3:4b × Atlas agent benchmark — {len(CASES)} cases × {args.runs} run(s)\n")
    for case in CASES:
        for r in range(args.runs):
            res = run_case(case, args.repo, args.timeout)
            results.append(res)
            if res["status"] != "OK":
                print(f"  {case['id']:16} {res['status']}")
                continue
            flag = lambda b: "\033[32m✓\033[0m" if b else "\033[31m✗\033[0m"
            print(
                f"  {case['id']:16} grounded={flag(res['grounded'])} "
                f"tool_fit={flag(res['tool_fit'])} correct={flag(res['correct'])} "
                f"steps={res['steps']} {res['elapsed']:5.1f}s  "
                f"first={res['calls'][0] if res['calls'] else '—'}"
            )

    ok = [r for r in results if r["status"] == "OK"]
    n = len(ok) or 1
    print("\nAGGREGATE")
    print(f"  completed:      {len(ok)}/{len(results)}")
    print(f"  grounded:       {sum(r['grounded'] for r in ok)}/{n}")
    print(f"  tool_fit:       {sum(r['tool_fit'] for r in ok)}/{n}")
    print(f"  correct:        {sum(r['correct'] for r in ok)}/{n}")
    print(f"  mean steps:     {sum(r['steps'] for r in ok) / n:.1f}")
    print(f"  mean latency:   {sum(r['elapsed'] for r in ok) / n:.1f}s")

    if args.json_out:
        with open(args.json_out, "w") as fh:
            json.dump(
                [{k: (sorted(v) if isinstance(v, set) else v) for k, v in r.items()}
                 for r in results],
                fh,
                indent=2,
            )
        print(f"\n  wrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
