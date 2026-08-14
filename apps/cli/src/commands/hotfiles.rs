use anyhow::Result;
use atlas_storage::Store;

pub fn run(limit: i64, repo_override: Option<&str>) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;

    let repo = match repo_override {
        Some(r) => super::canonical_repo_path(r),
        None    => super::discover_repo_root()?,
    };

    // Identity-aware aggregation.  If a file was renamed, its pre- and
    // post-rename commits collapse into a single entry keyed on the current
    // canonical path (not two rows under two paths).  Files without an
    // identity chain fall back to path-scoped counting.
    let identity_aware = store.has_materialized_identities(&repo)?;
    let files = if identity_aware {
        store.hot_files_identity_aware(&repo, limit)?
    } else {
        store.hot_files(&repo, limit)?
    };

    if files.is_empty() {
        println!("No file history found. Run `atlas ingest .` first.");
        return Ok(());
    }

    println!("Most frequently modified files:");
    if identity_aware {
        println!("  (counts span rename history — one entry per file identity, keyed on current path)");
    }
    println!();
    for row in &files {
        println!("  {:>4}×  {}", row.touch_count, row.file_path);
    }
    println!("\n{} file(s) shown.", files.len());
    Ok(())
}
