use anyhow::Result;
use atlas_core::build_focus;
use atlas_ir::{EpistemicLayer, FocusReport};
use atlas_storage::Store;

pub fn run(subject: &str, json: bool) -> Result<()> {
    let db_path = super::resolve_db_path();
    let store = Store::open(&db_path)?;
    let repo = super::discover_repo_root()?;
    let report = build_focus(subject, &repo, &store)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render(&report);
    }
    Ok(())
}

fn render(r: &FocusReport) {
    if let Some(rn) = &r.redirect_note {
        eprintln!(
            "note: `{}` is historical → focusing `{}` (identity {})",
            rn.original_subject, rn.current_path, rn.identity_id
        );
    }

    println!("ATLAS FOCUS");
    println!("  subject: {}  ({})", r.subject, r.subject_kind);
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
    }
    println!();

    if !r.outgoing.is_empty() {
        println!("OUTGOING  ({})", r.outgoing.len());
        for e in r.outgoing.iter().take(25) {
            println!("  {}", e);
        }
        if r.outgoing.len() > 25 {
            println!("  … {} more", r.outgoing.len() - 25);
        }
        println!();
    }

    if !r.incoming.is_empty() {
        println!("INCOMING  ({})", r.incoming.len());
        for e in r.incoming.iter().take(25) {
            println!("  {}", e);
        }
        if r.incoming.len() > 25 {
            println!("  … {} more", r.incoming.len() - 25);
        }
        println!();
    }

    if !r.related_tests.is_empty() {
        println!("TESTS");
        for t in &r.related_tests {
            println!("  · {}", t);
        }
        println!();
    }

    if !r.packages_observed.is_empty() {
        println!("PACKAGES (structurally observed from this subject)");
        for p in &r.packages_observed {
            println!("  · {}", p);
        }
        println!();
    }

    if !r.recent_commits.is_empty() {
        println!("RECENT COMMITS");
        for c in &r.recent_commits {
            println!("  · {}", c);
        }
        println!();
    }

    if !r.authors.is_empty() {
        println!("AUTHORS  (observed tuples — not ownership)");
        for a in &r.authors {
            println!("  · {}", a);
        }
        println!();
    }

    if !r.related_docs.is_empty() {
        println!("DOCUMENTS");
        for d in &r.related_docs {
            println!("  · {}", d);
        }
        println!();
    }

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
