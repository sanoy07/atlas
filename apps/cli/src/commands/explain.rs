use anyhow::Result;
use atlas_storage::Store;

pub fn run(file: &str) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let commits = store.commits_for_file(file, ".")?;
    let prs = store.prs_for_file(file, ".")?;

    println!("=== {} ===\n", file);

    println!("Touch history ({} commits):\n", commits.len());
    for c in &commits {
        println!("  {} — {} ({})", c.short_hash, c.message, c.author_name);
    }

    if !prs.is_empty() {
        println!("\nPull requests that touched this file:\n");
        for pr in &prs {
            println!("  #{} [{}] {} — {}", pr.number, pr.state, pr.title, pr.author);
        }
    }

    if commits.is_empty() && prs.is_empty() {
        println!("No data found. Run `atlas ingest .` first.");
        println!("For PR linkage: `atlas ingest --github .`");
    }

    Ok(())
}
