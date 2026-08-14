pub mod agent;
pub mod anomalies;
pub mod authors;
pub mod callers;
pub mod capabilities;
pub mod code_search;
pub mod focus_cmd;
pub mod impact_cmd;
pub mod implementations;
pub mod map_cmd;
pub mod campaign;
pub mod cohorts;
pub mod config_cmd;
pub mod conventions;
pub mod coupling;
pub mod deps;
pub mod eval;
pub mod investigations;
pub mod cochanges;
pub mod context;
pub mod feedback;
pub mod explain;
pub mod hotfiles;
pub mod ingest;
pub mod init;
pub mod inspect;
pub mod investigate;
pub mod modules;
pub mod plan;
pub mod project;
pub mod query;
pub mod review_context;
pub mod search;
pub mod show;
pub mod status;
pub mod structural;
pub mod tests_cmd;
pub mod timeline;
pub mod tree;
pub mod whenintroduced;

/// Print to stdout, ignoring broken-pipe (EPIPE) so `atlas … | head` never panics.
#[macro_export]
macro_rules! atlas_println {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        match writeln!(out, $($arg)*) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }};
}

/// Resolve `path` to a canonical absolute path so all repo_path values stored
/// in the DB are comparable regardless of how the caller referenced the directory.
/// Falls back to the input string if canonicalization fails (e.g. path does not exist).
pub fn canonical_repo_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Resolve a user-supplied path to its current canonical path via FileIdentity.
///
/// If `path` is a historical address (was renamed away), returns the current
/// path and prints a one-line notice to the user.  Otherwise returns `path`
/// unchanged and prints nothing.
///
/// P1-10: path-scoped read commands were silently returning zero results on
/// renamed files.  This helper is called at the top of every file-scoped
/// CLI command so users get either the redirect or a clear "no chain" answer.
pub fn resolve_and_notify_historical(
    store: &atlas_storage::Store,
    path: &str,
    repo: &str,
) -> String {
    match store.current_path_if_historical(path, repo) {
        Ok(Some(current)) => {
            eprintln!(
                "note: `{}` is a historical path; showing results for the current path `{}`.\n\
                 (Atlas file identity tracked this rename.  Use `atlas context {}` to see the full lifetime.)",
                path, current, path
            );
            current
        }
        _ => path.to_string(),
    }
}

/// Resolve the modules parent path for B5/B6-style commands.
///
/// When the user passes the historical default `src/modules` (or `auto` / empty)
/// and that yields zero modules, fall back to the same layout heuristic as
/// `atlas map` (`resolve_modules_subject`). Returns (path, note_if_auto).
pub fn resolve_modules_path_for_cli(
    requested: &str,
    repo: &str,
    store: &atlas_storage::Store,
) -> anyhow::Result<(String, Option<String>)> {
    use atlas_core::{compute_modules, resolve_modules_subject};

    let req = requested.trim();
    let use_auto = req.is_empty() || req == "auto" || req == "src/modules";
    if !use_auto {
        return Ok((req.to_string(), None));
    }

    // Prefer explicit src/modules when it has children.
    if req == "src/modules" || req.is_empty() || req == "auto" {
        if let Ok(r) = compute_modules("src/modules", repo, store) {
            if r.total_modules > 0 {
                return Ok(("src/modules".into(), None));
            }
        }
    }

    let auto = resolve_modules_subject(repo, store)?;
    if auto != "src/modules" {
        let note = format!(
            "note: no modules under `src/modules`; auto-resolved subject `{}` (same heuristic as `atlas map`).",
            auto
        );
        return Ok((auto, Some(note)));
    }
    Ok((auto, None))
}

/// Filename of the per-repository evidence database, stored at the repo root.
pub const DB_FILENAME: &str = "atlas.db";

/// Walk upward from `start` looking for `.git`, returning the canonical
/// repository root.
///
/// `.git` is a directory in a normal clone but a *file* in worktrees and
/// submodules, so this tests existence rather than directory-ness.
fn find_repo_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return std::fs::canonicalize(dir).ok();
        }
        dir = dir.parent()?;
    }
}

/// Resolve the SQLite DB path for the repository containing `start`.
///
/// Resolution order:
///   1. `ATLAS_DB` — explicit override, used verbatim (multi-repo eval DBs
///      and the benchmark harness depend on this winning outright).
///   2. `<git root>/atlas.db` — the repository owns its evidence.
///   3. `./atlas.db` — not inside a Git repo; preserves the old behaviour.
///
/// Prior behaviour was cwd-relative `./atlas.db` unconditionally, so running
/// any read command from a subdirectory opened a *different* (empty) database
/// and reported "no history found" rather than the evidence that had actually
/// been ingested.  Anchoring to the repo root makes `atlas` position-
/// independent the way `git` is; at the repo root the resolved path is
/// identical to the old one, so existing databases keep working.
pub fn resolve_db_path_from(start: &std::path::Path) -> String {
    resolve_db_path_inner(start, true)
}

/// Same resolution, without the "no database yet" warning — for `ingest` and
/// `init`, whose whole job is to create the database being reported missing.
pub fn resolve_db_path_for_write(start: &std::path::Path) -> String {
    resolve_db_path_inner(start, false)
}

fn resolve_db_path_inner(start: &std::path::Path, warn_if_missing: bool) -> String {
    if let Some(explicit) = std::env::var_os("ATLAS_DB") {
        let db_path = explicit.to_string_lossy().into_owned();
        if warn_if_missing && !std::path::Path::new(&db_path).exists() {
            eprintln!(
                "warning: ATLAS_DB {} does not exist; a fresh database will be created.\n\
                 If you meant to query an existing DB, check the path or run `atlas ingest .` first.",
                &db_path
            );
        }
        return db_path;
    }

    let db_path = match find_repo_root(start) {
        Some(root) => root.join(DB_FILENAME),
        None => std::path::PathBuf::from(DB_FILENAME),
    };

    if warn_if_missing && !db_path.exists() {
        eprintln!(
            "warning: no Atlas database at {}; a fresh one will be created.\n\
             Run `atlas ingest .` to build the evidence graph first.",
            db_path.display()
        );
    }
    db_path.to_string_lossy().into_owned()
}

/// Resolve the DB path for the current working directory.  See
/// [`resolve_db_path_from`] for the resolution order.
pub fn resolve_db_path() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    resolve_db_path_from(&cwd)
}

/// Discover the Git repository root by walking upward from the current directory.
///
/// Returns the canonical absolute path to the repository root, matching the
/// path stored in the database during `atlas ingest`.  Fails with a clear
/// message when no `.git` directory is found, rather than silently using the
/// current directory as the repo identity (which would produce empty results
/// instead of an actionable error).
pub fn discover_repo_root() -> anyhow::Result<String> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("cannot determine current directory: {}", e))?;

    match find_repo_root(&cwd) {
        Some(root) => Ok(root.to_string_lossy().into_owned()),
        None => anyhow::bail!(
            "not inside a Git repository (no .git found in {} or any parent directory)",
            cwd.display()
        ),
    }
}
