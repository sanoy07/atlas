# Atlas daily-driver helpers for Fish
# source from config.fish or NixOS fish.nix:
#   source ~/projects/atlas/shell/atlas.fish

# Prefer release install, fall back to cargo target
if test -x "$HOME/.local/bin/atlas"
    set -gx ATLAS_BIN "$HOME/.local/bin/atlas"
else if test -x "$HOME/projects/atlas/target/release/atlas"
    set -gx ATLAS_BIN "$HOME/projects/atlas/target/release/atlas"
else
    set -gx ATLAS_BIN atlas
end

# Local AI defaults (6GB GPU). Override in ~/.config/fish/conf.d/ if needed.
set -q ATLAS_OLLAMA_MODEL; or set -gx ATLAS_OLLAMA_MODEL qwen3:4b
set -q ATLAS_OLLAMA_SYNTHESIS_MODEL; or set -gx ATLAS_OLLAMA_SYNTHESIS_MODEL qwen2.5-coder:7b-instruct
set -q ATLAS_OLLAMA_NUM_CTX; or set -gx ATLAS_OLLAMA_NUM_CTX 12288
set -q ATLAS_OLLAMA_URL; or set -gx ATLAS_OLLAMA_URL http://localhost:11434
# Agent (tool loop)
set -q AGENT_MODEL; or set -gx AGENT_MODEL qwen3:4b
set -q AGENT_NUM_CTX; or set -gx AGENT_NUM_CTX 12288
set -q ATLAS_AGENT_SCRIPT; or set -gx ATLAS_AGENT_SCRIPT $HOME/projects/atlas/agent/atlas_agent.py
# Thinking off by default (faster on 6GB); set 1 for CoT
set -q ATLAS_AGENT_THINK; or set -gx ATLAS_AGENT_THINK 0
set -q ATLAS_AGENT_WEB; or set -gx ATLAS_AGENT_WEB 0
# Multi-repo RWATP evidence (optional — comment out for single-repo atlas.db)
# set -q ATLAS_DB; or set -gx ATLAS_DB $HOME/Vesta/rwatp-atlas.db

function atlas --wraps atlas --description 'Atlas knowledge engine'
    command $ATLAS_BIN $argv
end

function astatus --description 'Atlas health / doctor'
    command $ATLAS_BIN status $argv
end

function amap --description 'Atlas repository map'
    command $ATLAS_BIN map $argv
end

function aing --description 'Atlas ingest with TypeScript structure'
    command $ATLAS_BIN ingest . --typescript $argv
end

function aingh --description 'Atlas ingest + TypeScript + GitHub'
    command $ATLAS_BIN ingest . --typescript --github $argv
end

function ai --description 'Atlas investigate (evidence + optional local AI)'
    command $ATLAS_BIN investigate $argv
end

function airaw --description 'Atlas investigate without AI'
    command $ATLAS_BIN investigate --no-ai $argv
end

function afocus --description 'Atlas focus neighborhood'
    command $ATLAS_BIN focus $argv
end

function aimpact --description 'Atlas impact / blast radius'
    command $ATLAS_BIN impact $argv
end

function aconv --description 'Atlas conventions / peer patterns'
    command $ATLAS_BIN conventions $argv
end

function aplan --description 'Atlas plan from GitHub issue number'
    command $ATLAS_BIN plan $argv
end

function aagent --description 'Atlas agent: Ollama tool loop (read-only Atlas+rg+web)'
    command $ATLAS_BIN agent $argv
end
