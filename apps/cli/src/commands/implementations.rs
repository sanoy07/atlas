use anyhow::Result;
use atlas_core::find_implementations;
use atlas_storage::Store;

pub fn run(subject: &str, limit: usize, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = find_implementations(subject, &repo, &store, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("ATLAS IMPLEMENTATIONS");
        println!("  subject: {}", report.subject);
        println!();
        println!("INTERFACE FILES");
        if report.interface_files.is_empty() {
            println!("  (none resolved)");
        }
        for f in &report.interface_files {
            println!("  · {}", f);
        }
        println!();
        let any_observed = report
            .implementations
            .iter()
            .any(|h| h.reason.starts_with("OBSERVED"));
        println!(
            "IMPLEMENTATION CANDIDATES  ({})",
            if any_observed {
                "OBSERVED implements preferred"
            } else {
                "DERIVED heuristics"
            }
        );
        if report.implementations.is_empty() {
            println!("  (none)");
        }
        for h in &report.implementations {
            let tag = if h.is_test { "test" } else { "prod" };
            println!("  [{}] {}  — {}", tag, h.file, h.reason);
        }
        println!();
        println!("IMPORTERS  (all, may include non-impls)");
        for p in report.importers.iter().take(25) {
            println!("  · {}", p);
        }
        if report.importers.len() > 25 {
            println!("  … {} more", report.importers.len() - 25);
        }
        println!();
        println!("LIMITATIONS");
        for l in &report.limitations {
            println!("  · {}", l);
        }
    }
    Ok(())
}
