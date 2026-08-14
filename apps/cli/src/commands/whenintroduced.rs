use anyhow::Result;
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(file: &str) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;

    let repo = super::discover_repo_root()?;
    let file = super::resolve_and_notify_historical(&store, file, &repo);
    match store.first_seen(&file, &repo)? {
        None => {
            println!("'{}' not found in history.", &file);
            println!("Hint: run `atlas ingest .` first.");
        }
        Some(commit) => {
            let date = DateTime::<Utc>::from_timestamp(commit.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            println!("{} was first introduced in:\n", &file);
            println!("  {}  {}  {}  ({})",
                commit.short_hash, date, commit.message, commit.author_name);
        }
    }
    Ok(())
}
