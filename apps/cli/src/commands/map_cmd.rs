use anyhow::Result;
use atlas_core::build_map;
use atlas_ir::{EpistemicLayer, MapReport};
use atlas_storage::Store;

pub fn run(json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = build_map(&repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &MapReport) {
    println!("ATLAS MAP");
    println!("  repo: {}", r.repo_path);
    if let Some(h) = &r.git_head {
        println!("  git_head: {}", h);
    }
    println!("  modules_subject: {}", r.modules_subject);
    println!(
        "  modules ({}): {}",
        r.modules.len(),
        if r.modules.is_empty() {
            "(none)".into()
        } else {
            r.modules.join(", ")
        }
    );
    println!();

    println!("CLAIMS  (observed | derived | inferred | unknown)");
    for c in &r.claims {
        println!(
            "  [{}] {} — {}",
            layer_label(&c.layer),
            c.id,
            c.statement
        );
        println!("    method: {}", c.method);
        for e in c.evidence.iter().take(3) {
            println!("    evidence: [{}] {} — {}", e.kind, e.id, e.summary);
        }
        for l in &c.limitations {
            println!("    limitation: {}", l);
        }
        println!();
    }

    if !r.hot_files.is_empty() {
        println!("HOT FILES");
        for (p, n) in &r.hot_files {
            println!("  {:>4}  {}", n, p);
        }
        println!();
    }

    if !r.top_coupling.is_empty() {
        println!("TOP COUPLING CELLS");
        for (a, b, n) in &r.top_coupling {
            println!("  {:>4}  {} → {}", n, a, b);
        }
        println!();
    }

    if !r.config_artifacts.is_empty() {
        println!("CONFIG ARTIFACTS");
        for p in &r.config_artifacts {
            println!("  · {}", p);
        }
        println!();
    }

    println!("COVERAGE");
    for n in &r.coverage_notes {
        println!("  · {}", n);
    }
    println!();
    println!("LIMITATIONS");
    for l in &r.limitations {
        println!("  · {}", l);
    }
}

fn layer_label(l: &EpistemicLayer) -> &'static str {
    match l {
        EpistemicLayer::Observed => "OBSERVED",
        EpistemicLayer::Derived => "DERIVED",
        EpistemicLayer::Inferred => "INFERRED",
        EpistemicLayer::Unknown => "UNKNOWN",
    }
}
