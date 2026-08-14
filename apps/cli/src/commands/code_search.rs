use anyhow::Result;
use atlas_core::definition_ranked_search;
use atlas_storage::Store;

pub fn run(query: &str, limit: usize, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = definition_ranked_search(query, &repo, &store, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("ATLAS CODE-SEARCH  (definition-ranked structural)");
        println!("  query: {}", report.query);
        println!();
        println!(
            "  {:<12}  {:<10}  {}",
            "bucket", "kind", "path / symbol"
        );
        for h in &report.hits {
            let sym = h.symbol.as_deref().unwrap_or("-");
            let line = h
                .evidence_line
                .map(|l| format!(":{}", l))
                .unwrap_or_default();
            println!(
                "  {:<12}  {:<10}  {}{}  ({})",
                h.rank_bucket, h.kind, h.path, line, sym
            );
        }
        if report.hits.is_empty() {
            println!("  (no structural hits — try ripgrep via atlas agent, or re-ingest)");
        }
        println!();
        println!("LIMITATIONS");
        for l in &report.limitations {
            println!("  · {}", l);
        }
    }
    Ok(())
}
