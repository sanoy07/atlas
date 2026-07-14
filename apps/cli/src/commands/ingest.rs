use anyhow::Result;
use atlas_core;
use atlas_storage::Store;

pub fn run(path: &str, github: bool, typescript: bool) -> Result<()> {
    let db_path = std::env::var("ATLAS_DB").unwrap_or_else(|_| "./atlas.db".to_string());
    let store = Store::open(&db_path)?;

    let path = &super::canonical_repo_path(path);

    print!("Ingesting git history from {} … ", path);
    let count = atlas_core::ingest_git(path, &store)?;
    println!("{} commits  (build artifacts excluded)", count);

    print!("Ingesting rename evidence … ");
    let rename_count = atlas_core::ingest_rename_evidence(path, &store)?;
    println!("{} rename records", rename_count);

    print!("Building file identities … ");
    let id_count = atlas_core::rebuild_file_identities(path, &store)?;
    println!("{} identities", id_count);

    if github {
        print!("Fetching GitHub PRs … ");
        let pr_count = atlas_core::ingest_github(path, &store)?;
        println!("{} PRs", pr_count);
    }

    if typescript {
        print!("Extracting TypeScript structural edges … ");
        let edge_count = atlas_core::ingest_typescript(path, &store)?;
        println!("{} edges", edge_count);
    }

    print!("Ingesting decision records and ADRs … ");
    let doc_count = atlas_core::ingest_documents(path, &store)?;
    println!("{} documents", doc_count);

    println!("Done.");
    Ok(())
}
