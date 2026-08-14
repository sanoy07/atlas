use anyhow::Result;
use atlas_core::{probe_ollama, OllamaConfig};
use atlas_storage::Store;
use chrono::{DateTime, Utc};
use std::process::Command;

const RST: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GRN: &str = "\x1b[32m";
const YLW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";

fn ok(s: &str) -> String {
    format!("{GRN}✓{RST} {s}")
}
fn warn(s: &str) -> String {
    format!("{YLW}!{RST} {s}")
}
fn bad(s: &str) -> String {
    format!("{RED}✗{RST} {s}")
}

/// Health check + daily-driver guidance (`atlas status`).
pub fn run() -> Result<()> {
    println!("{BOLD}Atlas{RST}  {DIM}developer knowledge engine · local-first{RST}");
    println!();

    // ── Binary / version ──────────────────────────────────────────────
    let version = env!("CARGO_PKG_VERSION");
    println!("{BOLD}TOOL{RST}");
    println!("  {}", ok(&format!("atlas {version}")));
    if let Ok(exe) = std::env::current_exe() {
        println!("  {DIM}binary{RST}  {}", exe.display());
    }

    // ── Repository / DB ───────────────────────────────────────────────
    println!();
    println!("{BOLD}REPOSITORY{RST}");
    let db_path = super::resolve_db_path();
    let repo = super::discover_repo_root().ok();
    match &repo {
        Some(r) => println!("  {}  {DIM}(git root){RST}", ok(r)),
        None => println!(
            "  {}",
            warn("not inside a git repository — path-scoped commands need a repo")
        ),
    }

    let store = match Store::open(&db_path) {
        Ok(s) => {
            let exists = std::path::Path::new(&db_path).exists();
            if exists {
                println!("  {}  {DIM}{db_path}{RST}", ok("SQLite connected"));
            } else {
                println!(
                    "  {}  {DIM}{db_path} (will be created on ingest){RST}",
                    warn("SQLite path ready")
                );
            }
            Some(s)
        }
        Err(e) => {
            println!("  {}", bad(&format!("SQLite: {e}")));
            None
        }
    };

    let git_ok = Command::new("git").arg("--version").output().is_ok();
    println!(
        "  {}",
        if git_ok {
            ok("git ready")
        } else {
            bad("git not found")
        }
    );

    let gh_ok = Command::new("gh").arg("--version").output().is_ok();
    println!(
        "  {}",
        if gh_ok {
            ok("gh ready (GitHub ingest available)")
        } else {
            warn("gh not found — `atlas ingest --github` needs the GitHub CLI or a token")
        }
    );

    // ── Ingest snapshot ───────────────────────────────────────────────
    if let (Some(store), Some(repo)) = (store.as_ref(), repo.as_ref()) {
        // Project ingest and CLI ingest may store slightly different path forms;
        // try realpath, then non-canonical, then prefix match via SQL-less fallbacks.
        let candidates = {
            let mut v = vec![repo.clone()];
            if let Ok(c) = std::fs::canonicalize(repo) {
                let s = c.to_string_lossy().into_owned();
                if s != *repo {
                    v.push(s);
                }
            }
            // trailing slash variants
            if repo.ends_with('/') {
                v.push(repo.trim_end_matches('/').to_string());
            } else {
                v.push(format!("{repo}/"));
            }
            v
        };
        let mut found = None;
        let mut matched_repo = repo.clone();
        for c in &candidates {
            if let Ok(Some(run)) = store.latest_ingest_run(c) {
                found = Some(run);
                matched_repo = c.clone();
                break;
            }
        }

        if let Some(run) = found {
            println!();
            println!("{BOLD}LAST INGEST{RST}");
            println!("  {DIM}started{RST}   {}", fmt_ts(run.started_at));
            if let Some(t) = run.ended_at {
                println!("  {DIM}ended{RST}     {}", fmt_ts(t));
            }
            println!("  {DIM}version{RST}   atlas {}", run.atlas_version);
            if let Some(h) = &run.git_head {
                let short: String = h.chars().take(7).collect();
                println!(
                    "  {DIM}HEAD{RST}      {}{}",
                    short,
                    run.git_branch
                        .as_ref()
                        .map(|b| format!("  ({b})"))
                        .unwrap_or_default()
                );
            }
            println!("  {DIM}scope{RST}     {}", run.requested_scope);
            let status_line = match run.exit_status.as_str() {
                "ok" | "success" => ok(&run.exit_status),
                other if other.contains("fail") => bad(other),
                other => warn(other),
            };
            println!("  {DIM}status{RST}    {status_line}");

            // Does that snapshot still describe the working tree?
            if let Ok(f) = atlas_core::compute_freshness(&matched_repo, store) {
                let line = match &f.freshness {
                    atlas_core::Freshness::Current { .. } => ok("current with HEAD"),
                    atlas_core::Freshness::Stale { commits_behind: Some(n), .. } => {
                        warn(&format!("{n} commit(s) behind HEAD — re-run `atlas ingest . --typescript`"))
                    }
                    atlas_core::Freshness::Stale { commits_behind: None, .. } => warn(
                        "ingested commit unreachable (rebase or different clone) — re-run `atlas ingest . --typescript`",
                    ),
                    atlas_core::Freshness::NeverIngested => warn("no ingest recorded"),
                    atlas_core::Freshness::Unknown { reason } => warn(reason),
                };
                println!("  {DIM}freshness{RST} {line}");
            }

            render_stages_summary(&run.stages_json);
        } else {
            println!();
            println!("{BOLD}LAST INGEST{RST}");
            println!(
                "  {}",
                warn("no ingest yet — run:  atlas ingest . --typescript")
            );
            println!("  {DIM}optional:{RST} atlas ingest . --typescript --github");
            println!(
                "  {DIM}multi-repo:{RST} atlas project ingest <name> --typescript  (set ATLAS_DB)"
            );
        }
    }

    // ── Local AI ──────────────────────────────────────────────────────
    let cfg = OllamaConfig::from_env();
    let probe = probe_ollama(&cfg);

    println!();
    println!("{BOLD}LOCAL AI{RST}  {DIM}(Ollama · never cloud by default){RST}");
    println!("  {DIM}url{RST}           {}", cfg.base_url);
    println!(
        "  {DIM}reasoning{RST}     {}  {DIM}num_ctx={}  predict={}  timeout={}s{RST}",
        cfg.reasoning_model, cfg.num_ctx, cfg.reasoning_num_predict, cfg.timeout_secs
    );
    println!(
        "  {DIM}synthesis{RST}     {}  {DIM}predict={}  timeout={}s{RST}",
        cfg.synthesis_model, cfg.synthesis_num_predict, cfg.timeout_secs
    );

    if probe.reachable {
        println!("  {}", ok("Ollama reachable"));
        if probe.has_reasoning {
            println!("  {}", ok(&format!("reasoning model present ({})", cfg.reasoning_model)));
        } else {
            println!(
                "  {}",
                bad(&format!(
                    "missing reasoning model — run:  ollama pull {}",
                    cfg.reasoning_model
                ))
            );
        }
        if probe.has_synthesis {
            println!(
                "  {}",
                ok(&format!("synthesis model present ({})", cfg.synthesis_model))
            );
        } else {
            println!(
                "  {}",
                warn(&format!(
                    "missing synthesis model — run:  ollama pull {}",
                    cfg.synthesis_model
                ))
            );
        }
        if !probe.models.is_empty() {
            let shown: Vec<_> = probe.models.iter().take(8).map(|s| s.as_str()).collect();
            println!(
                "  {DIM}installed{RST}     {}{}",
                shown.join(", "),
                if probe.models.len() > 8 {
                    format!(" +{} more", probe.models.len() - 8)
                } else {
                    String::new()
                }
            );
        }
    } else {
        println!(
            "  {}",
            bad(probe
                .error
                .as_deref()
                .unwrap_or("Ollama not reachable"))
        );
        println!("  {DIM}start with:{RST}  ollama serve");
        println!(
            "  {DIM}then pull:{RST}   ollama pull {} && ollama pull {}",
            cfg.reasoning_model, cfg.synthesis_model
        );
    }

    // ── Daily workflow ────────────────────────────────────────────────
    println!();
    println!("{BOLD}DAILY WORKFLOW{RST}");
    println!("  {CYAN}1.{RST}  atlas ingest . --typescript          {DIM}# refresh evidence{RST}");
    println!("  {CYAN}2.{RST}  atlas map                            {DIM}# orient in the repo{RST}");
    println!("  {CYAN}3.{RST}  atlas investigate \"your question\"  {DIM}# evidence + verified reasoning{RST}");
    println!("  {CYAN}4.{RST}  atlas agent \"where is X?\"          {DIM}# tool loop: Atlas+rg+web (read-only){RST}");
    println!("  {CYAN}5.{RST}  atlas investigate topic --no-ai      {DIM}# packet only, no model{RST}");
    println!("  {CYAN}6.{RST}  atlas focus / impact / conventions   {DIM}# neighborhood · blast radius · peers{RST}");
    println!("  {CYAN}7.{RST}  atlas plan 42                        {DIM}# issue → human checklist{RST}");
    println!();
    println!("{DIM}Atlas owns facts. Agent only selects tools + synthesizes. You implement.{RST}");
    println!("{DIM}Env: ATLAS_DB  ATLAS_OLLAMA_*  AGENT_MODEL  ATLAS_AGENT_WEB  ATLAS_AGENT_SCRIPT{RST}");
    println!();

    Ok(())
}

fn fmt_ts(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn render_stages_summary(stages_json: &str) {
    let Ok(stages): std::result::Result<Vec<serde_json::Value>, _> =
        serde_json::from_str(stages_json)
    else {
        return;
    };
    if stages.is_empty() {
        return;
    }
    let mut ok_n = 0;
    let mut fail = 0;
    let mut skipped = 0;
    for s in &stages {
        match s.get("status").and_then(|v| v.as_str()) {
            Some("ok") => ok_n += 1,
            Some("failed") => fail += 1,
            Some("skipped") => skipped += 1,
            _ => {}
        }
    }
    println!(
        "  {DIM}stages{RST}    {ok_n} ok · {fail} failed · {skipped} skipped"
    );
    if fail > 0 {
        for s in &stages {
            if s.get("status").and_then(|v| v.as_str()) == Some("failed") {
                let name = s.get("stage").and_then(|v| v.as_str()).unwrap_or("?");
                let detail = s.get("detail").and_then(|v| v.as_str()).unwrap_or("");
                println!("             {RED}✗{RST} {name}: {detail}");
            }
        }
    }
}
