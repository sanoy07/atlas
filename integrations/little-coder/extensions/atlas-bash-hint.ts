/**
 * Optional little-coder / pi extension sketch.
 *
 * little-coder loads TypeScript extensions from ~/.config/little-coder/extensions/
 * or LITTLE_CODER_EXTRA_EXTENSIONS. This file is a *template* — wire it only if you
 * run little-coder against a repo with Atlas installed.
 *
 * It does NOT reimplement Atlas. It only:
 *  - reminds the model (via system/context injection hooks if available)
 *  - documents that `atlas` should be on LITTLE_CODER_BASH_ALLOW
 *
 * Full tool registration should shell to the Atlas CLI (same as atlas_agent.py).
 *
 * Install sketch:
 *   cp atlas-bash-hint.ts ~/.config/little-coder/extensions/
 *   export LITTLE_CODER_BASH_ALLOW="atlas "
 *
 * Prefer skills/atlas-evidence.md for day-one use; this extension is optional.
 */

export const ATLAS_HINT = `
Repository evidence: if the 'atlas' CLI is available, prefer:
  atlas investigate "…" --no-ai
  atlas callers <symbol>
  atlas capabilities
  atlas implementations <Interface>
  atlas code-search <symbol>
over inventing architecture from tests or fs.writeFile.
`;

// pi extension entry would register hooks here — kept as documentation until
// a live little-coder install is validated on this machine.
export default {
  name: "atlas-bash-hint",
  description: "Prefer Atlas CLI for repository structure questions",
  hint: ATLAS_HINT,
};
