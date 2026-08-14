use anyhow::Result;
use atlas_core::build_impact;
use atlas_ir::{EpistemicLayer, ImpactReport};
use atlas_storage::Store;

pub fn run(subject: &str, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = build_impact(subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &ImpactReport) {
    println!("ATLAS IMPACT");
    println!("  subject: {}", r.subject);
    println!();

    println!("CLAIMS");
    for c in &r.claims {
        println!(
            "  [{}] {} — {}",
            layer_label(&c.layer),
            c.id,
            c.statement
        );
        println!("    method: {}", c.method);
        for l in &c.limitations {
            println!("    limitation: {}", l);
        }
    }
    println!();

    println!("NEIGHBORS  (ranked investigation guidance — not change-safety)");
    println!(
        "  {:>6}  {:<10}  {}",
        "score", "layer", "path / reasons"
    );
    for n in &r.neighbors {
        println!(
            "  {:>6.2}  {:<10}  {}",
            n.rank_score,
            layer_label(&n.layer),
            n.path
        );
        println!(
            "          dims: rel={:.2} struct={:.2} cochange={:.2} corr={:.2}  ({})",
            n.dimensions.subject_relevance,
            n.dimensions.structural_connectivity,
            n.dimensions.historical_cochange,
            n.dimensions.corroboration,
            n.dimensions.provenance_note
        );
        if !n.reasons.is_empty() {
            println!("          reasons: {}", n.reasons.join("; "));
        }
    }
    println!();

    println!("DIMENSIONS METHODOLOGY");
    for m in &r.dimensions_methodology {
        println!("  · {}", m);
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
