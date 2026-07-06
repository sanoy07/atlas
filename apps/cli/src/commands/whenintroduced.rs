use anyhow::Result;
use atlas_storage::Store;
use chrono::{DateTime, Utc};

pub fn run(file: &str) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    match store.first_seen(file, ".")? {
        None => {
            println!("'{}' not found in history.", file);
            println!("Hint: run `atlas ingest .` first.");
        }
        Some(commit) => {
            let date = DateTime::<Utc>::from_timestamp(commit.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();

            println!("{} was first introduced in:\n", file);
            println!("  {}  {}  {}  ({})",
                commit.short_hash, date, commit.message, commit.author_name);
        }
    }
    Ok(())
}
