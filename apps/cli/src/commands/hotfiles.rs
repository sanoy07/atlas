use anyhow::Result;
use atlas_storage::Store;

pub fn run(limit: i64) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let files = store.hot_files(".", limit)?;

    if files.is_empty() {
        println!("No file history found. Run `atlas ingest .` first.");
        return Ok(());
    }

    println!("Most frequently modified files:\n");
    for row in &files {
        println!("  {:>4}×  {}", row.touch_count, row.file_path);
    }
    println!("\n{} file(s) shown.", files.len());
    Ok(())
}
