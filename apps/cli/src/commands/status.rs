use anyhow::Result;
use atlas_storage::Store;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("Atlas Core\n");

    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    match Store::open(&db_path) {
        Ok(_)  => println!("SQLite: Connected ({})", db_path),
        Err(e) => println!("SQLite: Error — {}", e),
    }

    let git_ok = Command::new("git").arg("--version").output().is_ok();
    println!("Git:    {}", if git_ok { "Ready" } else { "Not found" });

    let gh_ok = Command::new("gh").arg("--version").output().is_ok();
    println!("GitHub: {}", if gh_ok { "Ready" } else { "Not found — install gh" });

    println!("Parser: Ready");

    Ok(())
}
