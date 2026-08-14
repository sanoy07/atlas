use anyhow::Result;
use atlas_core::compute_capabilities;
use atlas_storage::Store;

pub fn run(json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = compute_capabilities(&repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("ATLAS CAPABILITIES");
        println!("  repo: {}", report.repo_path);
        println!();
        if report.capabilities.is_empty() {
            println!("  (no infrastructure import fan-in observed)");
            println!("  Hint: atlas ingest . --typescript");
        }
        for c in &report.capabilities {
            println!("CAPABILITY  {}", c.name);
            println!("  layer: {}", c.layer);
            println!("  infrastructure ({})", c.infrastructure.len());
            for f in c.infrastructure.iter().take(12) {
                println!("    · {}", f);
            }
            if c.infrastructure.len() > 12 {
                println!("    … {} more", c.infrastructure.len() - 12);
            }
            println!("  product_surfaces ({})", c.product_surfaces.len());
            for f in c.product_surfaces.iter().take(20) {
                println!("    · {}", f);
            }
            if c.product_surfaces.len() > 20 {
                println!("    … {} more", c.product_surfaces.len() - 20);
            }
            if !c.evidence_notes.is_empty() {
                println!("  notes:");
                for n in &c.evidence_notes {
                    println!("    · {}", n);
                }
            }
            println!();
        }
        println!("LIMITATIONS");
        for l in &report.limitations {
            println!("  · {}", l);
        }
    }
    Ok(())
}
