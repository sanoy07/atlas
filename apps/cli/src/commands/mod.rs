pub mod cochanges;
pub mod context;
pub mod feedback;
pub mod explain;
pub mod hotfiles;
pub mod ingest;
pub mod investigate;
pub mod query;
pub mod review_context;
pub mod search;
pub mod status;
pub mod structural;
pub mod timeline;
pub mod whenintroduced;

/// Resolve `path` to a canonical absolute path so all repo_path values stored
/// in the DB are comparable regardless of how the caller referenced the directory.
/// Falls back to the input string if canonicalization fails (e.g. path does not exist).
pub fn canonical_repo_path(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
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

    let mut dir = cwd.as_path();
    loop {
        if dir.join(".git").exists() {
            return std::fs::canonicalize(dir)
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| anyhow::anyhow!("cannot canonicalize repo root: {}", e));
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => anyhow::bail!(
                "not inside a Git repository (no .git found in {} or any parent directory)",
                cwd.display()
            ),
        }
    }
}
