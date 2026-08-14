#!/usr/bin/env bash
# Wrapper so `atlas agent` works on NixOS without python on PATH.
set -euo pipefail
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
AGENT="${ATLAS_AGENT_SCRIPT:-$SCRIPT_DIR/atlas_agent.py}"
if command -v python3 >/dev/null 2>&1; then
  exec python3 "$AGENT" "$@"
fi
if command -v nix-shell >/dev/null 2>&1; then
  exec nix-shell -p python3 --run "python3 $(printf %q "$AGENT") $(printf '%q ' "$@")"
fi
echo "python3 not found; install Python 3 or use nix-shell -p python3" >&2
exit 1
