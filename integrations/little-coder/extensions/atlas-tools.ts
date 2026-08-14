/**
 * little-coder / pi extension: register Atlas CLI as agent-facing tools.
 *
 * Drop into ~/.config/little-coder/extensions/ or set:
 *   LITTLE_CODER_EXTRA_EXTENSIONS=/path/to/atlas-tools.ts
 *   LITTLE_CODER_BASH_ALLOW="atlas "
 *
 * Requires: atlas on PATH, ATLAS_DB set for the project.
 *
 * This uses pi's extension API loosely — if your pi version differs, treat this
 * as a template and adapt registerTool / tool definitions to the installed API.
 * Fallback: rely on skills/atlas-evidence.md + bash allowlist alone.
 */

import { spawnSync } from "node:child_process";

const ATLAS = process.env.ATLAS_BIN || "atlas";

function atlas(args: string[], cwd?: string): string {
  const r = spawnSync(ATLAS, args, {
    cwd: cwd || process.cwd(),
    encoding: "utf8",
    env: process.env,
    maxBuffer: 4 * 1024 * 1024,
    timeout: 120_000,
  });
  if (r.error) return `ERROR: ${r.error.message}`;
  const out = `${r.stdout || ""}${r.stderr ? "\n" + r.stderr : ""}`.trim();
  return out || "(no output)";
}

export const tools = [
  {
    name: "atlas_investigate",
    description:
      "Deterministic Atlas evidence packet for a repository question (prefer over grep).",
    parameters: {
      type: "object",
      properties: {
        question: { type: "string", description: "Investigation question" },
      },
      required: ["question"],
    },
    execute: async ({ question }: { question: string }) =>
      atlas(["investigate", "--no-ai", question]),
  },
  {
    name: "atlas_callers",
    description: "Who calls this symbol? OBSERVED structural reverse edges.",
    parameters: {
      type: "object",
      properties: {
        subject: { type: "string", description: "Symbol or Class.method" },
      },
      required: ["subject"],
    },
    execute: async ({ subject }: { subject: string }) =>
      atlas(["callers", subject, "--limit", "60"]),
  },
  {
    name: "atlas_implementations",
    description: "Implementors of an interface (implements edges + heuristics).",
    parameters: {
      type: "object",
      properties: {
        subject: { type: "string", description: "Interface name e.g. IStorageProvider" },
      },
      required: ["subject"],
    },
    execute: async ({ subject }: { subject: string }) =>
      atlas(["implementations", subject]),
  },
  {
    name: "atlas_capabilities",
    description: "Infrastructure capabilities and product surfaces (storage, cache, …).",
    parameters: { type: "object", properties: {} },
    execute: async () => atlas(["capabilities"]),
  },
  {
    name: "atlas_code_search",
    description: "Definition-ranked structural code search.",
    parameters: {
      type: "object",
      properties: {
        query: { type: "string" },
      },
      required: ["query"],
    },
    execute: async ({ query }: { query: string }) =>
      atlas(["code-search", query, "--limit", "40"]),
  },
  {
    name: "atlas_map",
    description: "Repository orientation map (modules, coupling, hot files).",
    parameters: { type: "object", properties: {} },
    execute: async () => atlas(["map"]),
  },
];

export default {
  name: "atlas-tools",
  description: "Atlas local evidence engine tools (read-only)",
  tools,
};
