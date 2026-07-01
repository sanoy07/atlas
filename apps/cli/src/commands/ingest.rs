use anyhow::Result;
use atlas_core;
use atlas_storage::Store;

pub fn run(path: &str, github: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    print!("Ingesting git history from {} … ", path);
    let count = atlas_core::ingest_git(path, &store)?;
    println!("{} commits", count);

    if github {
        print!("Fetching GitHub PRs … ");
        let pr_count = atlas_core::ingest_github(path, &store)?;
        println!("{} PRs", pr_count);
    }

    println!("Done.");
    Ok(())
}
