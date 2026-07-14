use anyhow::Result;
use atlas_storage::Store;

pub fn run(file: &str, min_count: i64) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let repo = super::discover_repo_root()?;
    let co = store.co_changes_for_file(file, &repo, min_count)?;

    if co.is_empty() {
        if min_count > 1 {
            println!("No co-changed files with at least {min_count} shared commits for '{file}'.");
            println!("Try a lower --min-count, or run `atlas ingest .` first.");
        } else {
            println!("No co-change data for '{}'.", file);
            println!("Hint: run `atlas ingest .` first.");
        }
        return Ok(());
    }

    println!("Files that co-change with {} (min {} shared commit{}):\n",
        file,
        min_count,
        if min_count == 1 { "" } else { "s" }
    );
    for row in &co {
        println!("  {:>4}×  {}", row.change_count, row.file_path);
    }
    println!("\n{} co-changed file(s) found.", co.len());
    Ok(())
}
