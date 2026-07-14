use anyhow::Result;
use atlas_storage::Store;

pub fn run(file: &str) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let repo = super::discover_repo_root()?;
    let commits = store.commits_for_file(file, &repo)?;

    if commits.is_empty() {
        println!("No commits found for '{}'", file);
        println!("Hint: run `atlas ingest .` first.");
        return Ok(());
    }

    println!("Commits that modified {}:\n", file);
    for c in &commits {
        println!("  {} — {}", c.short_hash, c.message);
        println!("       {}", c.author_name);
    }
    println!("\n{} commits total.", commits.len());
    Ok(())
}
