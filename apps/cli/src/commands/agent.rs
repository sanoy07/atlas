//! `atlas agent` — local Ollama tool loop over Atlas evidence.
//!
//! Shells out to `agent/atlas_agent.py` (read-only tools: Atlas, ripgrep,
//! web_search/fetch). The model never writes the knowledge graph.

use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Resolve path to atlas_agent.py.
fn find_agent_script() -> Result<PathBuf> {
    if let Ok(p) = env::var("ATLAS_AGENT_SCRIPT") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        }
        bail!("ATLAS_AGENT_SCRIPT set but not a file: {p}");
    }

    // Walk up from CWD looking for agent/atlas_agent.py (dev tree).
    if let Ok(cwd) = env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            let cand = dir.join("agent/atlas_agent.py");
            if cand.is_file() {
                return Ok(cand);
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => break,
            }
        }
    }

    // Common install / home layout for this machine.
    let home = env::var_os("HOME").map(PathBuf::from);
    if let Some(h) = home {
        let cand = h.join("projects/atlas/agent/atlas_agent.py");
        if cand.is_file() {
            return Ok(cand);
        }
    }

    bail!(
        "could not find agent/atlas_agent.py\n\
         Set ATLAS_AGENT_SCRIPT=/path/to/atlas_agent.py\n\
         or run from the Atlas git checkout."
    )
}

fn find_python() -> Result<PathBuf> {
    if let Ok(p) = env::var("ATLAS_AGENT_PYTHON") {
        let pb = PathBuf::from(&p);
        if pb.is_file() || which_ok(&p) {
            return Ok(pb);
        }
    }
    for name in ["python3", "python"] {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Ok(PathBuf::from(s));
                }
            }
        }
    }
    // NixOS without python on PATH: resolve once via nix-shell (cached by store path).
    if which_ok("nix-shell") {
        if let Ok(out) = Command::new("nix-shell")
            .args(["-p", "python3", "--run", "which python3"])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() && Path::new(&s).is_file() {
                    eprintln!("note: using python from nix-shell: {s}");
                    return Ok(PathBuf::from(s));
                }
            }
        }
    }
    bail!(
        "python3 not found on PATH.\n\
         Install Python 3, or set ATLAS_AGENT_PYTHON, e.g.:\n\
           export ATLAS_AGENT_PYTHON=$(nix-shell -p python3 --run 'which python3')"
    )
}

fn which_ok(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn run(
    question: &[String],
    repo: Option<&str>,
    max_steps: u32,
    show_thinking: bool,
    fast: bool,
    no_web: bool,
) -> Result<()> {
    if question.is_empty() {
        bail!("provide a question, e.g. atlas agent \"where is order fulfillment?\"");
    }
    let q = question.join(" ");
    let script = find_agent_script()?;
    let python = find_python()?;

    let repo_path = match repo {
        Some(r) => super::canonical_repo_path(r),
        None => super::discover_repo_root()?,
    };

    // Prefer current ATLAS_DB; else repo-local atlas.db if present.
    if env::var_os("ATLAS_DB").is_none() {
        let local = Path::new(&repo_path).join("atlas.db");
        if local.is_file() {
            env::set_var("ATLAS_DB", &local);
        }
    }

    // Point agent at this atlas binary when possible.
    if env::var_os("ATLAS_BIN").is_none() {
        if let Ok(exe) = env::current_exe() {
            env::set_var("ATLAS_BIN", exe);
        }
    }

    if no_web {
        env::set_var("ATLAS_AGENT_WEB", "0");
    }

    let mut cmd = Command::new(&python);
    cmd.arg(&script).arg(&q).arg("--repo").arg(&repo_path);
    if max_steps > 0 {
        cmd.arg("--max-steps").arg(max_steps.to_string());
    }
    if show_thinking {
        cmd.arg("--show-thinking");
    }
    if fast {
        cmd.arg("--fast");
    }
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    eprintln!(
        "atlas agent → {}  model={}  repo={}",
        script.display(),
        env::var("AGENT_MODEL").unwrap_or_else(|_| "qwen3:4b".into()),
        repo_path
    );

    let status = cmd
        .status()
        .with_context(|| format!("failed to spawn {} {}", python.display(), script.display()))?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        bail!("agent exited with status {code}");
    }
    Ok(())
}
