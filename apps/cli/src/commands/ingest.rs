use anyhow::Result;
use atlas_core;
use atlas_git::{GitRepo, GitScope};
use atlas_storage::Store;
use serde::Serialize;
use std::time::Instant;

const ATLAS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-stage record persisted into `ingest_runs.stages_json`.
#[derive(Debug, Clone, Serialize)]
struct StageRecord {
    stage:       String,
    status:      String, // "ok" | "failed" | "skipped"
    detail:      String, // count message on ok, error message on failed
    duration_ms: u128,
}

/// A single stage function plus its display name.  Each stage returns a
/// human-readable success detail on Ok, or an error on Err.  The runner
/// wraps each in `with_transaction` so mid-stage failures roll back.
struct Stage {
    name:  &'static str,
    /// `run` returns Ok(detail) or Err(reason).  `enabled` gates skipping.
    run:   Box<dyn Fn(&Store) -> Result<String>>,
    enabled: bool,
}

pub fn run(path: &str, github: bool, typescript: bool, all_refs: bool) -> Result<()> {
    let repo    = super::canonical_repo_path(path);
    // Anchor the DB to the repository being ingested, not the cwd, so that
    // `atlas ingest` from a subdirectory writes where read commands look.
    let db_path = super::resolve_db_path_for_write(std::path::Path::new(&repo));
    let store   = Store::open(&db_path)?;

    // Discover repository state up-front so ingest_runs records it even if
    // a subsequent stage fails.
    let git_repo   = GitRepo::open(&repo).ok();
    let git_head   = git_repo.as_ref().and_then(|g| g.head_commit());
    let git_branch = git_repo.as_ref().and_then(|g| g.current_branch());
    let scope      = if all_refs { GitScope::AllRefs } else { GitScope::HeadOnly };

    let run_id = store.start_ingest_run(
        &repo,
        ATLAS_VERSION,
        git_head.as_deref(),
        git_branch.as_deref(),
        scope.as_str(),
    )?;

    // Build the stage list.  Extractors are auto-detected the same way the
    // previous implementation did; the only new behaviour is per-stage
    // transactions plus success/failure reporting.
    let has_c      = atlas_core::repo_has_c_files(&repo);
    let has_rust   = atlas_core::repo_has_rust_files(&repo);
    let has_python = atlas_core::repo_has_python_files(&repo);
    // TypeScript is the most developed extractor but was the only one gated
    // behind a flag, so `atlas ingest .` silently produced no structural edges
    // on the repos it serves best.  `--typescript` now only forces it on.
    let has_ts     = typescript || atlas_core::repo_has_typescript_files(&repo);

    let repo_g = repo.clone();
    let repo_r = repo.clone();
    let repo_i = repo.clone();
    let repo_h = repo.clone();
    let repo_p = repo.clone();
    let repo_c = repo.clone();
    let repo_ru = repo.clone();
    let repo_py = repo.clone();
    let repo_d = repo.clone();
    let repo_l = repo.clone();

    let stages: Vec<Stage> = vec![
        Stage {
            name:    "git history",
            enabled: true,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_git_scoped(&repo_g, s, scope)?;
                Ok(format!("{} commits", n))
            }),
        },
        Stage {
            name:    "rename evidence",
            enabled: true,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_rename_evidence(&repo_r, s)?;
                Ok(format!("{} rename records", n))
            }),
        },
        Stage {
            name:    "file identities",
            enabled: true,
            run: Box::new(move |s| {
                let n = atlas_core::rebuild_file_identities(&repo_i, s)?;
                Ok(format!("{} identities", n))
            }),
        },
        Stage {
            name:    "github",
            enabled: github,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_github(&repo_h, s)?;
                Ok(format!("{} PRs", n))
            }),
        },
        Stage {
            name:    "typescript structural",
            enabled: has_ts,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_typescript(&repo_p, s)?;
                Ok(format!("{} edges", n))
            }),
        },
        Stage {
            name:    "c/cpp structural",
            enabled: has_c,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_c(&repo_c, s)?;
                Ok(format!("{} edges", n))
            }),
        },
        Stage {
            name:    "rust structural",
            enabled: has_rust,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_rust(&repo_ru, s)?;
                Ok(format!("{} edges", n))
            }),
        },
        Stage {
            name:    "python structural",
            enabled: has_python,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_python(&repo_py, s)?;
                Ok(format!("{} edges", n))
            }),
        },
        Stage {
            name:    "documents",
            enabled: true,
            run: Box::new(move |s| {
                let n = atlas_core::ingest_documents(&repo_d, s)?;
                Ok(format!("{} documents", n))
            }),
        },
        Stage {
            name:    "lexicon",
            enabled: true,
            run: Box::new(move |s| {
                let n = atlas_core::build_lexicon(&repo_l, s)?;
                Ok(format!("{} vocabulary relationships", n))
            }),
        },
        Stage {
            name:    "configuration artifacts",
            enabled: true,
            run: Box::new({
                let repo = repo.clone();
                move |s| {
                    let n = atlas_core::ingest_configuration_artifacts(&repo, s)?;
                    Ok(format!("{} artifacts", n))
                }
            }),
        },
        Stage {
            name:    "profile claims",
            enabled: true,
            run: Box::new({
                let repo = repo.clone();
                move |s| {
                    let n = atlas_core::ingest_profile_claims(&repo, s)?;
                    Ok(format!("{} claims", n))
                }
            }),
        },
        Stage {
            name:    "analysis status",
            enabled: true,
            run: Box::new({
                let repo = repo.clone();
                move |s| {
                    let n = atlas_core::stamp_analysis_status(&repo, s)?;
                    Ok(format!("{} files classified", n))
                }
            }),
        },
    ];

    let total: usize = stages.iter().filter(|s| s.enabled).count();
    println!("Ingesting {} (scope: {})", repo, scope.as_str());
    println!();

    let mut records: Vec<StageRecord> = Vec::new();
    let mut idx = 0;
    let mut ok_count = 0usize;
    let mut fail_count = 0usize;

    for stage in &stages {
        if !stage.enabled {
            records.push(StageRecord {
                stage:       stage.name.to_string(),
                status:      "skipped".to_string(),
                detail:      "not enabled".to_string(),
                duration_ms: 0,
            });
            continue;
        }
        idx += 1;
        let started = Instant::now();
        // Each stage is its own atomic transaction: mid-stage failure rolls back
        // that stage's rows.  Failure of one stage does not skip later stages.
        let outcome = store.with_transaction(|s| (stage.run)(s));
        let elapsed = started.elapsed().as_millis();
        match outcome {
            Ok(detail) => {
                println!("[{:>2}/{:<2}] {:<24} ✓ {}", idx, total, stage.name, detail);
                records.push(StageRecord {
                    stage: stage.name.to_string(), status: "ok".to_string(),
                    detail, duration_ms: elapsed,
                });
                ok_count += 1;
            }
            Err(e) => {
                let reason = format!("{}", e);
                println!("[{:>2}/{:<2}] {:<24} ✗ {}", idx, total, stage.name, reason);
                records.push(StageRecord {
                    stage: stage.name.to_string(), status: "failed".to_string(),
                    detail: reason, duration_ms: elapsed,
                });
                fail_count += 1;
            }
        }
    }

    let exit_status = if fail_count == 0 {
        "ok"
    } else if ok_count == 0 {
        "failed"
    } else {
        "partial"
    };
    let stages_json = serde_json::to_string(&records).unwrap_or_else(|_| "[]".to_string());
    store.finish_ingest_run(run_id, exit_status, &stages_json, "[]")?;

    println!();
    println!("Ingest complete: {} succeeded, {} failed, {} skipped.",
        ok_count, fail_count, total.saturating_sub(ok_count + fail_count));
    match exit_status {
        "ok"      => Ok(()),
        _         => anyhow::bail!("{} of {} stages failed; see `atlas status` for details", fail_count, total),
    }
}
